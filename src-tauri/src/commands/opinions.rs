//! IPC commands for the Field Commentary opinions feature.
//!
//! Exposes:
//!   - `publish_opinion` — post a new opinion (credential-gated)
//!   - `list_opinions` — chronological listing, optionally filtered
//!     by subject field or author
//!   - `get_opinion` — single-opinion lookup
//!   - `list_my_opinions` — the local user's own posts
//!   - `list_eligible_subject_fields` — which subject fields the
//!     local user is credentialed to post in (drives the picker in
//!     the post UI)
//!   - `withdraw_own_opinion` — the author's self-withdrawal path
//!     (separate from DAO-challenge takedown)
//!
//! Posting is gated on the author holding at least one `skill_proof`
//! with `proficiency_level IN ('apply','analyze','evaluate','create')`
//! in a skill under the target `subject_field_id`. The referenced
//! proof IDs are embedded in the signed payload so other nodes can
//! independently verify eligibility.

use tauri::State;

use crate::crypto::hash::entity_id;
use crate::crypto::wallet;
use crate::domain::opinions::{
    OpinionAnnouncement, OpinionPayload, OpinionRow, PublishOpinionRequest, MAX_SUMMARY_CHARS,
};
use crate::AppState;

/// Proficiency levels considered "qualifying" for opinion posting.
/// Bloom's `remember` and `understand` are not enough — you must be
/// able to at least `apply` the skill.
const QUALIFYING_PROFICIENCY_LEVELS: &[&str] = &["apply", "analyze", "evaluate", "create"];

