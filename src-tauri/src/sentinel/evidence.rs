//! Appeal evidence: staged in memory, persisted only on a learner's say-so.
//!
//! `docs/sentinel.md` gives a learner the right to release evidence in order to
//! contest a flag, and states that only they can. That right needs something to
//! release. Sentinel keeps derived scores and drops everything else, so without
//! this module the right is nominal.
//!
//! The shape here is deliberate. When a snapshot is flagged, its evidence is
//! held **in memory only**, in [`EvidenceStaging`]. Nothing reaches the database
//! until the learner is shown the flag and chooses to preserve it. Decline, or
//! never answer, and the staged bytes die with the process — which is the same
//! outcome the system had before this module existed.
//!
//! That ordering is the entire privacy argument. A learner who does not intend
//! to appeal is never asked to accept retention they did not want, and the
//! documented guarantee that raw capture is never persisted continues to hold
//! for them literally rather than approximately.
//!
//! Bounds on what consent can authorise:
//!
//! - only sessions that were flagged,
//! - only the snapshots that carried a flag, not the whole session,
//! - an absolute `expires_at`, enforced on unlock and on a timer,
//! - deletable by the learner at any moment, before or after expiry.

use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// How long consented evidence survives. An appeal that has not begun within
/// two weeks is not going to be helped by keeping face imagery for a third.
pub const APPEAL_WINDOW_DAYS: i64 = 14;

/// A single piece of evidence backing one flagged snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub snapshot_id: String,
    /// `camera_frame` | `keystroke` | `mouse` | `gaze`
    pub kind: String,
    pub payload: Vec<u8>,
    pub captured_at: String,
}

/// What the consent prompt shows a learner: enough to know what they would be
/// keeping, without the payloads themselves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceSummary {
    pub session_id: String,
    pub camera_frames: usize,
    pub keystroke_windows: usize,
    pub mouse_windows: usize,
    pub gaze_windows: usize,
    /// Total bytes that would be written. Shown because "some camera frames"
    /// is not informed consent and a number is.
    pub total_bytes: usize,
}

impl EvidenceSummary {
    pub fn is_empty(&self) -> bool {
        self.camera_frames == 0
            && self.keystroke_windows == 0
            && self.mouse_windows == 0
            && self.gaze_windows == 0
    }
}

/// In-memory staging for flagged sessions awaiting a consent decision.
///
/// Never serialised, never written to disk. Dropped on process exit, on
/// profile lock, and on an explicit decline.
#[derive(Default)]
pub struct EvidenceStaging {
    by_session: Mutex<HashMap<String, Vec<EvidenceItem>>>,
    /// The most recent camera frame, JPEG-encoded, held for at most one
    /// snapshot interval.
    ///
    /// Camera frames arrive during scoring, which does not know whether the
    /// snapshot about to be written will carry a flag. So the newest frame is
    /// parked here and overwritten every time — and picked up only if the
    /// snapshot turns out to be flagged. An unflagged snapshot simply lets the
    /// next frame overwrite it, and it is never seen again.
    ///
    /// One slot, not a map: a profile runs one integrity session at a time.
    last_frame: Mutex<Option<(String, Vec<u8>)>>,
}

impl EvidenceStaging {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stage evidence for a flagged snapshot. Callers must only invoke this for
    /// snapshots that actually carried a flag — staging an unflagged snapshot
    /// would quietly widen what a later "yes" authorises.
    pub fn stage(&self, session_id: &str, item: EvidenceItem) {
        let mut map = self.by_session.lock().expect("staging lock");
        map.entry(session_id.to_string()).or_default().push(item);
    }

    pub fn summary(&self, session_id: &str) -> EvidenceSummary {
        let map = self.by_session.lock().expect("staging lock");
        let items = map.get(session_id);
        let mut s = EvidenceSummary {
            session_id: session_id.to_string(),
            camera_frames: 0,
            keystroke_windows: 0,
            mouse_windows: 0,
            gaze_windows: 0,
            total_bytes: 0,
        };
        for it in items.into_iter().flatten() {
            s.total_bytes += it.payload.len();
            match it.kind.as_str() {
                "camera_frame" => s.camera_frames += 1,
                "keystroke" => s.keystroke_windows += 1,
                "mouse" => s.mouse_windows += 1,
                "gaze" => s.gaze_windows += 1,
                _ => {}
            }
        }
        s
    }

