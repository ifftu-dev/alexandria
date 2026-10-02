//! macOS media-capture permission routing. Trusted main-frame requests use
//! WebKit/OS consent prompts; plugin frames require recorded host grants.
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
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, NSObject, Sel};
use objc2::{msg_send, sel, ClassType};
use objc2_foundation::NSString;

/// WKPermissionDecision values:
///   0 = prompt, 1 = grant, 2 = deny.
const WK_PERMISSION_DECISION_GRANT: i64 = 1;
const WK_PERMISSION_DECISION_PROMPT: i64 = 0;
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
fn capture_is_granted(capture_type: i64, g: MediaGrants) -> bool {
    match capture_type {
        WK_CAPTURE_TYPE_CAMERA => g.camera,
        WK_CAPTURE_TYPE_MICROPHONE => g.microphone,
        WK_CAPTURE_TYPE_CAMERA_AND_MICROPHONE => g.camera && g.microphone,
        // An unrecognised capture type is one this build does not know how to
        // ask consent for, so it cannot have been consented to.
        _ => false,
    }
}

fn capture_decision(
    main_frame: bool,
    trusted_app_origin: bool,
    capture_type: i64,
    grants: MediaGrants,
) -> i64 {
    if !matches!(
        capture_type,
        WK_CAPTURE_TYPE_CAMERA | WK_CAPTURE_TYPE_MICROPHONE | WK_CAPTURE_TYPE_CAMERA_AND_MICROPHONE
    ) {
        return WK_PERMISSION_DECISION_DENY;
    }
    if main_frame {
        // Sentinel/course camera controls live in the trusted main frame,
        // outside PluginHost's grants. Ask WebKit/the OS for consent there.
        return if trusted_app_origin {
            WK_PERMISSION_DECISION_PROMPT
        } else {
            WK_PERMISSION_DECISION_DENY
        };
    }
    if capture_is_granted(capture_type, grants) {
        WK_PERMISSION_DECISION_GRANT
    } else {
        WK_PERMISSION_DECISION_DENY
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
        let original: *mut AnyObject = msg_send![wk_webview, UIDelegate];
        if !original.is_null() && (*original).class() == cls {
            return;
        }
        let Some(delegate) = make_delegate(original) else {
            log::warn!("macOS: media-grant delegate alloc returned nil");
            return;
        };
        let _: () = msg_send![wk_webview, setUIDelegate: &*delegate];
        // WKWebView holds a weak reference. Keep the proxy and its original
        // Wry delegate alive for the app lifetime.
        std::mem::forget(delegate);
    }
}

// The pointer is retained by the proxy and released in dealloc. Forwarding
// preserves Wry's file pickers, JavaScript dialogs, and other UI callbacks.
unsafe fn original_delegate(this: &AnyObject) -> *mut AnyObject {
    let ivar = delegate_class()
        .instance_variable(c"originalDelegate")
        .expect("delegate ivar");
    unsafe { *ivar.load::<*mut AnyObject>(this) }
}

unsafe fn make_delegate(original: *mut AnyObject) -> Option<Retained<AnyObject>> {
    let cls = delegate_class();
    let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![cls, new] };
    if let Some(delegate) = &delegate {
        let original = unsafe { Retained::retain(original) };
        let ivar = cls
            .instance_variable(c"originalDelegate")
            .expect("delegate ivar");
        unsafe {
            *ivar.load_ptr::<*mut AnyObject>(delegate) = original
                .map(Retained::into_raw)
                .unwrap_or(std::ptr::null_mut());
        }
    }
    delegate
}

