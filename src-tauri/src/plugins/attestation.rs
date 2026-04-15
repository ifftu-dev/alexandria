//! Alexandria Plugin DAO attestation: multi-sig credential-eligibility
//! gate for community plugins.
//!
//! Phase 3 of the community plugin system. The DAO is a single
//! top-level community body (per the user's plan decision); attestation
//! is **additive only** — once a `(plugin_cid, grader_cid)` is attested,
//! the row stays forever. Advisories ride on a separate table and never
//! invalidate prior credentials.
//!
//! Multi-sig today is "N-of-M individual Ed25519 signatures from the
//! committee" — a true threshold scheme (FROST etc.) is out of scope for
//! v1. The verifier checks that at least `threshold` distinct committee
//! signatures validate over the canonical signed bytes.
//!
//! ```text
//! signed_bytes = BLAKE3(plugin_cid || grader_cid || canonical_terms_json)
//! ```
//!
//! Default policy: 5-of-7. Configurable via `AttestationPolicy`. The
//! committee key set itself is shipped with the host binary and updated
//! through standard governance key-rotation events.

use std::collections::HashSet;

use ed25519_dalek::{Signature, VerifyingKey};
use rusqlite::{params, OptionalExtension};

use crate::crypto::hash::entity_id;
use crate::db::Database;
use crate::domain::plugin::{
    PluginAdvisoryRecord, PluginAttestationEvent, PluginAttestationStatus, StoredPluginAttestation,
};

/// Default attestation threshold. Can be raised in policy but never
/// silently lowered — the policy struct is explicit about this.
pub const DEFAULT_THRESHOLD: usize = 5;

#[derive(Debug, Clone)]
pub struct AttestationPolicy {
    pub threshold: usize,
}

impl Default for AttestationPolicy {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
        }
    }
}

/// Compute the canonical bytes the committee signs.
///
/// Format: `BLAKE3(plugin_cid_bytes || 0x00 || grader_cid_bytes || 0x00 || canonical_terms_json)`.
/// The `0x00` separators prevent any concatenation ambiguity if a CID
/// or terms field ever contains the other's bytes.
pub fn signed_bytes(
    plugin_cid: &str,
    grader_cid: &str,
    attestation_terms: &serde_json::Value,
) -> Result<[u8; 32], String> {
    let terms_bytes =
        serde_json::to_vec(attestation_terms).map_err(|e| format!("bad terms json: {e}"))?;
    let mut buf =
        Vec::with_capacity(plugin_cid.len() + 1 + grader_cid.len() + 1 + terms_bytes.len());
    buf.extend_from_slice(plugin_cid.as_bytes());
    buf.push(0);
    buf.extend_from_slice(grader_cid.as_bytes());
    buf.push(0);
    buf.extend_from_slice(&terms_bytes);
    Ok(*blake3::hash(&buf).as_bytes())
}

/// Verify that an attestation event has at least `policy.threshold`
/// distinct, valid committee signatures.
pub fn verify_event(
    event: &PluginAttestationEvent,
    policy: &AttestationPolicy,
) -> Result<(), String> {
    if event.signatures.len() != event.signer_indices.len() {
        return Err("attestation: signatures and signer_indices length mismatch".into());
    }
    if event.signatures.len() < policy.threshold {
        return Err(format!(
            "attestation has {} signatures, threshold is {}",
            event.signatures.len(),
            policy.threshold
        ));
    }

    let signed = signed_bytes(
        &event.plugin_cid,
        &event.grader_cid,
        &event.attestation_terms,
    )?;
    let mut seen_indices = HashSet::new();
    let mut valid = 0usize;

    for (sig_hex, &idx) in event.signatures.iter().zip(event.signer_indices.iter()) {
        let idx_usize = idx as usize;
        if !seen_indices.insert(idx) {
            return Err(format!("attestation: signer index {idx} appears twice"));
        }
        let pk_hex = event
            .committee_pubkeys
            .get(idx_usize)
            .ok_or_else(|| format!("attestation: signer index {idx} out of range"))?;
        let pk_bytes = hex_decode_32(pk_hex)
            .ok_or_else(|| format!("attestation: committee pubkey #{idx} is not 32 hex bytes"))?;
        let vk = VerifyingKey::from_bytes(&pk_bytes)
            .map_err(|e| format!("attestation: committee pubkey #{idx} is not Ed25519: {e}"))?;
        let sig_bytes = hex_decode_64(sig_hex)
            .ok_or_else(|| format!("attestation: signature #{idx} is not 64 hex bytes"))?;
        let sig = Signature::from_bytes(&sig_bytes);
        if vk.verify_strict(&signed, &sig).is_ok() {
            valid += 1;
        } else {
            return Err(format!("attestation: signature #{idx} did not verify"));
        }
    }

    if valid < policy.threshold {
        return Err(format!(
            "attestation has only {valid} valid signatures, threshold is {}",
            policy.threshold
        ));
    }
    Ok(())
}

