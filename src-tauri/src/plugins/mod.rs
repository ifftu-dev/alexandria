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
pub mod manifest;
pub mod registry;
pub mod verifier;
pub mod wasm_runtime;
