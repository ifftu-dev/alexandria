//! Verifiable credentials: everything about a credential that touches the
//! database, the keystore or the network.
//!
//! [`crate::domain::vc`] holds the pure types, signing and ids. This module
//! issues credentials and keeps their revocation status lists. The
//! credential commands are thin wrappers over it.

pub mod bundle;
pub mod issue;
pub mod issuer;
pub mod lifecycle;
pub mod status_list;
pub mod store;

pub use issue::{issue, IssuancePolicy, IssueCredentialRequest};
pub use issuer::Issuer;

#[cfg(test)]
pub(crate) mod fixtures;
