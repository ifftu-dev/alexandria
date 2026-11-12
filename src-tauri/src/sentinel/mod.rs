//! Sentinel — backend ML for the Alexandria anti-cheat system.
//!
//! - `features` — paste-classifier feature extractor (12-dim)
//! - `paste_classifier` — frozen ONNX inference via tract
//! - `global_hotkeys` — listen-only system-wide modifier-combo monitor
//!   (desktop; phantom-hotkey tell for overlay tools; records combos only)
//! - `hidden_overlay` — capture-excluded / override-redirect window scan
//!   (desktop; the Cluely-class overlay tell)
//! - `keystroke_ae` — per-user autoencoder, candle backprop
//! - `liveness` — MiniFASNetV2 presentation-attack detector via tract
//!   (photo / replay / rendered face in front of any camera)
//! - `mouse_cnn` — reservoir-style trajectory CNN, candle dense head
//! - `face_detect` — YuNet face detector (5 landmarks) via tract, run at
//!   the frame's own resolution
//! - `camera_budget` — per-profile camera budgets and the device
//!   benchmark that checks them
//! - `camera_pipeline` — camera tick scheduling: slow-cadence detection,
//!   landmark tracking in between, liveness on trigger
//! - `inference_worker` — dedicated inference thread, one pending frame
//!   per lane, newer frames replace waiting ones
//! - `gaze` — head-pose / second-device detection + per-user
//!   calibration MLP (candle)
//! - `processes` — running-process watchlist (interview-cheat overlays,
//!   AI clients, remote-desktop hosts, virtual cameras, VM guest agents)
//! - `prior_blob` — labeled-samples blob shape and validation for the
//!   encrypted holdout set
//! - `types` — shared input shapes (keystroke events, mouse points,
//!   digraphs, camera frames) mirrored from the legacy TS structs
//! - `attestation` — device attestation (App Attest / Play Integrity) capture
//!   and on-device verification behind the `device_attested` assurance rung
//! - `android_environment` — Android assessment shield (FLAG_SECURE +
//!   hide-overlay-windows), obscured-touch counter, accessibility / ADB report
//! - `display_topology` — monitor count, external / mirrored display,
//!   mobile split-screen (native per-OS probes)
//! - `scoring` — window scores, flags and session fusion, re-exported from
//!   `alexandria_studio::integrity` (Tauri-free)
//! - `consistency` — longitudinal consistency of an attempt with the
//!   learner's practice, lessons and earlier attempts on the skill
//! - `snapshots` — the integrity snapshot chain: validation, commitment,
//!   severity, status, and flags the backend records itself
//! - `corpus_capture` — opt-in capture of committed windows for the
//!   measurement corpus; off unless a maintainer sets the capture env var
//! - `trust_profile` — the session's locked trust profile: selection at
//!   start (attestation, capabilities, benchmark, floor, consent), degradation
//!   flags, and the policy floor judged before the first item
//! - `evidence` — learner-consented retention of appeal evidence for
//!   flagged sessions; nothing is persisted without an explicit yes
//! - `verification` — verification results (probe outcomes) folded into a
//!   session's commitment chain, and the summary the credential reads
//!
//! Replaces `src/utils/sentinel/*.ts`. The frontend now buffers raw
//! events and sends them across IPC; everything else runs in this
//! crate.

pub mod active_app;
pub mod android_environment;
pub mod attestation;
pub mod camera_budget;
pub mod camera_pipeline;
pub mod consistency;
pub mod corpus_capture;
pub mod display_topology;
pub mod evidence;
pub mod face_detect;
pub mod features;
pub mod gaze;
pub mod global_hotkeys;
pub mod hidden_overlay;
pub mod inference_worker;
pub mod keystroke_ae;
pub mod liveness;
pub mod mouse_cnn;
pub mod paste_classifier;
pub mod prior_blob;
pub mod processes;
pub mod scoring;
pub mod snapshots;
pub mod trust_profile;
pub mod types;
pub mod verification;
pub mod voice_activity;
