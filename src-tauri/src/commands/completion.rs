//! Frontend-facing IPC for the completion-witness flow.
//!
//! * [`preview_completion_root`] — given an ordered set of element
//!   completions (element id, grader cid, submission hash, score,
//!   grader version), return the leaves + Merkle root the validator
//!   will require. The frontend uses this to confirm what it's about
//!   to submit before pulling the wallet.
//! * [`claim_course_completion`] — derives the Merkle root from persisted,
//!   passing submissions, commits learner self-claims, and queues an optional
//!   mint when Blockfrost is configured. Network work belongs to the profile
//!   worker.
//!
//! These are the bridge between the plugin-reported completion state
//! and the on-chain witness the observer later ingests.

use crate::profile::scope::ProfileState as State;
use ed25519_dalek::SigningKey;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::cardano::completion::{self as completion_obs, CompletionObservation};
use crate::cardano::{
    completion_queue::{self, WitnessState, WitnessStatus},
    completion_recovery, completion_tx_builder, script_refs,
};
use crate::commands::credentials::{now_rfc3339, IssueCredentialRequest};
use crate::crypto::did::{did_from_verifying_key, Did};
use crate::crypto::wallet;
use crate::db::executor::DatabaseWorkload;
use crate::domain::completion::{element_leaf, merkle_root, ElementCompletion};
use crate::domain::vc::{Claim, CredentialType, SkillClaim};
use crate::AppState;
use alexandria_verify::course::{
    CompletionEvidence, CourseCompletionBinding, CourseCompletionPolicy,
    COMPLETION_ENDORSEMENT_FORMAT_VERSION,
};

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
    if inputs
        .iter()
        .any(|input| !input.score.is_finite() || !(0.0..=1.0).contains(&input.score))
    {
        return Err("completion scores must be finite values between zero and one".into());
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
    _profile: crate::profile::scope::ProfileLease,
    elements: Vec<ElementCompletionInput>,
) -> Result<CompletionRootPreview, String> {
    compute_preview(&elements)
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionWitnessResult {
    pub claim_id: String,
    pub witness_status: WitnessStatus,
    pub tx_hash: String,
    pub completion_root: String,
    pub leaves: Vec<String>,
    /// Original local learner self-claim IDs. A subsequently confirmed
    /// witnessed VC is issued separately without rewriting these claims.
    pub credential_ids: Vec<String>,
    /// Canonical instructor-signing request for courses whose exact signed
    /// document requires endorsement. Local self-claims do not wait for it.
    pub endorsement_request: Option<CourseCompletionBinding>,
    /// Policy requirements for which the local completion has no evidence yet.
    /// The self-claim still succeeds; endorsement remains pending.
    pub endorsement_missing_evidence: Vec<alexandria_verify::course::EvidenceRequirement>,
}

#[derive(Debug)]
struct ExactEnrollment {
    id: String,
    course_document_cid: String,
    course_document_version: u32,
    completion_policy: Option<CourseCompletionPolicy>,
}

type ExactEnrollmentRow = (String, Option<String>, Option<i64>, Option<String>);

fn exact_enrollment(conn: &Connection, course_id: &str) -> Result<ExactEnrollment, String> {
    let row: Option<ExactEnrollmentRow> = conn
        .query_row(
            "SELECT e.id, e.course_document_cid, e.course_document_version, e.completion_policy_json \
             FROM enrollments e WHERE e.course_id = ?1 AND e.status IN ('active', 'completed') \
             ORDER BY CASE e.status WHEN 'active' THEN 0 ELSE 1 END, e.enrolled_at DESC, e.id DESC \
             LIMIT 1",
            [course_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((id, Some(cid), Some(version), policy_json)) = row else {
        return Err("enrollment has no exact verified course document binding".into());
    };
    let completion_policy = policy_json
        .as_deref()
        .map(serde_json::from_str::<CourseCompletionPolicy>)
        .transpose()
        .map_err(|error| format!("invalid enrolled completion policy: {error}"))?;
    if let Some(policy) = &completion_policy {
        policy
            .validate()
            .map_err(|error| format!("invalid enrolled completion policy: {error}"))?;
    }
    Ok(ExactEnrollment {
        id,
        course_document_cid: cid,
        course_document_version: u32::try_from(version)
            .map_err(|_| "invalid enrolled course document version".to_string())?,
        completion_policy,
    })
}

fn ensure_enrollment_is_current(
    conn: &Connection,
    course_id: &str,
    enrollment: &ExactEnrollment,
) -> Result<(), String> {
    let current_cid: Option<String> = conn
        .query_row(
            "SELECT content_cid FROM courses WHERE id = ?1",
            [course_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if current_cid.as_deref() != Some(enrollment.course_document_cid.as_str()) {
        return Err(
            "course document changed after enrollment; finish against retained content or start a new enrollment"
                .into(),
        );
    }
    Ok(())
}

/// Minimum per-element score that counts as "passed" for the purpose
/// of assembling a course-completion witness.
const COMPLETION_PASS_SCORE: f64 = 0.6;

/// A gradeable element the learner still needs to pass, with the score
/// context the UI shows when a credential can't yet be earned.
#[derive(Debug, Clone, Serialize)]
pub struct UnmetElement {
    pub element_id: String,
    pub title: String,
    /// `quiz` | `assessment` | `interactive`.
    pub element_type: String,
    /// Best submission score so far (0..1), or `None` if never attempted.
    pub best_score: Option<f64>,
    /// Score needed to pass (0..1).
    pub required_score: f64,
}

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
    /// Score/title detail for each unmet element — drives the "why no
    /// credential yet" explanation in the completion modal.
    pub unmet_elements: Vec<UnmetElement>,
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
    let enrollment = exact_enrollment(conn, course_id)?;
    ensure_enrollment_is_current(conn, course_id, &enrollment)?;
    let enrollment_id = enrollment.id;

    // Template = gradeable elements in chapter→element position order.
    let mut stmt = conn
        .prepare(
            "SELECT ce.id, ce.element_type FROM course_elements ce \
             JOIN course_chapters cc ON cc.id = ce.chapter_id \
             WHERE cc.course_id = ?1 \
               AND ce.element_type IN ('quiz', 'interactive', 'assessment') \
             ORDER BY cc.position ASC, ce.position ASC",
        )
        .map_err(|e| e.to_string())?;
    let elements: Vec<(String, String)> = stmt
        .query_map(rusqlite::params![course_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let required_count = elements.len();
    let mut inputs = Vec::new();
    let mut missing = Vec::new();

    for (element_id, element_type) in elements {
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
            // `interactive` elements are viewers (no gradeable submission by
            // design), so they'd otherwise block the whole course from ever
            // being claimable. Accept them as passed once the learner has
            // marked them complete, with a deterministic synthetic leaf.
            _ if element_type == "interactive"
                && element_progress_completed(conn, &enrollment_id, &element_id) =>
            {
                inputs.push(ElementCompletionInput {
                    element_id: element_id.clone(),
                    grader_cid: "builtin:interactive".to_string(),
                    submission_hash: format!("interactive-complete:{element_id}"),
                    grader_version: "viewer-1".to_string(),
                    score: 1.0,
                });
            }
            _ => missing.push(element_id),
        }
    }
    Ok((inputs, missing, required_count))
}

/// Whether the learner has marked an element complete on this enrollment.
fn element_progress_completed(conn: &Connection, enrollment_id: &str, element_id: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM element_progress \
         WHERE enrollment_id = ?1 AND element_id = ?2 AND status = 'completed' LIMIT 1",
        rusqlite::params![enrollment_id, element_id],
        |_| Ok(()),
    )
    .ok()
    .is_some()
}

/// Read-only completion status for a course — drives the "Claim
/// credential" affordance in the UI.
#[tauri::command]
pub async fn get_course_completion_status(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<CourseCompletionStatus, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "completion.course-status",
            move |db| {
                let (inputs, missing, required_count) =
                    assemble_from_template(db.conn(), &course_id)?;
                let ready = required_count > 0 && missing.is_empty();
                let preview = if ready {
                    Some(compute_preview(&inputs)?)
                } else {
                    None
                };
                let unmet_elements = build_unmet_elements(db.conn(), &course_id, &missing);
                Ok(CourseCompletionStatus {
                    ready,
                    missing_elements: missing,
                    required_count,
                    preview,
                    unmet_elements,
                })
            },
        )
        .await
}

/// Build the per-element score context for each unmet gradeable element:
/// its title and the learner's best submission score (if any) vs the
/// passing bar. Drives the completion modal's "why no credential" panel.
fn build_unmet_elements(
    conn: &Connection,
    course_id: &str,
    missing: &[String],
) -> Vec<UnmetElement> {
    let enrollment_id: Option<String> = conn
        .query_row(
            "SELECT id FROM enrollments WHERE course_id = ?1 ORDER BY enrolled_at ASC LIMIT 1",
            rusqlite::params![course_id],
            |r| r.get(0),
        )
        .ok();

    missing
        .iter()
        .map(|id| {
            let (title, element_type) = conn
                .query_row(
                    "SELECT title, element_type FROM course_elements WHERE id = ?1",
                    rusqlite::params![id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .unwrap_or_else(|_| (id.clone(), "assessment".to_string()));
            let best_score: Option<f64> = enrollment_id.as_ref().and_then(|eid| {
                conn.query_row(
                    "SELECT MAX(score) FROM element_submissions \
                     WHERE enrollment_id = ?1 AND element_id = ?2",
                    rusqlite::params![eid, id],
                    |r| r.get(0),
                )
                .ok()
                .flatten()
            });
            UnmetElement {
                element_id: id.clone(),
                title,
                element_type,
                best_score,
                required_score: COMPLETION_PASS_SCORE,
            }
        })
        .collect()
}

/// Auto-earn entry point: assemble the witness from the learner's
/// graded submissions (verified against the course template), persist local
/// claims and optionally queue a witness. The frontend supplies no hand-built
/// element list. Fails if the course isn't fully completed.
#[tauri::command]
pub async fn claim_course_completion(
    state: State<'_, AppState>,
    course_id: String,
    timestamp_ms: i64,
) -> Result<CompletionWitnessResult, String> {
    // `None` → content-only course (no gradeable elements): issue locally at a
    // baseline proficiency. `Some(inputs)` → graded course ready to anchor.
    let course_id_for_read = course_id.clone();
    let inputs: Option<Vec<ElementCompletionInput>> = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "completion.claim.prepare",
            move |db| {
                let (inputs, missing, required_count) =
                    assemble_from_template(db.conn(), &course_id_for_read)?;
                if required_count == 0 {
                    Ok(None)
                } else if !missing.is_empty() {
                    Err(format!(
                        "course not complete: {} element(s) lack a passing submission",
                        missing.len()
                    ))
                } else {
                    Ok(Some(inputs))
                }
            },
        )
        .await?;

    match inputs {
        Some(inputs) => submit_witness(&state, &course_id, &inputs, timestamp_ms).await,
        None => issue_content_completion(&state, &course_id, timestamp_ms).await,
    }
}

/// Mark the exact enrollment that produced a completion as completed.
fn mark_enrollment_completed(conn: &Connection, enrollment_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE enrollments \
         SET status = 'completed', \
             completed_at = COALESCE(completed_at, datetime('now')), \
             updated_at = datetime('now') \
         WHERE id = ?1",
        [enrollment_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Baseline proficiency score for finishing a content-only course (no
/// gradeable elements). Maps to a low level on the 0..=5 ladder — completing
/// the material demonstrates familiarity, not assessed mastery.
const CONTENT_COMPLETION_SCORE: f64 = 0.3;

/// Issue local completion credentials for a content-only course. No on-chain
/// witness (there are no graded leaves to anchor); learner self-claims
/// are evidenced by a deterministic completion root
/// derived from the exact course document identity.
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

    let course_id = course_id.to_owned();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "completion.content.persist",
            move |db| {
                let enrollment = exact_enrollment(db.conn(), &course_id)?;
                ensure_enrollment_is_current(db.conn(), &course_id, &enrollment)?;
                let root_material = format!(
                    "alexandria/content-completion/v2\0{}\0{}",
                    course_id, enrollment.course_document_cid
                );
                let root = hex::encode(blake3::hash(root_material.as_bytes()).as_bytes());
                persist_completion(
                    db.conn(),
                    &wallet,
                    &course_id,
                    &subject_pubkey,
                    &root,
                    CONTENT_COMPLETION_SCORE,
                    timestamp_ms,
                    vec![],
                    None,
                )
            },
        )
        .await
}

/// Shared witness builder: compute the root from `elements`, unlock the
/// vault, persist local claims and enqueue the optional completion mint tx.
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

    let requested_context = completion_recovery::CompletionContext {
        version: 1,
        policy_id: script_refs::COMPLETION_MINTING_SCRIPT_HASH.to_string(),
        course_id: course_id.to_owned(),
        subject_pubkey,
        payment_key_hash: wallet.payment_key_hash,
        leaves: leaves.clone(),
        root: root_bytes,
        mean_score: elements.iter().map(|e| e.score).sum::<f64>() / elements.len() as f64,
        timestamp_ms,
    };
    requested_context.validate()?;
    let course_id = course_id.to_owned();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "completion.witness.persist",
            move |db| {
                let configured =
                    crate::cardano::blockfrost::resolve_project_id(Some(db.conn())).is_some();
                persist_completion(
                    db.conn(),
                    &wallet,
                    &course_id,
                    &subject_pubkey,
                    &preview.root,
                    requested_context.mean_score,
                    timestamp_ms,
                    leaves_hex,
                    configured.then_some(&requested_context),
                )
            },
        )
        .await
}

fn build_endorsement_request(
    subject: &Did,
    course_id: &str,
    enrollment: &ExactEnrollment,
    root: &str,
    leaves: &[String],
) -> Result<
    (
        Option<CourseCompletionBinding>,
        Vec<alexandria_verify::course::EvidenceRequirement>,
    ),
    String,
> {
    let Some(policy) = &enrollment.completion_policy else {
        return Ok((None, Vec::new()));
    };
    let mut evidence = vec![CompletionEvidence {
        kind: "completion-root".into(),
        format_version: 1,
        id: "course-completion".into(),
        digest: root.to_owned(),
    }];
    evidence.extend(
        leaves
            .iter()
            .enumerate()
            .map(|(index, digest)| CompletionEvidence {
                kind: "completion-leaf".into(),
                format_version: 1,
                id: format!("leaf-{index:04}"),
                digest: digest.clone(),
            }),
    );
    evidence.sort();
    let missing = policy
        .evidence_requirements
        .iter()
        .filter(|requirement| {
            !evidence.iter().any(|candidate| {
                candidate.kind == requirement.kind
                    && candidate.format_version == requirement.format_version
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Ok((None, missing));
    }
    let network_id = crate::network_profile::embedded_preprod()
        .map_err(|error| error.to_string())?
        .network_id
        .clone();
    let binding = CourseCompletionBinding {
        format_version: COMPLETION_ENDORSEMENT_FORMAT_VERSION,
        network_id,
        subject_did: subject.clone(),
        course_id: course_id.to_owned(),
        course_document_cid: enrollment.course_document_cid.clone(),
        course_document_version: enrollment.course_document_version,
        completion_root: root.to_owned(),
        evidence,
        witness_tx_hash: None,
    };
    binding
        .validate(policy)
        .map_err(|error| error.to_string())?;
    Ok((Some(binding), Vec::new()))
}

// Receipt, credentials, enrollment state and optional intent commit together.
// No provider calls are permitted on this local completion path.
#[allow(clippy::too_many_arguments)]
fn persist_completion(
    conn: &Connection,
    wallet: &wallet::Wallet,
    course_id: &str,
    subject_pubkey: &[u8; 32],
    root: &str,
    mean_score: f64,
    timestamp_ms: i64,
    leaves: Vec<String>,
    witness: Option<&completion_recovery::CompletionContext>,
) -> Result<CompletionWitnessResult, String> {
    let subject = did_from_verifying_key(&wallet.signing_key.verifying_key());
    let enrollment = exact_enrollment(conn, course_id)?;
    let identity = serde_json::to_vec(&(
        subject.as_str(),
        course_id,
        enrollment.course_document_cid.as_str(),
        enrollment.course_document_version,
        root,
    ))
    .map_err(|error| error.to_string())?;
    let claim_id = blake3::hash(&identity).to_hex().to_string();
    let claim_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM completion_claims WHERE id = ?1)",
            [&claim_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !claim_exists {
        ensure_enrollment_is_current(conn, course_id, &enrollment)?;
    }
    let (endorsement_request, endorsement_missing_evidence) =
        build_endorsement_request(&subject, course_id, &enrollment, root, &leaves)?;
    let completion_binding_json = endorsement_request
        .as_ref()
        .map(serde_json_canonicalizer::to_string)
        .transpose()
        .map_err(|error| error.to_string())?;
    crate::db::with_transaction(conn, || {
        let existing: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT credential_ids_json, completion_binding_json \
                 FROM completion_claims WHERE id = ?1",
                [&claim_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let (credential_ids, stored_endorsement_request) = if let Some((json, binding_json)) =
            existing
        {
            let stored_binding = binding_json
                .as_deref()
                .map(serde_json::from_str::<CourseCompletionBinding>)
                .transpose()
                .map_err(|error| format!("invalid stored completion binding: {error}"))?;
            if stored_binding != endorsement_request {
                return Err("stored completion binding differs from the exact enrollment".into());
            }
            (
                serde_json::from_str::<Vec<String>>(&json).map_err(|e| e.to_string())?,
                stored_binding,
            )
        } else {
            let ids = self_issue_completion(
                conn,
                &wallet.signing_key,
                course_id,
                &enrollment.course_document_cid,
                enrollment.course_document_version,
                subject_pubkey,
                &wallet.payment_key_hash,
                root,
                mean_score,
                None,
                timestamp_ms,
            )?;
            conn.execute(
                "INSERT INTO completion_claims \
                 (id, subject_did, course_id, course_document_cid, course_document_version, \
                  completion_root, completion_binding_json, credential_ids_json, enrollment_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    claim_id,
                    subject.as_str(),
                    course_id,
                    enrollment.course_document_cid,
                    enrollment.course_document_version,
                    root,
                    completion_binding_json,
                    serde_json::to_string(&ids).map_err(|e| e.to_string())?,
                    enrollment.id,
                ],
            )
            .map_err(|e| e.to_string())?;
            (ids, endorsement_request.clone())
        };
        if let Some(context) = witness {
            completion_queue::enqueue(conn, &claim_id, context)?;
        }
        mark_enrollment_completed(conn, &enrollment.id)?;
        let status = completion_queue::state(conn, &claim_id)?;
        Ok(CompletionWitnessResult {
            claim_id: claim_id.clone(),
            witness_status: status.status,
            tx_hash: status.tx_hash.unwrap_or_default(),
            completion_root: root.to_owned(),
            leaves,
            credential_ids,
            endorsement_request: stored_endorsement_request,
            endorsement_missing_evidence,
        })
    })
}

#[tauri::command]
pub async fn get_completion_witness_status(
    state: State<'_, AppState>,
    claim_id: String,
) -> Result<WitnessState, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "completion.witness.status",
            move |db| completion_queue::state(db.conn(), &claim_id),
        )
        .await
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

/// Self-issue local completion credentials. An optional witness hash must
/// come from a successfully projected ledger receipt, not a submission ack.
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
    course_document_cid: &str,
    course_document_version: u32,
    subject_pubkey: &[u8; 32],
    payment_key_hash: &[u8; completion_tx_builder::LEARNER_PKH_LENGTH],
    completion_root_hex: &str,
    mean_score: f64,
    tx_hash: Option<&str>,
    timestamp_ms: i64,
) -> Result<Vec<String>, String> {
    let learner_did = did_from_verifying_key(&learner_key.verifying_key());
    // Ids of every credential issued here, in mint order: the witnessed
    // completion VC (when anchored), then per skill the learner's self-claim.
    // Instructor endorsement must be signed separately by the instructor.
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
        issued_ids.push(credential_id);
    }

    // (3) Self-claims do not imply an independent instructor endorsement.
    // Evidence is the confirmed witness or the local completion root.
    let mut evidence = vec![
        format!("course-document:blake3:{course_document_cid}:v{course_document_version}"),
        format!("completion-root:{completion_root_hex}"),
    ];
    if let Some(tx) = tx_hash {
        evidence.push(format!("witness:{tx}"));
    }
    let score = mean_score.clamp(0.0, 1.0);
    let level = (score * 5.0).round() as u8;
    let now = now_rfc3339();
    for skill_id in course_skill_ids(conn, course_id) {
        let self_claim = IssueCredentialRequest {
            credential_type: CredentialType::SelfAssertion,
            subject: learner_did.clone(),
            claim: Claim::Skill(SkillClaim {
                skill_id: skill_id.clone(),
                level,
                score,
                evidence_refs: evidence.clone(),
                rubric_version: None,
                assessment_method: Some("course_completion".into()),
                provenance: None,
            }),
            evidence_refs: evidence.clone(),
            expiration_date: None,
            supersedes: None,
            integrity_session_id: None,
            integrity_policy: None,
        };
        let vc = super::credentials::issue_credential_impl(
            conn,
            learner_key,
            &learner_did,
            &self_claim,
            &now,
        )?;
        issued_ids.push(vc.id.ok_or("issued completion credential has no id")?);
    }

    // (4) Refresh derived proficiency so the skill graph reflects the
    // new evidence right away.
    super::aggregation::recompute_all_impl(conn, &now)?;

    Ok(issued_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_receipt_is_atomic_idempotent_and_independent_of_provider() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_course_with_elements(&db);
        db.conn()
            .execute(
                "UPDATE courses SET skill_ids = '[\"one\",\"two\"]' WHERE id = 'c1'",
                [],
            )
            .unwrap();
        let wallet = wallet::wallet_from_mnemonic("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about").unwrap();
        let context = completion_recovery::CompletionContext {
            version: 1,
            policy_id: script_refs::COMPLETION_MINTING_SCRIPT_HASH.into(),
            course_id: "c1".into(),
            subject_pubkey: *wallet.signing_key.verifying_key().as_bytes(),
            payment_key_hash: wallet.payment_key_hash,
            leaves: vec![[9; 32]],
            root: [9; 32],
            mean_score: 0.8,
            timestamp_ms: 1_714_000_000_000,
        };
        let root = hex::encode(context.root);
        let persist = || {
            persist_completion(
                db.conn(),
                &wallet,
                "c1",
                &context.subject_pubkey,
                &root,
                context.mean_score,
                context.timestamp_ms,
                vec![root.clone()],
                Some(&context),
            )
        };
        db.conn().execute_batch("CREATE TRIGGER fail_completion_request BEFORE INSERT ON completion_witness_requests
            BEGIN SELECT RAISE(ABORT, 'injected request failure'); END;").unwrap();
        assert!(persist().unwrap_err().contains("injected request failure"));
        for table in [
            "credentials",
            "completion_claims",
            "completion_witness_requests",
            "credential_status_lists",
            "derived_skill_states",
        ] {
            let count: i64 = db
                .conn()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "partial completion in {table}");
        }
        let status: String = db
            .conn()
            .query_row(
                "SELECT status FROM enrollments WHERE id = 'enr1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "active");
        db.conn()
            .execute_batch("DROP TRIGGER fail_completion_request")
            .unwrap();
        let first = persist().unwrap();
        db.conn()
            .execute(
                "UPDATE courses SET content_cid = ?1 WHERE id = 'c1'",
                ["22".repeat(32)],
            )
            .unwrap();
        let retry = persist().unwrap();
        assert_eq!(first.credential_ids.len(), 2);
        assert_eq!(retry.credential_ids, first.credential_ids);
        assert_eq!(retry.claim_id, first.claim_id);
        assert_eq!(retry.witness_status, WitnessStatus::Pending);
        assert!(retry.tx_hash.is_empty());
        let credentials =
            super::super::credentials::list_credentials_impl(db.conn(), None, None).unwrap();
        assert_eq!(credentials.len(), 2);
        assert!(credentials.iter().all(|vc| vc.witness.is_none()));
        let status: String = db
            .conn()
            .query_row(
                "SELECT status FROM enrollments WHERE id = 'enr1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "completed");
    }

    #[test]
    fn endorsement_request_uses_exact_enrollment_and_reports_missing_evidence() {
        use alexandria_verify::course::{
            AuthorizedAttestor, EvidenceRequirement, COMPLETION_POLICY_FORMAT_VERSION,
        };

        let attestor_key = SigningKey::from_bytes(&[4; 32]);
        let policy = CourseCompletionPolicy {
            format_version: COMPLETION_POLICY_FORMAT_VERSION,
            required_attestors: 1,
            authorized_attestors: vec![AuthorizedAttestor {
                did: did_from_verifying_key(&attestor_key.verifying_key()),
                public_key_hex: hex::encode(attestor_key.verifying_key().as_bytes()),
            }],
            evidence_requirements: vec![EvidenceRequirement {
                kind: "completion-root".into(),
                format_version: 1,
            }],
        };
        let enrollment = ExactEnrollment {
            id: "enrollment".into(),
            course_document_cid: "11".repeat(32),
            course_document_version: 2,
            completion_policy: Some(policy),
        };
        let subject_key = SigningKey::from_bytes(&[9; 32]);
        let subject = did_from_verifying_key(&subject_key.verifying_key());
        let root = "22".repeat(32);

        let (request, missing) =
            build_endorsement_request(&subject, "course", &enrollment, &root, &[]).unwrap();
        assert!(missing.is_empty());
        let request = request.unwrap();
        assert_eq!(request.subject_did, subject);
        assert_eq!(request.course_document_cid, enrollment.course_document_cid);
        assert_eq!(request.completion_root, root);

        let mut missing_enrollment = enrollment;
        missing_enrollment
            .completion_policy
            .as_mut()
            .unwrap()
            .evidence_requirements[0]
            .kind = "oral-review".into();
        let (request, missing) = build_endorsement_request(
            &subject,
            "course",
            &missing_enrollment,
            &"22".repeat(32),
            &[],
        )
        .unwrap();
        assert!(request.is_none());
        assert_eq!(missing[0].kind, "oral-review");
    }

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
            "INSERT INTO courses \
             (id, title, author_address, status, content_cid, course_document_version) \
             VALUES ('c1', 'Course', 'stake_test1u', 'published', ?1, 2)",
            ["11".repeat(32)],
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
            "INSERT INTO enrollments \
             (id, course_id, status, course_document_cid, course_document_version) \
             VALUES ('enr1', 'c1', 'active', ?1, 2)",
            ["11".repeat(32)],
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
            &"11".repeat(32),
            2,
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

        // A course author address does not authorize instructor signatures.
        let skill_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credentials WHERE claim_kind = 'skill'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            skill_count, 2,
            "expected only a learner self-claim per course skill"
        );

        let attestation_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credentials \
                 WHERE credential_type = 'AttestationCredential'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            attestation_count, 0,
            "completion must not manufacture instructor endorsements"
        );

        let foreign_issuers: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credentials WHERE issuer_did != subject_did",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(foreign_issuers, 0);

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

    #[test]
    fn offline_completion_issues_only_learner_self_claims() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address, skill_ids)
            VALUES ('offline', 'Offline course', 'public-author', '[\"skill\"]')",
                [],
            )
            .unwrap();
        let key = SigningKey::from_bytes(&[9; 32]);
        let ids = self_issue_completion(
            db.conn(),
            &key,
            "offline",
            &"11".repeat(32),
            2,
            key.verifying_key().as_bytes(),
            &[1; 28],
            &"33".repeat(32),
            0.8,
            None,
            1_714_000_000_000,
        )
        .unwrap();
        assert_eq!(ids.len(), 1);
        let credentials =
            crate::commands::credentials::list_credentials_impl(db.conn(), None, None).unwrap();
        assert_eq!(credentials.len(), 1);
        assert_eq!(
            credentials[0].issuer,
            did_from_verifying_key(&key.verifying_key())
        );
        assert!(credentials[0].type_.contains(&"SelfAssertion".to_owned()));
        assert!(!credentials[0]
            .type_
            .contains(&"AttestationCredential".to_owned()));
        assert!(credentials[0].witness.is_none());
        assert!(completion_obs::pending_observations(db.conn())
            .unwrap()
            .is_empty());
        let state = super::super::aggregation::list_derived_states_impl(db.conn(), None).unwrap();
        assert_eq!(state.len(), 1);
        assert_eq!(state[0].unique_issuer_clusters, 1);
    }
}
