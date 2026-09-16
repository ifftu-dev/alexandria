//! Dynamic-assessment commands: start a randomized attempt and grade it
//! host-side. The correct-answer key lives in `assessment_items.grader_private`
//! and is loaded only inside grading — it is never part of a returned payload.
//!
//! Grading runs through [`crate::assessment::items`], so an attempt is scored
//! by the same deterministic grader that scores every other item kind, and
//! each item records the `(grader_cid, content_cid, submission_cid)` triple
//! that lets a third party re-derive the score without trusting this device.
//! Those triples are written to `attempt_items` and referenced from the
//! issued credential's `evidence_refs`.
//!
//! Sentinel is started by the frontend before an attempt (mirroring the course
//! player); its `integrity_session_id` is stored on the attempt and embedded in
//! the issued `AssessmentCredential`, so a consumer can see the attempt was
//! proctored. Passing raises the skill's confidence via aggregation (assessment
//! type weight 0.90 >> self-assertion 0.25).

use crate::profile::scope::ProfileState as State;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::assessment::items::{self, GradeEngine};
use crate::assessment::policy::{
    evaluate_attempt_policy, AttemptPolicy, AttemptRecord, PolicyDecision, ScorePolicy,
};
use crate::assessment::randomizer::{draw, QuestionMeta};
use crate::commands::credentials::load_issuer_key;
use crate::db::executor::DatabaseWorkload;
use crate::domain::vc::{Claim, CredentialType, SkillClaim};
use crate::settings::{registry::keys, SettingsStore};
use crate::AppState;

/// A question as served to the client — options already shuffled, NO key.
#[derive(Debug, Clone, Serialize)]
pub struct ServedQuestion {
    pub id: String,
    pub prompt: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartedAttempt {
    pub attempt_id: String,
    pub skill_id: String,
    pub pass_threshold: f64,
    pub questions: Vec<ServedQuestion>,
    /// Locally saved selections when the same live monitoring session retries
    /// the start request.
    pub draft_answers: Vec<SubmittedAnswer>,
}

/// One submitted answer: the served option POSITIONS the learner selected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmittedAnswer {
    pub question_id: String,
    pub selected: Vec<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GradeResult {
    pub score: f64,
    pub passed: bool,
    pub credential_id: Option<String>,
}

fn parse_json_vec<T: serde::de::DeserializeOwned>(s: &str) -> Vec<T> {
    serde_json::from_str(s).unwrap_or_default()
}

/// Prior attempts for one learner and skill, newest first.
///
/// Scoped by skill rather than by bank: a skill may be backed by more than
/// one ratified bank, and switching between them must not reset the
/// cooldown — that would be the same re-roll by another route.
pub(crate) fn load_attempt_history(
    conn: &rusqlite::Connection,
    subject_did: &str,
    skill_id: &str,
) -> Result<Vec<AttemptRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT started_at, graded_at, ended_at, passed FROM assessment_attempts \
              WHERE subject_did = ?1 AND skill_id = ?2 \
              ORDER BY started_at DESC LIMIT 200",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![subject_did, skill_id], |r| {
            Ok(AttemptRecord {
                started_at: r.get(0)?,
                graded_at: r.get(1)?,
                ended_at: r.get(2)?,
                passed: r.get::<_, Option<i64>>(3)?.map(|p| p != 0),
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|x| x.ok())
        .collect();

    Ok(rows)
}

/// Resolve a skill's ratified bank and its attempt policy, if one exists.
///
/// Returns the same bank-selection query `assessment_start_attempt` uses, so
/// the plan and the start path agree on which bank is in play.
pub(crate) fn resolve_bank_policy(
    conn: &rusqlite::Connection,
    skill_id: &str,
) -> Result<Option<(String, AttemptPolicy)>, String> {
    conn.query_row(
        "SELECT id, max_attempts, cooldown_hours, attempt_window_days, score_policy \
           FROM question_banks \
          WHERE skill_id = ?1 AND ratified = 1 ORDER BY created_at LIMIT 1",
        params![skill_id],
        |r| {
            let cooldowns: String = r.get(2)?;
            let window: i64 = r.get(3)?;
            let score_policy: String = r.get(4)?;
            Ok((
                r.get::<_, String>(0)?,
                AttemptPolicy {
                    max_attempts: r.get::<_, Option<i64>>(1)?.map(|m| m.max(0) as u32),
                    cooldown_hours: serde_json::from_str(&cooldowns)
                        .unwrap_or_else(|_| AttemptPolicy::default().cooldown_hours),
                    attempt_window_days: (window > 0).then_some(window as u32),
                    score_policy: ScorePolicy::parse_lenient(&score_policy),
                },
            ))
        },
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other.to_string()),
    })
}

/// Validate the monitoring lease supplied by the assessment UI. An ended or
/// invented session cannot be attached to a credential-bearing attempt.
pub(crate) fn require_live_integrity_session<'a>(
    conn: &rusqlite::Connection,
    integrity_session_id: Option<&'a str>,
) -> Result<&'a str, String> {
    let session_id = integrity_session_id.ok_or("assessment monitoring session is required")?;
    let live: bool = conn
        .query_row(
            "SELECT ended_at IS NULL FROM integrity_sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);
    if !live {
        return Err("assessment monitoring session is missing or already ended".into());
    }
    Ok(session_id)
}

/// Consume open attempts whose monitoring continuity has been lost. Seeing a
/// question under one session and later presenting an answer under another
/// must not be represented as one continuously monitored attempt.
pub(crate) fn end_interrupted_attempts(
    conn: &rusqlite::Connection,
    subject_did: &str,
    skill_id: &str,
    current_integrity_session_id: &str,
    now: &str,
) -> Result<u32, String> {
    conn.execute(
        "UPDATE assessment_attempts \
            SET ended_at = ?1, end_reason = 'interrupted' \
          WHERE subject_did = ?2 AND skill_id = ?3 \
            AND graded_at IS NULL AND ended_at IS NULL \
            AND integrity_session_id IS NOT ?4",
        params![now, subject_did, skill_id, current_integrity_session_id],
    )
    .map(|changed| changed as u32)
    .map_err(|e| e.to_string())
}

