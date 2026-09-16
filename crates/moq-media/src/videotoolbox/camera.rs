// Copyright 2025 N0, INC
// Modified by Alexandria Pvt. Ltd. — see crates/VENDORING.md
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

//! iOS camera capture using AVFoundation (AVCaptureSession).
//!
//! Implements the `VideoSource` trait by capturing BGRA frames from the
//! device camera via Objective-C runtime FFI. No `objc2` crate dependency —
//! uses raw `objc_msgSend` calls for maximum control and minimal deps.
//!
//! ## Architecture
//!
//! 1. Creates an `AVCaptureSession` with a preferred HD preset (`1280x720`)
//!    and falls back to `.medium` (`640x480 BGRA`) when HD is unavailable.
//! 2. Adds an `AVCaptureDeviceInput` for the front camera (or back as fallback).
//! 3. Adds an `AVCaptureVideoDataOutput` configured for BGRA pixel format.
//! 4. Sets a delegate (Rust-allocated ObjC class) that receives
//!    `captureOutput:didOutputSampleBuffer:fromConnection:` callbacks.
//! 5. The delegate callback locks the CVPixelBuffer, copies BGRA data, and
//!    pushes it into a `std::sync::mpsc::SyncSender<VideoFrame>`.
//! 6. `pop_frame()` drains the receiver, returning the latest frame.
//!
//! ## Important
//!
//! AVFoundation NSString constants (e.g. `AVCaptureSessionPreset1280x720`,
//! `AVCaptureSessionPresetMedium`, `AVMediaTypeVideo`) are loaded at runtime
//! via `dlsym` from the framework binary — NOT created as literal NSStrings.
//! This is critical because these constants are framework-exported `NSString *`
//! globals, and passing a manually-created NSString with the same text content
//! will NOT match.
//!
//! ## Safety
//!
//! Uses extensive `unsafe` for ObjC runtime and CoreVideo FFI.
//! All ObjC objects are retained/released correctly.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::{OnceLock, mpsc};

use anyhow::{Result, bail};
use bytes::Bytes;

use crate::av::{PixelFormat, VideoFormat, VideoFrame, VideoSource};

// ── ObjC runtime types ──────────────────────────────────────────────

type Id = *mut c_void;
type Class = *mut c_void;
type Sel = *mut c_void;
type Imp = *const c_void;
type ObjcBool = i8;

const NIL: Id = ptr::null_mut();
const YES: ObjcBool = 1;
#[allow(dead_code)]
const NO: ObjcBool = 0;

// CVPixelBuffer types (re-used from encoder)
type CVPixelBufferRef = *const c_void;
type CMSampleBufferRef = *const c_void;

const K_CV_PIXEL_FORMAT_TYPE_32_BGRA: u32 = 0x42475241;

