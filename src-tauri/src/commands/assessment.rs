//! Dynamic-assessment commands: start a randomized attempt and grade it
//! host-side. The correct-answer key (`bank_questions.correct_indices`) is
//! loaded only inside grading and is never included in any returned payload.
//!
//! Sentinel is started by the frontend before an attempt (mirroring the course
//! player); its `integrity_session_id` is stored on the attempt and embedded in
//! the issued `AssessmentCredential`, so a consumer can see the attempt was
//! proctored. Passing raises the skill's confidence via aggregation (assessment
//! type weight 0.90 >> self-assertion 0.25).

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::assessment::grader::{grade, Answer, GradedQuestion};
use crate::assessment::randomizer::{draw, QuestionMeta};
use crate::commands::credentials::load_issuer_key;
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
}

/// One submitted answer: the served option POSITIONS the learner selected.
#[derive(Debug, Clone, Deserialize)]
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

    let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();
    let subject_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
    if subject_did.is_empty() {
        return Err("no local identity".into());
    }

    // Pick a ratified bank for the skill.
    let (bank_id, pass_threshold, draw_count): (String, f64, i64) = conn
        .query_row(
            "SELECT id, pass_threshold, draw_count FROM question_banks \
             WHERE skill_id = ?1 AND ratified = 1 ORDER BY created_at LIMIT 1",
            params![skill_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                format!("no assessment available for skill '{skill_id}'")
            }
            other => other.to_string(),
        })?;

    // Load the bank's questions (id, prompt, options).
    let mut stmt = conn
        .prepare("SELECT id, prompt, options, difficulty FROM bank_questions WHERE bank_id = ?1")
        .map_err(|e| e.to_string())?;
    struct Q {
        id: String,
        prompt: String,
        options: Vec<String>,
        difficulty: u8,
    }
    let all: Vec<Q> = stmt
        .query_map(params![bank_id], |r| {
            Ok(Q {
                id: r.get(0)?,
                prompt: r.get(1)?,
                options: parse_json_vec::<String>(&r.get::<_, String>(2)?),
                difficulty: r.get::<_, i64>(3)? as u8,
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
        })
        .collect();
    let drawn = draw(&metas, draw_count.max(1) as usize, seed);

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

    conn.execute(
        "INSERT INTO assessment_attempts \
         (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders, \
          integrity_session_id, started_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            attempt_id,
            subject_did,
            bank_id,
            skill_id,
            seed as i64,
            serde_json::to_string(&drawn.question_ids).unwrap(),
            serde_json::to_string(&drawn.option_orders).unwrap(),
            integrity_session_id,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(StartedAttempt {
        attempt_id,
        skill_id,
        pass_threshold,
        questions: served,
    })
}

// ---- grade attempt ------------------------------------------------------

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

    let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    // Load attempt.
    let (bank_id, skill_id, question_ids_json, option_orders_json, integrity_session_id): (
        String,
        String,
        String,
        String,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT bank_id, skill_id, question_ids, option_orders, integrity_session_id \
             FROM assessment_attempts WHERE id = ?1 AND graded_at IS NULL",
            params![attempt_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
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

    // Build the graded questions (key loaded here, never leaves) in served order.
    let mut graded = Vec::with_capacity(question_ids.len());
    let mut ordered_answers: Vec<Answer> = Vec::with_capacity(question_ids.len());
    let by_answer: std::collections::HashMap<&str, &Vec<usize>> = answers
        .iter()
        .map(|a| (a.question_id.as_str(), &a.selected))
        .collect();

    for (qid, order) in question_ids.iter().zip(option_orders.iter()) {
        let (correct_json, points): (String, f64) = conn
            .query_row(
                "SELECT correct_indices, points FROM bank_questions WHERE id = ?1",
                params![qid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        graded.push(GradedQuestion {
            points,
            correct_indices: parse_json_vec(&correct_json),
            option_order: order.clone(),
        });
        ordered_answers.push(
            by_answer
                .get(qid.as_str())
                .map(|v| (*v).clone())
                .unwrap_or_default(),
        );
    }

    let score = grade(&graded, &ordered_answers);
    let passed = score >= threshold;

    // Issue an AssessmentCredential on pass, bound to the integrity session.
    let mut credential_id = None;
    if passed {
        let claim = SkillClaim {
            skill_id: skill_id.clone(),
            level: crate::aggregation::level::map_level(score),
            score,
            evidence_refs: vec![attempt_id.clone()],
            rubric_version: None,
            assessment_method: Some("proctored_quiz".into()),
            provenance: None, // AssessmentCredential type weight already dominates
        };
        let req = crate::commands::credentials::IssueCredentialRequest {
            credential_type: CredentialType::AssessmentCredential,
            subject: issuer_did.clone(),
            claim: Claim::Skill(claim),
            evidence_refs: vec![attempt_id.clone()],
            expiration_date: None,
            supersedes: None,
            integrity_session_id: integrity_session_id.clone(),
            integrity_policy: None,
        };
        match crate::commands::credentials::issue_credential_impl(
            conn,
            &signing_key,
            &issuer_did,
            &req,
            &now,
        ) {
            Ok(vc) => credential_id = vc.id.clone(),
            Err(e) => log::warn!("assessment: credential issuance failed: {e}"),
        }
    }

    conn.execute(
        "UPDATE assessment_attempts SET score = ?1, passed = ?2, credential_id = ?3, graded_at = ?4 \
         WHERE id = ?5",
        params![score, passed as i64, credential_id, now, attempt_id],
    )
    .map_err(|e| e.to_string())?;

    if passed {
        let _ = crate::commands::aggregation::recompute_all_impl(conn, &now);
    }

    Ok(GradeResult {
        score,
        passed,
        credential_id,
    })
}
