//! Verifiable credentials: everything about a credential that touches the
//! database, the keystore or the network.
//!
//! [`crate::domain::vc`] holds the pure types, signing and ids. This module
//! owns the rest, and the credential commands are thin wrappers over it:
//!
//! - [`issue`]: issue a credential, gated on its integrity session.
//! - [`evidence`]: the session's attestation evidence, bound to the
//!   credential when the platform allows.
//! - [`issuer`]: the [`Issuer`], a signing key and its DID as one value.
//! - [`status_list`]: revocation status lists and their publication.
//! - [`lifecycle`]: revoke, suspend and reinstate.
//! - [`store`]: read stored credentials; decode untrusted payloads.
//! - [`receive`]: verify and store a credential somebody else issued.
//! - [`bundle`]: survivability bundles and offline verification.
//! - [`trust`]: what a credential view states about its trust.
//! - [`auto`]: self-issued credentials for observed completion mints.
//! - [`collect`]: credentials organisations issued through Alexandria Cloud.
//!
//! Every function here returns [`CredentialError`]; commands convert it to
//! a string at their boundary.

pub mod auto;
pub mod bundle;
pub mod collect;
mod error;
pub mod evidence;
pub mod issue;
pub mod issuer;
pub mod lifecycle;
pub mod receive;
pub mod status_list;
pub mod store;
pub mod trust;

pub use error::{CredentialError, PolicyRefusal, StatusChange, SupersessionRequires};
pub use evidence::{
    BindRequest, EvidenceBinding, EvidenceSource, PreparedEvidence, RecordingAttester,
};
pub use issue::{issue, issue_with, IssuancePolicy, IssueCredentialRequest, Issued};
pub use issuer::Issuer;

#[cfg(test)]
pub(crate) mod fixtures;
