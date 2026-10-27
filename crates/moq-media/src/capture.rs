// Copyright 2025 N0, INC
// Modified by Pratyush Pundir <pratyush@alexandria.ifftu.dev> — see crates/VENDORING.md
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// The nokhwa-backed camera path is compiled out on Android (see the
// `CameraCapturer` alias below), so everything only it uses is gated the same
// way. `CameraIndex` stays unconditional: it is the public index type both
// backends hand back.
#[cfg(not(target_os = "android"))]
use std::str::FromStr;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::Duration,
};

#[cfg(not(target_os = "android"))]
use anyhow::Context;
use anyhow::Result;
pub use nokhwa::utils::CameraIndex;
#[cfg(not(target_os = "android"))]
use nokhwa::{
    nokhwa_initialize,
    pixel_format::RgbFormat,
    utils::{CameraFormat, FrameFormat, RequestedFormat, RequestedFormatType, Resolution},
};
#[cfg(not(target_os = "android"))]
use tracing::{debug, info, trace, warn};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use xcap::Monitor;

use crate::av::{PixelFormat, VideoFormat, VideoFrame, VideoSource};
#[cfg(not(target_os = "android"))]
use crate::ffmpeg::util::MjpgDecoder;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct ScreenCapturer {
    pub(crate) width: u32,
    pub(crate) height: u32,
    command_tx: SyncSender<ScreenCaptureCommand>,
    frame_rx: Receiver<xcap::Frame>,
    worker: Option<JoinHandle<()>>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
enum ScreenCaptureCommand {
    Start(SyncSender<Result<(), String>>),
    Stop(SyncSender<Result<(), String>>),
    Shutdown,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
type ScreenCaptureInit = Result<(u32, u32, Receiver<xcap::Frame>), String>;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct ScreenCapturer;

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Drop for ScreenCapturer {
    fn drop(&mut self) {
        let _ = self.command_tx.send(ScreenCaptureCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ScreenCapturer {
    pub fn new() -> Result<Self> {
        info!("Initializing screen capturer (xcap)");

        let (command_tx, command_rx) = mpsc::sync_channel(1);
        let (init_tx, init_rx) = mpsc::sync_channel::<ScreenCaptureInit>(1);
        let worker = std::thread::Builder::new()
            .name("screen-capture-native".into())
            .spawn(move || {
                let initialized = (|| -> Result<_, String> {
                    let monitors = Monitor::all()
                        .context("failed to enumerate monitors")
                        .map_err(|e| e.to_string())?;
                    info!("Available monitors: {monitors:?}");
                    let monitor = monitors
                        .into_iter()
                        .next()
                        .ok_or_else(|| "no monitors available".to_string())?;
                    let width = monitor.width().map_err(|e| e.to_string())?;
                    let height = monitor.height().map_err(|e| e.to_string())?;
                    let name = monitor
                        .name()
                        .unwrap_or_else(|_| "Unknown Monitor".to_string());
                    info!("Using monitor: {name} ({width}x{height})");
                    let (recorder, frame_rx) =
                        monitor.video_recorder().map_err(|e| e.to_string())?;
                    Ok((monitor, recorder, frame_rx, width, height))
                })();

                let (_monitor, recorder, native_frame_rx, width, height) = match initialized {
                    Ok(parts) => parts,
                    Err(error) => {
                        let _ = init_tx.send(Err(error));
                        return;
                    }
                };
                // xcap uses a zero-capacity callback channel. Keep draining it
                // on a dedicated thread while stop/shutdown waits in this
                // owner, otherwise a callback blocked in `send` can deadlock
                // the native recorder's synchronous stop operation. The
                // outward capacity of one bounds memory when encoding lags.
                let (frame_tx, frame_rx) = mpsc::sync_channel(1);
                let frame_shutdown = Arc::new(AtomicBool::new(false));
                let relay_shutdown = Arc::clone(&frame_shutdown);
                let frame_worker = match std::thread::Builder::new()
                    .name("screen-capture-frames".into())
                    .spawn(move || {
                        while !relay_shutdown.load(Ordering::Relaxed) {
                            match native_frame_rx.recv_timeout(Duration::from_millis(50)) {
                                Ok(frame) => match frame_tx.try_send(frame) {
                                    Ok(()) | Err(TrySendError::Full(_)) => {}
                                    Err(TrySendError::Disconnected(_)) => break,
                                },
                                Err(RecvTimeoutError::Timeout) => {}
                                Err(RecvTimeoutError::Disconnected) => break,
                            }
                        }
                    }) {
                    Ok(worker) => worker,
                    Err(error) => {
                        let _ = init_tx.send(Err(format!(
                            "failed to start screen-capture frame relay: {error}"
                        )));
                        return;
                    }
                };
                if init_tx.send(Ok((width, height, frame_rx))).is_err() {
                    frame_shutdown.store(true, Ordering::Relaxed);
                    drop(recorder);
                    let _ = frame_worker.join();
                    return;
                }

                while let Ok(command) = command_rx.recv() {
                    match command {
                        ScreenCaptureCommand::Start(reply) => {
                            let _ = reply.send(recorder.start().map_err(|e| e.to_string()));
                        }
                        ScreenCaptureCommand::Stop(reply) => {
                            let _ = reply.send(recorder.stop().map_err(|e| e.to_string()));
                        }
                        ScreenCaptureCommand::Shutdown => break,
                    }
                }
                let _ = recorder.stop();
                frame_shutdown.store(true, Ordering::Relaxed);
                drop(recorder);
                let _ = frame_worker.join();
            })
            .context("failed to start native screen-capture owner")?;

        let (width, height, frame_rx) = match init_rx.recv() {
            Ok(Ok(initialized)) => initialized,
            Ok(Err(error)) => {
                let _ = worker.join();
                return Err(anyhow::anyhow!(error));
            }
            Err(error) => {
                let _ = worker.join();
                return Err(anyhow::anyhow!(
                    "native screen-capture owner stopped during initialization: {error}"
                ));
            }
        };

        Ok(Self {
            width,
            height,
            command_tx,
            frame_rx,
            worker: Some(worker),
        })
    }

    fn send_control(
        &self,
        command: impl FnOnce(SyncSender<Result<(), String>>) -> ScreenCaptureCommand,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.command_tx
            .send(command(reply_tx))
            .context("native screen-capture owner is unavailable")?;
        reply_rx
            .recv()
            .context("native screen-capture owner stopped before replying")?
            .map_err(anyhow::Error::msg)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl ScreenCapturer {
    pub fn new() -> Result<Self> {
        Err(anyhow::anyhow!(
            "screen capture is unavailable on this platform"
        ))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl VideoSource for ScreenCapturer {
    fn name(&self) -> &str {
        "screen"
    }

    fn format(&self) -> VideoFormat {
        VideoFormat {
            pixel_format: PixelFormat::Rgba,
            dimensions: [self.width, self.height],
        }
    }

    fn start(&mut self) -> Result<()> {
        while self.frame_rx.try_recv().is_ok() {}
        self.send_control(ScreenCaptureCommand::Start)
    }

    fn stop(&mut self) -> Result<()> {
        self.send_control(ScreenCaptureCommand::Stop)
    }

    fn pop_frame(&mut self) -> anyhow::Result<Option<VideoFrame>> {
        let mut raw_frame = None;
        // We are only interested in the latest frame.
        // Drain the channel to not build up memory.
        while let Ok(next) = self.frame_rx.try_recv() {
            raw_frame = Some(next)
        }
        let raw_frame = match raw_frame {
            Some(frame) => frame,
            None => match self.frame_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(frame) => frame,
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(anyhow::anyhow!("screen recorder frame stream disconnected"));
                }
            },
        };
        Ok(Some(VideoFrame {
            format: VideoFormat {
                pixel_format: PixelFormat::Rgba,
                dimensions: [raw_frame.width, raw_frame.height],
            },
            raw: raw_frame.raw.into(),
        }))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl VideoSource for ScreenCapturer {
    fn name(&self) -> &str {
        "screen"
    }

    fn format(&self) -> VideoFormat {
        VideoFormat {
            pixel_format: PixelFormat::Rgba,
            dimensions: [0, 0],
        }
    }

    fn start(&mut self) -> Result<()> {
        Err(anyhow::anyhow!(
            "screen capture is unavailable on this platform"
        ))
    }

    fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    fn pop_frame(&mut self) -> anyhow::Result<Option<VideoFrame>> {
        Err(anyhow::anyhow!(
            "screen capture is unavailable on this platform"
        ))
    }
}

// Camera capture is nokhwa-backed on desktop. nokhwa has no Android backend
// (it gates on macos/windows/linux/ios only), so Android routes `CameraCapturer`
// to the NDK Camera2 implementation instead — same constructors, same
// `VideoSource` impl, so callers are unchanged.
#[cfg(target_os = "android")]
pub use crate::android::AndroidCameraSource as CameraCapturer;

#[cfg(not(target_os = "android"))]
pub struct CameraCapturer {
    pub(crate) camera: nokhwa::Camera,
    pub(crate) mjpg_decoder: MjpgDecoder,
    pub(crate) width: u32,
    pub(crate) height: u32,
    name: String,
}

#[cfg(not(target_os = "android"))]
impl CameraCapturer {
    /// Create a camera capturer using the default camera (or `IROH_LIVE_CAMERA` env var).
    pub fn new() -> Result<Self> {
        Self::with_index(None)
    }

    /// Create a camera capturer targeting a specific camera by index.
    ///
    /// If `index` is `None`, falls back to the `IROH_LIVE_CAMERA` env var,
    /// then tries every camera nokhwa reports, real devices before virtual
    /// ones, until one opens.
    pub fn with_index(index: Option<CameraIndex>) -> Result<Self> {
        info!("Initializing camera capturer (nokhwa)");
        nokhwa_initialize(|granted| {
            debug!("User selected camera access: {}", granted);
        });

        let cameras = nokhwa::query(nokhwa::utils::ApiBackend::Auto)?;
        if cameras.is_empty() {
            return Err(anyhow::anyhow!("No cameras available"));
        }
        info!("Available cameras: {cameras:?}");

        let candidates = match index {
            Some(idx) => vec![idx],
            None => match std::env::var("IROH_LIVE_CAMERA").ok() {
                None => camera_preference(&cameras),
                Some(camera_name) => vec![match u32::from_str(&camera_name).ok() {
                    Some(num) => CameraIndex::Index(num),
                    None => CameraIndex::String(camera_name),
                }],
            },
        };
        let mut last_err = None;
        for camera_index in candidates {
            match Self::open(camera_index.clone()) {
                Ok(capturer) => return Ok(capturer),
                Err(err) => {
                    warn!(?camera_index, "camera would not open: {err:#}");
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("No cameras available")))
    }

    fn open(camera_index: CameraIndex) -> Result<Self> {
        // 720p: what a call tile needs. 1080p doubled every per-frame cost on
        // both ends (conversion, encode, the receiver's decode) for detail
        // no tile shows. Ask for it when opening: that request is matched
        // against the device's own format list, whereas nokhwa's
        // `compatible_camera_formats` comes back empty on macOS (its
        // AVFoundation backend filters formats with `!=` fourcc), which left
        // the camera at 1080p (16 Oct).
        let mut camera = match nokhwa::Camera::new(
            camera_index.clone(),
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::HighestResolution(
                CALL_RESOLUTION,
            )),
        ) {
            Ok(camera) => camera,
            Err(err) => {
                debug!("camera has no {CALL_RESOLUTION} format ({err}), opening its highest");
                nokhwa::Camera::new(
                    camera_index,
                    RequestedFormat::new::<RgbFormat>(
                        RequestedFormatType::AbsoluteHighestResolution,
                    ),
                )?
            }
        };
        let name = camera.info().human_name();
        info!("Using camera: {name}");
        if camera.resolution() != CALL_RESOLUTION {
            let available_formats = camera.compatible_camera_formats()?;
            debug!("Available formats: {available_formats:?}",);
            if let Some(format) = Self::select_format(available_formats, CALL_RESOLUTION)
                && let Err(err) = camera.set_camera_requset(RequestedFormat::new::<RgbFormat>(
                    RequestedFormatType::Exact(format),
                ))
            {
                warn!(?format, "Failed to change camera format: {err:#}");
            }
        }
        info!("Using format: {}", camera.camera_format());
        let resolution = camera.resolution();
        Ok(Self {
            camera,
            mjpg_decoder: MjpgDecoder::new()?,
            width: resolution.width(),
            height: resolution.height(),
            name,
        })
    }

    /// The camera's name as the system reports it, e.g. "MacBook Pro Camera".
    /// The same label a webview gives the device, so a preview can open it.
    pub fn device_name(&self) -> &str {
        &self.name
    }

    /// List available cameras. Returns a vec of (index, human_name) pairs.
    pub fn list_cameras() -> Result<Vec<(CameraIndex, String)>> {
        nokhwa_initialize(|granted| {
            debug!("User selected camera access: {}", granted);
        });
        let cameras = nokhwa::query(nokhwa::utils::ApiBackend::Auto)?;
        Ok(cameras
            .into_iter()
            .map(|c| (c.index().clone(), c.human_name().to_string()))
            .collect())
    }

    fn select_format(
        mut formats: Vec<CameraFormat>,
        desired_resolution: Resolution,
    ) -> Option<CameraFormat> {
        formats.sort_by(|a, b| {
            a.resolution()
                .cmp(&b.resolution())
                .then(a.frame_rate().cmp(&b.frame_rate()))
        });
        formats
            .iter()
            .find(|format| format.resolution() >= desired_resolution)
            .or_else(|| formats.last())
            .cloned()
    }
}

/// The capture size a call asks a camera for.
#[cfg(not(target_os = "android"))]
const CALL_RESOLUTION: Resolution = Resolution {
    width_x: 1280,
    height_y: 720,
};

/// Names of virtual cameras, as products label them (lowercase). Matches the
/// app's own list in `src/utils/sentinel/virtual-camera.ts`.
#[cfg(not(target_os = "android"))]
const VIRTUAL_CAMERA_MARKERS: &[&str] = &[
    "obs virtual",
    "manycam",
    "snap camera",
    "xsplit",
    "camo",
    "v4l2loopback",
    "virtual cam",
];

/// The cameras to try, best first, when nobody picked one: the computer's own
/// camera, then other real cameras, then a nearby iPhone or iPad offered
/// through Continuity Camera, then virtual cameras.
///
/// nokhwa lists the primary camera last, so the list is reversed first, but
/// macOS lists a Continuity iPhone after the built-in camera and it ended up
/// first: the call silently took over the phone's camera, including a phone
/// that was itself in the call. Virtual cameras (OBS, Camo…) often deliver
/// pixel formats nokhwa cannot open, so they go last.
#[cfg(not(target_os = "android"))]
fn camera_preference(cameras: &[nokhwa::utils::CameraInfo]) -> Vec<CameraIndex> {
    let rank = |camera: &nokhwa::utils::CameraInfo| {
        let name = camera.human_name().to_lowercase();
        let description = camera.description().to_lowercase();
        if VIRTUAL_CAMERA_MARKERS.iter().any(|m| name.contains(m)) {
            3
        } else if ["iphone", "ipad"]
            .iter()
            .any(|m| name.contains(m) || description.contains(m))
        {
            2
        } else if description.contains("builtin") {
            0
        } else {
            1
        }
    };
    let mut ordered: Vec<_> = cameras.iter().rev().collect();
    ordered.sort_by_key(|c| rank(c));
    ordered.into_iter().map(|c| c.index().clone()).collect()
}

#[cfg(not(target_os = "android"))]
impl VideoSource for CameraCapturer {
    fn name(&self) -> &str {
        "cam"
    }
    fn format(&self) -> VideoFormat {
        VideoFormat {
            pixel_format: PixelFormat::Rgba,
            dimensions: [self.width, self.height],
        }
    }

    fn start(&mut self) -> Result<()> {
        self.camera.open_stream()?;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.camera.stop_stream()?;
        Ok(())
    }

    fn pop_frame(&mut self) -> anyhow::Result<Option<VideoFrame>> {
        let start = std::time::Instant::now();
        let frame = self
            .camera
            .frame()
            .context("Failed to capture camera frame")?;
        trace!("pop frame: capture took {:?}", start.elapsed());
        let start = std::time::Instant::now();
        let frame = match frame.source_frame_format() {
            FrameFormat::MJPEG if std::env::var("IROH_LIVE_MJPEG_FFMPEG").is_ok() => {
                trace!("decode ffmpeg");
                self.mjpg_decoder.decode_frame(frame.buffer())?
            }
            _ => {
                let image = frame
                    .decode_image::<nokhwa::pixel_format::RgbAFormat>()
                    .context("Failed to decode camera frame")?;
                VideoFrame {
                    format: self.format(),
                    raw: image.into_raw().into(),
                }
            }
        };
        trace!("pop frame: decode took {:?}", start.elapsed());
        Ok(Some(frame))
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::ScreenCapturer;

    #[test]
    fn screen_capturer_is_send_without_moving_native_handles() {
        fn assert_send<T: Send>() {}
        assert_send::<ScreenCapturer>();
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod camera_preference_tests {
    use super::camera_preference;
    use nokhwa::utils::{CameraIndex, CameraInfo};

    fn camera(index: u32, name: &str, description: &str) -> CameraInfo {
        CameraInfo::new(name, description, "", CameraIndex::Index(index))
    }

    #[test]
    fn the_computers_own_camera_beats_a_continuity_iphone_and_obs() {
        // As macOS listed them on the MacBook that took over the iPhone.
        let cameras = [
            camera(
                0,
                "MacBook Pro Camera",
                "Apple Inc.: MacBook Pro Camera - AVCaptureDeviceTypeBuiltInWideAngleCamera",
            ),
            camera(
                1,
                "OBS Virtual Camera",
                "OBS Project: OBS Camera Extension - AVCaptureDeviceTypeExternal",
            ),
            camera(
                2,
                "Pratyush’s iPhone Camera",
                ": iPhone18,3 - AVCaptureDeviceTypeExternal",
            ),
        ];
        assert_eq!(
            camera_preference(&cameras),
            vec![
                CameraIndex::Index(0),
                CameraIndex::Index(2),
                CameraIndex::Index(1),
            ]
        );
    }

    #[test]
    fn a_plugged_in_webcam_comes_before_a_continuity_iphone() {
        let cameras = [
            camera(
                0,
                "Logitech BRIO",
                "Logitech: BRIO - AVCaptureDeviceTypeExternal",
            ),
            camera(
                1,
                "Desk iPad Camera",
                ": iPad16,3 - AVCaptureDeviceTypeExternal",
            ),
        ];
        assert_eq!(
            camera_preference(&cameras),
            vec![CameraIndex::Index(0), CameraIndex::Index(1)]
        );
    }
}
