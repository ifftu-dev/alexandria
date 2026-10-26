//! iOS media: AVFoundation capture, VideoToolbox H.264, the AVAudioSession
//! route and its interruptions, and the GCD main-thread hop they all need.
//!
//! ## Main-thread requirement
//!
//! AVFoundation (AVCaptureSession, AVAudioSession) and CoreAudio APIs must
//! be called from the main thread (or a thread with an active run loop).
//! Tauri commands execute on a tokio worker thread, so this adapter forwards
//! those calls with `dispatch_sync_f(dispatch_get_main_queue(), ...)`,
//! blocking until they complete. The call manager runs the adapter's
//! blocking methods on blocking threads, never under its lock.
//!
//! ## What the manager sees
//!
//! AVAudioSession interruptions, route changes and media-services resets
//! are observed here through an ObjC class registered at runtime. The
//! handler reconfigures the session on a debounce and then raises one
//! [`MediaEvent`]; the manager only resumes its outputs. A watcher thread
//! polls `UIApplication.applicationState` for the length of a session and
//! raises background/foreground events so the camera is released and
//! reopened. The idle timer is disabled while a session runs so the phone
//! does not lock mid-call.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use live::media::audio::AudioBackend;
use live::media::av::{Quality, VideoPreset};
use live::media::videotoolbox::{IosCameraSource, VtDecoder, VtEncoder};
use tokio::sync::broadcast;

use super::main_thread::run_on_main_thread;

use super::{
    AudioDeviceInfo, AudioDeviceInventory, AudioSession, CameraDeviceInfo, DeviceSelection,
    FrameTuning, MediaDevices, MediaEvent, MediaProfile, OpenedVideo,
};

/// Mobile video presets — lower resolutions to save battery and bandwidth.
const MOBILE_VIDEO_PRESETS: [VideoPreset; 2] = [VideoPreset::P360, VideoPreset::P720];

/// The rendition a phone watches: 720p at most. Its tiles are never larger
/// than the 960px remote viewport, and decoding plus copying out 1080p frames
/// fell behind the sender (15 Oct).
const PHONE_VIDEO_QUALITY: Quality = Quality::High;

// ── Shared ObjC FFI types ──────────────────────────────────────────

/// ObjC object pointer (id in ObjC terminology).
type ObjcId = *mut std::ffi::c_void;
/// ObjC selector (SEL in ObjC terminology).
type ObjcSel = *mut std::ffi::c_void;

unsafe extern "C" {
    fn objc_getClass(name: *const i8) -> ObjcId;
    fn sel_registerName(name: *const i8) -> ObjcSel;
    fn objc_msgSend(receiver: ObjcId, sel: ObjcSel, ...) -> ObjcId;
    fn objc_allocateClassPair(superclass: ObjcId, name: *const i8, extraBytes: usize) -> ObjcId;
    fn objc_registerClassPair(cls: ObjcId);
    fn class_addMethod(
        cls: ObjcId,
        name: ObjcSel,
        imp: *mut std::ffi::c_void,
        types: *const i8,
    ) -> i8;
}

/// Query `UIApplication.shared.applicationState` on the main thread.
///
/// Returns: 0 = UIApplicationStateActive, 1 = Inactive, 2 = Background.
fn get_application_state() -> i64 {
    run_on_main_thread(|| unsafe {
        type Id = *mut std::ffi::c_void;
        type Sel = *mut std::ffi::c_void;
        unsafe extern "C" {
            fn objc_getClass(name: *const i8) -> Id;
            fn sel_registerName(name: *const i8) -> Sel;
            fn objc_msgSend(receiver: Id, sel: Sel, ...) -> Id;
        }

        type MsgSendNoArgs = unsafe extern "C" fn(Id, Sel) -> Id;
        type MsgSendState = unsafe extern "C" fn(Id, Sel) -> i64;

        let send: MsgSendNoArgs =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        let send_state: MsgSendState =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);

        let cls = objc_getClass(c"UIApplication".as_ptr());
        if cls.is_null() {
            return -1;
        }

        let shared_sel = sel_registerName(c"sharedApplication".as_ptr());
        let app: Id = send(cls, shared_sel);
        if app.is_null() {
            return -1;
        }

        let state_sel = sel_registerName(c"applicationState".as_ptr());
        send_state(app, state_sel)
    })
}

/// Set `UIApplication.shared.isIdleTimerDisabled` on the main thread.
///
/// When `disabled` is true, the iPhone won't auto-lock during a tutoring session.
fn set_idle_timer_disabled(disabled: bool) {
    run_on_main_thread(move || unsafe {
        type Id = *mut std::ffi::c_void;
        type Sel = *mut std::ffi::c_void;
        unsafe extern "C" {
            fn objc_getClass(name: *const i8) -> Id;
            fn sel_registerName(name: *const i8) -> Sel;
            fn objc_msgSend(receiver: Id, sel: Sel, ...) -> Id;
        }

        type MsgSendNoArgs = unsafe extern "C" fn(Id, Sel) -> Id;
        type MsgSendBool = unsafe extern "C" fn(Id, Sel, i8);

        let send: MsgSendNoArgs =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        let send_bool: MsgSendBool =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);

        let cls = objc_getClass(c"UIApplication".as_ptr());
        if cls.is_null() {
            log::error!("tutoring: UIApplication class not found");
            return;
        }

        let shared_sel = sel_registerName(c"sharedApplication".as_ptr());
        let app: Id = send(cls, shared_sel);
        if app.is_null() {
            log::error!("tutoring: UIApplication.sharedApplication returned nil");
            return;
        }

        let set_idle_sel = sel_registerName(c"setIdleTimerDisabled:".as_ptr());
        send_bool(app, set_idle_sel, if disabled { 1 } else { 0 });

        log::info!("tutoring: idleTimerDisabled = {disabled}");
    });
}

