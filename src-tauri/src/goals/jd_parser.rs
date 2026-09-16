//! On-device job-description → candidate-skill matcher.
//!
//! The implementation lives in `alexandria-studio` so that the app and the
//! assistant broker, which answers `resolve_goal` for a connected assistant,
//! match text against the taxonomy in exactly one way. Its tests live beside
//! it there.

pub use alexandria_studio::jd_parser::{extract_skills, Candidate, SkillEntry};