    /// Drop staged evidence without writing it. Used on decline and on lock.
    pub fn discard(&self, session_id: &str) {
        let mut map = self.by_session.lock().expect("staging lock");
        map.remove(session_id);
    }

    /// Drop everything. Called when a profile locks, so one learner's staged
    /// evidence cannot outlive their session into another's.
    pub fn clear_all(&self) {
        let mut map = self.by_session.lock().expect("staging lock");
        map.clear();
        drop(map);
        *self.last_frame.lock().expect("frame lock") = None;
    }

    /// Park the newest camera frame. Overwrites whatever was there.
    ///
    /// Encoding happens here so the parked copy is ~15 KB rather than the
    /// ~150 KB of raw RGBA, and so a frame that is never used costs little.
    pub fn remember_frame(&self, width: u32, height: u32, rgba: &[u8], captured_at: &str) {
        let Some(buf) = image::RgbaImage::from_raw(width, height, rgba.to_vec()) else {
            return;
        };
        let mut jpeg = Vec::new();
        let dynimg = image::DynamicImage::ImageRgba8(buf).to_rgb8();
        if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80)
            .encode(&dynimg, width, height, image::ExtendedColorType::Rgb8)
            .is_err()
        {
            return;
        }
        let mut slot = self.last_frame.lock().expect("frame lock");
        *slot = Some((captured_at.to_string(), jpeg));
    }

    /// Move the parked frame into staging for a flagged snapshot.
    ///
    /// Call only when the snapshot carried a flag. Returns true if a frame was
    /// available to stage.
    pub fn stage_last_frame(&self, session_id: &str, snapshot_id: &str) -> bool {
        let taken = { self.last_frame.lock().expect("frame lock").take() };
        match taken {
            Some((captured_at, payload)) => {
                self.stage(
                    session_id,
                    EvidenceItem {
                        snapshot_id: snapshot_id.to_string(),
                        kind: "camera_frame".into(),
                        payload,
                        captured_at,
                    },
                );
                true
            }
            None => false,
        }
    }

    fn take(&self, session_id: &str) -> Vec<EvidenceItem> {
        let mut map = self.by_session.lock().expect("staging lock");
        map.remove(session_id).unwrap_or_default()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EvidenceError {
    #[error("db error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("session {0} is not flagged; only flagged sessions may retain evidence")]
    NotFlagged(String),
    #[error("consent for session {0} was already decided")]
    AlreadyDecided(String),
}

/// Record a learner's decision and, if they said yes, write the staged evidence.
///
/// Returns the number of rows persisted — zero when declined.
///
/// Refuses sessions that are not flagged. Consent is meaningful only against a
/// specific accusation; a blanket "keep my evidence" over an unflagged session
/// is the retention this design exists to avoid.
pub fn decide(
    db: &Connection,
    staging: &EvidenceStaging,
    session_id: &str,
    granted: bool,
    now: &str,
) -> Result<usize, EvidenceError> {
    let already: Option<i64> = db
        .query_row(
            "SELECT granted FROM integrity_evidence_consent WHERE session_id = ?1",
            params![session_id],
            |r| r.get(0),
        )
        .ok();
    if already.is_some() {
        return Err(EvidenceError::AlreadyDecided(session_id.to_string()));
    }

    if !granted {
        db.execute(
            "INSERT INTO integrity_evidence_consent (session_id, granted, decided_at, expires_at) \
             VALUES (?1, 0, ?2, NULL)",
            params![session_id, now],
        )?;
        staging.discard(session_id);
        return Ok(0);
    }

    let status: String = db.query_row(
        "SELECT status FROM integrity_sessions WHERE id = ?1",
        params![session_id],
        |r| r.get(0),
    )?;
    if status != "flagged" {
        return Err(EvidenceError::NotFlagged(session_id.to_string()));
    }

    let expires_at = expiry_from(now);
    db.execute(
        "INSERT INTO integrity_evidence_consent (session_id, granted, decided_at, expires_at) \
         VALUES (?1, 1, ?2, ?3)",
        params![session_id, now, &expires_at],
    )?;

    let items = staging.take(session_id);
    let mut written = 0usize;
    for it in items {
        db.execute(
            "INSERT INTO integrity_evidence \
             (id, session_id, snapshot_id, kind, payload, captured_at, expires_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                uuid_like(session_id, written),
                session_id,
                &it.snapshot_id,
                &it.kind,
                &it.payload,
                &it.captured_at,
                &expires_at,
            ],
        )?;
        written += 1;
    }
    Ok(written)
}

