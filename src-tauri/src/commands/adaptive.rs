//! Adaptive assessment delivery — the turn-by-turn IPC over
//! [`crate::assessment::adaptive`] and [`crate::assessment::irt`].
//!
//! A fixed-form attempt (`commands::assessment`) draws its whole item set up
//! front and grades it in one shot. An adaptive attempt instead runs a loop:
//! serve one item, grade the answer, re-estimate ability, and either serve the
//! next item chosen from that estimate or stop once the estimate is precise
//! enough. It shares the container (`assessment_attempts`) and the per-item
//! store (`attempt_items`) with the fixed path, and reuses the same attempt
//! policy, grader, and credential issuance — only the delivery differs.
//!
//! # State lives in the database, one row per served item
//!
//! When an item is served it gets a `attempt_items` row with its shuffled
//! option order but no score — a *pending* row. Submitting that item's answer
//! grades it against the stored order and fills the row in, so the answer key
//! is read only host-side and the client never learns whether a single item
//! was correct: `submit` returns only whether the attempt is finished and how
//! many items remain, never correctness. Ability is re-estimated from every
//! graded row after each answer, and `theta_after` / `se_after` on the row
//! record how the estimate moved.
//!
//! # Item parameters
//!
//! 2PL parameters are bootstrapped from each item's 1–5 difficulty
//! ([`ItemParams::from_difficulty`]); calibrated parameters from response data
//! are a later, opt-in change. So adaptive delivery works on day one, just
//! less sharply than it will once items are calibrated.

use crate::profile::scope::ProfileState as State;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::assessment::adaptive::{select_next_item, should_stop, PoolItem, StopRule};
use crate::assessment::irt::{estimate_theta_eap, theta_to_score, AbilityEstimate, ItemParams};
use crate::assessment::items::{self, GradeEngine, ServedItem};
use crate::assessment::{shuffle, SplitMix64};
use crate::commands::assessment::{
    end_interrupted_attempts, ensure_attempt_owner, load_attempt_history,
    require_live_integrity_session, resolve_bank_policy, GradeResult,
};
use crate::commands::credentials::{load_issuer_key, now_rfc3339};
use crate::db::executor::DatabaseWorkload;
use crate::domain::vc::{Claim, CredentialType, SkillClaim};
use crate::settings::{registry::keys, SettingsStore};
use crate::AppState;

/// The learner's view after starting or advancing an adaptive attempt.
#[derive(Debug, Clone, Serialize)]
pub struct AdaptiveStep {
    pub attempt_id: String,
    /// The next item to answer, or `None` when the attempt has finished.
    pub item: Option<ServedItem>,
    /// How many items have been served so far.
    pub items_served: u32,
    /// True once no further items will be served — the learner should
    /// finalize. Deliberately carries no score or correctness.
    pub finished: bool,
}

// ---- start --------------------------------------------------------------

/// Begin an adaptive attempt for `skill_id`: enforce the attempt policy,
/// create the attempt, and serve the first item (chosen from the prior).
#[tauri::command]
pub async fn assessment_start_adaptive(
    state: State<'_, AppState>,
    skill_id: String,
    integrity_session_id: Option<String>,
) -> Result<AdaptiveStep, String> {
    let seed: u64 = rand::random();
    let attempt_id = now_rfc3339() + "-a-" + &seed.to_string();
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.start_adaptive",
            move |db| start_adaptive_db(db, skill_id, integrity_session_id, seed, attempt_id, now),
        )
        .await
}

