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

/// The certification in force, with the digest of its exact bytes. `None`
/// until a real (non-synthetic) certification is bundled; coverage
/// statements then say that no signal has a certified rate.
pub fn active_certification() -> Option<(measurement::Certification, String)> {
    None
}

/// The coverage statement with `digest`, resolved offline from the
/// statements bundled with the app under the certification in force.
pub fn resolve_coverage_statement(digest: &str) -> Option<CoverageStatement> {
    let certification = active_certification();
    resolve_coverage(digest, certification.as_ref().map(|(c, d)| (c, d.as_str())))
}
