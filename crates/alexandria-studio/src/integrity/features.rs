//! Raw per-window features a client collects, and the scores and flags the
//! backend derives from them.
//!
//! Features are measurements (counts, means, the latest native probe
//! reading, model outputs), never verdicts. Raw keystroke timings, pointer
//! coordinates and frames stay on the client; only these aggregates cross.

use serde::{Deserialize, Serialize};

// Thresholds carried over from the retired client scorer (docs/sentinel.md
// §Flagging Logic). They are unmeasured; the measurement corpus replaces them.
const GAZE_WANDER_RATIO: f64 = 0.30;
const GAZE_DOWN_GLANCE_COUNT: u32 = 3;
const GAZE_OCCLUDED_RATIO: f64 = 0.40;
const PASTE_ANOMALY_THRESHOLD: f64 = 0.95;
const PASTE_ANOMALY_CRITICAL_THRESHOLD: f64 = 0.99;
const KEYSTROKE_ANOMALY_THRESHOLD: f64 = 0.65;
const MOUSE_HUMAN_THRESHOLD: f64 = 0.5;
const LIVENESS_MIN_CHECKS: u32 = 3;
const LIVENESS_SPOOF_RATIO: f64 = 0.5;
const AI_ADVISORY_WEIGHT: f64 = 0.05;

