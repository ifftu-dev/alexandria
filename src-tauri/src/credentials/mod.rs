//! Verifiable credentials: everything about a credential that touches the
//! database, the keystore or the network.
//!
//! [`crate::domain::vc`] holds the pure types, signing and ids. This module
//! stores credentials and keeps their revocation status lists. The
//! credential commands are thin wrappers over it.

pub mod status_list;

#[cfg(test)]
pub(crate) mod fixtures;