const IOS_AUDIO_OUTPUT_AUTO_ID: &str = "ios:auto";
const IOS_AUDIO_OUTPUT_SPEAKER_ID: &str = "ios:speaker";
const IOS_AUDIO_PORT_ID_PREFIX: &str = "ios:port:";
const IOS_PORT_TYPE_BUILTIN_MIC: &str = "BuiltInMic";
const IOS_PORT_TYPE_BLUETOOTH_HFP: &str = "BluetoothHFP";
const IOS_PORT_TYPE_BLUETOOTH_LE: &str = "BluetoothLE";
const IOS_PORT_TYPE_HEADSET_MIC: &str = "HeadsetMic";
const IOS_PORT_TYPE_USB_AUDIO: &str = "USBAudio";
const IOS_PORT_TYPE_CAR_AUDIO: &str = "CarAudio";
const IOS_PORT_TYPE_CONTINUITY_MICROPHONE: &str = "ContinuityMicrophone";

#[derive(Debug, Clone)]
struct IosAudioPort {
    uid: String,
    name: String,
    port_type: String,
}

#[derive(Debug, Clone, Default)]
struct IosAudioRouteSnapshot {
    available_inputs: Vec<IosAudioPort>,
    current_input_uid: Option<String>,
}

// ── AVAudioSession observer ────────────────────────────────────────

/// Tracks whether the audio session was active before the last interruption.
/// Written by the ObjC interruption handler, read on interruption-end to decide
/// whether to reactivate the session.
static AUDIO_SESSION_WAS_ACTIVE: AtomicBool = AtomicBool::new(false);
static AUDIO_SESSION_OBSERVER: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
/// millis since UNIX_EPOCH of the most recent audio session recovery. Used to
/// debounce bursts of route-change notifications caused by the reconfigure
/// itself (setCategory/setActive fire CategoryChange → route change → etc).
static LAST_AUDIO_RECOVERY_MS: AtomicU64 = AtomicU64::new(0);
const AUDIO_RECOVERY_DEBOUNCE_MS: u64 = 2000;

/// Minimal handler invoked from ObjC when AVAudioSessionInterruptionNotification fires.
/// On `began`: records whether the session was active. On `ended`: reactivates if needed.
struct InterruptionBridge;

impl InterruptionBridge {
    fn new() -> Self {
        Self
    }

    fn handle_interruption(&self, notification: ObjcId) {
        unsafe {
            type MsgSendNoArgs = unsafe extern "C" fn(ObjcId, ObjcSel) -> ObjcId;
            type MsgSendCString = unsafe extern "C" fn(ObjcId, ObjcSel, *const i8) -> ObjcId;
            type MsgSendObj = unsafe extern "C" fn(ObjcId, ObjcSel, ObjcId) -> ObjcId;
            type MsgSendI32 = unsafe extern "C" fn(ObjcId, ObjcSel) -> i32;

            let nsnil: ObjcId = std::ptr::null_mut();
            let send_noargs: MsgSendNoArgs = std::mem::transmute(
                objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
            );
            let send_cstr: MsgSendCString = std::mem::transmute(
                objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
            );
            let send_obj: MsgSendObj = std::mem::transmute(
                objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
            );
            let send_i32: MsgSendI32 = std::mem::transmute(
                objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
            );

            let ns_string_cls = objc_getClass(c"NSString".as_ptr());
            if ns_string_cls.is_null() {
                log::error!("tutoring: NSString class not found while handling interruption");
                return;
            }

            let utf8_sel = sel_registerName(c"stringWithUTF8String:".as_ptr());
            let int_value_sel = sel_registerName(c"intValue".as_ptr());

            // AVAudioSessionInterruptionTypeKey
            let type_key = send_cstr(
                ns_string_cls,
                utf8_sel,
                c"AVAudioSessionInterruptionTypeKey".as_ptr(),
            );
            let user_info_sel = sel_registerName(c"userInfo".as_ptr());
            let user_info: ObjcId = if !notification.is_null() {
                send_noargs(notification, user_info_sel)
            } else {
                nsnil
            };
            let type_obj: ObjcId = if !user_info.is_null() {
                let dict_sel = sel_registerName(c"objectForKey:".as_ptr());
                send_obj(user_info, dict_sel, type_key)
            } else {
                nsnil
            };
            let interruption_type: i32 = if !type_obj.is_null() {
                send_i32(type_obj, int_value_sel)
            } else {
                0
            };

            if interruption_type == 0 {
                // Interruption began — record whether session was active so we restore it
                let shared_sel = sel_registerName(c"sharedInstance".as_ptr());
                let send_bool: unsafe extern "C" fn(ObjcId, ObjcSel) -> i8 = std::mem::transmute(
                    objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
                );
                let session: ObjcId =
                    send_noargs(objc_getClass(c"AVAudioSession".as_ptr()), shared_sel);
                let is_active_sel = sel_registerName(c"isActive".as_ptr());
                let was_active = send_bool(session, is_active_sel) != 0;
                AUDIO_SESSION_WAS_ACTIVE.store(was_active, Ordering::SeqCst);
                log::info!("tutoring: AVAudioSession interruption began, was_active={was_active}");
                crate::diag::log("iOS audio interruption began");
            } else {
                // Interruption ended — restore session if it was previously active
                let was_active = AUDIO_SESSION_WAS_ACTIVE.load(Ordering::SeqCst);
                if was_active {
                    log::info!("tutoring: AVAudioSession interruption ended, reactivating");
                    crate::diag::log("iOS audio interruption ended, reactivating");
                    schedule_audio_session_recovery("interruption ended");
                }
            }
        }
    }

