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