fn start_adaptive_db(
    db: &crate::db::Database,
    skill_id: String,
    integrity_session_id: Option<String>,
    seed: u64,
    attempt_id: String,
    now: String,
) -> Result<AdaptiveStep, String> {
    if crate::commands::diagnostics::is_enabled() {
        return Err("exit diagnostics before starting an assessment".into());
    }
    let conn = db.conn();

    let subject_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
    if subject_did.is_empty() {
        return Err("no local identity".into());
    }
    let integrity_session_id =
        require_live_integrity_session(conn, integrity_session_id.as_deref())?.to_string();
    end_interrupted_attempts(conn, &subject_did, &skill_id, &integrity_session_id, &now)?;

    // A ratified adaptive bank for the skill, with its policy.
    let (bank_id, policy) = resolve_bank_policy(conn, &skill_id)?
        .ok_or_else(|| format!("no assessment available for skill '{skill_id}'"))?;
    let delivery: String = conn
        .query_row(
            "SELECT delivery_mode FROM question_banks WHERE id = ?1",
            params![bank_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if delivery != "adaptive" {
        return Err(format!(
            "assessment for '{skill_id}' is fixed-form — use assessment_start_attempt"
        ));
    }

    // Same credential-bearing-attempt rate limit as the fixed path.
    let history = load_attempt_history(conn, &subject_did, &skill_id)?;
    let attempt_ordinal =
        match crate::assessment::policy::evaluate_attempt_policy(&history, &policy, &now) {
            crate::assessment::policy::PolicyDecision::Allow { ordinal } => ordinal,
            refused => return Err(refused.refusal().unwrap_or_default()),
        };

    // Interruption accounting above must remain committed even when policy
    // now refuses a replacement. The transaction begins only for the
    // idempotent resume read or the new attempt + first-item write.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;

    // Return the current state of an existing open attempt rather than
    // creating a second adaptive draw. Answered turns are already durable in
    // `attempt_items`; at most one pending item can be waiting for input.
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM assessment_attempts \
              WHERE subject_did = ?1 AND skill_id = ?2 \
                AND graded_at IS NULL AND ended_at IS NULL \
              ORDER BY started_at DESC LIMIT 1",
            params![subject_did, skill_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(existing_id) = existing {
        let pending: Option<(String, String)> = conn
            .query_row(
                "SELECT item_id, option_order FROM attempt_items \
                  WHERE attempt_id = ?1 AND score IS NULL ORDER BY ordinal LIMIT 1",
                [&existing_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let items_served: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM attempt_items WHERE attempt_id = ?1",
                [&existing_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| e.to_string())?
            .max(0) as u32;
        let item = if let Some((item_id, order_json)) = pending {
            let item = items::load_item(db, &item_id)?.ok_or("served item no longer exists")?;
            let order: Vec<usize> = serde_json::from_str(&order_json).unwrap_or_default();
            Some(items::served_item(&item, Some(&order)))
        } else {
            None
        };
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(AdaptiveStep {
            attempt_id: existing_id,
            finished: item.is_none(),
            item,
            items_served,
        });
    }

    conn.execute(
        "INSERT INTO assessment_attempts \
         (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders, \
          integrity_session_id, started_at, attempt_ordinal) \
         VALUES (?1, ?2, ?3, ?4, ?5, '[]', '[]', ?6, ?7, ?8)",
        params![
            attempt_id,
            subject_did,
            bank_id,
            skill_id,
            seed as i64,
            integrity_session_id,
            now,
            attempt_ordinal as i64,
        ],
    )
    .map_err(|e| e.to_string())?;

    // Serve the first item from the prior estimate.
    let served = serve_next(
        db,
        &attempt_id,
        &bank_id,
        seed,
        AbilityEstimate::prior().theta,
    )?
    .ok_or("assessment bank is empty")?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(AdaptiveStep {
        attempt_id,
        item: Some(served),
        items_served: 1,
        finished: false,
    })
}

// ---- submit -------------------------------------------------------------

/// Grade the learner's answer to the current item, re-estimate ability, and
/// either serve the next item or signal that the attempt is finished.
///
/// Returns no correctness signal — only the next item (if any) and the served
/// count — so the learner cannot read the answer key off the response.
#[tauri::command]
pub async fn assessment_submit_adaptive_item(
    state: State<'_, AppState>,
    attempt_id: String,
    item_id: String,
    selected: Vec<usize>,
) -> Result<AdaptiveStep, String> {
    let now = now_rfc3339();
    #[cfg(desktop)]
    let grader_runtime = state.grader_runtime.clone();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.submit_adaptive_item",
            move |db| {
                #[cfg(desktop)]
                let engine = GradeEngine {
                    runtime: &grader_runtime,
                    budgets: Default::default(),
                };
                #[cfg(not(desktop))]
                let engine = GradeEngine::default();
                submit_adaptive_impl(db, &engine, &attempt_id, &item_id, &selected, &now)
            },
        )
        .await
}

/// Core of `assessment_submit_adaptive_item`, separated from Tauri state so
/// the delivery loop is testable end to end.
pub fn submit_adaptive_impl(
    db: &crate::db::Database,
    engine: &GradeEngine<'_>,
    attempt_id: &str,
    item_id: &str,
    selected: &[usize],
    now: &str,
) -> Result<AdaptiveStep, String> {
    let conn = db.conn();
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let actor_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
    ensure_attempt_owner(conn, attempt_id, &actor_did)?;

    let (bank_id, seed, graded_at, ended_at): (String, i64, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT bank_id, seed, graded_at, ended_at FROM assessment_attempts WHERE id = ?1",
            params![attempt_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("attempt not found")?;
    if graded_at.is_some() {
        return Err("attempt already finalized".into());
    }
    if ended_at.is_some() {
        return Err("attempt was ended and cannot be resumed".into());
    }

    // The pending row for this item carries the shuffled order it was served
    // in. Its absence means the client submitted an item it was not served —
    // reject rather than grade something unserved.
    let (order_json, existing_score): (Option<String>, Option<f64>) = conn
        .query_row(
            "SELECT option_order, score FROM attempt_items \
              WHERE attempt_id = ?1 AND item_id = ?2",
            params![attempt_id, item_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("item was not served in this attempt")?;
    if existing_score.is_some() {
        return Err("item already answered".into());
    }

    let option_order: Vec<usize> = order_json
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();

    // Grade host-side. The key is read inside grade_item and never returned.
    let item = items::load_item(db, item_id)?.ok_or("item no longer exists")?;
    let grade = items::grade_item(
        db,
        engine,
        &item,
        &serde_json::json!({ "selected_positions": selected }),
        Some(&option_order),
    )?;

    // Re-estimate ability over every graded item once this one is folded in,
    // so theta_after/se_after on the row reflect the post-answer estimate.
    let mut responses = graded_responses(conn, attempt_id)?;
    responses.push((
        ItemParams::from_difficulty(item.difficulty),
        grade.score >= 0.5,
    ));
    let estimate = estimate_theta_eap(&responses);

    conn.execute(
        "UPDATE attempt_items SET submission_json = ?1, grader_cid = ?2, content_cid = ?3, \
             submission_cid = ?4, score = ?5, score_details = ?6, theta_after = ?7, \
             se_after = ?8, graded_at = ?9 \
           WHERE attempt_id = ?10 AND item_id = ?11",
        params![
            serde_json::to_string(&serde_json::json!({ "selected_positions": selected }))
                .unwrap_or_default(),
            grade.grader_cid,
            grade.content_cid,
            grade.submission_cid,
            grade.score,
            serde_json::to_string(&grade.details).unwrap_or_default(),
            estimate.theta,
            estimate.se,
            now,
            attempt_id,
            item_id,
        ],
    )
    .map_err(|e| e.to_string())?;

    let answered = responses.len() as u32;
    let rule = stop_rule(conn, &bank_id)?;

    if should_stop(&estimate, answered, &rule) {
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(AdaptiveStep {
            attempt_id: attempt_id.to_string(),
            item: None,
            items_served: answered,
            finished: true,
        });
    }

    // Not stopping: serve the next item chosen from the current estimate. If
    // the pool is exhausted the attempt finishes regardless of precision.
    let step = match serve_next(db, attempt_id, &bank_id, seed as u64, estimate.theta)? {
        Some(served) => AdaptiveStep {
            attempt_id: attempt_id.to_string(),
            item: Some(served),
            items_served: answered + 1,
            finished: false,
        },
        None => AdaptiveStep {
            attempt_id: attempt_id.to_string(),
            item: None,
            items_served: answered,
            finished: true,
        },
    };
    tx.commit().map_err(|e| e.to_string())?;
    Ok(step)
}

// ---- finalize -----------------------------------------------------------

/// Close a finished adaptive attempt: score it from the final ability
/// estimate, and on a pass issue an integrity-bound `AssessmentCredential`.
#[tauri::command]
pub async fn assessment_finalize_adaptive(
    state: State<'_, AppState>,
    attempt_id: String,
) -> Result<GradeResult, String> {
    let (signing_key, issuer_did) = load_issuer_key(&state).await?;
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.finalize_adaptive",
            move |db| finalize_adaptive_impl(db, &signing_key, &issuer_did, &attempt_id, &now),
        )
        .await
}

/// Core of `assessment_finalize_adaptive`, separated from Tauri state.
pub fn finalize_adaptive_impl(
    db: &crate::db::Database,
    signing_key: &ed25519_dalek::SigningKey,
    issuer_did: &crate::crypto::did::Did,
    attempt_id: &str,
    now: &str,
) -> Result<GradeResult, String> {
    if crate::crypto::did::derive_did_key(signing_key).as_str() != issuer_did.as_str() {
        return Err("assessment signing key does not match the learner".into());
    }
    let conn = db.conn();
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    ensure_attempt_owner(conn, attempt_id, issuer_did.as_str())?;

    let (bank_id, skill_id, integrity_session_id, attempt_ordinal, graded_at, ended_at): (
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT bank_id, skill_id, integrity_session_id, attempt_ordinal, graded_at, ended_at \
               FROM assessment_attempts WHERE id = ?1",
            params![attempt_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("attempt not found")?;
    if graded_at.is_some() {
        return Err("attempt already finalized".into());
    }
    if ended_at.is_some() {
        return Err("attempt was ended and cannot be resumed".into());
    }

    let responses = graded_responses(conn, attempt_id)?;
    if responses.is_empty() {
        return Err("attempt has no graded items".into());
    }
    let estimate = estimate_theta_eap(&responses);
    let score = theta_to_score(estimate.theta);

    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM attempt_items WHERE attempt_id = ?1 AND score IS NULL",
            params![attempt_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if pending > 0 {
        return Err("adaptive attempt still has an unanswered item".into());
    }
    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assessment_items i WHERE bank_id = ?1 AND NOT EXISTS
         (SELECT 1 FROM attempt_items a WHERE a.attempt_id = ?2 AND a.item_id = i.id)",
            params![bank_id, attempt_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if remaining > 0
        && !should_stop(
            &estimate,
            responses.len() as u32,
            &stop_rule(conn, &bank_id)?,
        )
    {
        return Err("adaptive attempt has not reached its stopping rule".into());
    }

    let threshold: f64 = conn
        .query_row(
            "SELECT pass_threshold FROM question_banks WHERE id = ?1",
            params![bank_id],
            |r| r.get(0),
        )
        .unwrap_or(0.7);
    let passed = score >= threshold;

    let mut credential_id = None;
    if passed {
        // Carry the per-item reproducibility triples and the ability estimate
        // into the credential, same shape as the fixed path.
        let mut evidence_refs = vec![attempt_id.to_string()];
        let triples: Vec<(String, String, String)> = conn
            .prepare(
                "SELECT grader_cid, content_cid, submission_cid FROM attempt_items \
                  WHERE attempt_id = ?1 AND score IS NOT NULL ORDER BY ordinal",
            )
            .and_then(|mut s| {
                s.query_map(params![attempt_id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .map_err(|e| e.to_string())?;
        for (g, c, s) in triples {
            evidence_refs.push(format!("grade:{g}:{c}:{s}"));
        }
        evidence_refs.push(format!("theta:{:.4}:se:{:.4}", estimate.theta, estimate.se));
        if let Some(ord) = attempt_ordinal {
            evidence_refs.push(format!("attempt_ordinal:{ord}"));
        }

        let claim = SkillClaim {
            skill_id: skill_id.clone(),
            level: crate::aggregation::level::map_level(score),
            score,
            evidence_refs: evidence_refs.clone(),
            rubric_version: None,
            assessment_method: Some("adaptive_quiz".into()),
            provenance: None,
        };
        let req = crate::commands::credentials::IssueCredentialRequest {
            credential_type: CredentialType::AssessmentCredential,
            subject: issuer_did.clone(),
            claim: Claim::Skill(claim),
            evidence_refs,
            expiration_date: None,
            supersedes: None,
            integrity_session_id: integrity_session_id.clone(),
            integrity_policy: None,
        };
        let vc = crate::commands::credentials::issue_credential_impl(
            conn,
            signing_key,
            issuer_did,
            &req,
            now,
        )?;
        credential_id = Some(vc.id.ok_or("issued assessment credential has no ID")?);
    }

    conn.execute(
        "UPDATE assessment_attempts SET score = ?1, passed = ?2, credential_id = ?3, graded_at = ?4 \
           WHERE id = ?5",
        params![score, passed as i64, credential_id, now, attempt_id],
    )
    .map_err(|e| e.to_string())?;

    if passed {
        crate::commands::aggregation::recompute_all_impl(conn, now)?;
    }

    tx.commit().map_err(|e| e.to_string())?;
    Ok(GradeResult {
        score,
        passed,
        credential_id,
    })
}

// ---- shared helpers -----------------------------------------------------

/// Select and serve the next item: build the pool with bootstrapped 2PL
/// parameters, pick from the current estimate excluding administered items,
/// shuffle its options deterministically, persist a pending `attempt_items`
/// row, and return the client-facing view. `None` when the pool is exhausted.
fn serve_next(
    db: &crate::db::Database,
    attempt_id: &str,
    bank_id: &str,
    seed: u64,
    theta: f64,
) -> Result<Option<ServedItem>, String> {
    let conn = db.conn();
    // The whole bank as a selectable pool.
    let pool: Vec<PoolItem> = conn
        .prepare("SELECT id, difficulty FROM assessment_items WHERE bank_id = ?1 ORDER BY id")
        .and_then(|mut s| {
            s.query_map(params![bank_id], |r| {
                let id: String = r.get(0)?;
                let difficulty: i64 = r.get(1)?;
                Ok(PoolItem {
                    id,
                    params: ItemParams::from_difficulty(difficulty as u8),
                })
            })
            .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        })
        .map_err(|e| e.to_string())?;

    // Items already served (pending or graded) in this attempt.
    let administered: Vec<String> = conn
        .prepare("SELECT item_id FROM attempt_items WHERE attempt_id = ?1")
        .and_then(|mut s| {
            s.query_map(params![attempt_id], |r| r.get::<_, String>(0))
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        })
        .map_err(|e| e.to_string())?;

    let ordinal = administered.len() as i64;

    // Deterministic per-attempt selection: fold the ordinal into the seed so
    // each step's draw is reproducible yet distinct.
    let mut rng = SplitMix64(seed ^ (ordinal as u64).wrapping_mul(0x9E37_79B9));
    let Some(chosen) = select_next_item(&pool, &administered, theta, &mut rng) else {
        return Ok(None);
    };
    let chosen_id = chosen.id.clone();

    // Shuffle the chosen item's options with the same RNG.
    let item = items::load_item(db, &chosen_id)?.ok_or("selected item vanished")?;
    let option_count = item
        .content_public
        .get("options")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let mut order: Vec<usize> = (0..option_count).collect();
    shuffle(&mut order, &mut rng);

    // Pending row: served, not yet graded.
    conn.execute(
        "INSERT INTO attempt_items (attempt_id, ordinal, item_id, option_order) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            attempt_id,
            ordinal,
            chosen_id,
            serde_json::to_string(&order).unwrap_or_default(),
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(Some(items::served_item(&item, Some(&order))))
}

/// The graded responses for an attempt as `(item params, correct)`, in served
/// order. Correctness dichotomises the item score at 0.5 for the 2PL model.
fn graded_responses(
    conn: &rusqlite::Connection,
    attempt_id: &str,
) -> Result<Vec<(ItemParams, bool)>, String> {
    conn.prepare(
        "SELECT ai.score, i.difficulty FROM attempt_items ai \
         JOIN assessment_items i ON i.id = ai.item_id \
         WHERE ai.attempt_id = ?1 AND ai.score IS NOT NULL ORDER BY ai.ordinal",
    )
    .and_then(|mut s| {
        s.query_map(params![attempt_id], |r| {
            let score: f64 = r.get(0)?;
            let difficulty: i64 = r.get(1)?;
            Ok((ItemParams::from_difficulty(difficulty as u8), score >= 0.5))
        })
        .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
    })
    .map_err(|e| e.to_string())
}

/// The stop rule configured on a bank (falls back to defaults).
fn stop_rule(conn: &rusqlite::Connection, bank_id: &str) -> Result<StopRule, String> {
    conn.query_row(
        "SELECT adaptive_se_target, adaptive_min_items, adaptive_max_items \
           FROM question_banks WHERE id = ?1",
        params![bank_id],
        |r| {
            Ok(StopRule {
                se_target: r.get(0)?,
                min_items: r.get::<_, i64>(1)?.max(1) as u32,
                max_items: r.get::<_, i64>(2)?.max(1) as u32,
            })
        },
    )
    .map_err(|e| e.to_string())
}

/// End-to-end adaptive delivery against a real database and the built-in
/// grader. Desktop-gated because it drives the wasm runtime; the native MCQ
/// path is proven equivalent by `assessment::mcq`.
#[cfg(all(test, desktop))]
mod tests {
    use super::*;
    use crate::crypto::did::{derive_did_key, Did};
    use crate::db::Database;
    use crate::plugins::wasm_runtime::GraderRuntime;
    use crate::plugins::{builtins, registry};
    use ed25519_dalek::SigningKey;
    use tempfile::TempDir;

    const NOW: &str = "2026-07-24T00:00:00Z";

    struct Ctx {
        db: Database,
        runtime: GraderRuntime,
        key: SigningKey,
        did: Did,
        _dir: TempDir,
    }

    impl Ctx {
        fn engine(&self) -> GradeEngine<'_> {
            GradeEngine {
                runtime: &self.runtime,
                budgets: Default::default(),
            }
        }
    }

    /// An adaptive bank of 12 single-answer items spanning difficulties 1..5,
    /// correct answer always option 0. Low min-items so tests finish fast.
    fn setup() -> Ctx {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let dir = TempDir::new().unwrap();
        let mcq = builtins::BUILTIN_PLUGINS
            .iter()
            .find(|b| b.slug == "mcq")
            .unwrap();
        registry::install_builtin(&db, dir.path(), mcq).unwrap();

        db.conn()
            .execute_batch(
                "INSERT INTO question_banks
                   (id, skill_id, label, ratified, delivery_mode,
                    adaptive_se_target, adaptive_min_items, adaptive_max_items)
                 VALUES ('bank_a', 'skill_rust', 'Adaptive', 1, 'adaptive', 0.3, 3, 8);",
            )
            .unwrap();
        for i in 0..12 {
            let diff = (i % 5) + 1;
            db.conn()
                .execute(
                    "INSERT INTO bank_questions (id, bank_id, prompt, options, correct_indices, difficulty)
                     VALUES (?1, 'bank_a', 'q?', '[\"a\",\"b\",\"c\"]', '[0]', ?2)",
                    params![format!("q{i}"), diff as i64],
                )
                .unwrap();
        }
        let (_, _, sql) = crate::db::schema::MIGRATIONS
            .iter()
            .find(|(v, _, _)| *v == 72)
            .unwrap();
        db.conn().execute_batch(sql).unwrap();

        let key = SigningKey::from_bytes(&[9u8; 32]);
        let did = derive_did_key(&key);
        crate::settings::SettingsStore::set(db.conn(), keys::IDENTITY_LOCAL_DID, did.0.clone())
            .unwrap();

        Ctx {
            db,
            runtime: GraderRuntime::new().unwrap(),
            key,
            did,
            _dir: dir,
        }
    }

    /// Start an adaptive attempt directly (mirrors the command body without
    /// Tauri state).
    fn start(ctx: &Ctx) -> AdaptiveStep {
        let seed = 12345u64;
        let attempt_id = format!("att-{seed}");
        ctx.db
            .conn()
            .execute(
                "INSERT INTO assessment_attempts
                   (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders,
                    started_at, attempt_ordinal)
                 VALUES (?1, ?2, 'bank_a', 'skill_rust', ?3, '[]', '[]', ?4, 1)",
                params![attempt_id, ctx.did.0, seed as i64, NOW],
            )
            .unwrap();
        let served = serve_next(&ctx.db, &attempt_id, "bank_a", seed, 0.0)
            .unwrap()
            .unwrap();
        AdaptiveStep {
            attempt_id,
            item: Some(served),
            items_served: 1,
            finished: false,
        }
    }

    /// Answer the current item correctly (option 0 → whatever served position
    /// holds original index 0) or incorrectly, and advance.
    fn answer(ctx: &Ctx, step: &AdaptiveStep, correct: bool) -> AdaptiveStep {
        let item = step.item.as_ref().expect("an item to answer");
        // The served options are shuffled; find where original index 0 landed.
        let order: Vec<usize> = ctx
            .db
            .conn()
            .query_row(
                "SELECT option_order FROM attempt_items WHERE attempt_id = ?1 AND item_id = ?2",
                params![step.attempt_id, item.id],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default();
        let pos_of_correct = order.iter().position(|&orig| orig == 0).unwrap_or(0);
        let selected = if correct {
            vec![pos_of_correct]
        } else {
            vec![(pos_of_correct + 1) % order.len().max(1)]
        };
        submit_adaptive_impl(
            &ctx.db,
            &ctx.engine(),
            &step.attempt_id,
            &item.id,
            &selected,
            NOW,
        )
        .expect("submit succeeds")
    }

    #[test]
    fn a_correct_run_finishes_and_credentials() {
        let ctx = setup();
        let mut step = start(&ctx);
        let mut guard = 0;
        while !step.finished && guard < 20 {
            step = answer(&ctx, &step, true);
            guard += 1;
        }
        assert!(step.finished, "attempt should finish");
        assert!(step.items_served >= 3, "must serve at least min_items");

        let result =
            finalize_adaptive_impl(&ctx.db, &ctx.key, &ctx.did, &step.attempt_id, NOW).unwrap();
        assert!(
            result.passed,
            "an all-correct run should pass, score {}",
            result.score
        );
        assert!(result.credential_id.is_some());
    }

    #[test]
    fn an_all_wrong_run_fails_and_credentials_nothing() {
        let ctx = setup();
        let mut step = start(&ctx);
        let mut guard = 0;
        while !step.finished && guard < 20 {
            step = answer(&ctx, &step, false);
            guard += 1;
        }
        let result =
            finalize_adaptive_impl(&ctx.db, &ctx.key, &ctx.did, &step.attempt_id, NOW).unwrap();
        assert!(
            !result.passed,
            "all-wrong should fail, score {}",
            result.score
        );
        assert!(result.credential_id.is_none());
    }

    #[test]
    fn another_learner_cannot_answer_or_finalize_an_attempt() {
        let ctx = setup();
        let mut step = start(&ctx);
        let other_key = SigningKey::from_bytes(&[88; 32]);
        let other_did = derive_did_key(&other_key);
        SettingsStore::set(ctx.db.conn(), keys::IDENTITY_LOCAL_DID, other_did.0.clone()).unwrap();
        let item = step.item.as_ref().unwrap();
        assert!(submit_adaptive_impl(
            &ctx.db,
            &ctx.engine(),
            &step.attempt_id,
            &item.id,
            &[0],
            NOW
        )
        .unwrap_err()
        .contains("another learner"));
        SettingsStore::set(ctx.db.conn(), keys::IDENTITY_LOCAL_DID, ctx.did.0.clone()).unwrap();
        while !step.finished {
            step = answer(&ctx, &step, true);
        }
        assert!(
            finalize_adaptive_impl(&ctx.db, &other_key, &other_did, &step.attempt_id, NOW)
                .unwrap_err()
                .contains("another learner")
        );
        assert!(
            finalize_adaptive_impl(&ctx.db, &other_key, &ctx.did, &step.attempt_id, NOW)
                .unwrap_err()
                .contains("signing key")
        );
        assert!(
            finalize_adaptive_impl(&ctx.db, &ctx.key, &ctx.did, &step.attempt_id, NOW)
                .unwrap()
                .passed
        );
    }

    #[test]
    fn next_item_failure_rolls_back_the_answer_and_allows_retry() {
        let ctx = setup();
        let step = start(&ctx);
        ctx.db.conn().execute_batch(
            "CREATE TEMP TRIGGER fail_next_item BEFORE INSERT ON attempt_items WHEN NEW.ordinal = 1
             BEGIN SELECT RAISE(ABORT, 'injected next item failure'); END;",
        ).unwrap();
        let item = step.item.as_ref().unwrap();
        assert!(submit_adaptive_impl(
            &ctx.db,
            &ctx.engine(),
            &step.attempt_id,
            &item.id,
            &[0],
            NOW
        )
        .unwrap_err()
        .contains("injected next item failure"));
        let unchanged: bool = ctx
            .db
            .conn()
            .query_row(
                "SELECT COUNT(*) = 1 AND SUM(score IS NOT NULL OR submission_json IS NOT NULL
                OR graded_at IS NOT NULL OR theta_after IS NOT NULL) = 0 FROM attempt_items",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(unchanged);
        ctx.db
            .conn()
            .execute_batch("DROP TRIGGER fail_next_item;")
            .unwrap();
        assert_eq!(answer(&ctx, &step, true).items_served, 2);
    }

    #[test]
    fn unfinished_adaptive_attempt_cannot_issue_a_credential() {
        let ctx = setup();
        let step = answer(&ctx, &start(&ctx), true);
        assert!(!step.finished);
        assert!(
            finalize_adaptive_impl(&ctx.db, &ctx.key, &ctx.did, &step.attempt_id, NOW)
                .unwrap_err()
                .contains("unanswered")
        );
        let count: i64 = ctx
            .db
            .conn()
            .query_row("SELECT COUNT(*) FROM credentials", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn failed_finalization_preserves_answers_and_rolls_back_credential_writes() {
        let ctx = setup();
        let mut step = start(&ctx);
        while !step.finished {
            step = answer(&ctx, &step, true);
        }
        let tables = [
            "attempt_items",
            "credentials",
            "credential_status_lists",
            "credential_anchors",
            "derived_skill_states",
            "derived_skill_state_history",
            "reputation_assertions",
        ];
        let counts = || {
            tables
                .iter()
                .map(|table| {
                    ctx.db
                        .conn()
                        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                            row.get::<_, i64>(0)
                        })
                        .unwrap()
                })
                .collect::<Vec<_>>()
        };
        let baseline = counts();
        for trigger in [
            "BEFORE INSERT ON credentials",
            "BEFORE UPDATE OF graded_at ON assessment_attempts",
            "BEFORE INSERT ON derived_skill_states",
        ] {
            ctx.db.conn().execute_batch(&format!(
                "CREATE TEMP TRIGGER fail_finalization {trigger} BEGIN SELECT RAISE(ABORT, 'injected finalization failure'); END;"
            )).unwrap();
            assert!(
                finalize_adaptive_impl(&ctx.db, &ctx.key, &ctx.did, &step.attempt_id, NOW)
                    .unwrap_err()
                    .contains("injected finalization failure")
            );
            assert_eq!(counts(), baseline);
            let unchanged: bool = ctx.db.conn().query_row(
                "SELECT graded_at IS NULL AND score IS NULL AND passed IS NULL AND credential_id IS NULL
                 FROM assessment_attempts WHERE id = ?1", [&step.attempt_id], |row| row.get(0),
            ).unwrap();
            assert!(unchanged);
            ctx.db
                .conn()
                .execute_batch("DROP TRIGGER fail_finalization;")
                .unwrap();
        }
        let result =
            finalize_adaptive_impl(&ctx.db, &ctx.key, &ctx.did, &step.attempt_id, NOW).unwrap();
        assert!(result.passed);
        assert!(result.credential_id.is_some());
        assert_eq!(counts()[0], baseline[0]);
        assert_eq!(counts()[1], baseline[1] + 1);
    }

    #[test]
    fn submit_never_reveals_correctness() {
        // The AdaptiveStep the learner sees must carry no score or correctness.
        let ctx = setup();
        let step = start(&ctx);
        let next = answer(&ctx, &step, true);
        let json = serde_json::to_string(&next).unwrap();
        assert!(!json.contains("score"), "step leaks a score: {json}");
        assert!(!json.contains("correct"), "step leaks correctness: {json}");
    }

    #[test]
    fn each_answer_records_a_moving_ability_estimate() {
        let ctx = setup();
        let mut step = start(&ctx);
        for _ in 0..4 {
            if step.finished {
                break;
            }
            step = answer(&ctx, &step, true);
        }
        let rows: Vec<(f64, f64)> = ctx
            .db
            .conn()
            .prepare(
                "SELECT theta_after, se_after FROM attempt_items \
                  WHERE attempt_id = ?1 AND score IS NOT NULL ORDER BY ordinal",
            )
            .unwrap()
            .query_map(params![step.attempt_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert!(rows.len() >= 3);
        // Consecutive correct answers should not lower the ability estimate.
        assert!(rows.last().unwrap().0 >= rows.first().unwrap().0);
        // And the standard error should shrink as evidence accrues.
        assert!(rows.last().unwrap().1 < rows.first().unwrap().1);
    }

    #[test]
    fn a_served_item_cannot_be_answered_twice() {
        let ctx = setup();
        let step = start(&ctx);
        let item = step.item.as_ref().unwrap();
        let first = submit_adaptive_impl(
            &ctx.db,
            &ctx.engine(),
            &step.attempt_id,
            &item.id,
            &[0],
            NOW,
        );
        assert!(first.is_ok());
        let again = submit_adaptive_impl(
            &ctx.db,
            &ctx.engine(),
            &step.attempt_id,
            &item.id,
            &[0],
            NOW,
        );
        assert!(again.is_err(), "re-answering a served item must fail");
    }

    #[test]
    fn an_unserved_item_cannot_be_answered() {
        let ctx = setup();
        let step = start(&ctx);
        let err = submit_adaptive_impl(
            &ctx.db,
            &ctx.engine(),
            &step.attempt_id,
            "q_not_served",
            &[0],
            NOW,
        )
        .unwrap_err();
        assert!(err.contains("not served"), "unexpected: {err}");
    }

    #[test]
    fn finalizing_twice_is_refused() {
        let ctx = setup();
        let mut step = start(&ctx);
        let mut guard = 0;
        while !step.finished && guard < 20 {
            step = answer(&ctx, &step, true);
            guard += 1;
        }
        finalize_adaptive_impl(&ctx.db, &ctx.key, &ctx.did, &step.attempt_id, NOW).unwrap();
        let second = finalize_adaptive_impl(&ctx.db, &ctx.key, &ctx.did, &step.attempt_id, NOW);
        assert!(second.is_err(), "an attempt must finalize only once");
    }

    #[test]
    fn the_answer_key_never_enters_a_served_item() {
        let ctx = setup();
        let step = start(&ctx);
        let json = serde_json::to_string(&step.item).unwrap();
        assert!(
            !json.contains("correct_indices"),
            "served item leaks the key"
        );
        assert!(!json.contains("grader_private"));
    }
}
