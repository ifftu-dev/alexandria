//! Verifiable credentials: everything about a credential that touches the
//! database, the keystore or the network.
//!
//! [`crate::domain::vc`] holds the pure types, signing and ids. This module
//! owns the rest, and the credential commands are thin wrappers over it:
//!
//! - [`issue`]: issue a credential, gated on its integrity session.
//! - [`issuer`]: the [`Issuer`], a signing key and its DID as one value.
//! - [`status_list`]: revocation status lists and their publication.
//! - [`lifecycle`]: revoke, suspend and reinstate.
//! - [`store`]: read stored credentials; decode untrusted payloads.
//! - [`bundle`]: survivability bundles and offline verification.
//! - [`auto`]: self-issued credentials for observed completion mints.

pub mod auto;
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