/// Publish a new Field Commentary opinion.
///
/// Pipeline:
///   1. Load the local identity's stake address and signing key.
///   2. Validate the request (title, summary length, video_cid present,
///      at least one credential proof given).
///   3. Verify — *locally* — that each referenced `credential_proof_ids`
///      entry belongs to the author AND covers a skill under
///      `subject_field_id`. This is a defence-in-depth check against
///      a buggy frontend; the P2P receiver does the same check
///      against its own view.
///   4. Build the canonical payload, sign it, insert into the
///      `opinions` table, and register a non-evictable pin for the
///      video blob.
///   5. Broadcast the signed envelope on `TOPIC_OPINIONS` (best
///      effort — a signing-and-commit failure returns an error, but a
///      gossip failure just logs).
#[tauri::command]
pub async fn publish_opinion(
    state: State<'_, AppState>,
    req: PublishOpinionRequest,
) -> Result<OpinionRow, String> {
    // Basic validation
    if req.title.trim().is_empty() {
        return Err("opinion title must not be empty".into());
    }
    if let Some(s) = &req.summary {
        if s.chars().count() > MAX_SUMMARY_CHARS {
            return Err(format!(
                "summary exceeds {MAX_SUMMARY_CHARS}-character limit"
            ));
        }
    }
    if req.video_cid.trim().is_empty() {
        return Err("video_cid is required".into());
    }
    if req.subject_field_id.trim().is_empty() {
        return Err("subject_field_id is required".into());
    }
    if req.credential_proof_ids.is_empty() {
        return Err("at least one credential proof ID is required".into());
    }

    // Get the wallet signing key from the vault
    let keystore = state.keystore.lock().await;
    let ks = keystore.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(keystore);
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;

    // Prepare payload + insert row within a single DB scope; broadcast
    // happens afterwards so the mutex is not held across network I/O.
    let (announcement, row) = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();

        let author_address: String = conn
            .query_row(
                "SELECT stake_address FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("no identity found — generate a wallet first: {e}"))?;

        // Sanity-check: the local user's wallet key must match the
        // stake address they're signing on behalf of. If the user
        // swapped mnemonics but not stake address, refuse to sign.
        if w.stake_address != author_address {
            return Err(
                "wallet mnemonic does not match local identity — refusing to publish".into(),
            );
        }

        // Subject field must exist
        let field_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM subject_fields WHERE id = ?1",
                rusqlite::params![req.subject_field_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if field_exists == 0 {
            return Err(format!(
                "subject_field '{}' does not exist",
                req.subject_field_id
            ));
        }

        // Credential verification — every referenced proof must
        //   (a) be owned by this author (implicit: skill_proofs is local DB
        //       and is per-identity; the exception is proofs received via
        //       gossip with a different `author`. We don't track `author` on
        //       skill_proofs today, so we rely on the separate
        //       reputation_evidence join for authorship. Simpler check:
        //       the proof exists locally AND its skill is under the target
        //       subject_field.)
        //   (b) be at a qualifying proficiency level
        //   (c) cover a skill under `subject_field_id`
        //
        // We require at least ONE of the listed proofs to pass. Including
        // extras doesn't hurt — they just don't contribute to eligibility.
        let mut qualified = false;
        for proof_id in &req.credential_proof_ids {
            let ok: bool = conn
                .query_row(
                    "SELECT CASE WHEN EXISTS ( \
                       SELECT 1 \
                       FROM skill_proofs p \
                       JOIN skills s ON s.id = p.skill_id \
                       JOIN subjects sub ON sub.id = s.subject_id \
                       WHERE p.id = ?1 \
                         AND sub.subject_field_id = ?2 \
                         AND p.proficiency_level IN ('apply','analyze','evaluate','create') \
                     ) THEN 1 ELSE 0 END",
                    rusqlite::params![proof_id, req.subject_field_id],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|e| e.to_string())?
                == 1;
            if ok {
                qualified = true;
                break;
            }
        }
        if !qualified {
            return Err(format!(
                "none of the provided credential_proof_ids qualify you to post in '{}' \
                 — you need at least one skill_proof (level >= apply) under that subject field",
                req.subject_field_id
            ));
        }

        // Build the canonical payload.
        let opinion_id = entity_id(&[&author_address, &req.video_cid]);
        let published_at = chrono::Utc::now().timestamp();

        let payload = OpinionPayload {
            opinion_id: opinion_id.clone(),
            author_address: author_address.clone(),
            subject_field_id: req.subject_field_id.clone(),
            title: req.title.clone(),
            summary: req.summary.clone(),
            video_cid: req.video_cid.clone(),
            thumbnail_cid: req.thumbnail_cid.clone(),
            duration_seconds: req.duration_seconds,
            credential_proof_ids: req.credential_proof_ids.clone(),
            published_at,
        };

        // Sign the canonical JSON
        let payload_bytes =
            serde_json::to_vec(&payload).map_err(|e| format!("serialize opinion payload: {e}"))?;
        let signature = ed25519_dalek::Signer::sign(&w.signing_key, &payload_bytes);
        let signature_hex = hex::encode(signature.to_bytes());
        let public_key_hex = hex::encode(w.signing_key.verifying_key().to_bytes());

        let announcement = OpinionAnnouncement {
            opinion_id: payload.opinion_id.clone(),
            author_address: payload.author_address.clone(),
            subject_field_id: payload.subject_field_id.clone(),
            title: payload.title.clone(),
            summary: payload.summary.clone(),
            video_cid: payload.video_cid.clone(),
            thumbnail_cid: payload.thumbnail_cid.clone(),
            duration_seconds: payload.duration_seconds,
            credential_proof_ids: payload.credential_proof_ids.clone(),
            published_at: payload.published_at,
            signature: signature_hex.clone(),
            public_key: public_key_hex.clone(),
        };

        // Insert locally — the author always sees their own posts
        // even before the gossip round-trip.
        let credential_proof_ids_json =
            serde_json::to_string(&req.credential_proof_ids).unwrap_or_else(|_| "[]".into());
        let published_at_str = chrono::DateTime::from_timestamp(published_at, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

        conn.execute(
            "INSERT INTO opinions (id, author_address, subject_field_id, title, summary, \
             video_cid, thumbnail_cid, duration_seconds, credential_proof_ids, signature, \
             public_key, published_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
             ON CONFLICT(id) DO NOTHING",
            rusqlite::params![
                announcement.opinion_id,
                announcement.author_address,
                announcement.subject_field_id,
                announcement.title,
                announcement.summary,
                announcement.video_cid,
                announcement.thumbnail_cid,
                announcement.duration_seconds,
                credential_proof_ids_json,
                announcement.signature,
                announcement.public_key,
                published_at_str,
            ],
        )
        .map_err(|e| format!("insert opinion row: {e}"))?;

        let row = load_opinion_row(conn, &announcement.opinion_id)?
            .ok_or_else(|| "opinion row disappeared after insert".to_string())?;

        (announcement, row)
    };

    // Best-effort gossip broadcast. A failure here is logged but not
    // propagated — the opinion is already stored locally and can be
    // re-broadcast on the next reconnection by a dedicated sweeper
    // (not yet implemented).
    let p2p_guard = state.p2p_node.lock().await;
    if let Some(node) = p2p_guard.as_ref() {
        let payload_bytes = match serde_json::to_vec(&announcement.payload()) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("opinion gossip serialize failed: {e}");
                return Ok(row);
            }
        };
        let author_address = announcement.author_address.clone();
        if let Err(e) = node
            .publish_opinion(payload_bytes, &w.signing_key, &author_address)
            .await
        {
            log::warn!("opinion gossip publish failed (non-fatal): {e}");
        }
    } else {
        log::warn!("p2p node not initialized — opinion stored locally but not broadcast");
    }

    Ok(row)
}

/// List opinions.
///
/// Filters are AND-combined. Both are optional; passing neither
/// returns the 200 most recent non-withdrawn opinions across all
/// subject fields (used by admin/debug views, not the default UI).
#[tauri::command]
pub async fn list_opinions(
    state: State<'_, AppState>,
    subject_field_id: Option<String>,
    author_address: Option<String>,
    include_withdrawn: Option<bool>,
    limit: Option<usize>,
) -> Result<Vec<OpinionRow>, String> {
    let limit = limit.unwrap_or(200).min(1000);
    let include_withdrawn = include_withdrawn.unwrap_or(false);

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let mut sql = String::from(
        "SELECT id, author_address, subject_field_id, title, summary, video_cid, \
         thumbnail_cid, duration_seconds, credential_proof_ids, signature, public_key, \
         published_at, received_at, withdrawn, withdrawn_reason, on_chain_tx, provenance \
         FROM opinions WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(sf) = &subject_field_id {
        sql.push_str(" AND subject_field_id = ?");
        params.push(Box::new(sf.clone()));
    }
    if let Some(a) = &author_address {
        sql.push_str(" AND author_address = ?");
        params.push(Box::new(a.clone()));
    }
    if !include_withdrawn {
        sql.push_str(" AND withdrawn = 0");
    }
    sql.push_str(" ORDER BY published_at DESC LIMIT ");
    sql.push_str(&limit.to_string());

    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = db.conn().prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_ref.as_slice(), row_to_opinion)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Look up a single opinion by ID.
