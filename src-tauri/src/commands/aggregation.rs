//! IPC commands for derived skill state (§14, §16, §17).
//!
//! Aggregation math lives in `crate::aggregation::aggregate_skill_state`
//! and is pure; loading inputs and caching the result live in
//! [`crate::aggregation::store`]. These commands read and recompute it, and
//! read a skill's dated history.

use crate::clock::now_rfc3339;
use crate::profile::scope::ProfileState as State;
use rusqlite::{params, Connection};

use crate::aggregation::store::{
    get_derived_skill_state_impl, list_derived_states_impl, recompute_all_impl,
};
use crate::aggregation::DerivedSkillState;
use crate::crypto::did::Did;
use crate::db::executor::DatabaseWorkload;
use crate::AppState;

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
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "aggregation.get_skill_state_history",
            move |db| get_skill_state_history_impl(db.conn(), &subject_did, &skill_id),
        )
        .await
}

fn get_skill_state_history_impl(
    conn: &Connection,
    subject_did: &str,
    skill_id: &str,
) -> Result<Vec<SkillHistoryPoint>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT snapshot_date, raw_score, confidence, trust_score, level, evidence_mass \
               FROM derived_skill_state_history \
              WHERE subject_did = ?1 AND skill_id = ?2 \
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
    use crate::aggregation::store::parse_credential_type;
    use crate::aggregation::AggregationConfig;
    use crate::commands::credentials::{issue_credential_impl, IssueCredentialRequest};
    use crate::crypto::did::derive_did_key;
    use crate::db::Database;
    use crate::domain::vc::{Claim, CredentialType, SkillClaim, VerifiableCredential};
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
            proof: crate::domain::vc::Proof::unsigned(NOW),
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
