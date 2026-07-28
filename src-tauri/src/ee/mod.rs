// SPDX-License-Identifier: LicenseRef-IFFTU-Enterprise
//! Enterprise Edition modules (IFFTU Enterprise License — see `LICENSE.md`
//! in this directory). Compiled only under `--features ee`.
//!
//! Nothing in here may be a prerequisite for learning, being assessed, or
//! holding and verifying a credential. See `docs/enterprise-boundary.md`
//! for the three-test rubric that decides what belongs here.
//!
//! ## How core calls into this module
//!
//! Core never calls `ee::` directly except through the single
//! `#[cfg(feature = "ee")] pub mod ee;` declaration in `lib.rs`. Instead,
//! core defines a trait with a no-op default implementation, and the
//! enterprise build swaps in the real one:
//!
//! ```ignore
//! // in core (MIT), e.g. src/domain/enterprise.rs:
//! pub trait TalentIndexClient: Send + Sync {
//!     fn publish(&self, record: &TalentIndexRecord) -> Result<(), String>;
//! }
//! pub struct NullTalentIndex;
//! impl TalentIndexClient for NullTalentIndex {
//!     fn publish(&self, _: &TalentIndexRecord) -> Result<(), String> {
//!         Err("talent index requires Alexandria Enterprise".into())
//!     }
//! }
//!
//! // wiring, the only place the cfg appears outside lib.rs:
//! #[cfg(feature = "ee")]
//! let index: Arc<dyn TalentIndexClient> = Arc::new(crate::ee::talent_index::Client::new(cfg));
//! #[cfg(not(feature = "ee"))]
//! let index: Arc<dyn TalentIndexClient> = Arc::new(NullTalentIndex);
//! ```
//!
//! One seam per engine, not one per function. If an engine already has a
//! seam, extend it rather than adding a second.
//!
//! ## Current contents
//!
//! * [`entitlement_issuer`] — mints `EntitlementCredential`s with the IFFTU
//!   signing key. Verification of those credentials stays MIT, so a customer
//!   can audit what they hold without enterprise sources; only minting is
//!   restricted.
//!
//! The rest of the enterprise surface (talent index, employer products)
//! follows in Phase 2.

pub mod entitlement_issuer;