unsafe extern "C" {
    // ObjC runtime
    fn objc_getClass(name: *const c_char) -> Class;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend(receiver: Id, sel: Sel, ...) -> Id;
    fn objc_allocateClassPair(superclass: Class, name: *const c_char, extra_bytes: usize) -> Class;
    fn objc_registerClassPair(cls: Class);
    fn class_addMethod(cls: Class, sel: Sel, imp: Imp, types: *const c_char) -> ObjcBool;
    fn class_addIvar(
        cls: Class,
        name: *const c_char,
        size: usize,
        alignment: u8,
        types: *const c_char,
    ) -> ObjcBool;
    fn object_getInstanceVariable(obj: Id, name: *const c_char, out_value: *mut *mut c_void) -> Id;
    fn object_setInstanceVariable(obj: Id, name: *const c_char, value: *mut c_void) -> Id;

    // CoreVideo
    fn CVPixelBufferLockBaseAddress(pb: CVPixelBufferRef, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(pb: CVPixelBufferRef, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddress(pb: CVPixelBufferRef) -> *const c_void;
    fn CVPixelBufferGetBytesPerRow(pb: CVPixelBufferRef) -> usize;
    fn CVPixelBufferGetWidth(pb: CVPixelBufferRef) -> usize;
    fn CVPixelBufferGetHeight(pb: CVPixelBufferRef) -> usize;

    // CoreMedia
    fn CMSampleBufferGetImageBuffer(sbuf: CMSampleBufferRef) -> CVPixelBufferRef;

    // Dynamic linker
    fn dlsym(handle: *mut c_void, symbol: *const u8) -> *mut c_void;
}

/// Sentinel handle meaning "search all loaded dylibs".
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

// ── Typed objc_msgSend wrappers ─────────────────────────────────────
//
// On arm64 iOS, the C variadic `objc_msgSend(Id, Sel, ...)` declaration
// causes extra arguments to be passed in **variadic registers** (stack),
// but ObjC methods expect them in **fixed parameter registers** (x2, x3, …).
// This ABI mismatch causes SIGSEGV. The fix is to transmute `objc_msgSend`
// to a concrete function pointer type matching the exact method signature
// before each call.

/// Zero-arg message send (e.g. `[obj alloc]`, `[obj init]`, `[obj release]`).
/// These are safe even with variadic decl, but we use typed version for consistency.
unsafe fn msg_send_0(obj: Id, sel_name: &CStr) -> Id {
    type F = unsafe extern "C" fn(Id, Sel) -> Id;
    // SAFETY: `sel_name` is NUL-terminated and the typed function signature
    // matches the Objective-C method selected by every caller of this helper.
    unsafe {
        let sel = sel_registerName(sel_name.as_ptr());
        let f: F = std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        f(obj, sel)
    }
}

/// Zero-arg message send with no return value (e.g. `startRunning`, `release`).
unsafe fn msg_send_void_0(obj: Id, sel_name: &CStr) {
    type F = unsafe extern "C" fn(Id, Sel);
    // SAFETY: `sel_name` is NUL-terminated and identifies a void method.
    unsafe {
        let sel = sel_registerName(sel_name.as_ptr());
        let f: F = std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        f(obj, sel);
    }
}

/// One-arg message send where arg is Id (pointer-sized).
unsafe fn msg_send_1id(obj: Id, sel_name: &CStr, a1: Id) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Id) -> Id;
    // SAFETY: see `msg_send_0`; this helper is only used for one object argument.
    unsafe {
        let sel = sel_registerName(sel_name.as_ptr());
        let f: F = std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        f(obj, sel, a1)
    }
}

/// One-object-argument message send with no return value.
unsafe fn msg_send_void_1id(obj: Id, sel_name: &CStr, a1: Id) {
    type F = unsafe extern "C" fn(Id, Sel, Id);
    // SAFETY: `sel_name` identifies a void method with one object argument.
    unsafe {
        let sel = sel_registerName(sel_name.as_ptr());
        let f: F = std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        f(obj, sel, a1);
    }
}

/// One-arg message send where arg is Id and return value is BOOL.
unsafe fn msg_send_bool_1id(obj: Id, sel_name: &CStr, a1: Id) -> ObjcBool {
    type F = unsafe extern "C" fn(Id, Sel, Id) -> ObjcBool;
    // SAFETY: see `msg_send_0`; this helper has the method's BOOL return ABI.
    unsafe {
        let sel = sel_registerName(sel_name.as_ptr());
        let f: F = std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        f(obj, sel, a1)
    }
}

/// One-arg message send where arg is c_int (for BOOL/int params).
unsafe fn msg_send_void_1int(obj: Id, sel_name: &CStr, a1: c_int) {
    type F = unsafe extern "C" fn(Id, Sel, c_int);
    // SAFETY: `sel_name` identifies a void method with one integer argument.
    unsafe {
        let sel = sel_registerName(sel_name.as_ptr());
        let f: F = std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        f(obj, sel, a1);
    }
}

/// One-arg message send where arg is u32 (for numberWithUnsignedInt:).
unsafe fn msg_send_1u32(obj: Id, sel_name: &CStr, a1: u32) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, u32) -> Id;
    // SAFETY: see `msg_send_0`; this helper is only used for one u32 argument.
    unsafe {
        let sel = sel_registerName(sel_name.as_ptr());
        let f: F = std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        f(obj, sel, a1)
    }
}

/// Two-arg message send (Id, Id) — e.g. `[dict dictionaryWithObject:forKey:]`.
unsafe fn msg_send_2id(obj: Id, sel_name: &CStr, a1: Id, a2: Id) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Id, Id) -> Id;
    // SAFETY: see `msg_send_0`; this helper is only used for two object arguments.
    unsafe {
        let sel = sel_registerName(sel_name.as_ptr());
        let f: F = std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        f(obj, sel, a1, a2)
    }
}

