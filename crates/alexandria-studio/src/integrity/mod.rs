//! Integrity scoring and fusion for Sentinel sessions.
//!
//! The client is a collector: once per snapshot window it submits raw
//! [`WindowFeatures`]. This module turns a window into scores and flags
//! ([`derive_window`]), and a session's committed flags into a decision
//! ([`decide`]) by accumulating per-signal log-likelihood ratios from a
//! [`RatioManifest`]. It has no Tauri or database dependency, so the app, the
//! offline evaluator and tests share one implementation (ADR-0004).
//!
//! A signal without a measured ratio contributes zero and is reported as
//! advisory. A single flag puts a session in review; a credential-affecting
//! outcome needs two independent channels over threshold or a failed
//! verification. Only backend-recorded tamper flags suspend.

mod consistency;
mod features;
mod flags;
mod fusion;
pub mod measurement;
mod response;

pub use consistency::{
    assess as assess_consistency, Consistency, ConsistencyVerdict, EvidenceSource, History,
    Observation, DIVERGENCE_SIGNAL, LOOKBACK_HOURS,
};
pub use features::{
    derive_window, AndroidFeatures, BehaviouralBaseline, CameraFeatures, ClipboardFeatures,
    DisplayFeatures, FocusFeatures, HotkeyFeatures, ModelFeatures, MouseFeatures, OverlayFeatures,
    ProcessFeatures, TypingFeatures, WindowAssessment, WindowFeatures, WindowScores,
};
pub use flags::{channel, is_tamper_flag, severity, Severity, TAMPER_FLAGS};
pub use fusion::{
    decide, ChannelEvidence, Decision, FusionSummary, ProfileRatios, RatioManifest,
    SessionEvidence, SessionStatus, DEFAULT_PROFILE, UNCERTIFIED_MANIFEST_VERSION,
};
pub use response::{
    reading_floor_ms, BaselineItem, BaselineSource, InputKind, ItemResponse, ResponseBaseline,
    ResponseFeatures, RESPONSE_PROCESS_SIGNALS,
};
