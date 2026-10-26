//! Exact course-completion endorsements: the store.
//!
//! A completion claim is bound to the exact author-signed course document
//! and completion policy its enrollment froze. Instructors' endorsements of
//! a claim are verified against that policy and binding before they are
//! stored, and counted against its threshold when read. Database rows never
//! create or widen authority: a corrupt or ambiguous stored claim is an
//! error, not a lower trust state.
//!
//! The endorsement commands (`commands::attestation`) and the trust and
//! scoring readers in `db` all read claims through here.

use rusqlite::{params, Connection, OptionalExtension};

use alexandria_verify::course::{evaluate_completion_endorsements, verify_completion_endorsement};
use alexandria_verify::trust::CourseEndorsementEvidence;

use crate::domain::attestation::{
    CourseCompletionBinding, CourseCompletionEndorsement, CourseCompletionEndorsementStatus,
    CourseCompletionPolicy,
};

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

/// The exact binding an instructor endorses for a claim.
pub fn endorsement_request(
    conn: &Connection,
    claim_id: &str,
) -> Result<CourseCompletionBinding, String> {
    Ok(load_claim_context(conn, claim_id)?.binding)
}

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

/// Import a completion endorsement from the exact JSON bytes an instructor
/// exported. The bytes are parsed under the verifier's endorsement limits, so
/// duplicate keys, unsafe numbers, hostile nesting and oversized documents are
/// refused before verification or storage.
pub fn import_completion_endorsement_json_impl(
    conn: &Connection,
    claim_id: &str,
    endorsement_json: &[u8],
) -> Result<CourseCompletionEndorsement, String> {
    let endorsement = alexandria_verify::course::decode_completion_endorsement(endorsement_json)
        .map_err(|error| format!("invalid completion endorsement JSON: {error}"))?;
    import_completion_endorsement_impl(conn, claim_id, &endorsement)
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

#[cfg(test)]
pub(crate) mod fixtures {
    use alexandria_verify::course::{
        AuthorizedAttestor, CompletionEvidence, EvidenceRequirement,
        COMPLETION_ENDORSEMENT_FORMAT_VERSION, COMPLETION_POLICY_FORMAT_VERSION,
    };
    use alexandria_verify::did::did_from_verifying_key;
    use ed25519_dalek::SigningKey;
    use rusqlite::params;

    use crate::domain::attestation::{CourseCompletionBinding, CourseCompletionPolicy};

    /// A migrated database with one enrollment and its completion claim
    /// (`claim`), the claim's policy and binding, and the two authorized
    /// attestors' keys.
    pub(crate) fn fixture() -> (
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
}

#[cfg(test)]
mod tests {
    use super::fixtures::fixture;
    use super::*;
    use alexandria_verify::course::sign_completion_endorsement;

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

    /// The app consumes the verifier's exact endorsement vector bytes through
    /// its import path and must reach the same outcome for each file.
    #[test]
    fn shared_endorsement_vectors_produce_the_same_outcomes_on_import() {
        let (db, policy, binding, _, _) = fixture();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/alexandria-verify/tests/vectors/endorsements");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(
            serde_json::from_value::<CourseCompletionPolicy>(manifest["policy"].clone()).unwrap(),
            policy
        );
        assert_eq!(
            serde_json::from_value::<CourseCompletionBinding>(manifest["expectedBinding"].clone())
                .unwrap(),
            binding
        );

        for case in manifest["cases"].as_array().unwrap() {
            let file = case["file"].as_str().unwrap();
            let expect = case["expect"].as_str().unwrap();
            let bytes = std::fs::read(dir.join(file)).unwrap();
            let result = import_completion_endorsement_json_impl(db.conn(), "claim", &bytes);
            let reason = match (&result, expect) {
                (Ok(_), "valid") => continue,
                (Err(reason), _) if expect != "valid" => reason,
                _ => panic!("{file}: expected {expect}, got {result:?}"),
            };
            let marker = match expect {
                "binding_mismatch" => "does not match the expected completion binding",
                "unauthorized_attestor" => "not authorized",
                "invalid_signature" => "invalid Ed25519 signature",
                "duplicate_key" => "duplicate JSON object key",
                "too_large" => "exceeds",
                "too_deep" => "nesting exceeds",
                "unsafe_number" => "exactly representable",
                other => panic!("{file}: unmapped outcome {other}"),
            };
            assert!(reason.contains(marker), "{file}: {reason}");
        }

        let status = completion_endorsement_status_impl(db.conn(), "claim").unwrap();
        assert!(status.satisfied);
        assert_eq!(status.valid_attestors.len(), 2);
        assert_eq!(status.endorsements.len(), 2);
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
