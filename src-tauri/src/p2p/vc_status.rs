//! `TOPIC_VC_STATUS` handler — issuers broadcast status list snapshots
//! and deltas for revocation/suspension.
//!
//! A status list decides whether every credential an issuer ever signed reads
//! as revoked, so the only thing that may write one is the issuer. The payload
//! carries a detached Ed25519 signature over the canonical
//! `(list_id, issuer, version, bits)` tuple, verified against the key the
//! issuer's `did:key` self-resolves to. An unsigned or wrongly-signed document
//! is dropped.
//!
//! This used to check only that the issuer DID appeared in the local
//! `key_registry` — which any peer can arrange, since `TOPIC_VC_DID` inserts
//! rows — and no signature was ever checked despite the doc comment saying so.
//! That let any network participant mass-revoke an issuer's credentials, mass
//! *un*-revoke them by publishing a higher version with zeroed bits, or lock
//! the real issuer out permanently by claiming `version = i64::MAX`.

use base64::Engine;

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

/// Ceiling on a decoded status bitmap.
///
/// 1 MiB is 8.4 million credentials in one list, far past any real issuer, and
/// the value arrives base64-encoded from the network with nothing else bounding
/// it.
const MAX_BITS_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusIngest {
    Applied,
    IgnoredNewer,
    IgnoredUnknownIssuer,
    /// Parsed and attributed, but the issuer signature is absent or wrong.
    IgnoredBadSignature,
}

#[derive(serde::Deserialize)]
struct StatusMessage {
    issuer: String,
    /// Optional explicit list_id — if absent we derive
    /// `urn:alexandria:status-list:<issuer>:1` so this matches the
    /// list_id format `commands::credentials` uses for local issuance.
    #[serde(default)]
    list_id: Option<String>,
    version: i64,
    /// Base64-encoded bitmap.
    bits: String,
    /// Base64 (standard alphabet) Ed25519 signature by the issuer over
    /// [`canonical_status_bytes`]. Required.
    #[serde(default)]
    proof: Option<String>,
}

/// The bytes an issuer signs to authorise a status list.
///
/// Every field that changes the meaning of the document is covered:
/// `list_id` so a signature cannot be moved onto a different list, `issuer` so
/// it cannot be re-attributed, `version` so the rollback guard cannot be
/// weaponised by replaying an old signature at a new number, and `bits` so the
/// revocation state itself is authenticated. Length-prefixed so no two
/// different tuples can produce the same byte string.
pub fn canonical_status_bytes(list_id: &str, issuer: &str, version: i64, bits: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(list_id.len() + issuer.len() + bits.len() + 32);
    out.extend_from_slice(b"alexandria-status-list-v1");
    for field in [list_id.as_bytes(), issuer.as_bytes()] {
        out.extend_from_slice(&(field.len() as u64).to_be_bytes());
        out.extend_from_slice(field);
    }
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&(bits.len() as u64).to_be_bytes());
    out.extend_from_slice(bits);
    out
}

/// Build the signed status document a publisher broadcasts.
///
/// Producer and verifier share [`canonical_status_bytes`] through this, so the
/// two cannot drift — and there is no path to publishing an unsigned document
/// by forgetting a field, because the only way to build one is to pass the key.
///
/// The issuer DID is derived from `signing_key` rather than taken as an
/// argument: a document whose `issuer` names a DID the signer does not control
/// would fail verification everywhere, so there is no reason to make it
/// expressible.
pub fn build_signed_status_document(
    signing_key: &ed25519_dalek::SigningKey,
    list_id: Option<&str>,
    version: i64,
    bits: &[u8],
) -> Result<Vec<u8>, String> {
    use ed25519_dalek::Signer;

    if bits.len() > MAX_BITS_BYTES {
        return Err(format!(
            "status list bitmap is {} bytes, over the {MAX_BITS_BYTES}-byte cap",
            bits.len()
        ));
    }
    let issuer = alexandria_verify::did::derive_did_key(signing_key);
    let list_id = list_id
        .map(str::to_string)
        .unwrap_or_else(|| format!("urn:alexandria:status-list:{}:1", issuer.as_str()));
    let sig = signing_key.sign(&canonical_status_bytes(
        &list_id,
        issuer.as_str(),
        version,
        bits,
    ));
    serde_json::to_vec(&serde_json::json!({
        "issuer": issuer.as_str(),
        "list_id": list_id,
        "version": version,
        "bits": base64::engine::general_purpose::STANDARD.encode(bits),
        "proof": base64::engine::general_purpose::STANDARD.encode(sig.to_bytes()),
    }))
    .map_err(|e| format!("serialize status document: {e}"))
}

