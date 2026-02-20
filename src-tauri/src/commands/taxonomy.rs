//! IPC commands for taxonomy DAO ratification.
//!
//! Exposes the taxonomy ratification workflow to the frontend:
//!   - Propose a taxonomy change via governance
//!   - Preview what a change would affect
//!   - Publish a ratified taxonomy version
//!   - Query taxonomy versions

use tauri::State;

use crate::domain::taxonomy::{
    ProposeTaxonomyParams, TaxonomyPreview, TaxonomyPublishResult, TaxonomyVersion,
};
use crate::evidence::taxonomy;
use crate::AppState;

/// Propose a taxonomy change via a governance proposal.
///
/// Creates a draft proposal with category 'taxonomy_change' under
/// the specified DAO. The changes are stored as JSON and will be
/// applied when the proposal is approved and published.
#[tauri::command]
pub async fn propose_taxonomy_change(
    state: State<'_, AppState>,
    params: ProposeTaxonomyParams,
) -> Result<String, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    // Get proposer address from local identity
    let proposer: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no local identity: {e}"))?;

    // Validate changes first
    let warnings = taxonomy::validate_changes(conn, &params.changes)?;
    if !warnings.is_empty() {
        log::warn!("taxonomy proposal warnings: {:?}", warnings);
    }

    taxonomy::propose_taxonomy_change(
        conn,
        &params.dao_id,
        &params.title,
        params.description.as_deref(),
        &params.changes,
        &proposer,
    )
}

/// Preview what a taxonomy change would affect.
///
/// Returns counts of affected items and lists of new vs modified skill IDs.
#[tauri::command]
pub async fn preview_taxonomy_change(
    state: State<'_, AppState>,
    params: ProposeTaxonomyParams,
) -> Result<TaxonomyPreview, String> {
    let db = state.db.lock().await;
    taxonomy::preview_taxonomy_change(db.conn(), &params.changes)
}

/// Publish a ratified taxonomy version.
///
/// Called after a taxonomy_change proposal is approved by the DAO.
/// Applies changes to local skill tables, records the version,
/// and prepares the taxonomy document for IPFS storage.
#[tauri::command]
pub async fn publish_taxonomy_ratification(
    state: State<'_, AppState>,
    proposal_id: String,
    ratified_by: Vec<String>,
    signature: String,
) -> Result<TaxonomyPublishResult, String> {
    let db = state.db.lock().await;
    taxonomy::publish_taxonomy_ratification(
        db.conn(),
        &proposal_id,
        &ratified_by,
        &signature,
    )
}

/// Get the current (latest) taxonomy version.
#[tauri::command]
pub async fn get_taxonomy_version(
    state: State<'_, AppState>,
) -> Result<Option<TaxonomyVersion>, String> {
    let db = state.db.lock().await;
    taxonomy::get_current_version(db.conn())
}

/// List all taxonomy versions (most recent first).
#[tauri::command]
pub async fn list_taxonomy_versions(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<TaxonomyVersion>, String> {
    let db = state.db.lock().await;
    taxonomy::list_versions(db.conn(), limit.unwrap_or(50))
}

/// Validate a set of taxonomy changes.
///
/// Returns a list of warnings (empty = all valid).
#[tauri::command]
pub async fn validate_taxonomy_changes(
    state: State<'_, AppState>,
    params: ProposeTaxonomyParams,
) -> Result<Vec<String>, String> {
    let db = state.db.lock().await;
    taxonomy::validate_changes(db.conn(), &params.changes)
}
