//// Alexandria Completion Witness — On-chain types
////
//// A `completion` mint is the on-chain witness that a learner
//// finished every required element of a registered course. The
//// off-chain observer watches for `completion_minting` mints under
//// the learner's verification key hash and auto-issues a self-signed
//// W3C Verifiable Credential whose `witness` block records the mint
//// tx hash + validator script hash + validator name.
////
//// Asset name encoding (32 bytes total, maxed out):
////   learner_pkh (28 bytes) ++ course_tag (4 bytes)
////
//// `course_tag` is `blake2b_224(course_id)[..4]` — collisions are
//// acceptable because the full `course_id` lives in the datum of
//// the output receiving the token.
////
//// Inline datum on the output carrying the minted token:
////   CompletionDatum { subject_pubkey, course_id, completion_root, timestamp }
////
//// The validator reconstructs `completion_root` from the redeemer-
//// supplied `element_leaves` via a blake2b-256 binary Merkle tree
//// and requires it to match the datum.

use aiken/builtin
use aiken/crypto.{VerificationKeyHash, blake2b_256}

// ─── Constants ──────────────────────────────────────────────────────────────

/// Bytes reserved for the learner's pkh inside a completion asset name.
pub const learner_pkh_length = 28

/// Bytes reserved for the course-tag (truncated hash) inside a completion
/// asset name. Chosen so that `learner_pkh_length + course_tag_length = 32`.
pub const course_tag_length = 4

/// Full length of a completion asset name.
pub const completion_asset_name_length = 32

/// Validator name embedded in the VC `witness.validator_name` block.
/// Off-chain consumers compare against this constant.
pub const validator_name: ByteArray = "completion"

// ─── Datum ──────────────────────────────────────────────────────────────────

/// Inline datum carried on the output that receives the minted completion
/// token. Encodes the full completion claim so the observer can materialise
/// a VC without re-deriving anything from the asset name alone.
pub type CompletionDatum {
  /// Ed25519 verification key of the learner (32 bytes). This is what
  /// their `did:key` DID encodes; it pairs with the 28-byte pkh in
  /// the asset name.
  subject_pubkey: ByteArray,
  /// Full course identifier (16 bytes by convention — a UUID-sized
  /// blake2b-128 of the course template).
  course_id: ByteArray,
  /// Completion Merkle root: `blake2b_256` root over the per-element
  /// completion hashes. The validator reconstructs this from the
  /// redeemer and requires equality.
  completion_root: ByteArray,
  /// POSIX milliseconds at completion. The validator requires the tx
  /// validity range to contain this timestamp.
  timestamp: Int,
}

// ─── Redeemer ───────────────────────────────────────────────────────────────

/// Mint/burn selector for the completion policy.
pub type CompletionRedeemer {
  /// Mint a completion witness. Supplies the raw leaves so the
  /// validator can verify the Merkle root in the inline datum.
  MintCompletion { element_leaves: List<ByteArray> }
  /// Burn a previously-minted completion token. Used to clean up
  /// local UTxOs; does not affect the VC (which is already issued).
  BurnCompletion
}

// ─── Merkle root ────────────────────────────────────────────────────────────

/// Compute the blake2b-256 Merkle root of a non-empty list of leaves.
///
/// Pairs are hashed as `blake2b_256(left ++ right)`. If a level has an
/// odd number of nodes the last node is duplicated (Bitcoin-style). An
/// empty list is rejected — a completion with no elements is nonsensical.
pub fn merkle_root(leaves: List<ByteArray>) -> ByteArray {
  when leaves is {
    [] -> fail @"completion requires at least one element leaf"
    [single] -> single
    _ -> merkle_root(reduce_layer(leaves))
  }
}

fn reduce_layer(layer: List<ByteArray>) -> List<ByteArray> {
  when layer is {
    [] -> []
    [single] -> [hash_pair(single, single)]
    [a, b, ..rest] -> [hash_pair(a, b), ..reduce_layer(rest)]
  }
}

fn hash_pair(left: ByteArray, right: ByteArray) -> ByteArray {
  blake2b_256(builtin.append_bytearray(left, right))
}

// ─── Asset name helpers ─────────────────────────────────────────────────────

/// Extract the learner's pkh (first 28 bytes) from a completion asset name.
pub fn extract_learner_pkh(asset_name: ByteArray) -> VerificationKeyHash {
  builtin.slice_bytearray(0, learner_pkh_length, asset_name)
}

/// Extract the 4-byte course tag (last 4 bytes) from a completion asset name.
pub fn extract_course_tag(asset_name: ByteArray) -> ByteArray {
  builtin.slice_bytearray(learner_pkh_length, course_tag_length, asset_name)
}

/// True when `asset_name` has the exact expected length.
pub fn has_valid_length(asset_name: ByteArray) -> Bool {
  builtin.length_of_bytearray(asset_name) == completion_asset_name_length
}

// ─── Tests ──────────────────────────────────────────────────────────────────

test merkle_single_leaf_is_identity() {
  merkle_root([#"aa"]) == #"aa"
}

test merkle_two_leaves_hashes_pair() {
  let root = merkle_root([#"aa", #"bb"])
  root == blake2b_256(#"aabb")
}

test merkle_three_leaves_duplicates_last() {
  // Level 1 pairs: (aa,bb) , (cc,cc) → [h(aabb), h(cccc)]
  // Level 2: h(h(aabb)++h(cccc))
  let l1 = blake2b_256(#"aabb")
  let l2 = blake2b_256(#"cccc")
  merkle_root([#"aa", #"bb", #"cc"]) == blake2b_256(
    builtin.append_bytearray(l1, l2),
  )
}

test asset_name_length_checks() {
  // learner_pkh is 28 bytes, course_tag is 4 bytes → 32 total.
  let pkh = #"aa" |> pad_to(28)
  let good = builtin.append_bytearray(pkh, #"deadbeef")
  has_valid_length(good) && !has_valid_length(#"aabbcc")
}

test asset_name_extracts_fields() {
  let pkh = pad_to(28)(#"aa")
  let tag = #"deadbeef"
  let name = builtin.append_bytearray(pkh, tag)
  extract_learner_pkh(name) == pkh && extract_course_tag(name) == tag
}

fn pad_to(n: Int) -> fn(ByteArray) -> ByteArray {
  fn(b) { pad_bytes(b, n) }
}

fn pad_bytes(b: ByteArray, target: Int) -> ByteArray {
  let current = builtin.length_of_bytearray(b)
  if current >= target {
    builtin.slice_bytearray(0, target, b)
  } else {
    pad_bytes(builtin.append_bytearray(b, #"00"), target)
  }
}
