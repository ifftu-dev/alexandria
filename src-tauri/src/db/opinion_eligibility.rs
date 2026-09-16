//! Field-opinion posting qualification.
//!
//! Posting in a subject field requires a credential that satisfies the pinned
//! subject qualification policy for that field (see
//! `alexandria_verify::qualification`). Local publication, inbound gossip,
//! pending promotion and the eligible-field picker all use this module, so
//! they apply one rule. A stored row, its JSON shape, or an issuer that differs
//! from the subject grants nothing on its own.

use std::collections::BTreeSet;

use rusqlite::{params, Connection, OptionalExtension};

use alexandria_verify::qualification::{
    NotQualifiedReason, PolicyQualification, QualificationAction, QualificationDecision,
    QualificationInput, QualificationPolicySet,
};
use alexandria_verify::vc::VerificationPendingReason;
use alexandria_verify::Did;

use crate::commands::attestation::{stored_completion_evidence, StoredCompletionEvidence};
use crate::domain::vc::{
    verify_credential_db, SkillClaim, VerifiableCredential, VerificationPolicy,
};

/// Upper bound on credential references in one opinion. Each reference costs
/// a signature verification, so the bound applies before any lookup.
pub(crate) const MAX_OPINION_CREDENTIAL_PROOFS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpinionCredentialEligibility {
    /// The referenced credential is not stored locally yet.
    Unknown,
    /// Verification evidence for the credential is missing or unavailable.
    Pending(Vec<VerificationPendingReason>),
    /// The credential is known and does not satisfy the policy.
    Unqualified(NotQualifiedReason),
    Qualified(Box<PolicyQualification>),
}

