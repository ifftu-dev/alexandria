//! Enrolment: a learner taking a course.
//!
//! Enrolling freezes the exact verified course document the learner signed
//! up for (its CID, version and completion policy), so later edits to the
//! course never change what an existing enrolment is measured against. The
//! enrolment commands, and chat when the learner allows an `enroll`
//! proposal, both go through here.

use rusqlite::{params, OptionalExtension};

use crate::crypto::hash::entity_id;
use crate::domain::course_document::{CourseCompletionPolicy, COURSE_DOCUMENT_VERSION};
use crate::domain::enrollment::Enrollment;

/// The learner's enrolments, optionally with one status, most recently
/// active first.
pub fn list(db: &crate::db::Database, status: Option<String>) -> Result<Vec<Enrollment>, String> {
    // Order by most recent learner activity so the dashboard "pick up where
    // you left off" card surfaces the course/tutorial the user last
    // viewed/progressed. Activity is the latest element_progress.updated_at
    // (written whenever an element is opened or completed); enrollments with
    // no progress yet fall back to enrolled_at.
    const LAST_ACTIVITY: &str = "COALESCE(\
        (SELECT MAX(ep.updated_at) FROM element_progress ep \
         WHERE ep.enrollment_id = enrollments.id), enrolled_at)";

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(ref s) =
        status
    {
        (
                format!(
                    "SELECT id, course_id, course_document_cid, course_document_version, completion_policy_json, \
                            enrolled_at, completed_at, status, updated_at \
                     FROM enrollments WHERE status = ?1 ORDER BY {LAST_ACTIVITY} DESC"
                ),
                vec![Box::new(s.clone())],
            )
    } else {
        (
                format!(
                    "SELECT id, course_id, course_document_cid, course_document_version, completion_policy_json, \
                            enrolled_at, completed_at, status, updated_at \
                     FROM enrollments ORDER BY {LAST_ACTIVITY} DESC"
                ),
                vec![],
            )
    };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = db.conn().prepare(&sql).map_err(|e| e.to_string())?;

    let enrollments = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(Enrollment {
                id: row.get(0)?,
                course_id: row.get(1)?,
                course_document_cid: row.get(2)?,
                course_document_version: row.get(3)?,
                completion_policy_json: row.get(4)?,
                enrolled_at: row.get(5)?,
                completed_at: row.get(6)?,
                status: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(enrollments)
}

/// Enrols the learner in a course, freezing its verified document. Refused
/// for a course with no verified document and when already enrolled.
pub fn enroll(db: &crate::db::Database, course_id: &str) -> Result<Enrollment, String> {
    // Get the local user's stake address for deterministic ID
    let stake_address: String = db
        .conn()
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no identity found — generate a wallet first: {}", e))?;

    // Read only the projection written by verified publication/hydration.
    let course_binding: Option<(Option<String>, Option<i64>, Option<String>)> = db
        .conn()
        .query_row(
            "SELECT content_cid, course_document_version, completion_policy_json \
             FROM courses WHERE id = ?1",
            params![course_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((Some(course_document_cid), Some(course_document_version), completion_policy_json)) =
        course_binding
    else {
        return Err(
            "course has no verified document to enroll in; publish or hydrate it first".into(),
        );
    };
    validate_course_binding(
        &course_document_cid,
        course_document_version,
        completion_policy_json.as_deref(),
    )?;

    // Check for existing active enrollment
    let already_enrolled: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM enrollments WHERE course_id = ?1 AND status = 'active'",
            params![course_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    if already_enrolled {
        return Err("already enrolled in this course".into());
    }

    let id = entity_id(&[&stake_address, course_id, &course_document_cid]);

    db.conn()
        .execute(
            "INSERT INTO enrollments \
             (id, course_id, course_document_cid, course_document_version, completion_policy_json) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                course_id,
                course_document_cid,
                course_document_version,
                completion_policy_json,
            ],
        )
        .map_err(|e| e.to_string())?;

    db.conn()
        .query_row(
            "SELECT id, course_id, course_document_cid, course_document_version, completion_policy_json, \
                    enrolled_at, completed_at, status, updated_at \
             FROM enrollments WHERE id = ?1",
            params![id],
            |row| {
                Ok(Enrollment {
                    id: row.get(0)?,
                    course_id: row.get(1)?,
                    course_document_cid: row.get(2)?,
                    course_document_version: row.get(3)?,
                    completion_policy_json: row.get(4)?,
                    enrolled_at: row.get(5)?,
                    completed_at: row.get(6)?,
                    status: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )
        .map_err(|e| e.to_string())
}

fn validate_course_binding(
    course_document_cid: &str,
    course_document_version: i64,
    completion_policy_json: Option<&str>,
) -> Result<(), String> {
    if course_document_cid.len() != 64
        || !course_document_cid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("verified course document CID must be canonical lowercase 32-byte hex".into());
    }
    let version = u32::try_from(course_document_version)
        .map_err(|_| "invalid verified course document version".to_string())?;
    match version {
        COURSE_DOCUMENT_VERSION => {}
        _ => {
            return Err(format!(
                "unsupported verified course document version: {version}"
            ))
        }
    }
    if let Some(json) = completion_policy_json {
        let policy: CourseCompletionPolicy = serde_json::from_str(json)
            .map_err(|error| format!("invalid completion policy projection: {error}"))?;
        policy
            .validate()
            .map_err(|error| format!("invalid completion policy projection: {error}"))?;
        let canonical = serde_json_canonicalizer::to_string(&policy)
            .map_err(|error| format!("canonicalize completion policy: {error}"))?;
        if canonical != json {
            return Err("completion policy projection is not canonical JSON".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{enroll, list};
    use crate::db::Database;
    use alexandria_verify::course::{
        AuthorizedAttestor, CourseCompletionPolicy, EvidenceRequirement,
        COMPLETION_POLICY_FORMAT_VERSION,
    };
    use alexandria_verify::did::did_from_verifying_key;
    use ed25519_dalek::SigningKey;
    use rusqlite::params;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn setup_identity(db: &Database) {
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test1u', 'addr_test1q')",
                [],
            )
            .unwrap();
    }

    fn policy_json(seed: u8) -> String {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let policy = CourseCompletionPolicy {
            format_version: COMPLETION_POLICY_FORMAT_VERSION,
            required_attestors: 1,
            authorized_attestors: vec![AuthorizedAttestor {
                did: did_from_verifying_key(&key.verifying_key()),
                public_key_hex: hex::encode(key.verifying_key().as_bytes()),
            }],
            evidence_requirements: vec![EvidenceRequirement {
                kind: "completion-root".into(),
                format_version: 1,
            }],
        };
        serde_json_canonicalizer::to_string(&policy).unwrap()
    }

    #[test]
    fn enroll_requires_a_verified_course_document_projection() {
        let db = test_db();
        setup_identity(&db);
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) VALUES ('c1', 'Draft', 'author')",
                [],
            )
            .unwrap();

        let error = enroll(&db, "c1").unwrap_err();
        assert!(error.contains("no verified document"), "got: {error}");
    }

    #[test]
    fn enrollment_freezes_exact_cid_version_and_policy() {
        let db = test_db();
        setup_identity(&db);
        let first_cid = "11".repeat(32);
        let second_cid = "22".repeat(32);
        let first_policy = policy_json(1);
        let second_policy = policy_json(2);
        db.conn()
            .execute(
                "INSERT INTO courses \
                 (id, title, author_address, content_cid, course_document_version, completion_policy_json) \
                 VALUES ('c1', 'Course', 'author', ?1, 2, ?2)",
                params![first_cid, first_policy],
            )
            .unwrap();

        let enrolled = enroll(&db, "c1").unwrap();
        assert_eq!(
            enrolled.course_document_cid.as_deref(),
            Some(first_cid.as_str())
        );
        assert_eq!(enrolled.course_document_version, Some(2));
        assert_eq!(
            enrolled.completion_policy_json.as_deref(),
            Some(first_policy.as_str())
        );

        db.conn()
            .execute(
                "UPDATE courses SET content_cid = ?1, completion_policy_json = ?2 WHERE id = 'c1'",
                params![second_cid, second_policy],
            )
            .unwrap();
        let rows = list(&db, None).unwrap();
        assert_eq!(
            rows[0].course_document_cid.as_deref(),
            Some(first_cid.as_str())
        );
        assert_eq!(
            rows[0].completion_policy_json.as_deref(),
            Some(first_policy.as_str())
        );
    }

    #[test]
    fn malformed_policy_projection_is_not_enrolled() {
        let db = test_db();
        setup_identity(&db);
        db.conn()
            .execute(
                "INSERT INTO courses \
                 (id, title, author_address, content_cid, course_document_version, completion_policy_json) \
                 VALUES ('c1', 'Course', 'author', ?1, 2, '{\"required_attestors\":0}')",
                ["11".repeat(32)],
            )
            .unwrap();

        let error = enroll(&db, "c1").unwrap_err();
        assert!(
            error.contains("invalid completion policy projection"),
            "got: {error}"
        );
    }

    /// Version 2 is the only accepted course document format, and signing and
    /// verification already refuse version 1. A course row stored before that
    /// change still carries version 1, and enrolment reads it straight from
    /// the row -- so that is the path that has to refuse, and it had no test.
    #[test]
    fn a_stored_version_one_document_is_not_enrolled() {
        let db = test_db();
        setup_identity(&db);
        db.conn()
            .execute(
                "INSERT INTO courses \
                 (id, title, author_address, content_cid, course_document_version) \
                 VALUES ('c1', 'Course', 'author', ?1, 1)",
                ["11".repeat(32)],
            )
            .unwrap();

        let error = enroll(&db, "c1").unwrap_err();
        assert!(
            error.contains("unsupported verified course document version: 1"),
            "got: {error}"
        );
        let enrolled: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM enrollments", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            enrolled, 0,
            "a refused document must not leave an enrollment"
        );
    }
}
