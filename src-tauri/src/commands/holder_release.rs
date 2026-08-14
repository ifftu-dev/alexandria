// SPDX-License-Identifier: AGPL-3.0-or-later
//! Releasing evidence to contest a flag, and taking it back.
//!
//! # Why this is here
//!
//! Sentinel can decide it cannot explain a session. What follows is an
//! adjudication about a person, made somewhere they cannot see, and the only
//! thing they have to argue with is the capture the flag was raised on — which
//! `sentinel::evidence` keeps on their own device, at their own say-so, and
//! nowhere else.
//!
//! Handing that to the service that raised the flag is the single most
//! consequential thing this application does with a camera frame. The code that
//! decides whether it happens is therefore here, in the repository the person
//! it affects can read, under the licence that keeps it that way — the same
//! rule that puts the disclosure client in `holder_pull` and the service on the
//! other end somewhere else. See `docs/enterprise-boundary.md`.
//!
//! # What a release is bounded by
//!
//! - Only evidence the learner already chose to keep. Nothing is captured or
//!   retained in order to release it; this reads what consent already wrote.
//! - Only against a run that service says is theirs, learned from their own
//!   export rather than typed in or guessed.
//! - Only when they ask, once per asking. There is no retry on failure that
//!   the learner did not initiate, because a release that happens later than
//!   the decision is a release they may have changed their mind about.
//! - All of it or none of it. A session larger than one request is sent in
//!   several, and if any of them fails the part that arrived is withdrawn —
//!   the service refuses partial releases because half a session's evidence
//!   reads as the whole of what somebody chose to send, and batching must not
//!   reintroduce at the session level what it forbids per request.
//!
//! Withdrawal is the mirror image and is *not* bounded that way: it retries
//! until it succeeds. The asymmetry is deliberate. Sending is something a
//! person does; taking back is something they are owed, and being offline at
//! the moment of the decision must not quietly cost them it.

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::State;

use super::holder_pull::{directories, is_loopback, proof, Directory};
use crate::sentinel::evidence;
use crate::AppState;

/// One of this person's assessments at one service, as they can see it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContestableRun {
    /// The learner's own label for the service, so a person with several can
    /// tell which one is talking about them.
    pub directory: String,
    pub directory_url: String,
    /// The organisation that ran it, by name, as that service reports it.
    pub organisation: String,
    /// Their identifier for the run. Needed to address anything at all to it.
    pub run_id: String,
    pub role: String,
    pub status: String,
    /// Whether that service says this session could not be explained.
    pub integrity_flagged: bool,
    /// How many items they currently hold that this person released.
    pub evidence_released: i64,
}

/// What went wrong at one service, named so a person knows which.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Problem {
    pub directory: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunsResult {
    pub items: Vec<ContestableRun>,
    pub problems: Vec<Problem>,
}

fn https_only(url: &str) -> Result<(), String> {
    if url.starts_with("https://") || is_loopback(url) {
        return Ok(());
    }
    Err("a directory must be https".into())
}

/// Send a signed request carrying a body, or none.
///
/// `holder_pull::signed_get` covers reads. This is the same proof over the same
/// challenge for the two methods that change something.
async fn signed_send(
    client: &reqwest::Client,
    base: &str,
    sk: &ed25519_dalek::SigningKey,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    https_only(base)?;
    let timestamp = chrono::Utc::now().timestamp();
    let base = base.trim_end_matches('/');
    let url = format!("{base}{path}");

    let mut req = match method {
        "POST" => client.post(&url),
        "DELETE" => client.delete(&url),
        other => return Err(format!("{other} is not a method this sends")),
    }
    .header("x-alexandria-timestamp", timestamp.to_string())
    .header("x-alexandria-proof", proof(sk, method, path, timestamp))
    // Camera frames are not small and a home connection is not fast. The read
    // timeout in `holder_pull` is fifteen seconds, which is right for a list
    // of names and would abandon an upload most of the way through.
    .timeout(std::time::Duration::from_secs(120));

    if let Some(b) = body {
        req = req.json(&b);
    }

    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let detail = resp.text().await.unwrap_or_default();
        return Err(format!("{code}: {detail}"));
    }
    resp.json().await.map_err(|e| e.to_string())
}