/// Two-arg message send (Id, *mut Id) — e.g. `[cls deviceInputWithDevice:error:]`.
unsafe fn msg_send_id_perr(obj: Id, sel_name: &CStr, a1: Id, a2: *mut Id) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Id, *mut Id) -> Id;
    // SAFETY: see `msg_send_0`; this helper is only used for an object and NSError**.
    unsafe {
        let sel = sel_registerName(sel_name.as_ptr());
        let f: F = std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        f(obj, sel, a1, a2)
    }
}

/// Two-arg message send (Id, Id) — for setSampleBufferDelegate:queue:
unsafe fn msg_send_void_2id(obj: Id, sel_name: &CStr, a1: Id, a2: Id) {
    type F = unsafe extern "C" fn(Id, Sel, Id, Id);
    // SAFETY: `sel_name` identifies a void method with two object arguments.
    unsafe {
        let sel = sel_registerName(sel_name.as_ptr());
        let f: F = std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        f(obj, sel, a1, a2);
    }
}

/// Three-arg message send (Id, Id, isize) — e.g. `defaultDeviceWithDeviceType:mediaType:position:`.
unsafe fn msg_send_id_id_isize(obj: Id, sel_name: &CStr, a1: Id, a2: Id, a3: isize) -> Id {
    type F = unsafe extern "C" fn(Id, Sel, Id, Id, isize) -> Id;
    // SAFETY: see `msg_send_0`; this helper matches the three-argument method.
    unsafe {
        let sel = sel_registerName(sel_name.as_ptr());
        let f: F = std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        f(obj, sel, a1, a2, a3)
    }
}

// ── Framework constant loading ──────────────────────────────────────

/// Load an `NSString *` constant exported by a framework (e.g. AVFoundation).
///
/// These constants are global `NSString *` variables whose *address* is the
/// symbol. `dlsym` returns a pointer to the global, so we dereference once
/// to get the actual `NSString *` (`Id`).
///
/// Returns `NIL` if the symbol isn't found.
unsafe fn load_framework_nsstring(symbol: &CStr) -> Id {
    // SAFETY: `symbol` is NUL-terminated and RTLD_DEFAULT asks dyld to search
    // already-loaded images. A successful result is the address of an exported
    // `NSString *` global, so exactly one pointer read is valid.
    let ptr = unsafe { dlsym(RTLD_DEFAULT, symbol.as_ptr().cast()) };
    if ptr.is_null() {
        tracing::warn!("dlsym failed for {:?}", symbol.to_string_lossy());
        return NIL;
    }
    // The symbol is a `NSString * const` global — read the pointer value.
    unsafe { *(ptr as *const Id) }
}

// ── Delegate class registration ─────────────────────────────────────

/// The delegate's `_frameSender` ivar stores a raw pointer to
/// `Box<mpsc::SyncSender<VideoFrame>>`. It is set when the delegate is allocated
/// and freed when the camera source is dropped.
static DELEGATE_CLASS: OnceLock<usize> = OnceLock::new();

fn delegate_class() -> Class {
    *DELEGATE_CLASS.get_or_init(|| unsafe {
        let superclass = objc_getClass(c"NSObject".as_ptr());
        assert!(!superclass.is_null(), "NSObject class not found");
        let cls = objc_allocateClassPair(superclass, c"AlexandriaFrameDelegate".as_ptr(), 0);
        assert!(!cls.is_null(), "Failed to allocate ObjC delegate class");

        // Add ivar: void *_frameSender
        let pointer_alignment = std::mem::align_of::<*mut c_void>().trailing_zeros() as u8;
        assert_eq!(
            class_addIvar(
                cls,
                c"_frameSender".as_ptr(),
                std::mem::size_of::<*mut c_void>(),
                pointer_alignment,
                c"^v".as_ptr(),
            ),
            YES,
            "Failed to add frame-sender ivar"
        );

        // Add method: captureOutput:didOutputSampleBuffer:fromConnection:
        assert_eq!(
            class_addMethod(
                cls,
                sel_registerName(c"captureOutput:didOutputSampleBuffer:fromConnection:".as_ptr()),
                delegate_callback as Imp,
                c"v@:@@@".as_ptr(), // return void, self, _cmd, 3 id args
            ),
            YES,
            "Failed to add frame callback method"
        );

        objc_registerClassPair(cls);
        cls as usize
    }) as Class
}

