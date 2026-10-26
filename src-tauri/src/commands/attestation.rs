//! Exact course-completion endorsement commands: requesting, signing and
//! importing endorsements, and the trust of a stored credential.
//!
//! Policies come only from the exact author-signed course document selected by
//! the enrollment. The endorsement store, which verifies an endorsement before
//! storing it, is [`crate::evidence::completion`].

use crate::profile::scope::ProfileState as State;
use rusqlite::Connection;

use alexandria_verify::course::sign_completion_endorsement;
use alexandria_verify::trust::{classify_credential, CredentialTrust};

use crate::commands::credentials::get_credential_impl;
use crate::content_store::course as content_course;
use crate::crypto::wallet;
use crate::db::executor::DatabaseWorkload;
use crate::domain::attestation::{
    CourseCompletionBinding, CourseCompletionEndorsement, CourseCompletionEndorsementStatus,
};
use crate::domain::vc::{verify_credential_db, VerificationPolicy};
use crate::evidence::completion::{
    self, completion_endorsement_status_impl, import_completion_endorsement_json_impl,
    stored_completion_evidence, StoredCompletionEvidence,
};
use crate::network_profile::embedded_preprod;
use crate::AppState;

/// Classify a stored credential from its signed bytes and, for a completion
/// self-claim, the exact endorsement evidence of the claim that lists it.
///
/// Lookup failures and corrupt or ambiguous stored completion evidence are
/// errors: they must not collapse into a settled-looking lower trust state.
pub fn credential_trust_impl(
    conn: &Connection,
    credential_id: &str,
    verification_time: &str,
    network_id: &str,
) -> Result<Option<CredentialTrust>, String> {
    let Some(credential) = get_credential_impl(conn, credential_id)? else {
        return Ok(None);
    };
    let policy = VerificationPolicy::default();
    let verification = verify_credential_db(conn, &credential, verification_time, &policy);
    let evidence = stored_completion_evidence(conn, credential_id)?;
    Ok(Some(classify_credential(
        &credential,
        &verification,
        &policy,
        network_id,
        evidence.as_ref().map(StoredCompletionEvidence::as_evidence),
    )))
}

#[tauri::command]
pub async fn get_credential_trust(
    state: State<'_, AppState>,
    credential_id: String,
) -> Result<Option<CredentialTrust>, String> {
    let network_id = embedded_preprod()
        .map_err(|error| error.to_string())?
        .network_id
        .clone();
    let verification_time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.trust",
            move |db| {
                credential_trust_impl(db.conn(), &credential_id, &verification_time, &network_id)
            },
        )
        .await
}

#[tauri::command]
pub async fn get_course_completion_endorsement_request(
    state: State<'_, AppState>,
    claim_id: String,
) -> Result<CourseCompletionBinding, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "completion.endorsement.request",
            move |db| completion::endorsement_request(db.conn(), &claim_id),
        )
        .await
}

fn verify_portable_course_document(
    json: &str,
    expected_cid: &str,
) -> Result<crate::domain::course_document::SignedCourseDocument, String> {
    let document = content_course::decode_course_document(json.as_bytes())
        .map_err(|error| error.to_string())?;
    if blake3::hash(json.as_bytes()).to_hex().as_str() != expected_cid {
        return Err("portable course document does not match the requested content hash".into());
    }
    content_course::verify_course_document(&document).map_err(|error| error.to_string())?;
    Ok(document)
}

#[tauri::command]
pub async fn sign_course_completion_endorsement(
    state: State<'_, AppState>,
    binding: CourseCompletionBinding,
    course_document_json: Option<String>,
) -> Result<CourseCompletionEndorsement, String> {
    let document = if let Some(json) = course_document_json {
        verify_portable_course_document(&json, &binding.course_document_cid)?
    } else {
        content_course::resolve_course_document(&state.content_node, &binding.course_document_cid)
            .await
            .map_err(|error| format!("resolve exact course document: {error}"))?
    };
    if document.course_id != binding.course_id
        || document.version != binding.course_document_version
    {
        return Err("endorsement request does not match the exact course document".into());
    }
    let policy = document
        .completion_policy
        .ok_or("exact course document has no completion endorsement policy")?;
    let keystore = state.keystore.lock().await;
    let mnemonic = keystore
        .as_ref()
        .ok_or("vault is locked — unlock first")?
        .retrieve_mnemonic()
        .map_err(|error| error.to_string())?;
    drop(keystore);
    let instructor = wallet::wallet_from_mnemonic(&mnemonic).map_err(|error| error.to_string())?;
    sign_completion_endorsement(&policy, binding, &instructor.signing_key)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn import_course_completion_endorsement(
    state: State<'_, AppState>,
    claim_id: String,
    endorsement_json: String,
) -> Result<CourseCompletionEndorsement, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "completion.endorsement.import",
            move |db| {
                import_completion_endorsement_json_impl(
                    db.conn(),
                    &claim_id,
                    endorsement_json.as_bytes(),
                )
            },
        )
        .await
}

