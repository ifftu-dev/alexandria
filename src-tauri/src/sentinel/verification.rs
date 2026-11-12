//! Verification results in an integrity session's commitment chain.
//!
//! A verification step is positive evidence that the learner did the work,
//! independent of the device: today a probe after an attempt (see
//! [`crate::assessment::probes`]), later interview follow-ups and generated
//! probes. Each result is one row in `integrity_verifications`, folded into
//! the session's `commitment_root` the same way a snapshot is, so the stream
//! of results is order-fixed and tamper-evident.
//!
//! Outcomes are `satisfied`, `failed` and `skipped`. Skipped (provider
//! unavailable, time box ran out, nothing eligible, policy `none`) never
//! counts against the learner: it only fails to raise the verification rung.
//!
//! [`session_verification_summary`] is the one read path. The credential
//! surface derives the rung and the probe summary from it at issuance.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::crypto::hash::entity_id;
use crate::sentinel::scoring::SessionStatus;

/// What a verification step concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationOutcome {
    Satisfied,
    Failed,
    Skipped,
}

impl VerificationOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Satisfied => "satisfied",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "satisfied" => Ok(Self::Satisfied),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            other => Err(format!("unknown verification outcome '{other}'")),
        }
    }
}

/// Step name for probe results.
pub const STEP_PROBE: &str = "probe";

/// Step name for an unscripted live segment in an Alexandria-hosted
/// interview. A satisfied one is the `live` verification rung.
pub const STEP_INTERVIEW: &str = "interview";

/// A result about to be recorded.
#[derive(Debug, Clone)]
pub struct NewVerification<'a> {
    pub session_id: &'a str,
    pub attempt_id: Option<&'a str>,
    /// What was verified, e.g. [`STEP_PROBE`].
    pub step: &'a str,
    /// How: the probe policy in force (`none`, `reseed`, `authored`, ...).
    pub policy: &'a str,
    pub outcome: VerificationOutcome,
    /// Why a step was skipped (`timeout`, `provider_unavailable`, ...).
    pub reason: Option<&'a str>,
    /// What the result is about, e.g. a probe id.
    pub subject_ref: Option<&'a str>,
    /// Step-specific facts (scores, latencies, grader triple). Opaque here.
    pub detail: serde_json::Value,
}

/// One recorded result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationResult {
    pub id: String,
    pub session_id: String,
    pub attempt_id: Option<String>,
    pub step: String,
    pub policy: String,
    pub outcome: VerificationOutcome,
    pub reason: Option<String>,
    pub subject_ref: Option<String>,
    pub detail: serde_json::Value,
    pub commitment_hash: String,
    pub recorded_at: String,
}

/// Every verification result of a session and what they add up to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationSummary {
    pub session_id: String,
    /// `None` when no step ran. Otherwise `failed` if any result failed,
    /// else `satisfied` if any was satisfied, else `skipped`.
    pub outcome: Option<VerificationOutcome>,
    /// Distinct policies the results were produced under, in first-use order.
    pub policies: Vec<String>,
    pub satisfied: u32,
    pub failed: u32,
    pub skipped: u32,
    /// In the order they were folded into the chain.
    pub results: Vec<VerificationResult>,
}