/// ObjC method implementation for the delegate callback.
///
/// # Safety
/// Called by AVFoundation on its internal dispatch queue.
unsafe extern "C" fn delegate_callback(
    this: Id,
    _cmd: Sel,
    _output: Id,       // AVCaptureOutput
    sample_buffer: Id, // CMSampleBuffer
    _connection: Id,   // AVCaptureConnection
) {
    // An Objective-C callback is an FFI boundary. Contain any Rust panic here;
    // unwinding into AVFoundation would be undefined behavior. Teardown first
    // unregisters this delegate and drains its serial queue before freeing the
    // sender pointer used below.
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: AVFoundation supplies valid callback objects for the duration
        // of this call, and `IosCameraSource::drop` drains the callback queue
        // before invalidating `_frameSender`.
        unsafe { delegate_callback_inner(this, sample_buffer) }
    }));
    if result.is_err() {
        tracing::error!("panic contained inside iOS camera callback");
    }
}

unsafe fn delegate_callback_inner(this: Id, sample_buffer: Id) {
    if sample_buffer.is_null() {
        return;
    }

    // SAFETY: all raw objects are supplied by AVFoundation for this callback.
    // Bounds derived from CoreVideo are checked before constructing slices.
    unsafe {
        let mut sender_ptr: *mut c_void = ptr::null_mut();
        object_getInstanceVariable(this, c"_frameSender".as_ptr(), &mut sender_ptr);
        if sender_ptr.is_null() {
            return;
        }
        let sender = &*(sender_ptr as *const mpsc::SyncSender<VideoFrame>);

        let pixel_buffer = CMSampleBufferGetImageBuffer(sample_buffer as CMSampleBufferRef);
        if pixel_buffer.is_null() || CVPixelBufferLockBaseAddress(pixel_buffer, 1) != 0 {
            return;
        }
        let _pixel_buffer_lock = PixelBufferLock(pixel_buffer);

        let base = CVPixelBufferGetBaseAddress(pixel_buffer);
        let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
        let Ok(width) = u32::try_from(CVPixelBufferGetWidth(pixel_buffer)) else {
            return;
        };
        let Ok(height) = u32::try_from(CVPixelBufferGetHeight(pixel_buffer)) else {
            return;
        };
        let Some(expected_row) = (width as usize).checked_mul(4) else {
            return;
        };
        let Some(total) = expected_row.checked_mul(height as usize) else {
            return;
        };

        if base.is_null() || width == 0 || height == 0 || bytes_per_row < expected_row {
            return;
        }

        let data = if bytes_per_row == expected_row {
            std::slice::from_raw_parts(base.cast::<u8>(), total).to_vec()
        } else {
            let mut buf = Vec::with_capacity(total);
            for y in 0..height as usize {
                let Some(offset) = y.checked_mul(bytes_per_row) else {
                    return;
                };
                let row_ptr = base.cast::<u8>().add(offset);
                let row = std::slice::from_raw_parts(row_ptr, expected_row);
                buf.extend_from_slice(row);
            }
            buf
        };

        let frame = VideoFrame {
            format: VideoFormat {
                pixel_format: PixelFormat::Bgra,
                dimensions: [width, height],
            },
            raw: Bytes::from(data),
        };

        let _ = sender.try_send(frame);
    }
}

struct PixelBufferLock(CVPixelBufferRef);

impl Drop for PixelBufferLock {
    fn drop(&mut self) {
        // SAFETY: constructed only after a successful read-only lock and owns
        // exactly that lock for the callback's lexical lifetime.
        unsafe {
            CVPixelBufferUnlockBaseAddress(self.0, 1);
        }
    }
}

// ── IosCameraSource ─────────────────────────────────────────────────

