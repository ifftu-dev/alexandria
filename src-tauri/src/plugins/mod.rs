//! Community plugin system runtime.
//!
//! Phase 1 — local-file install of iframe-sandboxed interactive plugins.
//! Phase 2 — deterministic Wasmtime grader runtime for credential-bearing
//! assessments.
//!
//! Public surface:
//! - [`manifest`] — parse + validate the signed `manifest.json`.
//! - [`verifier`] — verify Ed25519 signatures against the author's DID-Key
//!   and compute the content-addressed `plugin_cid`.
//! - [`registry`] — on-disk bundle store + SQLite-backed install/list/uninstall
//!   and per-plugin capability grants.
//! - [`asset_protocol`] — `plugin://` URI scheme handler with per-plugin CSP.
//! - [`wasm_runtime`] — Wasmtime-backed grader runtime configured for
//!   reproducible execution. Phase 2.

pub mod asset_protocol;
pub mod attestation;
pub mod builtins;
pub mod catalog;
pub mod irl_review;
pub mod manifest;
pub mod registry;
pub mod verifier;
// The grader runtime runs wherever Cranelift can emit native code — desktop
// and Android. iOS is the sole exception: the platform forbids JIT, so the
// module is omitted there (`grader` cfg, emitted by build.rs) and the IPC
// layer exposes a stub that returns a `GraderUnavailable` error. Native
// built-in graders (MCQ, essay) continue to work everywhere, iOS included.
// Wasmtime itself supports aarch64-linux-android directly.
#[cfg(grader)]
pub mod wasm_runtime;