/// Build a goal assessment plan: order the goal's skills by prerequisite,
/// then annotate each with whether it can be assessed right now.
///
/// A goal assessment is a sequence of ordinary single-skill attempts, so
/// this is planning and navigation — the learner walks the plan and each
/// assessable skill runs through `assessment_start_attempt` /
/// `assessment_grade` unchanged. See `assessment::goal_plan`.
#[tauri::command]
pub async fn assessment_plan_goal(
    state: State<'_, AppState>,
    goal_skill_ids: Vec<String>,
) -> Result<crate::assessment::goal_plan::GoalAssessmentPlan, String> {
    let now = crate::commands::credentials::now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.plan_goal",
            move |db| {
                let conn = db.conn();
                let subject_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
                plan_goal_impl(conn, &subject_did, &goal_skill_ids, &now)
            },
        )
        .await
}

/// Planning core, separated from Tauri state so the DB glue — bank
/// resolution, earned-skill lookup, per-skill policy evaluation — is
/// testable without a running app.
pub fn plan_goal_impl(
    conn: &rusqlite::Connection,
    subject_did: &str,
    goal_skill_ids: &[String],
    now: &str,
) -> Result<crate::assessment::goal_plan::GoalAssessmentPlan, String> {
    use crate::assessment::goal_plan::{assemble_goal_plan, AssessInfo, PlanStepInput};

    // Skills already proven — same query the learning path uses.
    let earned: std::collections::HashSet<String> = if subject_did.is_empty() {
        std::collections::HashSet::new()
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT skill_id FROM credentials \
                  WHERE subject_did = ?1 AND skill_id IS NOT NULL AND revoked = 0",
            )
            .map_err(|e| e.to_string())?;
        let set = stmt
            .query_map([subject_did], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|x| x.ok())
            .collect();
        set
    };

    // Reuse the learning path: prerequisite-ordered, cycle-guarded.
    let path = crate::commands::graph::compute_path(conn, goal_skill_ids, &earned)?;

    // Per-skill assessability: does a ratified bank exist, and what does the
    // attempt policy say for this learner right now.
    let mut info = std::collections::HashMap::new();
    for step in &path.steps {
        let entry = match resolve_bank_policy(conn, &step.skill_id)? {
            None => AssessInfo {
                has_bank: false,
                decision: None,
            },
            Some((_bank_id, policy)) => {
                let history = load_attempt_history(conn, subject_did, &step.skill_id)?;
                AssessInfo {
                    has_bank: true,
                    decision: Some(evaluate_attempt_policy(&history, &policy, now)),
                }
            }
        };
        info.insert(step.skill_id.clone(), entry);
    }

    let inputs: Vec<PlanStepInput> = path
        .steps
        .iter()
        .map(|s| PlanStepInput {
            skill_id: s.skill_id.clone(),
            name: s.name.clone(),
            bloom_level: crate::domain::bloom::BloomLevel::parse_lenient(&s.bloom_level),
            status: s.status.clone(),
            is_goal: s.is_goal,
        })
        .collect();

    Ok(assemble_goal_plan(&inputs, &info))
}

// ---- start attempt ------------------------------------------------------

/// Begin an attempt for `skill_id`: pick a ratified bank, draw a randomized,
/// difficulty-stratified subset with shuffled options, persist the attempt, and
/// return the served questions (without answers).
#[tauri::command]
pub async fn assessment_start_attempt(
    state: State<'_, AppState>,
    skill_id: String,
    integrity_session_id: Option<String>,
) -> Result<StartedAttempt, String> {
    let seed: u64 = rand::random();
    let attempt_id = crate::commands::credentials::now_rfc3339() + "-" + &seed.to_string();
    let now = crate::commands::credentials::now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.start_attempt",
            move |db| start_attempt_db(db, skill_id, integrity_session_id, seed, attempt_id, now),
        )
        .await
}

