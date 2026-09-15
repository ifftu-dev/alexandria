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

use crate::profile::scope::ProfileState as State;
use rusqlite::{params, Connection, OptionalExtension};

use crate::aggregation::{
    aggregate_skill_state, AggregationConfig, AggregationInput, DerivedSkillState,
};
use crate::crypto::did::Did;
use crate::db::executor::DatabaseWorkload;
use crate::db::scoring_inputs::{scoring_input_fingerprint, verified_skill_inputs};
use crate::domain::vc::{CredentialType, ProvenanceTier, VerifiableCredential};
use crate::network_profile::embedded_preprod;
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
    crate::db::with_transaction(conn, || {
        get_derived_skill_state_in_transaction(conn, subject_did, skill_id, now)
    })
}

fn get_derived_skill_state_in_transaction(
    conn: &Connection,
    subject_did: &Did,
    skill_id: &str,
    now: &str,
) -> Result<Option<DerivedSkillState>, String> {
    refresh_invalidated_states(conn, Some(subject_did.as_str()), now)?;
    let cfg = AggregationConfig::default();
    let fingerprint =
        scoring_input_fingerprint(conn, subject_did.as_str(), skill_id, &cfg.version)?;

    // A cached state is served only while its inputs are unchanged.
    if let Some(state) = read_cached(conn, subject_did, skill_id, &cfg.version, &fingerprint)? {
        return Ok(Some(state));
    }
    refresh_pair(conn, subject_did, skill_id, now, &cfg, &fingerprint)
}

/// Recompute one pair from verified inputs and cache it with the fingerprint
/// it was computed from. No verified input removes the cached state rather
/// than leaving an old score behind.
fn refresh_pair(
    conn: &Connection,
    subject: &Did,
    skill_id: &str,
    now: &str,
    cfg: &AggregationConfig,
    fingerprint: &str,
) -> Result<Option<DerivedSkillState>, String> {
    let evidence = load_evidence_for(conn, subject, skill_id, now, cfg)?;
    if evidence.is_empty() {
        conn.execute(
            "DELETE FROM derived_skill_states WHERE subject_did = ?1 AND skill_id = ?2",
            params![subject.as_str(), skill_id],
        )
        .map_err(|e| e.to_string())?;
        return Ok(None);
    }
    let state = aggregate_skill_state(subject, skill_id, &evidence, now, cfg);
    upsert_cached(conn, &state, fingerprint)?;
    Ok(Some(state))
}

/// Drop states from other calculation versions and recompute cached states
/// whose inputs changed, so cached and direct readers agree.
fn revalidate_cached_states(
    conn: &Connection,
    subject: Option<&str>,
    now: &str,
) -> Result<(), String> {
    let cfg = AggregationConfig::default();
    conn.execute(
        "DELETE FROM derived_skill_states \
         WHERE calculation_version != ?1 AND (?2 IS NULL OR subject_did = ?2)",
        params![cfg.version, subject],
    )
    .map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT subject_did, skill_id, input_fingerprint FROM derived_skill_states \
             WHERE ?1 IS NULL OR subject_did = ?1 ORDER BY subject_did, skill_id",
        )
        .map_err(|e| e.to_string())?;
    let cached = stmt
        .query_map([subject], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    for (subject, skill, stored) in cached {
        let did = Did(subject);
        let fingerprint = scoring_input_fingerprint(conn, did.as_str(), &skill, &cfg.version)?;
        if stored.as_deref() != Some(fingerprint.as_str()) {
            refresh_pair(conn, &did, &skill, now, &cfg, &fingerprint)?;
        }
    }
    Ok(())
}

/// List cached derived states, optionally filtered by subject. States are
/// revalidated against their inputs, but pairs never computed are not
/// computed here; live recomputation is the responsibility of `recompute_all`.
pub fn list_derived_states_impl(
    conn: &Connection,
    subject_did: Option<&str>,
) -> Result<Vec<DerivedSkillState>, String> {
    crate::db::with_transaction(conn, || {
        let now = now_rfc3339();
        refresh_invalidated_states(conn, subject_did, &now)?;
        revalidate_cached_states(conn, subject_did, &now)?;
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
    })
}

/// Recompute every (subject, skill) pair present in the credentials
/// table and refresh the `derived_skill_states` cache. Returns the
/// number of (subject, skill) pairs processed.
pub fn recompute_all_impl(conn: &Connection, now: &str) -> Result<u32, String> {
    crate::db::with_transaction(conn, || recompute_all_in_transaction(conn, now))
}

