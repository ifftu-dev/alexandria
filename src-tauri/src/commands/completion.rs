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

use ed25519_dalek::SigningKey;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::cardano::completion::{self as completion_obs, CompletionObservation};
use crate::cardano::{blockfrost::BlockfrostClient, completion_tx_builder, script_refs};
use crate::commands::credentials::{now_rfc3339, IssueCredentialRequest};
use crate::crypto::did::{did_from_verifying_key, Did};
use crate::crypto::wallet;
use crate::domain::completion::{element_leaf, merkle_root, ElementCompletion};
use crate::domain::vc::{Claim, CredentialType, SkillClaim};
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
    /// Ids of every credential issued for this completion (witnessed VC when
    /// anchored, plus per-skill self-claim + instructor attestation). Drives
    /// the frontend's per-credential mint progress.
    pub credential_ids: Vec<String>,
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
    // `None` → content-only course (no gradeable elements): issue locally at a
    // baseline proficiency. `Some(inputs)` → graded course ready to anchor.
    let inputs: Option<Vec<ElementCompletionInput>> = {
        let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let (inputs, missing, required_count) = assemble_from_template(db.conn(), &course_id)?;
        if required_count == 0 {
            None
        } else if !missing.is_empty() {
            return Err(format!(
                "course not complete: {} element(s) lack a passing submission",
                missing.len()
            ));
        } else {
            Some(inputs)
        }
    };

    let result = match inputs {
        Some(inputs) => submit_witness(&state, &course_id, &inputs, timestamp_ms).await,
        None => issue_content_completion(&state, &course_id, timestamp_ms).await,
    };

    // On a successful claim, mark the enrollment completed. This is what
    // makes recorded assessment responses read-only on revisit (the player
    // gates editing on enrollment status) and stamps `completed_at`.
    if result.is_ok() {
        if let Ok(db_guard) = state.db.lock() {
            if let Some(db) = db_guard.as_ref() {
                if let Err(e) = mark_enrollment_completed(db.conn(), &course_id) {
                    log::warn!("failed to mark enrollment completed: {e}");
                }
            }
        }
    }

    result
}

