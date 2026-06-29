//! Sentinel — backend ML for the Alexandria anti-cheat system.
//!
//! - `features` — paste-classifier feature extractor (12-dim)
//! - `paste_classifier` — frozen ONNX inference via tract
//! - `keystroke_ae` — per-user autoencoder, candle backprop
//! - `mouse_cnn` — reservoir-style trajectory CNN, candle dense head
//! - `face_detect` — YuNet face detector (5 landmarks) via tract
//! - `gaze` — head-pose / second-device detection + per-user
//!   calibration MLP (candle)
//! - `types` — shared input shapes (keystroke events, mouse points,
//!   digraphs, camera frames) mirrored from the legacy TS structs
//!
//! Replaces `src/utils/sentinel/*.ts`. The frontend now buffers raw
//! events and sends them across IPC; everything else runs in this
//! crate.

pub mod face_detect;
pub mod features;
pub mod gaze;
pub mod keystroke_ae;
pub mod mouse_cnn;
pub mod paste_classifier;
pub mod types;
