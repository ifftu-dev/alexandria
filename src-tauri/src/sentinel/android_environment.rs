//! Android environment report and assessment shield (Sentinel, native).
//!
//! Android lets an app *prevent* two cheat channels the webview cannot
//! even see, and report a third:
//!
//! - **Shield** — `FLAG_SECURE` keeps the assessment window out of
//!   screenshots, screen recordings, casts and the recents thumbnail;
//!   `Window.setHideOverlayWindows(true)` (API 31+) hides every non-system
//!   overlay drawn over the app. Engaged for the life of an integrity
//!   session, released afterwards.
//! - **Obscured touches** — a touch that lands while another window is
//!   drawn over ours carries `FLAG_WINDOW_IS_OBSCURED`; the activity counts
//!   them and `take_obscured_touches` drains the count per window.
//! - **Environment** — enabled accessibility services (a non-system one can
//!   read the screen on the app's behalf), ADB / developer options
//!   (`scrcpy` mirroring to a helper's laptop), and whether the shield is
//!   actually engaged.
//!
//! The Kotlin half is `MainActivity.environmentReport()` /
//! `setAssessmentShield()` / `takeObscuredTouches()`, reached through the
//! same JNI bridge as `display_topology`. Every JSON key is optional so an
//! older Kotlin half degrades to "nothing observed". Non-Android builds
//! report `None` and treat the shield as a no-op.

use serde::{Deserialize, Serialize};

/// One enabled accessibility service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessibilityService {
    /// `<package>/<class>` as Android names it.
    pub id: String,
    /// Shipped with the system image (or an update to such an app).
    pub system: bool,
}

/// What the activity reports about its environment right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidEnvironment {
    /// Every enabled accessibility service.
    pub accessibility_services: Vec<AccessibilityService>,
    /// Ids of the non-system ones — the review-relevant subset.
    pub foreign_accessibility: Vec<String>,
    /// USB / wireless debugging is on.
    pub adb_enabled: bool,
    /// Developer options are unlocked.
    pub development_settings_enabled: bool,
    /// The shield is engaged (as the activity remembers it).
    pub shield_active: bool,
    /// `setHideOverlayWindows` exists on this device (API 31+).
    pub overlay_hiding_supported: bool,
    /// Running obscured-touch count at report time (not reset).
    pub obscured_touches: u32,
    /// `Build.VERSION.SDK_INT`.
    pub sdk_int: u32,
    /// Which probe produced this reading.
    pub source: String,
}

/// Parse the JSON `MainActivity.environmentReport()` returns.
///
/// Shape: `{"accessibility_services": [{"id": s, "system": b}],
/// "adb_enabled": b, "development_settings_enabled": b, "shield_active": b,
/// "overlay_hiding_supported": b, "obscured_touches": n, "sdk_int": n}`.
/// Missing keys read as empty / false / zero.
pub fn from_json(raw: &str) -> Result<AndroidEnvironment, String> {
    #[derive(Deserialize)]
    struct RawService {
        #[serde(default)]
        id: String,
        #[serde(default)]
        system: bool,
    }
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        accessibility_services: Vec<RawService>,
        #[serde(default)]
        adb_enabled: bool,
        #[serde(default)]
        development_settings_enabled: bool,
        #[serde(default)]
        shield_active: bool,
        #[serde(default)]
        overlay_hiding_supported: bool,
        #[serde(default)]
        obscured_touches: u32,
        #[serde(default)]
        sdk_int: u32,
    }
    let r: Raw = serde_json::from_str(raw).map_err(|e| format!("android environment JSON: {e}"))?;
    let accessibility_services: Vec<AccessibilityService> = r
        .accessibility_services
        .into_iter()
        .filter(|s| !s.id.is_empty())
        .map(|s| AccessibilityService {
            id: s.id,
            system: s.system,
        })
        .collect();
    let foreign_accessibility = accessibility_services
        .iter()
        .filter(|s| !s.system)
        .map(|s| s.id.clone())
        .collect();
    Ok(AndroidEnvironment {
        accessibility_services,
        foreign_accessibility,
        adb_enabled: r.adb_enabled,
        development_settings_enabled: r.development_settings_enabled,
        shield_active: r.shield_active,
        overlay_hiding_supported: r.overlay_hiding_supported,
        obscured_touches: r.obscured_touches,
        sdk_int: r.sdk_int,
        source: "android".to_owned(),
    })
}

/// Current environment report, or `None` off Android / on bridge failure.
pub fn report() -> Option<AndroidEnvironment> {
    imp::report()
}

/// Engage (`true`) or release (`false`) the assessment shield. A no-op
/// success off Android.
pub fn set_shield(on: bool) -> Result<(), String> {
    imp::set_shield(on)
}

/// Obscured touches since the last call; resets the counter. Zero off
/// Android or on bridge failure.
pub fn take_obscured_touches() -> u32 {
    imp::take_obscured_touches()
}

#[cfg(target_os = "android")]
mod imp {
    use super::{from_json, AndroidEnvironment};
    use jni::objects::{JString, JValue};

