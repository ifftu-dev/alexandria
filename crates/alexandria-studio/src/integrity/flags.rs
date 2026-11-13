//! The flag catalog: severity, the independent channel each signal belongs
//! to, and the tamper flags only the backend records.

/// How serious a flag is on its own. Severity no longer decides a session's
/// outcome by counting (see [`super::decide`]); it still separates context
/// (`Info`) from flags that send a session to review.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }
}

/// Flags recorded by the backend, never sent by a client: an AI feature or an
/// external assistant reached the backend while an attempt was open. These
/// are the only flags that suspend a session.
pub const TAMPER_FLAGS: &[&str] = &["in_app_assistant_during_attempt", "mcp_call_during_attempt"];

pub fn is_tamper_flag(flag: &str) -> bool {
    TAMPER_FLAGS.contains(&flag)
}

/// Severity per flag (docs/sentinel.md §Flagging Logic). Unknown flags are
/// `Info`, so client/server version skew never escalates a session.
pub fn severity(flag: &str) -> Severity {
    match flag {
        "bot_suspected"
        | "face_mismatch"
        | "paste_classifier_critical"
        | "device_glance"
        | "split_screen"
        | "screen_captured"
        | "obscured_touch"
        | "hidden_overlay"
        | "cheat_tool_process"
        | "phantom_hotkey_repeated"
        | "in_app_assistant_during_attempt"
        | "mcp_call_during_attempt" => Severity::Critical,
        "behavior_shift"
        | "paste_detected"
        | "multiple_faces"
        | "prolonged_absence"
        | "low_integrity"
        | "paste_classifier_anomaly"
        | "gaze_wander"
        | "gaze_occluded"
        | "app_switch"
        | "display_change"
        | "foreign_accessibility_service"
        | "debug_bridge_enabled"
        | "virtual_camera"
        | "spoof_suspected"
        | "unauthorized_process"
        | "phantom_hotkey"
        | "camera_degraded" => Severity::Warning,
        // Includes the response-process signals (`RESPONSE_PROCESS_SIGNALS`):
        // unmeasured, they reach a decision only through a certified ratio.
        _ => Severity::Info,
    }
}

/// The independent evidence channel a signal belongs to. Two signals in one
/// channel are one piece of evidence, so the decision rule counts channels,
/// not flags. An unknown signal is its own channel.
pub fn channel(signal: &str) -> &str {
    match signal {
        "behavior_shift" => "typing",
        "bot_suspected" => "pointer",
        "paste_detected" | "paste_classifier_anomaly" | "paste_classifier_critical" => "clipboard",
        "tab_switching" | "app_switch" => "focus",
        "split_screen" | "screen_captured" | "display_change" | "external_display" => "display",
        "obscured_touch" | "foreign_accessibility_service" | "debug_bridge_enabled" => {
            "device_environment"
        }
        "hidden_overlay" => "overlay",
        "cheat_tool_process"
        | "unauthorized_process"
        | "ai_assistant_running"
        | "screen_share_running" => "process",
        "phantom_hotkey" | "phantom_hotkey_repeated" => "hotkey",
        "no_face" | "multiple_faces" | "prolonged_absence" | "frequent_absence" => {
            "camera_presence"
        }
        // The locked profile runs the camera and a window had none.
        "camera_degraded" => "camera_coverage",
        "face_mismatch" | "virtual_camera" | "spoof_suspected" => "camera_identity",
        // A gaze-down just before a response is gaze evidence, not a second
        // independent channel.
        "device_glance" | "gaze_wander" | "gaze_occluded" | "gaze_down_then_response" => "gaze",
        "response_below_reading_floor"
        | "response_relay_lag"
        | "response_idle_burst"
        | "answer_change_after_idle" => "response_process",
        "voice_then_response" => "voice",
        // Derived from the other scores, so never independent of them.
        "low_integrity" => "composite",
        "in_app_assistant_during_attempt" | "mcp_call_during_attempt" => "assistant",
        // Derived from the learner's history, not from the session.
        "longitudinal_divergence" => "longitudinal",
        other => other,
    }
}
