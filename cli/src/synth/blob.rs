//! Blob format for synthetic Sentinel priors.
//!
//! Mirrors the on-wire schema in `docs/sentinel-adversarial-priors.md`:
//! one `PriorBlob` per (label, run); each blob carries `samples`, where
//! every sample is a per-keystroke time series with parallel arrays
//! (`digraphs`, `dwell_ms`, `flight_ms`, `speed_ratio`).
//!
//! Field names use snake_case in JSON (renames are explicit) to match
//! the existing TS consumer in `src/utils/sentinel/`.

use serde::{Deserialize, Serialize};

/// Bump when the on-disk format breaks compatibility with consumers.
pub const SCHEMA_VERSION: u32 = 1;

/// Generator code version. Bump when distributions change so retrained
/// models can be tied back to the exact code that produced them.
pub const SYNTH_VERSION: &str = "v2";

/// Top-level blob — one published artifact per (label, run).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorBlob {
    pub schema_version: u32,
    pub model_kind: String,
    pub label: String,
    pub synth_seed: u64,
    pub synth_version: String,
    pub notes: String,
    pub samples: Vec<KeystrokeSample>,
}

/// One typing sequence — e.g. one "session" of a bot or one paragraph
/// of a synthetic human. Lengths are intentionally variable so the
/// classifier can't shortcut on sequence length.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeystrokeSample {
    pub digraphs: Vec<String>,
    pub dwell_ms: Vec<f32>,
    pub flight_ms: Vec<f32>,
    pub speed_ratio: Vec<f32>,
}