    fn handle_route_change(&self, notification: ObjcId) {
        // Parse `AVAudioSessionRouteChangeReasonKey` from userInfo so we only
        // recover on genuine device events. Self-induced reasons (CategoryChange,
        // Override, RouteConfigurationChange) would otherwise produce an
        // infinite feedback loop: setCategory/setActive → route change → recovery
        // → setCategory/setActive → …
        let reason = unsafe {
            type MsgSendNoArgs = unsafe extern "C" fn(ObjcId, ObjcSel) -> ObjcId;
            type MsgSendCString = unsafe extern "C" fn(ObjcId, ObjcSel, *const i8) -> ObjcId;
            type MsgSendObj = unsafe extern "C" fn(ObjcId, ObjcSel, ObjcId) -> ObjcId;
            type MsgSendI32 = unsafe extern "C" fn(ObjcId, ObjcSel) -> i32;

            let send_noargs: MsgSendNoArgs = std::mem::transmute(
                objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
            );
            let send_cstr: MsgSendCString = std::mem::transmute(
                objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
            );
            let send_obj: MsgSendObj = std::mem::transmute(
                objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
            );
            let send_i32: MsgSendI32 = std::mem::transmute(
                objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
            );

            if notification.is_null() {
                return;
            }
            let user_info_sel = sel_registerName(c"userInfo".as_ptr());
            let user_info: ObjcId = send_noargs(notification, user_info_sel);
            if user_info.is_null() {
                return;
            }
            let ns_string_cls = objc_getClass(c"NSString".as_ptr());
            if ns_string_cls.is_null() {
                return;
            }
            let utf8_sel = sel_registerName(c"stringWithUTF8String:".as_ptr());
            let reason_key = send_cstr(
                ns_string_cls,
                utf8_sel,
                c"AVAudioSessionRouteChangeReasonKey".as_ptr(),
            );
            let dict_sel = sel_registerName(c"objectForKey:".as_ptr());
            let reason_obj: ObjcId = send_obj(user_info, dict_sel, reason_key);
            if reason_obj.is_null() {
                return;
            }
            let int_value_sel = sel_registerName(c"intValue".as_ptr());
            send_i32(reason_obj, int_value_sel)
        };

        // AVAudioSessionRouteChangeReason values:
        //   0 Unknown, 1 NewDeviceAvailable, 2 OldDeviceUnavailable,
        //   3 CategoryChange, 4 Override, 6 WakeFromSleep,
        //   7 NoSuitableRouteForCategory, 8 RouteConfigurationChange
        let should_recover = matches!(reason, 1 | 2 | 7);
        log::info!(
            "tutoring: AVAudioSession route changed, reason={reason}, recover={should_recover}"
        );
        crate::diag::log(&format!(
            "iOS audio route changed reason={reason} recover={should_recover}"
        ));
        if should_recover {
            schedule_audio_session_recovery("route change");
        }
    }

    fn handle_media_services_reset(&self, _notification: ObjcId) {
        log::warn!("tutoring: AVAudioSession media services reset, refreshing session/output");
        crate::diag::log("iOS audio media services reset, refreshing session/output");
        schedule_audio_session_recovery("media services reset");
    }
}

/// ObjC trampoline: invoked by NSNotificationCenter when AVAudioSession interruption fires.
/// Calls `InterruptionBridge::handle_interruption` with the notification userInfo.
unsafe extern "C" fn observer_trampoline(_self: ObjcId, _sel: ObjcSel, notification: ObjcId) {
    let bridge = InterruptionBridge::new();
    bridge.handle_interruption(notification);
}

unsafe extern "C" fn route_change_trampoline(_self: ObjcId, _sel: ObjcSel, notification: ObjcId) {
    let bridge = InterruptionBridge::new();
    bridge.handle_route_change(notification);
}

unsafe extern "C" fn media_services_reset_trampoline(
    _self: ObjcId,
    _sel: ObjcSel,
    notification: ObjcId,
) {
    let bridge = InterruptionBridge::new();
    bridge.handle_media_services_reset(notification);
}

/// Registers the AVAudioSession interruption observer. Safe to call multiple times
/// (uses Once). Dispatches registration to the main thread via GCD.
fn register_audio_session_observer() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        run_on_main_thread(|| unsafe {
            type MsgSendNoArgs = unsafe extern "C" fn(ObjcId, ObjcSel) -> ObjcId;
            type MsgSendCString = unsafe extern "C" fn(ObjcId, ObjcSel, *const i8) -> ObjcId;
            type MsgSendAddObserver =
                unsafe extern "C" fn(ObjcId, ObjcSel, ObjcId, ObjcSel, ObjcId, ObjcId);

            let send_noargs: MsgSendNoArgs = std::mem::transmute(
                objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
            );
            let send_cstr: MsgSendCString = std::mem::transmute(
                objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
            );
            let send_add_observer: MsgSendAddObserver = std::mem::transmute(
                objc_msgSend as unsafe extern "C" fn(ObjcId, ObjcSel, ...) -> ObjcId,
            );

            let cls_name = c"AudioSessionObserver".as_ptr();
            let mut cls = objc_getClass(cls_name);
            if cls.is_null() {
                cls = objc_allocateClassPair(objc_getClass(c"NSObject".as_ptr()), cls_name, 0);
                if cls.is_null() {
                    log::error!("tutoring: failed to allocate AudioSessionObserver class");
                    return;
                }
                let v08 = c"v@:@".as_ptr();
                class_addMethod(
                    cls,
                    sel_registerName(c"handleInterruption:".as_ptr()),
                    observer_trampoline as *mut std::ffi::c_void,
                    v08,
                );
                class_addMethod(
                    cls,
                    sel_registerName(c"handleRouteChange:".as_ptr()),
                    route_change_trampoline as *mut std::ffi::c_void,
                    v08,
                );
                class_addMethod(
                    cls,
                    sel_registerName(c"handleMediaServicesReset:".as_ptr()),
                    media_services_reset_trampoline as *mut std::ffi::c_void,
                    v08,
                );
                objc_registerClassPair(cls);
                log::info!("tutoring: AudioSessionObserver class registered at startup");
            }

            let observer = {
                let existing = AUDIO_SESSION_OBSERVER.load(Ordering::SeqCst);
                if !existing.is_null() {
                    existing
                } else {
                    let alloc_sel = sel_registerName(c"alloc".as_ptr());
                    let init_sel = sel_registerName(c"init".as_ptr());
                    let allocated = send_noargs(cls, alloc_sel);
                    if allocated.is_null() {
                        log::error!("tutoring: failed to allocate AudioSessionObserver instance");
                        return;
                    }
                    let instance = send_noargs(allocated, init_sel);
                    if instance.is_null() {
                        log::error!("tutoring: failed to init AudioSessionObserver instance");
                        return;
                    }
                    AUDIO_SESSION_OBSERVER.store(instance, Ordering::SeqCst);
                    instance
                }
            };

            let nsstring_cls = objc_getClass(c"NSString".as_ptr());
            if nsstring_cls.is_null() {
                log::error!("tutoring: NSString class not found during observer registration");
                return;
            }
            let utf8_sel = sel_registerName(c"stringWithUTF8String:".as_ptr());
            let center_cls = objc_getClass(c"NSNotificationCenter".as_ptr());
            if center_cls.is_null() {
                log::error!(
                    "tutoring: NSNotificationCenter class not found during observer registration"
                );
                return;
            }
            let center_sel = sel_registerName(c"defaultCenter".as_ptr());
            let center: ObjcId = send_noargs(center_cls, center_sel);
            let add_obs_sel = sel_registerName(c"addObserver:selector:name:object:".as_ptr());
            let int_notif = send_cstr(
                nsstring_cls,
                utf8_sel,
                c"AVAudioSessionInterruptionNotification".as_ptr(),
            );
            let int_sel = sel_registerName(c"handleInterruption:".as_ptr());
            let nil: ObjcId = std::ptr::null_mut();
            send_add_observer(center, add_obs_sel, observer, int_sel, int_notif, nil);
            let route_notif = send_cstr(
                nsstring_cls,
                utf8_sel,
                c"AVAudioSessionRouteChangeNotification".as_ptr(),
            );
            let route_sel = sel_registerName(c"handleRouteChange:".as_ptr());
            send_add_observer(center, add_obs_sel, observer, route_sel, route_notif, nil);
            let reset_notif = send_cstr(
                nsstring_cls,
                utf8_sel,
                c"AVAudioSessionMediaServicesWereResetNotification".as_ptr(),
            );
            let reset_sel = sel_registerName(c"handleMediaServicesReset:".as_ptr());
            send_add_observer(center, add_obs_sel, observer, reset_sel, reset_notif, nil);
            log::info!("tutoring: AVAudioSession interruption observer registered");
        });
    });
}

