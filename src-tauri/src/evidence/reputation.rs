//! Reputation engine (post-VC-first rebuild).
//!
//! The legacy engine (~1400 LOC) computed instructor attribution from
//! `evidence_records` → `skill_assessments` → `courses` joins plus a
//! distribution-metrics pipeline. Those input tables are gone. The
//! VC-first replacement derives the same high-level signal — one
//! `learner` row per (subject, skill, level) and one `instructor` row
//! per (issuer, skill, level) — directly from `credentials`:
//!
//! * **Learner reputation**: for every accepted skill-kind VC, the
//!   `subject_did` accumulates a reputation row at (skill, level)
//!   whose score mirrors the highest `SkillClaim.score` observed.
//! * **Instructor reputation**: when `issuer_did != subject_did`
//!   (a third-party-issued credential, not a self-asserted one), the
//!   issuer accumulates a reputation row at (skill, level) whose
//!   score is the mean of the scores they've issued. Self-asserted /
//!   self-witnessed VCs (our auto-issuance path) contribute only to
//!   the learner row — there's no instructor to credit.
//!
//! Variance / percentile metrics from the v1 engine are left at
//! `NULL` / `0` for now; they can be layered on top of the same
//! aggregation query when there's enough data to warrant them.
//!
//! Entry point: [`on_credential_accepted`]. Callers — the VC
//! issuance paths in `commands::credentials` and
//! `commands::auto_issuance` — should invoke it after a credential
//! lands. `recompute_for_subject` is available for full rebuild from
//! scratch (seeds/tests).

use rusqlite::{params, Connection, OptionalExtension};

use crate::crypto::hash::entity_id;

/// Map a SkillClaim integer level (0..=5) to the canonical string.
pub fn level_to_str(level: i64) -> &'static str {
    match level {
        0 => "remember",
        1 => "understand",
        2 => "apply",
        3 => "analyze",
        4 => "evaluate",
        5 => "create",
        _ => "remember",
    }
}

/// Called whenever a credential is accepted into the local store.
/// Pure function — no network, no vault, no async.
pub fn on_credential_accepted(conn: &Connection, credential_id: &str) -> Result<(), String> {
    let Some(cred) = load_credential_row(conn, credential_id)? else {
        return Err(format!("credential not found: {credential_id}"));
    };
    // We only reward skill-kind credentials; role and custom claims
    // don't carry proficiency signal.
    if cred.claim_kind != "skill" {
        return Ok(());
    }
    let Some((level, score, skill_id)) = skill_fields(&cred)? else {
        return Ok(());
    };

    update_learner(conn, &cred.subject_did, &skill_id, level, score)?;

    if cred.issuer_did != cred.subject_did {
        update_instructor(conn, &cred.issuer_did, &skill_id, level)?;
    }
    Ok(())
}