/// iOS camera video source using AVCaptureSession.
///
/// Captures BGRA frames from the device camera and delivers them via
/// the `VideoSource` trait.
pub struct IosCameraSource {
    session: Id,  // AVCaptureSession (retained)
    output: Id,   // AVCaptureVideoDataOutput (retained)
    delegate: Id, // AlexandriaFrameDelegate (retained)
    callback_queue: Id,
    rx: mpsc::Receiver<VideoFrame>,
    /// Leaked Box<SyncSender> — freed on drop.
    sender_ptr: *mut mpsc::SyncSender<VideoFrame>,
    width: u32,
    height: u32,
    running: bool,
}

// SAFETY: this type is not Sync and all session mutations require `&mut self`.
// AVFoundation permits the retained session/output objects to be moved between
// threads, while frame delivery remains confined to `callback_queue`. Drop
// unregisters the delegate and synchronously drains that queue before freeing
// callback state or releasing the retained Objective-C objects.
unsafe impl Send for IosCameraSource {}

impl IosCameraSource {
    /// Create a new camera source using the back camera (front as fallback).
    ///
    /// Resolution prefers 1280x720 and falls back to 640x480.
    pub fn new() -> Result<Self> {
        Self::with_position(1) // 1 = AVCaptureDevicePositionBack
    }

    /// Create using front camera.
    pub fn front() -> Result<Self> {
        Self::with_position(2) // 2 = AVCaptureDevicePositionFront
    }

    fn with_position(position: isize) -> Result<Self> {
        let delegate_class = delegate_class();

        unsafe {
            // Load framework constants via dlsym
            let preset_hd = load_framework_nsstring(c"AVCaptureSessionPreset1280x720");
            let preset_medium = load_framework_nsstring(c"AVCaptureSessionPresetMedium");
            if preset_medium.is_null() {
                bail!("Failed to load AVCaptureSessionPresetMedium constant");
            }

            // Create AVCaptureSession
            let session_class = objc_getClass(c"AVCaptureSession".as_ptr());
            if session_class.is_null() {
                bail!("AVCaptureSession class not found");
            }
            let alloc = msg_send_0(session_class, c"alloc");
            let session: Id = msg_send_0(alloc, c"init");
            if session.is_null() {
                bail!("Failed to create AVCaptureSession");
            }

            let preferred_preset = if !preset_hd.is_null()
                && msg_send_bool_1id(session, c"canSetSessionPreset:", preset_hd) == YES
            {
                tracing::info!("iOS camera: using AVCaptureSessionPreset1280x720");
                preset_hd
            } else {
                tracing::info!("iOS camera: falling back to AVCaptureSessionPresetMedium");
                preset_medium
            };
            msg_send_void_1id(session, c"setSessionPreset:", preferred_preset);

            // Get camera device
            let device = find_camera_device(position);
            if device.is_null() {
                // Fallback: try the other position
                let fallback_pos = if position == 1 { 2 } else { 1 };
                let device = find_camera_device(fallback_pos);
                if device.is_null() {
                    release(session);
                    bail!("No camera device found");
                }
                Self::setup_session(session, device, delegate_class)
            } else {
                Self::setup_session(session, device, delegate_class)
            }
        }
    }

    fn setup_session(session: Id, device: Id, delegate_class: Class) -> Result<Self> {
        Self::setup_session_inner(session, device, delegate_class)
    }

