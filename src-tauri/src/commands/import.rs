// SPDX-License-Identifier: MIT
//! Accept a credential somebody handed you.
//!
//! Every other way a credential enters the local store either mints it here
//! (`commands::credentials::issue_credential`, signed with *this* identity) or
//! pulls it from a peer over P2P. Neither covers the ordinary case of being
//! given a credential out of band — a file, a download, a blob fetched by
//! hash — which is how an issuer that is not a peer delivers one.
//!
//! This is MIT and unconditional. Receiving and verifying a credential is the
//! community product; nothing about accepting one is commercial. It is also
//! deliberately type-agnostic: it stores any valid VC, and leaves policy about
//! *what a credential means* to the readers. `get_entitlement_snapshot` decides
//! whether an entitlement counts; this only decides whether it is real.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::domain::vc::verify::verify_credential;
use crate::domain::vc::{
    AcceptanceDecision, CredentialSubject, EntitlementClaim, RoleClaim, SkillClaim,
    VerifiableCredential, VerificationPolicy,
};
use crate::AppState;

/// Outcome of an import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    /// Envelope id of the credential now in the store.
    pub credential_id: String,
    /// False when a credential with this id was already present. Import is
    /// idempotent, so re-importing is a success, not an error — but the caller
    /// usually wants to say "already installed" rather than "installed".
    pub stored: bool,
}

/// The policy an imported credential must satisfy.
///
/// `allowed_types` is empty on purpose, which means "no type constraint". This
/// is a general inbox: an entitlement, a membership role, a skill credential
/// from an institution are all legitimate things to be handed, and deciding
/// which ones *matter* belongs to whoever reads them later. Everything else is
/// strict — an expired, revoked, suspended or superseded credential is not
/// worth storing.
fn import_policy() -> VerificationPolicy {
    VerificationPolicy {
        reject_expired: true,
        require_integrity_anchor: false,
        allowed_types: vec![],
        reject_suspended: true,
        reject_superseded: true,
    }
}

/// The `claim_kind` column value for a subject, mirroring `Claim::kind_str`.
///
/// An imported credential arrives in its on-disk W3C shape with the claim
/// inlined in the subject, so the discriminator has to be recovered from the
/// marker properties rather than read off a `Claim` enum. Order matters: the
/// markers are distinct (`skillId`, `role`, `entitlementPlan`), and anything
/// carrying none of them is a custom claim, which is legitimate.
fn claim_kind_of(subject: &CredentialSubject) -> &'static str {
    if SkillClaim::extract(subject).is_some() {
        "skill"
    } else if RoleClaim::extract(subject).is_some() {
        "role"
    } else if EntitlementClaim::extract(subject).is_some() {
        "entitlement"
    } else {
        "custom"
    }
}

