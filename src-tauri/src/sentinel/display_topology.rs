//! Display-topology detection (Sentinel, native).
//!
//! Reports how many displays the assessment device is driving, whether
//! the app shares the screen with another app, and whether the screen is
//! being captured or mirrored. The webview sees none of this: a second
//! monitor, an iPad Split View pane, or an AirPlay mirror is invisible to
//! `window.screen`, and a window resize deliberately generates no signal
//! (see the retired resize heuristic in docs/sentinel.md).
//!
//! Best-effort per platform; always `None` rather than an error so a
//! missing capability never breaks monitoring.
//!
//! - Desktop (macOS, Windows, Linux X11/Wayland) — Tauri's monitor
//!   enumeration via `AppHandle::available_monitors`. Mirrored display
//!   sets are reported by the OS as one monitor, so `mirrored` only
//!   fires on duplicate geometry.
//! - iOS — `UIScreen.screens` (external display or AirPlay mirror),
//!   `UIScreen.mainScreen.isCaptured` (recording / mirroring / AirPlay),
//!   and the key window's bounds against the screen bounds (Split View,
//!   Slide Over, Stage Manager).
//! - Android — `MainActivity.displayTopology()`: `DisplayManager`
//!   displays plus `isInMultiWindowMode` / `isInPictureInPictureMode`.

use serde::{Deserialize, Serialize};

/// What the OS reports about the display arrangement right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayTopology {
    /// Displays the OS is driving, including the built-in one.
    pub display_count: u32,
    /// More than one display is attached, or a presentation / external
    /// display is connected (mobile).
    pub external_display: bool,
    /// The screen is being captured, recorded, or mirrored elsewhere.
    /// iOS reports this directly; desktop infers it from duplicate
    /// monitor geometry only; Android reports `false` in this phase.
    pub mirrored: bool,
    /// The app does not own the whole screen: multi-window / Split View /
    /// Slide Over / Picture-in-Picture (mobile only).
    pub split_screen: bool,
    /// Which probe produced this reading, for review context.
    pub source: String,
}

/// Minimal per-monitor geometry, decoupled from Tauri's `Monitor` so the
/// duplicate-geometry heuristic is unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Desktop topology from a monitor list. Two monitors with identical
/// position and size are the only mirroring the OS lets us see; mirrored
/// sets normally collapse to one entry.
pub fn from_monitors(monitors: &[MonitorGeometry], source: &str) -> DisplayTopology {
    let display_count = monitors.len() as u32;
    let mut mirrored = false;
    for (i, a) in monitors.iter().enumerate() {
        if monitors[i + 1..].iter().any(|b| b == a) {
            mirrored = true;
            break;
        }
    }
    DisplayTopology {
        display_count,
        external_display: display_count > 1,
        mirrored,
        split_screen: false,
        source: source.to_owned(),
    }
}

/// Parse the JSON `MainActivity.displayTopology()` returns.
///
/// Shape: `{"display_count": n, "presentation_count": n,
/// "multi_window": bool, "picture_in_picture": bool}`. Missing keys read
/// as zero / false so an older Kotlin half degrades to "nothing seen".
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn from_android_json(raw: &str) -> Result<DisplayTopology, String> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        display_count: u32,
        #[serde(default)]
        presentation_count: u32,
        #[serde(default)]
        multi_window: bool,
        #[serde(default)]
        picture_in_picture: bool,
    }
    let r: Raw = serde_json::from_str(raw).map_err(|e| format!("display topology JSON: {e}"))?;
    Ok(DisplayTopology {
        display_count: r.display_count.max(1),
        external_display: r.display_count > 1 || r.presentation_count > 0,
        mirrored: false,
        split_screen: r.multi_window || r.picture_in_picture,
        source: "android".to_owned(),
    })
}

