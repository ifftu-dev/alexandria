//! Exact course-completion endorsement acquisition and persistence.
//!
//! Policies come only from the exact author-signed course document selected by
//! the enrollment. Stored endorsements pass the shared I/O-free verifier before
//! insertion; database rows never create or widen authority.

use crate::profile::scope::ProfileState as State;
use rusqlite::{params, Connection, OptionalExtension};

use alexandria_verify::course::{
    evaluate_completion_endorsements, sign_completion_endorsement, verify_completion_endorsement,
};
use alexandria_verify::trust::{classify_credential, CourseEndorsementEvidence, CredentialTrust};

use crate::commands::credentials::get_credential_impl;
use crate::content_store::course as content_course;
use crate::crypto::wallet;
use crate::db::executor::DatabaseWorkload;
use crate::domain::attestation::{
    CourseCompletionBinding, CourseCompletionEndorsement, CourseCompletionEndorsementStatus,
    CourseCompletionPolicy,
};
use crate::domain::vc::{verify_credential_db, VerificationPolicy};
use crate::network_profile::embedded_preprod;
use crate::AppState;

struct ClaimContext {
    policy: CourseCompletionPolicy,
    binding: CourseCompletionBinding,
}

type ClaimContextRow = (
    Option<String>,
    Option<String>,
    String,
    Option<i64>,
    Option<String>,
    String,
    String,
    Option<String>,
);

