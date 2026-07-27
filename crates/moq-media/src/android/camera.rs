//! Android camera capture via the NDK Camera2 API.
//!
//! Implements the [`VideoSource`] trait by capturing frames from
//! `libcamera2ndk` / `libmediandk`, mirroring what `videotoolbox::camera`
//! does for iOS. It exists because `nokhwa` — the capture backend used on
//! desktop — has no Android implementation at all (it gates on `macos`,
//! `windows`, `linux` and `ios` only), so before this module Android had no
//! camera path: enumeration returned an empty list and every tutoring session
//! silently fell back to audio-only.
//!
//! Pipeline:
//!
//! 1. `ACameraManager_getCameraIdList` enumerates devices; `ACAMERA_LENS_FACING`
//!    from each device's characteristics labels them front/back.
//! 2. An `AImageReader` is created for `YUV_420_888` — the one format Camera2
//!    guarantees for every device — and its `ANativeWindow` becomes the capture
//!    target.
//! 3. The device is opened, a `TEMPLATE_PREVIEW` request is pointed at that
//!    window, and a repeating request keeps frames flowing.
//! 4. Each delivered image is converted to RGBA (the trait's frame format) and
//!    parked in a one-slot mailbox that `pop_frame` drains.
//!
//! Threading: the NDK delivers images on its own callback thread, so the
//! mailbox is a `Mutex<Option<VideoFrame>>` holding only the newest frame —
//! matching the iOS source's behaviour of dropping stale frames rather than
//! queueing latency when the encoder falls behind.

// Every function below is a thin shim over `libcamera2ndk` / `libmediandk`, so
// essentially each line is an FFI call. Marking each one individually with
// `unsafe {}` inside an already-`unsafe fn` would bury the handful of places
// where the safety obligation actually differs (pointer validity, plane bounds)
// under uniform noise. The obligations are documented per function instead.
#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow, bail};
use ndk_sys as sys;
use nokhwa::utils::CameraIndex;

use crate::av::{PixelFormat, VideoFormat, VideoFrame, VideoSource};

/// Requested capture size. Camera2 must support this on essentially every
/// device; the negotiated size is read back from the reader rather than
/// assumed, so a device that substitutes another resolution still works.
const REQUEST_WIDTH: i32 = 1280;
const REQUEST_HEIGHT: i32 = 720;

/// `AIMAGE_FORMAT_YUV_420_888`. Not re-exported by ndk-sys as a constant we can
/// name directly, and it is stable ABI, so it is spelled out here.
const AIMAGE_FORMAT_YUV_420_888: i32 = 0x23;

/// Depth of the reader's buffer queue. Two lets the producer fill one while we
/// hold the other; more only adds latency given we keep just the newest frame.
const MAX_IMAGES: i32 = 2;

/// Newest captured frame, shared with the NDK callback thread.
type FrameSlot = Arc<Mutex<Option<VideoFrame>>>;

/// A camera opened through the NDK Camera2 API.
pub struct AndroidCameraSource {
    name: String,
    camera_id: CString,
    format: VideoFormat,
    slot: FrameSlot,
    inner: Option<Session>,
}

/// The native objects that only exist while capture is running. Kept in one
/// struct so `stop` (and `Drop`) tear them down in the correct order.
struct Session {
    manager: *mut sys::ACameraManager,
    device: *mut sys::ACameraDevice,
    session: *mut sys::ACameraCaptureSession,
    request: *mut sys::ACaptureRequest,
    output_target: *mut sys::ACameraOutputTarget,
    session_output: *mut sys::ACaptureSessionOutput,
    output_container: *mut sys::ACaptureSessionOutputContainer,
    reader: *mut sys::AImageReader,
    /// Boxed so the pointer handed to the NDK listener stays valid for the
    /// lifetime of the session.
    listener: Box<sys::AImageReader_ImageListener>,
    /// Keeps the callback's context allocation alive alongside the listener.
    _ctx: Box<FrameSlot>,
}

// The raw NDK handles are only touched from methods on `&mut self`, and the
// frame mailbox is behind a mutex, so the source moves between threads safely.
unsafe impl Send for AndroidCameraSource {}