fn delegate_class() -> &'static AnyClass {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        let mut builder = ClassBuilder::new(c"AlexMediaGrantDelegate", NSObject::class())
            .expect("AlexMediaGrantDelegate class name collision");
        builder.add_ivar::<*mut AnyObject>(c"originalDelegate");

        unsafe extern "C-unwind" fn responds(this: &AnyObject, _cmd: Sel, selector: Sel) -> Bool {
            if this.class().responds_to(selector) {
                return Bool::YES;
            }
            let original = unsafe { original_delegate(this) };
            if original.is_null() {
                Bool::NO
            } else {
                unsafe { msg_send![original, respondsToSelector: selector] }
            }
        }
        unsafe extern "C-unwind" fn forward(this: &AnyObject, _cmd: Sel, _selector: Sel) -> *mut AnyObject {
            unsafe { original_delegate(this) }
        }
        unsafe extern "C-unwind" fn dealloc(this: &AnyObject, _cmd: Sel) {
            unsafe {
                drop(Retained::<AnyObject>::from_raw(original_delegate(this)));
                let _: () = msg_send![super(this, NSObject::class()), dealloc];
            }
        }
        unsafe {
            builder.add_method(sel!(respondsToSelector:), responds as unsafe extern "C-unwind" fn(_, _, _) -> _);
            builder.add_method(sel!(forwardingTargetForSelector:), forward as unsafe extern "C-unwind" fn(_, _, _) -> _);
            builder.add_method(sel!(dealloc), dealloc as unsafe extern "C-unwind" fn(_, _) -> _);
        }

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
            if _origin.is_null() || _frame.is_null() {
                unsafe { (*decision_handler).call((WK_PERMISSION_DECISION_DENY,)) };
                return;
            }
            let main_frame: bool = unsafe { msg_send![&*_frame, isMainFrame] };
            let protocol: Retained<NSString> = unsafe { msg_send![&*_origin, protocol] };
            let host: Retained<NSString> = unsafe { msg_send![&*_origin, host] };
            let protocol = protocol.to_string();
            let host = host.to_string();
            let trusted = (protocol == "tauri" && host == "localhost")
                || (cfg!(debug_assertions) && protocol == "http" && matches!(host.as_str(), "localhost" | "127.0.0.1"));
            let decision = capture_decision(main_frame, trusted, capture_type, current_grants());
            if decision == WK_PERMISSION_DECISION_DENY {
                log::warn!(
                    "macOS: denying media capture (type {capture_type}) — no matching user grant"
                );
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_proxy_preserves_optional_file_picker_callback() {
        unsafe extern "C-unwind" fn open_panel(
            _this: &AnyObject,
            _cmd: Sel,
            _webview: *mut AnyObject,
            _parameters: *mut AnyObject,
            _frame: *mut AnyObject,
            completion: *mut block2::Block<dyn Fn(*mut AnyObject)>,
        ) {
            unsafe { (*completion).call((std::ptr::null_mut(),)) };
        }
        unsafe {
            let mut builder =
                ClassBuilder::new(c"AlexOriginalUIDelegateTest", NSObject::class()).unwrap();
            let selector =
                sel!(webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:);
            builder.add_method(
                selector,
                open_panel as unsafe extern "C-unwind" fn(_, _, _, _, _, _) -> _,
            );
            let original: Retained<AnyObject> = msg_send![builder.register(), new];
            let proxy = make_delegate(Retained::as_ptr(&original).cast_mut()).unwrap();
            drop(original);

            let responds: bool = msg_send![&*proxy, respondsToSelector: selector];
            assert!(responds);
            let media: bool = msg_send![&*proxy, respondsToSelector: sel!(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:)];
            assert!(media);
            let unknown: bool = msg_send![&*proxy, respondsToSelector: sel!(alexUnknownCallback)];
            assert!(!unknown);

            let called = std::cell::Cell::new(false);
            let completion = block2::RcBlock::new(|_urls: *mut AnyObject| called.set(true));
            let nil = std::ptr::null_mut::<AnyObject>();
            let _: () = msg_send![&*proxy, webView: nil, runOpenPanelWithParameters: nil, initiatedByFrame: nil, completionHandler: &*completion];
            assert!(called.get());
        }
    }

    #[test]
    fn trusted_main_frame_prompts_and_plugin_still_requires_grant() {
        let none = MediaGrants::default();
        assert_eq!(
            capture_decision(true, true, WK_CAPTURE_TYPE_CAMERA, none),
            WK_PERMISSION_DECISION_PROMPT
        );
        assert_eq!(
            capture_decision(true, false, WK_CAPTURE_TYPE_CAMERA, none),
            WK_PERMISSION_DECISION_DENY
        );
        assert_eq!(
            capture_decision(false, true, WK_CAPTURE_TYPE_CAMERA, none),
            WK_PERMISSION_DECISION_DENY
        );
        let camera = MediaGrants {
            camera: true,
            microphone: false,
        };
        assert_eq!(
            capture_decision(false, true, WK_CAPTURE_TYPE_CAMERA, camera),
            WK_PERMISSION_DECISION_GRANT
        );
        assert_eq!(
            capture_decision(false, true, WK_CAPTURE_TYPE_CAMERA_AND_MICROPHONE, camera),
            WK_PERMISSION_DECISION_DENY
        );
        assert_eq!(
            capture_decision(true, true, 99, camera),
            WK_PERMISSION_DECISION_DENY
        );
    }
}
