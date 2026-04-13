//! End-to-end test scaffolding for the VC-first credential migration.
//!
//! Every test in this file is `#[ignore]`'d with a pointer to the
//! implementation PR that will un-ignore it. Together these tests
//! specify the full user-visible behaviour of the credential system
//! described in `alexandria-credential-reputation-protocol-v1.md`.
//!
//! Structure: the top-level module is the test binary; each sub-module
//! below mirrors one scenario area of the plan:
//!
//!   - `did` — §4.1, §5.1, §5.3: DID derivation, rotation, historical verify
//!   - `vc_issue_verify` — §7, §9, §10, §11, §13: issue → verify → revoke
//!   - `aggregation` — §14: weighted mean, saturating confidence, levels
//!   - `antigaming` — §15: clustering, z-score, re-issuance dedup
//!   - `anchor` — §12.3: auto-anchor credential hash to Cardano
//!   - `p2p_did_status` — two-node: DID doc + status list propagation
//!   - `p2p_vc_fetch` — pull-based credential retrieval + authority
//!   - `p2p_survival` — subject offline, PinBoard pinner serves
//!   - `pinning` — 5-tier eviction precedence
//!   - `presentation` — §18: selective disclosure + audience + nonce
//!   - `survivability` — §20.4: export verifies without Alexandria infra
//!
//! All helper code lives in `mod common` below (Rust integration tests
//! can't share `tests/common/` modules without per-file `mod common;`
//! declarations — consolidating into one binary avoids that friction).

// `#[path]` keeps the submodules inside `tests/e2e_vc/` so they don't
// leak into sibling-test namespaces. Without the attribute, Rust's
// integration-test module resolution would look for these files at
// `tests/<name>.rs`.

#[allow(dead_code)]
#[path = "e2e_vc/common.rs"]
mod common;

#[path = "e2e_vc/aggregation.rs"]
mod aggregation;
#[path = "e2e_vc/anchor.rs"]
mod anchor;
#[path = "e2e_vc/antigaming.rs"]
mod antigaming;
#[path = "e2e_vc/did.rs"]
mod did;
#[path = "e2e_vc/p2p_did_status.rs"]
mod p2p_did_status;
#[path = "e2e_vc/p2p_survival.rs"]
mod p2p_survival;
#[path = "e2e_vc/p2p_vc_fetch.rs"]
mod p2p_vc_fetch;
#[path = "e2e_vc/pinning.rs"]
mod pinning;
#[path = "e2e_vc/presentation.rs"]
mod presentation;
#[path = "e2e_vc/survivability.rs"]
mod survivability;
#[path = "e2e_vc/vc_issue_verify.rs"]
mod vc_issue_verify;