/// iOS topology from the raw UIKit readings. Split screen means the key
/// window is smaller than the screen on either axis, with a one-point
/// tolerance for rounding.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub fn from_uikit(
    screen_count: usize,
    captured: bool,
    screen_size: (f64, f64),
    window_size: Option<(f64, f64)>,
) -> DisplayTopology {
    let split_screen = match window_size {
        Some((w, h)) => w + 1.0 < screen_size.0 || h + 1.0 < screen_size.1,
        None => false,
    };
    DisplayTopology {
        display_count: screen_count.max(1) as u32,
        external_display: screen_count > 1,
        mirrored: captured,
        split_screen,
        source: "uikit".to_owned(),
    }
}

/// Resolve the current display topology, or `None` if the platform
/// offers no probe or the probe failed.
pub fn current(app: &tauri::AppHandle) -> Option<DisplayTopology> {
    imp::current(app)
}

#[cfg(desktop)]
mod imp {
    use super::{from_monitors, DisplayTopology, MonitorGeometry};

    pub fn current(app: &tauri::AppHandle) -> Option<DisplayTopology> {
        let monitors = app.available_monitors().ok()?;
        let geometry: Vec<MonitorGeometry> = monitors
            .iter()
            .map(|m| MonitorGeometry {
                x: m.position().x,
                y: m.position().y,
                width: m.size().width,
                height: m.size().height,
            })
            .collect();
        if geometry.is_empty() {
            return None;
        }
        Some(from_monitors(&geometry, "tauri"))
    }
}

#[cfg(target_os = "ios")]
mod imp {
    use super::{from_uikit, DisplayTopology};

    pub fn current(_app: &tauri::AppHandle) -> Option<DisplayTopology> {
        // UIKit is main-thread-affine; the command runs on a tokio worker.
        crate::tutoring::manager_mobile::run_on_main_thread(|| read_uikit())
    }

    fn read_uikit() -> Option<DisplayTopology> {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_core_foundation::CGRect;

        objc2::rc::autoreleasepool(|_| {
            // SAFETY: every selector below is a plain UIKit getter on a
            // live class or on an object the previous call returned; all
            // run on the main thread (see `current`). Return types match
            // the UIKit declarations: NSArray / UIScreen / UIWindow
            // objects, NSUInteger counts, BOOL, CGRect.
            unsafe {
                let screen_cls = class!(UIScreen);
                let screens: Retained<AnyObject> = msg_send![screen_cls, screens];
                let screen_count: usize = msg_send![&*screens, count];
                let main: Retained<AnyObject> = msg_send![screen_cls, mainScreen];
                let captured: bool = msg_send![&*main, isCaptured];
                let screen_bounds: CGRect = msg_send![&*main, bounds];

                let app: Retained<AnyObject> = msg_send![class!(UIApplication), sharedApplication];
                let windows: Retained<AnyObject> = msg_send![&*app, windows];
                let window_count: usize = msg_send![&*windows, count];
                let window_size = if window_count > 0 {
                    let w: Retained<AnyObject> = msg_send![&*windows, firstObject];
                    let b: CGRect = msg_send![&*w, bounds];
                    Some((b.size.width, b.size.height))
                } else {
                    None
                };

                Some(from_uikit(
                    screen_count,
                    captured,
                    (screen_bounds.size.width, screen_bounds.size.height),
                    window_size,
                ))
            }
        })
    }
}

#[cfg(target_os = "android")]
mod imp {
    use super::{from_android_json, DisplayTopology};
    use jni::objects::JString;

    pub fn current(_app: &tauri::AppHandle) -> Option<DisplayTopology> {
        let json = crate::av_permissions::jni::with_activity_class(|env, class| {
            let response = JString::from(
                env.call_static_method(class, "displayTopology", "()Ljava/lang/String;", &[])?
                    .l()?,
            );
            let s: String = env.get_string(&response)?.into();
            Ok(s)
        })
        .map_err(|e| log::warn!(target: "sentinel", "display topology bridge: {e}"))
        .ok()?;
        from_android_json(&json)
            .map_err(|e| log::warn!(target: "sentinel", "{e}"))
            .ok()
    }
}