fn load_claim_context(conn: &Connection, claim_id: &str) -> Result<ClaimContext, String> {
    let row: Option<ClaimContextRow> = conn
        .query_row(
            "SELECT e.completion_policy_json, cc.completion_binding_json, \
                    cc.course_id, cc.course_document_version, cc.course_document_cid, \
                    cc.subject_did, cc.completion_root, cc.enrollment_id \
             FROM completion_claims cc \
             JOIN enrollments e ON e.id = cc.enrollment_id \
             WHERE cc.id = ?1",
            [claim_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((policy_json, binding_json, course_id, version, cid, subject, root, enrollment_id)) =
        row
    else {
        return Err("completion has no instructor endorsement request".into());
    };
    let policy_json = policy_json.ok_or("completion enrollment has no exact endorsement policy")?;
    let binding_json = binding_json.ok_or("completion has no exact endorsement binding")?;
    let version = version.ok_or("completion has no exact course document version")?;
    let cid = cid.ok_or("completion has no exact course document CID")?;
    enrollment_id.ok_or("completion has no exact enrollment")?;
    let policy: CourseCompletionPolicy = serde_json::from_str(&policy_json)
        .map_err(|error| format!("invalid stored completion policy: {error}"))?;
    policy
        .validate()
        .map_err(|error| format!("invalid stored completion policy: {error}"))?;
    if serde_json_canonicalizer::to_string(&policy).map_err(|error| error.to_string())?
        != policy_json
    {
        return Err("stored completion policy is not canonical".into());
    }
    let binding: CourseCompletionBinding = serde_json::from_str(&binding_json)
        .map_err(|error| format!("invalid stored completion binding: {error}"))?;
    binding
        .validate(&policy)
        .map_err(|error| format!("invalid stored completion binding: {error}"))?;
    if serde_json_canonicalizer::to_string(&binding).map_err(|error| error.to_string())?
        != binding_json
        || binding.course_id != course_id
        || i64::from(binding.course_document_version) != version
        || binding.course_document_cid != cid
        || binding.subject_did.as_str() != subject
        || binding.completion_root != root
    {
        return Err("stored completion binding does not match its claim".into());
    }
    Ok(ClaimContext { policy, binding })
}

pub fn import_completion_endorsement_impl(
    conn: &Connection,
    claim_id: &str,
    endorsement: &CourseCompletionEndorsement,
) -> Result<CourseCompletionEndorsement, String> {
    let context = load_claim_context(conn, claim_id)?;
    verify_completion_endorsement(&context.policy, &context.binding, endorsement)
        .map_err(|error| error.to_string())?;
    let canonical = serde_json_canonicalizer::to_string(endorsement)
        .map_err(|error| format!("canonicalize completion endorsement: {error}"))?;
    let id = blake3::hash(canonical.as_bytes()).to_hex().to_string();
    conn.execute(
        "INSERT OR IGNORE INTO course_completion_endorsements \
         (id, claim_id, attestor_did, endorsement_json) VALUES (?1, ?2, ?3, ?4)",
        params![id, claim_id, endorsement.attestor_did.as_str(), canonical],
    )
    .map_err(|error| error.to_string())?;
    let stored: String = conn
        .query_row(
            "SELECT endorsement_json FROM course_completion_endorsements \
             WHERE claim_id = ?1 AND attestor_did = ?2",
            params![claim_id, endorsement.attestor_did.as_str()],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if stored != canonical {
        return Err("attestor already submitted a different endorsement for this claim".into());
    }
    serde_json::from_str(&stored).map_err(|error| error.to_string())
}

fn list_completion_endorsements(
    conn: &Connection,
    claim_id: &str,
) -> Result<Vec<CourseCompletionEndorsement>, String> {
    let mut statement = conn
        .prepare(
            "SELECT endorsement_json FROM course_completion_endorsements \
             WHERE claim_id = ?1 ORDER BY attestor_did",
        )
        .map_err(|error| error.to_string())?;
    let endorsements = statement
        .query_map([claim_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .map(|row| {
            let json = row.map_err(|error| error.to_string())?;
            serde_json::from_str(&json).map_err(|error| error.to_string())
        })
        .collect();
    endorsements
}

pub fn completion_endorsement_status_impl(
    conn: &Connection,
    claim_id: &str,
) -> Result<CourseCompletionEndorsementStatus, String> {
    let context = load_claim_context(conn, claim_id)?;
    let endorsements = list_completion_endorsements(conn, claim_id)?;
    let threshold =
        evaluate_completion_endorsements(&context.policy, &context.binding, &endorsements)
            .map_err(|error| error.to_string())?;
    Ok(CourseCompletionEndorsementStatus {
        claim_id: claim_id.to_owned(),
        required_attestors: threshold.required_attestors,
        valid_attestors: threshold.valid_attestors,
        rejected_endorsements: threshold.rejected_endorsements,
        satisfied: threshold.satisfied,
        endorsements,
    })
}

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

/// Exact completion policy, binding, and stored endorsements of the single
/// claim that lists a credential.
pub(crate) struct StoredCompletionEvidence {
    context: ClaimContext,
    endorsements: Vec<CourseCompletionEndorsement>,
}

impl StoredCompletionEvidence {
    pub(crate) fn as_evidence(&self) -> CourseEndorsementEvidence<'_> {
        CourseEndorsementEvidence {
            policy: &self.context.policy,
            binding: &self.context.binding,
            endorsements: &self.endorsements,
        }
    }
}

/// Completion evidence for a credential, if a claim with an exact policy
/// lists it. Corrupt or ambiguous stored evidence is an error.
pub(crate) fn stored_completion_evidence(
    conn: &Connection,
    credential_id: &str,
) -> Result<Option<StoredCompletionEvidence>, String> {
    match completion_claims_for_credential(conn, credential_id)?.as_slice() {
        [] | [(_, None)] => Ok(None),
        [(claim_id, Some(_))] => Ok(Some(StoredCompletionEvidence {
            context: load_claim_context(conn, claim_id)?,
            endorsements: list_completion_endorsements(conn, claim_id)?,
        })),
        _ => Err("credential is listed by more than one completion claim".into()),
    }
}

/// Claims listing a credential, with the frozen enrollment policy if any.
/// At most two rows are read: more than one is already ambiguous.
fn completion_claims_for_credential(
    conn: &Connection,
    credential_id: &str,
) -> Result<Vec<(String, Option<String>)>, String> {
    let mut statement = conn
        .prepare(
            "SELECT cc.id, e.completion_policy_json \
             FROM completion_claims cc \
             LEFT JOIN enrollments e ON e.id = cc.enrollment_id \
             WHERE EXISTS ( \
                 SELECT 1 FROM json_each(cc.credential_ids_json) \
                 WHERE json_each.value = ?1 \
             ) \
             ORDER BY cc.id LIMIT 2",
        )
        .map_err(|error| error.to_string())?;
    let claims = statement
        .query_map([credential_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string());
    claims
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
            move |db| Ok(load_claim_context(db.conn(), &claim_id)?.binding),
        )
        .await
}

#[tauri::command]
pub async fn sign_course_completion_endorsement(
    state: State<'_, AppState>,
    binding: CourseCompletionBinding,
) -> Result<CourseCompletionEndorsement, String> {
    let document =
        content_course::resolve_course_document(&state.content_node, &binding.course_document_cid)
            .await
            .map_err(|error| format!("resolve exact course document: {error}"))?;
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
    endorsement: CourseCompletionEndorsement,
) -> Result<CourseCompletionEndorsement, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "completion.endorsement.import",
            move |db| import_completion_endorsement_impl(db.conn(), &claim_id, &endorsement),
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
    use alexandria_verify::course::{
        AuthorizedAttestor, CompletionEvidence, EvidenceRequirement,
        COMPLETION_ENDORSEMENT_FORMAT_VERSION, COMPLETION_POLICY_FORMAT_VERSION,
    };
    use alexandria_verify::did::did_from_verifying_key;
    use ed25519_dalek::SigningKey;

    fn fixture() -> (
        crate::db::Database,
        CourseCompletionPolicy,
        CourseCompletionBinding,
        SigningKey,
        SigningKey,
    ) {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let first = SigningKey::from_bytes(&[1; 32]);
        let second = SigningKey::from_bytes(&[2; 32]);
        let subject = SigningKey::from_bytes(&[9; 32]);
        let mut authorized_attestors = [&first, &second]
            .into_iter()
            .map(|key| AuthorizedAttestor {
                did: did_from_verifying_key(&key.verifying_key()),
                public_key_hex: hex::encode(key.verifying_key().as_bytes()),
            })
            .collect::<Vec<_>>();
        authorized_attestors.sort_by(|left, right| left.did.as_str().cmp(right.did.as_str()));
        let policy = CourseCompletionPolicy {
            format_version: COMPLETION_POLICY_FORMAT_VERSION,
            required_attestors: 2,
            authorized_attestors,
            evidence_requirements: vec![EvidenceRequirement {
                kind: "completion-root".into(),
                format_version: 1,
            }],
        };
        let binding = CourseCompletionBinding {
            format_version: COMPLETION_ENDORSEMENT_FORMAT_VERSION,
            network_id: "preprod".into(),
            subject_did: did_from_verifying_key(&subject.verifying_key()),
            course_id: "course".into(),
            course_document_cid: "11".repeat(32),
            course_document_version: 2,
            completion_root: "22".repeat(32),
            evidence: vec![CompletionEvidence {
                kind: "completion-root".into(),
                format_version: 1,
                id: "course-completion".into(),
                digest: "22".repeat(32),
            }],
            witness_tx_hash: None,
        };
        let policy_json = serde_json_canonicalizer::to_string(&policy).unwrap();
        let binding_json = serde_json_canonicalizer::to_string(&binding).unwrap();
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) VALUES ('course', 'Course', 'author')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO enrollments \
                 (id, course_id, status, course_document_cid, course_document_version, completion_policy_json) \
                 VALUES ('enrollment', 'course', 'completed', ?1, 2, ?2)",
                params![binding.course_document_cid, policy_json],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO completion_claims \
                 (id, subject_did, course_id, course_document_cid, course_document_version, \
                  completion_root, completion_binding_json, credential_ids_json, enrollment_id) \
                 VALUES ('claim', ?1, 'course', ?2, 2, ?3, ?4, '[]', 'enrollment')",
                params![
                    binding.subject_did.as_str(),
                    binding.course_document_cid,
                    binding.completion_root,
                    binding_json,
                ],
            )
            .unwrap();
        (db, policy, binding, first, second)
    }

    #[test]
    fn only_valid_distinct_exact_binding_endorsements_are_persisted_and_counted() {
        let (db, policy, binding, first, second) = fixture();
        let first_endorsement =
            sign_completion_endorsement(&policy, binding.clone(), &first).unwrap();
        import_completion_endorsement_impl(db.conn(), "claim", &first_endorsement).unwrap();
        import_completion_endorsement_impl(db.conn(), "claim", &first_endorsement).unwrap();
        let partial = completion_endorsement_status_impl(db.conn(), "claim").unwrap();
        assert!(!partial.satisfied);
        assert_eq!(partial.valid_attestors.len(), 1);

        let second_endorsement =
            sign_completion_endorsement(&policy, binding.clone(), &second).unwrap();
        import_completion_endorsement_impl(db.conn(), "claim", &second_endorsement).unwrap();
        let complete = completion_endorsement_status_impl(db.conn(), "claim").unwrap();
        assert!(complete.satisfied);
        assert_eq!(complete.valid_attestors.len(), 2);

        let mut tampered = first_endorsement;
        tampered.binding.network_id = "other".into();
        assert!(import_completion_endorsement_impl(db.conn(), "claim", &tampered).is_err());
    }

    const CREDENTIAL_ID: &str = "urn:uuid:completion-self-claim";
    const NOW: &str = "2026-09-15T00:00:00Z";

    fn store_self_claim(db: &crate::db::Database, binding: &CourseCompletionBinding) {
        use alexandria_verify::did::VerificationMethodRef;
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
            proof: Proof {
                type_: "Ed25519Signature2020".into(),
                created: "2026-01-01T00:00:00Z".into(),
                verification_method: VerificationMethodRef(format!(
                    "{}#key-1",
                    subject_did.as_str()
                )),
                proof_purpose: "assertionMethod".into(),
                jws: String::new(),
            },
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

    #[test]
    fn legacy_mutable_attestation_tables_are_removed() {
        let (db, _, _, _, _) = fixture();
        for table in [
            "completion_attestation_requirements",
            "completion_attestations",
        ] {
            let exists: bool = db
                .conn()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(!exists, "{table} should be retired");
        }
    }
}
