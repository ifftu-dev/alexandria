//! Shared input types for backend Sentinel ML.
//!
//! Mirror the legacy TS shapes one-for-one so the frontend can keep
//! sending the same JSON. The frontend buffers raw events, batches them
//! per snapshot, and sends them across IPC; all feature extraction,
//! inference, and training happens in this crate.

use serde::{Deserialize, Serialize};

/// One keystroke event captured by the frontend. Mirrors
/// `paste-features.ts::KeystrokeEvent`. `key` is always `'char'` for
/// printable characters — raw key identity is never persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeystrokeEvent {
    pub key: String,
    #[serde(rename = "dwellMs")]
    pub dwell_ms: f32,
    #[serde(rename = "flightMs")]
    pub flight_ms: f32,
}

/// One mouse trajectory point. Mirrors
/// `mouse-trajectory-cnn.ts::MousePoint`. `x`/`y` are device-local
/// coordinates that stay inside the in-memory buffer — only deltas
/// feed the CNN.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MousePoint {
    pub x: f32,
    pub y: f32,
    pub t: f32,
}

/// One digraph feature record consumed by the keystroke autoencoder.
/// Mirrors `keystroke-autoencoder.ts::DigraphFeatures`. The frontend
/// can either send raw `KeystrokeEvent`s and have the backend extract
/// digraphs, or send pre-extracted digraphs directly.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DigraphFeatures {
    #[serde(rename = "dwellMs1")]
    pub dwell_ms1: f32,
    #[serde(rename = "dwellMs2")]
    pub dwell_ms2: f32,
    #[serde(rename = "flightMs")]
    pub flight_ms: f32,
    #[serde(rename = "speedRatio")]
    pub speed_ratio: f32,
}
