//! The grade envelope — ABI v1's data contract.
//!
//! Deliberately separate from [`super::wasm_runtime`], which is
//! `#[cfg(desktop)]` because Wasmtime has no iOS or Android target. The
//! *contract* is platform-independent even where the wasm *engine* is not:
//! native graders on mobile (MCQ today) speak the same envelope, so a score
//! means the same thing regardless of which implementation produced it.
//!
//! `wasm_runtime` re-exports both types, so desktop call sites can keep
//! importing them from there.

use serde::{Deserialize, Serialize};

/// JSON envelope passed to a grader.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradeInput {
    pub version: String,
    pub content: serde_json::Value,
    pub submission: serde_json::Value,
}

/// JSON envelope returned by a grader. `score` is a fraction in `[0.0, 1.0]`;
/// `details` is grader-defined and treated as opaque by the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreRecord {
    pub version: String,
    pub score: f64,
    #[serde(default)]
    pub details: serde_json::Value,
}

/// Probe envelope version. Additive to ABI v1: a grader that exports
/// `alex_probe` speaks it; one that does not simply has no probes.
pub const PROBE_ENVELOPE_VERSION: &str = "1";

/// Shortest and longest time box the host accepts from a probe. A grader
/// asking for less is raised to the floor and more is cut to the ceiling, so
/// no grader can set a probe the learner cannot physically answer, or one
/// that turns verification into a second exam.
pub const PROBE_TIME_BOX_MIN_MS: u64 = 5_000;
pub const PROBE_TIME_BOX_MAX_MS: u64 = 600_000;

/// JSON envelope passed to a grader's optional `alex_probe` export: the same
/// item content and normalised submission the grade envelope carries, plus a
/// seed so a perturbation is reproducible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeInput {
    pub version: String,
    pub content: serde_json::Value,
    pub submission: serde_json::Value,
    pub seed: u64,
}

/// JSON envelope returned by `alex_probe`.
///
/// `question` and `expectation` follow the item content split: `question` may
/// be shown to the learner, `expectation` never leaves the host. The response
/// is graded by the same grader's `alex_grade` over `question ∪ expectation`
/// (expectation winning on collision, as for items), so the expectation is
/// machine-gradable by construction. A `null` question means the grader has
/// no probe for this submission. The host never interprets `question`,
/// `expectation` or `details` beyond that.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeRecord {
    pub version: String,
    #[serde(default)]
    pub question: serde_json::Value,
    #[serde(default)]
    pub expectation: serde_json::Value,
    #[serde(default)]
    pub time_box_ms: u64,
    /// Score at or above which the probe response counts as satisfied.
    #[serde(default = "default_pass_score")]
    pub pass_score: f64,
    #[serde(default)]
    pub details: serde_json::Value,
}

fn default_pass_score() -> f64 {
    1.0
}

impl ProbeRecord {
    /// Check a grader's probe output and clamp its time box. `Ok(None)` when
    /// the grader declined to probe.
    pub fn validated(mut self) -> Result<Option<Self>, String> {
        if self.version != PROBE_ENVELOPE_VERSION {
            return Err(format!(
                "probe output declared version '{}', host expects '{PROBE_ENVELOPE_VERSION}'",
                self.version
            ));
        }
        if self.question.is_null() {
            return Ok(None);
        }
        if !self.pass_score.is_finite() || self.pass_score <= 0.0 || self.pass_score > 1.0 {
            return Err(format!(
                "probe pass score {} is outside (0, 1]",
                self.pass_score
            ));
        }
        self.time_box_ms = self
            .time_box_ms
            .clamp(PROBE_TIME_BOX_MIN_MS, PROBE_TIME_BOX_MAX_MS);
        Ok(Some(self))
    }
}
