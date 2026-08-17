//! macOS WKUIDelegate that auto-grants media-capture (microphone, camera)
//! permission requests issued by plugin iframes.
//!
//! `WKWebView` denies `getUserMedia` calls when no UIDelegate implements
//! `_webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:`.
//! Plugin iframes don't have a separate WKWebView — they share the main
//! window's webview — so the main webview's UIDelegate is what the OS
//! consults for iframe requests too.
//!
//! Our consent UX runs in PluginHost.vue (PermissionPrompt), and without a
//! delegate WebKit flat-out denies, which is what blocked the Music Reviews +
//! future camera plugins.
//!
//! What this delegate must NOT do is grant unconditionally. It used to, and
//! `_origin` and `_capture_type` were both ignored, so any frame in the webview
//! got camera and microphone with no OS prompt. The in-app prompt was the only
//! control, and a plugin is not obliged to use the postMessage bridge that
//! prompt lives behind — it can call `navigator.mediaDevices.getUserMedia()`
//! directly. Combined with the iframe's Permissions-Policy `allow` attribute
//! being built from *declared* capabilities, merely listing `camera` in a
//! manifest was enough to capture silently. Given the product context —
//! proctored assessment, learners including minors, guardian links — that is
//! the worst possible place for a silent capture path.
//!
//! So: grants are recorded by the host when the user actually consents (see
//! [`grant`] / [`revoke_all`]), and this delegate answers from that record.
//! The iframe `allow` attribute is now built from granted capabilities too, so
//! the two layers agree.

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, NSObject, Sel};
use objc2::{msg_send, sel, ClassType};

/// WKPermissionDecision values:
///   0 = prompt, 1 = grant, 2 = deny.
const WK_PERMISSION_DECISION_GRANT: i64 = 1;
/// WKPermissionDecision: deny.
const WK_PERMISSION_DECISION_DENY: i64 = 2;

/// `_WKCaptureType` values as WebKit passes them.
const WK_CAPTURE_TYPE_CAMERA: i64 = 0;
const WK_CAPTURE_TYPE_MICROPHONE: i64 = 1;
const WK_CAPTURE_TYPE_CAMERA_AND_MICROPHONE: i64 = 2;

/// What the user has consented to, for the plugin currently mounted.
///
/// A process-wide cell rather than a per-plugin map because exactly one plugin
/// iframe is mounted at a time and the host clears this on teardown — see
/// `PluginHost.vue`. Keeping it minimal is deliberate: this is consulted from
/// an Objective-C callback on WebKit's thread, and the less it can do there
/// the better.
#[derive(Default, Clone, Copy)]
pub struct MediaGrants {
    pub camera: bool,
    pub microphone: bool,
}

static GRANTS: std::sync::Mutex<MediaGrants> = std::sync::Mutex::new(MediaGrants {
    camera: false,
    microphone: false,
});

/// Record what the user granted for the plugin being mounted.
pub fn grant(grants: MediaGrants) {
    if let Ok(mut g) = GRANTS.lock() {
        *g = grants;
    }
}

/// Drop every grant. Called when a plugin is torn down, so a grant cannot
/// outlive the plugin it was given to.
pub fn revoke_all() {
    if let Ok(mut g) = GRANTS.lock() {
        *g = MediaGrants::default();
    }
}

fn current_grants() -> MediaGrants {
    GRANTS.lock().map(|g| *g).unwrap_or_default()
}

/// Whether the user has consented to this capture type.
///
/// `CameraAndMicrophone` needs both — a partial grant is a denial, because
/// WebKit gives us one answer for the pair and answering "yes" would hand over
/// the half that was never consented to.
fn capture_is_granted(capture_type: i64) -> bool {
    let g = current_grants();
    match capture_type {
        WK_CAPTURE_TYPE_CAMERA => g.camera,
        WK_CAPTURE_TYPE_MICROPHONE => g.microphone,
        WK_CAPTURE_TYPE_CAMERA_AND_MICROPHONE => g.camera && g.microphone,
        // An unrecognised capture type is one this build does not know how to
        // ask consent for, so it cannot have been consented to.
        _ => false,
    }
}