/// Persist a verified attestation. Append-only — duplicates are no-ops.
pub fn persist_event(db: &Database, event: &PluginAttestationEvent) -> Result<(), String> {
    let pubkeys_json =
        serde_json::to_string(&event.committee_pubkeys).map_err(|e| e.to_string())?;
    let terms_json = serde_json::to_string(&event.attestation_terms).map_err(|e| e.to_string())?;
    let blob = serde_json::to_vec(&serde_json::json!({
        "signatures": &event.signatures,
        "signer_indices": &event.signer_indices,
    }))
    .map_err(|e| e.to_string())?;
    db.conn()
        .execute(
            "INSERT OR IGNORE INTO plugin_attestations \
             (plugin_cid, grader_cid, attestation_terms, threshold_signature_blob, \
              committee_pubkeys_json, issued_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.plugin_cid,
                event.grader_cid,
                terms_json,
                blob,
                pubkeys_json,
                event.issued_at,
            ],
        )
        .map_err(|e| format!("failed to persist attestation: {e}"))?;
    Ok(())
}

/// Insert a non-blocking advisory note. Does not affect prior attestations.
pub fn add_advisory(
    db: &Database,
    plugin_cid: &str,
    kind: &str,
    message: &str,
    committee_pubkeys: &[String],
    threshold_signature_blob: &[u8],
) -> Result<(), String> {
    if !matches!(kind, "deprecated" | "superseded" | "known_flawed") {
        return Err(format!("unknown advisory kind '{kind}'"));
    }
    let id = entity_id(&[plugin_cid, kind, message]);
    let pubkeys_json = serde_json::to_string(committee_pubkeys).map_err(|e| e.to_string())?;
    db.conn()
        .execute(
            "INSERT OR IGNORE INTO plugin_advisories \
             (id, plugin_cid, kind, message, threshold_signature_blob, committee_pubkeys_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                plugin_cid,
                kind,
                message,
                threshold_signature_blob,
                pubkeys_json
            ],
        )
        .map_err(|e| format!("failed to insert advisory: {e}"))?;
    Ok(())
}

