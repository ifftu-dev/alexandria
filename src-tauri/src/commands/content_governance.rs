//! IPC commands for community-content DAO ratification (goal templates +
//! question banks): propose → (vote via governance) → publish → apply, plus a
//! direct apply for received/ratified version docs (gossip inbound / import).
//! Thin wrappers over [`crate::domain::content_ratification`].

use crate::profile::scope::ProfileState as State;

use crate::crypto::wallet;
use crate::db::{executor::DatabaseWorkload, Database};
use crate::domain::content_ratification::{self as cr, ContentKind, PublishResult, VersionDoc};
use crate::p2p::signing::sign_gossip_message;
use crate::p2p::types::{TOPIC_GOAL_TEMPLATES, TOPIC_QUESTION_BANKS};
use crate::AppState;

async fn content_governance_db<T, F>(
    state: &State<'_, AppState>,
    workload: DatabaseWorkload,
    label: &'static str,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
{
    state
        .db_executor
        .execute(workload, state.profile_lease(), label, operation)
        .await
}

fn topic_for_category(category: &str) -> Option<&'static str> {
    match category {
        "goal_template_change" => Some(TOPIC_GOAL_TEMPLATES),
        "question_bank_change" => Some(TOPIC_QUESTION_BANKS),
        _ => None,
    }
}

fn proposer(conn: &rusqlite::Connection) -> Result<String, String> {
    conn.query_row(
        "SELECT stake_address FROM local_identity WHERE id = 1",
        [],
        |r| r.get(0),
    )
    .map_err(|e| format!("no local identity: {e}"))
}

async fn propose(
    state: &State<'_, AppState>,
    kind: ContentKind,
    dao_id: String,
    title: String,
    description: Option<String>,
    change_json: String,
) -> Result<String, String> {
    content_governance_db(
        state,
        DatabaseWorkload::Instructor,
        "content-governance.propose",
        move |db| {
            let conn = db.conn();
            let who = proposer(conn)?;
            cr::propose(
                conn,
                kind,
                &dao_id,
                &title,
                description.as_deref(),
                &change_json,
                &who,
            )
        },
    )
    .await
}

async fn publish(
    state: &State<'_, AppState>,
    proposal_id: String,
    ratified_by: Vec<String>,
    signature: String,
) -> Result<PublishResult, String> {
    // Apply + record the version locally (scoped so the DB lock drops before
    // the async broadcast below).
    let result = content_governance_db(
        state,
        DatabaseWorkload::Instructor,
        "content-governance.publish",
        move |db| cr::publish(db.conn(), &proposal_id, &ratified_by, &signature),
    )
    .await?;

    // Broadcast the ratified version doc to peers on its topic (best-effort:
    // a publish is durable locally even if the node is offline). Signed with
    // the wallet's gossip key so receivers' registry check passes on the
    // privileged topic.
    if let Some(topic) = topic_for_category(&result.category) {
        if let Ok(w) = wallet_for_broadcast(state).await {
            let signed = sign_gossip_message(
                topic,
                result.doc_json.clone().into_bytes(),
                &w.signing_key,
                &w.stake_address,
            );
            let node = state.p2p_node.lock().await;
            if let Some(ref node) = *node {
                if let Err(e) = node.publish_signed(&signed).await {
                    log::warn!("content ratification: broadcast on {topic} failed: {e}");
                }
            }
        }
    }
    Ok(result)
}

async fn wallet_for_broadcast(state: &State<'_, AppState>) -> Result<wallet::Wallet, String> {
    let keystore = state.keystore.lock().await;
    let ks = keystore.as_ref().ok_or("vault is locked")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(keystore);
    wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn propose_goal_template_change(
    state: State<'_, AppState>,
    dao_id: String,
    title: String,
    description: Option<String>,
    change_json: String,
) -> Result<String, String> {
    propose(
        &state,
        ContentKind::GoalTemplate,
        dao_id,
        title,
        description,
        change_json,
    )
    .await
}

#[tauri::command]
pub async fn publish_goal_template_ratification(
    state: State<'_, AppState>,
    proposal_id: String,
    ratified_by: Vec<String>,
    signature: String,
) -> Result<PublishResult, String> {
    publish(&state, proposal_id, ratified_by, signature).await
}

#[tauri::command]
pub async fn propose_question_bank_change(
    state: State<'_, AppState>,
    dao_id: String,
    title: String,
    description: Option<String>,
    change_json: String,
) -> Result<String, String> {
    propose(
        &state,
        ContentKind::QuestionBank,
        dao_id,
        title,
        description,
        change_json,
    )
    .await
}

#[tauri::command]
pub async fn publish_question_bank_ratification(
    state: State<'_, AppState>,
    proposal_id: String,
    ratified_by: Vec<String>,
    signature: String,
) -> Result<PublishResult, String> {
    publish(&state, proposal_id, ratified_by, signature).await
}

/// Apply a ratified version document (received over gossip, or imported).
/// Idempotent; verifies nothing beyond structural validity — trust comes from
/// the DAO signature the publishing node attached.
#[tauri::command]
pub async fn apply_content_version(
    state: State<'_, AppState>,
    doc: VersionDoc,
) -> Result<usize, String> {
    content_governance_db(
        &state,
        DatabaseWorkload::Background,
        "content-governance.apply-version",
        move |db| cr::apply_version_doc(db.conn(), &doc),
    )
    .await
}
