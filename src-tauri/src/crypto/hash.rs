//! Hash primitives, re-exported from `alexandria-verify`.
//!
//! They live there because credential ids are derived from them, so anything
//! verifying a credential needs them without linking this application.

pub use alexandria_verify::hash::*;