/// Check one referenced credential against the pinned policy for posting in
/// `subject_field_id`.
///
/// The credential is re-verified from its signed bytes and the acting
/// identity must be its subject. Database failures are errors; a malformed
/// stored credential is an invalid credential, not a lookup failure.
pub(crate) fn check_opinion_credential(
    conn: &Connection,
    policies: &QualificationPolicySet,
    credential_id: &str,
    author_did: &Did,
    subject_field_id: &str,
    verification_time: &str,
) -> Result<OpinionCredentialEligibility, String> {
    if policies
        .applicable(QualificationAction::OpinionPosting, subject_field_id)
        .is_none()
    {
        return Ok(OpinionCredentialEligibility::Unqualified(
            NotQualifiedReason::NoApplicablePolicy,
        ));
    }
    let signed_json: Option<String> = conn
        .query_row(
            "SELECT signed_vc_json FROM credentials WHERE id = ?1",
            params![credential_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(signed_json) = signed_json else {
        return Ok(OpinionCredentialEligibility::Unknown);
    };
    let Ok(credential) = serde_json::from_str::<VerifiableCredential>(&signed_json) else {
        return Ok(OpinionCredentialEligibility::Unqualified(
            NotQualifiedReason::InvalidCredential,
        ));
    };
    let verification_policy = VerificationPolicy::default();
    let verification =
        verify_credential_db(conn, &credential, verification_time, &verification_policy);
    let evidence = stored_completion_evidence(conn, credential_id)?;
    let skill_subject_field = match SkillClaim::extract(&credential.credential_subject) {
        Some(claim) => skill_subject_field(conn, &claim.skill_id)?,
        None => None,
    };
    let decision = policies.evaluate(QualificationInput {
        action: QualificationAction::OpinionPosting,
        actor_did: author_did,
        subject_field_id,
        credential: &credential,
        verification: &verification,
        verification_policy: &verification_policy,
        endorsement: evidence.as_ref().map(StoredCompletionEvidence::as_evidence),
        skill_subject_field_id: skill_subject_field.as_deref(),
    });
    Ok(match decision {
        QualificationDecision::Qualified(qualification) => {
            OpinionCredentialEligibility::Qualified(Box::new(qualification))
        }
        QualificationDecision::Pending { reasons } => {
            OpinionCredentialEligibility::Pending(reasons)
        }
        QualificationDecision::NotQualified { reason } => {
            OpinionCredentialEligibility::Unqualified(reason)
        }
    })
}

/// Subject fields in which the author holds at least one policy-qualified
/// credential. Fields no pinned policy governs are never listed.
pub(crate) fn eligible_opinion_subject_fields(
    conn: &Connection,
    policies: &QualificationPolicySet,
    author_did: &Did,
    verification_time: &str,
) -> Result<Vec<String>, String> {
    let governed = policies
        .policies()
        .iter()
        .filter(|pinned| pinned.policy().action == QualificationAction::OpinionPosting)
        .flat_map(|pinned| pinned.policy().subject_field_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    if governed.is_empty() {
        return Ok(Vec::new());
    }
    // Indexed columns only select candidates; every candidate is re-verified.
    let mut statement = conn
        .prepare(
            "SELECT c.id, sub.subject_field_id \
             FROM credentials c \
             JOIN skills s ON s.id = c.skill_id \
             JOIN subjects sub ON sub.id = s.subject_id \
             WHERE c.subject_did = ?1 AND c.claim_kind = 'skill' AND c.revoked = 0 \
             ORDER BY sub.subject_field_id, c.id",
        )
        .map_err(|error| error.to_string())?;
    let candidates = statement
        .query_map([author_did.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;

    let mut eligible = BTreeSet::new();
    for (credential_id, field) in candidates {
        if !governed.contains(&field) || eligible.contains(&field) {
            continue;
        }
        if matches!(
            check_opinion_credential(
                conn,
                policies,
                &credential_id,
                author_did,
                &field,
                verification_time,
            )?,
            OpinionCredentialEligibility::Qualified(_)
        ) {
            eligible.insert(field);
        }
    }
    Ok(eligible.into_iter().collect())
}

/// Actionable explanation for an opinion that no referenced credential
/// qualifies. `reason` is the first conclusive refusal, if any.
pub(crate) fn opinion_refusal_message(
    subject_field_id: &str,
    reason: Option<NotQualifiedReason>,
) -> String {
    let explanation = match reason {
        None => "the referenced credentials are not available or still await verification evidence",
        Some(NotQualifiedReason::NoApplicablePolicy) => {
            return format!(
                "no pinned qualification policy governs opinion posting in '{subject_field_id}' on this network"
            )
        }
        Some(NotQualifiedReason::InvalidCredential) => "a referenced credential failed verification",
        Some(NotQualifiedReason::ActorNotSubject) => "the credential belongs to someone else",
        Some(NotQualifiedReason::NotSkillClaim) => "the credential is not a skill credential",
        Some(NotQualifiedReason::SkillNotInTaxonomy) => "the credential's skill is not in the local taxonomy",
        Some(NotQualifiedReason::SkillOutsideScope) => "the credential's skill is outside this subject field",
        Some(NotQualifiedReason::LevelBelowMinimum) => "the credential's proficiency is below the policy minimum",
        Some(NotQualifiedReason::SelfClaimNotAccepted) => {
            "your own claims do not qualify; the policy requires an accepted issuer or accepted instructor endorsement"
        }
        Some(NotQualifiedReason::IssuerNotAccepted) => "the issuer is not accepted by the policy",
        Some(NotQualifiedReason::EndorsementNotAccepted) => {
            "the course endorsement does not include enough accepted issuers"
        }
    };
    format!("none qualify for opinion posting in '{subject_field_id}': {explanation}")
}

/// Verification time for privilege checks, in the credential envelope format.
pub(crate) fn verification_time_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn skill_subject_field(conn: &Connection, skill_id: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT sub.subject_field_id FROM skills s \
         JOIN subjects sub ON sub.id = s.subject_id WHERE s.id = ?1",
        params![skill_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| error.to_string())
}

/// Signed fixtures for opinion qualification tests. Keys are test-only and
/// unrelated to any hosted identity.
#[cfg(test)]
pub(crate) mod test_support {
    use alexandria_verify::did::{derive_did_key, VerificationMethodRef};
    use alexandria_verify::qualification::{
        qualification_policy_digest, QualificationAction, QualificationPolicySet,
        QualificationRoute, SubjectQualificationPolicy, QUALIFICATION_POLICY_FORMAT_VERSION,
    };
    use alexandria_verify::vc::sign::{sign_credential, UnsignedCredential};
    use alexandria_verify::vc::{Claim, CredentialStatus, Proof, SkillClaim, VerifiableCredential};
    use alexandria_verify::Did;
    use ed25519_dalek::SigningKey;
    use rusqlite::params;

    use crate::db::Database;

    pub(crate) const NOW: &str = "2026-09-15T00:00:00Z";

    pub(crate) fn policy_set(
        accepted: &[&SigningKey],
        subject_field_ids: &[&str],
    ) -> QualificationPolicySet {
        let mut accepted_issuers = accepted
            .iter()
            .map(|key| derive_did_key(key))
            .collect::<Vec<_>>();
        accepted_issuers.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        let mut fields = subject_field_ids
            .iter()
            .map(|field| field.to_string())
            .collect::<Vec<_>>();
        fields.sort();
        let policy = SubjectQualificationPolicy {
            format_version: QUALIFICATION_POLICY_FORMAT_VERSION,
            policy_id: "test-opinion-policy".into(),
            network_id: "preprod".into(),
            action: QualificationAction::OpinionPosting,
            subject_field_ids: fields,
            minimum_level: 2,
            accepted_issuers,
            routes: vec![
                QualificationRoute::AcceptedCourseEndorsement,
                QualificationRoute::AcceptedIssuer,
            ],
            minimum_accepted_attestors: 1,
        };
        let bytes = serde_json_canonicalizer::to_vec(&policy).unwrap();
        let digest = qualification_policy_digest(&bytes);
        QualificationPolicySet::from_pinned(&[&bytes], &[digest], "preprod").unwrap()
    }

    pub(crate) fn empty_policy_set() -> QualificationPolicySet {
        QualificationPolicySet::from_pinned(&[], &[], "preprod").unwrap()
    }

    pub(crate) fn store_skill_credential(
        db: &Database,
        credential_id: &str,
        issuer: &SigningKey,
        subject: &Did,
        skill_id: &str,
        level: u8,
    ) {
        store_credential(db, credential_id, issuer, subject, skill_id, level, None);
    }

    pub(crate) fn store_credential(
        db: &Database,
        credential_id: &str,
        issuer: &SigningKey,
        subject: &Did,
        skill_id: &str,
        level: u8,
        status: Option<CredentialStatus>,
    ) {
        store_scored_credential(
            db,
            credential_id,
            issuer,
            subject,
            skill_id,
            level,
            0.9,
            status,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn store_scored_credential(
        db: &Database,
        credential_id: &str,
        issuer: &SigningKey,
        subject: &Did,
        skill_id: &str,
        level: u8,
        score: f64,
        status: Option<CredentialStatus>,
    ) {
        let issuer_did = derive_did_key(issuer);
        let class = if issuer_did == *subject {
            "SelfAssertion"
        } else {
            "AssessmentCredential"
        };
        let claim = Claim::Skill(SkillClaim {
            skill_id: skill_id.into(),
            level,
            score,
            evidence_refs: vec![],
            rubric_version: None,
            assessment_method: None,
            provenance: None,
        });
        let unsigned = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some(credential_id.into()),
            type_: vec!["VerifiableCredential".into(), class.into()],
            issuer: issuer_did.clone(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_until: None,
            credential_subject: claim.into_subject(subject.clone()),
            credential_status: status,
            terms_of_use: None,
            witness: None,
            integrity: None,
            proof: Proof {
                type_: "Ed25519Signature2020".into(),
                created: "2026-01-01T00:00:00Z".into(),
                verification_method: VerificationMethodRef(format!(
                    "{}#key-1",
                    issuer_did.as_str()
                )),
                proof_purpose: "assertionMethod".into(),
                jws: String::new(),
            },
        };
        let credential = sign_credential(
            UnsignedCredential {
                credential: unsigned,
            },
            issuer,
            &issuer_did,
        )
        .unwrap();
        db.conn()
            .execute(
                "INSERT INTO credentials (id, issuer_did, subject_did, credential_type, \
                 claim_kind, skill_id, issuance_date, signed_vc_json, integrity_hash, revoked) \
                 VALUES (?1, ?2, ?3, ?4, 'skill', ?5, '2026-01-01T00:00:00Z', ?6, ?7, 0)",
                params![
                    credential_id,
                    issuer_did.as_str(),
                    subject.as_str(),
                    class,
                    skill_id,
                    serde_json::to_string(&credential).unwrap(),
                    crate::commands::credentials::integrity_hash_of(&credential).unwrap()
                ],
            )
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use alexandria_verify::did::derive_did_key;
    use alexandria_verify::vc::CredentialStatus;
    use ed25519_dalek::SigningKey;

    use super::test_support::{
        empty_policy_set, policy_set, store_credential, store_skill_credential, NOW,
    };
    use super::*;
    use crate::db::Database;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open database");
        db.run_migrations().expect("run migrations");
        for (sql, label) in [
            (
                "INSERT INTO subject_fields (id, name) VALUES ('field', 'Field')",
                "field",
            ),
            (
                "INSERT INTO subject_fields (id, name) VALUES ('other', 'Other')",
                "other field",
            ),
            (
                "INSERT INTO subjects (id, name, subject_field_id) \
                 VALUES ('subject', 'Subject', 'field')",
                "subject",
            ),
            (
                "INSERT INTO skills (id, name, subject_id, bloom_level) \
                 VALUES ('skill', 'Skill', 'subject', 'apply')",
                "skill",
            ),
        ] {
            db.conn().execute(sql, []).expect(label);
        }
        db
    }

    fn check(
        db: &Database,
        policies: &QualificationPolicySet,
        credential_id: &str,
        author: &Did,
        field: &str,
    ) -> OpinionCredentialEligibility {
        check_opinion_credential(db.conn(), policies, credential_id, author, field, NOW)
            .expect("check credential")
    }

    #[test]
    fn accepted_issuer_credential_qualifies_in_its_governed_field() {
        let db = test_db();
        let instructor = key(3);
        let learner = derive_did_key(&key(1));
        let policies = policy_set(&[&instructor], &["field"]);
        store_skill_credential(&db, "credential", &instructor, &learner, "skill", 2);

        match check(&db, &policies, "credential", &learner, "field") {
            OpinionCredentialEligibility::Qualified(qualification) => {
                assert_eq!(qualification.policy_digest, policies.policies()[0].digest());
                assert_eq!(
                    qualification.qualifying_issuers,
                    vec![derive_did_key(&instructor)]
                );
            }
            other => panic!("expected qualification, got {other:?}"),
        }
        assert_eq!(
            eligible_opinion_subject_fields(db.conn(), &policies, &learner, NOW).unwrap(),
            vec!["field".to_string()]
        );
    }

    #[test]
    fn self_claims_and_unaccepted_issuers_do_not_qualify() {
        let db = test_db();
        let instructor = key(3);
        let learner_key = key(1);
        let learner = derive_did_key(&learner_key);
        let policies = policy_set(&[&instructor], &["field"]);
        store_skill_credential(&db, "self", &learner_key, &learner, "skill", 5);
        store_skill_credential(&db, "puppet", &key(2), &learner, "skill", 5);

        assert_eq!(
            check(&db, &policies, "self", &learner, "field"),
            OpinionCredentialEligibility::Unqualified(NotQualifiedReason::SelfClaimNotAccepted)
        );
        assert_eq!(
            check(&db, &policies, "puppet", &learner, "field"),
            OpinionCredentialEligibility::Unqualified(NotQualifiedReason::IssuerNotAccepted)
        );
        assert!(
            eligible_opinion_subject_fields(db.conn(), &policies, &learner, NOW)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn no_pinned_policy_grants_nothing() {
        let db = test_db();
        let instructor = key(3);
        let learner = derive_did_key(&key(1));
        store_skill_credential(&db, "credential", &instructor, &learner, "skill", 5);
        let governed_elsewhere = policy_set(&[&instructor], &["other"]);

        for policies in [empty_policy_set(), governed_elsewhere] {
            assert_eq!(
                check(&db, &policies, "credential", &learner, "field"),
                OpinionCredentialEligibility::Unqualified(NotQualifiedReason::NoApplicablePolicy)
            );
            assert!(
                eligible_opinion_subject_fields(db.conn(), &policies, &learner, NOW)
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn credential_must_belong_to_opinion_author_and_verify() {
        let db = test_db();
        let instructor = key(3);
        let learner = derive_did_key(&key(1));
        let policies = policy_set(&[&instructor], &["field"]);
        store_skill_credential(&db, "credential", &instructor, &learner, "skill", 2);

        let someone_else = derive_did_key(&key(9));
        assert_eq!(
            check(&db, &policies, "credential", &someone_else, "field"),
            OpinionCredentialEligibility::Unqualified(NotQualifiedReason::ActorNotSubject)
        );

        // A stored row whose signed body was altered after signing, and a row
        // that is not a credential at all, grant nothing.
        db.conn()
            .execute(
                "UPDATE credentials SET signed_vc_json = \
                 json_set(signed_vc_json, '$.credentialSubject.level', 5) WHERE id = 'credential'",
                [],
            )
            .unwrap();
        assert_eq!(
            check(&db, &policies, "credential", &learner, "field"),
            OpinionCredentialEligibility::Unqualified(NotQualifiedReason::InvalidCredential)
        );
        db.conn()
            .execute(
                "UPDATE credentials SET signed_vc_json = '{\"credentialSubject\":{}}' \
                 WHERE id = 'credential'",
                [],
            )
            .unwrap();
        assert_eq!(
            check(&db, &policies, "credential", &learner, "field"),
            OpinionCredentialEligibility::Unqualified(NotQualifiedReason::InvalidCredential)
        );
        assert_eq!(
            check(&db, &policies, "missing", &learner, "field"),
            OpinionCredentialEligibility::Unknown
        );
    }

    #[test]
    fn missing_status_evidence_is_pending_not_qualified() {
        let db = test_db();
        let instructor = key(3);
        let learner = derive_did_key(&key(1));
        let policies = policy_set(&[&instructor], &["field"]);
        store_credential(
            &db,
            "credential",
            &instructor,
            &learner,
            "skill",
            3,
            Some(CredentialStatus {
                id: "urn:status:1#0".into(),
                type_: "BitstringStatusListEntry".into(),
                status_purpose: "revocation".into(),
                status_list_index: "0".into(),
                status_list_credential: "urn:status:1".into(),
            }),
        );

        assert_eq!(
            check(&db, &policies, "credential", &learner, "field"),
            OpinionCredentialEligibility::Pending(vec![
                VerificationPendingReason::StatusListMissing
            ])
        );
    }

    #[test]
    fn refusal_messages_explain_the_missing_requirement() {
        assert!(
            opinion_refusal_message("field", Some(NotQualifiedReason::NoApplicablePolicy))
                .contains("no pinned qualification policy")
        );
        assert!(
            opinion_refusal_message("field", Some(NotQualifiedReason::SelfClaimNotAccepted))
                .contains("accepted issuer")
        );
        assert!(opinion_refusal_message("field", None).contains("none qualify"));
    }
}