/// Delete a session's evidence outright, whenever the learner asks.
///
/// Leaves the consent row so the prompt is not shown again — the decision was
/// made, then revoked, and re-asking would read as pressure.
pub fn delete_for_session(db: &Connection, session_id: &str) -> Result<usize, EvidenceError> {
    let n = db.execute(
        "DELETE FROM integrity_evidence WHERE session_id = ?1",
        params![session_id],
    )?;
    Ok(n)
}

/// Delete every expired row. Called on profile unlock and periodically.
///
/// Compares absolute timestamps, so evidence expires on schedule even if the
/// app was closed for the whole window.
pub fn purge_expired(db: &Connection, now: &str) -> Result<usize, EvidenceError> {
    let n = db.execute(
        "DELETE FROM integrity_evidence WHERE expires_at <= ?1",
        params![now],
    )?;
    Ok(n)
}

/// One piece of evidence rendered for the learner to look at.
///
/// Consent that cannot see what it is authorising is not informed. A learner
/// deciding whether to keep camera frames is entitled to look at the frames
/// first — including to discover that the "second face" was a poster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePreview {
    pub snapshot_id: String,
    pub kind: String,
    pub captured_at: String,
    /// `data:` URL, ready for an `<img src>`. Camera frames only; other kinds
    /// carry `None` and are described by `kind` alone.
    pub data_url: Option<String>,
}

fn to_preview(snapshot_id: &str, kind: &str, captured_at: &str, payload: &[u8]) -> EvidencePreview {
    use base64::Engine;
    let data_url = (kind == "camera_frame").then(|| {
        format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(payload)
        )
    });
    EvidencePreview {
        snapshot_id: snapshot_id.to_string(),
        kind: kind.to_string(),
        captured_at: captured_at.to_string(),
        data_url,
    }
}

/// Staged (not yet persisted) evidence, rendered for the consent prompt.
pub fn preview_staged(staging: &EvidenceStaging, session_id: &str) -> Vec<EvidencePreview> {
    let map = staging.by_session.lock().expect("staging lock");
    map.get(session_id)
        .map(|items| {
            items
                .iter()
                .map(|it| to_preview(&it.snapshot_id, &it.kind, &it.captured_at, &it.payload))
                .collect()
        })
        .unwrap_or_default()
}

/// Retained evidence, rendered for the learner's own review after consent.
pub fn preview_stored(
    db: &Connection,
    session_id: &str,
) -> Result<Vec<EvidencePreview>, EvidenceError> {
    let mut stmt = db.prepare(
        "SELECT snapshot_id, kind, captured_at, payload FROM integrity_evidence \
         WHERE session_id = ?1 ORDER BY captured_at",
    )?;
    let rows = stmt.query_map(params![session_id], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Vec<u8>>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (snap, kind, at, payload) = row?;
        out.push(to_preview(
            snap.as_deref().unwrap_or(""),
            &kind,
            &at,
            &payload,
        ));
    }
    Ok(out)
}