/// Every assessment each configured service says it holds about this person.
///
/// Read from that person's own export, which is the only place a device can
/// learn the identifier a release has to be addressed to. Runs that were not
/// flagged are included: a person is entitled to see what is recorded about
/// them, and a list that showed only the accusations would be a worse answer to
/// "what do they have" than the export already gives.
#[tauri::command]
pub async fn holder_contestable_runs(state: State<'_, AppState>) -> Result<RunsResult, String> {
    let dirs = directories(&state)?;
    let (sk, did) = crate::commands::credentials::load_issuer_key(&state).await?;
    let client = reqwest::Client::new();

    let mut items = Vec::new();
    let mut problems = Vec::new();

    for dir in &dirs {
        match export_from(&client, dir, &sk, did.as_str()).await {
            Ok(runs) => items.extend(runs),
            Err(detail) => problems.push(Problem {
                directory: dir.name.clone(),
                detail,
            }),
        }
    }

    // Noted as they go past. Whatever else this call was for, it is the moment
    // this device learns that somebody has been accused of something.
    note_flags(&state, &items)?;

    Ok(RunsResult { items, problems })
}

/// Record any flag seen here that this device has not recorded before.
///
/// Written from an export that was being read anyway. Nothing is fetched in
/// order to do this, and a person with no directories configured never gets a
/// row — the same rule the rest of this module follows.
fn note_flags(state: &State<'_, AppState>, runs: &[ContestableRun]) -> Result<(), String> {
    let flagged: Vec<&ContestableRun> = runs.iter().filter(|r| r.integrity_flagged).collect();
    if flagged.is_empty() {
        return Ok(());
    }
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    for r in flagged {
        // Left alone if it is already here. The row carries when this device
        // first learned of the flag and whether the learner has been told, and
        // re-reading the export must not reset either.
        db.conn()
            .execute(
                "INSERT INTO integrity_flag_notice \
                     (directory_url, run_id, organisation, role_label) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT (directory_url, run_id) DO NOTHING",
                rusqlite::params![&r.directory_url, &r.run_id, &r.organisation, &r.role],
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A flag this person has not been told about.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagNotice {
    pub directory_url: String,
    pub run_id: String,
    pub organisation: String,
    pub role: String,
}

/// Check every configured service for flags this person has not heard about.
///
/// Called on unlock. Being accused of something is not a thing to find out by
/// happening to open a settings page, and the right to contest a flag is worth
/// what the chance of hearing about it is worth.
///
/// Answers from what it can reach. A service that is down produces no notice
/// rather than an error: this runs in the background behind whatever the person
/// actually opened the application to do, and has no business interrupting it
/// to report that a server was unreachable.
#[tauri::command]
pub async fn holder_unseen_flags(state: State<'_, AppState>) -> Result<Vec<FlagNotice>, String> {
    // Refreshes the record as a side effect, which is the point: this is the
    // call that notices a flag raised since the last time anybody looked.
    let _ = holder_contestable_runs(state.clone()).await?;

    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT directory_url, run_id, organisation, role_label \
             FROM integrity_flag_notice WHERE told_at IS NULL ORDER BY first_seen_at",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(FlagNotice {
                directory_url: r.get(0)?,
                run_id: r.get(1)?,
                organisation: r.get(2)?,
                role: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// Record that the learner has been shown these flags.
///
/// A record of what was displayed, not of agreement. Closing a notice about an
/// accusation is not accepting it, and nothing here should ever be read as
/// though it were — which is why the column is `told_at` and not `accepted_at`.
#[tauri::command]
pub async fn holder_mark_flags_seen(
    state: State<'_, AppState>,
    runs: Vec<FlagNotice>,
) -> Result<(), String> {
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    for r in runs {
        db.conn()
            .execute(
                "UPDATE integrity_flag_notice SET told_at = datetime('now') \
                 WHERE directory_url = ?1 AND run_id = ?2 AND told_at IS NULL",
                rusqlite::params![&r.directory_url, &r.run_id],
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn export_from(
    client: &reqwest::Client,
    dir: &Directory,
    sk: &ed25519_dalek::SigningKey,
    did: &str,
) -> Result<Vec<ContestableRun>, String> {
    https_only(&dir.url)?;
    let path = format!("/api/export/{did}");
    let timestamp = chrono::Utc::now().timestamp();
    let base = dir.url.trim_end_matches('/');

    let resp = client
        .get(format!("{base}{path}"))
        .header("x-alexandria-timestamp", timestamp.to_string())
        .header("x-alexandria-proof", proof(sk, "GET", &path, timestamp))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        return Err(format!("{code}: {}", resp.text().await.unwrap_or_default()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for org in body["organisations"].as_array().unwrap_or(&Vec::new()) {
        let organisation = org["organisation"].as_str().unwrap_or_default().to_string();
        for run in org["runs"].as_array().unwrap_or(&Vec::new()) {
            // A run with no id cannot be addressed, so it is not offered as
            // something to answer. An older service that does not send one
            // still lists what it holds, through the export screen.
            let Some(run_id) = run["id"].as_str() else {
                continue;
            };
            out.push(ContestableRun {
                directory: dir.name.clone(),
                directory_url: dir.url.clone(),
                organisation: organisation.clone(),
                run_id: run_id.to_string(),
                role: run["role"].as_str().unwrap_or_default().to_string(),
                status: run["status"].as_str().unwrap_or_default().to_string(),
                integrity_flagged: run["integrityFlagged"].as_bool().unwrap_or(false),
                evidence_released: run["evidenceReleased"].as_i64().unwrap_or(0),
            });
        }
    }
    Ok(out)
}

/// What a release did, so the screen can say so rather than imply it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Released {
    pub items: usize,
    pub bytes: usize,
}

/// How many items the service accepts in one release.
///
/// Its limit, mirrored here so a person is not handed a refusal they cannot
/// act on. A session that retained more than this is sent in several requests
/// rather than rejected wholesale — the alternative is telling somebody
/// contesting an accusation that they kept too much evidence.
const BATCH_ITEMS: usize = 50;

/// How many raw bytes go in one request.
///
/// The service caps a request body at twelve megabytes and payloads travel as
/// base64, which is four bytes on the wire for every three of image. Eight
/// megabytes of frames is about eleven of body, and the rest is JSON around it.
const BATCH_BYTES: usize = 8 * 1024 * 1024;

/// The most any one assessment can hold at the service.
const RUN_CAPACITY: usize = 200;

/// Split into requests the service will accept, preserving order.
///
/// An item too large to fit a batch on its own still gets its own batch: it
/// will be refused by the service, with the service's own message about why,
/// which is a better answer than this silently dropping it.
fn batches(items: &[evidence::EvidenceItem]) -> Vec<&[evidence::EvidenceItem]> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut bytes = 0;

    for i in 0..items.len() {
        let size = items[i].payload.len();
        let full = i - start >= BATCH_ITEMS || (i > start && bytes + size > BATCH_BYTES);
        if full {
            out.push(&items[start..i]);
            start = i;
            bytes = 0;
        }
        bytes += size;
    }
    if start < items.len() {
        out.push(&items[start..]);
    }
    out
}

/// Send this session's retained evidence to the service that raised the flag.
///
/// Reads only what consent already wrote to disk. A session whose evidence the
/// learner declined, or has since deleted, has nothing to send and says so
/// instead of quietly succeeding — "sent" and "there was nothing to send" are
/// different answers to somebody staking an appeal on it.
#[tauri::command]
pub async fn holder_release_evidence(
    state: State<'_, AppState>,
    directory_url: String,
    run_id: String,
    session_id: String,
) -> Result<Released, String> {
    https_only(&directory_url)?;
    let (sk, _did) = crate::commands::credentials::load_issuer_key(&state).await?;

    let items = {
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        evidence::stored_items(db.conn(), &session_id).map_err(|e| e.to_string())?
    };
    if items.is_empty() {
        return Err(
            "there is no retained evidence for this session — either it was declined, or \
             it has been deleted"
                .into(),
        );
    }

    // Said here rather than discovered halfway through sending. The service
    // refuses the batch that crosses its ceiling, which would leave a partial
    // release on a reviewer's screen and a person wondering which half arrived.
    if items.len() > RUN_CAPACITY {
        return Err(format!(
            "this session kept {} items and an assessment can hold {RUN_CAPACITY} — \
             delete some of what you kept, or withdraw an earlier release, before sending",
            items.len()
        ));
    }

    let bytes = items.iter().map(|i| i.payload.len()).sum();
    let b64 = base64::engine::general_purpose::STANDARD;
    let path = format!("/api/runs/{run_id}/evidence");
    let client = reqwest::Client::new();

    // Sent in the order it was captured, so that a release interrupted partway
    // leaves the earlier part of the session at the other end rather than an
    // arbitrary subset of it.
    let mut sent = 0usize;
    for batch in batches(&items) {
        let body = serde_json::json!({
            "items": batch.iter().map(|i| serde_json::json!({
                "kind": i.kind,
                "captured_at": i.captured_at,
                "payload": b64.encode(&i.payload),
            })).collect::<Vec<_>>()
        });

        if let Err(e) = signed_send(&client, &directory_url, &sk, "POST", &path, Some(body)).await {
            if sent == 0 {
                return Err(e);
            }

            // Part of it is already on somebody else's disk, and the service
            // refuses a partial release for a reason it states plainly: half a
            // session's evidence on a reviewer's screen is worse than none,
            // because it looks like the whole of what somebody chose to send.
            // Sending in batches would break that at the session level, so an
            // incomplete release is taken back rather than left standing.
            //
            // Recorded first. The record is the address a withdrawal is sent
            // to, and if this device dies here the retry on next unlock is what
            // finishes the job.
            record_release(&state, &session_id, &directory_url, &run_id, sent)?;
            {
                let guard = state.db.lock().map_err(|e| e.to_string())?;
                let db = guard.as_ref().ok_or("database not initialized")?;
                want_withdrawal(db.conn(), &session_id, &directory_url, &run_id)
                    .map_err(|e| e.to_string())?;
            }
            let taken_back = withdraw_one(&state, &sk, &session_id, &directory_url, &run_id)
                .await
                .is_ok();

            return Err(if taken_back {
                format!(
                    "this stopped after {sent} of {} item(s), so the part that arrived was \
                     taken back — a half-sent appeal reads as the whole of it. Nothing was \
                     kept there. ({e})",
                    items.len()
                )
            } else {
                format!(
                    "this stopped after {sent} of {} item(s). Taking that part back could \
                     not be sent either, so it is queued and will be withdrawn when this \
                     device next reaches them. ({e})",
                    items.len()
                )
            });
        }
        sent += batch.len();
    }

    record_release(&state, &session_id, &directory_url, &run_id, sent)?;

    Ok(Released { items: sent, bytes })
}

/// Note that a copy now exists at that service, so it can be ended later.
///
/// Written only after the service accepted something. A row is a claim that a
/// copy exists somewhere, and the whole purpose of the row is to be able to
/// destroy that copy — one written for a release that never landed would send a
/// withdrawal for something that was never there.
fn record_release(
    state: &State<'_, AppState>,
    session_id: &str,
    directory_url: &str,
    run_id: &str,
    items: usize,
) -> Result<(), String> {
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    db.conn()
        .execute(
            "INSERT INTO integrity_evidence_release \
                 (session_id, directory_url, run_id, item_count) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT (session_id, directory_url, run_id) DO UPDATE SET \
                 released_at = datetime('now'), item_count = excluded.item_count, \
                 revoke_wanted_at = NULL, revoked_at = NULL",
            rusqlite::params![session_id, directory_url, run_id, items as i64],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Ask a service to destroy what this person released to it.
///
/// Marked as wanted before it is attempted, so a withdrawal survives the
/// attempt failing. Everything that has been asked for is retried by
/// [`holder_retry_withdrawals`].
#[tauri::command]
pub async fn holder_withdraw_evidence(
    state: State<'_, AppState>,
    directory_url: String,
    run_id: String,
    session_id: String,
) -> Result<(), String> {
    {
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        want_withdrawal(db.conn(), &session_id, &directory_url, &run_id)
            .map_err(|e| e.to_string())?;
    }
    let (sk, _did) = crate::commands::credentials::load_issuer_key(&state).await?;
    withdraw_one(&state, &sk, &session_id, &directory_url, &run_id).await
}

/// Record that a withdrawal is owed, whether or not one can be sent now.
pub(crate) fn want_withdrawal(
    conn: &rusqlite::Connection,
    session_id: &str,
    directory_url: &str,
    run_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE integrity_evidence_release SET revoke_wanted_at = datetime('now') \
         WHERE session_id = ?1 AND directory_url = ?2 AND run_id = ?3 \
           AND revoke_wanted_at IS NULL",
        rusqlite::params![session_id, directory_url, run_id],
    )?;
    Ok(())
}

async fn withdraw_one(
    state: &State<'_, AppState>,
    sk: &ed25519_dalek::SigningKey,
    session_id: &str,
    directory_url: &str,
    run_id: &str,
) -> Result<(), String> {
    let path = format!("/api/runs/{run_id}/evidence");
    signed_send(
        &reqwest::Client::new(),
        directory_url,
        sk,
        "DELETE",
        &path,
        None,
    )
    .await?;

    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    db.conn()
        .execute(
            "UPDATE integrity_evidence_release SET revoked_at = datetime('now') \
             WHERE session_id = ?1 AND directory_url = ?2 AND run_id = ?3",
            rusqlite::params![session_id, directory_url, run_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Retry every withdrawal this device still owes.
///
/// Called when a profile is unlocked, and again whenever the Integrity screen
/// is opened. A person who deleted their evidence on a train and closed the
/// laptop has asked for it to be gone; the asking is what matters, and this is
/// what makes the asking survive the connection.
///
/// Driven from the frontend rather than the Rust unlock path because the
/// withdrawal is signed with the learner's own key, which is not available
/// until the vault is open.
///
/// Returns how many are still owed afterwards, so a screen can say "still
/// trying" honestly rather than claiming a deletion that has not happened.
#[tauri::command]
pub async fn holder_retry_withdrawals(state: State<'_, AppState>) -> Result<usize, String> {
    let owed: Vec<(String, String, String)> = {
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        let mut stmt = db
            .conn()
            .prepare(
                "SELECT session_id, directory_url, run_id FROM integrity_evidence_release \
                 WHERE revoke_wanted_at IS NOT NULL AND revoked_at IS NULL",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?;
        rows.filter_map(Result::ok).collect()
    };
    if owed.is_empty() {
        return Ok(0);
    }

    let (sk, _did) = crate::commands::credentials::load_issuer_key(&state).await?;
    let mut remaining = 0usize;
    for (session_id, directory_url, run_id) in owed {
        if withdraw_one(&state, &sk, &session_id, &directory_url, &run_id)
            .await
            .is_err()
        {
            // Left owed on purpose. The next unlock tries again, and a service
            // that is down today is not a reason to stop asking.
            remaining += 1;
        }
    }
    Ok(remaining)
}

/// Everywhere this session's evidence was sent and not yet withdrawn.
pub(crate) fn releases_for_session(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT directory_url, run_id FROM integrity_evidence_release \
         WHERE session_id = ?1 AND revoked_at IS NULL",
    )?;
    let rows = stmt.query_map(rusqlite::params![session_id], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn plaintext_is_refused_and_loopback_is_not() {
        assert!(https_only("https://example.com").is_ok());
        assert!(https_only("http://127.0.0.1:8787").is_ok());
        assert!(https_only("http://example.com").is_err());
        // The costume case: a host that merely begins with a loopback address.
        assert!(https_only("http://127.0.0.1.example.com").is_err());
    }

    fn item(bytes: usize) -> evidence::EvidenceItem {
        evidence::EvidenceItem {
            snapshot_id: "snap".into(),
            kind: "camera_frame".into(),
            payload: vec![0u8; bytes],
            captured_at: "2026-08-14T09:15:00Z".into(),
        }
    }

    /// Everything that fits goes in one request, because most sessions do.
    #[test]
    fn a_small_session_is_one_request() {
        let items: Vec<_> = (0..10).map(|_| item(1024)).collect();
        assert_eq!(batches(&items).len(), 1);
        assert_eq!(batches(&items)[0].len(), 10);
    }

    /// The service refuses more than fifty in one release. A person who kept
    /// sixty frames must not be told their appeal is too big to make.
    #[test]
    fn more_items_than_the_service_takes_are_split_not_refused() {
        let items: Vec<_> = (0..120).map(|_| item(16)).collect();
        let out = batches(&items);
        assert_eq!(out.len(), 3, "fifty, fifty, twenty");
        assert_eq!(
            out.iter().map(|b| b.len()).collect::<Vec<_>>(),
            [50, 50, 20]
        );
        assert_eq!(
            out.iter().map(|b| b.len()).sum::<usize>(),
            items.len(),
            "every item is in exactly one batch"
        );
    }

    /// Camera frames are large, so the byte ceiling binds before the count
    /// does. Base64 inflates by a third and the service caps the body.
    #[test]
    fn heavy_frames_split_on_size_before_they_reach_the_count() {
        // Three megabytes each: three fit under the eight-megabyte ceiling,
        // the fourth starts a new request, and none of this is near fifty.
        let items: Vec<_> = (0..7).map(|_| item(3 * 1024 * 1024)).collect();
        let out = batches(&items);
        assert_eq!(
            out.iter().map(|b| b.len()).collect::<Vec<_>>(),
            [2, 2, 2, 1]
        );
        for b in &out {
            let bytes: usize = b.iter().map(|i| i.payload.len()).sum();
            assert!(bytes <= BATCH_BYTES, "a batch exceeded the ceiling");
        }
    }

    /// One item bigger than a whole batch still gets sent, alone, so that the
    /// service's own explanation of why it will not take it reaches the person.
    /// Dropping it here would be a silent refusal with no author.
    #[test]
    fn an_oversized_item_is_sent_alone_rather_than_dropped() {
        let items = vec![item(16), item(BATCH_BYTES * 2), item(16)];
        let out = batches(&items);
        assert_eq!(out.iter().map(|b| b.len()).sum::<usize>(), 3);
        assert!(
            out.iter()
                .any(|b| b.len() == 1 && b[0].payload.len() > BATCH_BYTES),
            "the large item is in a batch of its own"
        );
    }

    #[test]
    fn nothing_produces_no_requests() {
        assert!(batches(&[]).is_empty());
    }

    fn untold(db: &Database) -> Vec<String> {
        let mut stmt = db
            .conn()
            .prepare("SELECT run_id FROM integrity_flag_notice WHERE told_at IS NULL")
            .expect("prepare");
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .expect("query");
        rows.filter_map(Result::ok).collect()
    }

    fn note(db: &Database, run: &str) {
        db.conn()
            .execute(
                "INSERT INTO integrity_flag_notice (directory_url, run_id, organisation, role_label) \
                 VALUES ('https://svc.example', ?1, 'Acme', 'Backend engineer') \
                 ON CONFLICT (directory_url, run_id) DO NOTHING",
                rusqlite::params![run],
            )
            .expect("note");
    }

    fn tell(db: &Database, run: &str) {
        db.conn()
            .execute(
                "UPDATE integrity_flag_notice SET told_at = datetime('now') \
                 WHERE run_id = ?1 AND told_at IS NULL",
                rusqlite::params![run],
            )
            .expect("tell");
    }

    /// Being accused of something is said once. Repeating it on every unlock
    /// would be its own kind of pressure.
    #[test]
    fn a_flag_is_raised_with_the_learner_once() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");

        note(&db, "run-1");
        assert_eq!(untold(&db), vec!["run-1".to_string()]);

        tell(&db, "run-1");
        assert!(untold(&db).is_empty(), "told once");

        // The export is read again on the next unlock and reports the same
        // flag; the person has already been told and must not be told again.
        note(&db, "run-1");
        assert!(untold(&db).is_empty(), "and not again");
    }

    /// A second, different flag is its own notice. Having been told about one
    /// accusation is not being told about the next.
    #[test]
    fn a_later_flag_is_its_own_notice() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");

        note(&db, "run-1");
        tell(&db, "run-1");
        note(&db, "run-2");

        assert_eq!(untold(&db), vec!["run-2".to_string()]);
    }

    /// What the notice needs in order to name the assessment survives, so a
    /// person is told *which* one rather than that something happened.
    #[test]
    fn a_notice_remembers_what_it_is_about() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        note(&db, "run-1");

        let (org, role): (String, String) = db
            .conn()
            .query_row(
                "SELECT organisation, role_label FROM integrity_flag_notice WHERE run_id='run-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("read");
        assert_eq!(org, "Acme");
        assert_eq!(role, "Backend engineer");
    }

    fn db_with_release() -> Database {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        db.conn()
            .execute(
                "INSERT INTO integrity_evidence_release \
                     (session_id, directory_url, run_id, item_count) \
                 VALUES ('s1', 'https://svc.example', 'run-1', 3)",
                [],
            )
            .expect("seed release");
        db
    }

    fn owed(db: &Database) -> i64 {
        db.conn()
            .query_row(
                "SELECT COUNT(*) FROM integrity_evidence_release \
                 WHERE revoke_wanted_at IS NOT NULL AND revoked_at IS NULL",
                [],
                |r| r.get(0),
            )
            .expect("count")
    }

    /// The whole point of the table: after a release there is somewhere to send
    /// a withdrawal, and it is found by session rather than by asking the
    /// learner to remember who they sent it to.
    #[test]
    fn a_release_is_findable_by_the_session_it_came_from() {
        let db = db_with_release();
        let found = releases_for_session(db.conn(), "s1").expect("read");
        assert_eq!(
            found,
            vec![("https://svc.example".to_string(), "run-1".to_string())]
        );
        assert!(releases_for_session(db.conn(), "other")
            .expect("read")
            .is_empty());
    }

    /// Asking is recorded before anything is sent, so a withdrawal survives the
    /// send failing. This is what makes deleting while offline mean something.
    #[test]
    fn wanting_a_withdrawal_is_recorded_whether_or_not_it_can_be_sent() {
        let db = db_with_release();
        assert_eq!(owed(&db), 0);

        want_withdrawal(db.conn(), "s1", "https://svc.example", "run-1").expect("want");
        assert_eq!(owed(&db), 1, "still owed until a service confirms");
    }

    /// The first asking is the one that counts. A second call must not move the
    /// timestamp forward — the record is of when the person asked, and a retry
    /// loop that rewrote it would make an old request look new every sweep.
    #[test]
    fn asking_twice_does_not_restart_the_clock() {
        let db = db_with_release();
        want_withdrawal(db.conn(), "s1", "https://svc.example", "run-1").expect("want");
        let first: String = db
            .conn()
            .query_row(
                "SELECT revoke_wanted_at FROM integrity_evidence_release",
                [],
                |r| r.get(0),
            )
            .expect("read");

        want_withdrawal(db.conn(), "s1", "https://svc.example", "run-1").expect("want again");
        let second: String = db
            .conn()
            .query_row(
                "SELECT revoke_wanted_at FROM integrity_evidence_release",
                [],
                |r| r.get(0),
            )
            .expect("read");
        assert_eq!(first, second);
    }

    /// A withdrawn release is no longer somewhere a copy exists, so deleting
    /// the same session again does not queue a second withdrawal for it.
    #[test]
    fn a_withdrawn_release_stops_being_a_place_a_copy_lives() {
        let db = db_with_release();
        db.conn()
            .execute(
                "UPDATE integrity_evidence_release SET revoke_wanted_at = datetime('now'), \
                 revoked_at = datetime('now')",
                [],
            )
            .expect("confirm");
        assert!(releases_for_session(db.conn(), "s1")
            .expect("read")
            .is_empty());
        assert_eq!(owed(&db), 0);
    }

    /// The release record outlives the session's own rows. A person may delete
    /// everything about a session locally while a service still holds a copy,
    /// and the instruction to withdraw must not be deleted along with the
    /// reason for it — which a foreign key onto `integrity_sessions` would do.
    #[test]
    fn a_withdrawal_survives_the_session_being_deleted() {
        let db = db_with_release();
        want_withdrawal(db.conn(), "s1", "https://svc.example", "run-1").expect("want");
        db.conn()
            .execute("DELETE FROM integrity_sessions WHERE id = 's1'", [])
            .expect("delete session");
        assert_eq!(owed(&db), 1, "still owed to the service");
    }
}
