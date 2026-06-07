//! macOS WKUIDelegate that auto-grants media-capture (microphone, camera)
//! permission requests issued by plugin iframes.
//!
//! `WKWebView` denies `getUserMedia` calls when no UIDelegate implements
//! `_webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:`.
//! Plugin iframes don't have a separate WKWebView — they share the main
//! window's webview — so the main webview's UIDelegate is what the OS
//! consults for iframe requests too.
//!
//! Our consent UX already runs in PluginHost.vue (PermissionPrompt). The
//! WebKit-level prompt would be redundant, and without a delegate WebKit
//! flat-out denies, which is what blocks the Music Reviews + future
//! camera plugins.

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, NSObject, Sel};
use objc2::{msg_send, sel, ClassType};

/// WKPermissionDecision values:
///   0 = prompt, 1 = grant, 2 = deny.
const WK_PERMISSION_DECISION_GRANT: i64 = 1;

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
            _capture_type: i64,
            decision_handler: *mut block2::Block<dyn Fn(i64)>,
        ) {
            if decision_handler.is_null() {
                return;
            }
            unsafe { (*decision_handler).call((WK_PERMISSION_DECISION_GRANT,)) };
        }
        unsafe {
            builder.add_method(
                sel!(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:),
                request_media_capture
                    as unsafe extern "C-unwind" fn(_, _, _, _, _, _, _) -> _,
            );
        }

        // -- requestDeviceOrientationAndMotionPermissionForOrigin --
        // Some macOS WebKit versions also call this for sensor APIs; auto-
        // grant for symmetry. Signature ends with the same decisionHandler
        // block.
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
