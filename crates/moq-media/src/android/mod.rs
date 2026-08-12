// Copyright 2026 Alexandria Pvt. Ltd.
// Licensed under the Apache License, Version 2.0 — see LICENSE-APACHE.
//
// Original Alexandria work, not derived from n0-computer/iroh-live.
// See crates/VENDORING.md.
//! Android-specific media backends.
//!
//! Currently just camera capture over the NDK Camera2 API, which fills the gap
//! left by `nokhwa` having no Android support. Encoding still goes through the
//! shared ffmpeg path.

pub mod camera;

pub use camera::AndroidCameraSource;
