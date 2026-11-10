//! Integrity anchors (spec §11.3.1, §14.12.3).
//!
//! An issuer's node anchors a batch of credentials at once: it builds a Merkle
//! tree over their integrity hashes `H(c)` and records the root in one
//! confirmed Cardano transaction (label-1697 metadata, version 2). The anchor
//! proves each credential existed no later than the block that holds it. It
//! does not replace the signature check.
//!
//! Each credential carries an [`AnchorReceipt`]: its position in the batch and
//! the sibling hashes up to the root. A verifier recomputes `H(c)`, folds the
//! receipt up to a root, and compares it with what the caller observed on the
//! ledger ([`IntegrityAnchor`]).
//!
//! # Tree
//!
//! RFC 6962 / RFC 9162 Merkle tree with BLAKE3 and domain separation:
//!
//! ```text
//! leaf  = BLAKE3(0x00 || H(c))
//! node  = BLAKE3(0x01 || left || right)
//! MTH({d})  = leaf(d)
//! MTH(D[n]) = node(MTH(D[0:k]), MTH(D[k:n])), k = largest power of two < n
//! ```
//!
//! This crate reads no chain. The caller observes the transaction (from its
//! own submission journal, Blockfrost, a node or an archive) and hands both
//! pieces over through
//! [`VerificationStore::integrity_anchor`](crate::VerificationStore::integrity_anchor).

use serde::{Deserialize, Serialize};

use super::{canonicalize::canonicalize, VcError, VerifiableCredential};

/// Receipt format version.
pub const RECEIPT_VERSION: u32 = 1;

/// One label-1697 anchor transaction, as the caller observed it on the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityAnchor {
    /// `root` from the metadata: hex Merkle root over the batch.
    pub root: String,
    /// `leaf_count` from the metadata.
    pub leaf_count: u64,
    /// `issuer_did` from the metadata.
    pub issuer_did: String,
    /// The transaction's required signers, as hex Ed25519 key hashes.
    pub required_signers: Vec<String>,
    /// The anchoring transaction.
    pub tx_hash: String,
    /// When the ledger confirmed the transaction, if the caller knows.
    pub anchored_at: Option<String>,
}

/// Proof that one credential's `H(c)` is a leaf of an anchored batch. A sidecar
/// to the credential: never embedded in it, so the signature is unaffected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorReceipt {
    /// [`RECEIPT_VERSION`].
    pub v: u32,
    /// The leaf's position in the batch, from 0.
    pub leaf_index: u64,
    /// Number of leaves in the batch.
    pub leaf_count: u64,
    /// RFC 9162 audit path, leaf to root, hex.
    pub path: Vec<String>,
    /// The batch root, hex.
    pub root: String,
    /// The anchoring transaction.
    pub tx_hash: String,
    /// The issuer that anchored the batch.
    pub issuer_did: String,
}

/// What a store hands the verifier: the credential's receipt and the ledger
/// observation of the transaction it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorEvidence {
    pub receipt: AnchorReceipt,
    pub observed: IntegrityAnchor,
}

/// `H(c)`: BLAKE3 over the JCS-canonical credential with `proofValue` emptied,
/// lower-case hex. It names the signed content rather than one signature over
/// it, and it is the leaf value an anchor commits to.
pub fn integrity_hash(credential: &VerifiableCredential) -> Result<String, VcError> {
    let mut unsigned = credential.clone();
    unsigned.proof.proof_value.clear();
    let value = serde_json::to_value(&unsigned)?;
    let bytes = canonicalize(&value)?;
    Ok(hex::encode(blake3::hash(&bytes).as_bytes()))
}

fn leaf_hash(leaf: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[0x00]);
    hasher.update(leaf);
    *hasher.finalize().as_bytes()
}

fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[0x01]);
    hasher.update(left);
    hasher.update(right);
    *hasher.finalize().as_bytes()
}

/// Largest power of two strictly less than `n` (`n >= 2`).
fn split(n: usize) -> usize {
    let mut k = 1;
    while k * 2 < n {
        k *= 2;
    }
    k
}

