//! Android TutoringManager — stub for now.
//!
//! Android does not yet have video support (no VideoToolbox equivalent).
//! This stub provides the `TutoringManager` struct expected by `AppState`
//! in `lib.rs` so the app compiles and runs on Android with the tutoring
//! commands returning "unsupported" errors via `tutoring_stubs.rs`.

use std::sync::Arc;
use tokio::sync::Mutex;

/// Stub tutoring manager for Android.
///
/// Thread-safe via `Arc<Mutex<>>`. Stored in Tauri `AppState`.
pub struct TutoringManager {
    _inner: Arc<Mutex<()>>,
}

impl TutoringManager {
    pub fn new() -> Self {
        Self {
            _inner: Arc::new(Mutex::new(())),
        }
    }
}