impl AndroidCameraSource {
    /// Enumerate cameras as `(index, label)`, newest-API-first ordering as the
    /// platform reports them.
    ///
    /// The index is a [`CameraIndex::String`] holding the Camera2 id (`"0"`,
    /// `"1"`, …) rather than a positional integer, because Camera2 ids are
    /// opaque strings and are what `openCamera` expects back.
    pub fn list_cameras() -> Result<Vec<(CameraIndex, String)>> {
        unsafe {
            let manager = sys::ACameraManager_create();
            if manager.is_null() {
                bail!("ACameraManager_create returned null");
            }
            // Everything below borrows `manager`; delete it on every path.
            let result = Self::list_with_manager(manager);
            sys::ACameraManager_delete(manager);
            result
        }
    }

    unsafe fn list_with_manager(
        manager: *mut sys::ACameraManager,
    ) -> Result<Vec<(CameraIndex, String)>> {
        let mut id_list: *mut sys::ACameraIdList = ptr::null_mut();
        let status = sys::ACameraManager_getCameraIdList(manager, &mut id_list);
        if status != sys::camera_status_t::ACAMERA_OK || id_list.is_null() {
            bail!("ACameraManager_getCameraIdList failed: {status:?}");
        }

        let mut out = Vec::new();
        let count = (*id_list).numCameras.max(0) as usize;
        for i in 0..count {
            let raw_id = *(*id_list).cameraIds.add(i);
            if raw_id.is_null() {
                continue;
            }
            let id = CStr::from_ptr(raw_id).to_string_lossy().into_owned();
            let label = Self::describe(manager, raw_id, &id);
            out.push((CameraIndex::String(id), label));
        }
        sys::ACameraManager_deleteCameraIdList(id_list);
        Ok(out)
    }

    /// Human-readable label for a camera id, from its lens facing. Falls back
    /// to the bare id when characteristics are unavailable — a missing label
    /// should never make a usable camera disappear from the list.
    unsafe fn describe(
        manager: *mut sys::ACameraManager,
        raw_id: *const std::os::raw::c_char,
        id: &str,
    ) -> String {
        let mut chars: *mut sys::ACameraMetadata = ptr::null_mut();
        if sys::ACameraManager_getCameraCharacteristics(manager, raw_id, &mut chars)
            != sys::camera_status_t::ACAMERA_OK
            || chars.is_null()
        {
            return format!("Camera {id}");
        }
        let mut entry = std::mem::zeroed::<sys::ACameraMetadata_const_entry>();
        let facing = if sys::ACameraMetadata_getConstEntry(
            chars,
            sys::acamera_metadata_tag::ACAMERA_LENS_FACING.0,
            &mut entry,
        ) == sys::camera_status_t::ACAMERA_OK
            && entry.count > 0
            && !entry.data.u8_.is_null()
        {
            Some(*entry.data.u8_)
        } else {
            None
        };
        sys::ACameraMetadata_free(chars);

        match facing {
            Some(f)
                if f as u32
                    == sys::acamera_metadata_enum_acamera_lens_facing::ACAMERA_LENS_FACING_FRONT
                        .0 =>
            {
                format!("Front camera ({id})")
            }
            Some(f)
                if f as u32
                    == sys::acamera_metadata_enum_acamera_lens_facing::ACAMERA_LENS_FACING_BACK
                        .0 =>
            {
                format!("Back camera ({id})")
            }
            _ => format!("Camera {id}"),
        }
    }

    /// Open the camera at `index`, or the first available one when `None`.
    pub fn with_index(index: Option<CameraIndex>) -> Result<Self> {
        let cameras = Self::list_cameras()?;
        if cameras.is_empty() {
            bail!("no cameras available");
        }
        let (chosen, name) = match index {
            Some(CameraIndex::String(id)) => cameras
                .iter()
                .find(|(i, _)| matches!(i, CameraIndex::String(s) if *s == id))
                .cloned()
                .ok_or_else(|| anyhow!("camera id {id} not found"))?,
            // A positional index is accepted so callers that stored one from a
            // desktop session still resolve to something sensible.
            Some(CameraIndex::Index(n)) => cameras
                .get(n as usize)
                .cloned()
                .ok_or_else(|| anyhow!("camera index {n} out of range"))?,
            None => cameras[0].clone(),
        };
        let id = match &chosen {
            CameraIndex::String(s) => s.clone(),
            CameraIndex::Index(n) => n.to_string(),
        };

        Ok(Self {
            name,
            camera_id: CString::new(id)?,
            format: VideoFormat {
                pixel_format: PixelFormat::Rgba,
                dimensions: [REQUEST_WIDTH as u32, REQUEST_HEIGHT as u32],
            },
            slot: Arc::new(Mutex::new(None)),
            inner: None,
        })
    }