/// Look up the attestation status for a single plugin.
pub fn status_for(db: &Database, plugin_cid: &str) -> Result<PluginAttestationStatus, String> {
    let attestation: Option<StoredPluginAttestation> = db
        .conn()
        .query_row(
            "SELECT plugin_cid, grader_cid, attestation_terms, committee_pubkeys_json, \
                    issued_at, advisory_kind, advisory_message \
             FROM plugin_attestations WHERE plugin_cid = ?1 LIMIT 1",
            params![plugin_cid],
            |row| {
                let terms_json: String = row.get(2)?;
                let pubkeys_json: String = row.get(3)?;
                Ok(StoredPluginAttestation {
                    plugin_cid: row.get(0)?,
                    grader_cid: row.get(1)?,
                    attestation_terms: serde_json::from_str(&terms_json)
                        .unwrap_or(serde_json::Value::Null),
                    committee_pubkeys: serde_json::from_str(&pubkeys_json).unwrap_or_default(),
                    issued_at: row.get(4)?,
                    advisory_kind: row.get(5)?,
                    advisory_message: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, plugin_cid, kind, message, issued_at \
             FROM plugin_advisories WHERE plugin_cid = ?1 ORDER BY issued_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let advisories = stmt
        .query_map(params![plugin_cid], |row| {
            Ok(PluginAdvisoryRecord {
                id: row.get(0)?,
                plugin_cid: row.get(1)?,
                kind: row.get(2)?,
                message: row.get(3)?,
                issued_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(PluginAttestationStatus {
        plugin_cid: plugin_cid.to_string(),
        attested: attestation.is_some(),
        attestation,
        advisories,
    })
}

fn hex_decode_32(s: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(s).ok()?;
    bytes.try_into().ok()
}

fn hex_decode_64(s: &str) -> Option<[u8; 64]> {
    let bytes = hex::decode(s).ok()?;
    bytes.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use rand::Rng;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("db");
        db.run_migrations().expect("migrations");
        db
    }

    fn make_committee(n: usize) -> Vec<SigningKey> {
        (0..n).map(|_| SigningKey::generate(&mut OsRng)).collect()
    }

    fn make_event(
        committee: &[SigningKey],
        signers: &[usize],
        plugin_cid: &str,
        grader_cid: &str,
    ) -> PluginAttestationEvent {
        let pubkeys: Vec<String> = committee
            .iter()
            .map(|sk| hex::encode(sk.verifying_key().as_bytes()))
            .collect();
        let terms = serde_json::json!({"version": "1", "scope": "credential-eligible"});
        let signed = signed_bytes(plugin_cid, grader_cid, &terms).unwrap();
        let mut sigs = Vec::new();
        let mut indices = Vec::new();
        for &i in signers {
            let sig = committee[i].sign(&signed);
            sigs.push(hex::encode(sig.to_bytes()));
            indices.push(i as u32);
        }
        PluginAttestationEvent {
            plugin_cid: plugin_cid.to_string(),
            grader_cid: grader_cid.to_string(),
            attestation_terms: terms,
            committee_pubkeys: pubkeys,
            signatures: sigs,
            signer_indices: indices,
            issued_at: "2026-04-15T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn five_of_seven_verifies() {
        let committee = make_committee(7);
        let event = make_event(&committee, &[0, 1, 2, 3, 4], "plug", "grade");
        verify_event(&event, &AttestationPolicy::default()).expect("5-of-7 must verify");
    }

    #[test]
    fn four_of_seven_below_threshold_rejected() {
        let committee = make_committee(7);
        let event = make_event(&committee, &[0, 1, 2, 3], "plug", "grade");
        let err = verify_event(&event, &AttestationPolicy::default());
        assert!(err.is_err(), "4-of-7 must fail (threshold 5)");
    }

    #[test]
    fn duplicate_signer_rejected() {
        let committee = make_committee(7);
        let mut event = make_event(&committee, &[0, 1, 2, 3, 4], "plug", "grade");
        // Tamper: re-use index 0.
        event.signer_indices[1] = 0;
        let err = verify_event(&event, &AttestationPolicy::default());
        assert!(err.is_err());
    }

    #[test]
    fn tampered_terms_rejected() {
        let committee = make_committee(7);
        let mut event = make_event(&committee, &[0, 1, 2, 3, 4], "plug", "grade");
        // Tamper after signing.
        event.attestation_terms = serde_json::json!({"version": "1", "scope": "EVIL"});
        let err = verify_event(&event, &AttestationPolicy::default());
        assert!(err.is_err());
    }

    #[test]
    fn signature_from_non_committee_rejected() {
        let committee = make_committee(7);
        let interloper = SigningKey::from_bytes(&{
            let mut b = [0u8; 32];
            OsRng.fill(&mut b);
            b
        });
        let mut event = make_event(&committee, &[0, 1, 2, 3, 4], "plug", "grade");
        // Replace committee[0]'s signature with the interloper's.
        let signed = signed_bytes(
            &event.plugin_cid,
            &event.grader_cid,
            &event.attestation_terms,
        )
        .unwrap();
        let sig = interloper.sign(&signed);
        event.signatures[0] = hex::encode(sig.to_bytes());
        let err = verify_event(&event, &AttestationPolicy::default());
        assert!(err.is_err());
    }

    #[test]
    fn persist_then_status() {
        let db = test_db();
        let committee = make_committee(7);
        let event = make_event(&committee, &[0, 1, 2, 3, 4], "plug-cid", "grade-cid");
        persist_event(&db, &event).unwrap();
        let st = status_for(&db, "plug-cid").unwrap();
        assert!(st.attested);
        assert_eq!(st.attestation.as_ref().unwrap().grader_cid, "grade-cid");
    }

    #[test]
    fn advisory_does_not_attest() {
        let db = test_db();
        add_advisory(&db, "plug-cid", "deprecated", "use v2", &[], &[]).unwrap();
        let st = status_for(&db, "plug-cid").unwrap();
        assert!(!st.attested);
        assert_eq!(st.advisories.len(), 1);
        assert_eq!(st.advisories[0].kind, "deprecated");
    }
}
