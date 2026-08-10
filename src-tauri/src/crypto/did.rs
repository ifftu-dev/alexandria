//! DID primitives.
//!
//! The pure ones — `did:key` derivation, parsing, self-resolution, and the
//! registry types — live in `alexandria-verify` so that anything verifying a
//! credential can use them without linking this application. The two that need
//! the database stay here, in [`key_registry`](super::key_registry).
//!
//! This module re-exports both halves so callers see one coherent surface.

pub use alexandria_verify::did::*;

pub use super::key_registry::{resolve_key_at, rotate_key, KeyRegistryError};