#[cfg(not(any(desktop, target_os = "ios", target_os = "android")))]
mod imp {
    use super::DisplayTopology;
    pub fn current(_app: &tauri::AppHandle) -> Option<DisplayTopology> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(x: i32, y: i32, w: u32, h: u32) -> MonitorGeometry {
        MonitorGeometry {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn single_monitor_is_clean() {
        let t = from_monitors(&[mon(0, 0, 2560, 1440)], "tauri");
        assert_eq!(t.display_count, 1);
        assert!(!t.external_display);
        assert!(!t.mirrored);
        assert!(!t.split_screen);
        assert_eq!(t.source, "tauri");
    }

    #[test]
    fn second_monitor_is_external_not_mirrored() {
        let t = from_monitors(&[mon(0, 0, 2560, 1440), mon(2560, 0, 1920, 1080)], "tauri");
        assert_eq!(t.display_count, 2);
        assert!(t.external_display);
        assert!(!t.mirrored);
    }

    #[test]
    fn duplicate_geometry_reads_as_mirrored() {
        let t = from_monitors(&[mon(0, 0, 1920, 1080), mon(0, 0, 1920, 1080)], "tauri");
        assert!(t.external_display);
        assert!(t.mirrored);
    }

    #[test]
    fn android_json_maps_multi_window_and_presentation() {
        let t = from_android_json(
            r#"{"display_count":2,"presentation_count":1,"multi_window":true,"picture_in_picture":false}"#,
        )
        .unwrap();
        assert_eq!(t.display_count, 2);
        assert!(t.external_display);
        assert!(t.split_screen);
        assert!(!t.mirrored);
        assert_eq!(t.source, "android");
    }

    #[test]
    fn android_json_pip_alone_is_split_screen() {
        let t = from_android_json(r#"{"display_count":1,"picture_in_picture":true}"#).unwrap();
        assert!(t.split_screen);
        assert!(!t.external_display);
    }

    #[test]
    fn android_json_missing_keys_degrade_to_clean() {
        let t = from_android_json("{}").unwrap();
        assert_eq!(t.display_count, 1);
        assert!(!t.external_display);
        assert!(!t.split_screen);
    }

    #[test]
    fn android_json_garbage_is_an_error() {
        assert!(from_android_json("not json").is_err());
    }

    #[test]
    fn uikit_full_screen_window_is_clean() {
        let t = from_uikit(1, false, (1024.0, 1366.0), Some((1024.0, 1366.0)));
        assert!(!t.split_screen);
        assert!(!t.mirrored);
        assert!(!t.external_display);
        assert_eq!(t.source, "uikit");
    }

    #[test]
    fn uikit_narrow_window_is_split_screen() {
        let t = from_uikit(1, false, (1024.0, 1366.0), Some((507.0, 1366.0)));
        assert!(t.split_screen);
    }

    #[test]
    fn uikit_rounding_does_not_trip_split_screen() {
        let t = from_uikit(1, false, (1024.0, 1366.0), Some((1023.5, 1366.0)));
        assert!(!t.split_screen);
    }

    #[test]
    fn uikit_captured_and_second_screen() {
        let t = from_uikit(2, true, (390.0, 844.0), Some((390.0, 844.0)));
        assert_eq!(t.display_count, 2);
        assert!(t.external_display);
        assert!(t.mirrored);
        assert!(!t.split_screen);
    }

    #[test]
    fn uikit_without_window_never_reports_split_screen() {
        let t = from_uikit(1, false, (390.0, 844.0), None);
        assert!(!t.split_screen);
    }

    #[test]
    fn serializes_with_snake_case_fields() {
        let t = from_monitors(&[mon(0, 0, 1, 1)], "tauri");
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["display_count"], 1);
        assert_eq!(v["external_display"], false);
        assert_eq!(v["mirrored"], false);
        assert_eq!(v["split_screen"], false);
        assert_eq!(v["source"], "tauri");
    }
}