fn recompute_all_in_transaction(conn: &Connection, now: &str) -> Result<u32, String> {
    refresh_invalidated_states(conn, None, now)?;
    let cfg = AggregationConfig::default();
    conn.execute(
        "DELETE FROM derived_skill_states WHERE calculation_version != ?1",
        params![cfg.version],
    )
    .map_err(|e| e.to_string())?;
    // Cached pairs are included so a pair whose inputs all disappeared is
    // removed rather than left with its old score.
    let mut stmt = conn
        .prepare(
            "SELECT subject_did, skill_id FROM scoring_credentials \
             WHERE skill_id IS NOT NULL AND revoked = 0 \
             UNION SELECT subject_did, skill_id FROM derived_skill_states \
             ORDER BY 1, 2",
        )
        .map_err(|e| e.to_string())?;
    let pairs: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    let mut count = 0u32;
    for (subject, skill) in pairs {
        let did = Did(subject);
        let fingerprint = scoring_input_fingerprint(conn, did.as_str(), &skill, &cfg.version)?;
        if refresh_pair(conn, &did, &skill, now, &cfg, &fingerprint)?.is_some() {
            count += 1;
        }
    }
    Ok(count)
}

// ---- helpers -------------------------------------------------------------

/// Repair only pairs invalidated by an exact legacy issuer match. The original
/// credentials and invalidated historical snapshots are retained. A failure
/// keeps the work queued, and no stale current cache was left by recognition.
pub(crate) fn refresh_invalidated_states(
    conn: &Connection,
    subject: Option<&str>,
    now: &str,
) -> Result<(), String> {
    let tx = if conn.is_autocommit() {
        Some(conn.unchecked_transaction().map_err(|e| e.to_string())?)
    } else {
        None
    };
    let mut stmt = conn
        .prepare(
            "SELECT subject_did, skill_id FROM derived_skill_refresh_queue \
         WHERE ?1 IS NULL OR subject_did = ?1 ORDER BY subject_did, skill_id",
        )
        .map_err(|e| e.to_string())?;
    let pairs = stmt
        .query_map([subject], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    let cfg = AggregationConfig::default();
    for (subject, skill) in pairs {
        let did = Did(subject);
        let fingerprint = scoring_input_fingerprint(conn, did.as_str(), &skill, &cfg.version)?;
        refresh_pair(conn, &did, &skill, now, &cfg, &fingerprint)?;
        conn.execute(
            "DELETE FROM derived_skill_refresh_queue WHERE subject_did = ?1 AND skill_id = ?2",
            params![did.as_str(), skill],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(tx) = tx {
        tx.commit().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Turn the verified inputs for (subject, skill) into `AggregationInput`s.
/// Only credentials that verify at `verification_time` and whose signed
/// subject, skill and id match their row are scored (see
/// `db::scoring_inputs`). The quality factors (rubric / proctoring /
/// traceability) are derived from the claim's provenance tier via
/// `config.quality_triple`; a claim with no provenance resolves to
/// `(1.0, 1.0, 1.0)`.
fn load_evidence_for(
    conn: &Connection,
    subject: &Did,
    skill_id: &str,
    verification_time: &str,
    config: &AggregationConfig,
) -> Result<Vec<AggregationInput>, String> {
    let network_id = &embedded_preprod()
        .map_err(|error| error.to_string())?
        .network_id;
    let inputs = verified_skill_inputs(
        conn,
        subject.as_str(),
        skill_id,
        verification_time,
        network_id,
    )?;
    Ok(inputs
        .into_iter()
        .map(|input| {
            let self_issued = input.self_issued();
            let credential_type = parse_credential_type(&input.credential);
            let (rubric, proctoring, traceability) = config.quality_triple(input.claim.provenance);
            AggregationInput {
                credential_id: input.credential_id,
                issuer: input.credential.issuer,
                credential_type,
                raw_score: input.claim.score.clamp(0.0, 1.0),
                issuance_time: input.credential.valid_from,
                expiration_time: input.credential.valid_until,
                rubric_completeness: rubric,
                proctoring_reliability: proctoring,
                evidence_traceability: traceability,
                self_issued,
            }
        })
        .collect())
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
    fingerprint: &str,
) -> Result<Option<DerivedSkillState>, String> {
    let row: Option<String> = conn
        .query_row(
            "SELECT state_json FROM derived_skill_states \
             WHERE subject_did = ?1 AND skill_id = ?2 AND calculation_version = ?3 \
               AND input_fingerprint = ?4",
            params![subject.as_str(), skill_id, version, fingerprint],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match row {
        Some(s) => Ok(Some(serde_json::from_str(&s).map_err(|e| e.to_string())?)),
        None => Ok(None),
    }
}

fn upsert_cached(
    conn: &Connection,
    state: &DerivedSkillState,
    fingerprint: &str,
) -> Result<(), String> {
    let json = serde_json::to_string(state).map_err(|e| e.to_string())?;
    let dominant_provenance =
        dominant_provenance_for(conn, state.subject.as_str(), &state.skill_id);
    conn.execute(
        "INSERT INTO derived_skill_states \
         (subject_did, skill_id, calculation_version, raw_score, confidence, \
          trust_score, level, evidence_mass, unique_issuer_clusters, \
          active_evidence_count, state_json, computed_at, dominant_provenance, \
          input_fingerprint) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14) \
         ON CONFLICT(subject_did, skill_id, calculation_version) DO UPDATE SET \
            input_fingerprint = excluded.input_fingerprint, \
            raw_score = excluded.raw_score, \
            confidence = excluded.confidence, \
            trust_score = excluded.trust_score, \
            level = excluded.level, \
            evidence_mass = excluded.evidence_mass, \
            unique_issuer_clusters = excluded.unique_issuer_clusters, \
            active_evidence_count = excluded.active_evidence_count, \
            state_json = excluded.state_json, \
            computed_at = excluded.computed_at, \
            dominant_provenance = excluded.dominant_provenance",
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
            dominant_provenance,
            fingerprint,
        ],
    )
    .map_err(|e| e.to_string())?;

    snapshot_history(conn, state)?;
    Ok(())
}

/// Record a dated snapshot of this state for the decay curve and velocity.
///
/// One row per (subject, skill, day): `INSERT ... ON CONFLICT DO UPDATE` so a
/// second recompute on the same day overwrites that day's snapshot rather
/// than duplicating it, keeping the history one point per day.
fn snapshot_history(conn: &Connection, state: &DerivedSkillState) -> Result<(), String> {
    conn.execute(
        "INSERT INTO derived_skill_state_history \
         (subject_did, skill_id, snapshot_date, raw_score, confidence, trust_score, \
          level, evidence_mass, computed_at) \
         VALUES (?1, ?2, date(?3), ?4, ?5, ?6, ?7, ?8, ?3) \
         ON CONFLICT(subject_did, skill_id, snapshot_date) DO UPDATE SET \
            raw_score = excluded.raw_score, \
            confidence = excluded.confidence, \
            trust_score = excluded.trust_score, \
            level = excluded.level, \
            evidence_mass = excluded.evidence_mass, \
            computed_at = excluded.computed_at, \
            input_policy_valid = 1",
        params![
            state.subject.as_str(),
            state.skill_id,
            state.computed_at,
            state.raw_score,
            state.confidence,
            state.trust_score,
            state.level,
            state.evidence_mass,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Highest provenance tier across a skill's non-revoked credentials, as its
/// snake_case token (for UI badges). `None` if no row carries a provenance.
fn dominant_provenance_for(conn: &Connection, subject_did: &str, skill_id: &str) -> Option<String> {
    let mut stmt = conn
        .prepare(
            "SELECT provenance FROM scoring_credentials \
             WHERE subject_did = ?1 AND skill_id = ?2 AND revoked = 0 \
               AND provenance IS NOT NULL",
        )
        .ok()?;
    let tiers = stmt
        .query_map(params![subject_did, skill_id], |r| r.get::<_, String>(0))
        .ok()?
        .filter_map(|r| r.ok())
        .filter_map(|s| serde_json::from_value::<ProvenanceTier>(serde_json::Value::String(s)).ok())
        .collect::<Vec<_>>();
    tiers.into_iter().max().map(|t| t.as_str().to_string())
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
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "aggregation.get_derived_skill_state",
            move |db| get_derived_skill_state_impl(db.conn(), &Did(subject_did), &skill_id, &now),
        )
        .await
}

#[tauri::command]
pub async fn list_derived_states(
    state: State<'_, AppState>,
    subject_did: Option<String>,
) -> Result<Vec<DerivedSkillState>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "aggregation.list_derived_states",
            move |db| list_derived_states_impl(db.conn(), subject_did.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn recompute_all(state: State<'_, AppState>) -> Result<u32, String> {
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Background,
            state.profile_lease(),
            "aggregation.recompute_all",
            move |db| recompute_all_impl(db.conn(), &now),
        )
        .await
}

/// One dated point on a skill's confidence/trust history.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillHistoryPoint {
    pub snapshot_date: String,
    pub raw_score: f64,
    pub confidence: f64,
    pub trust_score: f64,
    pub level: u8,
    pub evidence_mass: f64,
}

/// The dated trust/confidence history for one skill, oldest first — the data
/// behind a decay curve and a learning-velocity readout.
#[tauri::command]
pub async fn get_skill_state_history(
    state: State<'_, AppState>,
    subject_did: String,
    skill_id: String,
) -> Result<Vec<SkillHistoryPoint>, String> {
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "aggregation.get_skill_state_history",
            move |db| get_skill_state_history_impl(db.conn(), &subject_did, &skill_id, &now),
        )
        .await
}

fn get_skill_state_history_impl(
    conn: &Connection,
    subject_did: &str,
    skill_id: &str,
    now: &str,
) -> Result<Vec<SkillHistoryPoint>, String> {
    refresh_invalidated_states(conn, Some(subject_did), now)?;
    let mut stmt = conn
        .prepare(
            "SELECT snapshot_date, raw_score, confidence, trust_score, level, evidence_mass \
               FROM derived_skill_state_history \
              WHERE subject_did = ?1 AND skill_id = ?2 AND input_policy_valid = 1 \
              ORDER BY snapshot_date ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![subject_did, skill_id], |r| {
            Ok(SkillHistoryPoint {
                snapshot_date: r.get(0)?,
                raw_score: r.get(1)?,
                confidence: r.get(2)?,
                trust_score: r.get(3)?,
                level: r.get::<_, i64>(4)? as u8,
                evidence_mass: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|x| x.ok())
        .collect();
    Ok(rows)
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
                provenance: None,
            }),
            evidence_refs: vec![],
            expiration_date: None,
            supersedes: None,
            integrity_session_id: None,
            integrity_policy: None,
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

    fn cached_rows(db: &Database, subject: &Did, skill: &str) -> i64 {
        db.conn()
            .query_row(
                "SELECT COUNT(*) FROM derived_skill_states \
                 WHERE subject_did = ?1 AND skill_id = ?2",
                params![subject.as_str(), skill],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn revoking_the_last_input_removes_the_cached_state_for_every_reader() {
        let (db, subject, skill) = setup_with("skill_revoked", 0.8);
        assert!(
            get_derived_skill_state_impl(db.conn(), &subject, &skill, NOW)
                .unwrap()
                .is_some()
        );
        assert_eq!(cached_rows(&db, &subject, &skill), 1);

        db.conn()
            .execute("UPDATE credentials SET revoked = 1", [])
            .unwrap();

        // The cached reader notices the changed inputs instead of serving the
        // old score, and agrees with a direct recomputation.
        assert!(list_derived_states_impl(db.conn(), Some(subject.as_str()))
            .unwrap()
            .is_empty());
        assert_eq!(cached_rows(&db, &subject, &skill), 0);
        assert!(
            get_derived_skill_state_impl(db.conn(), &subject, &skill, NOW)
                .unwrap()
                .is_none()
        );
        assert_eq!(recompute_all_impl(db.conn(), NOW).unwrap(), 0);
    }

    #[test]
    fn a_row_whose_signed_body_was_altered_is_not_scored() {
        let (db, subject, skill) = setup_with("skill_tampered", 0.3);
        let before = get_derived_skill_state_impl(db.conn(), &subject, &skill, NOW)
            .unwrap()
            .expect("verified state");

        db.conn()
            .execute(
                "UPDATE credentials SET signed_vc_json = \
                 json_set(signed_vc_json, '$.credentialSubject.score', 1.0)",
                [],
            )
            .unwrap();

        assert!(before.raw_score < 0.5);
        assert!(
            get_derived_skill_state_impl(db.conn(), &subject, &skill, NOW)
                .unwrap()
                .is_none()
        );
        assert_eq!(cached_rows(&db, &subject, &skill), 0);
    }

    #[test]
    fn stale_cached_states_from_other_versions_are_dropped() {
        let (db, subject, skill) = setup_with("skill_versions", 0.8);
        recompute_all_impl(db.conn(), NOW).unwrap();
        db.conn()
            .execute(
                "UPDATE derived_skill_states SET calculation_version = 'retired'",
                [],
            )
            .unwrap();

        let states = list_derived_states_impl(db.conn(), Some(subject.as_str())).unwrap();

        assert!(states.is_empty());
        assert_eq!(cached_rows(&db, &subject, &skill), 0);
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
        let claim = Claim::Skill(SkillClaim {
            skill_id: "x".into(),
            level: 1,
            score: 0.0,
            evidence_refs: vec![],
            rubric_version: None,
            assessment_method: None,
            provenance: None,
        });
        let vc = VerifiableCredential {
            context: vec![],
            id: Some("x".into()),
            type_: vec![
                "VerifiableCredential".into(),
                "AttestationCredential".into(),
            ],
            issuer: Did("did:key:zI".into()),
            valid_from: NOW.into(),
            valid_until: None,
            credential_subject: claim.into_subject(Did("did:key:zS".into())),
            credential_status: None,
            terms_of_use: None,
            witness: None,
            integrity: None,
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

    fn history(db: &Database, subject: &Did, skill: &str) -> Vec<(String, f64)> {
        db.conn()
            .prepare(
                "SELECT snapshot_date, trust_score FROM derived_skill_state_history                   WHERE subject_did = ?1 AND skill_id = ?2 ORDER BY snapshot_date",
            )
            .unwrap()
            .query_map(params![subject.as_str(), skill], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
            })
            .unwrap()
            .map(|x| x.unwrap())
            .collect()
    }

    #[test]
    fn recompute_at_a_later_time_lowers_confidence() {
        // The whole point of 1.6: a credential earned long ago is worth less
        // now. Recomputing at a later `now` must decay trust, with no new
        // evidence — otherwise displayed confidence is frozen at issuance.
        let (db, subject, skill) = setup_with("skill_decay", 0.9);

        recompute_all_impl(db.conn(), "2026-04-13T00:00:00Z").unwrap();
        let fresh =
            get_derived_skill_state_impl(db.conn(), &subject, &skill, "2026-04-13T00:00:00Z")
                .unwrap()
                .unwrap()
                .trust_score;

        // Four years later, same evidence.
        recompute_all_impl(db.conn(), "2030-04-13T00:00:00Z").unwrap();
        let stale =
            get_derived_skill_state_impl(db.conn(), &subject, &skill, "2030-04-13T00:00:00Z")
                .unwrap()
                .unwrap()
                .trust_score;

        assert!(
            stale < fresh,
            "trust should decay over time: {stale} !< {fresh}"
        );
    }

    #[test]
    fn recompute_records_a_dated_history_snapshot() {
        let (db, subject, skill) = setup_with("skill_hist", 0.8);
        recompute_all_impl(db.conn(), "2026-04-13T00:00:00Z").unwrap();

        let h = history(&db, &subject, &skill);
        assert_eq!(h.len(), 1, "one snapshot after one recompute");
        assert_eq!(h[0].0, "2026-04-13", "snapshot dated to the recompute day");
    }

    #[test]
    fn same_day_recompute_overwrites_rather_than_duplicates() {
        let (db, subject, skill) = setup_with("skill_sameday", 0.8);
        recompute_all_impl(db.conn(), "2026-04-13T09:00:00Z").unwrap();
        recompute_all_impl(db.conn(), "2026-04-13T18:00:00Z").unwrap();

        let h = history(&db, &subject, &skill);
        assert_eq!(
            h.len(),
            1,
            "two recomputes on one day are one history point"
        );
    }

    #[test]
    fn history_accumulates_one_point_per_day() {
        let (db, subject, skill) = setup_with("skill_curve", 0.9);
        for day in ["2026-04-13", "2027-04-13", "2028-04-13"] {
            recompute_all_impl(db.conn(), &format!("{day}T00:00:00Z")).unwrap();
        }
        let h = history(&db, &subject, &skill);
        assert_eq!(h.len(), 3, "three distinct days, three points");
        // Trust falls across the snapshots as the evidence ages.
        assert!(h[0].1 > h[2].1, "the curve should decline: {:?}", h);
    }
}
