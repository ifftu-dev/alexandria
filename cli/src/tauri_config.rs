//! Shared paths to platform-specific Tauri config files.
//!
//! These are applied via `--config <path>` when invoking `cargo tauri …`
//! to ensure platform-specific features (`tutoring-video-ios`,
//! `tutoring-video-android`, `dev-seed`, etc.) get merged into the build.
//!
//! Paths are relative to the project root (where `alex` is invoked).

pub const IOS: &str = "src-tauri/tauri.ios.conf.json";
pub const ANDROID: &str = "src-tauri/tauri.android.conf.json";