fn start_attempt_db(
    db: &crate::db::Database,
    skill_id: String,
    integrity_session_id: Option<String>,
    seed: u64,
    attempt_id: String,
    now: String,
) -> Result<StartedAttempt, String> {
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

    // Pick a ratified bank for the skill.
    let (bank_id, pass_threshold, draw_count, policy) = conn
        .query_row(
            "SELECT id, pass_threshold, draw_count, \
                    max_attempts, cooldown_hours, attempt_window_days, score_policy \
               FROM question_banks \
              WHERE skill_id = ?1 AND ratified = 1 ORDER BY created_at LIMIT 1",
            params![skill_id],
            |r| {
                let cooldowns: String = r.get(4)?;
                let window: i64 = r.get(5)?;
                let score_policy: String = r.get(6)?;
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, i64>(2)?,
                    AttemptPolicy {
                        max_attempts: r.get::<_, Option<i64>>(3)?.map(|m| m.max(0) as u32),
                        cooldown_hours: serde_json::from_str(&cooldowns)
                            .unwrap_or_else(|_| AttemptPolicy::default().cooldown_hours),
                        attempt_window_days: (window > 0).then_some(window as u32),
                        score_policy: ScorePolicy::parse_lenient(&score_policy),
                    },
                ))
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                format!("no assessment available for skill '{skill_id}'")
            }
            other => other.to_string(),
        })?;

    // Rate-limit credential-bearing attempts. Study is untouched; this only
    // governs attempts that can mint a credential, because a score built by
    // re-rolling until a favourable draw measures persistence rather than
    // capability. See `assessment::policy`.
    let history = load_attempt_history(conn, &subject_did, &skill_id)?;
    let decision = evaluate_attempt_policy(&history, &policy, &now);
    let attempt_ordinal = match &decision {
        PolicyDecision::Allow { ordinal } => *ordinal,
        refused => return Err(refused.refusal().unwrap_or_default()),
    };

    // Load the bank's items. Reading `assessment_items` rather than
    // `bank_questions` is what routes this attempt through the unified
    // grader; `content_public` is the half that may reach a client, so the
    // key cannot be selected here even by accident.
    let mut stmt = conn
        .prepare(
            "SELECT id, content_public, difficulty, bloom_level FROM assessment_items \
             WHERE bank_id = ?1 ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    struct Q {
        id: String,
        prompt: String,
        options: Vec<String>,
        difficulty: u8,
        bloom: crate::domain::bloom::BloomLevel,
    }
    let all: Vec<Q> = stmt
        .query_map(params![bank_id], |r| {
            let id: String = r.get(0)?;
            let content: serde_json::Value =
                serde_json::from_str(&r.get::<_, String>(1)?).unwrap_or_default();
            Ok(Q {
                id,
                prompt: content
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                options: content
                    .get("options")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|o| o.as_str().unwrap_or_default().to_string())
                            .collect()
                    })
                    .unwrap_or_default(),
                difficulty: r.get::<_, i64>(2)? as u8,
                // NULL for items authored before the Bloom axis existed;
                // `FromSql` normalises those to the default level.
                bloom: r
                    .get::<_, Option<crate::domain::bloom::BloomLevel>>(3)?
                    .unwrap_or_default(),
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|x| x.ok())
        .collect();
    if all.is_empty() {
        return Err("assessment bank is empty".into());
    }

    let metas: Vec<QuestionMeta> = all
        .iter()
        .map(|q| QuestionMeta {
            id: q.id.clone(),
            difficulty: q.difficulty,
            option_count: q.options.len(),
            bloom: q.bloom,
        })
        .collect();
    // Resume the one open attempt verbatim. Re-drawing here would let a
    // learner refresh until they receive a favourable question set.
    let resumed: Option<(String, String, String, Option<String>)> = conn
        .query_row(
            "SELECT id, question_ids, option_orders, draft_answers_json \
               FROM assessment_attempts \
              WHERE subject_did = ?1 AND skill_id = ?2 \
                AND graded_at IS NULL AND ended_at IS NULL \
              ORDER BY started_at DESC LIMIT 1",
            params![subject_did, skill_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let (attempt_id, drawn, draft_answers, is_resumed) =
        if let Some((existing_id, question_ids_json, option_orders_json, draft_json)) = resumed {
            (
                existing_id,
                crate::assessment::randomizer::Draw {
                    question_ids: parse_json_vec(&question_ids_json),
                    option_orders: parse_json_vec(&option_orders_json),
                },
                draft_json
                    .as_deref()
                    .map(parse_json_vec)
                    .unwrap_or_default(),
                true,
            )
        } else {
            (
                attempt_id,
                draw(&metas, draw_count.max(1) as usize, seed),
                Vec::new(),
                false,
            )
        };

    // Build served questions with options reordered per the shuffle.
    let by_id: std::collections::HashMap<&str, &Q> =
        all.iter().map(|q| (q.id.as_str(), q)).collect();
    let mut served = Vec::with_capacity(drawn.question_ids.len());
    for (qid, order) in drawn.question_ids.iter().zip(drawn.option_orders.iter()) {
        let q = by_id.get(qid.as_str()).ok_or("drawn question vanished")?;
        let options = order
            .iter()
            .filter_map(|&i| q.options.get(i).cloned())
            .collect();
        served.push(ServedQuestion {
            id: qid.clone(),
            prompt: q.prompt.clone(),
            options,
        });
    }

    if !is_resumed {
        conn.execute(
            "INSERT INTO assessment_attempts \
             (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders, \
              integrity_session_id, started_at, attempt_ordinal) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                attempt_id,
                subject_did,
                bank_id,
                skill_id,
                seed as i64,
                serde_json::to_string(&drawn.question_ids).unwrap_or_default(),
                serde_json::to_string(&drawn.option_orders).unwrap_or_default(),
                integrity_session_id,
                now,
                attempt_ordinal as i64,
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(StartedAttempt {
        attempt_id,
        skill_id,
        pass_threshold,
        questions: served,
        draft_answers,
    })
}

/// Save fixed-form selections without grading. This is used before entering
/// diagnostics and for idempotent retries within the same monitoring session.
#[tauri::command]
pub async fn assessment_save_draft(
    state: State<'_, AppState>,
    attempt_id: String,
    answers: Vec<SubmittedAnswer>,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.save_draft",
            move |db| {
                let conn = db.conn();
                let actor_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
                ensure_attempt_owner(conn, &attempt_id, &actor_did)?;
                save_draft_impl(conn, &attempt_id, &answers)
            },
        )
        .await
}

pub(crate) fn save_draft_impl(
    conn: &rusqlite::Connection,
    attempt_id: &str,
    answers: &[SubmittedAnswer],
) -> Result<(), String> {
    let (question_ids_json, option_orders_json): (String, String) = conn
        .query_row(
            "SELECT question_ids, option_orders FROM assessment_attempts \
              WHERE id = ?1 AND graded_at IS NULL AND ended_at IS NULL",
            params![attempt_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "attempt is not open".to_string(),
            other => other.to_string(),
        })?;
    let question_ids: Vec<String> = parse_json_vec(&question_ids_json);
    let option_orders: Vec<Vec<usize>> = parse_json_vec(&option_orders_json);
    let allowed: std::collections::HashMap<&str, usize> = question_ids
        .iter()
        .zip(option_orders.iter())
        .map(|(id, order)| (id.as_str(), order.len()))
        .collect();
    let mut seen = std::collections::HashSet::new();
    for answer in answers {
        let option_count = allowed
            .get(answer.question_id.as_str())
            .ok_or_else(|| format!("question '{}' was not served", answer.question_id))?;
        if !seen.insert(answer.question_id.as_str()) {
            return Err(format!(
                "question '{}' was submitted twice",
                answer.question_id
            ));
        }
        if answer
            .selected
            .iter()
            .any(|position| position >= option_count)
        {
            return Err(format!(
                "question '{}' contains an invalid option position",
                answer.question_id
            ));
        }
    }
    let json = serde_json::to_string(answers).map_err(|e| e.to_string())?;
    let changed = conn
        .execute(
            "UPDATE assessment_attempts SET draft_answers_json = ?1 \
              WHERE id = ?2 AND graded_at IS NULL AND ended_at IS NULL",
            params![json, attempt_id],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err("attempt is not open".into());
    }
    Ok(())
}

// ---- grade attempt ------------------------------------------------------

pub(crate) fn ensure_attempt_owner(
    conn: &rusqlite::Connection,
    attempt_id: &str,
    actor_did: &str,
) -> Result<(), String> {
    if actor_did.is_empty() {
        return Err("no local identity".into());
    }
    let owner: String = conn
        .query_row(
            "SELECT subject_did FROM assessment_attempts WHERE id = ?1",
            params![attempt_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if owner != actor_did {
        return Err("assessment attempt belongs to another learner".into());
    }
    Ok(())
}

/// Grade a submitted attempt host-side. On pass, issue an `AssessmentCredential`
/// bound to the attempt's integrity session and recompute derived skill state.
#[tauri::command]
pub async fn assessment_grade(
    state: State<'_, AppState>,
    attempt_id: String,
    answers: Vec<SubmittedAnswer>,
) -> Result<GradeResult, String> {
    let (signing_key, issuer_did) = load_issuer_key(&state).await?;
    let now = crate::commands::credentials::now_rfc3339();
    #[cfg(desktop)]
    let grader_runtime = state.grader_runtime.clone();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.grade",
            move |db| {
                #[cfg(desktop)]
                let engine = GradeEngine {
                    runtime: &grader_runtime,
                    budgets: Default::default(),
                };
                #[cfg(not(desktop))]
                let engine = GradeEngine::default();
                grade_attempt_impl(
                    db,
                    &engine,
                    &signing_key,
                    &issuer_did,
                    &attempt_id,
                    &answers,
                    &now,
                )
            },
        )
        .await
}

/// Grading core, separated from Tauri state so it is directly testable.
///
/// Everything that decides whether a credential is earned lives here: the
/// answer key is read, each item is graded, per-item reproducibility triples
/// are persisted, and the attempt is closed exactly once.
pub fn grade_attempt_impl(
    db: &crate::db::Database,
    engine: &GradeEngine<'_>,
    signing_key: &ed25519_dalek::SigningKey,
    issuer_did: &crate::crypto::did::Did,
    attempt_id: &str,
    answers: &[SubmittedAnswer],
    now: &str,
) -> Result<GradeResult, String> {
    if crate::crypto::did::derive_did_key(signing_key).as_str() != issuer_did.as_str() {
        return Err("assessment signing key does not match the learner".into());
    }
    let conn = db.conn();
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    ensure_attempt_owner(conn, attempt_id, issuer_did.as_str())?;

    // Load attempt.
    #[allow(clippy::type_complexity)]
    let (
        bank_id,
        skill_id,
        question_ids_json,
        option_orders_json,
        integrity_session_id,
        attempt_ordinal,
    ): (String, String, String, String, Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT bank_id, skill_id, question_ids, option_orders, integrity_session_id, \
                    attempt_ordinal \
               FROM assessment_attempts \
              WHERE id = ?1 AND graded_at IS NULL AND ended_at IS NULL",
            params![attempt_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "attempt not found or already graded".into(),
            other => other.to_string(),
        })?;

    let question_ids: Vec<String> = parse_json_vec(&question_ids_json);
    let option_orders: Vec<Vec<usize>> = parse_json_vec(&option_orders_json);
    let threshold: f64 = conn
        .query_row(
            "SELECT pass_threshold FROM question_banks WHERE id = ?1",
            params![bank_id],
            |r| r.get(0),
        )
        .unwrap_or(0.7);

    // Grade each item through the unified grader. The key is read inside
    // `items::grade_item` and never reaches a payload.
    let by_answer: std::collections::HashMap<&str, &Vec<usize>> = answers
        .iter()
        .map(|a| (a.question_id.as_str(), &a.selected))
        .collect();

    struct Graded {
        item_id: String,
        points: f64,
        grade: items::ItemGrade,
        option_order: Vec<usize>,
        submission: serde_json::Value,
    }

    let mut graded: Vec<Graded> = Vec::with_capacity(question_ids.len());
    for (item_id, order) in question_ids.iter().zip(option_orders.iter()) {
        let item = items::load_item(db, item_id)?
            .ok_or_else(|| format!("assessment item {item_id} no longer exists"))?;

        let selected = by_answer
            .get(item_id.as_str())
            .map(|v| (*v).clone())
            .unwrap_or_default();
        let submission = serde_json::json!({ "selected_positions": selected });

        let grade = items::grade_item(db, engine, &item, &submission, Some(order))?;
        graded.push(Graded {
            item_id: item_id.clone(),
            points: item.points,
            grade,
            option_order: order.clone(),
            submission,
        });
    }

    // Points-weighted mean, matching the previous host grader's aggregation.
    let total_points: f64 = graded.iter().map(|g| g.points).sum();
    let score = if total_points > 0.0 {
        graded.iter().map(|g| g.grade.score * g.points).sum::<f64>() / total_points
    } else {
        0.0
    };
    let passed = score >= threshold;

    // Persist per-item results, including the reproducibility triple for
    // each one — an attempt total alone cannot be re-derived.
    for (ordinal, g) in graded.iter().enumerate() {
        conn.execute(
            "INSERT OR REPLACE INTO attempt_items \
             (attempt_id, ordinal, item_id, option_order, submission_json, \
              grader_cid, content_cid, submission_cid, score, score_details, graded_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                attempt_id,
                ordinal as i64,
                g.item_id,
                serde_json::to_string(&g.option_order).unwrap_or_default(),
                serde_json::to_string(&g.submission).unwrap_or_default(),
                g.grade.grader_cid,
                g.grade.content_cid,
                g.grade.submission_cid,
                g.grade.score,
                serde_json::to_string(&g.grade.details).unwrap_or_default(),
                now,
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    // Issue an AssessmentCredential on pass, bound to the integrity session.
    let mut credential_id = None;
    if passed {
        // Carry each item's reproducibility triple into the credential, so a
        // verifier holding only the VC can re-run the graders and confirm the
        // score without access to this device's database.
        let mut evidence_refs = vec![attempt_id.to_string()];
        for g in &graded {
            evidence_refs.push(format!(
                "grade:{}:{}:{}",
                g.grade.grader_cid, g.grade.content_cid, g.grade.submission_cid
            ));
        }

        // Which try this was. A score says nothing about how many attempts
        // preceded it, and "passed on the seventh" is information an
        // employer is entitled to — it is the difference between capability
        // and persistence.
        if let Some(ordinal) = attempt_ordinal {
            evidence_refs.push(format!("attempt_ordinal:{ordinal}"));
        }

        let claim = SkillClaim {
            skill_id: skill_id.clone(),
            level: crate::aggregation::level::map_level(score),
            score,
            evidence_refs: evidence_refs.clone(),
            rubric_version: None,
            assessment_method: Some("proctored_quiz".into()),
            provenance: None, // AssessmentCredential type weight already dominates
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

#[cfg(test)]
mod interruption_tests {
    use super::*;
    use crate::db::Database;

    fn setup() -> Database {
        let db = Database::open_in_memory().expect("db");
        db.run_migrations().expect("migrations");
        db.conn()
            .execute_batch(
                "INSERT INTO question_banks (id, skill_id, label, ratified)
                 VALUES ('bank', 'skill', 'Skill', 1);
                 INSERT INTO integrity_sessions (id, status) VALUES ('old', 'active');
                 INSERT INTO integrity_sessions (id, status) VALUES ('current', 'active');
                 INSERT INTO assessment_attempts
                   (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders,
                    integrity_session_id, started_at)
                 VALUES ('attempt', 'did:key:zLearner', 'bank', 'skill', 1, '[]', '[]',
                         'old', '2026-09-14T00:00:00Z');",
            )
            .expect("fixture");
        db
    }

    #[test]
    fn a_different_monitoring_session_consumes_the_open_attempt() {
        let db = setup();
        assert_eq!(
            end_interrupted_attempts(
                db.conn(),
                "did:key:zLearner",
                "skill",
                "current",
                "2026-09-14T01:00:00Z",
            )
            .unwrap(),
            1
        );
        let state: (Option<String>, Option<String>, Option<String>, Option<i64>) = db
            .conn()
            .query_row(
                "SELECT ended_at, end_reason, graded_at, passed
                   FROM assessment_attempts WHERE id = 'attempt'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(state.0.as_deref(), Some("2026-09-14T01:00:00Z"));
        assert_eq!(state.1.as_deref(), Some("interrupted"));
        assert!(state.2.is_none());
        assert!(state.3.is_none());
    }

    #[test]
    fn the_same_monitoring_session_can_retry_without_consuming_again() {
        let db = setup();
        assert_eq!(
            end_interrupted_attempts(
                db.conn(),
                "did:key:zLearner",
                "skill",
                "old",
                "2026-09-14T01:00:00Z",
            )
            .unwrap(),
            0
        );
        let ended: Option<String> = db
            .conn()
            .query_row(
                "SELECT ended_at FROM assessment_attempts WHERE id = 'attempt'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(ended.is_none());
    }

    #[test]
    fn monitoring_session_must_exist_and_remain_open() {
        let db = setup();
        assert!(require_live_integrity_session(db.conn(), None)
            .unwrap_err()
            .contains("required"));
        assert!(require_live_integrity_session(db.conn(), Some("missing"))
            .unwrap_err()
            .contains("missing"));
        db.conn()
            .execute(
                "UPDATE integrity_sessions SET ended_at = '2026-09-14T00:30:00Z'
                  WHERE id = 'current'",
                [],
            )
            .unwrap();
        assert!(require_live_integrity_session(db.conn(), Some("current"))
            .unwrap_err()
            .contains("ended"));
        assert_eq!(
            require_live_integrity_session(db.conn(), Some("old")).unwrap(),
            "old"
        );
    }
}

/// Attempt grading, end to end, against a real database and the real
/// built-in grader.
///
/// Desktop-gated because it drives the wasm runtime. The mobile path is
/// covered by `assessment::mcq`'s equivalence tests, which prove the native
/// scorer produces what this one does.
#[cfg(all(test, desktop))]
mod tests {
    use super::*;
    use crate::crypto::did::{derive_did_key, Did};
    use crate::db::Database;
    use crate::plugins::wasm_runtime::GraderRuntime;
    use crate::plugins::{builtins, registry};
    use ed25519_dalek::SigningKey;
    use tempfile::TempDir;

    const NOW: &str = "2026-07-22T00:00:00Z";

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

    /// A bank of two single-answer items: item 0 correct is option 0, item 1
    /// correct is option 1. Threshold defaults to 0.7, so one of two right
    /// (0.5) fails and both right (1.0) passes.
    fn setup() -> Ctx {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let dir = TempDir::new().unwrap();

        let mcq = builtins::BUILTIN_PLUGINS
            .iter()
            .find(|b| b.slug == "mcq")
            .expect("mcq builtin");
        registry::install_builtin(&db, dir.path(), mcq).expect("install mcq");

        db.conn()
            .execute_batch(
                r#"
                INSERT INTO question_banks (id, skill_id, label, ratified, draw_count)
                VALUES ('bank_t', 'skill_rust', 'T', 1, 2);
                INSERT INTO assessment_items
                    (id, item_kind, skill_id, content_public, grader_private, bank_id, ratified)
                VALUES
                  ('q1', 'mcq', 'skill_rust',
                   json_object('kind', 'single', 'prompt', 'one?',
                               'options', json('["a","b","c"]')),
                   json_object('correct_indices', json('[0]')), 'bank_t', 1),
                  ('q2', 'mcq', 'skill_rust',
                   json_object('kind', 'single', 'prompt', 'two?',
                               'options', json('["a","b","c"]')),
                   json_object('correct_indices', json('[1]')), 'bank_t', 1);
                "#,
            )
            .unwrap();

        let key = SigningKey::from_bytes(&[7u8; 32]);
        let did = derive_did_key(&key);
        db.conn()
            .execute(
                "INSERT INTO assessment_attempts \
                 (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders, \
                  started_at, attempt_ordinal) \
                 VALUES ('att_1', ?1, 'bank_t', 'skill_rust', 1, ?2, ?3, ?4, 1)",
                params![
                    did.0,
                    r#"["q1","q2"]"#,
                    // Identity orders: served position == original index.
                    r#"[[0,1,2],[0,1,2]]"#,
                    NOW,
                ],
            )
            .unwrap();

        Ctx {
            db,
            runtime: GraderRuntime::new().unwrap(),
            key,
            did,
            _dir: dir,
        }
    }

    fn answer(q: &str, selected: &[usize]) -> SubmittedAnswer {
        SubmittedAnswer {
            question_id: q.to_string(),
            selected: selected.to_vec(),
        }
    }

    fn grade(ctx: &Ctx, answers: &[SubmittedAnswer]) -> GradeResult {
        grade_attempt_impl(
            &ctx.db,
            &ctx.engine(),
            &ctx.key,
            &ctx.did,
            "att_1",
            answers,
            NOW,
        )
        .expect("grading succeeds")
    }

    #[test]
    fn all_correct_passes_and_issues_a_credential() {
        let ctx = setup();
        let r = grade(&ctx, &[answer("q1", &[0]), answer("q2", &[1])]);
        assert_eq!(r.score, 1.0);
        assert!(r.passed);
        assert!(
            r.credential_id.is_some(),
            "a passing attempt must credential"
        );
    }

    #[test]
    fn another_learner_or_mismatched_signing_key_cannot_grade_the_attempt() {
        let ctx = setup();
        let other_key = SigningKey::from_bytes(&[88; 32]);
        for did in [&ctx.did, &derive_did_key(&other_key)] {
            assert!(grade_attempt_impl(
                &ctx.db,
                &ctx.engine(),
                &other_key,
                did,
                "att_1",
                &[answer("q1", &[0]), answer("q2", &[1])],
                NOW
            )
            .is_err());
        }
        let rows: i64 = ctx
            .db
            .conn()
            .query_row("SELECT COUNT(*) FROM attempt_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0);
        assert!(grade(&ctx, &[answer("q1", &[0]), answer("q2", &[1])]).passed);
    }

    #[test]
    fn failed_grading_rolls_back_items_credential_status_and_derived_state_before_retry() {
        let ctx = setup();
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
            "BEFORE INSERT ON attempt_items WHEN NEW.ordinal = 1",
            "BEFORE INSERT ON credentials",
            "BEFORE UPDATE OF graded_at ON assessment_attempts",
            "BEFORE INSERT ON derived_skill_states",
        ] {
            ctx.db.conn().execute_batch(&format!(
                "CREATE TEMP TRIGGER fail_grade {trigger} BEGIN SELECT RAISE(ABORT, 'injected grade failure'); END;"
            )).unwrap();
            let result = grade_attempt_impl(
                &ctx.db,
                &ctx.engine(),
                &ctx.key,
                &ctx.did,
                "att_1",
                &[answer("q1", &[0]), answer("q2", &[1])],
                NOW,
            );
            assert!(result.unwrap_err().contains("injected grade failure"));
            assert_eq!(counts(), baseline);
            let unchanged: bool = ctx.db.conn().query_row(
                "SELECT graded_at IS NULL AND score IS NULL AND passed IS NULL AND credential_id IS NULL
                 FROM assessment_attempts WHERE id = 'att_1'", [], |row| row.get(0),
            ).unwrap();
            assert!(unchanged);
            ctx.db
                .conn()
                .execute_batch("DROP TRIGGER fail_grade;")
                .unwrap();
        }
        let result = grade(&ctx, &[answer("q1", &[0]), answer("q2", &[1])]);
        assert!(result.passed);
        assert!(result.credential_id.is_some());
        assert_eq!(counts()[0], 2);
        assert_eq!(counts()[1], baseline[1] + 1);
    }

    #[test]
    fn half_correct_fails_and_issues_nothing() {
        let ctx = setup();
        let r = grade(&ctx, &[answer("q1", &[0]), answer("q2", &[0])]);
        assert_eq!(r.score, 0.5);
        assert!(!r.passed);
        assert!(r.credential_id.is_none());
    }

    #[test]
    fn every_item_records_its_reproducibility_triple() {
        // The point of routing through the unified grader: an attempt total
        // is not re-derivable, but each item is.
        let ctx = setup();
        grade(&ctx, &[answer("q1", &[0]), answer("q2", &[1])]);

        let mut stmt = ctx
            .db
            .conn()
            .prepare(
                "SELECT ordinal, item_id, grader_cid, content_cid, submission_cid, score \
                   FROM attempt_items WHERE attempt_id = 'att_1' ORDER BY ordinal",
            )
            .unwrap();
        let rows: Vec<(i64, String, String, String, String, f64)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .unwrap()
            .map(|x| x.unwrap())
            .collect();

        assert_eq!(rows.len(), 2, "one row per served item");
        for (ordinal, item, grader, content, submission, score) in rows {
            assert!(
                ["q1", "q2"].contains(&item.as_str()),
                "unexpected item {item}"
            );
            assert_eq!(grader.len(), 64, "item {ordinal} has no grader cid");
            assert_eq!(content.len(), 64);
            assert_eq!(submission.len(), 64);
            assert_eq!(score, 1.0);
        }
    }

    #[test]
    fn credential_carries_each_grade_triple() {
        // A verifier holding only the VC must be able to re-run the graders,
        // so the triples travel with the credential, not just the local DB.
        let ctx = setup();
        let r = grade(&ctx, &[answer("q1", &[0]), answer("q2", &[1])]);

        let vc_json: String = ctx
            .db
            .conn()
            .query_row(
                "SELECT signed_vc_json FROM credentials WHERE id = ?1",
                params![r.credential_id.unwrap()],
                |row| row.get(0),
            )
            .unwrap();

        let grade_refs = vc_json.matches("grade:").count();
        assert!(
            grade_refs >= 2,
            "credential should reference both item grades, found {grade_refs} in {vc_json}"
        );
        assert!(
            vc_json.contains("att_1"),
            "credential should reference the attempt"
        );
    }

    #[test]
    fn the_answer_key_never_enters_the_credential() {
        let ctx = setup();
        let r = grade(&ctx, &[answer("q1", &[0]), answer("q2", &[1])]);
        let vc_json: String = ctx
            .db
            .conn()
            .query_row(
                "SELECT signed_vc_json FROM credentials WHERE id = ?1",
                params![r.credential_id.unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            !vc_json.contains("correct_indices"),
            "credential leaks the key"
        );
        assert!(
            !vc_json.contains("grader_private"),
            "credential leaks private content"
        );
    }

    #[test]
    fn an_attempt_grades_only_once() {
        // `graded_at IS NULL` is the single-use guard; without it a learner
        // could resubmit until they passed.
        let ctx = setup();
        grade(&ctx, &[answer("q1", &[0]), answer("q2", &[1])]);

        let second = grade_attempt_impl(
            &ctx.db,
            &ctx.engine(),
            &ctx.key,
            &ctx.did,
            "att_1",
            &[answer("q1", &[0]), answer("q2", &[1])],
            NOW,
        );
        assert!(second.is_err(), "a graded attempt must not regrade");
    }

    #[test]
    fn unanswered_items_score_zero_rather_than_failing() {
        let ctx = setup();
        let r = grade(&ctx, &[answer("q1", &[0])]);
        assert_eq!(r.score, 0.5);
        assert!(!r.passed);
    }

    #[test]
    fn attempt_row_is_closed_with_score_and_verdict() {
        let ctx = setup();
        grade(&ctx, &[answer("q1", &[0]), answer("q2", &[1])]);

        let (score, passed, graded_at): (f64, i64, Option<String>) = ctx
            .db
            .conn()
            .query_row(
                "SELECT score, passed, graded_at FROM assessment_attempts WHERE id = 'att_1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(passed, 1);
        assert!(graded_at.is_some());
    }

    #[test]
    fn credential_records_which_attempt_succeeded() {
        // "Passed on attempt 1" and "passed on attempt 9" describe very
        // different learners, and only the credential can carry that.
        let ctx = setup();
        let r = grade(&ctx, &[answer("q1", &[0]), answer("q2", &[1])]);

        let vc_json: String = ctx
            .db
            .conn()
            .query_row(
                "SELECT signed_vc_json FROM credentials WHERE id = ?1",
                params![r.credential_id.unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            vc_json.contains("attempt_ordinal:"),
            "credential should record the attempt ordinal: {vc_json}"
        );
    }

    #[test]
    fn attempt_history_is_scoped_to_one_learner_and_skill() {
        // A cooldown must not leak across learners or skills; either would
        // block someone for another person's activity.
        let ctx = setup();
        ctx.db
            .conn()
            .execute_batch(
                "INSERT INTO assessment_attempts
                   (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders, started_at)
                 VALUES
                   ('other_did', 'did:key:zSomeoneElse', 'bank_t', 'skill_rust', 1, '[]', '[]',
                    '2026-07-22T11:00:00Z'),
                   ('other_skill', 'did:key:zSelf', 'bank_t', 'skill_go', 1, '[]', '[]',
                    '2026-07-22T11:00:00Z');",
            )
            .unwrap();

        let mine = load_attempt_history(ctx.db.conn(), &ctx.did.0, "skill_rust").unwrap();
        assert_eq!(
            mine.len(),
            1,
            "only this learner's attempts at this skill should count"
        );
    }

    #[test]
    fn history_reports_ungraded_attempts_as_open() {
        let ctx = setup();
        let history = load_attempt_history(ctx.db.conn(), &ctx.did.0, "skill_rust").unwrap();
        assert_eq!(history.len(), 1);
        assert!(
            history[0].graded_at.is_none(),
            "the seeded attempt has not been graded yet"
        );
        assert!(history[0].passed.is_none());
    }

    #[test]
    fn fixed_attempt_draft_is_validated_and_saved() {
        let ctx = setup();
        save_draft_impl(
            ctx.db.conn(),
            "att_1",
            &[answer("q1", &[0, 2]), answer("q2", &[])],
        )
        .unwrap();
        let saved: String = ctx
            .db
            .conn()
            .query_row(
                "SELECT draft_answers_json FROM assessment_attempts WHERE id = 'att_1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let round_trip: Vec<SubmittedAnswer> = serde_json::from_str(&saved).unwrap();
        assert_eq!(round_trip.len(), 2);
        assert_eq!(round_trip[0].question_id, "q1");
        assert_eq!(round_trip[0].selected, vec![0, 2]);

        assert!(
            save_draft_impl(ctx.db.conn(), "att_1", &[answer("not-served", &[0])])
                .unwrap_err()
                .contains("not served")
        );
        assert!(
            save_draft_impl(ctx.db.conn(), "att_1", &[answer("q1", &[3])])
                .unwrap_err()
                .contains("invalid option")
        );
    }

    #[test]
    fn diagnostics_ended_attempt_rejects_draft_and_grading() {
        let ctx = setup();
        ctx.db
            .conn()
            .execute(
                "UPDATE assessment_attempts SET ended_at = ?1, end_reason = 'diagnostics' \
                  WHERE id = 'att_1'",
                [NOW],
            )
            .unwrap();

        assert!(save_draft_impl(ctx.db.conn(), "att_1", &[])
            .unwrap_err()
            .contains("not open"));
        assert!(grade_attempt_impl(
            &ctx.db,
            &ctx.engine(),
            &ctx.key,
            &ctx.did,
            "att_1",
            &[],
            NOW,
        )
        .unwrap_err()
        .contains("not found or already graded"));
    }

    #[test]
    fn history_reflects_a_graded_attempt() {
        let ctx = setup();
        grade(&ctx, &[answer("q1", &[0]), answer("q2", &[1])]);

        let history = load_attempt_history(ctx.db.conn(), &ctx.did.0, "skill_rust").unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].graded_at.is_some());
        assert_eq!(history[0].passed, Some(true));
    }

    #[test]
    fn banks_carry_the_default_policy_after_migration() {
        // Existing banks must acquire a sane policy rather than NULLs that
        // would parse into something permissive by accident.
        let ctx = setup();
        let (cooldowns, window, score_policy, max): (String, i64, String, Option<i64>) = ctx
            .db
            .conn()
            .query_row(
                "SELECT cooldown_hours, attempt_window_days, score_policy, max_attempts
                   FROM question_banks WHERE id = 'bank_t'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();

        assert_eq!(cooldowns, "[0,24,72,168]");
        assert_eq!(window, 90);
        assert_eq!(score_policy, "best");
        assert!(
            max.is_none(),
            "no hard cap by default — cooldowns do the work"
        );
    }

    fn hours_before_now(h: i64) -> String {
        (chrono::DateTime::parse_from_rfc3339(NOW).unwrap() - chrono::Duration::hours(h))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }

    fn seed_taxonomy(ctx: &Ctx) {
        // compute_path joins skills -> subjects; a plan needs real skill rows.
        ctx.db
            .conn()
            .execute_batch(
                "INSERT OR IGNORE INTO subject_fields (id, name) VALUES ('sf', 'Field');
                 INSERT OR IGNORE INTO subjects (id, name, subject_field_id)
                 VALUES ('sub', 'Subject', 'sf');
                 INSERT OR IGNORE INTO skills (id, name, subject_id, bloom_level)
                 VALUES ('skill_rust', 'Rust', 'sub', 'apply'),
                        ('skill_async', 'Async', 'sub', 'analyze');
                 INSERT OR IGNORE INTO skill_prerequisites (skill_id, prerequisite_id)
                 VALUES ('skill_async', 'skill_rust');",
            )
            .unwrap();
    }

    #[test]
    fn plan_marks_a_skill_with_a_ratified_bank_assessable() {
        let ctx = setup();
        seed_taxonomy(&ctx);
        // Clear the seeded open attempt so it doesn't drive a cooldown.
        ctx.db
            .conn()
            .execute_batch("DELETE FROM assessment_attempts")
            .unwrap();

        let plan = plan_goal_impl(ctx.db.conn(), &ctx.did.0, &["skill_rust".into()], NOW).unwrap();
        let rust = plan
            .steps
            .iter()
            .find(|s| s.skill_id == "skill_rust")
            .unwrap();
        assert!(rust.has_assessment);
        assert!(rust.assessable_now);
        assert_eq!(plan.next_skill_id.as_deref(), Some("skill_rust"));
    }

    #[test]
    fn plan_pulls_in_prerequisites_and_orders_them_first() {
        let ctx = setup();
        seed_taxonomy(&ctx);
        ctx.db
            .conn()
            .execute_batch("DELETE FROM assessment_attempts")
            .unwrap();

        // Goal is the advanced skill; its prerequisite must appear, earlier.
        let plan = plan_goal_impl(ctx.db.conn(), &ctx.did.0, &["skill_async".into()], NOW).unwrap();
        let ids: Vec<&str> = plan.steps.iter().map(|s| s.skill_id.as_str()).collect();
        assert!(
            ids.contains(&"skill_rust"),
            "prerequisite should be in the plan"
        );
        let rust_pos = ids.iter().position(|s| *s == "skill_rust").unwrap();
        let async_pos = ids.iter().position(|s| *s == "skill_async").unwrap();
        assert!(
            rust_pos < async_pos,
            "prerequisite must precede its dependent"
        );
    }

    #[test]
    fn plan_locks_a_skill_whose_prerequisite_is_unproven() {
        let ctx = setup();
        seed_taxonomy(&ctx);
        ctx.db
            .conn()
            .execute_batch("DELETE FROM assessment_attempts")
            .unwrap();

        let plan = plan_goal_impl(ctx.db.conn(), &ctx.did.0, &["skill_async".into()], NOW).unwrap();
        let adv = plan
            .steps
            .iter()
            .find(|s| s.skill_id == "skill_async")
            .unwrap();
        assert_eq!(adv.status, "locked");
        assert!(!adv.assessable_now);
        assert_eq!(
            adv.blocked_reason.as_deref(),
            Some("prerequisites not yet met")
        );
    }

    #[test]
    fn plan_reflects_a_cooldown_from_recent_attempts() {
        let ctx = setup();
        seed_taxonomy(&ctx);
        // A graded attempt one hour ago triggers the first (24h) cooldown.
        ctx.db
            .conn()
            .execute_batch("DELETE FROM assessment_attempts")
            .unwrap();
        ctx.db
            .conn()
            .execute(
                "INSERT INTO assessment_attempts
                   (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders,
                    started_at, graded_at, passed, attempt_ordinal)
                 VALUES ('recent', ?1, 'bank_t', 'skill_rust', 1, '[]', '[]', ?2, ?2, 0, 1)",
                params![ctx.did.0, hours_before_now(1)],
            )
            .unwrap();

        let plan = plan_goal_impl(ctx.db.conn(), &ctx.did.0, &["skill_rust".into()], NOW).unwrap();
        let rust = plan
            .steps
            .iter()
            .find(|s| s.skill_id == "skill_rust")
            .unwrap();
        assert!(
            !rust.assessable_now,
            "a skill in cooldown is not assessable"
        );
        assert!(rust.cooldown_until.is_some());
        assert_eq!(rust.blocked_reason.as_deref(), Some("in cooldown"));
    }

    #[test]
    fn plan_says_no_assessment_when_the_bank_is_unratified() {
        let ctx = setup();
        seed_taxonomy(&ctx);
        ctx.db
            .conn()
            .execute_batch(
                "DELETE FROM assessment_attempts;
             UPDATE question_banks SET ratified = 0 WHERE id = 'bank_t';",
            )
            .unwrap();

        let plan = plan_goal_impl(ctx.db.conn(), &ctx.did.0, &["skill_rust".into()], NOW).unwrap();
        let rust = plan
            .steps
            .iter()
            .find(|s| s.skill_id == "skill_rust")
            .unwrap();
        assert!(
            !rust.has_assessment,
            "an unratified bank is not an available assessment"
        );
        assert_eq!(
            rust.blocked_reason.as_deref(),
            Some("no assessment available yet")
        );
    }
}