    pub fn report() -> Option<AndroidEnvironment> {
        let json = crate::av_permissions::jni::with_activity_class(|env, class| {
            let response = JString::from(
                env.call_static_method(class, "environmentReport", "()Ljava/lang/String;", &[])?
                    .l()?,
            );
            let s: String = env.get_string(&response)?.into();
            Ok(s)
        })
        .map_err(|e| log::warn!(target: "sentinel", "android environment bridge: {e}"))
        .ok()?;
        from_json(&json)
            .map_err(|e| log::warn!(target: "sentinel", "{e}"))
            .ok()
    }

    pub fn set_shield(on: bool) -> Result<(), String> {
        crate::av_permissions::jni::with_activity_class(|env, class| {
            env.call_static_method(
                class,
                "setAssessmentShield",
                "(Z)V",
                &[JValue::Bool(on as u8)],
            )
            .map(|_| ())
        })
    }

    pub fn take_obscured_touches() -> u32 {
        crate::av_permissions::jni::with_activity_class(|env, class| {
            env.call_static_method(class, "takeObscuredTouches", "()I", &[])
                .and_then(|v| v.i())
        })
        .map_err(|e| log::warn!(target: "sentinel", "obscured-touch bridge: {e}"))
        .ok()
        .map(|n| n.max(0) as u32)
        .unwrap_or(0)
    }
}

#[cfg(not(target_os = "android"))]
mod imp {
    use super::AndroidEnvironment;

    pub fn report() -> Option<AndroidEnvironment> {
        None
    }

    pub fn set_shield(_on: bool) -> Result<(), String> {
        Ok(())
    }

    pub fn take_obscured_touches() -> u32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"{
        "accessibility_services": [
            {"id": "com.google.android.marvin.talkback/.TalkBackService", "system": true},
            {"id": "com.example.reader/.ScreenReader", "system": false}
        ],
        "adb_enabled": true,
        "development_settings_enabled": true,
        "shield_active": true,
        "overlay_hiding_supported": true,
        "multi_window": false,
        "obscured_touches": 3,
        "sdk_int": 36
    }"#;

    #[test]
    fn full_report_round_trips() {
        let e = from_json(FULL).unwrap();
        assert_eq!(e.accessibility_services.len(), 2);
        assert!(e.adb_enabled);
        assert!(e.development_settings_enabled);
        assert!(e.shield_active);
        assert!(e.overlay_hiding_supported);
        assert_eq!(e.obscured_touches, 3);
        assert_eq!(e.sdk_int, 36);
        assert_eq!(e.source, "android");
    }

    #[test]
    fn foreign_accessibility_excludes_system_services() {
        let e = from_json(FULL).unwrap();
        assert_eq!(
            e.foreign_accessibility,
            vec!["com.example.reader/.ScreenReader".to_owned()]
        );
    }

    #[test]
    fn missing_keys_degrade_to_clean() {
        let e = from_json("{}").unwrap();
        assert!(e.accessibility_services.is_empty());
        assert!(e.foreign_accessibility.is_empty());
        assert!(!e.adb_enabled);
        assert!(!e.development_settings_enabled);
        assert!(!e.shield_active);
        assert!(!e.overlay_hiding_supported);
        assert_eq!(e.obscured_touches, 0);
        assert_eq!(e.sdk_int, 0);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(from_json("not json").is_err());
    }

    #[test]
    fn unknown_keys_are_ignored_and_empty_ids_dropped() {
        let e = from_json(
            r#"{"accessibility_services":[{"id":"","system":false},{"system":true}],"future_key":1}"#,
        )
        .unwrap();
        assert!(e.accessibility_services.is_empty());
        assert!(e.foreign_accessibility.is_empty());
    }

    #[test]
    fn only_system_services_means_no_foreign_entries() {
        let e =
            from_json(r#"{"accessibility_services":[{"id":"com.android.a/.X","system":true}]}"#)
                .unwrap();
        assert_eq!(e.accessibility_services.len(), 1);
        assert!(e.foreign_accessibility.is_empty());
    }

    #[test]
    fn serializes_with_snake_case_fields() {
        let v = serde_json::to_value(from_json(FULL).unwrap()).unwrap();
        assert_eq!(
            v["foreign_accessibility"][0],
            "com.example.reader/.ScreenReader"
        );
        assert_eq!(v["adb_enabled"], true);
        assert_eq!(v["development_settings_enabled"], true);
        assert_eq!(v["shield_active"], true);
        assert_eq!(v["overlay_hiding_supported"], true);
        assert_eq!(v["obscured_touches"], 3);
        assert_eq!(v["sdk_int"], 36);
        assert_eq!(v["source"], "android");
        assert_eq!(v["accessibility_services"][0]["system"], true);
    }

    #[test]
    fn off_android_probes_are_inert() {
        #[cfg(not(target_os = "android"))]
        {
            assert!(report().is_none());
            assert_eq!(set_shield(true), Ok(()));
            assert_eq!(take_obscured_touches(), 0);
        }
    }
}
