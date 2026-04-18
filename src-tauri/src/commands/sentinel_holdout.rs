//! Sentinel DAO holdout evaluation set — encrypted, Shamir-split.
//!
//! The holdout is an unpublished labeled-samples blob the DAO uses to
//! measure Sentinel's classifier accuracy and false-positive rate
//! without leaking evaluation criteria to attackers (decision 7 in
//! `docs/sentinel-federation.md`).
//!
//! Crypto stack:
//!   1. Random AES-256 key → AES-256-GCM encrypt the blob
//!   2. Shamir-split the key into N shares over GF(256)
//!   3. Seal each share to one committee member's X25519 pubkey using
//!      the existing ECDH + AES-GCM wrapper in [`crate::crypto::group_key`]
//!   4. Pin the encrypted blob; store metadata + sealed shares in SQL
//!
//! Decrypt flow (out-of-band coordination between ≥ `threshold` members):
//!   1. Each member calls [`sentinel_holdout_unseal_share`] locally with
//!      their X25519 static secret to recover their plaintext share
//!   2. The evaluator collects ≥ `threshold` shares (via P2P, DMs,
//!      whatever the DAO coordinates on) and calls
//!      [`sentinel_holdout_evaluate`] with them
//!   3. Shamir combines → AES decrypts → parsed [`PriorBlob`] is returned
//!      for client-side evaluation against the local classifier
//!
//! Private keys never cross the IPC boundary to a remote host — this is
//! a Tauri local-app. The `our_x25519_secret_hex` parameter of
//! [`sentinel_holdout_unseal_share`] is passed within the same process.

use std::collections::BTreeSet;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::commands::sentinel_priors::{validate_prior_blob, ModelKind, PriorBlob};
use crate::crypto::group_key::{decrypt_message, encrypt_message, generate_group_key};
use crate::crypto::hash::entity_id;
use crate::crypto::shamir::{self, Share};
use crate::ipfs::{content, storage};
use crate::AppState;