// ── AVAudioSession route and configuration ─────────────────────────

fn ios_port_device_id(uid: &str) -> String {
    format!("{IOS_AUDIO_PORT_ID_PREFIX}{uid}")
}

fn ios_port_uid(device_id: Option<&str>) -> Option<&str> {
    device_id.and_then(|id| id.strip_prefix(IOS_AUDIO_PORT_ID_PREFIX))
}

fn ios_output_name_for_port(port: &IosAudioPort) -> String {
    match port.port_type.as_str() {
        IOS_PORT_TYPE_BLUETOOTH_HFP | IOS_PORT_TYPE_BLUETOOTH_LE => {
            format!("{} (Bluetooth)", port.name)
        }
        IOS_PORT_TYPE_HEADSET_MIC => format!("{} (Headset)", port.name),
        IOS_PORT_TYPE_USB_AUDIO => format!("{} (USB)", port.name),
        IOS_PORT_TYPE_CAR_AUDIO => format!("{} (Car Audio)", port.name),
        IOS_PORT_TYPE_CONTINUITY_MICROPHONE => {
            format!("{} (Continuity)", port.name)
        }
        _ => port.name.clone(),
    }
}

fn ios_input_name_for_port(port: &IosAudioPort) -> String {
    match port.port_type.as_str() {
        IOS_PORT_TYPE_BUILTIN_MIC => "iPhone Microphone".into(),
        IOS_PORT_TYPE_BLUETOOTH_HFP | IOS_PORT_TYPE_BLUETOOTH_LE => {
            format!("{} (Bluetooth Mic)", port.name)
        }
        IOS_PORT_TYPE_HEADSET_MIC => format!("{} (Headset Mic)", port.name),
        IOS_PORT_TYPE_USB_AUDIO => format!("{} (USB Mic)", port.name),
        IOS_PORT_TYPE_CAR_AUDIO => format!("{} (Car Mic)", port.name),
        IOS_PORT_TYPE_CONTINUITY_MICROPHONE => {
            format!("{} (Continuity Mic)", port.name)
        }
        _ => port.name.clone(),
    }
}

fn ios_port_supports_output(port: &IosAudioPort) -> bool {
    !matches!(port.port_type.as_str(), IOS_PORT_TYPE_BUILTIN_MIC)
}

