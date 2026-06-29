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

/// A raw camera frame forwarded from the frontend for backend face /
/// gaze inference. `rgba` is row-major RGBA8 (canvas `ImageData`
/// layout). Frames are processed in-place and **never persisted** —
/// only derived scores leave this crate.
#[derive(Debug, Clone, Deserialize)]
pub struct FaceFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// One detected face: bounding box `[x, y, w, h]` and 5 landmarks, all
/// in original-frame pixel coordinates. YuNet landmark order is
/// `[right_eye, left_eye, nose_tip, right_mouth, left_mouth]`.
#[derive(Debug, Clone, Serialize)]
pub struct FaceDetection {
    pub bbox: [f32; 4],
    #[serde(rename = "landmarks5")]
    pub landmarks5: [[f32; 2]; 5],
    pub score: f32,
}

/// Indices into `FaceDetection::landmarks5`.
pub const LM_RIGHT_EYE: usize = 0;
pub const LM_LEFT_EYE: usize = 1;
pub const LM_NOSE: usize = 2;
pub const LM_RIGHT_MOUTH: usize = 3;
pub const LM_LEFT_MOUTH: usize = 4;

/// Gaze / head-orientation estimate for one frame. `screen_x/y` are the
/// calibrated point-of-regard in normalized screen space `[0,1]`,
/// present only when a per-user calibration model exists. `on_screen`
/// is the actionable bit the integrity layer consumes.
#[derive(Debug, Clone, Serialize)]
pub struct GazeEstimate {
    pub yaw: f32,
    pub pitch: f32,
    #[serde(rename = "screenX")]
    pub screen_x: Option<f32>,
    #[serde(rename = "screenY")]
    pub screen_y: Option<f32>,
    #[serde(rename = "onScreen")]
    pub on_screen: bool,
    pub occluded: bool,
    pub confidence: f32,
}

/// One labeled calibration sample captured by the wizard's 9-point gaze
/// step: pose proxies + iris offsets paired with the known screen
/// target the user was looking at.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct GazeCalibSample {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    #[serde(rename = "irisDx")]
    pub iris_dx: f32,
    #[serde(rename = "irisDy")]
    pub iris_dy: f32,
    #[serde(rename = "targetX")]
    pub target_x: f32,
    #[serde(rename = "targetY")]
    pub target_y: f32,
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
