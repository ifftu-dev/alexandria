//! Sign a VC payload with Ed25519Signature2020. Stub — implementation in PR 4.

use ed25519_dalek::SigningKey;

use super::{VcError, VerifiableCredential};
use crate::crypto::did::Did;

/// The unsigned portion of a VC — everything except `proof`.
///
/// The caller constructs this explicitly; `sign_credential` canonicalizes
/// the JCS bytes, produces an Ed25519 signature, and returns the full
/// signed `VerifiableCredential`.
#[derive(Debug, Clone)]
pub struct UnsignedCredential {
    pub credential: VerifiableCredential,
}

pub fn sign_credential(
    _unsigned: UnsignedCredential,
    _signing_key: &SigningKey,
    _issuer_did: &Did,
) -> Result<VerifiableCredential, VcError> {
    unimplemented!("PR 4 — Ed25519Signature2020")
}