fn ios_audio_route_snapshot() -> IosAudioRouteSnapshot {
    run_on_main_thread(|| unsafe {
        type Id = ObjcId;
        type Sel = ObjcSel;
        type MsgSendNoArgs = unsafe extern "C" fn(Id, Sel) -> Id;
        type MsgSendCount = unsafe extern "C" fn(Id, Sel) -> usize;
        type MsgSendIndex = unsafe extern "C" fn(Id, Sel, usize) -> Id;
        type MsgSendUtf8 = unsafe extern "C" fn(Id, Sel) -> *const i8;

        let send_noargs: MsgSendNoArgs =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        let send_count: MsgSendCount =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        let send_index: MsgSendIndex =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
        let send_utf8: MsgSendUtf8 =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);

        let to_string = |obj: Id| -> Option<String> {
            if obj.is_null() {
                return None;
            }
            let utf8_sel = sel_registerName(c"UTF8String".as_ptr());
            let cstr = send_utf8(obj, utf8_sel);
            if cstr.is_null() {
                None
            } else {
                Some(
                    std::ffi::CStr::from_ptr(cstr)
                        .to_string_lossy()
                        .into_owned(),
                )
            }
        };

        let ports_from_array = |array: Id| -> Vec<IosAudioPort> {
            if array.is_null() {
                return Vec::new();
            }
            let count_sel = sel_registerName(c"count".as_ptr());
            let object_sel = sel_registerName(c"objectAtIndex:".as_ptr());
            let uid_sel = sel_registerName(c"UID".as_ptr());
            let name_sel = sel_registerName(c"portName".as_ptr());
            let type_sel = sel_registerName(c"portType".as_ptr());

            let count = send_count(array, count_sel);
            let mut ports = Vec::with_capacity(count);
            for index in 0..count {
                let port = send_index(array, object_sel, index);
                if port.is_null() {
                    continue;
                }
                let Some(uid) = to_string(send_noargs(port, uid_sel)) else {
                    continue;
                };
                let name = to_string(send_noargs(port, name_sel)).unwrap_or_else(|| uid.clone());
                let port_type = to_string(send_noargs(port, type_sel)).unwrap_or_default();
                ports.push(IosAudioPort {
                    uid,
                    name,
                    port_type,
                });
            }
            ports
        };

        let cls = objc_getClass(c"AVAudioSession".as_ptr());
        if cls.is_null() {
            return IosAudioRouteSnapshot::default();
        }
        let shared_sel = sel_registerName(c"sharedInstance".as_ptr());
        let session = send_noargs(cls, shared_sel);
        if session.is_null() {
            return IosAudioRouteSnapshot::default();
        }

        let available_inputs_sel = sel_registerName(c"availableInputs".as_ptr());
        let current_route_sel = sel_registerName(c"currentRoute".as_ptr());
        let inputs_sel = sel_registerName(c"inputs".as_ptr());

        let available_inputs = ports_from_array(send_noargs(session, available_inputs_sel));
        let current_route = send_noargs(session, current_route_sel);
        let current_inputs = if current_route.is_null() {
            Vec::new()
        } else {
            ports_from_array(send_noargs(current_route, inputs_sel))
        };
        IosAudioRouteSnapshot {
            available_inputs,
            current_input_uid: current_inputs.first().map(|port| port.uid.clone()),
        }
    })
}

fn ios_selected_output_id(
    selection: Option<&DeviceSelection>,
    snapshot: &IosAudioRouteSnapshot,
) -> Option<String> {
    if let Some(device_id) = selection.and_then(|devices| devices.speaker_device_id.clone()) {
        return Some(device_id);
    }

    if let Some(uid) = snapshot.current_input_uid.as_deref() {
        return Some(ios_port_device_id(uid));
    }

    Some(IOS_AUDIO_OUTPUT_AUTO_ID.into())
}

fn build_audio_device_inventory(selection: Option<&DeviceSelection>) -> AudioDeviceInventory {
    configure_ios_audio_session_with_selection(selection);
    let snapshot = ios_audio_route_snapshot();
    let selected_input = selection
        .and_then(|devices| devices.mic_device_id.clone())
        .or_else(|| {
            snapshot
                .current_input_uid
                .as_deref()
                .map(ios_port_device_id)
        });
    let selected_output = ios_selected_output_id(selection, &snapshot);

    let inputs = snapshot
        .available_inputs
        .iter()
        .map(|port| {
            let id = ios_port_device_id(&port.uid);
            AudioDeviceInfo {
                name: Some(ios_input_name_for_port(port)),
                is_default: selected_input.as_deref() == Some(id.as_str()),
                id,
            }
        })
        .collect();

    let mut outputs = vec![
        AudioDeviceInfo {
            id: IOS_AUDIO_OUTPUT_AUTO_ID.into(),
            name: Some("Automatic (Speaker or Connected Device)".into()),
            is_default: selected_output.as_deref() == Some(IOS_AUDIO_OUTPUT_AUTO_ID),
        },
        AudioDeviceInfo {
            id: IOS_AUDIO_OUTPUT_SPEAKER_ID.into(),
            name: Some("iPhone Speaker".into()),
            is_default: selected_output.as_deref() == Some(IOS_AUDIO_OUTPUT_SPEAKER_ID),
        },
    ];

    for port in snapshot
        .available_inputs
        .iter()
        .filter(|port| ios_port_supports_output(port))
    {
        let id = ios_port_device_id(&port.uid);
        outputs.push(AudioDeviceInfo {
            name: Some(ios_output_name_for_port(port)),
            is_default: selected_output.as_deref() == Some(id.as_str()),
            id,
        });
    }

    AudioDeviceInventory {
        inputs,
        outputs,
        selected_input,
        selected_output,
    }
}

