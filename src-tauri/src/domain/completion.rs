//! Course-completion hashing for the VC-first auto-issuance flow.
//!
//! The on-chain `completion.ak` validator checks that the learner's
//! completion datum carries a Merkle root that equals
//! `merkle_root(element_leaves)` reconstructed from the redeemer.
//! This module produces those leaves and that root in Rust so the
//! values match byte-for-byte between off-chain tx builder and
//! on-chain validator:
//!
//! 1. Each completed element is reduced to a **leaf** — the
//!    blake2b-256 of a canonical JSON record of the element
//!    identifier, the grader CID, a BLAKE3 hash of the submission
//!    bytes, the score, and the grader's self-declared version.
//! 2. The leaves are folded into a single 32-byte root via a binary
//!    Merkle tree using `blake2b_256(left || right)` — duplicating
//!    the final leaf on odd levels, matching `merkle_root` in
//!    `lib/alexandria/completion.ak`.
//!
//! The hashing inputs are deliberately minimal so different learners
//! can converge on the same root for the same course template even
//! if their plugin UI presents things differently. Anything cosmetic
//! (render choice, locale, timestamps) MUST NOT enter the leaf.

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use serde::Serialize;

/// A single completion leaf input. One per element on a course.
///
/// `score` is a 0.0–1.0 fraction; we encode it with six decimal
/// places to ensure cross-platform determinism (floating-point
/// serialization without a fixed format can diverge between runtimes).
#[derive(Debug, Clone, Serialize)]
pub struct ElementCompletion<'a> {
    pub element_id: &'a str,
    pub grader_cid: &'a str,
    /// Hex-encoded BLAKE3 hash of the submission input bytes.
    pub submission_hash: &'a str,
    /// Grader self-declared version from the returned `ScoreRecord`.
    pub grader_version: &'a str,
    /// Score in \[0.0, 1.0\].
    pub score: f64,
}

/// Compute the 32-byte completion leaf for a single element. The
/// canonical JSON is deterministic across runtimes; the BLAKE2b-256
/// output matches the Aiken validator.
pub fn element_leaf(record: &ElementCompletion<'_>) -> [u8; 32] {
    // Round score to six decimals for stability. This loses resolution
    // below 1e-6 but the grader API already emits scores as scaled
    // fractions — six decimals is plenty for recognition thresholds.
    let scaled = (record.score * 1_000_000.0).round() as i64;
    let value = serde_json::json!({
        "element_id": record.element_id,
        "grader_cid": record.grader_cid,
        "grader_version": record.grader_version,
        "score_millionths": scaled,
        "submission_hash": record.submission_hash,
    });
    let canonical = serde_json_canonicalizer::to_vec(&value).expect("canonicalize completion leaf");

    let mut hasher = Blake2b::<U32>::new();
    hasher.update(&canonical);
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

/// Compute the Merkle root over an ordered list of leaves. The order
/// MUST match the order the learner declares on-chain — typically the
/// element order declared by the course template.
///
/// Exactly mirrors `lib/alexandria/completion.ak::merkle_root`:
///   * empty list → panic (the on-chain validator fails),
///   * single leaf → the leaf itself,
///   * otherwise fold `reduce_layer` until a single node remains.
///   * odd-count layers duplicate the final node before pairing.
pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    assert!(
        !leaves.is_empty(),
        "course completion requires at least one element leaf"
    );
    if leaves.len() == 1 {
        return leaves[0];
    }

    let mut current: Vec<[u8; 32]> = leaves.to_vec();
    while current.len() > 1 {
        current = reduce_layer(&current);
    }
    current[0]
}

fn reduce_layer(layer: &[[u8; 32]]) -> Vec<[u8; 32]> {
    let mut next = Vec::with_capacity(layer.len().div_ceil(2));
    let mut i = 0;
    while i < layer.len() {
        let left = layer[i];
        let right = if i + 1 < layer.len() {
            layer[i + 1]
        } else {
            left
        };
        next.push(hash_pair(&left, &right));
        i += 2;
    }
    next
}

fn hash_pair(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(left);
    hasher.update(right);
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf_n(n: u8) -> [u8; 32] {
        [n; 32]
    }

    #[test]
    fn single_leaf_is_identity() {
        let l = leaf_n(0xaa);
        assert_eq!(merkle_root(&[l]), l);
    }

    #[test]
    fn two_leaves_match_expected() {
        let a = leaf_n(0xaa);
        let b = leaf_n(0xbb);
        // blake2b_256(aa..aa || bb..bb)
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(a);
        hasher.update(b);
        let mut expected = [0u8; 32];
        expected.copy_from_slice(&hasher.finalize());
        assert_eq!(merkle_root(&[a, b]), expected);
    }

    #[test]
    fn three_leaves_duplicate_last() {
        // Layer 1: [h(ab), h(cc)]
        // Layer 2: h(h(ab) || h(cc))
        let a = leaf_n(0xaa);
        let b = leaf_n(0xbb);
        let c = leaf_n(0xcc);

        let ab = hash_pair(&a, &b);
        let cc = hash_pair(&c, &c);
        let expected = hash_pair(&ab, &cc);

        assert_eq!(merkle_root(&[a, b, c]), expected);
    }

    #[test]
    fn leaf_is_deterministic() {
        let rec = ElementCompletion {
            element_id: "el_1",
            grader_cid: "cid_abc",
            grader_version: "v1.0.0",
            submission_hash: "b3hash",
            score: 0.92,
        };
        let a = element_leaf(&rec);
        let b = element_leaf(&rec);
        assert_eq!(a, b, "same input must yield same leaf bytes");
    }

    #[test]
    fn leaf_changes_with_any_field() {
        let base = ElementCompletion {
            element_id: "el_1",
            grader_cid: "cid_abc",
            grader_version: "v1.0.0",
            submission_hash: "b3",
            score: 0.50,
        };
        let baseline = element_leaf(&base);

        let different_score = ElementCompletion {
            score: 0.51,
            ..base.clone()
        };
        assert_ne!(element_leaf(&different_score), baseline);

        let different_id = ElementCompletion {
            element_id: "el_2",
            ..base.clone()
        };
        assert_ne!(element_leaf(&different_id), baseline);

        let different_grader = ElementCompletion {
            grader_cid: "cid_xyz",
            ..base.clone()
        };
        assert_ne!(element_leaf(&different_grader), baseline);

        let different_version = ElementCompletion {
            grader_version: "v2.0.0",
            ..base.clone()
        };
        assert_ne!(element_leaf(&different_version), baseline);

        let different_submission = ElementCompletion {
            submission_hash: "b3alt",
            ..base.clone()
        };
        assert_ne!(element_leaf(&different_submission), baseline);
    }

    #[test]
    fn score_rounds_to_millionths() {
        // Scores that differ below 1e-6 resolve to the same leaf —
        // this is intentional so grader output jitter doesn't move
        // the Merkle root.
        let base = ElementCompletion {
            element_id: "el",
            grader_cid: "cid",
            grader_version: "v",
            submission_hash: "h",
            score: 0.5000001,
        };
        let jittered = ElementCompletion {
            score: 0.5000002,
            ..base.clone()
        };
        assert_eq!(element_leaf(&base), element_leaf(&jittered));
    }

    #[test]
    #[should_panic(expected = "at least one element")]
    fn empty_leaf_list_panics() {
        let _ = merkle_root(&[]);
    }
}
