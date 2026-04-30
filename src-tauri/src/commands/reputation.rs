//! Reputation IPC (VC-first rebuild).
//!
//! Exposes the rebuilt reputation engine to the frontend:
//!   * `list_reputation_rows` — query the stored `reputation_assertions`
//!     with optional filters.
//!   * `recompute_reputation_for_subject` — replay every accepted
//!     credential for a given subject DID, producing a fresh learner
//!     row (and any issuer rows if that subject also issued credentials).
//!
//! The heavy whitepaper pipeline (distribution metrics, impact
//! deltas, full replay, verification) is deferred — it can be
//! layered on top of the simpler VC-sourced engine if/when there's
//! enough accumulated credential volume to warrant it.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::evidence::reputation;
use crate::AppState;

/// A single reputation row surfaced to the frontend. Mirrors the
/// subset of columns the VC-sourced engine populates (the legacy
/// distribution columns stay `NULL` and are omitted from the shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationRow {
    pub id: String,
    pub actor_address: String,
    pub role: String,
    pub skill_id: Option<String>,
    pub proficiency_level: Option<String>,
    pub score: f64,
    pub evidence_count: i64,
    pub computation_spec: String,
    pub updated_at: String,
}

/// IPC filter for [`list_reputation_rows`]. All fields are optional.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReputationQuery {
    pub actor: Option<String>,
    pub role: Option<String>,
    pub skill_id: Option<String>,
    pub proficiency_level: Option<String>,
    pub limit: Option<i64>,
}

#[tauri::command]
pub async fn list_reputation_rows(
    state: State<'_, AppState>,
    query: ReputationQuery,
) -> Result<Vec<ReputationRow>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let mut conditions: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut i = 1;
    if let Some(ref v) = query.actor {
        conditions.push(format!("actor_address = ?{i}"));
        values.push(Box::new(v.clone()));
        i += 1;
    }
    if let Some(ref v) = query.role {
        conditions.push(format!("role = ?{i}"));
        values.push(Box::new(v.clone()));
        i += 1;
    }
    if let Some(ref v) = query.skill_id {
        conditions.push(format!("skill_id = ?{i}"));
        values.push(Box::new(v.clone()));
        i += 1;
    }
    if let Some(ref v) = query.proficiency_level {
        conditions.push(format!("proficiency_level = ?{i}"));
        values.push(Box::new(v.clone()));
        i += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT id, actor_address, role, skill_id, proficiency_level, \
                score, evidence_count, computation_spec, updated_at \
         FROM reputation_assertions {where_clause} \
         ORDER BY updated_at DESC LIMIT ?{i}"
    );
    let limit = query.limit.unwrap_or(100);
    values.push(Box::new(limit));

    let params_ref: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(ReputationRow {
                id: row.get(0)?,
                actor_address: row.get(1)?,
                role: row.get(2)?,
                skill_id: row.get(3)?,
                proficiency_level: row.get(4)?,
                score: row.get(5)?,
                evidence_count: row.get(6)?,
                computation_spec: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[tauri::command]
pub async fn recompute_reputation_for_subject(
    state: State<'_, AppState>,
    subject_did: String,
) -> Result<i64, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    reputation::recompute_for_subject(db.conn(), &subject_did)?;
    let count: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM reputation_assertions WHERE actor_address = ?1",
            params![subject_did],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use rusqlite::params;

    #[test]
    fn query_builder_respects_filters() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        // Taxonomy for FKs
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
                "INSERT INTO skills (id, name, subject_id) VALUES ('sk', 'A', 'sub')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO reputation_assertions \
                   (id, actor_address, role, skill_id, proficiency_level, score, \
                    evidence_count, computation_spec) \
                 VALUES ('a', 'did:A', 'learner', 'sk', 'apply', 0.9, 1, 'v3-vc'), \
                        ('b', 'did:B', 'instructor', 'sk', 'apply', 0.8, 3, 'v3-vc')",
                [],
            )
            .unwrap();

        // No filter → both rows
        let all = fetch(&db, ReputationQuery::default());
        assert_eq!(all.len(), 2);

        // Actor filter
        let one = fetch(
            &db,
            ReputationQuery {
                actor: Some("did:A".into()),
                ..Default::default()
            },
        );
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].role, "learner");

        // Role filter
        let instructors = fetch(
            &db,
            ReputationQuery {
                role: Some("instructor".into()),
                ..Default::default()
            },
        );
        assert_eq!(instructors.len(), 1);
        assert_eq!(instructors[0].actor_address, "did:B");
    }

    fn fetch(db: &Database, q: ReputationQuery) -> Vec<ReputationRow> {
        // Mirror the command body without going through Tauri State.
        let conn = db.conn();
        let mut conditions: Vec<String> = Vec::new();
        let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut i = 1;
        if let Some(ref v) = q.actor {
            conditions.push(format!("actor_address = ?{i}"));
            values.push(Box::new(v.clone()));
            i += 1;
        }
        if let Some(ref v) = q.role {
            conditions.push(format!("role = ?{i}"));
            values.push(Box::new(v.clone()));
            i += 1;
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        values.push(Box::new(1000i64));
        let sql = format!(
            "SELECT id, actor_address, role, skill_id, proficiency_level, \
                    score, evidence_count, computation_spec, updated_at \
             FROM reputation_assertions {where_clause} \
             ORDER BY updated_at DESC LIMIT ?{i}"
        );
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            values.iter().map(|v| v.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).unwrap();
        stmt.query_map(params_ref.as_slice(), |row| {
            Ok(ReputationRow {
                id: row.get(0)?,
                actor_address: row.get(1)?,
                role: row.get(2)?,
                skill_id: row.get(3)?,
                proficiency_level: row.get(4)?,
                score: row.get(5)?,
                evidence_count: row.get(6)?,
                computation_spec: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    }

    #[test]
    fn recompute_from_credentials_round_trip() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
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
                "INSERT INTO skills (id, name, subject_id) VALUES ('sk', 'A', 'sub')",
                [],
            )
            .unwrap();

        let vc = serde_json::json!({
            "@context": ["https://www.w3.org/ns/credentials/v2"],
            "credentialSubject": {
                "id": "did:L",
                "skillId": "sk",
                "level": 2,
                "score": 0.77,
                "evidenceRefs": [],
            }
        });
        db.conn()
            .execute(
                "INSERT INTO credentials ( \
                   id, issuer_did, subject_did, credential_type, claim_kind, \
                   skill_id, issuance_date, signed_vc_json, integrity_hash, \
                   revoked \
                 ) VALUES ('c1', 'did:L', 'did:L', 'SelfAssertion', 'skill', 'sk', \
                    datetime('now'), ?1, 'h', 0)",
                params![serde_json::to_string(&vc).unwrap()],
            )
            .unwrap();

        reputation::recompute_for_subject(db.conn(), "did:L").unwrap();
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM reputation_assertions WHERE actor_address = 'did:L'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // One learner row; no instructor row (self-asserted).
        assert_eq!(count, 1);
    }
}