/// Verify a credential and store it.
///
/// Verification happens **before** the insert and a failure is an error, not a
/// stored-but-flagged row. A credential that does not verify is not a
/// credential, and letting one land in the table would put the burden of
/// re-checking on every future reader — one of which would eventually forget.
///
/// Idempotent on the envelope `id`: re-importing the same credential reports
/// success with `stored: false` rather than raising a constraint violation.
/// Delivery channels retry, and a customer who clicks the same link twice has
/// not done anything wrong.
pub fn import_credential_impl(
    conn: &rusqlite::Connection,
    vc: &VerifiableCredential,
    now: &str,
) -> Result<ImportOutcome, String> {
    let credential_id = vc
        .id
        .clone()
        .ok_or_else(|| "credential has no envelope id".to_string())?;

    let result = verify_credential(conn, vc, now, &import_policy());
    if result.acceptance_decision != AcceptanceDecision::Accept {
        // Report the failed checks rather than a bare "invalid" — an import
        // that fails for an expired credential and one that fails for a bad
        // signature call for very different responses from the user.
        let mut why = Vec::new();
        if !result.valid_signature {
            why.push("signature");
        }
        if !result.issuer_resolved {
            why.push("issuer not resolvable");
        }
        if !result.subject_bound {
            why.push("subject not a DID");
        }
        if result.revoked {
            why.push("revoked");
        }
        if result.expired {
            why.push("expired");
        }
        if result.suspended {
            why.push("suspended");
        }
        if result.superseded {
            why.push("superseded");
        }
        if why.is_empty() {
            why.push("rejected by policy");
        }
        return Err(format!("credential did not verify: {}", why.join(", ")));
    }

    let already: Option<String> = conn
        .query_row(
            "SELECT id FROM credentials WHERE id = ?1",
            params![credential_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if already.is_some() {
        return Ok(ImportOutcome {
            credential_id,
            stored: false,
        });
    }

    // Derive the denormalized columns from the credential body. Nothing here
    // is trusted input in its own right — every one of these is recomputed
    // from the payload whose signature was just verified.
    let claim_kind = claim_kind_of(&vc.credential_subject);
    let skill_id = SkillClaim::extract(&vc.credential_subject).map(|s| s.skill_id);
    let provenance = SkillClaim::extract(&vc.credential_subject)
        .and_then(|s| s.provenance)
        .map(|p| p.as_str().to_string());
    let credential_type = vc
        .type_
        .iter()
        .find(|t| t.as_str() != "VerifiableCredential")
        .cloned()
        .ok_or_else(|| "credential names no class beyond VerifiableCredential".to_string())?;
    let (list_id, list_index) = match &vc.credential_status {
        Some(s) => (
            Some(s.status_list_credential.clone()),
            Some(s.status_list_index.clone()),
        ),
        None => (None, None),
    };
    // Same derivation `issue_credential` uses, reused rather than reimplemented
    // so an imported credential and a locally issued one hash identically.
    let integrity_hash = super::credentials::integrity_hash_of(vc)?;
    let signed_json = serde_json::to_string(vc).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO credentials \
         (id, issuer_did, subject_did, credential_type, claim_kind, skill_id, \
          issuance_date, expiration_date, signed_vc_json, integrity_hash, \
          status_list_id, status_list_index, provenance) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            credential_id,
            vc.issuer.as_str(),
            vc.credential_subject.id.as_str(),
            credential_type,
            claim_kind,
            skill_id,
            vc.valid_from,
            vc.valid_until,
            signed_json,
            integrity_hash,
            list_id,
            list_index,
            provenance,
        ],
    )
    .map_err(|e| format!("insert imported credential: {e}"))?;

    Ok(ImportOutcome {
        credential_id,
        stored: true,
    })
}

