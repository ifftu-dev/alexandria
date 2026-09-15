use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpinionCredentialEligibility {
    Unknown,
    Unqualified,
    Qualified,
}

/// Check a credential used to authorize a field-commentary opinion.
///
/// The indexed subject, skill, and proficiency are checked against the signed
/// credential body so a stale or malformed projection cannot grant posting
/// authority. `Unknown` is distinct because remote opinions may wait for a
/// referenced credential to arrive.
pub(crate) fn check_opinion_credential(
    conn: &Connection,
    credential_id: &str,
    author_did: &str,
    subject_field_id: &str,
) -> Result<OpinionCredentialEligibility, String> {
    let qualified = conn
        .query_row(
            "SELECT CASE WHEN \
                 c.subject_did = ?2 \
                 AND c.claim_kind = 'skill' \
                 AND c.revoked = 0 \
                 AND c.skill_id IS NOT NULL \
                 AND json_type(c.signed_vc_json, '$.credentialSubject.id') = 'text' \
                 AND json_extract(c.signed_vc_json, '$.credentialSubject.id') = ?2 \
                 AND json_type(c.signed_vc_json, '$.credentialSubject.skillId') = 'text' \
                 AND json_extract(c.signed_vc_json, '$.credentialSubject.skillId') = c.skill_id \
                 AND json_type(c.signed_vc_json, '$.credentialSubject.level') = 'integer' \
                 AND CAST(json_extract(c.signed_vc_json, '$.credentialSubject.level') AS INTEGER) \
                     BETWEEN 2 AND 5 \
                 AND EXISTS ( \
                     SELECT 1 FROM skills s \
                     JOIN subjects sub ON sub.id = s.subject_id \
                     WHERE s.id = c.skill_id AND sub.subject_field_id = ?3 \
                 ) \
             THEN 1 ELSE 0 END \
             FROM credentials c WHERE c.id = ?1",
            params![credential_id, author_did, subject_field_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;

    Ok(match qualified {
        None => OpinionCredentialEligibility::Unknown,
        Some(0) => OpinionCredentialEligibility::Unqualified,
        Some(_) => OpinionCredentialEligibility::Qualified,
    })
}

pub(crate) fn eligible_opinion_subject_fields(
    conn: &Connection,
    author_did: &str,
) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT sub.subject_field_id \
             FROM credentials c \
             JOIN skills s ON s.id = c.skill_id \
             JOIN subjects sub ON sub.id = s.subject_id \
             WHERE c.subject_did = ?1 \
               AND c.claim_kind = 'skill' \
               AND c.revoked = 0 \
               AND json_type(c.signed_vc_json, '$.credentialSubject.id') = 'text' \
               AND json_extract(c.signed_vc_json, '$.credentialSubject.id') = ?1 \
               AND json_type(c.signed_vc_json, '$.credentialSubject.skillId') = 'text' \
               AND json_extract(c.signed_vc_json, '$.credentialSubject.skillId') = c.skill_id \
               AND json_type(c.signed_vc_json, '$.credentialSubject.level') = 'integer' \
               AND CAST(json_extract(c.signed_vc_json, '$.credentialSubject.level') AS INTEGER) \
                   BETWEEN 2 AND 5 \
             ORDER BY sub.subject_field_id",
        )
        .map_err(|error| error.to_string())?;
    let fields = stmt
        .query_map([author_did], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open database");
        db.run_migrations().expect("run migrations");
        db.conn()
            .execute(
                "INSERT INTO subject_fields (id, name) VALUES ('field', 'Field')",
                [],
            )
            .expect("insert field");
        db.conn()
            .execute(
                "INSERT INTO subjects (id, name, subject_field_id) \
                 VALUES ('subject', 'Subject', 'field')",
                [],
            )
            .expect("insert subject");
        db.conn()
            .execute(
                "INSERT INTO skills (id, name, subject_id, bloom_level) \
                 VALUES ('skill', 'Skill', 'subject', 'apply')",
                [],
            )
            .expect("insert skill");
        db
    }

    fn insert_credential(db: &Database, id: &str, indexed_subject: &str, body_subject: &str) {
        let body = serde_json::json!({
            "credentialSubject": {
                "id": body_subject,
                "skillId": "skill",
                "level": 2,
                "score": 0.8,
                "evidenceRefs": []
            }
        });
        db.conn()
            .execute(
                "INSERT INTO credentials (id, issuer_did, subject_did, credential_type, \
                 claim_kind, skill_id, issuance_date, signed_vc_json, integrity_hash, revoked) \
                 VALUES (?1, 'did:key:issuer', ?2, 'SelfAssertion', 'skill', 'skill', \
                         '2026-01-01T00:00:00Z', ?3, ?1, 0)",
                params![id, indexed_subject, body.to_string()],
            )
            .expect("insert credential");
    }

    #[test]
    fn credential_must_belong_to_opinion_author() {
        let db = test_db();
        insert_credential(&db, "credential", "did:key:alice", "did:key:alice");

        assert_eq!(
            check_opinion_credential(db.conn(), "credential", "did:key:bob", "field")
                .expect("check credential"),
            OpinionCredentialEligibility::Unqualified
        );
        assert_eq!(
            check_opinion_credential(db.conn(), "credential", "did:key:alice", "field")
                .expect("check credential"),
            OpinionCredentialEligibility::Qualified
        );
    }

    #[test]
    fn signed_body_must_match_indexed_subject() {
        let db = test_db();
        insert_credential(&db, "credential", "did:key:alice", "did:key:bob");

        assert_eq!(
            check_opinion_credential(db.conn(), "credential", "did:key:alice", "field")
                .expect("check credential"),
            OpinionCredentialEligibility::Unqualified
        );
        assert!(eligible_opinion_subject_fields(db.conn(), "did:key:alice")
            .expect("list fields")
            .is_empty());
    }
}
