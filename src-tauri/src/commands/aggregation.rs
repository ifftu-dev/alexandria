//! IPC commands for derived skill state (§14, §16, §17).
//!
//! Aggregation math lives in `crate::aggregation::aggregate_skill_state`
//! and is pure. The command layer here:
//!   1. Loads accepted credentials from the local store
//!   2. Converts each into an `AggregationInput`
//!   3. Runs the pipeline
//!   4. Caches the explainable output in `derived_skill_states`
//!
//! Persistence lets recruiter / consumer queries (§17) hit a fast
//! point-lookup; recompute is cheap (~ms per skill) so callers can
//! invalidate by deleting the row and re-querying.

use rusqlite::{params, Connection, OptionalExtension};
use tauri::State;

use crate::aggregation::{
    aggregate_skill_state, AggregationConfig, AggregationInput, DerivedSkillState,
};
use crate::crypto::did::Did;
use crate::domain::vc::{Claim, CredentialType, VerifiableCredential};
use crate::AppState;

/// Read a cached `DerivedSkillState` for `(subject, skill)` if one
/// exists for the current calculation version, else recompute live
/// from the local credentials and cache the result.
pub fn get_derived_skill_state_impl(
    conn: &Connection,
    subject_did: &Did,
    skill_id: &str,
    now: &str,
) -> Result<Option<DerivedSkillState>, String> {
    let cfg = AggregationConfig::default();

    // Cached lookup first — (subject, skill, version) PK gives O(1).
    if let Some(state) = read_cached(conn, subject_did, skill_id, &cfg.version)? {
        return Ok(Some(state));
    }

    // Compute live, cache, and return.
    let evidence = load_evidence_for(conn, subject_did, skill_id)?;
    if evidence.is_empty() {
        // No credentials at all ⇒ no state worth caching.
        return Ok(None);
    }
    let state = aggregate_skill_state(subject_did, skill_id, &evidence, now, &cfg);
    upsert_cached(conn, &state)?;
    Ok(Some(state))
}

/// List every cached derived state, optionally filtered by subject.
/// Live recomputation is the responsibility of `recompute_all`.
pub fn list_derived_states_impl(
    conn: &Connection,
    subject_did: Option<&str>,
) -> Result<Vec<DerivedSkillState>, String> {
    let mut sql = String::from("SELECT state_json FROM derived_skill_states");
    let mut args: Vec<String> = Vec::new();
    if let Some(s) = subject_did {
        sql.push_str(" WHERE subject_did = ?");
        args.push(s.to_string());
    }
    sql.push_str(" ORDER BY subject_did, skill_id");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args.iter()), |r| {
            r.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        let json = r.map_err(|e| e.to_string())?;
        out.push(serde_json::from_str(&json).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Recompute every (subject, skill) pair present in the credentials
/// table and refresh the `derived_skill_states` cache. Returns the
/// number of (subject, skill) pairs processed.
pub fn recompute_all_impl(conn: &Connection, now: &str) -> Result<u32, String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT subject_did, skill_id FROM credentials \
             WHERE skill_id IS NOT NULL AND revoked = 0",
        )
        .map_err(|e| e.to_string())?;
    let pairs: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|x| x.ok())
        .collect();

    let cfg = AggregationConfig::default();
    let mut count = 0u32;
    for (subject, skill) in pairs {
        let did = Did(subject);
        let evidence = load_evidence_for(conn, &did, &skill)?;
        if evidence.is_empty() {
            continue;
        }
        let state = aggregate_skill_state(&did, &skill, &evidence, now, &cfg);
        upsert_cached(conn, &state)?;
        count += 1;
    }
    Ok(count)
}

// ---- helpers -------------------------------------------------------------