/// Verify `proof` against the key the issuer's `did:key` resolves to.
fn issuer_signed(issuer: &str, proof: &str, signed: &[u8]) -> bool {
    let Ok(sig_bytes) = base64::engine::general_purpose::STANDARD.decode(proof.as_bytes()) else {
        return false;
    };
    let Ok(sig_array): Result<[u8; 64], _> = sig_bytes.try_into() else {
        return false;
    };
    let Ok(parsed) = alexandria_verify::did::parse_did_key(issuer) else {
        return false;
    };
    let Ok(vk) = alexandria_verify::did::resolve_did_key(&parsed) else {
        return false;
    };
    vk.verify_strict(signed, &ed25519_dalek::Signature::from_bytes(&sig_array))
        .is_ok()
}

pub fn handle_status_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<StatusIngest, String> {
    let parsed: StatusMessage = match serde_json::from_slice(&message.payload) {
        Ok(p) => p,
        Err(_) => return Ok(StatusIngest::IgnoredUnknownIssuer),
    };

    // Issuer must be known via the local key registry. Kept as a cheap
    // pre-filter — it means we do not spend a signature verification on a DID
    // nothing here has ever referenced — but it is no longer the authorisation
    // check. `TOPIC_VC_DID` lets any peer insert a row, so presence in that
    // table proves only that someone mentioned the DID.
    let known: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM key_registry WHERE did = ?1",
            rusqlite::params![&parsed.issuer],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if known == 0 {
        return Ok(StatusIngest::IgnoredUnknownIssuer);
    }

    let list_id = parsed
        .list_id
        .clone()
        .unwrap_or_else(|| format!("urn:alexandria:status-list:{}:1", parsed.issuer));

    // Refuse older versions to prevent rollback (§11.2). Checked before the
    // signature so a replayed old document costs nothing.
    let existing: Option<i64> = db
        .conn()
        .query_row(
            "SELECT version FROM credential_status_lists WHERE list_id = ?1",
            rusqlite::params![&list_id],
            |r| r.get(0),
        )
        .ok();
    if let Some(prev) = existing {
        if parsed.version <= prev {
            return Ok(StatusIngest::IgnoredNewer);
        }
    }

    let bits = base64::engine::general_purpose::STANDARD
        .decode(parsed.bits.as_bytes())
        .map_err(|e| format!("base64 decode bits: {e}"))?;
    if bits.len() > MAX_BITS_BYTES {
        return Err(format!(
            "status list bitmap is {} bytes, over the {MAX_BITS_BYTES}-byte cap",
            bits.len()
        ));
    }

    // The authorisation check. Only the issuer may say what its own
    // credentials' status is, so nothing below this line runs for a document
    // the issuer did not sign.
    let Some(proof) = parsed.proof.as_deref() else {
        log::warn!(
            "vc-status: unsigned status list for {} — dropping",
            parsed.issuer
        );
        return Ok(StatusIngest::IgnoredBadSignature);
    };
    let signed = canonical_status_bytes(&list_id, &parsed.issuer, parsed.version, &bits);
    if !issuer_signed(&parsed.issuer, proof, &signed) {
        log::warn!(
            "vc-status: status list for {} does not verify against that DID — dropping",
            parsed.issuer
        );
        return Ok(StatusIngest::IgnoredBadSignature);
    }
    let bit_length = (bits.len() as i64) * 8;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    db.conn()
        .execute(
            "INSERT INTO credential_status_lists \
             (list_id, issuer_did, version, status_purpose, bits, bit_length, updated_at) \
             VALUES (?1, ?2, ?3, 'revocation', ?4, ?5, ?6) \
             ON CONFLICT(list_id) DO UPDATE SET \
                version = excluded.version, \
                bits = excluded.bits, \
                bit_length = excluded.bit_length, \
                updated_at = excluded.updated_at",
            rusqlite::params![
                &list_id,
                &parsed.issuer,
                parsed.version,
                bits,
                bit_length,
                &now
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(StatusIngest::Applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn stub_msg(payload: &[u8]) -> SignedGossipMessage {
        SignedGossipMessage {
            topic: "/alexandria/vc-status/1.0".into(),
            payload: payload.to_vec(),
            signature: vec![0u8; 64],
            public_key: vec![0u8; 32],
            stake_address: "stake_test1...".into(),
            timestamp: 1_712_880_000,
            encrypted: false,
            key_id: None,
        }
    }

    fn issuer_key() -> SigningKey {
        SigningKey::from_bytes(&[3u8; 32])
    }

    /// Pre-register the issuer in `key_registry` so the handler's cheap
    /// pre-filter passes. Real propagation gets this from a prior
    /// `handle_did_message` call.
    fn register_issuer(db: &Database, did: &str) {
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO key_registry \
                 (did, key_id, public_key_hex, valid_from, valid_until, rotated_by) \
                 VALUES (?1, 'key-1', '', '1970-01-01T00:00:00Z', NULL, NULL)",
                rusqlite::params![did],
            )
            .unwrap();
    }

    /// A status document as a real issuer would publish it.
    fn signed_payload(sk: &SigningKey, version: i64, bits_b64: &str) -> Vec<u8> {
        let issuer = alexandria_verify::did::derive_did_key(sk);
        let list_id = format!("urn:alexandria:status-list:{}:1", issuer.as_str());
        let bits = base64::engine::general_purpose::STANDARD
            .decode(bits_b64)
            .unwrap();
        let sig = sk.sign(&canonical_status_bytes(
            &list_id,
            issuer.as_str(),
            version,
            &bits,
        ));
        serde_json::to_vec(&serde_json::json!({
            "issuer": issuer.as_str(),
            "version": version,
            "bits": bits_b64,
            "proof": base64::engine::general_purpose::STANDARD.encode(sig.to_bytes()),
        }))
        .unwrap()
    }

    fn db_with_issuer(sk: &SigningKey) -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        register_issuer(&db, alexandria_verify::did::derive_did_key(sk).as_str());
        db
    }

    #[test]
    fn a_status_list_the_issuer_signed_is_applied() {
        let sk = issuer_key();
        let db = db_with_issuer(&sk);
        let msg = stub_msg(&signed_payload(&sk, 1, "AQID"));
        assert_eq!(
            handle_status_message(&db, &msg).unwrap(),
            StatusIngest::Applied
        );
    }

    /// The finding: any peer could publish any issuer's status list. Setting
    /// every bit mass-revokes; zeroing them mass-*un*-revokes, which is the
    /// worse direction because a revoked credential silently returns to Accept.
    #[test]
    fn a_status_list_nobody_signed_is_dropped() {
        let sk = issuer_key();
        let db = db_with_issuer(&sk);
        let issuer = alexandria_verify::did::derive_did_key(&sk);
        let payload = serde_json::to_vec(&serde_json::json!({
            "issuer": issuer.as_str(),
            "version": 1,
            "bits": "//////8=",
        }))
        .unwrap();
        assert_eq!(
            handle_status_message(&db, &stub_msg(&payload)).unwrap(),
            StatusIngest::IgnoredBadSignature
        );
    }

    /// Signed by *a* key, just not the issuer's.
    #[test]
    fn a_status_list_signed_by_an_impostor_is_dropped() {
        let real = issuer_key();
        let db = db_with_issuer(&real);
        let impostor = SigningKey::from_bytes(&[9u8; 32]);

        let issuer = alexandria_verify::did::derive_did_key(&real);
        let list_id = format!("urn:alexandria:status-list:{}:1", issuer.as_str());
        let bits = vec![0xffu8; 4];
        let sig = impostor.sign(&canonical_status_bytes(&list_id, issuer.as_str(), 1, &bits));
        let payload = serde_json::to_vec(&serde_json::json!({
            "issuer": issuer.as_str(),
            "version": 1,
            "bits": base64::engine::general_purpose::STANDARD.encode(&bits),
            "proof": base64::engine::general_purpose::STANDARD.encode(sig.to_bytes()),
        }))
        .unwrap();

        assert_eq!(
            handle_status_message(&db, &stub_msg(&payload)).unwrap(),
            StatusIngest::IgnoredBadSignature
        );
    }

    /// The signature covers the bitmap, so flipping revocation bits after
    /// signing must invalidate it — otherwise a captured document could be
    /// edited in flight.
    #[test]
    fn tampering_with_the_bitmap_breaks_the_signature() {
        let sk = issuer_key();
        let db = db_with_issuer(&sk);
        let mut doc: serde_json::Value =
            serde_json::from_slice(&signed_payload(&sk, 1, "AQID")).unwrap();
        doc["bits"] = serde_json::json!("//////8=");
        let payload = serde_json::to_vec(&doc).unwrap();
        assert_eq!(
            handle_status_message(&db, &stub_msg(&payload)).unwrap(),
            StatusIngest::IgnoredBadSignature
        );
    }

    /// The version is signed too, so an old document cannot be replayed at a
    /// higher number to defeat the rollback guard.
    #[test]
    fn tampering_with_the_version_breaks_the_signature() {
        let sk = issuer_key();
        let db = db_with_issuer(&sk);
        let mut doc: serde_json::Value =
            serde_json::from_slice(&signed_payload(&sk, 1, "AQID")).unwrap();
        doc["version"] = serde_json::json!(9_000);
        let payload = serde_json::to_vec(&doc).unwrap();
        assert_eq!(
            handle_status_message(&db, &stub_msg(&payload)).unwrap(),
            StatusIngest::IgnoredBadSignature
        );
    }

    #[test]
    fn older_version_is_ignored() {
        // Spec §11.2: status lists are versioned; a lower or equal
        // version MUST NOT overwrite a newer one we already hold.
        let sk = issuer_key();
        let db = db_with_issuer(&sk);
        assert_eq!(
            handle_status_message(&db, &stub_msg(&signed_payload(&sk, 2, "AgID"))).unwrap(),
            StatusIngest::Applied
        );
        assert_eq!(
            handle_status_message(&db, &stub_msg(&signed_payload(&sk, 1, "AQID"))).unwrap(),
            StatusIngest::IgnoredNewer
        );
    }

    #[test]
    fn unknown_issuer_is_deferred() {
        // No registry entry ⇒ nothing here has ever referenced this DID, so
        // defer rather than spend a signature verification on it.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg(br#"{"issuer":"did:key:zUnknown","version":1,"bits":"AQ"}"#);
        assert_eq!(
            handle_status_message(&db, &msg).unwrap(),
            StatusIngest::IgnoredUnknownIssuer
        );
    }

    /// The builder is the only supported way to produce a document, so what it
    /// emits must be exactly what the handler accepts.
    #[test]
    fn the_builder_produces_a_document_the_handler_accepts() {
        let sk = issuer_key();
        let db = db_with_issuer(&sk);
        let doc = build_signed_status_document(&sk, None, 1, &[0x01, 0x02, 0x03]).unwrap();
        assert_eq!(
            handle_status_message(&db, &stub_msg(&doc)).unwrap(),
            StatusIngest::Applied
        );
    }

    #[test]
    fn an_oversized_bitmap_is_refused() {
        let sk = issuer_key();
        let db = db_with_issuer(&sk);
        let huge = base64::engine::general_purpose::STANDARD.encode(vec![0u8; MAX_BITS_BYTES + 1]);
        let payload = signed_payload(&sk, 1, &huge);
        assert!(handle_status_message(&db, &stub_msg(&payload)).is_err());
    }
}