    fn setup_session_inner(session: Id, device: Id, delegate_class: Class) -> Result<Self> {
        // SAFETY: the outer setup contract establishes valid retained inputs.
        // Each created object is checked and either transferred to the returned
        // owner or released on its error path.
        unsafe {
            // Create AVCaptureDeviceInput
            let input_class = objc_getClass(c"AVCaptureDeviceInput".as_ptr());
            let mut error: Id = NIL;
            let input: Id = msg_send_id_perr(
                input_class,
                c"deviceInputWithDevice:error:",
                device,
                &mut error as *mut Id,
            );
            if input.is_null() || !error.is_null() {
                release(session);
                bail!("Failed to create AVCaptureDeviceInput");
            }

            // Add input
            let can_add_input = msg_send_bool_1id(session, c"canAddInput:", input);
            if can_add_input != YES {
                release(session);
                bail!("Cannot add camera input to session");
            }
            msg_send_void_1id(session, c"addInput:", input);

            // Create AVCaptureVideoDataOutput
            let output_class = objc_getClass(c"AVCaptureVideoDataOutput".as_ptr());
            let alloc_out = msg_send_0(output_class, c"alloc");
            let output: Id = msg_send_0(alloc_out, c"init");
            if output.is_null() {
                release(session);
                bail!("Failed to create AVCaptureVideoDataOutput");
            }

            // Set pixel format to BGRA using the real kCVPixelBufferPixelFormatTypeKey
            let settings = create_pixel_format_settings(K_CV_PIXEL_FORMAT_TYPE_32_BGRA);
            if !settings.is_null() {
                msg_send_void_1id(output, c"setVideoSettings:", settings);
            }

            // Discard late frames
            msg_send_void_1int(output, c"setAlwaysDiscardsLateVideoFrames:", YES as c_int);

            // Create delegate
            let alloc_del = msg_send_0(delegate_class, c"alloc");
            let delegate: Id = msg_send_0(alloc_del, c"init");
            if delegate.is_null() {
                release(output);
                release(session);
                bail!("Failed to create frame delegate");
            }

            // Create channel for frames
            let (tx, rx) = mpsc::sync_channel::<VideoFrame>(2);
            let sender_box = Box::new(tx);
            let sender_ptr = Box::into_raw(sender_box);

            // Store sender in delegate ivar
            object_setInstanceVariable(
                delegate,
                c"_frameSender".as_ptr(),
                sender_ptr as *mut c_void,
            );

            // Set delegate with a serial dispatch queue
            let queue = create_dispatch_queue(c"org.alexandria.camera");
            msg_send_void_2id(output, c"setSampleBufferDelegate:queue:", delegate, queue);

            // Add output
            let can_add_output = msg_send_bool_1id(session, c"canAddOutput:", output);
            if can_add_output != YES {
                msg_send_void_2id(output, c"setSampleBufferDelegate:queue:", NIL, NIL);
                dispatch_release(queue);
                release(delegate);
                release(output);
                release(session);
                let _ = Box::from_raw(sender_ptr); // reclaim
                bail!("Cannot add video output to session");
            }
            msg_send_void_1id(session, c"addOutput:", output);

            // Rotate output to portrait so frames are upright when phone is
            // held vertically.  Changes output from 640×480 → 480×640.
            let media_type_video = load_framework_nsstring(c"AVMediaTypeVideo");
            if !media_type_video.is_null() {
                let connection: Id =
                    msg_send_1id(output, c"connectionWithMediaType:", media_type_video);
                if !connection.is_null() {
                    // AVCaptureVideoOrientationPortrait = 1
                    msg_send_void_1int(connection, c"setVideoOrientation:", 1);
                    tracing::info!("iOS camera: set videoOrientation = portrait");
                }
            }

            Ok(IosCameraSource {
                session,
                output,
                delegate,
                callback_queue: queue,
                rx,
                sender_ptr,
                width: 480,
                height: 640,
                running: false,
            })
        }
    }
}

impl Drop for IosCameraSource {
    fn drop(&mut self) {
        unsafe {
            if self.running {
                msg_send_void_0(self.session, c"stopRunning");
            }

            // Stop new callbacks, then wait until any callback that already
            // loaded `_frameSender` has returned before reclaiming it.
            msg_send_void_2id(self.output, c"setSampleBufferDelegate:queue:", NIL, NIL);
            dispatch_sync_f(self.callback_queue, ptr::null_mut(), dispatch_barrier);
            object_setInstanceVariable(self.delegate, c"_frameSender".as_ptr(), ptr::null_mut());

            // Reclaim the leaked sender
            if !self.sender_ptr.is_null() {
                let _ = Box::from_raw(self.sender_ptr);
                self.sender_ptr = ptr::null_mut();
            }

            release(self.delegate);
            release(self.output);
            release(self.session);
            dispatch_release(self.callback_queue);
        }
    }
}

impl VideoSource for IosCameraSource {
    fn name(&self) -> &str {
        "ios-camera"
    }

    fn format(&self) -> VideoFormat {
        VideoFormat {
            pixel_format: PixelFormat::Bgra,
            dimensions: [self.width, self.height],
        }
    }