/// What is currently retained for a session, for the learner's own review.
pub fn stored_summary(db: &Connection, session_id: &str) -> Result<EvidenceSummary, EvidenceError> {
    let mut stmt =
        db.prepare("SELECT kind, length(payload) FROM integrity_evidence WHERE session_id = ?1")?;
    let rows = stmt.query_map(params![session_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut s = EvidenceSummary {
        session_id: session_id.to_string(),
        camera_frames: 0,
        keystroke_windows: 0,
        mouse_windows: 0,
        gaze_windows: 0,
        total_bytes: 0,
    };
    for row in rows {
        let (kind, len) = row?;
        s.total_bytes += len.max(0) as usize;
        match kind.as_str() {
            "camera_frame" => s.camera_frames += 1,
            "keystroke" => s.keystroke_windows += 1,
            "mouse" => s.mouse_windows += 1,
            "gaze" => s.gaze_windows += 1,
            _ => {}
        }
    }
    Ok(s)
}

/// `now` plus the appeal window, as an ISO 8601 UTC string.
fn expiry_from(now: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(now) {
        Ok(dt) => (dt + chrono::Duration::days(APPEAL_WINDOW_DAYS))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        // An unparseable clock must not silently mean "never expires".
        Err(_) => (chrono::Utc::now() + chrono::Duration::days(APPEAL_WINDOW_DAYS))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    }
}

/// Stable per-row id. Evidence rows are addressed only within a session, so a
/// session-scoped counter is sufficient and keeps inserts deterministic in
/// tests.
fn uuid_like(session_id: &str, n: usize) -> String {
    format!("{session_id}:ev:{n}")
}

// ---------------------------------------------------------------------------
// Tests.
//
// These assert a privacy posture, not just behaviour. The load-bearing ones are
// the negatives: that staging writes nothing, that a decline writes nothing, and
// that expiry actually deletes. A regression in any of those turns a documented
// guarantee into a false statement, so each is pinned separately rather than
// folded into a happy-path test.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    const NOW: &str = "2026-08-12T00:00:00Z";

    fn db_with_session(status: &str) -> Database {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        db.conn()
            .execute(
                "INSERT INTO integrity_sessions (id, status, started_at) VALUES ('s1', ?1, ?2)",
                params![status, NOW],
            )
            .expect("seed session");
        // Evidence is keyed to the snapshot it backs, and the FK is enforced —
        // a row cannot claim to be evidence for a snapshot that never happened.
        for n in 1..=2 {
            db.conn()
                .execute(
                    "INSERT INTO integrity_snapshots (id, session_id, captured_at) \
                     VALUES (?1, 's1', ?2)",
                    params![format!("snap{n}"), NOW],
                )
                .expect("seed snapshot");
        }
        db
    }

    fn frame(n: usize) -> EvidenceItem {
        EvidenceItem {
            snapshot_id: format!("snap{n}"),
            kind: "camera_frame".into(),
            payload: vec![0xAB; 128],
            captured_at: NOW.into(),
        }
    }

    fn stored_rows(db: &Database) -> i64 {
        db.conn()
            .query_row("SELECT COUNT(*) FROM integrity_evidence", [], |r| r.get(0))
            .expect("count")
    }

    #[test]
    fn staging_alone_writes_nothing_to_the_database() {
        // The whole privacy argument rests on this ordering: evidence exists in
        // memory while the learner decides, and nowhere else.
        let db = db_with_session("flagged");
        let staging = EvidenceStaging::new();
        staging.stage("s1", frame(1));
        staging.stage("s1", frame(2));

        assert_eq!(staging.summary("s1").camera_frames, 2);
        assert_eq!(stored_rows(&db), 0, "staging must not touch the database");
    }

    #[test]
    fn declining_persists_nothing_and_drops_the_staged_bytes() {
        let db = db_with_session("flagged");
        let staging = EvidenceStaging::new();
        staging.stage("s1", frame(1));

        let written = decide(db.conn(), &staging, "s1", false, NOW).expect("decline");
        assert_eq!(written, 0);
        assert_eq!(stored_rows(&db), 0);
        assert!(
            staging.summary("s1").is_empty(),
            "declining must discard the staged evidence, not merely skip writing it"
        );
    }

    #[test]
    fn granting_persists_only_what_was_staged() {
        let db = db_with_session("flagged");
        let staging = EvidenceStaging::new();
        staging.stage("s1", frame(1));
        staging.stage("s1", frame(2));

        let written = decide(db.conn(), &staging, "s1", true, NOW).expect("grant");
        assert_eq!(written, 2);
        assert_eq!(stored_rows(&db), 2);

        let s = stored_summary(db.conn(), "s1").expect("summary");
        assert_eq!(s.camera_frames, 2);
        assert_eq!(s.total_bytes, 256);
    }

    #[test]
    fn consent_is_refused_for_a_session_that_was_never_flagged() {
        // Consent answers a specific accusation. Without one there is nothing to
        // contest, and a blanket yes would authorise open-ended retention.
        let db = db_with_session("completed");
        let staging = EvidenceStaging::new();
        staging.stage("s1", frame(1));

        let err = decide(db.conn(), &staging, "s1", true, NOW).expect_err("must refuse");
        assert!(matches!(err, EvidenceError::NotFlagged(_)));
        assert_eq!(stored_rows(&db), 0);
    }

    #[test]
    fn consent_cannot_be_asked_twice() {
        // Re-prompting after a no is how a refusal gets worn down.
        let db = db_with_session("flagged");
        let staging = EvidenceStaging::new();
        decide(db.conn(), &staging, "s1", false, NOW).expect("first");

        staging.stage("s1", frame(1));
        let err = decide(db.conn(), &staging, "s1", true, NOW).expect_err("must refuse");
        assert!(matches!(err, EvidenceError::AlreadyDecided(_)));
        assert_eq!(stored_rows(&db), 0);
    }

    #[test]
    fn expired_evidence_is_purged() {
        let db = db_with_session("flagged");
        let staging = EvidenceStaging::new();
        staging.stage("s1", frame(1));
        decide(db.conn(), &staging, "s1", true, NOW).expect("grant");
        assert_eq!(stored_rows(&db), 1);

        // One second before the deadline: still retained.
        let just_before = "2026-08-25T23:59:59Z";
        purge_expired(db.conn(), just_before).expect("purge");
        assert_eq!(stored_rows(&db), 1, "must not expire early");

        // Past the deadline, even if the app was closed the whole time.
        let after = "2026-09-01T00:00:00Z";
        let n = purge_expired(db.conn(), after).expect("purge");
        assert_eq!(n, 1);
        assert_eq!(stored_rows(&db), 0);
    }

    #[test]
    fn the_learner_can_delete_evidence_before_it_expires() {
        let db = db_with_session("flagged");
        let staging = EvidenceStaging::new();
        staging.stage("s1", frame(1));
        decide(db.conn(), &staging, "s1", true, NOW).expect("grant");

        let n = delete_for_session(db.conn(), "s1").expect("delete");
        assert_eq!(n, 1);
        assert_eq!(stored_rows(&db), 0);

        // The consent row survives, so the prompt is not shown again.
        let consent: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM integrity_evidence_consent WHERE session_id = 's1'",
                [],
                |r| r.get(0),
            )
            .expect("consent row");
        assert_eq!(consent, 1);
    }

    #[test]
    fn locking_a_profile_clears_staged_evidence_for_every_session() {
        let staging = EvidenceStaging::new();
        staging.stage("s1", frame(1));
        staging.stage("s2", frame(2));
        staging.clear_all();
        assert!(staging.summary("s1").is_empty());
        assert!(staging.summary("s2").is_empty());
    }

    #[test]
    fn deleting_the_session_cascades_to_its_evidence() {
        let db = db_with_session("flagged");
        let staging = EvidenceStaging::new();
        staging.stage("s1", frame(1));
        decide(db.conn(), &staging, "s1", true, NOW).expect("grant");

        db.conn()
            .execute("PRAGMA foreign_keys = ON", [])
            .expect("fk pragma");
        db.conn()
            .execute("DELETE FROM integrity_sessions WHERE id = 's1'", [])
            .expect("delete session");
        assert_eq!(stored_rows(&db), 0, "evidence must not outlive its session");
    }
}