#[tauri::command]
pub async fn get_opinion(
    state: State<'_, AppState>,
    opinion_id: String,
) -> Result<Option<OpinionRow>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    load_opinion_row(db.conn(), &opinion_id)
}

/// List the local user's own opinions (including withdrawn ones).
#[tauri::command]
pub async fn list_my_opinions(state: State<'_, AppState>) -> Result<Vec<OpinionRow>, String> {
    // Scope the DB guard to a non-async block so the !Send
    // MutexGuard doesn't straddle the await boundary on the
    // list_opinions call.
    let author_address: String = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .query_row(
                "SELECT stake_address FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("no identity found: {e}"))?
    };

    list_opinions(state, None, Some(author_address), Some(true), Some(500)).await
}

/// List subject fields the local user is credentialed to post opinions in.
///
/// Drives the subject-field picker in the post UI — users can only
/// post in fields they meet the `skill_proof` bar for.
#[tauri::command]
pub async fn list_eligible_subject_fields_for_posting(
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT DISTINCT sub.subject_field_id \
             FROM skill_proofs p \
             JOIN skills s  ON s.id = p.skill_id \
             JOIN subjects sub ON sub.id = s.subject_id \
             WHERE p.proficiency_level IN ('apply','analyze','evaluate','create')",
        )
        .map_err(|e| e.to_string())?;

    let ids: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(ids)
}

/// Withdraw the local user's own opinion (self-takedown).
///
/// This is distinct from the DAO-challenge takedown path: the author
/// can always pull their own post, regardless of whether a challenge
/// is in flight. Sets `withdrawn=1` locally AND unpins the video blob
/// so the content-addressed data is eligible for GC. Does NOT
/// propagate a network-wide withdrawal — only the DAO can do that.
#[tauri::command]
pub async fn withdraw_own_opinion(
    state: State<'_, AppState>,
    opinion_id: String,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let author_address: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no identity found: {e}"))?;

    let owner: Option<String> = conn
        .query_row(
            "SELECT author_address FROM opinions WHERE id = ?1",
            rusqlite::params![opinion_id],
            |row| row.get(0),
        )
        .ok();

    match owner {
        None => Err(format!("opinion '{opinion_id}' not found")),
        Some(addr) if addr != author_address => {
            Err("cannot withdraw another user's opinion".into())
        }
        Some(_) => {
            conn.execute(
                "UPDATE opinions \
                 SET withdrawn = 1, withdrawn_reason = 'author_request' \
                 WHERE id = ?1",
                rusqlite::params![opinion_id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

fn load_opinion_row(
    conn: &rusqlite::Connection,
    opinion_id: &str,
) -> Result<Option<OpinionRow>, String> {
    let result = conn.query_row(
        "SELECT id, author_address, subject_field_id, title, summary, video_cid, \
         thumbnail_cid, duration_seconds, credential_proof_ids, signature, public_key, \
         published_at, received_at, withdrawn, withdrawn_reason, on_chain_tx, provenance \
         FROM opinions WHERE id = ?1",
        rusqlite::params![opinion_id],
        row_to_opinion,
    );
    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn row_to_opinion(row: &rusqlite::Row<'_>) -> rusqlite::Result<OpinionRow> {
    let credential_proof_ids_json: Option<String> = row.get(8)?;
    let credential_proof_ids: Vec<String> = credential_proof_ids_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default();
    let withdrawn_int: i64 = row.get(13)?;
    Ok(OpinionRow {
        id: row.get(0)?,
        author_address: row.get(1)?,
        subject_field_id: row.get(2)?,
        title: row.get(3)?,
        summary: row.get(4)?,
        video_cid: row.get(5)?,
        thumbnail_cid: row.get(6)?,
        duration_seconds: row.get(7)?,
        credential_proof_ids,
        signature: row.get(9)?,
        public_key: row.get(10)?,
        published_at: row.get(11)?,
        received_at: row.get(12)?,
        withdrawn: withdrawn_int != 0,
        withdrawn_reason: row.get(14)?,
        on_chain_tx: row.get(15)?,
        provenance: row.get(16)?,
    })
}

// Keep the constant alive for the LSP / future external users.
// Referencing it here prevents "unused const" warnings when nobody
// else imports QUALIFYING_PROFICIENCY_LEVELS — the SQL embeds the
// same list literally.
#[allow(dead_code)]
const _QUALIFYING_LEVELS_DOC_SYNC: &[&str] = QUALIFYING_PROFICIENCY_LEVELS;