/// Everything a client observed in one snapshot window.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowFeatures {
    pub typing: TypingFeatures,
    pub mouse: MouseFeatures,
    /// The learner's stored behavioural baseline on this device, if any.
    pub baseline: Option<BehaviouralBaseline>,
    pub focus: FocusFeatures,
    pub clipboard: ClipboardFeatures,
    /// Latest native display sample; `None` where the platform has no probe.
    pub display: Option<DisplayFeatures>,
    /// Display arrangement transitions inside the window.
    pub display_changes: u32,
    /// Android environment report; `None` off Android.
    pub android: Option<AndroidFeatures>,
    pub obscured_touches: u32,
    /// Desktop hidden-overlay scan; `None` on mobile and Wayland.
    pub overlay: Option<OverlayFeatures>,
    /// Desktop process watchlist; `None` on mobile.
    pub processes: Option<ProcessFeatures>,
    /// Global hotkey monitor; `None` when it is not running.
    pub hotkeys: Option<HotkeyFeatures>,
    /// Camera tallies; present only while the learner has opted in.
    pub camera: Option<CameraFeatures>,
    pub models: ModelFeatures,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TypingFeatures {
    pub keystrokes: u32,
    pub mean_dwell_ms: f64,
    /// Mean of the non-zero flight times; `None` when there were none.
    pub mean_flight_ms: Option<f64>,
    pub speed_wpm: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MouseFeatures {
    /// Pointer samples buffered (moves and clicks).
    pub points: u32,
    /// Move-to-move velocity samples behind the two statistics below.
    pub velocity_samples: u32,
    pub mean_velocity: f64,
    pub velocity_variance: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BehaviouralBaseline {
    pub typing_samples: u32,
    pub avg_dwell_ms: f64,
    pub avg_flight_ms: f64,
    pub speed_wpm: f64,
    pub mouse_samples: u32,
    pub avg_velocity: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FocusFeatures {
    pub tab_switches: u32,
    pub unfocused_ms: f64,
    /// Times the OS moved the foreground to another app.
    pub app_switches: u32,
    pub app_unfocused_ms: f64,
    pub environment_changed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClipboardFeatures {
    pub paste_events: u32,
    pub pasted_chars: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayFeatures {
    pub display_count: u32,
    pub external_display: bool,
    pub split_screen: bool,
    pub screen_captured: bool,
    /// The native transition counter moved between samples: a split view or
    /// mirror began and ended inside the window.
    pub native_toggle: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AndroidFeatures {
    pub foreign_accessibility_services: u32,
    pub adb_enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OverlayFeatures {
    pub suspicious: u32,
    pub scanned: u32,
    pub allowlisted: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessFeatures {
    /// Distinct watchlist categories seen running.
    pub categories: Vec<String>,
    pub watched: u32,
    pub scanned: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HotkeyFeatures {
    /// Combos the OS saw this window.
    pub native: u32,
    /// Of those, combos the webview never received while focused.
    pub phantom: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CameraFeatures {
    /// Whether the camera device looks virtual; `None` until it is known.
    pub device_virtual: Option<bool>,
    pub face_present: Option<bool>,
    pub face_count: Option<u32>,
    pub face_consistency: Option<f64>,
    pub face_similarity: Option<f64>,
    pub face_match: Option<bool>,
    pub consecutive_no_face: u32,
    pub face_checks: u32,
    pub face_absent_checks: u32,
    pub gaze_checks: u32,
    pub gaze_offscreen_checks: u32,
    pub gaze_occluded_checks: u32,
    pub gaze_down_glances: u32,
    pub liveness_checks: u32,
    pub liveness_spoof_checks: u32,
    /// Mean real-face probability over the window's liveness ticks.
    pub liveness_real_prob: Option<f64>,
}

/// Outputs of the on-device models, each absent when the model did not run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelFeatures {
    pub paste_anomaly: Option<f64>,
    pub keystroke_anomaly: Option<f64>,
    pub mouse_human_prob: Option<f64>,
    /// The learner's advisory-AI setting; adds small weights to the score.
    pub advisory_enabled: bool,
    pub paste_classifier_enabled: bool,
}

/// Scores derived for one window. `integrity` is descriptive: it no longer
/// decides a session's outcome.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WindowScores {
    pub integrity: f64,
    pub consistency: f64,
    pub typing: f64,
    pub mouse: f64,
    pub human: f64,
    pub tab: f64,
    pub paste: f64,
    pub camera: Option<f64>,
    pub ai_paste_anomaly: Option<f64>,
    pub gaze_offscreen_ratio: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WindowAssessment {
    pub scores: WindowScores,
    /// Distinct, in catalog order.
    pub flags: Vec<String>,
}

impl WindowFeatures {
    /// Refuses values no honest collector produces, before anything is
    /// derived or written.
    pub fn validate(&self) -> Result<(), String> {
        let measures = [
            self.typing.mean_dwell_ms,
            self.typing.mean_flight_ms.unwrap_or(0.0),
            self.typing.speed_wpm,
            self.mouse.mean_velocity,
            self.mouse.velocity_variance,
            self.focus.unfocused_ms,
            self.focus.app_unfocused_ms,
        ]
        .into_iter()
        .chain(
            self.baseline
                .iter()
                .flat_map(|b| [b.avg_dwell_ms, b.avg_flight_ms, b.speed_wpm, b.avg_velocity]),
        );
        for value in measures {
            if !value.is_finite() || value < 0.0 {
                return Err("window features must be finite, non-negative measurements".into());
            }
        }
        let camera = self.camera.as_ref();
        let probabilities = [
            self.models.paste_anomaly,
            self.models.keystroke_anomaly,
            self.models.mouse_human_prob,
            camera.and_then(|c| c.face_consistency),
            camera.and_then(|c| c.face_similarity),
            camera.and_then(|c| c.liveness_real_prob),
        ];
        for value in probabilities.into_iter().flatten() {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(
                    "window probabilities must be finite values between zero and one".into(),
                );
            }
        }
        let mut subsets = vec![(self.mouse.velocity_samples, self.mouse.points)];
        if let Some(c) = camera {
            subsets.extend([
                (c.face_absent_checks, c.face_checks),
                (c.gaze_offscreen_checks, c.gaze_checks),
                (c.gaze_occluded_checks, c.gaze_checks),
                (c.gaze_down_glances, c.gaze_checks),
                (c.liveness_spoof_checks, c.liveness_checks),
            ]);
        }
        if subsets.into_iter().any(|(part, whole)| part > whole) {
            return Err("window counts are inconsistent".into());
        }
        Ok(())
    }
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn typing_consistency(f: &WindowFeatures) -> f64 {
    let t = &f.typing;
    if t.keystrokes < 5 {
        return 0.5;
    }
    let Some(b) = f.baseline.as_ref().filter(|b| b.typing_samples > 10) else {
        return 0.7;
    };
    let flight = t.mean_flight_ms.unwrap_or(100.0);
    let dwell_score = (1.0 - (t.mean_dwell_ms - b.avg_dwell_ms).abs() / 100.0).max(0.0);
    let flight_score = (1.0 - (flight - b.avg_flight_ms).abs() / 200.0).max(0.0);
    let speed_score = (1.0 - (t.speed_wpm - b.speed_wpm).abs() / 50.0).max(0.0);
    (dwell_score * 0.3 + flight_score * 0.3 + speed_score * 0.4).clamp(0.0, 1.0)
}

/// Mouse consistency and whether the trajectory looks human.
fn mouse_analysis(f: &WindowFeatures) -> (f64, bool) {
    let m = &f.mouse;
    if m.points < 10 || m.velocity_samples == 0 {
        return (0.5, true);
    }
    let variance = m.velocity_variance;
    let variance_score = if variance > 0.001 && variance < 100.0 {
        0.9
    } else {
        0.3
    };
    let human = variance > 0.0001 && m.mean_velocity < 50.0;
    let consistency = match f.baseline.as_ref().filter(|b| b.mouse_samples > 10) {
        Some(b) => {
            (1.0 - (m.mean_velocity - b.avg_velocity).abs() / 10.0).max(0.0) * 0.6
                + variance_score * 0.4
        }
        None => variance_score,
    };
    (consistency.clamp(0.0, 1.0), human)
}

/// Scores and flags for one window. Pure: the same features always give the
/// same assessment, which is what lets the offline evaluator replay a corpus.
pub fn derive_window(f: &WindowFeatures) -> WindowAssessment {
    let typing = typing_consistency(f);
    let (mouse, human) = mouse_analysis(f);
    let camera = f.camera.as_ref();
    let mut flags: Vec<&str> = Vec::new();

    if let Some(d) = &f.display {
        if d.external_display {
            flags.push("external_display");
        }
        if d.split_screen || d.native_toggle {
            flags.push("split_screen");
        }
        if d.screen_captured {
            flags.push("screen_captured");
        }
    }
    if f.display_changes > 0 {
        flags.push("display_change");
    }
    if let Some(a) = &f.android {
        if a.foreign_accessibility_services > 0 {
            flags.push("foreign_accessibility_service");
        }
        if a.adb_enabled {
            flags.push("debug_bridge_enabled");
        }
    }
    if f.obscured_touches > 0 {
        flags.push("obscured_touch");
    }
    if f.overlay.as_ref().is_some_and(|o| o.suspicious > 0) {
        flags.push("hidden_overlay");
    }
    if camera.is_some_and(|c| c.device_virtual == Some(true)) {
        flags.push("virtual_camera");
    }
    if let Some(p) = &f.processes {
        let has = |c: &str| p.categories.iter().any(|x| x == c);
        if has("interview_cheat") {
            flags.push("cheat_tool_process");
        }
        if has("remote_desktop") || has("virtual_camera") || has("virtual_machine") {
            flags.push("unauthorized_process");
        }
        if has("ai_assistant") {
            flags.push("ai_assistant_running");
        }
        if has("screen_share") {
            flags.push("screen_share_running");
        }
    }
    let phantom = f.hotkeys.as_ref().map_or(0, |h| h.phantom);
    if phantom >= 3 {
        flags.push("phantom_hotkey_repeated");
    }
    if phantom >= 1 {
        flags.push("phantom_hotkey");
    }

    let keystroke_anomaly = f.models.keystroke_anomaly.map(round3);
    let mouse_human_prob = f.models.mouse_human_prob.map(round3);
    let paste_anomaly = f.models.paste_anomaly.map(round3);
    if keystroke_anomaly.is_some_and(|a| a >= KEYSTROKE_ANOMALY_THRESHOLD) {
        flags.push("behavior_shift");
    }
    if mouse_human_prob.is_some_and(|p| p < MOUSE_HUMAN_THRESHOLD) {
        flags.push("bot_suspected");
    }
    let face_similarity = camera.and_then(|c| c.face_similarity).map(round3);
    if let Some(c) = camera {
        if c.face_similarity.is_some()
            && c.face_match == Some(false)
            && c.face_present == Some(true)
        {
            flags.push("face_mismatch");
        }
    }
    if let Some(raw) = f.models.paste_anomaly {
        if raw >= PASTE_ANOMALY_CRITICAL_THRESHOLD {
            flags.push("paste_classifier_critical");
        } else if raw >= PASTE_ANOMALY_THRESHOLD {
            flags.push("paste_classifier_anomaly");
        }
    }

    // Descriptive composite: the weighted average the client used to compute.
    let tab = (1.0 - f64::from(f.focus.tab_switches) / 15.0).max(0.0);
    let paste = (1.0 - f64::from(f.clipboard.pasted_chars) / 1000.0).max(0.0);
    let mut weighted = typing * 0.20 + mouse * 0.15 + if human { 0.15 } else { 0.0 } + tab * 0.15;
    weighted += paste * 0.10;
    let mut weights = 0.75;
    let face_consistency = camera
        .filter(|c| c.face_present.is_some())
        .and_then(|c| c.face_consistency);
    if let Some(c) = camera.filter(|c| c.face_present.is_some()) {
        let face = if c.face_present == Some(true) && c.face_count == Some(1) {
            c.face_consistency.unwrap_or(0.8)
        } else {
            0.2
        };
        weighted += face * 0.15;
        weights += 0.15;
    }
    if f.models.advisory_enabled {
        let advisory = [
            keystroke_anomaly.map(|a| 1.0 - a),
            mouse_human_prob,
            face_similarity,
            paste_anomaly
                .filter(|_| f.models.paste_classifier_enabled)
                .map(|p| 1.0 - p),
        ];
        for value in advisory.into_iter().flatten() {
            weighted += value * AI_ADVISORY_WEIGHT;
            weights += AI_ADVISORY_WEIGHT;
        }
    }
    let integrity = (weighted / weights).clamp(0.0, 1.0);
    let consistency = match camera.and_then(|c| c.face_consistency) {
        Some(face) => (typing + mouse + face) / 3.0,
        None => (typing + mouse) / 2.0,
    }
    .clamp(0.0, 1.0);

    if integrity < 0.40 {
        flags.push("low_integrity");
    }
    if consistency < 0.35 {
        flags.push("behavior_shift");
    }
    if f.focus.tab_switches > 10 {
        flags.push("tab_switching");
    }
    if f.clipboard.pasted_chars > 500 {
        flags.push("paste_detected");
    }
    if !human {
        flags.push("bot_suspected");
    }
    let mut gaze_offscreen_ratio = None;
    if let Some(c) = camera {
        if c.face_present == Some(false) {
            flags.push("no_face");
        }
        if c.face_count.is_some_and(|n| n > 1) {
            flags.push("multiple_faces");
        }
        if c.consecutive_no_face >= 5 {
            flags.push("prolonged_absence");
        }
        if c.face_checks > 0 && f64::from(c.face_absent_checks) / f64::from(c.face_checks) > 0.5 {
            flags.push("frequent_absence");
        }
        if c.gaze_checks > 0 {
            let total = f64::from(c.gaze_checks);
            let offscreen = f64::from(c.gaze_offscreen_checks) / total;
            gaze_offscreen_ratio = Some(offscreen);
            if c.gaze_down_glances >= GAZE_DOWN_GLANCE_COUNT {
                flags.push("device_glance");
            }
            if offscreen > GAZE_WANDER_RATIO {
                flags.push("gaze_wander");
            }
            if f64::from(c.gaze_occluded_checks) / total > GAZE_OCCLUDED_RATIO {
                flags.push("gaze_occluded");
            }
        }
        // A single low tick is lighting as often as an attack, so the flag
        // needs a majority of enough ticks.
        if c.liveness_checks >= LIVENESS_MIN_CHECKS
            && f64::from(c.liveness_spoof_checks) / f64::from(c.liveness_checks)
                > LIVENESS_SPOOF_RATIO
        {
            flags.push("spoof_suspected");
        }
    }
    if f.focus.app_switches > 0 {
        flags.push("app_switch");
    }

    let mut distinct: Vec<String> = Vec::with_capacity(flags.len());
    for flag in flags {
        if !distinct.iter().any(|f| f == flag) {
            distinct.push(flag.to_string());
        }
    }
    WindowAssessment {
        scores: WindowScores {
            integrity,
            consistency,
            typing,
            mouse,
            human: if human { 1.0 } else { 0.0 },
            tab,
            paste,
            camera: face_consistency,
            ai_paste_anomaly: paste_anomaly,
            gaze_offscreen_ratio,
        },
        flags: distinct,
    }
}
