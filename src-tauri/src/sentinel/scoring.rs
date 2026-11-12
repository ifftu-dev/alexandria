//! Integrity scoring and fusion.
//!
//! The implementation lives in `alexandria-studio` behind the Tauri-free
//! boundary (ADR-0004), so the app, the offline evaluator and tests derive
//! scores, flags and session decisions in exactly one way.

pub use alexandria_studio::integrity::*;

/// The ratio manifest sessions are scored against. No signal has been
/// measured yet, so every signal is advisory and contributes zero.
pub fn active_manifest() -> RatioManifest {
    RatioManifest::uncertified()
}
