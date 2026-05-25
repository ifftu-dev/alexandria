//! Frontend-facing IPC for the completion-witness flow.
//!
//! * [`preview_completion_root`] — given an ordered set of element
//!   completions (element id, grader cid, submission hash, score,
//!   grader version), return the leaves + Merkle root the validator
//!   will require. The frontend uses this to confirm what it's about
//!   to submit before pulling the wallet.
//! * [`submit_completion_witness`] — derives the Merkle root, unlocks
//!   the vault, and submits the mint tx via the completion tx
//!   builder. Gated on `ALEXANDRIA_COMPLETION_POLICY_ID` + Blockfrost
//!   availability.
//!
//! These are the bridge between the plugin-reported completion state
//! and the on-chain witness the observer later ingests.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::cardano::{blockfrost::BlockfrostClient, completion_tx_builder};
use crate::crypto::wallet;
use crate::domain::completion::{element_leaf, merkle_root, ElementCompletion};
use crate::AppState;

#[derive(Debug, Clone, Deserialize)]
pub struct ElementCompletionInput {
    pub element_id: String,
    pub grader_cid: String,
    pub submission_hash: String,
    pub grader_version: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionRootPreview {
    /// Hex-encoded 32-byte leaves, one per element.
    pub leaves: Vec<String>,
    /// Hex-encoded 32-byte Merkle root.
    pub root: String,
}

fn compute_preview(inputs: &[ElementCompletionInput]) -> Result<CompletionRootPreview, String> {
    if inputs.is_empty() {
        return Err("course completion requires at least one element".into());
    }
    let leaves: Vec<[u8; 32]> = inputs
        .iter()
        .map(|e| {
            element_leaf(&ElementCompletion {
                element_id: &e.element_id,
                grader_cid: &e.grader_cid,
                submission_hash: &e.submission_hash,
                grader_version: &e.grader_version,
                score: e.score,
            })
        })
        .collect();
    let root = merkle_root(&leaves);
    Ok(CompletionRootPreview {
        leaves: leaves.iter().map(hex::encode).collect(),
        root: hex::encode(root),
    })
}

#[tauri::command]
pub async fn preview_completion_root(
    elements: Vec<ElementCompletionInput>,
) -> Result<CompletionRootPreview, String> {
    compute_preview(&elements)
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionWitnessResult {
    pub tx_hash: String,
    pub completion_root: String,
    pub leaves: Vec<String>,
}

/// Minimum per-element score that counts as "passed" for the purpose
/// of assembling a course-completion witness.
const COMPLETION_PASS_SCORE: f64 = 0.6;

/// Readiness of a course's auto-earn completion claim.
#[derive(Debug, Clone, Serialize)]
pub struct CourseCompletionStatus {
    /// True when every gradeable template element has a passing
    /// submission and a witness can be claimed.
    pub ready: bool,
    /// Element ids still missing a passing submission (in template order).
    pub missing_elements: Vec<String>,
    /// Number of gradeable elements the course template declares.
    pub required_count: usize,
    /// Preview of the witness root, present only when `ready`.
    pub preview: Option<CompletionRootPreview>,
}

/// Assemble the ordered completion inputs for a course from the
/// learner's recorded graded submissions, verified against the course
/// template (the gradeable `course_elements` in chapter/element order).
///
/// Returns the inputs plus the list of elements still missing a passing
/// submission. An empty `missing` with a non-empty `inputs` means the
/// course is ready to claim.
fn assemble_from_template(
    conn: &rusqlite::Connection,
    course_id: &str,
) -> Result<(Vec<ElementCompletionInput>, Vec<String>, usize), String> {
    let enrollment_id: Option<String> = conn
        .query_row(
            "SELECT id FROM enrollments WHERE course_id = ?1 ORDER BY enrolled_at ASC LIMIT 1",
            rusqlite::params![course_id],
            |r| r.get(0),
        )
        .ok();

    // Template = gradeable elements in chapter→element position order.
    let mut stmt = conn
        .prepare(
            "SELECT ce.id FROM course_elements ce \
             JOIN course_chapters cc ON cc.id = ce.chapter_id \
             WHERE cc.course_id = ?1 \
               AND ce.element_type IN ('quiz', 'interactive', 'assessment') \
             ORDER BY cc.position ASC, ce.position ASC",
        )
        .map_err(|e| e.to_string())?;
    let element_ids: Vec<String> = stmt
        .query_map(rusqlite::params![course_id], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let required_count = element_ids.len();
    let mut inputs = Vec::new();
    let mut missing = Vec::new();

    let Some(enrollment_id) = enrollment_id else {
        // Not enrolled → everything is missing.
        return Ok((inputs, element_ids, required_count));
    };

    for element_id in element_ids {
        // Best passing submission for this element on this enrollment.
        let row: Option<(String, String, String, f64)> = conn
            .query_row(
                "SELECT grader_cid, submission_cid, grader_version, score \
                 FROM element_submissions \
                 WHERE enrollment_id = ?1 AND element_id = ?2 \
                 ORDER BY score DESC, created_at DESC LIMIT 1",
                rusqlite::params![enrollment_id, element_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .ok();
        match row {
            Some((grader_cid, submission_cid, grader_version, score))
                if score >= COMPLETION_PASS_SCORE =>
            {
                inputs.push(ElementCompletionInput {
                    element_id,
                    grader_cid,
                    submission_hash: submission_cid,
                    grader_version,
                    score,
                });
            }
            _ => missing.push(element_id),
        }
    }
    Ok((inputs, missing, required_count))
}

/// Read-only completion status for a course — drives the "Claim
/// credential" affordance in the UI.
#[tauri::command]
pub async fn get_course_completion_status(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<CourseCompletionStatus, String> {
    let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let (inputs, missing, required_count) = assemble_from_template(db.conn(), &course_id)?;
    let ready = required_count > 0 && missing.is_empty();
    let preview = if ready {
        Some(compute_preview(&inputs)?)
    } else {
        None
    };
    Ok(CourseCompletionStatus {
        ready,
        missing_elements: missing,
        required_count,
        preview,
    })
}

/// Auto-earn entry point: assemble the witness from the learner's
/// graded submissions (verified against the course template) and submit
/// it on-chain. The frontend calls this directly — no hand-built
/// element list. Fails if the course isn't fully completed.
#[tauri::command]
pub async fn claim_course_completion(
    state: State<'_, AppState>,
    course_id: String,
    timestamp_ms: i64,
) -> Result<CompletionWitnessResult, String> {
    let inputs = {
        let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let (inputs, missing, required_count) = assemble_from_template(db.conn(), &course_id)?;
        if required_count == 0 {
            return Err("course has no gradeable elements to credential".into());
        }
        if !missing.is_empty() {
            return Err(format!(
                "course not complete: {} element(s) lack a passing submission",
                missing.len()
            ));
        }
        inputs
    };
    submit_witness(&state, &course_id, &inputs, timestamp_ms).await
}

#[tauri::command]
pub async fn submit_completion_witness(
    state: State<'_, AppState>,
    course_id: String,
    elements: Vec<ElementCompletionInput>,
    timestamp_ms: i64,
) -> Result<CompletionWitnessResult, String> {
    submit_witness(&state, &course_id, &elements, timestamp_ms).await
}

/// Shared witness builder: compute the root from `elements`, unlock the
/// vault, build + submit the completion mint tx.
async fn submit_witness(
    state: &State<'_, AppState>,
    course_id: &str,
    elements: &[ElementCompletionInput],
    timestamp_ms: i64,
) -> Result<CompletionWitnessResult, String> {
    let preview = compute_preview(elements)?;
    let leaves_hex = preview.leaves.clone();

    // Decode leaves to [u8; 32].
    let leaves: Vec<[u8; 32]> = leaves_hex
        .iter()
        .map(|h| {
            hex::decode(h)
                .ok()
                .and_then(|v| v.try_into().ok())
                .ok_or_else(|| format!("invalid leaf hex: {h}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let root_bytes: [u8; 32] = hex::decode(&preview.root)
        .map_err(|e| format!("invalid root hex: {e}"))?
        .try_into()
        .map_err(|_| "root must decode to 32 bytes".to_string())?;

    // Blockfrost — required for tx submission. Prefers the per-device
    // `cardano.blockfrost_project_id` setting; falls back to the
    // BLOCKFROST_PROJECT_ID env var.
    let project_id = {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let conn = db_guard.as_ref().map(|db| db.conn());
        crate::cardano::blockfrost::resolve_project_id(conn)
    }
    .ok_or_else(|| {
        "Blockfrost project id not configured \
         (set in Settings → Cardano, or export BLOCKFROST_PROJECT_ID) — \
         cannot submit completion witness"
            .to_string()
    })?;
    let bf = BlockfrostClient::new(project_id).map_err(|e| e.to_string())?;

    // Unlock the vault and derive the wallet.
    let wallet = {
        let ks_guard = state.keystore.lock().await;
        let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
        let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
        wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?
    };

    // Subject pubkey = learner Ed25519 verification key (32 bytes).
    let subject_pubkey: [u8; 32] = *wallet.signing_key.verifying_key().as_bytes();

    // Build + sign + submit.
    let built = completion_tx_builder::build_completion_mint_tx(
        &bf,
        &wallet.payment_address,
        &wallet.payment_key_hash,
        &wallet.payment_key_extended,
        &subject_pubkey,
        course_id.as_bytes(),
        &leaves,
        &root_bytes,
        timestamp_ms,
    )
    .await
    .map_err(|e| e.to_string())?;

    let submitted_hash = bf
        .submit_tx(&built.tx_cbor)
        .await
        .map_err(|e| e.to_string())?;

    Ok(CompletionWitnessResult {
        tx_hash: submitted_hash,
        completion_root: preview.root,
        leaves: leaves_hex,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_fails_on_empty_input() {
        assert!(compute_preview(&[]).is_err());
    }

    #[test]
    fn preview_matches_domain_merkle_root_for_single_element() {
        let inp = ElementCompletionInput {
            element_id: "el".into(),
            grader_cid: "cid".into(),
            submission_hash: "hash".into(),
            grader_version: "v".into(),
            score: 0.5,
        };
        let preview = compute_preview(std::slice::from_ref(&inp)).unwrap();
        let leaf = element_leaf(&ElementCompletion {
            element_id: &inp.element_id,
            grader_cid: &inp.grader_cid,
            submission_hash: &inp.submission_hash,
            grader_version: &inp.grader_version,
            score: inp.score,
        });
        assert_eq!(preview.root, hex::encode(leaf));
        assert_eq!(preview.leaves, vec![hex::encode(leaf)]);
    }

    fn seed_course_with_elements(db: &crate::db::Database) {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO courses (id, title, author_address, status) \
             VALUES ('c1', 'Course', 'stake_test1u', 'published')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO course_chapters (id, course_id, title, position) \
             VALUES ('ch1', 'c1', 'Chapter', 0)",
            [],
        )
        .unwrap();
        // Two gradeable elements + one video (ignored by the template).
        conn.execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, position) VALUES \
             ('el_a', 'ch1', 'A', 'quiz', 0), \
             ('el_b', 'ch1', 'B', 'assessment', 1), \
             ('el_v', 'ch1', 'V', 'video', 2)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO enrollments (id, course_id, status) VALUES ('enr1', 'c1', 'active')",
            [],
        )
        .unwrap();
    }

    fn add_submission(db: &crate::db::Database, element_id: &str, score: f64) {
        db.conn()
            .execute(
                "INSERT INTO element_submissions \
                 (id, element_id, enrollment_id, submission_cid, grader_cid, content_cid, \
                  score, learner_did, grader_version) \
                 VALUES (?1, ?2, 'enr1', ?3, 'gcid', 'ccid', ?4, 'did:key:zL', 'v1')",
                rusqlite::params![
                    format!("sub_{element_id}_{score}"),
                    element_id,
                    format!("sub_{element_id}"),
                    score
                ],
            )
            .unwrap();
    }

    #[test]
    fn assemble_reports_missing_until_all_gradeable_pass() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_course_with_elements(&db);

        // Nothing submitted → both gradeable elements missing.
        let (inputs, missing, required) = assemble_from_template(db.conn(), "c1").unwrap();
        assert_eq!(required, 2);
        assert!(inputs.is_empty());
        assert_eq!(missing, vec!["el_a".to_string(), "el_b".to_string()]);

        // Pass el_a, fail el_b → el_b still missing.
        add_submission(&db, "el_a", 0.9);
        add_submission(&db, "el_b", 0.4);
        let (inputs, missing, _) = assemble_from_template(db.conn(), "c1").unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].element_id, "el_a");
        assert_eq!(missing, vec!["el_b".to_string()]);

        // Pass el_b → ready, inputs in template order.
        add_submission(&db, "el_b", 0.75);
        let (inputs, missing, _) = assemble_from_template(db.conn(), "c1").unwrap();
        assert!(missing.is_empty());
        assert_eq!(
            inputs
                .iter()
                .map(|i| i.element_id.as_str())
                .collect::<Vec<_>>(),
            vec!["el_a", "el_b"]
        );
        // The video element never enters the template.
        assert!(inputs.iter().all(|i| i.element_id != "el_v"));
    }

    #[test]
    fn preview_is_deterministic_across_calls() {
        let inputs = vec![
            ElementCompletionInput {
                element_id: "el_1".into(),
                grader_cid: "c1".into(),
                submission_hash: "h1".into(),
                grader_version: "v".into(),
                score: 0.8,
            },
            ElementCompletionInput {
                element_id: "el_2".into(),
                grader_cid: "c2".into(),
                submission_hash: "h2".into(),
                grader_version: "v".into(),
                score: 0.9,
            },
        ];
        let a = compute_preview(&inputs).unwrap();
        let b = compute_preview(&inputs).unwrap();
        assert_eq!(a.root, b.root);
        assert_eq!(a.leaves, b.leaves);
    }
}