/// Load every accepted credential matching (subject, skill) and turn
/// each into an `AggregationInput`. Revoked / non-skill rows are
/// excluded by the SQL — quality factors default to 1.0 since the
/// `credentials` table doesn't track rubric/proctor/trace metadata
/// in v1; richer evidence quality lands when those columns do.
fn load_evidence_for(
    conn: &Connection,
    subject: &Did,
    skill_id: &str,
) -> Result<Vec<AggregationInput>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, signed_vc_json FROM credentials \
             WHERE subject_did = ?1 AND skill_id = ?2 AND revoked = 0 \
             ORDER BY issuance_date",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![subject.as_str(), skill_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for r in rows {
        let (id, json) = r.map_err(|e| e.to_string())?;
        let vc: VerifiableCredential = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let credential_type = parse_credential_type(&vc);
        let raw_score = match &vc.credential_subject.claim {
            Claim::Skill(s) => s.score.clamp(0.0, 1.0),
            // Non-skill claims would have been filtered by the SQL;
            // defensive default if a future `claim_kind = 'skill'`
            // ever stores a non-Skill payload.
            _ => continue,
        };
        out.push(AggregationInput {
            credential_id: id,
            issuer: vc.issuer,
            credential_type,
            raw_score,
            issuance_time: vc.issuance_date,
            expiration_time: vc.expiration_date,
            rubric_completeness: 1.0,
            proctoring_reliability: 1.0,
            evidence_traceability: 1.0,
        });
    }
    Ok(out)
}