/// Configure the iOS AVAudioSession for play-and-record on the selected
/// route.
///
/// Must be called before any CoreAudio / AVCaptureSession usage, otherwise
/// iOS will crash when the mic or speaker is accessed. Dispatches to the
/// **main thread** via GCD because AVAudioSession APIs require it.
fn configure_ios_audio_session_with_selection(selection: Option<&DeviceSelection>) {
    run_on_main_thread(|| {
        unsafe {
            // ObjC: [[AVAudioSession sharedInstance] setCategory:AVAudioSessionCategoryPlayAndRecord
            //         withOptions:AVAudioSessionCategoryOptionDefaultToSpeaker|AllowBluetooth
            //         error:nil]
            // Then: [[AVAudioSession sharedInstance] setActive:YES error:nil]
            //
            // IMPORTANT: On arm64 iOS, objc_msgSend with C variadic `...` passes
            // extra arguments in different registers than ObjC methods expect.
            // We MUST cast objc_msgSend to the exact function pointer type for
            // each call signature. This is the standard pattern for raw ObjC FFI
            // on arm64.
            type Id = ObjcId;
            type Sel = ObjcSel;

            // Type aliases for specific objc_msgSend signatures.
            // Each matches the exact parameter list of the ObjC method being called.
            type MsgSendNoArgs = unsafe extern "C" fn(Id, Sel) -> Id;
            type MsgSendOnePtr = unsafe extern "C" fn(Id, Sel, *const i8) -> Id;
            type MsgSendCatOpts = unsafe extern "C" fn(Id, Sel, Id, u64, Id) -> i8; // BOOL return
            type MsgSendMode = unsafe extern "C" fn(Id, Sel, Id, Id) -> i8; // BOOL return
            type MsgSendActivate = unsafe extern "C" fn(Id, Sel, i8, Id) -> i8; // BOOL return

            let send: MsgSendNoArgs =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
            let send_ptr: MsgSendOnePtr =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
            let send_cat: MsgSendCatOpts =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
            let send_mode: MsgSendMode =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
            let send_act: MsgSendActivate =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);

            let cls = objc_getClass(c"AVAudioSession".as_ptr());
            if cls.is_null() {
                log::error!("tutoring: AVAudioSession class not found");
                return;
            }

            // [AVAudioSession sharedInstance]
            let shared_sel = sel_registerName(c"sharedInstance".as_ptr());
            let session: Id = send(cls, shared_sel);
            if session.is_null() {
                log::error!("tutoring: AVAudioSession.sharedInstance returned nil");
                return;
            }

            // Create NSString @"AVAudioSessionCategoryPlayAndRecord"
            // via [NSString stringWithUTF8String:"AVAudioSessionCategoryPlayAndRecord"]
            let nsstring_cls = objc_getClass(c"NSString".as_ptr());
            let utf8_sel = sel_registerName(c"stringWithUTF8String:".as_ptr());
            let category: Id = send_ptr(
                nsstring_cls,
                utf8_sel,
                c"AVAudioSessionCategoryPlayAndRecord".as_ptr(),
            );
            if category.is_null() {
                log::error!("tutoring: failed to create category NSString");
                return;
            }

            // [session setCategory:category withOptions:(DefaultToSpeaker|AllowBluetoothHFP) error:nil]
            //
            // Do not request AllowBluetoothA2DP here. For two-way tutoring/call audio we
            // want the system's voice route selection, not high-quality output-only A2DP.
            let options: u64 = 0x08 | 0x04;
            let set_cat_sel = sel_registerName(c"setCategory:withOptions:error:".as_ptr());
            let nil: Id = std::ptr::null_mut();
            let set_cat_ok: i8 = send_cat(session, set_cat_sel, category, options, nil);
            if set_cat_ok == 0 {
                crate::diag::log("configure_ios_audio_session: setCategory failed");
            } else {
                crate::diag::log("configure_ios_audio_session: setCategory OK");
            }

            // Use Default mode (not VideoChat). VideoChat clamps the iOS
            // hardware sample rate to 16/24 kHz which forces firewheel's
            // resampler to downsample 48 kHz Opus output and may drop samples
            // when its ring buffer isn't primed. Default mode allows the
            // native 48 kHz rate and doesn't add voice-processing filters
            // (we handle AEC separately via webrtc-audio-processing).
            let mode: Id = send_ptr(
                nsstring_cls,
                utf8_sel,
                c"AVAudioSessionModeDefault".as_ptr(),
            );
            if mode.is_null() {
                crate::diag::log("configure_ios_audio_session: failed to create mode NSString");
            } else {
                let set_mode_sel = sel_registerName(c"setMode:error:".as_ptr());
                let set_mode_ok: i8 = send_mode(session, set_mode_sel, mode, nil);
                if set_mode_ok == 0 {
                    crate::diag::log("configure_ios_audio_session: setMode(Default) failed");
                } else {
                    crate::diag::log("configure_ios_audio_session: setMode(Default) OK");
                }
            }

            // [session setActive:YES error:nil]
            let set_active_sel = sel_registerName(c"setActive:error:".as_ptr());
            let set_active_ok: i8 = send_act(session, set_active_sel, 1i8, nil);
            if set_active_ok == 0 {
                crate::diag::log("configure_ios_audio_session: setActive failed");
            } else {
                crate::diag::log("configure_ios_audio_session: setActive OK");
            }

            let available_inputs_sel = sel_registerName(c"availableInputs".as_ptr());
            let uid_sel = sel_registerName(c"UID".as_ptr());
            let count_sel = sel_registerName(c"count".as_ptr());
            let object_sel = sel_registerName(c"objectAtIndex:".as_ptr());
            let set_pref_input_sel = sel_registerName(c"setPreferredInput:error:".as_ptr());
            let override_sel = sel_registerName(c"overrideOutputAudioPort:error:".as_ptr());
            let utf8_string_sel = sel_registerName(c"UTF8String".as_ptr());

            type MsgSendCount = unsafe extern "C" fn(Id, Sel) -> usize;
            type MsgSendIndex = unsafe extern "C" fn(Id, Sel, usize) -> Id;
            type MsgSendObj = unsafe extern "C" fn(Id, Sel, Id, Id) -> i8;
            type MsgSendI64 = unsafe extern "C" fn(Id, Sel, i64, Id) -> i8;
            type MsgSendUtf8 = unsafe extern "C" fn(Id, Sel) -> *const i8;

            let send_count: MsgSendCount =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
            let send_index: MsgSendIndex =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
            let send_obj: MsgSendObj =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
            let send_i64: MsgSendI64 =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
            let send_utf8: MsgSendUtf8 =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);

            let preferred_uid = selection
                .and_then(|devices| {
                    ios_port_uid(devices.mic_device_id.as_deref())
                        .or_else(|| ios_port_uid(devices.speaker_device_id.as_deref()))
                })
                .map(str::to_string);
            let available_inputs = send(session, available_inputs_sel);
            let preferred_port = if let Some(uid) = preferred_uid.as_deref() {
                if available_inputs.is_null() {
                    nil
                } else {
                    let count = send_count(available_inputs, count_sel);
                    let mut matched: Id = nil;
                    for index in 0..count {
                        let port = send_index(available_inputs, object_sel, index);
                        if port.is_null() {
                            continue;
                        }
                        let uid_obj = send(port, uid_sel);
                        let uid_cstr = if uid_obj.is_null() {
                            std::ptr::null()
                        } else {
                            send_utf8(uid_obj, utf8_string_sel)
                        };
                        if uid_cstr.is_null() {
                            continue;
                        }
                        let candidate = std::ffi::CStr::from_ptr(uid_cstr).to_string_lossy();
                        if candidate == uid {
                            matched = port;
                            break;
                        }
                    }
                    matched
                }
            } else {
                nil
            };

            let set_pref_ok = send_obj(session, set_pref_input_sel, preferred_port, nil);
            if set_pref_ok == 0 {
                crate::diag::log("configure_ios_audio_session: setPreferredInput failed");
            }

            // Override rules:
            //   - No user selection (default) or user explicitly picked the
            //     built-in loudspeaker → force Speaker override. VideoChat
            //     mode otherwise defaults to the earpiece (very quiet / looks
            //     muted), which is the bug we're fixing.
            //   - User picked any other port (Bluetooth / wired / AirPods /
            //     car audio) → clear override; iOS will route to the active
            //     port automatically as long as it's connected.
            let port_override = match selection
                .and_then(|devices| devices.speaker_device_id.as_deref())
            {
                None | Some(IOS_AUDIO_OUTPUT_SPEAKER_ID) => i64::from(u32::from_be_bytes(*b"spkr")),
                Some(_) => 0,
            };
            let override_ok = send_i64(session, override_sel, port_override, nil);
            if override_ok == 0 {
                crate::diag::log("configure_ios_audio_session: overrideOutputAudioPort failed");
            }

            log::info!("tutoring: AVAudioSession configured for PlayAndRecord");
            crate::diag::log(
                "iOS audio session configured: PlayAndRecord + Default + DefaultToSpeaker + AllowBluetoothHFP",
            );

            // Log the negotiated hardware sample rate. Confirms the mode
            // change actually lifted the 16/24 kHz VideoChat clamp.
            type MsgSendF64 = unsafe extern "C" fn(Id, Sel) -> f64;
            let send_f64: MsgSendF64 =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
            let sr_sel = sel_registerName(c"sampleRate".as_ptr());
            let sample_rate = send_f64(session, sr_sel);
            crate::diag::log(&format!("iOS audio sampleRate: {sample_rate:.0} Hz"));

            // Log the currently active route so we can see exactly where
            // audio is going after each reconfigure. Essential for debugging
            // silent-output bugs where iOS silently picks a surprising port.
            let current_route_sel = sel_registerName(c"currentRoute".as_ptr());
            let outputs_sel = sel_registerName(c"outputs".as_ptr());
            let inputs_sel = sel_registerName(c"inputs".as_ptr());
            let port_type_sel = sel_registerName(c"portType".as_ptr());
            let port_name_sel = sel_registerName(c"portName".as_ptr());
            let route: Id = send(session, current_route_sel);
            if !route.is_null() {
                let log_ports = |label: &str, collection: Id| {
                    if collection.is_null() {
                        return;
                    }
                    let count = send_count(collection, count_sel);
                    if count == 0 {
                        crate::diag::log(&format!("iOS audio route {label}: <none>"));
                        return;
                    }
                    for index in 0..count {
                        let port = send_index(collection, object_sel, index);
                        if port.is_null() {
                            continue;
                        }
                        let type_obj = send(port, port_type_sel);
                        let name_obj = send(port, port_name_sel);
                        let type_c = if type_obj.is_null() {
                            std::ptr::null()
                        } else {
                            send_utf8(type_obj, utf8_string_sel)
                        };
                        let name_c = if name_obj.is_null() {
                            std::ptr::null()
                        } else {
                            send_utf8(name_obj, utf8_string_sel)
                        };
                        let type_str = if type_c.is_null() {
                            std::borrow::Cow::Borrowed("?")
                        } else {
                            std::ffi::CStr::from_ptr(type_c).to_string_lossy()
                        };
                        let name_str = if name_c.is_null() {
                            std::borrow::Cow::Borrowed("?")
                        } else {
                            std::ffi::CStr::from_ptr(name_c).to_string_lossy()
                        };
                        crate::diag::log(&format!(
                            "iOS audio route {label}[{index}]: {type_str} ({name_str})"
                        ));
                    }
                };
                log_ports("output", send(route, outputs_sel));
                log_ports("input", send(route, inputs_sel));
            }

            register_audio_session_observer();
        }
    });
}