/// Record a verification result in a live session and fold it into the
/// session's commitment chain. Refused once the session has ended: a result
/// produced after monitoring stopped was not produced inside the session.
pub fn record_verification(
    conn: &rusqlite::Connection,
    new: &NewVerification<'_>,
) -> Result<VerificationResult, String> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let (ended_at, prev_root): (Option<String>, Option<String>) = tx
        .query_row(
            "SELECT ended_at, commitment_root FROM integrity_sessions WHERE id = ?1",
            params![new.session_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    if ended_at.is_some() {
        return Err("session has ended; verification results are recorded inside it".into());
    }

    let id = entity_id(&[
        new.session_id,
        new.step,
        new.subject_ref.unwrap_or(""),
        &chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
    ]);
    let detail_json = serde_json::to_string(&new.detail).map_err(|e| e.to_string())?;
    // Canonical bytes cover every persisted field of the result. The tag keeps
    // a result from ever colliding with a snapshot's canonical form.
    let canonical = format!(
        "verification|{id}|{}|{}|{}|{}|{}|{}|{detail_json}",
        new.attempt_id.unwrap_or(""),
        new.step,
        new.policy,
        new.outcome.as_str(),
        new.reason.unwrap_or(""),
        new.subject_ref.unwrap_or(""),
    );
    let commitment_hash = crate::domain::integrity_commitment::fold_commitment(
        prev_root.as_deref().unwrap_or(""),
        canonical.as_bytes(),
    );

    tx.execute(
        "INSERT INTO integrity_verifications \
         (id, session_id, attempt_id, step, policy, outcome, reason, subject_ref, detail_json, \
          commitment_hash) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id,
            new.session_id,
            new.attempt_id,
            new.step,
            new.policy,
            new.outcome.as_str(),
            new.reason,
            new.subject_ref,
            detail_json,
            commitment_hash,
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE integrity_sessions SET commitment_root = ?2 WHERE id = ?1",
        params![new.session_id, commitment_hash],
    )
    .map_err(|e| e.to_string())?;
    // A failed verification is credential-affecting on its own (the fusion
    // decision rule): raise the session to `flagged`. Status only moves up.
    if new.outcome == VerificationOutcome::Failed {
        let current: String = tx
            .query_row(
                "SELECT status FROM integrity_sessions WHERE id = ?1",
                params![new.session_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if SessionStatus::parse(&current).unwrap_or(SessionStatus::Active) < SessionStatus::Flagged
        {
            tx.execute(
                "UPDATE integrity_sessions SET status = ?2 WHERE id = ?1",
                params![new.session_id, SessionStatus::Flagged.as_str()],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    let result = read_result(&tx, &id)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(result)
}

/// Whether any verification step in the session failed. The fusion
/// decision's `verification_failed` input.
pub fn has_failed_verification(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM integrity_verifications \
                        WHERE session_id = ?1 AND outcome = 'failed')",
        params![session_id],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

const RESULT_COLUMNS: &str = "id, session_id, attempt_id, step, policy, outcome, reason, \
     subject_ref, detail_json, commitment_hash, recorded_at";

fn map_result(r: &rusqlite::Row<'_>) -> rusqlite::Result<VerificationResult> {
    let outcome: String = r.get(5)?;
    let detail: String = r.get(8)?;
    Ok(VerificationResult {
        id: r.get(0)?,
        session_id: r.get(1)?,
        attempt_id: r.get(2)?,
        step: r.get(3)?,
        policy: r.get(4)?,
        outcome: VerificationOutcome::parse(&outcome).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, e.into())
        })?,
        reason: r.get(6)?,
        subject_ref: r.get(7)?,
        detail: serde_json::from_str(&detail).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, e.into())
        })?,
        commitment_hash: r.get(9)?,
        recorded_at: r.get(10)?,
    })
}

fn read_result(conn: &rusqlite::Connection, id: &str) -> Result<VerificationResult, String> {
    conn.query_row(
        &format!("SELECT {RESULT_COLUMNS} FROM integrity_verifications WHERE id = ?1"),
        params![id],
        map_result,
    )
    .map_err(|e| e.to_string())
}