    pub fn new() -> Result<Self> {
        Self::with_index(None)
    }
}

/// Called by the NDK when a new image is ready. Converts to RGBA and replaces
/// whatever is in the mailbox — the newest frame always wins.
unsafe extern "C" fn on_image_available(
    ctx: *mut std::os::raw::c_void,
    reader: *mut sys::AImageReader,
) {
    if ctx.is_null() || reader.is_null() {
        return;
    }
    let slot = &*(ctx as *const FrameSlot);

    let mut image: *mut sys::AImage = ptr::null_mut();
    if sys::AImageReader_acquireLatestImage(reader, &mut image) != sys::media_status_t::AMEDIA_OK
        || image.is_null()
    {
        return;
    }
    let frame = yuv420_to_rgba(image);
    sys::AImage_delete(image);

    if let (Some(frame), Ok(mut guard)) = (frame, slot.lock()) {
        *guard = Some(frame);
    }
}

/// Convert a `YUV_420_888` image to a packed RGBA buffer.
///
/// Camera2 does not promise a memory layout: the chroma planes may be
/// interleaved (NV12/NV21, pixel stride 2) or fully planar (I420, pixel stride
/// 1), and every plane carries its own row stride with padding. Both strides
/// are therefore read per plane rather than assumed, which is what makes this
/// work across devices instead of only the one it was written on.
unsafe fn yuv420_to_rgba(image: *mut sys::AImage) -> Option<VideoFrame> {
    let (mut width, mut height) = (0i32, 0i32);
    if sys::AImage_getWidth(image, &mut width) != sys::media_status_t::AMEDIA_OK
        || sys::AImage_getHeight(image, &mut height) != sys::media_status_t::AMEDIA_OK
        || width <= 0
        || height <= 0
    {
        return None;
    }

    let plane = |idx: i32| -> Option<(*mut u8, i32, i32, i32)> {
        let mut data: *mut u8 = ptr::null_mut();
        let mut len: std::os::raw::c_int = 0;
        let mut row_stride = 0i32;
        let mut pixel_stride = 0i32;
        if sys::AImage_getPlaneData(image, idx, &mut data, &mut len)
            != sys::media_status_t::AMEDIA_OK
            || data.is_null()
            || sys::AImage_getPlaneRowStride(image, idx, &mut row_stride)
                != sys::media_status_t::AMEDIA_OK
            || sys::AImage_getPlanePixelStride(image, idx, &mut pixel_stride)
                != sys::media_status_t::AMEDIA_OK
        {
            return None;
        }
        Some((data, len, row_stride, pixel_stride))
    };

    let (y_ptr, y_len, y_row, _) = plane(0)?;
    let (u_ptr, u_len, u_row, u_pix) = plane(1)?;
    let (v_ptr, v_len, v_row, v_pix) = plane(2)?;

    let (w, h) = (width as usize, height as usize);
    let mut rgba = vec![0u8; w * h * 4];

    for row in 0..h {
        let uv_row = row / 2;
        for col in 0..w {
            let y_idx = row * y_row as usize + col;
            if y_idx >= y_len as usize {
                continue;
            }
            let uv_idx_u = uv_row * u_row as usize + (col / 2) * u_pix as usize;
            let uv_idx_v = uv_row * v_row as usize + (col / 2) * v_pix as usize;
            if uv_idx_u >= u_len as usize || uv_idx_v >= v_len as usize {
                continue;
            }

            let y = *y_ptr.add(y_idx) as f32;
            let u = *u_ptr.add(uv_idx_u) as f32 - 128.0;
            let v = *v_ptr.add(uv_idx_v) as f32 - 128.0;

            // BT.601 full-range, matching what the desktop capture path feeds
            // the encoder.
            let r = y + 1.402 * v;
            let g = y - 0.344_136 * u - 0.714_136 * v;
            let b = y + 1.772 * u;

            let o = (row * w + col) * 4;
            rgba[o] = r.clamp(0.0, 255.0) as u8;
            rgba[o + 1] = g.clamp(0.0, 255.0) as u8;
            rgba[o + 2] = b.clamp(0.0, 255.0) as u8;
            rgba[o + 3] = 255;
        }
    }

    Some(VideoFrame {
        format: VideoFormat {
            pixel_format: PixelFormat::Rgba,
            dimensions: [width as u32, height as u32],
        },
        raw: bytes::Bytes::from(rgba),
    })
}