// ── The adapter ──────────────────────────────────────────────────────

/// Events raised from the ObjC observer and the lifecycle watcher. One
/// sender for the process: the observer is registered once and outlives
/// any manager.
static EVENTS: OnceLock<broadcast::Sender<MediaEvent>> = OnceLock::new();
/// The route the current (or last) session asked for, so a recovery after
/// an interruption restores the learner's choice.
static SELECTION: Mutex<Option<DeviceSelection>> = Mutex::new(None);

fn events() -> &'static broadcast::Sender<MediaEvent> {
    EVENTS.get_or_init(|| broadcast::channel(16).0)
}

fn raise(event: MediaEvent) {
    let _ = events().send(event);
}

fn remembered_selection() -> Option<DeviceSelection> {
    SELECTION.lock().ok().and_then(|s| s.clone())
}

fn remember_selection(selection: Option<DeviceSelection>) {
    if let Ok(mut slot) = SELECTION.lock() {
        *slot = selection;
    }
}

pub struct IosMedia {
    /// Set while a session runs; the lifecycle watcher thread stops when it
    /// clears.
    session_running: Arc<AtomicBool>,
}

impl Default for IosMedia {
    fn default() -> Self {
        Self::new()
    }
}

impl IosMedia {
    pub fn new() -> Self {
        // Register the AVAudioSession observer once, at app start.
        register_audio_session_observer();
        Self {
            session_running: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl MediaDevices for IosMedia {
    type Audio = IosAudio;
    type VideoSource = IosCameraSource;
    type VideoEncoder = VtEncoder;
    type VideoDecoder = VtDecoder;

    fn profile(&self) -> MediaProfile {
        MediaProfile {
            name: "ios",
            video: true,
            screen_share: false,
            camera_switch: false,
            audio_route_switch: true,
            remote_quality: PHONE_VIDEO_QUALITY,
            // The decoder scales each frame to fit this box before the JPEG
            // step, which is the costly part: a 720x1280 self view encoded
            // several times a second held a core at 100% (iOS killed the app
            // for it). The self view only fills a phone-sized tile, so
            // 360x640 is plenty; a remote camera fits 960x960 whichever way
            // up it is held. The encoder scales again only if a frame still
            // arrives larger.
            self_view: FrameTuning {
                viewport: (360, 640),
                jpeg_max: (360, 640),
                jpeg_quality: 75,
                interval: Duration::from_millis(42),
            },
            remote_view: FrameTuning {
                viewport: (960, 960),
                jpeg_max: (960, 960),
                jpeg_quality: 70,
                interval: Duration::from_millis(50),
            },
            audio_level_interval: Duration::from_millis(200),
        }
    }

    async fn cameras(&self) -> Vec<CameraDeviceInfo> {
        // The front camera is the only one a call uses; it is always present
        // on a device (the simulator may not produce frames).
        vec![CameraDeviceInfo {
            index: "front".into(),
            name: "Front Camera".into(),
        }]
    }

    fn audio_devices(&self) -> AudioDeviceInventory {
        build_audio_device_inventory(remembered_selection().as_ref())
    }

    fn open_audio(&self, selection: &DeviceSelection) -> Result<IosAudio, String> {
        crate::diag::log("open_audio: configuring AVAudioSession");
        remember_selection(Some(selection.clone()));
        // Configure the audio session first — required before any CoreAudio
        // usage. CoreAudio (cpal) init also needs the main thread on iOS.
        configure_ios_audio_session_with_selection(Some(selection));
        let backend = run_on_main_thread(|| AudioBackend::new_with_devices(None, None));
        crate::diag::log("open_audio: OK");
        Ok(IosAudio { backend })
    }

    fn open_camera(&self, _camera: Option<&str>) -> Result<OpenedVideo<IosCameraSource>, String> {
        // AVCaptureSession has to be set up on the main thread.
        let camera = self
            .on_main_thread(IosCameraSource::front)
            .map_err(|e| format!("camera unavailable: {e}"))?;
        Ok(OpenedVideo {
            source: camera,
            camera_name: None,
            presets: MOBILE_VIDEO_PRESETS.to_vec(),
        })
    }

    fn open_screen(&self) -> Result<OpenedVideo<IosCameraSource>, String> {
        Err("Screen sharing is not available on mobile".into())
    }

    fn set_audio_route(&self, selection: &DeviceSelection) -> Result<(), String> {
        remember_selection(Some(selection.clone()));
        configure_ios_audio_session_with_selection(Some(selection));
        Ok(())
    }

    fn events(&self) -> broadcast::Receiver<MediaEvent> {
        events().subscribe()
    }

    fn session_started(&self) {
        set_idle_timer_disabled(true);
        if !self.session_running.swap(true, Ordering::SeqCst) {
            start_lifecycle_watcher(self.session_running.clone());
        }
    }

    fn session_ended(&self) {
        self.session_running.store(false, Ordering::SeqCst);
        remember_selection(None);
        set_idle_timer_disabled(false);
    }

    fn on_main_thread<R: Send>(&self, work: impl FnOnce() -> R + Send) -> R {
        run_on_main_thread(work)
    }
}

/// The cpal backend behind an AVAudioSession that is reconfigured before
/// each speaker stream opens, so the stream lands on the learner's route.
#[derive(Clone)]
pub struct IosAudio {
    backend: AudioBackend,
}

impl AudioSession for IosAudio {
    type Input = live::media::audio::InputStream;
    type Output = live::media::audio::OutputStream;

    async fn input(&self) -> Result<Self::Input, String> {
        AudioSession::input(&self.backend).await
    }

    async fn output(&self) -> Result<Self::Output, String> {
        configure_ios_audio_session_with_selection(remembered_selection().as_ref());
        AudioSession::output(&self.backend).await
    }
}

/// Reconfigure the audio session after an interruption, route change or
/// media-services reset, then tell the manager to resume its outputs.
///
/// Debounced: the reconfigure itself emits route-change / media-services
/// notifications, so without a guard it would recurse.
fn schedule_audio_session_recovery(reason: &'static str) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last = LAST_AUDIO_RECOVERY_MS.load(Ordering::SeqCst);
    if now_ms.saturating_sub(last) < AUDIO_RECOVERY_DEBOUNCE_MS {
        log::info!("tutoring: skipping iOS audio recovery after {reason} (debounced)");
        return;
    }
    LAST_AUDIO_RECOVERY_MS.store(now_ms, Ordering::SeqCst);

    log::info!("tutoring: scheduling iOS audio recovery after {reason}");
    crate::diag::log(&format!("iOS audio recovery scheduled after {reason}"));

    // Off the notification's thread: the reconfigure dispatches to the main
    // queue itself.
    std::thread::spawn(move || {
        configure_ios_audio_session_with_selection(remembered_selection().as_ref());
        raise(if reason == "interruption ended" {
            MediaEvent::AudioResumed
        } else {
            MediaEvent::AudioRouteChanged
        });
    });
}

/// Poll `UIApplication.applicationState` while a session runs and raise
/// background/foreground transitions, so the camera is released on the way
/// out and reopened after unlock.
fn start_lifecycle_watcher(running: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("call-lifecycle".into())
        .spawn(move || {
            // UIApplicationState: 0 = Active; every other available state is
            // treated as inactive for capture teardown.
            const STATE_ACTIVE: i64 = 0;
            let mut was_active = get_application_state() == STATE_ACTIVE;
            while running.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1500));
                let state = get_application_state();
                if state < 0 {
                    continue; // UIApplication not available
                }
                let is_active = state == STATE_ACTIVE;
                if was_active && !is_active {
                    log::info!("tutoring: app entering background");
                    raise(MediaEvent::AppBackgrounded);
                } else if !was_active && is_active {
                    log::info!("tutoring: app returning to foreground");
                    raise(MediaEvent::AppForegrounded);
                }
                was_active = is_active;
            }
            log::info!("tutoring: lifecycle watcher ended");
        })
        .ok();
}
