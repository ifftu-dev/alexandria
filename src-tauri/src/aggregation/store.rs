//! Derived skill state, stored: the cache in `derived_skill_states` and its
//! dated history.
//!
//! The math in [`super::aggregate_skill_state`] is pure. This module:
//!   1. Loads accepted credentials from the local store
//!   2. Converts each into an [`AggregationInput`]
//!   3. Runs the pipeline
//!   4. Caches the explainable output in `derived_skill_states`
//!
//! Persistence lets recruiter / consumer queries (§17) hit a fast
//! point-lookup; recompute is cheap (~ms per skill) so callers can
//! invalidate by deleting the row and re-querying. The derived state
//! commands, the background recompute and every issuance path that must
//! refresh scores call it.

use rusqlite::{params, Connection, OptionalExtension};

use super::{aggregate_skill_state, AggregationConfig, AggregationInput, DerivedSkillState};
use crate::clock::now_rfc3339;
use crate::crypto::did::Did;
use crate::db::scoring_inputs::{scoring_input_fingerprint, verified_skill_inputs};
use crate::domain::vc::{CredentialType, VerifiableCredential};
use crate::network_profile::embedded_preprod;

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
    let (evidence, dominant_provenance) = load_evidence_for(conn, subject, skill_id, now, cfg)?;
    if evidence.is_empty() {
        conn.execute(
            "DELETE FROM derived_skill_states WHERE subject_did = ?1 AND skill_id = ?2",
            params![subject.as_str(), skill_id],
        )
        .map_err(|e| e.to_string())?;
        return Ok(None);
    }
    let state = aggregate_skill_state(subject, skill_id, &evidence, now, cfg);
    upsert_cached(conn, &state, fingerprint, dominant_provenance)?;
    Ok(Some(state))
}

/// Drop states from other calculation versions and recompute cached states
/// whose inputs changed, so cached and direct readers agree.
pub(crate) fn revalidate_cached_states(
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
            "SELECT subject_did, skill_id FROM credentials \
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

/// Turn the verified inputs for (subject, skill) into `AggregationInput`s,
/// with the highest provenance tier among them as its snake_case token for
/// UI badges.
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
) -> Result<(Vec<AggregationInput>, Option<String>), String> {
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
    let dominant_provenance = inputs
        .iter()
        .filter_map(|input| input.claim.provenance.as_ref())
        .max()
        .map(|tier| tier.as_str().to_string());
    let evidence = inputs
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
        .collect();
    Ok((evidence, dominant_provenance))
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
    dominant_provenance: Option<String>,
) -> Result<(), String> {
    let json = serde_json::to_string(state).map_err(|e| e.to_string())?;
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
            computed_at = excluded.computed_at",
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

/// What the store's tests in `commands::aggregation` reach that production
/// code does not. They stay there until credential issuance moves out of
/// the commands layer.
#[cfg(test)]
pub(crate) mod test_support {
    use crate::domain::vc::{CredentialType, VerifiableCredential};

    pub(crate) fn parse_credential_type(vc: &VerifiableCredential) -> CredentialType {
        super::parse_credential_type(vc)
    }
}