/// `vc.type_` is `["VerifiableCredential", "<class>"]` per §7. Pull
/// the class string and map it back to `CredentialType` via serde.
fn parse_credential_type(vc: &VerifiableCredential) -> CredentialType {
    for t in &vc.type_ {
        if t == "VerifiableCredential" {
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<CredentialType>(&format!("\"{t}\"")) {
            return parsed;
        }
    }
    // Fallback — treat unknown types as the lowest-weight class so
    // they don't accidentally inflate aggregation.
    CredentialType::SelfAssertion
}

fn read_cached(
    conn: &Connection,
    subject: &Did,
    skill_id: &str,
    version: &str,
) -> Result<Option<DerivedSkillState>, String> {
    let row: Option<String> = conn
        .query_row(
            "SELECT state_json FROM derived_skill_states \
             WHERE subject_did = ?1 AND skill_id = ?2 AND calculation_version = ?3",
            params![subject.as_str(), skill_id, version],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match row {
        Some(s) => Ok(Some(serde_json::from_str(&s).map_err(|e| e.to_string())?)),
        None => Ok(None),
    }
}

fn upsert_cached(conn: &Connection, state: &DerivedSkillState) -> Result<(), String> {
    let json = serde_json::to_string(state).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO derived_skill_states \
         (subject_did, skill_id, calculation_version, raw_score, confidence, \
          trust_score, level, evidence_mass, unique_issuer_clusters, \
          active_evidence_count, state_json, computed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
         ON CONFLICT(subject_did, skill_id, calculation_version) DO UPDATE SET \
            raw_score = excluded.raw_score, \
            confidence = excluded.confidence, \
            trust_score = excluded.trust_score, \
            level = excluded.level, \
            evidence_mass = excluded.evidence_mass, \
            unique_issuer_clusters = excluded.unique_issuer_clusters, \
            active_evidence_count = excluded.active_evidence_count, \
            state_json = excluded.state_json, \
            computed_at = excluded.computed_at",
        params![
            state.subject.as_str(),
            state.skill_id,
            state.calculation_version,
            state.raw_score,
            state.confidence,
            state.trust_score,
            state.level,
            state.evidence_mass,
            state.unique_issuer_clusters,
            state.active_evidence_count,
            json,
            state.computed_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

// ---- tauri command handlers ---------------------------------------------

#[tauri::command]
pub async fn get_derived_skill_state(
    state: State<'_, AppState>,
    subject_did: String,
    skill_id: String,
) -> Result<Option<DerivedSkillState>, String> {
    let now = now_rfc3339();
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    get_derived_skill_state_impl(db.conn(), &Did(subject_did), &skill_id, &now)
}

#[tauri::command]
pub async fn list_derived_states(
    state: State<'_, AppState>,
    subject_did: Option<String>,
) -> Result<Vec<DerivedSkillState>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    list_derived_states_impl(db.conn(), subject_did.as_deref())
}

#[tauri::command]
pub async fn recompute_all(state: State<'_, AppState>) -> Result<u32, String> {
    let now = now_rfc3339();
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    recompute_all_impl(db.conn(), &now)
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::credentials::{issue_credential_impl, IssueCredentialRequest};
    use crate::crypto::did::derive_did_key;
    use crate::db::Database;
    use crate::domain::vc::{Claim, SkillClaim};
    use ed25519_dalek::SigningKey;

    const NOW: &str = "2026-04-13T00:00:00Z";

    fn key(role: &str) -> SigningKey {
        let mut bytes = [0u8; 32];
        let b = role.as_bytes();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = b[i % b.len().max(1)];
        }
        SigningKey::from_bytes(&bytes)
    }

    fn setup_with(skill: &str, score: f64) -> (Database, Did, String) {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let issuer_key = key("issuer");
        let issuer = derive_did_key(&issuer_key);
        let subject = derive_did_key(&key("subject"));
        let req = IssueCredentialRequest {
            credential_type: CredentialType::FormalCredential,
            subject: subject.clone(),
            claim: Claim::Skill(SkillClaim {
                skill_id: skill.into(),
                level: 4,
                score,
                evidence_refs: vec![],
                rubric_version: Some("v1".into()),
                assessment_method: Some("exam".into()),
            }),
            evidence_refs: vec![],
            expiration_date: None,
        };
        issue_credential_impl(db.conn(), &issuer_key, &issuer, &req, NOW).unwrap();
        (db, subject, skill.to_string())
    }

    #[test]
    fn get_derived_skill_state_with_no_credentials_returns_none() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let s = get_derived_skill_state_impl(
            db.conn(),
            &Did("did:key:zUnknown".into()),
            "skill_x",
            NOW,
        )
        .unwrap();
        assert!(s.is_none());
    }

    #[test]
    fn get_derived_skill_state_caches_first_call() {
        // First call computes + caches; second call hits the cache.
        // Verify the row is present in derived_skill_states.
        let (db, subject, skill) = setup_with("skill_cache_test", 0.85);
        let first = get_derived_skill_state_impl(db.conn(), &subject, &skill, NOW)
            .unwrap()
            .expect("derived state");
        let cached: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM derived_skill_states \
                 WHERE subject_did = ?1 AND skill_id = ?2",
                params![subject.as_str(), skill],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cached, 1);
        assert!(first.raw_score > 0.0);
        // Idempotent re-call.
        let second = get_derived_skill_state_impl(db.conn(), &subject, &skill, NOW)
            .unwrap()
            .expect("re-call");
        assert_eq!(first.raw_score, second.raw_score);
    }

    #[test]
    fn list_derived_states_returns_cached_only() {
        // list_* MUST NOT trigger recomputation — it surfaces the
        // cached snapshot. Calling it without get_* first should
        // return empty even though credentials exist.
        let (db, _subject, _skill) = setup_with("skill_list_test", 0.9);
        let empty = list_derived_states_impl(db.conn(), None).unwrap();
        assert!(empty.is_empty(), "list must not auto-compute");
    }

    #[test]
    fn recompute_all_populates_cache_for_every_pair() {
        let (db, subject, skill) = setup_with("skill_recompute_test", 0.75);
        let n = recompute_all_impl(db.conn(), NOW).unwrap();
        assert_eq!(n, 1);
        let states = list_derived_states_impl(db.conn(), Some(subject.as_str())).unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].skill_id, skill);
        assert_eq!(
            states[0].calculation_version,
            AggregationConfig::default().version
        );
    }

    #[test]
    fn parse_credential_type_handles_known_classes() {
        let vc = VerifiableCredential {
            context: vec![],
            id: "x".into(),
            type_: vec![
                "VerifiableCredential".into(),
                "AttestationCredential".into(),
            ],
            issuer: Did("did:key:zI".into()),
            issuance_date: NOW.into(),
            expiration_date: None,
            credential_subject: crate::domain::vc::CredentialSubject {
                id: Did("did:key:zS".into()),
                claim: Claim::Skill(SkillClaim {
                    skill_id: "x".into(),
                    level: 1,
                    score: 0.0,
                    evidence_refs: vec![],
                    rubric_version: None,
                    assessment_method: None,
                }),
            },
            credential_status: None,
            terms_of_use: None,
            proof: crate::domain::vc::Proof {
                type_: "Ed25519Signature2020".into(),
                created: NOW.into(),
                verification_method: crate::crypto::did::VerificationMethodRef("x".into()),
                proof_purpose: "assertionMethod".into(),
                jws: String::new(),
            },
        };
        assert_eq!(
            parse_credential_type(&vc),
            CredentialType::AttestationCredential
        );
    }
}