/// Rebuild every reputation row that could be derived from `subject_did`
/// (as learner or as issuer) by replaying every accepted credential.
/// Useful for tests and for seed-time reconciliation.
pub fn recompute_for_subject(conn: &Connection, subject_did: &str) -> Result<(), String> {
    // Learner-side: clear and replay rows where actor == subject_did.
    conn.execute(
        "DELETE FROM reputation_assertions WHERE actor_address = ?1 AND role = 'learner'",
        params![subject_did],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM reputation_assertions WHERE actor_address = ?1 AND role = 'instructor'",
        params![subject_did],
    )
    .map_err(|e| e.to_string())?;

    // Pull every credential where subject_did is the subject, plus
    // every credential they've issued to someone else.
    let mut stmt = conn
        .prepare(
            "SELECT id FROM credentials \
             WHERE (subject_did = ?1 OR issuer_did = ?1) \
               AND revoked = 0 \
             ORDER BY issuance_date ASC",
        )
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = stmt
        .query_map(params![subject_did], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    for id in ids {
        on_credential_accepted(conn, &id)?;
    }
    Ok(())
}

// ---------- internal ----------

struct CredentialRow {
    issuer_did: String,
    subject_did: String,
    claim_kind: String,
    signed_vc_json: String,
}

fn load_credential_row(
    conn: &Connection,
    credential_id: &str,
) -> Result<Option<CredentialRow>, String> {
    conn.query_row(
        "SELECT issuer_did, subject_did, claim_kind, signed_vc_json \
         FROM credentials WHERE id = ?1 AND revoked = 0",
        params![credential_id],
        |row| {
            Ok(CredentialRow {
                issuer_did: row.get(0)?,
                subject_did: row.get(1)?,
                claim_kind: row.get(2)?,
                signed_vc_json: row.get(3)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// Extract `(level, score, skill_id)` from a SkillClaim VC payload.
/// Returns `None` when the JSON is a different claim shape.
fn skill_fields(cred: &CredentialRow) -> Result<Option<(i64, f64, String)>, String> {
    let value: serde_json::Value = serde_json::from_str(&cred.signed_vc_json)
        .map_err(|e| format!("parse signed_vc_json: {e}"))?;
    let claim = value
        .pointer("/credentialSubject/claim")
        .ok_or_else(|| "missing credentialSubject.claim".to_string())?;
    if claim.get("kind").and_then(|v| v.as_str()) != Some("skill") {
        return Ok(None);
    }
    let level = claim
        .get("level")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "skill claim missing integer level".to_string())?;
    let score = claim
        .get("score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let skill_id = claim
        .get("skill_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "skill claim missing skill_id".to_string())?
        .to_string();
    Ok(Some((level, score, skill_id)))
}

fn update_learner(
    conn: &Connection,
    subject_did: &str,
    skill_id: &str,
    level: i64,
    score: f64,
) -> Result<(), String> {
    let level_str = level_to_str(level);
    let id = entity_id(&[subject_did, "learner", skill_id, level_str]);

    // Learner score is the max observed on this (skill, level);
    // later credentials at the same level either match or beat the
    // score, never lower it.
    conn.execute(
        "INSERT INTO reputation_assertions \
         (id, actor_address, role, skill_id, proficiency_level, \
          score, evidence_count, computation_spec) \
         VALUES (?1, ?2, 'learner', ?3, ?4, ?5, 1, 'v3-vc') \
         ON CONFLICT(id) DO UPDATE SET \
             score = MAX(score, excluded.score), \
             evidence_count = evidence_count + 1, \
             updated_at = datetime('now')",
        params![id, subject_did, skill_id, level_str, score],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn update_instructor(
    conn: &Connection,
    issuer_did: &str,
    skill_id: &str,
    level: i64,
) -> Result<(), String> {
    let level_str = level_to_str(level);
    let id = entity_id(&[issuer_did, "instructor", skill_id, level_str]);

    // Recompute the instructor's score as the mean score of every
    // non-revoked credential they've issued at this (skill, level).
    // Using json_extract to read the score out of each row.
    let (mean_score, count): (f64, i64) = conn
        .query_row(
            "SELECT \
                COALESCE(AVG(CAST(json_extract(signed_vc_json, \
                    '$.credentialSubject.claim.score') AS REAL)), 0.0), \
                COUNT(*) \
             FROM credentials \
             WHERE issuer_did = ?1 \
               AND skill_id = ?2 \
               AND claim_kind = 'skill' \
               AND revoked = 0 \
               AND CAST(json_extract(signed_vc_json, \
                    '$.credentialSubject.claim.level') AS INTEGER) = ?3",
            params![issuer_did, skill_id, level],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    if count == 0 {
        return Ok(());
    }

    conn.execute(
        "INSERT INTO reputation_assertions \
         (id, actor_address, role, skill_id, proficiency_level, \
          score, evidence_count, computation_spec) \
         VALUES (?1, ?2, 'instructor', ?3, ?4, ?5, ?6, 'v3-vc') \
         ON CONFLICT(id) DO UPDATE SET \
             score = excluded.score, \
             evidence_count = excluded.evidence_count, \
             updated_at = datetime('now')",
        params![id, issuer_did, skill_id, level_str, mean_score, count],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        // Minimal taxonomy so skill FKs in `reputation_assertions`
        // resolve; the reputation engine doesn't use any taxonomy
        // fields beyond the skill_id that was in the credential.
        db.conn()
            .execute(
                "INSERT INTO subject_fields (id, name) VALUES ('sf', 'CS')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO subjects (id, name, subject_field_id) \
                 VALUES ('sub', 'Algo', 'sf')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO skills (id, name, subject_id) VALUES ('skill_a', 'A', 'sub')",
                [],
            )
            .unwrap();
        db
    }

    fn insert_skill_credential(
        db: &Database,
        id: &str,
        issuer: &str,
        subject: &str,
        skill_id: &str,
        level: i64,
        score: f64,
    ) {
        let vc = serde_json::json!({
            "@context": ["https://www.w3.org/2018/credentials/v1"],
            "credentialSubject": {
                "id": subject,
                "claim": {
                    "kind": "skill",
                    "skill_id": skill_id,
                    "level": level,
                    "score": score,
                    "evidence_refs": [],
                }
            }
        });
        db.conn()
            .execute(
                "INSERT INTO credentials ( \
                   id, issuer_did, subject_did, credential_type, claim_kind, \
                   skill_id, issuance_date, signed_vc_json, integrity_hash, \
                   revoked \
                 ) VALUES (?1, ?2, ?3, 'FormalCredential', 'skill', ?4, \
                           datetime('now'), ?5, 'h', 0)",
                params![
                    id,
                    issuer,
                    subject,
                    skill_id,
                    serde_json::to_string(&vc).unwrap(),
                ],
            )
            .unwrap();
    }

    #[test]
    fn learner_score_is_monotonic_max() {
        let db = test_db();
        insert_skill_credential(
            &db,
            "c1",
            "did:key:zLearner",
            "did:key:zLearner",
            "skill_a",
            2,
            0.75,
        );
        on_credential_accepted(db.conn(), "c1").unwrap();

        insert_skill_credential(
            &db,
            "c2",
            "did:key:zLearner",
            "did:key:zLearner",
            "skill_a",
            2,
            0.50,
        );
        on_credential_accepted(db.conn(), "c2").unwrap();

        insert_skill_credential(
            &db,
            "c3",
            "did:key:zLearner",
            "did:key:zLearner",
            "skill_a",
            2,
            0.95,
        );
        on_credential_accepted(db.conn(), "c3").unwrap();

        let (score, count): (f64, i64) = db
            .conn()
            .query_row(
                "SELECT score, evidence_count FROM reputation_assertions \
                 WHERE actor_address = 'did:key:zLearner' AND role = 'learner' \
                   AND skill_id = 'skill_a' AND proficiency_level = 'apply'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!((score - 0.95).abs() < 1e-9);
        assert_eq!(count, 3);
    }

    #[test]
    fn self_asserted_credential_does_not_create_instructor_row() {
        let db = test_db();
        insert_skill_credential(
            &db,
            "c1",
            "did:key:zLearner",
            "did:key:zLearner", // same → self-asserted
            "skill_a",
            2,
            0.85,
        );
        on_credential_accepted(db.conn(), "c1").unwrap();

        let instructor_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM reputation_assertions WHERE role = 'instructor'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(instructor_count, 0);
    }

    #[test]
    fn third_party_credential_credits_instructor_with_mean_score() {
        let db = test_db();
        let instructor = "did:key:zInstructor";
        insert_skill_credential(&db, "c1", instructor, "did:key:zA", "skill_a", 3, 0.80);
        insert_skill_credential(&db, "c2", instructor, "did:key:zB", "skill_a", 3, 0.90);
        on_credential_accepted(db.conn(), "c1").unwrap();
        on_credential_accepted(db.conn(), "c2").unwrap();

        let (score, count): (f64, i64) = db
            .conn()
            .query_row(
                "SELECT score, evidence_count FROM reputation_assertions \
                 WHERE actor_address = 'did:key:zInstructor' AND role = 'instructor'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!((score - 0.85).abs() < 1e-9);
        assert_eq!(count, 2);
    }

    #[test]
    fn revoked_credential_does_not_count() {
        let db = test_db();
        let instructor = "did:key:zInstructor";
        insert_skill_credential(&db, "c1", instructor, "did:key:zA", "skill_a", 3, 0.80);
        insert_skill_credential(&db, "c2", instructor, "did:key:zB", "skill_a", 3, 0.90);
        on_credential_accepted(db.conn(), "c1").unwrap();
        on_credential_accepted(db.conn(), "c2").unwrap();

        db.conn()
            .execute("UPDATE credentials SET revoked = 1 WHERE id = 'c2'", [])
            .unwrap();

        // Re-run — the revoked row must drop out of the instructor mean.
        recompute_for_subject(db.conn(), instructor).unwrap();

        let (score, count): (f64, i64) = db
            .conn()
            .query_row(
                "SELECT score, evidence_count FROM reputation_assertions \
                 WHERE actor_address = 'did:key:zInstructor' AND role = 'instructor'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!((score - 0.80).abs() < 1e-9);
        assert_eq!(count, 1);
    }

    #[test]
    fn non_skill_claims_are_ignored() {
        let db = test_db();
        let role_vc = serde_json::json!({
            "@context": ["https://www.w3.org/2018/credentials/v1"],
            "credentialSubject": {
                "id": "did:key:zLearner",
                "claim": { "kind": "role", "role": "mentor" }
            }
        });
        db.conn()
            .execute(
                "INSERT INTO credentials ( \
                   id, issuer_did, subject_did, credential_type, claim_kind, \
                   issuance_date, signed_vc_json, integrity_hash, revoked \
                 ) VALUES ('r1', 'did:key:zIssuer', 'did:key:zLearner', \
                    'RoleCredential', 'role', datetime('now'), ?1, 'h', 0)",
                params![serde_json::to_string(&role_vc).unwrap()],
            )
            .unwrap();

        on_credential_accepted(db.conn(), "r1").unwrap();

        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM reputation_assertions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn level_to_str_covers_all_valid_levels() {
        assert_eq!(level_to_str(0), "remember");
        assert_eq!(level_to_str(5), "create");
        assert_eq!(level_to_str(99), "remember"); // fallback
    }
}
