//! Android-specific media backends.
//!
//! Currently just camera capture over the NDK Camera2 API, which fills the gap
//! left by `nokhwa` having no Android support. Encoding still goes through the
//! shared ffmpeg path.

pub mod camera;

pub use camera::AndroidCameraSource;