#[tauri::command]
pub async fn import_credential(
    state: State<'_, AppState>,
    credential: VerifiableCredential,
) -> Result<ImportOutcome, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let now = super::credentials::now_rfc3339();
    import_credential_impl(db.conn(), &credential, &now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::{derive_did_key, Did, VerificationMethodRef};
    use crate::db::Database;
    use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
    use crate::domain::vc::{CredentialType, Proof};
    use ed25519_dalek::SigningKey;

    const NOW: &str = "2026-04-13T00:00:00Z";

    fn key(seed: &str) -> SigningKey {
        let mut bytes = [0u8; 32];
        let s = seed.as_bytes();
        bytes[..s.len().min(32)].copy_from_slice(&s[..s.len().min(32)]);
        SigningKey::from_bytes(&bytes)
    }

    fn open_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    /// A signed credential of `class` carrying `properties`, never stored.
    fn signed_vc(
        issuer_key: &SigningKey,
        class: CredentialType,
        id: &str,
        properties: serde_json::Value,
        valid_until: Option<&str>,
    ) -> VerifiableCredential {
        let issuer = derive_did_key(issuer_key);
        let vc = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some(id.to_string()),
            type_: vec!["VerifiableCredential".into(), class.as_str().to_string()],
            issuer: issuer.clone(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_until: valid_until.map(str::to_string),
            credential_subject: CredentialSubject {
                id: Did("did:key:z6MkTheSubject".into()),
                properties: properties.as_object().unwrap().clone(),
            },
            credential_status: None,
            terms_of_use: None,
            witness: None,
            integrity: None,
            proof: Proof {
                type_: "Ed25519Signature2020".into(),
                created: "2026-01-01T00:00:00Z".into(),
                verification_method: VerificationMethodRef(format!("{}#key-1", issuer.as_str())),
                proof_purpose: "assertionMethod".into(),
                jws: String::new(),
            },
        };
        sign_credential(UnsignedCredential { credential: vc }, issuer_key, &issuer).unwrap()
    }

    fn entitlement_props() -> serde_json::Value {
        serde_json::json!({
            "orgId": "org-1",
            "entitlementPlan": "enterprise",
            "seats": 10,
            "features": ["talent_index"],
        })
    }

    #[test]
    fn a_valid_credential_is_stored() {
        let db = open_db();
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:1",
            entitlement_props(),
            None,
        );

        let outcome = import_credential_impl(db.conn(), &vc, NOW).unwrap();
        assert!(outcome.stored);
        assert_eq!(outcome.credential_id, "urn:test:import:1");

        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credentials WHERE id = ?1",
                params!["urn:test:import:1"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    /// A tampered credential must not reach the table at all. Storing it and
    /// flagging it would push the re-check onto every future reader.
    #[test]
    fn a_tampered_credential_is_refused_and_not_stored() {
        let db = open_db();
        let mut vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:2",
            entitlement_props(),
            None,
        );
        // Grant an extra feature without re-signing.
        let mut claim = EntitlementClaim::extract(&vc.credential_subject).unwrap();
        claim.features.push("employer_console".into());
        vc.credential_subject.properties = claim.into_properties();

        let err = import_credential_impl(db.conn(), &vc, NOW).unwrap_err();
        assert!(err.contains("signature"), "unexpected reason: {err}");

        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn an_expired_credential_is_refused() {
        let db = open_db();
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:3",
            entitlement_props(),
            Some("2026-02-01T00:00:00Z"),
        );

        let err = import_credential_impl(db.conn(), &vc, NOW).unwrap_err();
        assert!(err.contains("expired"), "unexpected reason: {err}");
    }

    /// Delivery channels retry and customers click links twice.
    #[test]
    fn importing_the_same_credential_twice_is_idempotent() {
        let db = open_db();
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:4",
            entitlement_props(),
            None,
        );

        assert!(import_credential_impl(db.conn(), &vc, NOW).unwrap().stored);
        let second = import_credential_impl(db.conn(), &vc, NOW).unwrap();
        assert!(!second.stored, "second import must not duplicate");

        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    /// The denormalized columns must be recomputed from the signed body, since
    /// readers filter on them. `get_entitlement_snapshot` queries by
    /// `credential_type`, so getting this wrong would make an imported
    /// entitlement invisible.
    #[test]
    fn denormalized_columns_are_derived_from_the_credential_body() {
        let db = open_db();
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:5",
            entitlement_props(),
            None,
        );
        import_credential_impl(db.conn(), &vc, NOW).unwrap();

        let (ctype, kind, issuer, subject): (String, String, String, String) = db
            .conn()
            .query_row(
                "SELECT credential_type, claim_kind, issuer_did, subject_did \
                 FROM credentials WHERE id = ?1",
                params!["urn:test:import:5"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(ctype, CredentialType::EntitlementCredential.as_str());
        assert_eq!(kind, "entitlement");
        assert_eq!(issuer, vc.issuer.as_str());
        assert_eq!(subject, "did:key:z6MkTheSubject");
    }

    /// Role credentials import too — Unit 2's per-org route needs membership
    /// credentials to arrive by the same door as entitlements.
    #[test]
    fn a_role_credential_imports_with_the_role_claim_kind() {
        let db = open_db();
        let vc = signed_vc(
            &key("org"),
            CredentialType::RoleCredential,
            "urn:test:import:6",
            serde_json::json!({ "role": "member", "scope": "did:key:z6MkOrg" }),
            None,
        );
        import_credential_impl(db.conn(), &vc, NOW).unwrap();

        let kind: String = db
            .conn()
            .query_row(
                "SELECT claim_kind FROM credentials WHERE id = ?1",
                params!["urn:test:import:6"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "role");
    }

    /// The import policy imposes no type constraint on purpose — this is a
    /// general inbox, not an entitlement-specific one.
    #[test]
    fn import_policy_accepts_any_credential_class() {
        assert!(
            import_policy().allowed_types.is_empty(),
            "an empty allowed_types is what makes this a general inbox"
        );
    }

    #[test]
    fn a_credential_without_an_envelope_id_is_refused() {
        let db = open_db();
        let mut vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:7",
            entitlement_props(),
            None,
        );
        vc.id = None;

        let err = import_credential_impl(db.conn(), &vc, NOW).unwrap_err();
        assert!(err.contains("envelope id"), "unexpected reason: {err}");
    }
}