/// Install our UIDelegate on the given WKWebView. Idempotent across calls
/// (the dynamic class is created once and cached). The delegate object
/// itself is leaked so the WKWebView's weak reference stays valid for
/// the app lifetime.
///
/// SAFETY: `wk_webview` must be a valid retained `WKWebView` pointer.
pub fn install(wk_webview: &AnyObject) {
    let cls = delegate_class();
    unsafe {
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let delegate: *mut AnyObject = msg_send![alloc, init];
        if delegate.is_null() {
            log::warn!("macOS: media-grant delegate alloc returned nil");
            return;
        }
        // Set as UIDelegate. WKWebView holds a weak reference so we must
        // keep the delegate alive: we Box::leak the strong reference into
        // a 'static.
        let _: () = msg_send![wk_webview, setUIDelegate: delegate];
        // Leak: app-lifetime delegate.
        if let Some(retained) = Retained::<AnyObject>::from_raw(delegate) {
            std::mem::forget(retained);
        }
    }
}

fn delegate_class() -> &'static AnyClass {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        let mut builder = ClassBuilder::new(c"AlexMediaGrantDelegate", NSObject::class())
            .expect("AlexMediaGrantDelegate class name collision");

        // -- requestMediaCapturePermissionForOrigin --
        // `webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:`
        // Signature (id, SEL, id, id, id, NSInteger, void(^)(NSInteger))
        unsafe extern "C-unwind" fn request_media_capture(
            _this: *mut AnyObject,
            _cmd: Sel,
            _webview: *mut AnyObject,
            _origin: *mut AnyObject,
            _frame: *mut AnyObject,
            capture_type: i64,
            decision_handler: *mut block2::Block<dyn Fn(i64)>,
        ) {
            if decision_handler.is_null() {
                return;
            }
            // Answer from what the user actually consented to. A plugin that
            // calls getUserMedia without going through the host's prompt now
            // gets a denial rather than a camera.
            let decision = if capture_is_granted(capture_type) {
                WK_PERMISSION_DECISION_GRANT
            } else {
                log::warn!(
                    "macOS: denying media capture (type {capture_type}) — no matching user grant"
                );
                WK_PERMISSION_DECISION_DENY
            };
            unsafe { (*decision_handler).call((decision,)) };
        }
        unsafe {
            builder.add_method(
                sel!(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:),
                request_media_capture
                    as unsafe extern "C-unwind" fn(_, _, _, _, _, _, _) -> _,
            );
        }

        // -- requestDeviceOrientationAndMotionPermissionForOrigin --
        // Some macOS WebKit versions also call this for sensor APIs. Granted:
        // orientation and motion on a desktop machine reveal nothing about the
        // person, no Alexandria capability gates them, and a denial breaks
        // plugins that read them for layout. Spelled out rather than left as
        // "symmetry" so the difference from media capture is on the record.
        unsafe extern "C-unwind" fn request_device_motion(
            _this: *mut AnyObject,
            _cmd: Sel,
            _webview: *mut AnyObject,
            _origin: *mut AnyObject,
            _frame: *mut AnyObject,
            decision_handler: *mut block2::Block<dyn Fn(i64)>,
        ) {
            if decision_handler.is_null() {
                return;
            }
            unsafe { (*decision_handler).call((WK_PERMISSION_DECISION_GRANT,)) };
        }
        unsafe {
            builder.add_method(
                sel!(webView:requestDeviceOrientationAndMotionPermissionForOrigin:initiatedByFrame:decisionHandler:),
                request_device_motion
                    as unsafe extern "C-unwind" fn(_, _, _, _, _, _) -> _,
            );
        }

        builder.register()
    })
}