/// Merkle root over `leaves` (each an `H(c)`), in batch order.
///
/// # Panics
/// On an empty batch: there is nothing to anchor.
pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    assert!(!leaves.is_empty(), "an anchor batch has at least one leaf");
    if leaves.len() == 1 {
        return leaf_hash(&leaves[0]);
    }
    let k = split(leaves.len());
    node_hash(&merkle_root(&leaves[..k]), &merkle_root(&leaves[k..]))
}

/// RFC 9162 audit path for `leaves[index]`, ordered leaf to root.
///
/// # Panics
/// When `index` is out of range.
pub fn merkle_path(leaves: &[[u8; 32]], index: usize) -> Vec<[u8; 32]> {
    assert!(index < leaves.len(), "leaf index out of range");
    if leaves.len() == 1 {
        return Vec::new();
    }
    let k = split(leaves.len());
    let (mut path, sibling) = if index < k {
        (merkle_path(&leaves[..k], index), merkle_root(&leaves[k..]))
    } else {
        (
            merkle_path(&leaves[k..], index - k),
            merkle_root(&leaves[..k]),
        )
    };
    path.push(sibling);
    path
}

fn decode32(hex_str: &str) -> Option<[u8; 32]> {
    hex::decode(hex_str).ok()?.try_into().ok()
}

/// Fold `credential_hash` up `receipt.path` (RFC 9162 §2.1.3.2) and return the
/// root it reaches, lower-case hex. `None` when the receipt is malformed: an
/// index outside the batch, a path too long for its size, or bad hex.
pub fn receipt_root(credential_hash: &str, receipt: &AnchorReceipt) -> Option<String> {
    if receipt.leaf_index >= receipt.leaf_count {
        return None;
    }
    let mut fnode = receipt.leaf_index;
    let mut snode = receipt.leaf_count - 1;
    let mut r = leaf_hash(&decode32(credential_hash)?);
    for sibling in &receipt.path {
        let p = decode32(sibling)?;
        if snode == 0 {
            return None;
        }
        if fnode & 1 == 1 || fnode == snode {
            r = node_hash(&p, &r);
            while fnode & 1 == 0 && fnode != 0 {
                fnode >>= 1;
                snode >>= 1;
            }
        } else {
            r = node_hash(&r, &p);
        }
        fnode >>= 1;
        snode >>= 1;
    }
    (snode == 0).then(|| hex::encode(r))
}

/// The Cardano key hash of a `did:key` issuer: Blake2b-224 of its Ed25519
/// public key, lower-case hex. `None` for anything that is not a `did:key`.
pub fn issuer_key_hash(issuer_did: &str) -> Option<String> {
    let key = crate::did::resolve_did_key(&crate::did::Did(issuer_did.to_string())).ok()?;
    Some(hex::encode(crate::hash::blake2b_224(key.as_bytes())))
}

