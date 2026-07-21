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
//! None. The enterprise surface is introduced in Phase 2 (talent index and
//! employer products). This module exists so the boundary — build wiring,
//! CI gates, and license carve-out — is proven and enforced before there is
//! any enterprise code to protect.

// Placeholder so the crate compiles under `--features ee` with no EE
// features implemented yet. Remove when the first real module lands.
#[allow(dead_code)]
pub(crate) const EE_BUILD: bool = true;