impl VideoSource for AndroidCameraSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn format(&self) -> VideoFormat {
        self.format.clone()
    }

    fn pop_frame(&mut self) -> Result<Option<VideoFrame>> {
        Ok(self
            .slot
            .lock()
            .map_err(|_| anyhow!("camera frame mailbox poisoned"))?
            .take())
    }

    fn start(&mut self) -> Result<()> {
        if self.inner.is_some() {
            return Ok(());
        }
        let session = unsafe { Session::open(&self.camera_id, Arc::clone(&self.slot))? };
        // Report the size the reader actually negotiated, not what we asked
        // for, so downstream encoders size themselves correctly.
        self.format.dimensions = session.dimensions;
        self.inner = Some(session.into_inner());
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.inner.take();
        if let Ok(mut guard) = self.slot.lock() {
            *guard = None;
        }
        Ok(())
    }
}

impl Drop for AndroidCameraSource {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// A started session plus the size its reader negotiated.
struct OpenedSession {
    session: Session,
    dimensions: [u32; 2],
}

impl OpenedSession {
    fn into_inner(self) -> Session {
        self.session
    }
}

impl std::ops::Deref for OpenedSession {
    type Target = Session;
    fn deref(&self) -> &Session {
        &self.session
    }
}

impl Session {
    /// Bring up reader → device → session → repeating request.
    unsafe fn open(camera_id: &CStr, slot: FrameSlot) -> Result<OpenedSession> {
        let manager = sys::ACameraManager_create();
        if manager.is_null() {
            bail!("ACameraManager_create returned null");
        }

        // Reader first: its window is the target every later object needs.
        let mut reader: *mut sys::AImageReader = ptr::null_mut();
        if sys::AImageReader_new(
            REQUEST_WIDTH,
            REQUEST_HEIGHT,
            AIMAGE_FORMAT_YUV_420_888,
            MAX_IMAGES,
            &mut reader,
        ) != sys::media_status_t::AMEDIA_OK
            || reader.is_null()
        {
            sys::ACameraManager_delete(manager);
            bail!("AImageReader_new failed");
        }

        let mut width = REQUEST_WIDTH;
        let mut height = REQUEST_HEIGHT;
        let _ = sys::AImageReader_getWidth(reader, &mut width);
        let _ = sys::AImageReader_getHeight(reader, &mut height);

        let ctx = Box::new(slot);
        let mut listener = Box::new(sys::AImageReader_ImageListener {
            context: (&*ctx as *const FrameSlot) as *mut std::os::raw::c_void,
            onImageAvailable: Some(on_image_available),
        });
        if sys::AImageReader_setImageListener(reader, &mut *listener)
            != sys::media_status_t::AMEDIA_OK
        {
            sys::AImageReader_delete(reader);
            sys::ACameraManager_delete(manager);
            bail!("AImageReader_setImageListener failed");
        }

        let mut window: *mut sys::ANativeWindow = ptr::null_mut();
        if sys::AImageReader_getWindow(reader, &mut window) != sys::media_status_t::AMEDIA_OK
            || window.is_null()
        {
            sys::AImageReader_delete(reader);
            sys::ACameraManager_delete(manager);
            bail!("AImageReader_getWindow failed");
        }

        // Opening requires the CAMERA runtime permission; without it the NDK
        // reports ACAMERA_ERROR_PERMISSION_DENIED rather than prompting.
        let mut device_cbs = sys::ACameraDevice_StateCallbacks {
            context: ptr::null_mut(),
            onDisconnected: Some(on_device_disconnected),
            onError: Some(on_device_error),
        };
        let mut device: *mut sys::ACameraDevice = ptr::null_mut();
        let status = sys::ACameraManager_openCamera(
            manager,
            camera_id.as_ptr(),
            &mut device_cbs,
            &mut device,
        );
        if status != sys::camera_status_t::ACAMERA_OK || device.is_null() {
            sys::AImageReader_delete(reader);
            sys::ACameraManager_delete(manager);
            bail!(
                "ACameraManager_openCamera failed: {status:?} (is the CAMERA permission granted?)"
            );
        }

        let mut request: *mut sys::ACaptureRequest = ptr::null_mut();
        if sys::ACameraDevice_createCaptureRequest(
            device,
            sys::ACameraDevice_request_template::TEMPLATE_PREVIEW,
            &mut request,
        ) != sys::camera_status_t::ACAMERA_OK
            || request.is_null()
        {
            sys::ACameraDevice_close(device);
            sys::AImageReader_delete(reader);
            sys::ACameraManager_delete(manager);
            bail!("ACameraDevice_createCaptureRequest failed");
        }

        let mut output_target: *mut sys::ACameraOutputTarget = ptr::null_mut();
        sys::ACameraOutputTarget_create(window, &mut output_target);
        sys::ACaptureRequest_addTarget(request, output_target);

        let mut session_output: *mut sys::ACaptureSessionOutput = ptr::null_mut();
        sys::ACaptureSessionOutput_create(window, &mut session_output);
        let mut output_container: *mut sys::ACaptureSessionOutputContainer = ptr::null_mut();
        sys::ACaptureSessionOutputContainer_create(&mut output_container);
        sys::ACaptureSessionOutputContainer_add(output_container, session_output);

        let session_cbs = sys::ACameraCaptureSession_stateCallbacks {
            context: ptr::null_mut(),
            onClosed: Some(on_session_closed),
            onReady: Some(on_session_ready),
            onActive: Some(on_session_active),
        };
        let mut session: *mut sys::ACameraCaptureSession = ptr::null_mut();
        if sys::ACameraDevice_createCaptureSession(
            device,
            output_container,
            &session_cbs,
            &mut session,
        ) != sys::camera_status_t::ACAMERA_OK
            || session.is_null()
        {
            sys::ACaptureSessionOutputContainer_free(output_container);
            sys::ACaptureSessionOutput_free(session_output);
            sys::ACameraOutputTarget_free(output_target);
            sys::ACaptureRequest_free(request);
            sys::ACameraDevice_close(device);
            sys::AImageReader_delete(reader);
            sys::ACameraManager_delete(manager);
            bail!("ACameraDevice_createCaptureSession failed");
        }

        let mut req_ptr = request;
        // Null capture callbacks: per-frame capture results are not needed —
        // frames arrive through the AImageReader listener instead.
        if sys::ACameraCaptureSession_setRepeatingRequest(
            session,
            ptr::null_mut(),
            1,
            &mut req_ptr,
            ptr::null_mut(),
        ) != sys::camera_status_t::ACAMERA_OK
        {
            sys::ACameraCaptureSession_close(session);
            sys::ACaptureSessionOutputContainer_free(output_container);
            sys::ACaptureSessionOutput_free(session_output);
            sys::ACameraOutputTarget_free(output_target);
            sys::ACaptureRequest_free(request);
            sys::ACameraDevice_close(device);
            sys::AImageReader_delete(reader);
            sys::ACameraManager_delete(manager);
            bail!("ACameraCaptureSession_setRepeatingRequest failed");
        }

        Ok(OpenedSession {
            session: Session {
                manager,
                device,
                session,
                request,
                output_target,
                session_output,
                output_container,
                reader,
                listener,
                _ctx: ctx,
            },
            dimensions: [width as u32, height as u32],
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Reverse construction order: stop the flow, then release targets, then
        // the device, and only then the reader whose window they referenced.
        unsafe {
            sys::ACameraCaptureSession_stopRepeating(self.session);
            sys::ACameraCaptureSession_close(self.session);
            sys::ACaptureSessionOutputContainer_free(self.output_container);
            sys::ACaptureSessionOutput_free(self.session_output);
            sys::ACameraOutputTarget_free(self.output_target);
            sys::ACaptureRequest_free(self.request);
            sys::ACameraDevice_close(self.device);
            sys::AImageReader_setImageListener(self.reader, ptr::null_mut());
            sys::AImageReader_delete(self.reader);
            sys::ACameraManager_delete(self.manager);
        }
        let _ = &self.listener;
    }
}

unsafe extern "C" fn on_device_disconnected(
    _ctx: *mut std::os::raw::c_void,
    _d: *mut sys::ACameraDevice,
) {
}
unsafe extern "C" fn on_device_error(
    _ctx: *mut std::os::raw::c_void,
    _d: *mut sys::ACameraDevice,
    _error: std::os::raw::c_int,
) {
}
unsafe extern "C" fn on_session_closed(
    _ctx: *mut std::os::raw::c_void,
    _s: *mut sys::ACameraCaptureSession,
) {
}
unsafe extern "C" fn on_session_ready(
    _ctx: *mut std::os::raw::c_void,
    _s: *mut sys::ACameraCaptureSession,
) {
}
unsafe extern "C" fn on_session_active(
    _ctx: *mut std::os::raw::c_void,
    _s: *mut sys::ACameraCaptureSession,
) {
}