/// Whether `evidence` anchors this credential:
///
/// - the receipt folds from `credential_hash` to its root;
/// - the receipt's root, batch size and transaction are the observed ones;
/// - the receipt and the observed metadata name the credential's issuer;
/// - the issuer's key is a required signer of the transaction.
///
/// Hex case is not significant.
pub fn anchor_matches(
    evidence: &AnchorEvidence,
    credential: &VerifiableCredential,
    credential_hash: &str,
) -> bool {
    let (receipt, observed) = (&evidence.receipt, &evidence.observed);
    let issuer = credential.issuer.as_str();
    let Some(folded) = receipt_root(credential_hash, receipt) else {
        return false;
    };
    let Some(key_hash) = issuer_key_hash(issuer) else {
        return false;
    };
    receipt.v == RECEIPT_VERSION
        && folded.eq_ignore_ascii_case(&receipt.root)
        && receipt.root.eq_ignore_ascii_case(&observed.root)
        && receipt.leaf_count == observed.leaf_count
        && receipt.tx_hash.eq_ignore_ascii_case(&observed.tx_hash)
        && receipt.issuer_did == issuer
        && observed.issuer_did == issuer
        && observed
            .required_signers
            .iter()
            .any(|signer| signer.eq_ignore_ascii_case(&key_hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(n: u8) -> [u8; 32] {
        [n; 32]
    }

    // Worked examples straight from the spec's hashing rule, written out by
    // hand rather than through the tree code.
    fn h_leaf(l: [u8; 32]) -> [u8; 32] {
        let mut b = vec![0x00];
        b.extend_from_slice(&l);
        *blake3::hash(&b).as_bytes()
    }
    fn h_node(l: [u8; 32], r: [u8; 32]) -> [u8; 32] {
        let mut b = vec![0x01];
        b.extend_from_slice(&l);
        b.extend_from_slice(&r);
        *blake3::hash(&b).as_bytes()
    }

    #[test]
    fn tree_roots_follow_the_rfc6962_shape() {
        let (a, b, c, d, e) = (leaf(1), leaf(2), leaf(3), leaf(4), leaf(5));
        assert_eq!(merkle_root(&[a]), h_leaf(a));
        assert_eq!(merkle_root(&[a, b]), h_node(h_leaf(a), h_leaf(b)));
        assert_eq!(
            merkle_root(&[a, b, c]),
            h_node(h_node(h_leaf(a), h_leaf(b)), h_leaf(c))
        );
        let ab_cd = h_node(h_node(h_leaf(a), h_leaf(b)), h_node(h_leaf(c), h_leaf(d)));
        assert_eq!(merkle_root(&[a, b, c, d, e]), h_node(ab_cd, h_leaf(e)));
    }

    #[test]
    fn audit_path_of_the_last_odd_leaf_is_its_left_sibling_subtree() {
        let leaves = [leaf(1), leaf(2), leaf(3)];
        let ab = h_node(h_leaf(leaves[0]), h_leaf(leaves[1]));
        assert_eq!(merkle_path(&leaves, 2), vec![ab]);
        assert_eq!(
            merkle_path(&leaves, 0),
            vec![h_leaf(leaves[1]), h_leaf(leaves[2])]
        );
    }

    fn receipt_for(leaves: &[[u8; 32]], index: usize) -> AnchorReceipt {
        AnchorReceipt {
            v: 1,
            leaf_index: index as u64,
            leaf_count: leaves.len() as u64,
            path: merkle_path(leaves, index).iter().map(hex::encode).collect(),
            root: hex::encode(merkle_root(leaves)),
            tx_hash: "ab".repeat(32),
            issuer_did: "did:key:zIssuer".into(),
        }
    }

    #[test]
    fn every_leaf_of_a_256_leaf_batch_folds_to_the_root() {
        let leaves: Vec<[u8; 32]> = (0..=255u8).map(leaf).collect();
        let root = hex::encode(merkle_root(&leaves));
        for (i, l) in leaves.iter().enumerate() {
            let receipt = receipt_for(&leaves, i);
            assert_eq!(
                receipt_root(&hex::encode(l), &receipt).as_deref(),
                Some(root.as_str())
            );
        }
    }

    #[test]
    fn a_receipt_folds_to_the_root_only_for_its_own_leaf_and_position() {
        let leaves: Vec<[u8; 32]> = (1..=5u8).map(leaf).collect();
        let root = hex::encode(merkle_root(&leaves));
        let receipt = receipt_for(&leaves, 3);
        assert_eq!(
            receipt_root(&hex::encode(leaves[3]), &receipt).unwrap(),
            root
        );
        assert_ne!(
            receipt_root(&hex::encode(leaves[2]), &receipt).unwrap(),
            root
        );
        let moved = AnchorReceipt {
            leaf_index: 2,
            ..receipt.clone()
        };
        assert_ne!(
            receipt_root(&hex::encode(leaves[3]), &moved),
            Some(root.clone())
        );
        let out_of_range = AnchorReceipt {
            leaf_index: 5,
            ..receipt.clone()
        };
        assert_eq!(receipt_root(&hex::encode(leaves[3]), &out_of_range), None);
        let mut short = receipt.clone();
        short.path.pop();
        assert_ne!(receipt_root(&hex::encode(leaves[3]), &short), Some(root));
        assert_eq!(receipt_root("not hex", &receipt), None);
    }

    #[test]
    fn the_issuer_key_hash_is_blake2b_224_of_the_did_key() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        let did = crate::did::derive_did_key(&key);
        let expected = hex::encode(crate::hash::blake2b_224(key.verifying_key().as_bytes()));
        assert_eq!(
            issuer_key_hash(did.as_str()).as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(issuer_key_hash("did:web:example.org"), None);
    }
}
