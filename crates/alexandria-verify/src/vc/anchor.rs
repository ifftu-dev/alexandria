//! Integrity anchors (spec §11.3.1, §14.12.3).
//!
//! An anchor is a confirmed Cardano transaction whose label-1697 metadata
//! records `A(c) = (H(c), t_i, issuer_ref)`: the credential's integrity hash,
//! its issuance time and its issuer. It proves the credential existed no later
//! than the block that holds it. It does not replace the signature check.
//!
//! This crate reads no chain. A caller observes the transaction (from its own
//! submission journal, Blockfrost, a node or an archive) and hands the
//! recorded fields over through
//! [`VerificationStore::integrity_anchor`](crate::VerificationStore::integrity_anchor).
//! The verifier then recomputes `H(c)` itself and accepts the anchor only when
//! the recorded hash and issuer are the credential's own.

use serde::{Deserialize, Serialize};

use super::{canonicalize::canonicalize, VcError, VerifiableCredential};

/// The fields of a confirmed label-1697 anchor transaction, as the caller
/// observed them on the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityAnchor {
    /// `credential_hash` from the metadata: hex BLAKE3, see [`integrity_hash`].
    pub credential_hash: String,
    /// `issuer_did` from the metadata.
    pub issuer_did: String,
    /// The anchoring transaction.
    pub tx_hash: String,
    /// When the ledger confirmed the transaction, if the caller knows.
    pub anchored_at: Option<String>,
}

/// `H(c)`: BLAKE3 over the JCS-canonical credential with `proofValue` emptied,
/// lower-case hex. It names the signed content rather than one signature over
/// it, and it is the value an anchor records.
pub fn integrity_hash(credential: &VerifiableCredential) -> Result<String, VcError> {
    let mut unsigned = credential.clone();
    unsigned.proof.proof_value.clear();
    let value = serde_json::to_value(&unsigned)?;
    let bytes = canonicalize(&value)?;
    Ok(hex::encode(blake3::hash(&bytes).as_bytes()))
}

/// Whether `anchor` records this credential: the same integrity hash and the
/// same issuer. Hex case is not significant.
pub fn anchor_matches(
    anchor: &IntegrityAnchor,
    credential: &VerifiableCredential,
    credential_hash: &str,
) -> bool {
    anchor.credential_hash.eq_ignore_ascii_case(credential_hash)
        && anchor.issuer_did == credential.issuer.as_str()
}
