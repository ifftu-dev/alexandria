//! Community plugin system runtime.
//!
//! Phase 1 — local-file install of iframe-sandboxed interactive plugins.
//! See `/Users/hack/.claude/plans/prancy-bubbling-grove.md`.
//!
//! Public surface:
//! - [`manifest`] — parse + validate the signed `manifest.json`.
//! - [`verifier`] — verify Ed25519 signatures against the author's DID-Key
//!   and compute the content-addressed `plugin_cid`.
//! - [`registry`] — on-disk bundle store + SQLite-backed install/list/uninstall
//!   and per-plugin capability grants.

pub mod asset_protocol;
pub mod manifest;
pub mod registry;
pub mod verifier;