/// Pin type for encrypted holdout blobs. Separate from 'sentinel_prior'
/// so eviction heuristics can treat them independently — the holdout is
/// smaller and re-creating it is a governance event, not a re-sync.
const PIN_TYPE_SENTINEL_HOLDOUT: &str = "sentinel_holdout";

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberSealRequest {
    pub stake_address: String,
    /// 32-byte X25519 public key, hex-encoded (64 chars).
    pub x25519_pubkey_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedShare {
    pub share_index: u8,
    pub stake_address: String,
    pub x25519_pubkey_hex: String,
    /// The sender's (uploader's) ephemeral X25519 public key so the
    /// member can do ECDH to recover the wrapping AES key. Hex-encoded.
    pub sender_x25519_pubkey_hex: String,
    /// Hex-encoded output of `group_key::encrypt_key_for_member` applied
    /// to the raw Shamir share bytes: `nonce(12) || ciphertext`.
    pub sealed_share_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPolicy {
    pub threshold: u8,
    pub shares: Vec<SealedShare>,
}

#[derive(Debug, Deserialize)]
pub struct UploadHoldoutRequest {
    pub model_kind: String,
    pub threshold: u8,
    pub members: Vec<MemberSealRequest>,
    /// The labeled-samples blob bytes. Must parse under the prior-blob
    /// envelope with the same `model_kind`. Keep this in memory only —
    /// it's the plaintext holdout.
    pub plaintext: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct UploadHoldoutResponse {
    pub holdout_id: String,
    pub encrypted_cid: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HoldoutRef {
    pub id: String,
    pub encrypted_cid: String,
    pub model_kind: String,
    pub threshold: u8,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct UnsealShareRequest {
    pub holdout_id: String,
    /// Hex-encoded 32-byte X25519 static secret of the calling member.
    /// Stays in-process; not persisted anywhere beyond the command's
    /// stack frame.
    pub our_x25519_secret_hex: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaintextShare {
    pub share_index: u8,
    pub y_hex: String,
}

#[derive(Debug, Deserialize)]
pub struct EvaluateRequest {
    pub holdout_id: String,
    pub shares: Vec<PlaintextShare>,
}

// ============================================================================
// Hex helpers (local, narrow-purpose; avoids pulling a new dep)
// ============================================================================

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("hex string length must be even".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("invalid hex: {e}")))
        .collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn parse_x25519_pubkey(hex_str: &str) -> Result<PublicKey, String> {
    let bytes = hex_decode(hex_str)?;
    if bytes.len() != 32 {
        return Err(format!("expected 32-byte pubkey, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(PublicKey::from(arr))
}

fn parse_x25519_secret(hex_str: &str) -> Result<StaticSecret, String> {
    let bytes = hex_decode(hex_str)?;
    if bytes.len() != 32 {
        return Err(format!("expected 32-byte secret, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(StaticSecret::from(arr))
}

// ============================================================================
// Commands
// ============================================================================

/// Upload a new holdout evaluation set.
///
/// Encrypts the plaintext blob with a fresh AES-256 key, Shamir-splits
/// the key to the N committee members, seals each share to its
/// member's X25519 pubkey, pins the encrypted blob, and records the
/// policy envelope in `sentinel_holdout_refs`.
///
/// Only the uploader ever holds the AES key — it's dropped at the end
/// of this function. The plaintext parameter stays in memory only for
/// the duration of the call.
#[tauri::command]
pub async fn sentinel_holdout_upload(
    state: State<'_, AppState>,
    req: UploadHoldoutRequest,
) -> Result<UploadHoldoutResponse, String> {
    // Validate model_kind (rejects 'face' and anything unknown).
    let _kind = <ModelKind as std::str::FromStr>::from_str(&req.model_kind)?;

    // Validate the blob body matches the envelope contract before we
    // commit to encryption + pinning.
    let json = std::str::from_utf8(&req.plaintext)
        .map_err(|_| "holdout bytes are not valid UTF-8 JSON".to_string())?;
    let blob = validate_prior_blob(json)?;
    if blob.model_kind != req.model_kind {
        return Err(format!(
            "blob model_kind={} does not match request model_kind={}",
            blob.model_kind, req.model_kind
        ));
    }

    // Shamir constraints.
    let threshold = req.threshold as usize;
    let n = req.members.len();
    if threshold == 0 || threshold > n || n > 255 {
        return Err(format!(
            "invalid (threshold, n) = ({threshold}, {n}); need 1 <= threshold <= n <= 255"
        ));
    }

    // Reject duplicate member pubkeys — Shamir x coords are derived
    // implicitly (1..=n), but identical pubkeys would mean the same
    // human holds two shares, which undermines the threshold.
    let mut seen = BTreeSet::new();
    for m in &req.members {
        if !seen.insert(m.x25519_pubkey_hex.clone()) {
            return Err(format!("duplicate member pubkey: {}", m.x25519_pubkey_hex));
        }
    }

    // 1. Generate AES key, encrypt the plaintext.
    let aes_key = generate_group_key();
    let ciphertext =
        encrypt_message(&aes_key, &req.plaintext).map_err(|e| format!("AES encrypt: {e}"))?;

    // 2. Shamir-split the AES key.
    let shares = shamir::split(&aes_key, threshold, n).map_err(|e| format!("shamir: {e}"))?;

    // 3. Seal each share to its member. Use a fresh ephemeral X25519
    //    secret per upload — the sender isn't a stable identity here.
    let ephemeral_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let ephemeral_pub = PublicKey::from(&ephemeral_secret);
    let ephemeral_pub_hex = hex_encode(ephemeral_pub.as_bytes());

    // Shamir share y-bytes have `|secret|` length (matches |aes_key|=32
    // for an AES-256 key, but the algorithm is byte-wise so any length
    // works). group_key::encrypt_key_for_member is specialized to 32
    // bytes, so we do ECDH + AES-GCM inline against arbitrary lengths.
    let mut sealed_shares: Vec<SealedShare> = Vec::with_capacity(n);
    for (share, member) in shares.iter().zip(req.members.iter()) {
        let member_pub = parse_x25519_pubkey(&member.x25519_pubkey_hex)
            .map_err(|e| format!("member {}: {e}", member.stake_address))?;
        let wrap_key = derive_wrap_key(&ephemeral_secret, &member_pub);
        let sealed_bytes = encrypt_message(&wrap_key, &share.y)
            .map_err(|e| format!("seal share {}: {e}", share.x))?;

        sealed_shares.push(SealedShare {
            share_index: share.x,
            stake_address: member.stake_address.clone(),
            x25519_pubkey_hex: member.x25519_pubkey_hex.clone(),
            sender_x25519_pubkey_hex: ephemeral_pub_hex.clone(),
            sealed_share_hex: hex_encode(&sealed_bytes),
        });
    }

    let policy = KeyPolicy {
        threshold: req.threshold,
        shares: sealed_shares,
    };
    let policy_json = serde_json::to_string(&policy).map_err(|e| e.to_string())?;

    // 4. Pin the encrypted blob (size = ciphertext.len()).
    let add_result = content::add_bytes(&state.content_node, &ciphertext)
        .await
        .map_err(|e| format!("content_add: {e}"))?;

    // 5. Record the metadata + policy in SQLite.
    let holdout_id = entity_id(&[
        "sentinel-holdout",
        &add_result.hash,
        &req.model_kind,
        &chrono::Utc::now().to_rfc3339(),
    ]);

    {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();

        conn.execute(
            "INSERT INTO sentinel_holdout_refs
                 (id, encrypted_cid, model_kind, threshold, key_policy)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                holdout_id,
                add_result.hash,
                req.model_kind,
                req.threshold as i64,
                policy_json,
            ],
        )
        .map_err(|e| format!("holdout insert failed: {e}"))?;

        storage::upsert_pin(
            conn,
            &add_result.hash,
            PIN_TYPE_SENTINEL_HOLDOUT,
            add_result.size,
            false,
        );
    }

    // Note: `aes_key` is `[u8; 32]` (Copy) so `drop` is a no-op. Real
    // zeroization would need the `zeroize` crate; left as follow-up
    // since the key never crosses a process boundary in this call.
    let _ = aes_key;

    Ok(UploadHoldoutResponse {
        holdout_id,
        encrypted_cid: add_result.hash,
    })
}

/// List all holdout sets — returns metadata only, never the policy.
#[tauri::command]
pub async fn sentinel_holdout_list(state: State<'_, AppState>) -> Result<Vec<HoldoutRef>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT id, encrypted_cid, model_kind, threshold, created_at
             FROM sentinel_holdout_refs ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<HoldoutRef> = stmt
        .query_map([], |row| {
            Ok(HoldoutRef {
                id: row.get(0)?,
                encrypted_cid: row.get(1)?,
                model_kind: row.get(2)?,
                threshold: row.get::<_, i64>(3)? as u8,
                created_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Fetch the sealed key policy for a given holdout.
///
/// The policy is public-by-design — it contains only pubkeys and
/// sealed-share ciphertexts, which do not leak plaintext shares.
#[tauri::command]
pub async fn sentinel_holdout_get_policy(
    state: State<'_, AppState>,
    holdout_id: String,
) -> Result<KeyPolicy, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let policy_json: String = conn
        .query_row(
            "SELECT key_policy FROM sentinel_holdout_refs WHERE id = ?1",
            params![holdout_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("holdout not found: {e}"))?;

    serde_json::from_str(&policy_json).map_err(|e| format!("policy parse: {e}"))
}

/// Unseal the caller's share of a given holdout using their own X25519
/// static secret. Returns the plaintext share so the caller can hand it
/// to the evaluator. The secret parameter never leaves this process.
#[tauri::command]
pub async fn sentinel_holdout_unseal_share(
    state: State<'_, AppState>,
    req: UnsealShareRequest,
) -> Result<PlaintextShare, String> {
    let policy = sentinel_holdout_get_policy(state, req.holdout_id).await?;

    let our_secret = parse_x25519_secret(&req.our_x25519_secret_hex)?;
    let our_pub = PublicKey::from(&our_secret);
    let our_pub_hex = hex_encode(our_pub.as_bytes());

    let sealed = policy
        .shares
        .iter()
        .find(|s| s.x25519_pubkey_hex == our_pub_hex)
        .ok_or_else(|| "no share addressed to our pubkey in this holdout".to_string())?;

    // Reverse of the upload-side seal: ECDH with the sender's ephemeral
    // pubkey, SHA-256 to derive the wrap key, AES-GCM decrypt.
    let sender_pub = parse_x25519_pubkey(&sealed.sender_x25519_pubkey_hex)?;
    let wrap_key = derive_wrap_key(&our_secret, &sender_pub);
    let sealed_bytes = hex_decode(&sealed.sealed_share_hex)?;
    let plaintext_y =
        decrypt_message(&wrap_key, &sealed_bytes).map_err(|e| format!("unseal failed: {e}"))?;

    Ok(PlaintextShare {
        share_index: sealed.share_index,
        y_hex: hex_encode(&plaintext_y),
    })
}

/// ECDH + SHA-256 to derive a 32-byte AES key from a static/ephemeral
/// X25519 pair. Symmetric in its two inputs from the field's
/// perspective: upload uses `(our_ephemeral_secret, member_pubkey)`,
/// unseal uses `(member_secret, sender_ephemeral_pubkey)`, and both
/// produce the same shared DH output → same SHA-256 wrap key.
fn derive_wrap_key(our_secret: &StaticSecret, their_pubkey: &PublicKey) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let shared = our_secret.diffie_hellman(their_pubkey);
    let digest = Sha256::digest(shared.as_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Combine ≥ `threshold` plaintext shares, AES-decrypt the holdout,
/// and return the parsed blob so a caller can feed it into their
/// local classifier.
///
/// The returned blob contains the labeled evaluation samples in the
/// clear. It MUST stay on the evaluator's device — re-publishing it
/// would leak the holdout criteria to attackers.
#[tauri::command]
pub async fn sentinel_holdout_evaluate(
    state: State<'_, AppState>,
    req: EvaluateRequest,
) -> Result<PriorBlob, String> {
    // Fetch the policy for threshold + encrypted CID lookup.
    let (encrypted_cid, threshold): (String, i64) = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .query_row(
                "SELECT encrypted_cid, threshold FROM sentinel_holdout_refs WHERE id = ?1",
                params![req.holdout_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("holdout not found: {e}"))?
    };

    if (req.shares.len() as i64) < threshold {
        return Err(format!(
            "insufficient shares: got {}, need >= {}",
            req.shares.len(),
            threshold
        ));
    }

    let shamir_shares: Vec<Share> = req
        .shares
        .into_iter()
        .map(|s| {
            let y = hex_decode(&s.y_hex)?;
            Ok::<_, String>(Share {
                x: s.share_index,
                y,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let aes_key_vec = shamir::combine(&shamir_shares).map_err(|e| format!("shamir: {e}"))?;
    if aes_key_vec.len() != 32 {
        return Err(format!(
            "reconstructed key has wrong length: {}",
            aes_key_vec.len()
        ));
    }
    let mut aes_key = [0u8; 32];
    aes_key.copy_from_slice(&aes_key_vec);

    // Fetch the encrypted blob from the content store.
    let ciphertext = content::get_bytes(&state.content_node, &encrypted_cid)
        .await
        .map_err(|e| format!("content_get: {e}"))?;
    let plaintext = decrypt_message(&aes_key, &ciphertext)
        .map_err(|e| format!("AES decrypt failed (wrong shares?): {e}"))?;

    // `aes_key` is Copy; `drop` is a no-op on it. Same follow-up as
    // upload: introduce zeroize to actually scrub.
    let _ = aes_key;
    drop(aes_key_vec);

    let json = std::str::from_utf8(&plaintext)
        .map_err(|_| "decrypted holdout is not valid UTF-8 JSON".to_string())?;
    validate_prior_blob(json)
}

// ============================================================================
// Tests — pure helpers only; crypto round-trip tests live in
// shamir.rs and group_key.rs. Integration tests for the command
// path would require AppState plumbing and are deferred to a
// follow-up.
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip_preserves_bytes() {
        let cases: &[&[u8]] = &[b"", &[0], &[255, 0, 128, 64]];
        for &c in cases {
            let encoded = hex_encode(c);
            let decoded = hex_decode(&encoded).unwrap();
            assert_eq!(decoded, c);
        }
    }

    #[test]
    fn hex_decode_rejects_odd_length() {
        assert!(hex_decode("abc").is_err());
    }

    #[test]
    fn hex_decode_rejects_garbage() {
        assert!(hex_decode("zz").is_err());
    }

    #[test]
    fn parse_x25519_pubkey_requires_32_bytes() {
        assert!(parse_x25519_pubkey(&hex_encode(&[0u8; 32])).is_ok());
        assert!(parse_x25519_pubkey(&hex_encode(&[0u8; 16])).is_err());
    }

    #[test]
    fn seal_unseal_roundtrip_recovers_share_bytes() {
        // End-to-end test of the seal/unseal path without any SQL /
        // content store — just the crypto.
        let sender_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let sender_pub = PublicKey::from(&sender_secret);

        let member_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let member_pub = PublicKey::from(&member_secret);

        let share_y = [0x42u8; 32];
        let wrap_upload = derive_wrap_key(&sender_secret, &member_pub);
        let sealed = encrypt_message(&wrap_upload, &share_y).unwrap();

        let wrap_unseal = derive_wrap_key(&member_secret, &sender_pub);
        let recovered = decrypt_message(&wrap_unseal, &sealed).unwrap();

        assert_eq!(recovered, share_y);
    }

    #[test]
    fn unseal_with_wrong_secret_fails() {
        let sender_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let member_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let wrong_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let member_pub = PublicKey::from(&member_secret);
        let sender_pub = PublicKey::from(&sender_secret);

        let share_y = [0xABu8; 32];
        let wrap_upload = derive_wrap_key(&sender_secret, &member_pub);
        let sealed = encrypt_message(&wrap_upload, &share_y).unwrap();

        // The wrong secret yields a different wrap key, so AES-GCM
        // authentication fails.
        let wrong_wrap = derive_wrap_key(&wrong_secret, &sender_pub);
        assert!(decrypt_message(&wrong_wrap, &sealed).is_err());
    }
}