#[tauri::command]
pub async fn get_course_completion_endorsement_status(
    state: State<'_, AppState>,
    claim_id: String,
) -> Result<CourseCompletionEndorsementStatus, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "completion.endorsement.status",
            move |db| completion_endorsement_status_impl(db.conn(), &claim_id),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::completion::fixtures::fixture;
    use crate::evidence::completion::import_completion_endorsement_impl;
    use alexandria_verify::did::did_from_verifying_key;
    use ed25519_dalek::SigningKey;
    use rusqlite::params;

    #[test]
    fn portable_course_requires_exact_bytes_and_author_signature() {
        let key = SigningKey::from_bytes(&[42; 32]);
        let payload = crate::domain::course_document::CourseDocumentPayload {
            version: 2,
            course_id: "demo-course".into(),
            author_address: "demo-author".into(),
            author_did: Some(did_from_verifying_key(&key.verifying_key())),
            title: "Public demo course".into(),
            description: None,
            thumbnail_hash: None,
            tags: vec![],
            skill_ids: vec![],
            chapters: vec![],
            created_at: 1,
            updated_at: 1,
            kind: "course".into(),
            completion_policy: None,
            tutor_policy: Default::default(),
        };
        let mut signed = content_course::sign_course_document(&payload, &key).unwrap();
        let json = serde_json::to_string(&signed).unwrap();
        let cid = blake3::hash(json.as_bytes()).to_hex().to_string();
        assert!(verify_portable_course_document(&json, &cid).is_ok());
        assert!(verify_portable_course_document(&json, &"00".repeat(32)).is_err());
        signed.title = "Tampered".into();
        let tampered = serde_json::to_string(&signed).unwrap();
        let tampered_cid = blake3::hash(tampered.as_bytes()).to_hex().to_string();
        assert!(verify_portable_course_document(&tampered, &tampered_cid).is_err());
    }

    const CREDENTIAL_ID: &str = "urn:uuid:completion-self-claim";
    const NOW: &str = "2026-09-15T00:00:00Z";

    fn store_self_claim(db: &crate::db::Database, binding: &CourseCompletionBinding) {
        use alexandria_verify::trust::{
            completion_root_evidence_ref, course_document_evidence_ref,
        };
        use alexandria_verify::vc::sign::{sign_credential, UnsignedCredential};
        use alexandria_verify::vc::{Claim, Proof, SkillClaim, VerifiableCredential};

        let subject = SigningKey::from_bytes(&[9; 32]);
        let subject_did = did_from_verifying_key(&subject.verifying_key());
        assert_eq!(subject_did, binding.subject_did);
        let claim = Claim::Skill(SkillClaim {
            skill_id: "skill".into(),
            level: 3,
            score: 0.8,
            evidence_refs: vec![
                course_document_evidence_ref(
                    &binding.course_document_cid,
                    binding.course_document_version,
                ),
                completion_root_evidence_ref(&binding.completion_root),
            ],
            rubric_version: None,
            assessment_method: Some("course_completion".into()),
            provenance: None,
        });
        let unsigned = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some(CREDENTIAL_ID.into()),
            type_: vec!["VerifiableCredential".into(), "SelfAssertion".into()],
            issuer: subject_did.clone(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_until: None,
            credential_subject: claim.into_subject(subject_did.clone()),
            credential_status: None,
            terms_of_use: None,
            witness: None,
            integrity: None,
            proof: Proof::unsigned("2026-01-01T00:00:00Z"),
        };
        let credential = sign_credential(
            UnsignedCredential {
                credential: unsigned,
            },
            &subject,
            &subject_did,
        )
        .unwrap();
        db.conn()
            .execute(
                "INSERT INTO credentials (id, issuer_did, subject_did, credential_type, \
                 claim_kind, skill_id, issuance_date, signed_vc_json, integrity_hash, revoked) \
                 VALUES (?1, ?2, ?2, 'SelfAssertion', 'skill', 'skill', \
                         '2026-01-01T00:00:00Z', ?3, ?1, 0)",
                params![
                    CREDENTIAL_ID,
                    subject_did.as_str(),
                    serde_json::to_string(&credential).unwrap()
                ],
            )
            .unwrap();
    }

    fn list_credential_on_claim(db: &crate::db::Database, claim_id: &str) {
        db.conn()
            .execute(
                "UPDATE completion_claims SET credential_ids_json = json_array(?1) WHERE id = ?2",
                params![CREDENTIAL_ID, claim_id],
            )
            .unwrap();
    }

    #[test]
    fn completion_self_claim_trust_follows_the_exact_endorsement_threshold() {
        use alexandria_verify::trust::{EndorsementMismatch, EndorsementOutcome};

        let (db, policy, binding, first, second) = fixture();
        store_self_claim(&db, &binding);
        let trust = |network: &str| {
            credential_trust_impl(db.conn(), CREDENTIAL_ID, NOW, network)
                .unwrap()
                .expect("stored credential")
        };
        let self_claim = |endorsement| CredentialTrust::VerifiedSelfClaim { endorsement };

        // Not listed by any claim: no completion evidence applies.
        assert_eq!(
            trust("preprod"),
            self_claim(EndorsementOutcome::NotSupplied)
        );

        list_credential_on_claim(&db, "claim");
        assert_eq!(
            trust("preprod"),
            self_claim(EndorsementOutcome::ThresholdUnmet {
                required_attestors: 2,
                valid_attestors: 0,
            })
        );

        let first_endorsement =
            sign_completion_endorsement(&policy, binding.clone(), &first).unwrap();
        import_completion_endorsement_impl(db.conn(), "claim", &first_endorsement).unwrap();
        assert_eq!(
            trust("preprod"),
            self_claim(EndorsementOutcome::ThresholdUnmet {
                required_attestors: 2,
                valid_attestors: 1,
            })
        );

        let second_endorsement =
            sign_completion_endorsement(&policy, binding.clone(), &second).unwrap();
        import_completion_endorsement_impl(db.conn(), "claim", &second_endorsement).unwrap();
        match trust("preprod") {
            CredentialTrust::VerifiedCourseEndorsement {
                course_document_cid,
                course_document_version,
                attestors,
                ..
            } => {
                assert_eq!(course_document_cid, binding.course_document_cid);
                assert_eq!(course_document_version, 2);
                assert_eq!(attestors.len(), 2);
            }
            other => panic!("expected an exact course endorsement, got {other:?}"),
        }

        // A build for another network does not honour these endorsements.
        assert_eq!(
            trust("mainnet"),
            self_claim(EndorsementOutcome::NotApplicable {
                reason: EndorsementMismatch::WrongNetwork,
            })
        );
        assert!(
            credential_trust_impl(db.conn(), "urn:uuid:missing", NOW, "preprod")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn corrupt_or_ambiguous_completion_evidence_is_an_error_not_a_lower_trust_state() {
        let (db, _, binding, _, _) = fixture();
        store_self_claim(&db, &binding);
        list_credential_on_claim(&db, "claim");

        db.conn()
            .execute(
                "INSERT INTO completion_claims \
                 (id, subject_did, course_id, completion_root, credential_ids_json) \
                 VALUES ('other-claim', ?1, 'course', ?2, json_array(?3))",
                params![binding.subject_did.as_str(), "33".repeat(32), CREDENTIAL_ID],
            )
            .unwrap();
        assert!(credential_trust_impl(db.conn(), CREDENTIAL_ID, NOW, "preprod").is_err());

        db.conn()
            .execute("DELETE FROM completion_claims WHERE id = 'other-claim'", [])
            .unwrap();
        db.conn()
            .execute(
                "UPDATE completion_claims SET completion_binding_json = '{}' WHERE id = 'claim'",
                [],
            )
            .unwrap();
        assert!(credential_trust_impl(db.conn(), CREDENTIAL_ID, NOW, "preprod").is_err());
    }
}
