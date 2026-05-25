//! Deterministic credential IDs per protocol spec §3.3.
//!
//! Every issued VC gets an `id` derived from its content + issuance slot
//! rather than a random UUID. This buys:
//!
//! - **Dedup across the gossip mesh** — two peers re-broadcasting the
//!   same VC collapse to one row keyed by `id`, no application-level
//!   dedup table needed.
//! - **Replayability** — given the same inputs (issuer, subject, claim,
//!   `validFrom`, status-list slot) a verifier can re-derive the id and
//!   detect tampering of the id field itself.
//! - **Spec conformance** — §3.3 mandates `hex(blake2b_256(parts.join("|")))`
//!   for all entity ids; the credential id is no exception.
//!
//! The status-list `(list_id, index)` pair is included to keep the id
//! unique across legitimate re-issuance of the same claim for the same
//! subject (e.g. renewing a role-assignment credential): a new slot is
//! always allocated before id derivation, so renewals never collide
//! with the original.

use super::{canonicalize::canonicalize, Claim};
use crate::crypto::did::Did;
use crate::crypto::hash::{blake2b_256, entity_id};

/// Build the deterministic id for a VC about to be issued.
///
/// The status-list slot must already be allocated by the caller — the
/// `(list_id, index)` pair is part of the hash input, so it must be
/// stable for the lifetime of this id.
pub fn deterministic_credential_id(
    issuer_did: &Did,
    subject_did: &Did,
    claim: &Claim,
    valid_from: &str,
    status_list_id: &str,
    status_list_index: i64,
) -> Result<String, String> {
    let claim_value = serde_json::to_value(claim).map_err(|e| format!("claim serialize: {e}"))?;
    let claim_canonical =
        canonicalize(&claim_value).map_err(|e| format!("claim canonicalize: {e}"))?;
    let claim_digest = hex::encode(blake2b_256(&claim_canonical));
    let index_str = status_list_index.to_string();
    let id_hex = entity_id(&[
        // Domain separator — bump if the id derivation changes so old
        // and new ids can never collide.
        "vc:v1",
        issuer_did.as_str(),
        subject_did.as_str(),
        claim.kind_str(),
        &claim_digest,
        valid_from,
        status_list_id,
        &index_str,
    ]);
    Ok(format!("urn:alexandria:vc:{id_hex}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::vc::{CustomClaim, SkillClaim};

    fn did(s: &str) -> Did {
        Did(s.to_string())
    }

    fn skill_claim() -> Claim {
        Claim::Skill(SkillClaim {
            skill_id: "rust.async".into(),
            level: 3,
            score: 0.85,
            evidence_refs: vec![],
            rubric_version: None,
            assessment_method: None,
        })
    }

    fn custom_claim_a() -> Claim {
        Claim::Custom(CustomClaim {
            properties: serde_json::Map::from_iter([
                ("a".into(), serde_json::json!(1)),
                ("b".into(), serde_json::json!(2)),
            ]),
        })
    }

    fn custom_claim_b_same_keys_different_order() -> Claim {
        Claim::Custom(CustomClaim {
            properties: serde_json::Map::from_iter([
                ("b".into(), serde_json::json!(2)),
                ("a".into(), serde_json::json!(1)),
            ]),
        })
    }

    #[test]
    fn same_inputs_produce_same_id() {
        let id1 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zSubject"),
            &skill_claim(),
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            42,
        )
        .unwrap();
        let id2 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zSubject"),
            &skill_claim(),
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            42,
        )
        .unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn id_has_expected_shape() {
        let id = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zSubject"),
            &skill_claim(),
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            0,
        )
        .unwrap();
        // urn:alexandria:vc:<64 hex chars>
        assert!(id.starts_with("urn:alexandria:vc:"), "got {id}");
        let hex_part = id.trim_start_matches("urn:alexandria:vc:");
        assert_eq!(hex_part.len(), 64, "expected 64-hex digest, got {hex_part}");
        assert!(hex_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn different_subject_changes_id() {
        let id1 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zAlice"),
            &skill_claim(),
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            0,
        )
        .unwrap();
        let id2 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zBob"),
            &skill_claim(),
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            0,
        )
        .unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn different_slot_changes_id() {
        let id1 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zSubject"),
            &skill_claim(),
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            0,
        )
        .unwrap();
        let id2 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zSubject"),
            &skill_claim(),
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            1,
        )
        .unwrap();
        assert_ne!(id1, id2, "slot change must yield distinct id");
    }

    #[test]
    fn different_valid_from_changes_id() {
        let id1 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zSubject"),
            &skill_claim(),
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            0,
        )
        .unwrap();
        let id2 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zSubject"),
            &skill_claim(),
            "2026-05-25T10:00:01Z",
            "urn:status:list:1",
            0,
        )
        .unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn claim_property_order_does_not_change_id() {
        // JCS canonicalization sorts object keys — re-ordering the same
        // properties at the call site must not change the id.
        let id1 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zSubject"),
            &custom_claim_a(),
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            0,
        )
        .unwrap();
        let id2 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zSubject"),
            &custom_claim_b_same_keys_different_order(),
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            0,
        )
        .unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn different_claim_kind_changes_id() {
        let skill = skill_claim();
        let custom = custom_claim_a();
        let id1 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zSubject"),
            &skill,
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            0,
        )
        .unwrap();
        let id2 = deterministic_credential_id(
            &did("did:key:zIssuer"),
            &did("did:key:zSubject"),
            &custom,
            "2026-05-25T10:00:00Z",
            "urn:status:list:1",
            0,
        )
        .unwrap();
        assert_ne!(id1, id2);
    }
}
