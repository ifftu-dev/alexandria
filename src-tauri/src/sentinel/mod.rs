//! Sentinel — backend ML for the Alexandria anti-cheat system.
//!
//! - `features` — paste-classifier feature extractor (12-dim)
//! - `paste_classifier` — frozen ONNX inference via tract
//! - `global_hotkeys` — listen-only system-wide modifier-combo monitor
//!   (desktop; phantom-hotkey tell for overlay tools; records combos only)
//! - `hidden_overlay` — capture-excluded / override-redirect window scan
//!   (desktop; the Cluely-class overlay tell)
//! - `keystroke_ae` — per-user autoencoder, candle backprop
//! - `mouse_cnn` — reservoir-style trajectory CNN, candle dense head
//! - `face_detect` — YuNet face detector (5 landmarks) via tract
//! - `gaze` — head-pose / second-device detection + per-user
//!   calibration MLP (candle)
//! - `processes` — running-process watchlist (interview-cheat overlays,
//!   AI clients, remote-desktop hosts, virtual cameras, VM guest agents)
//! - `prior_blob` — labeled-samples blob shape and validation for the
//!   encrypted holdout set
//! - `types` — shared input shapes (keystroke events, mouse points,
//!   digraphs, camera frames) mirrored from the legacy TS structs
//! - `android_environment` — Android assessment shield (FLAG_SECURE +
//!   hide-overlay-windows), obscured-touch counter, accessibility / ADB report
//! - `display_topology` — monitor count, external / mirrored display,
//!   mobile split-screen (native per-OS probes)
//! - `evidence` — learner-consented retention of appeal evidence for
//!   flagged sessions; nothing is persisted without an explicit yes
//!
//! Replaces `src/utils/sentinel/*.ts`. The frontend now buffers raw
//! events and sends them across IPC; everything else runs in this
//! crate.

pub mod active_app;
pub mod android_environment;
pub mod display_topology;
pub mod evidence;
pub mod face_detect;
pub mod features;
pub mod gaze;
pub mod global_hotkeys;
pub mod hidden_overlay;
pub mod keystroke_ae;
pub mod mouse_cnn;
pub mod paste_classifier;
pub mod prior_blob;
pub mod processes;
pub mod types;