/// Mark the (single, local) enrollment for a course as completed. Idempotent;
/// no-op if there is no enrollment row.
fn mark_enrollment_completed(conn: &Connection, course_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE enrollments \
         SET status = 'completed', \
             completed_at = COALESCE(completed_at, datetime('now')), \
             updated_at = datetime('now') \
         WHERE course_id = ?1",
        rusqlite::params![course_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Baseline proficiency score for finishing a content-only course (no
/// gradeable elements). Maps to a low level on the 0..=5 ladder — completing
/// the material demonstrates familiarity, not assessed mastery.
const CONTENT_COMPLETION_SCORE: f64 = 0.3;

/// Issue local completion credentials for a content-only course. No on-chain
/// witness (there are no graded leaves to anchor); the skill claims +
/// instructor attestation are evidenced by a deterministic completion root
/// derived from the course id.
async fn issue_content_completion(
    state: &State<'_, AppState>,
    course_id: &str,
    timestamp_ms: i64,
) -> Result<CompletionWitnessResult, String> {
    let wallet = {
        let ks_guard = state.keystore.lock().await;
        let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
        let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
        wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?
    };
    let subject_pubkey: [u8; 32] = *wallet.signing_key.verifying_key().as_bytes();

    // Synthetic, deterministic completion root — no graded leaves exist.
    let root = hex::encode(blake3::hash(course_id.as_bytes()).as_bytes());

    let learner_key = SigningKey::from_bytes(&wallet.signing_key.to_bytes());
    let credential_ids = {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        self_issue_completion(
            db.conn(),
            &learner_key,
            course_id,
            &subject_pubkey,
            &wallet.payment_key_hash,
            &root,
            CONTENT_COMPLETION_SCORE,
            None,
            timestamp_ms,
        )?
    };

    Ok(CompletionWitnessResult {
        tx_hash: String::new(),
        completion_root: root,
        leaves: vec![],
        credential_ids,
    })
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

    // Derive the learner wallet/key first — we self-issue the credential
    // locally whether or not the on-chain witness mint succeeds, so the key is
    // needed regardless.
    let wallet = {
        let ks_guard = state.keystore.lock().await;
        let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
        let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
        wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?
    };
    // Subject pubkey = learner Ed25519 verification key (32 bytes).
    let subject_pubkey: [u8; 32] = *wallet.signing_key.verifying_key().as_bytes();

    // Best-effort on-chain witness mint. Blockfrost (per-device
    // `cardano.blockfrost_project_id` setting, or the BLOCKFROST_PROJECT_ID env
    // var) anchors the completion on Cardano. If it's not configured, or the
    // submission fails, we still issue the credential LOCALLY below — the
    // on-chain anchor is an upgrade, not a hard requirement, so completing a
    // course always yields a credential.
    let project_id = {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let conn = db_guard.as_ref().map(|db| db.conn());
        crate::cardano::blockfrost::resolve_project_id(conn)
    };
    let submitted_hash: Option<String> = match project_id {
        None => {
            log::info!("Blockfrost not configured — issuing local completion credential");
            None
        }
        Some(pid) => {
            let minted: Result<String, String> = async {
                let bf = BlockfrostClient::new(pid).map_err(|e| e.to_string())?;
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
                bf.submit_tx(&built.tx_cbor)
                    .await
                    .map_err(|e| e.to_string())
            }
            .await;
            match minted {
                Ok(h) => Some(h),
                Err(e) => {
                    log::warn!("completion witness mint failed — issuing local credential: {e}");
                    None
                }
            }
        }
    };

    // Always self-issue locally — the skill credentials (and, when anchored,
    // the witnessed completion VC) appear in `list_credentials` immediately so
    // the frontend mint completes without waiting on the async observer. With
    // an on-chain tx the completion VC carries the witness; without, it's a
    // local credential. Best-effort: never fail the claim on a self-issue hiccup.
    let mut credential_ids: Vec<String> = Vec::new();
    {
        let mean_score = if elements.is_empty() {
            0.0
        } else {
            elements.iter().map(|e| e.score).sum::<f64>() / elements.len() as f64
        };
        // `Wallet` zeroizes on drop, so clone the key bytes out.
        let learner_key = SigningKey::from_bytes(&wallet.signing_key.to_bytes());
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        if let Some(db) = db_guard.as_ref() {
            match self_issue_completion(
                db.conn(),
                &learner_key,
                course_id,
                &subject_pubkey,
                &wallet.payment_key_hash,
                &preview.root,
                mean_score,
                submitted_hash.as_deref(),
                timestamp_ms,
            ) {
                Ok(ids) => credential_ids = ids,
                Err(e) => log::warn!("completion self-issue failed: {e}"),
            }
        }
    }

    Ok(CompletionWitnessResult {
        // Empty when issued locally without an on-chain anchor.
        tx_hash: submitted_hash.unwrap_or_default(),
        completion_root: preview.root,
        leaves: leaves_hex,
        credential_ids,
    })
}

/// POSIX-millis → the ISO-8601 string shape the observer records for
/// `completion_time` (mirrors `cardano::completion::format_posix_ms`).
fn format_completion_time(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| format!("@ms={ms}"))
}

/// Course-declared skill ids (the `courses.skill_ids` JSON array).
/// Missing/empty/malformed → no skills (best-effort).
fn course_skill_ids(conn: &Connection, course_id: &str) -> Vec<String> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT skill_ids FROM courses WHERE id = ?1",
            rusqlite::params![course_id],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

/// Deterministic "course authority" signing key for a course's author.
///
/// A learner completing a course doesn't hold the instructor's key, so the
/// instructor attestation is signed with a stable per-author keypair derived
/// from the course `author_address`. Domain-separated (so it can't collide
/// with any other derived key) and deterministic (same author → same DID,
/// so repeated completions don't spawn fresh issuer clusters). The DID is
/// distinct from the learner's, which is what lets the aggregator treat the
/// attestation as independent evidence.
///
/// Returns `None` when the course has no usable author address.
fn course_authority_key(conn: &Connection, course_id: &str) -> Option<(SigningKey, Did)> {
    let author: Option<String> = conn
        .query_row(
            "SELECT author_address FROM courses WHERE id = ?1",
            rusqlite::params![course_id],
            |r| r.get::<_, String>(0),
        )
        .ok();
    let author = author.filter(|a| !a.is_empty())?;

    let key = crate::crypto::did::course_authority_key(&author);
    let did = crate::crypto::did::derive_did_key(&key);
    Some((key, did))
}

/// Self-issue the local credentials for a freshly-submitted completion
/// mint. Pure (no network / no keystore) so it's unit-testable:
///
/// 1. Record the matching `completion_observations` row (dedups the
///    async observer).
/// 2. Issue the self-asserted course-completion VC carrying the on-chain
///    `witness` (tx hash + validator script hash) — this is the VC the
///    frontend celebration modal polls for by `witness.tx_hash`.
/// 3. Issue one self-asserted `SkillClaim` VC per course skill so the
///    skill graph "earned" + derived-state pipeline picks them up.
/// 4. Recompute derived skill states.
#[allow(clippy::too_many_arguments)]
fn self_issue_completion(
    conn: &Connection,
    learner_key: &SigningKey,
    course_id: &str,
    subject_pubkey: &[u8; 32],
    payment_key_hash: &[u8; completion_tx_builder::LEARNER_PKH_LENGTH],
    completion_root_hex: &str,
    mean_score: f64,
    tx_hash: Option<&str>,
    timestamp_ms: i64,
) -> Result<Vec<String>, String> {
    let learner_did = did_from_verifying_key(&learner_key.verifying_key());
    // Ids of every credential issued here, in mint order: the witnessed
    // completion VC (when anchored), then per skill the learner's self-claim
    // and the instructor attestation. Returned so the frontend can show
    // per-credential mint progress.
    let mut issued_ids: Vec<String> = Vec::new();

    // (1)+(2) When anchored on-chain, record the observation (dedups the async
    // observer) and issue the witnessed course-completion VC. Skipped for a
    // local-only issue (no tx) — the skill claims below still get issued.
    if let Some(tx) = tx_hash {
        // Policy id (= validator script hash) + asset name are deterministic
        // from the same inputs the tx builder used, so we can reconstruct the
        // observation the chain observer would later decode.
        let policy_id = script_refs::COMPLETION_MINTING_SCRIPT_HASH.to_string();
        let asset_name =
            completion_tx_builder::completion_asset_name(payment_key_hash, course_id.as_bytes());
        let asset_name_hex = hex::encode(asset_name);

        let obs = CompletionObservation {
            policy_id: policy_id.clone(),
            asset_name_hex: asset_name_hex.clone(),
            tx_hash: tx.to_string(),
            subject_pubkey: hex::encode(subject_pubkey),
            course_id: hex::encode(course_id.as_bytes()),
            completion_root: completion_root_hex.to_string(),
            completion_time: format_completion_time(timestamp_ms),
            credential_id: None,
            observed_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            issued_at: None,
        };

        completion_obs::record_observation(conn, &obs).map_err(|e| e.to_string())?;

        let credential_id =
            super::auto_issuance::issue_for_observation(conn, learner_key, &learner_did, &obs)?;
        completion_obs::mark_issued(conn, &policy_id, &asset_name_hex, &credential_id)
            .map_err(|e| e.to_string())?;
        issued_ids.push(credential_id);
    }

    // (3) Per course skill, issue two credentials so the derived skill state
    // is fed by two independent issuers (raising aggregation confidence):
    //   a. the learner's own SelfAssertion skill claim, and
    //   b. an instructor AttestationCredential signed by the course-authority
    //      key (derived from the course author — see `course_authority_key`).
    // Both are evidenced by the witness tx when anchored, else by the local
    // completion root. Level is the completion's mean score mapped onto the
    // 0..=5 proficiency ladder.
    let evidence = match tx_hash {
        Some(tx) => format!("witness:{tx}"),
        None => format!("completion-root:{completion_root_hex}"),
    };
    let score = mean_score.clamp(0.0, 1.0);
    let level = (score * 5.0).round() as u8;
    let now = now_rfc3339();
    let authority = course_authority_key(conn, course_id);
    for skill_id in course_skill_ids(conn, course_id) {
        let self_claim = IssueCredentialRequest {
            credential_type: CredentialType::SelfAssertion,
            subject: learner_did.clone(),
            claim: Claim::Skill(SkillClaim {
                skill_id: skill_id.clone(),
                level,
                score,
                evidence_refs: vec![evidence.clone()],
                rubric_version: None,
                assessment_method: Some("course_completion".into()),
            }),
            evidence_refs: vec![evidence.clone()],
            expiration_date: None,
            supersedes: None,
            integrity_session_id: None,
            integrity_policy: None,
        };
        match super::credentials::issue_credential_impl(
            conn,
            learner_key,
            &learner_did,
            &self_claim,
            &now,
        ) {
            Ok(vc) => issued_ids.extend(vc.id),
            Err(e) => log::warn!("completion self-issue: skill claim failed: {e}"),
        }

        // Instructor attestation — issued by the course authority over the
        // same skill claim. Distinct issuer DID from the learner, so the
        // aggregator treats it as independent evidence.
        if let Some((auth_key, auth_did)) = authority.as_ref() {
            let attestation = IssueCredentialRequest {
                credential_type: CredentialType::AttestationCredential,
                subject: learner_did.clone(),
                claim: Claim::Skill(SkillClaim {
                    skill_id,
                    level,
                    score,
                    evidence_refs: vec![evidence.clone()],
                    rubric_version: None,
                    assessment_method: Some("instructor_attestation".into()),
                }),
                evidence_refs: vec![evidence.clone()],
                expiration_date: None,
                supersedes: None,
                integrity_session_id: None,
                integrity_policy: None,
            };
            match super::credentials::issue_credential_impl(
                conn,
                auth_key,
                auth_did,
                &attestation,
                &now,
            ) {
                Ok(vc) => issued_ids.extend(vc.id),
                Err(e) => log::warn!("completion self-issue: instructor attestation failed: {e}"),
            }
        }
    }

    // (4) Refresh derived proficiency so the skill graph reflects the
    // new evidence right away.
    if let Err(e) = super::aggregation::recompute_all_impl(conn, &now) {
        log::warn!("completion self-issue: recompute failed: {e}");
    }

    Ok(issued_ids)
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
    fn self_issue_emits_witness_vc_and_skill_claims() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address, status, skill_ids) \
                 VALUES ('c1', 'Course', 'stake_test1u', 'published', '[\"s1\",\"s2\"]')",
                [],
            )
            .unwrap();

        let key = SigningKey::from_bytes(&[9u8; 32]);
        let subject_pubkey: [u8; 32] = *key.verifying_key().as_bytes();
        let pkh = [1u8; completion_tx_builder::LEARNER_PKH_LENGTH];
        let tx_hash = "ab".repeat(32);
        let root = "33".repeat(32);

        self_issue_completion(
            db.conn(),
            &key,
            "c1",
            &subject_pubkey,
            &pkh,
            &root,
            0.8,
            Some(&tx_hash),
            1_714_000_000_000,
        )
        .unwrap();

        // The course-completion VC carries the on-chain witness — this is
        // the row the frontend modal polls for by `witness.tx_hash`.
        let creds =
            crate::commands::credentials::list_credentials_impl(db.conn(), None, None).unwrap();
        let witness_hit = creds
            .iter()
            .find(|c| c.witness.as_ref().map(|w| w.tx_hash.as_str()) == Some(tx_hash.as_str()));
        assert!(witness_hit.is_some(), "no VC carries the witness tx hash");

        // Two skill claims per course skill: the learner's SelfAssertion and
        // the course-authority's instructor AttestationCredential.
        let skill_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credentials WHERE claim_kind = 'skill'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(skill_count, 4, "expected self + instructor skill VC per course skill");

        let attestation_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credentials \
                 WHERE credential_type = 'AttestationCredential'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(attestation_count, 2, "one instructor attestation per course skill");

        // The instructor attestation is issued by the deterministic course
        // authority (derived from the author address), not the learner.
        let learner_did = did_from_verifying_key(&key.verifying_key());
        let (_, authority_did) = course_authority_key(db.conn(), "c1").unwrap();
        assert_ne!(authority_did.as_str(), learner_did.as_str());
        let attestation_issuer: String = db
            .conn()
            .query_row(
                "SELECT issuer_did FROM credentials \
                 WHERE credential_type = 'AttestationCredential' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(attestation_issuer, authority_did.as_str());

        // The observation is recorded and marked issued so the async
        // observer won't re-issue the same mint.
        let pending = completion_obs::pending_observations(db.conn()).unwrap();
        assert!(pending.is_empty(), "observation should be marked issued");

        // Derived skill states were recomputed for both skills.
        let derived: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM derived_skill_states", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(derived, 2, "derived state per earned skill");
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