/// Every verification result recorded in a session, in chain order, and the
/// session-level outcome. An empty summary (`outcome: None`) means no step
/// ran, which reads exactly like `skipped`: it never counts against the
/// learner and never raises the rung.
pub fn session_verification_summary(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<VerificationSummary, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {RESULT_COLUMNS} FROM integrity_verifications \
              WHERE session_id = ?1 ORDER BY rowid"
        ))
        .map_err(|e| e.to_string())?;
    let results = stmt
        .query_map(params![session_id], map_result)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let count = |o: VerificationOutcome| results.iter().filter(|r| r.outcome == o).count() as u32;
    let (satisfied, failed, skipped) = (
        count(VerificationOutcome::Satisfied),
        count(VerificationOutcome::Failed),
        count(VerificationOutcome::Skipped),
    );
    let outcome = if results.is_empty() {
        None
    } else if failed > 0 {
        Some(VerificationOutcome::Failed)
    } else if satisfied > 0 {
        Some(VerificationOutcome::Satisfied)
    } else {
        Some(VerificationOutcome::Skipped)
    };
    let mut policies: Vec<String> = Vec::new();
    for r in &results {
        if !policies.contains(&r.policy) {
            policies.push(r.policy.clone());
        }
    }
    Ok(VerificationSummary {
        session_id: session_id.to_string(),
        outcome,
        policies,
        satisfied,
        failed,
        skipped,
        results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn db_with_session() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute(
                "INSERT INTO integrity_sessions (id, status) VALUES ('s1', 'active')",
                [],
            )
            .unwrap();
        db
    }

    fn result(outcome: VerificationOutcome, policy: &str) -> NewVerification<'_> {
        NewVerification {
            session_id: "s1",
            attempt_id: Some("att"),
            step: STEP_PROBE,
            policy,
            outcome,
            reason: (outcome == VerificationOutcome::Skipped).then_some("timeout"),
            subject_ref: Some("probe-1"),
            detail: serde_json::json!({ "latency_ms": 1200 }),
        }
    }

    fn root(db: &Database) -> Option<String> {
        db.conn()
            .query_row(
                "SELECT commitment_root FROM integrity_sessions WHERE id = 's1'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn a_session_without_verification_has_no_outcome() {
        let db = db_with_session();
        let summary = session_verification_summary(db.conn(), "s1").unwrap();
        assert_eq!(summary.outcome, None);
        assert!(summary.results.is_empty());
    }

    #[test]
    fn each_result_extends_the_commitment_chain() {
        let db = db_with_session();
        let first =
            record_verification(db.conn(), &result(VerificationOutcome::Satisfied, "reseed"))
                .unwrap();
        assert_eq!(root(&db).as_deref(), Some(first.commitment_hash.as_str()));
        let second =
            record_verification(db.conn(), &result(VerificationOutcome::Skipped, "reseed"))
                .unwrap();
        assert_ne!(first.commitment_hash, second.commitment_hash);
        assert_eq!(root(&db).as_deref(), Some(second.commitment_hash.as_str()));

        let summary = session_verification_summary(db.conn(), "s1").unwrap();
        assert_eq!(summary.results, vec![first, second]);
        assert_eq!(summary.results[0].detail["latency_ms"], 1200);
    }

    #[test]
    fn skipped_never_outweighs_a_satisfied_result_and_failed_outweighs_both() {
        let db = db_with_session();
        record_verification(db.conn(), &result(VerificationOutcome::Skipped, "plugin")).unwrap();
        let s = session_verification_summary(db.conn(), "s1").unwrap();
        assert_eq!(s.outcome, Some(VerificationOutcome::Skipped));

        record_verification(db.conn(), &result(VerificationOutcome::Satisfied, "reseed")).unwrap();
        record_verification(db.conn(), &result(VerificationOutcome::Skipped, "reseed")).unwrap();
        let s = session_verification_summary(db.conn(), "s1").unwrap();
        assert_eq!(s.outcome, Some(VerificationOutcome::Satisfied));
        assert_eq!((s.satisfied, s.failed, s.skipped), (1, 0, 2));
        assert_eq!(s.policies, vec!["plugin".to_string(), "reseed".to_string()]);

        record_verification(db.conn(), &result(VerificationOutcome::Failed, "reseed")).unwrap();
        let s = session_verification_summary(db.conn(), "s1").unwrap();
        assert_eq!(s.outcome, Some(VerificationOutcome::Failed));
    }

    fn status(db: &Database) -> String {
        db.conn()
            .query_row(
                "SELECT status FROM integrity_sessions WHERE id = 's1'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn a_failed_verification_flags_the_session_and_skipped_does_not() {
        let db = db_with_session();
        record_verification(db.conn(), &result(VerificationOutcome::Skipped, "reseed")).unwrap();
        record_verification(db.conn(), &result(VerificationOutcome::Satisfied, "reseed")).unwrap();
        assert_eq!(status(&db), "active");
        assert!(!has_failed_verification(db.conn(), "s1").unwrap());

        record_verification(db.conn(), &result(VerificationOutcome::Failed, "reseed")).unwrap();
        assert_eq!(status(&db), "flagged");
        assert!(has_failed_verification(db.conn(), "s1").unwrap());

        // Status only moves up: a suspended session stays suspended.
        db.conn()
            .execute("UPDATE integrity_sessions SET status = 'suspended'", [])
            .unwrap();
        record_verification(db.conn(), &result(VerificationOutcome::Failed, "reseed")).unwrap();
        assert_eq!(status(&db), "suspended");
    }

    #[test]
    fn grading_time_decisions_keep_a_failed_verification() {
        let db = db_with_session();
        record_verification(db.conn(), &result(VerificationOutcome::Failed, "reseed")).unwrap();
        // Grading re-runs the decision rule on the stored chain; a failed
        // verification must still flag it, whatever the extra signals say.
        db.conn()
            .execute("UPDATE integrity_sessions SET status = 'active'", [])
            .unwrap();
        let decision = crate::sentinel::snapshots::decide_with_verification(
            db.conn(),
            "s1",
            &[],
            &crate::sentinel::scoring::active_manifest(),
        )
        .unwrap();
        assert_eq!(decision.status.as_str(), "flagged");
        assert_eq!(status(&db), "flagged");
    }

    #[test]
    fn an_ended_session_takes_no_more_results() {
        let db = db_with_session();
        db.conn()
            .execute(
                "UPDATE integrity_sessions SET ended_at = datetime('now'), status = 'completed'",
                [],
            )
            .unwrap();
        let err = record_verification(db.conn(), &result(VerificationOutcome::Satisfied, "reseed"))
            .unwrap_err();
        assert!(err.contains("ended"), "{err}");
        assert_eq!(root(&db), None, "nothing was folded");
    }
}