    fn start(&mut self) -> Result<()> {
        if self.running {
            return Ok(());
        }
        unsafe {
            msg_send_void_0(self.session, c"startRunning");
        }
        self.running = true;
        tracing::info!("iOS camera started ({}x{})", self.width, self.height);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if !self.running {
            return Ok(());
        }
        unsafe {
            msg_send_void_0(self.session, c"stopRunning");
        }
        self.running = false;
        // Drain any pending frames
        while self.rx.try_recv().is_ok() {}
        tracing::info!("iOS camera stopped");
        Ok(())
    }

    fn pop_frame(&mut self) -> Result<Option<VideoFrame>> {
        // Drain to latest frame (non-blocking)
        let mut latest = None;
        while let Ok(frame) = self.rx.try_recv() {
            latest = Some(frame);
        }
        Ok(latest)
    }
}

// ── ObjC helper functions ───────────────────────────────────────────

unsafe fn release(obj: Id) {
    if !obj.is_null() {
        // SAFETY: callers pass one retained Objective-C reference and release
        // it at most once through this helper.
        unsafe { msg_send_void_0(obj, c"release") };
    }
}

/// Find a camera device by position (1=back, 2=front).
fn find_camera_device(position: isize) -> Id {
    find_camera_device_inner(position)
}

fn find_camera_device_inner(position: isize) -> Id {
    // SAFETY: runtime lookups and message sends use NUL-terminated names and
    // exact fixed signatures; returned autoreleased objects are only borrowed.
    unsafe {
        let device_class = objc_getClass(c"AVCaptureDevice".as_ptr());
        if device_class.is_null() {
            return NIL;
        }

        // Load framework constants
        let device_type = load_framework_nsstring(c"AVCaptureDeviceTypeBuiltInWideAngleCamera");
        let media_type = load_framework_nsstring(c"AVMediaTypeVideo");

        if !device_type.is_null() && !media_type.is_null() {
            let device: Id = msg_send_id_id_isize(
                device_class,
                c"defaultDeviceWithDeviceType:mediaType:position:",
                device_type,
                media_type,
                position,
            );
            if !device.is_null() {
                return device;
            }
        }

        // Fallback: AVCaptureDevice.defaultDeviceWithMediaType:
        if !media_type.is_null() {
            let device: Id = msg_send_1id(device_class, c"defaultDeviceWithMediaType:", media_type);
            if !device.is_null() {
                return device;
            }
        }

        NIL
    }
}

/// Create an NSDictionary with kCVPixelBufferPixelFormatTypeKey → pixel format.
///
/// Uses the real `kCVPixelBufferPixelFormatTypeKey` framework constant
/// loaded via `dlsym`.
fn create_pixel_format_settings(pixel_format: u32) -> Id {
    create_pixel_format_settings_inner(pixel_format)
}

fn create_pixel_format_settings_inner(pixel_format: u32) -> Id {
    // SAFETY: runtime lookups and message sends use NUL-terminated names and
    // exact fixed signatures; returned objects are autoreleased.
    unsafe {
        let nsnum_class = objc_getClass(c"NSNumber".as_ptr());
        let nsdict_class = objc_getClass(c"NSDictionary".as_ptr());
        if nsnum_class.is_null() || nsdict_class.is_null() {
            return NIL;
        }

        let num: Id = msg_send_1u32(nsnum_class, c"numberWithUnsignedInt:", pixel_format);

        // Load the real kCVPixelBufferPixelFormatTypeKey constant
        let key = load_framework_nsstring(c"kCVPixelBufferPixelFormatTypeKey");
        if key.is_null() || num.is_null() {
            return NIL;
        }

        msg_send_2id(nsdict_class, c"dictionaryWithObject:forKey:", num, key)
    }
}

// GCD dispatch queue creation
unsafe extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: Id) -> Id;
    fn dispatch_sync_f(queue: Id, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_release(queue: Id);
}

/// Create a serial dispatch queue for camera callbacks.
unsafe fn create_dispatch_queue(label: &CStr) -> Id {
    // SAFETY: label is NUL-terminated; NULL attributes request a serial queue.
    unsafe { dispatch_queue_create(label.as_ptr(), NIL) }
}

extern "C" fn dispatch_barrier(_context: *mut c_void) {}
