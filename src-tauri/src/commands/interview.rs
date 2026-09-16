//! Local-first interview planning, transcription, and review.
//!
//! Live media continues to use the tutoring transport. This module owns the
//! durable interview record only: consent, attributed transcript segments,
//! rubric coverage, private notes, follow-up suggestions, and the review
//! draft. None of these tables participate in P2P sync or gossip.

use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::crypto::hash::entity_id;
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterviewSession {
    pub id: String,
    pub title: String,
    pub objective: Option<String>,
    pub role_assessment_id: Option<String>,
    pub tutoring_session_id: Option<String>,
    pub status: String,
    pub duration_minutes: i64,
    pub retention_days: i64,
    pub record_audio: bool,
    pub record_video: bool,
    pub sentinel_enabled: bool,
    pub integrity_session_id: Option<String>,
    pub summary: Option<String>,
    pub conclusion: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterviewParticipant {
    pub id: String,
    pub session_id: String,
    pub peer_id: Option<String>,
    pub display_name: String,
    pub role: String,
    pub pseudonym: String,
    pub consent_transcription: bool,
    pub consent_audio_recording: bool,
    pub consent_video_recording: bool,
    pub consent_sentinel: bool,
    pub consent_camera: bool,
    pub consented_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterviewCriterion {
    pub id: String,
    pub session_id: String,
    pub label: String,
    pub position: i64,
    pub status: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterviewTranscriptSegment {
    pub id: String,
    pub session_id: String,
    pub participant_id: String,
    pub speaker_label: String,
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub is_final: bool,
    pub confidence: Option<f64>,
    pub source: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterviewNote {
    pub id: String,
    pub session_id: String,
    pub text: String,
    pub is_private: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterviewFollowup {
    pub id: String,
    pub session_id: String,
    pub source_segment_id: Option<String>,
    pub criterion_id: Option<String>,
    pub question: String,
    pub reason: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterviewBundle {
    pub session: InterviewSession,
    pub participants: Vec<InterviewParticipant>,
    pub criteria: Vec<InterviewCriterion>,
    pub transcript: Vec<InterviewTranscriptSegment>,
    pub notes: Vec<InterviewNote>,
    pub followups: Vec<InterviewFollowup>,
}

#[derive(Debug, Deserialize)]
pub struct CreateInterviewParticipant {
    pub display_name: String,
    pub role: String,
    #[serde(default)]
    pub peer_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateInterviewRequest {
    pub title: String,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub role_assessment_id: Option<String>,
    #[serde(default)]
    pub tutoring_session_id: Option<String>,
    #[serde(default = "default_duration")]
    pub duration_minutes: i64,
    #[serde(default = "default_retention")]
    pub retention_days: i64,
    #[serde(default)]
    pub record_audio: bool,
    #[serde(default)]
    pub record_video: bool,
    #[serde(default = "default_true")]
    pub sentinel_enabled: bool,
    #[serde(default)]
    pub participants: Vec<CreateInterviewParticipant>,
    #[serde(default)]
    pub criteria: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct InterviewConsentRequest {
    pub consent_transcription: bool,
    pub consent_audio_recording: bool,
    pub consent_video_recording: bool,
    pub consent_sentinel: bool,
    pub consent_camera: bool,
}

#[derive(Debug, Deserialize)]
pub struct AppendTranscriptRequest {
    pub session_id: String,
    #[serde(default)]
    pub participant_id: String,
    pub speaker_label: String,
    pub text: String,
    #[serde(default)]
    pub start_ms: i64,
    #[serde(default)]
    pub end_ms: i64,
    #[serde(default = "default_true")]
    pub is_final: bool,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default = "default_transcript_source")]
    pub source: String,
}

fn default_duration() -> i64 {
    45
}

fn default_retention() -> i64 {
    30
}

fn default_true() -> bool {
    true
}

fn default_transcript_source() -> String {
    "manual".to_owned()
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn bool_col(row: &Row<'_>, index: usize) -> rusqlite::Result<bool> {
    Ok(row.get::<_, i64>(index)? != 0)
}

fn map_session(row: &Row<'_>) -> rusqlite::Result<InterviewSession> {
    Ok(InterviewSession {
        id: row.get(0)?,
        title: row.get(1)?,
        objective: row.get(2)?,
        role_assessment_id: row.get(3)?,
        tutoring_session_id: row.get(4)?,
        status: row.get(5)?,
        duration_minutes: row.get(6)?,
        retention_days: row.get(7)?,
        record_audio: bool_col(row, 8)?,
        record_video: bool_col(row, 9)?,
        sentinel_enabled: bool_col(row, 10)?,
        integrity_session_id: row.get(11)?,
        summary: row.get(12)?,
        conclusion: row.get(13)?,
        created_at: row.get(14)?,
        started_at: row.get(15)?,
        ended_at: row.get(16)?,
        expires_at: row.get(17)?,
    })
}

const SESSION_COLUMNS: &str =
    "id, title, objective, role_assessment_id, tutoring_session_id, status,
    duration_minutes, retention_days, record_audio, record_video, sentinel_enabled,
    integrity_session_id, summary, conclusion, created_at, started_at, ended_at, expires_at";

pub fn create_interview_impl(
    conn: &Connection,
    req: &CreateInterviewRequest,
    created_at: &str,
) -> Result<InterviewBundle, String> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err("interview title is required".into());
    }
    if !(5..=480).contains(&req.duration_minutes) {
        return Err("duration_minutes must be between 5 and 480".into());
    }
    if !(1..=365).contains(&req.retention_days) {
        return Err("retention_days must be between 1 and 365".into());
    }
    if req.participants.is_empty() {
        return Err("at least one interview participant is required".into());
    }
    if !req
        .participants
        .iter()
        .any(|participant| participant.role == "candidate")
    {
        return Err("at least one candidate participant is required".into());
    }
    for participant in &req.participants {
        if participant.display_name.trim().is_empty() {
            return Err("participant display_name is required".into());
        }
        if !matches!(
            participant.role.as_str(),
            "interviewer" | "candidate" | "observer"
        ) {
            return Err(format!("invalid participant role: {}", participant.role));
        }
    }

    let id = entity_id(&[title, created_at]);
    let expires_at = (Utc::now() + Duration::days(req.retention_days)).to_rfc3339();
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO interview_sessions
         (id, title, objective, role_assessment_id, tutoring_session_id, duration_minutes,
          retention_days, record_audio, record_video, sentinel_enabled, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            id,
            title,
            req.objective
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
            req.role_assessment_id,
            req.tutoring_session_id,
            req.duration_minutes,
            req.retention_days,
            req.record_audio,
            req.record_video,
            req.sentinel_enabled,
            created_at,
            expires_at,
        ],
    )
    .map_err(|e| e.to_string())?;

    for (index, participant) in req.participants.iter().enumerate() {
        let participant_id = entity_id(&[&id, &participant.role, &index.to_string()]);
        let pseudonym = format!("Participant {}", index + 1);
        tx.execute(
            "INSERT INTO interview_participants
             (id, session_id, peer_id, display_name, role, pseudonym, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                participant_id,
                id,
                participant.peer_id,
                participant.display_name.trim(),
                participant.role,
                pseudonym,
                created_at,
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for (index, label) in req.criteria.iter().enumerate() {
        let trimmed = label.trim();
        if trimmed.is_empty() {
            continue;
        }
        let criterion_id = entity_id(&[&id, trimmed, &index.to_string()]);
        tx.execute(
            "INSERT INTO interview_criteria (id, session_id, label, position)
             VALUES (?1, ?2, ?3, ?4)",
            params![criterion_id, id, trimmed, index as i64],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    get_interview_impl(conn, &id)?.ok_or_else(|| "failed to create interview".into())
}

fn list_participants(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<InterviewParticipant>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, peer_id, display_name, role, pseudonym,
             consent_transcription, consent_audio_recording, consent_video_recording,
             consent_sentinel, consent_camera, consented_at, revoked_at
             FROM interview_participants WHERE session_id = ?1 ORDER BY created_at, id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(InterviewParticipant {
                id: row.get(0)?,
                session_id: row.get(1)?,
                peer_id: row.get(2)?,
                display_name: row.get(3)?,
                role: row.get(4)?,
                pseudonym: row.get(5)?,
                consent_transcription: bool_col(row, 6)?,
                consent_audio_recording: bool_col(row, 7)?,
                consent_video_recording: bool_col(row, 8)?,
                consent_sentinel: bool_col(row, 9)?,
                consent_camera: bool_col(row, 10)?,
                consented_at: row.get(11)?,
                revoked_at: row.get(12)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn list_criteria(conn: &Connection, session_id: &str) -> Result<Vec<InterviewCriterion>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, label, position, status, notes FROM interview_criteria
             WHERE session_id = ?1 ORDER BY position, id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(InterviewCriterion {
                id: row.get(0)?,
                session_id: row.get(1)?,
                label: row.get(2)?,
                position: row.get(3)?,
                status: row.get(4)?,
                notes: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn list_transcript(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<InterviewTranscriptSegment>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, participant_id, speaker_label, text, start_ms, end_ms,
             is_final, confidence, source, created_at FROM interview_transcript_segments
             WHERE session_id = ?1 ORDER BY start_ms, created_at, id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(InterviewTranscriptSegment {
                id: row.get(0)?,
                session_id: row.get(1)?,
                participant_id: row.get(2)?,
                speaker_label: row.get(3)?,
                text: row.get(4)?,
                start_ms: row.get(5)?,
                end_ms: row.get(6)?,
                is_final: bool_col(row, 7)?,
                confidence: row.get(8)?,
                source: row.get(9)?,
                created_at: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn list_notes(conn: &Connection, session_id: &str) -> Result<Vec<InterviewNote>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, text, is_private, created_at, updated_at
             FROM interview_notes WHERE session_id = ?1 ORDER BY created_at, id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(InterviewNote {
                id: row.get(0)?,
                session_id: row.get(1)?,
                text: row.get(2)?,
                is_private: bool_col(row, 3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn list_followups(conn: &Connection, session_id: &str) -> Result<Vec<InterviewFollowup>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, source_segment_id, criterion_id, question, reason, status, created_at
             FROM interview_followups WHERE session_id = ?1
             ORDER BY CASE status WHEN 'suggested' THEN 0 WHEN 'asked' THEN 1 ELSE 2 END, created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(InterviewFollowup {
                id: row.get(0)?,
                session_id: row.get(1)?,
                source_segment_id: row.get(2)?,
                criterion_id: row.get(3)?,
                question: row.get(4)?,
                reason: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn get_interview_impl(conn: &Connection, id: &str) -> Result<Option<InterviewBundle>, String> {
    let sql = format!("SELECT {SESSION_COLUMNS} FROM interview_sessions WHERE id = ?1");
    let session = conn
        .query_row(&sql, params![id], map_session)
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(session) = session else {
        return Ok(None);
    };
    Ok(Some(InterviewBundle {
        participants: list_participants(conn, id)?,
        criteria: list_criteria(conn, id)?,
        transcript: list_transcript(conn, id)?,
        notes: list_notes(conn, id)?,
        followups: list_followups(conn, id)?,
        session,
    }))
}

pub fn list_interviews_impl(conn: &Connection) -> Result<Vec<InterviewSession>, String> {
    let sql = format!(
        "SELECT {SESSION_COLUMNS} FROM interview_sessions WHERE julianday(expires_at) > julianday('now')
         ORDER BY CASE status WHEN 'live' THEN 0 WHEN 'ready' THEN 1 WHEN 'draft' THEN 2 ELSE 3 END,
         created_at DESC"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], map_session).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn append_transcript_impl(
    conn: &Connection,
    req: &AppendTranscriptRequest,
    created_at: &str,
) -> Result<InterviewTranscriptSegment, String> {
    let text = req.text.trim();
    if text.is_empty() {
        return Err("transcript text is required".into());
    }
    if !matches!(req.source.as_str(), "local_stt" | "remote_stt" | "manual") {
        return Err(format!("invalid transcript source: {}", req.source));
    }
    let participant_id = req.participant_id.trim();
    if participant_id.is_empty() {
        return Err("an attributed participant is required for transcript storage".into());
    }
    let (consent, revoked_at, speaker_label) = conn
        .query_row(
            "SELECT consent_transcription, revoked_at, display_name FROM interview_participants
             WHERE id = ?1 AND session_id = ?2",
            params![participant_id, req.session_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? != 0,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "participant not found in interview".to_owned())?;
    if !consent || revoked_at.is_some() {
        return Err("participant has not consented to transcription".into());
    }
    let id = entity_id(&[&req.session_id, created_at, text]);
    conn.execute(
        "INSERT INTO interview_transcript_segments
         (id, session_id, participant_id, speaker_label, text, start_ms, end_ms,
          is_final, confidence, source, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            req.session_id,
            participant_id,
            speaker_label,
            text,
            req.start_ms.max(0),
            req.end_ms.max(req.start_ms),
            req.is_final,
            req.confidence.map(|value| value.clamp(0.0, 1.0)),
            req.source,
            created_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    list_transcript(conn, &req.session_id)?
        .into_iter()
        .find(|segment| segment.id == id)
        .ok_or_else(|| "failed to append transcript segment".into())
}

pub fn recommend_followups_impl(
    conn: &Connection,
    session_id: &str,
    source_segment_id: Option<&str>,
    created_at: &str,
) -> Result<Vec<InterviewFollowup>, String> {
    let transcript = list_transcript(conn, session_id)?;
    let source = source_segment_id
        .and_then(|id| transcript.iter().find(|segment| segment.id == id))
        .or_else(|| transcript.last());
    let criteria = list_criteria(conn, session_id)?;
    let mut candidates: Vec<(String, String, Option<String>)> = Vec::new();

    if let Some(segment) = source {
        let lower = segment.text.to_lowercase();
        let has_number = segment
            .text
            .chars()
            .any(|character| character.is_ascii_digit());
        if !has_number
            && [
                "improved",
                "faster",
                "better",
                "reduced",
                "increased",
                "successful",
            ]
            .iter()
            .any(|word| lower.contains(word))
        {
            candidates.push((
                "What baseline and measurement did you use to verify that improvement?".into(),
                "The answer states an outcome without a measurable baseline.".into(),
                None,
            ));
        }
        if lower.contains("we ") || lower.starts_with("we") {
            candidates.push((
                "Which decisions and deliverables were specifically yours?".into(),
                "Clarifies the candidate's individual contribution.".into(),
                None,
            ));
        }
        if ["cache", "queue", "database", "service", "api", "model"]
            .iter()
            .any(|word| lower.contains(word))
        {
            candidates.push((
                "What failure mode worried you most, and how did you test or mitigate it?".into(),
                "Probes operational depth and trade-off awareness.".into(),
                None,
            ));
        }
        if segment.text.split_whitespace().count() < 18 {
            candidates.push((
                "Could you walk me through a concrete example step by step?".into(),
                "The answer is brief enough that important evidence may be missing.".into(),
                None,
            ));
        }
    }

    if let Some(criterion) = criteria
        .iter()
        .find(|criterion| criterion.status == "not_covered")
    {
        candidates.push((
            format!(
                "Can you share an example that demonstrates {}?",
                criterion.label
            ),
            "Keeps the interview aligned with an uncovered criterion.".into(),
            Some(criterion.id.clone()),
        ));
    }

    for (index, (question, reason, criterion_id)) in candidates.into_iter().take(3).enumerate() {
        let id = entity_id(&[
            session_id,
            source
                .map(|segment| segment.id.as_str())
                .unwrap_or("opening"),
            &index.to_string(),
            &question,
        ]);
        conn.execute(
            "INSERT OR IGNORE INTO interview_followups
             (id, session_id, source_segment_id, criterion_id, question, reason, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                session_id,
                source.map(|segment| segment.id.as_str()),
                criterion_id,
                question,
                reason,
                created_at,
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    list_followups(conn, session_id)
}

pub fn generate_summary_impl(conn: &Connection, session_id: &str) -> Result<String, String> {
    let transcript = list_transcript(conn, session_id)?;
    let criteria = list_criteria(conn, session_id)?;
    let covered = criteria
        .iter()
        .filter(|criterion| criterion.status == "covered")
        .count();
    let partial = criteria
        .iter()
        .filter(|criterion| criterion.status == "partial")
        .count();
    let mut lines = vec![
        "Interview summary draft".to_owned(),
        format!(
            "Coverage: {covered} covered, {partial} partial, {} not covered.",
            criteria.len().saturating_sub(covered + partial)
        ),
        format!("Transcript: {} attributed segments.", transcript.len()),
    ];
    if !criteria.is_empty() {
        lines.push("Criteria:".into());
        for criterion in &criteria {
            lines.push(format!(
                "- {} — {}",
                criterion.label,
                criterion.status.replace('_', " ")
            ));
        }
    }
    if !transcript.is_empty() {
        lines.push("Evidence highlights:".into());
        for segment in transcript.iter().rev().take(5).rev() {
            let minutes = segment.start_ms / 60_000;
            let seconds = (segment.start_ms / 1_000) % 60;
            let excerpt: String = segment.text.chars().take(180).collect();
            lines.push(format!(
                "- [{minutes:02}:{seconds:02}] {}: {}{}",
                segment.speaker_label,
                excerpt,
                if segment.text.chars().count() > 180 {
                    "…"
                } else {
                    ""
                }
            ));
        }
    }
    let summary = lines.join("\n");
    conn.execute(
        "UPDATE interview_sessions SET summary = ?2 WHERE id = ?1",
        params![session_id, summary],
    )
    .map_err(|e| e.to_string())?;
    Ok(summary)
}

/// Run interview SQL on the bounded executor under the caller's profile lease.
async fn run_interview_job<T, F>(
    state: &State<'_, AppState>,
    label: &'static str,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T, String> + Send + 'static,
{
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            label,
            move |db| operation(db.conn()),
        )
        .await
}

#[tauri::command]
pub async fn interview_create(
    state: State<'_, AppState>,
    req: CreateInterviewRequest,
) -> Result<InterviewBundle, String> {
    run_interview_job(&state, "interview.create", move |conn| {
        create_interview_impl(conn, &req, &now())
    })
    .await
}

#[tauri::command]
pub async fn interview_list(state: State<'_, AppState>) -> Result<Vec<InterviewSession>, String> {
    run_interview_job(&state, "interview.list", list_interviews_impl).await
}

#[tauri::command]
pub async fn interview_get(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<InterviewBundle>, String> {
    run_interview_job(&state, "interview.get", move |conn| {
        get_interview_impl(conn, &id)
    })
    .await
}

#[tauri::command]
pub async fn interview_record_consent(
    state: State<'_, AppState>,
    participant_id: String,
    consent: InterviewConsentRequest,
) -> Result<InterviewParticipant, String> {
    run_interview_job(&state, "interview.record-consent", move |conn| {
        record_consent_impl(conn, &participant_id, &consent, &now())
    })
    .await
}

fn record_consent_impl(
    conn: &Connection,
    participant_id: &str,
    consent: &InterviewConsentRequest,
    timestamp: &str,
) -> Result<InterviewParticipant, String> {
    let all_revoked = !consent.consent_transcription
        && !consent.consent_audio_recording
        && !consent.consent_video_recording
        && !consent.consent_sentinel
        && !consent.consent_camera;
    conn.execute(
        "UPDATE interview_participants SET
             consent_transcription = ?2, consent_audio_recording = ?3,
             consent_video_recording = ?4, consent_sentinel = ?5, consent_camera = ?6,
             consented_at = ?8,
             revoked_at = CASE WHEN ?7 = 1 THEN ?8 ELSE NULL END
             WHERE id = ?1",
        params![
            participant_id,
            consent.consent_transcription,
            consent.consent_audio_recording,
            consent.consent_video_recording,
            consent.consent_sentinel,
            consent.consent_camera,
            all_revoked,
            timestamp,
        ],
    )
    .map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id, session_id, peer_id, display_name, role, pseudonym,
             consent_transcription, consent_audio_recording, consent_video_recording,
             consent_sentinel, consent_camera, consented_at, revoked_at
             FROM interview_participants WHERE id = ?1",
        params![participant_id],
        |row| {
            Ok(InterviewParticipant {
                id: row.get(0)?,
                session_id: row.get(1)?,
                peer_id: row.get(2)?,
                display_name: row.get(3)?,
                role: row.get(4)?,
                pseudonym: row.get(5)?,
                consent_transcription: bool_col(row, 6)?,
                consent_audio_recording: bool_col(row, 7)?,
                consent_video_recording: bool_col(row, 8)?,
                consent_sentinel: bool_col(row, 9)?,
                consent_camera: bool_col(row, 10)?,
                consented_at: row.get(11)?,
                revoked_at: row.get(12)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn interview_append_transcript(
    state: State<'_, AppState>,
    req: AppendTranscriptRequest,
) -> Result<InterviewTranscriptSegment, String> {
    run_interview_job(&state, "interview.append-transcript", move |conn| {
        append_transcript_impl(conn, &req, &now())
    })
    .await
}

#[tauri::command]
pub async fn interview_recommend_followups(
    state: State<'_, AppState>,
    session_id: String,
    source_segment_id: Option<String>,
) -> Result<Vec<InterviewFollowup>, String> {
    run_interview_job(&state, "interview.recommend-followups", move |conn| {
        recommend_followups_impl(conn, &session_id, source_segment_id.as_deref(), &now())
    })
    .await
}

#[tauri::command]
pub async fn interview_set_followup_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
) -> Result<(), String> {
    if !matches!(status.as_str(), "suggested" | "asked" | "dismissed") {
        return Err(format!("invalid follow-up status: {status}"));
    }
    run_interview_job(&state, "interview.set-followup-status", move |conn| {
        conn.execute(
            "UPDATE interview_followups SET status = ?2 WHERE id = ?1",
            params![id, status],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn interview_set_criterion(
    state: State<'_, AppState>,
    id: String,
    status: String,
    notes: Option<String>,
) -> Result<(), String> {
    if !matches!(status.as_str(), "not_covered" | "partial" | "covered") {
        return Err(format!("invalid criterion status: {status}"));
    }
    run_interview_job(&state, "interview.set-criterion", move |conn| {
        conn.execute(
            "UPDATE interview_criteria SET status = ?2, notes = ?3 WHERE id = ?1",
            params![id, status, notes],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn interview_save_note(
    state: State<'_, AppState>,
    session_id: String,
    id: Option<String>,
    text: String,
) -> Result<InterviewNote, String> {
    let trimmed = text.trim().to_owned();
    if trimmed.is_empty() {
        return Err("note text is required".into());
    }
    run_interview_job(&state, "interview.save-note", move |conn| {
        let timestamp = now();
        let note_id = id.unwrap_or_else(|| entity_id(&[&session_id, &timestamp, &trimmed]));
        conn.execute(
            "INSERT INTO interview_notes (id, session_id, text, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(id) DO UPDATE SET text = excluded.text, updated_at = excluded.updated_at",
            params![note_id, session_id, trimmed, timestamp],
        )
        .map_err(|e| e.to_string())?;
        list_notes(conn, &session_id)?
            .into_iter()
            .find(|note| note.id == note_id)
            .ok_or_else(|| "failed to save note".into())
    })
    .await
}

#[tauri::command]
pub async fn interview_set_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
    tutoring_session_id: Option<String>,
    integrity_session_id: Option<String>,
) -> Result<InterviewBundle, String> {
    if !matches!(status.as_str(), "draft" | "ready" | "live" | "completed") {
        return Err(format!("invalid interview status: {status}"));
    }
    run_interview_job(&state, "interview.set-status", move |conn| {
        let timestamp = now();
        conn.execute(
            "UPDATE interview_sessions SET status = ?2,
             tutoring_session_id = COALESCE(?3, tutoring_session_id),
             integrity_session_id = COALESCE(?4, integrity_session_id),
             started_at = CASE WHEN ?2 = 'live' THEN COALESCE(started_at, ?5) ELSE started_at END,
             ended_at = CASE WHEN ?2 = 'completed' THEN COALESCE(ended_at, ?5) ELSE ended_at END
             WHERE id = ?1",
            params![
                id,
                status,
                tutoring_session_id,
                integrity_session_id,
                timestamp
            ],
        )
        .map_err(|e| e.to_string())?;
        get_interview_impl(conn, &id)?.ok_or_else(|| "interview not found".into())
    })
    .await
}

#[tauri::command]
pub async fn interview_generate_summary(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    run_interview_job(&state, "interview.generate-summary", move |conn| {
        generate_summary_impl(conn, &session_id)
    })
    .await
}

#[tauri::command]
pub async fn interview_save_review(
    state: State<'_, AppState>,
    session_id: String,
    summary: String,
    conclusion: String,
) -> Result<(), String> {
    run_interview_job(&state, "interview.save-review", move |conn| {
        conn.execute(
            "UPDATE interview_sessions SET summary = ?2, conclusion = ?3 WHERE id = ?1",
            params![session_id, summary.trim(), conclusion.trim()],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn interview_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    run_interview_job(&state, "interview.delete", move |conn| {
        conn.execute("DELETE FROM interview_sessions WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn interview_purge_expired(state: State<'_, AppState>) -> Result<u64, String> {
    run_interview_job(&state, "interview.purge-expired", |conn| {
        let count = conn
            .execute(
                "DELETE FROM interview_sessions WHERE julianday(expires_at) <= julianday('now')",
                [],
            )
            .map_err(|e| e.to_string())?;
        Ok(count as u64)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    const NOW: &str = "2026-09-15T10:00:00Z";

    fn setup() -> Database {
        let db = Database::open_in_memory().expect("in-memory database");
        db.run_migrations().expect("migrations");
        db
    }

    fn request() -> CreateInterviewRequest {
        CreateInterviewRequest {
            title: "Systems interview".into(),
            objective: Some("Assess operational reasoning".into()),
            role_assessment_id: None,
            tutoring_session_id: None,
            duration_minutes: 45,
            retention_days: 30,
            record_audio: false,
            record_video: false,
            sentinel_enabled: true,
            participants: vec![CreateInterviewParticipant {
                display_name: "Candidate A".into(),
                role: "candidate".into(),
                peer_id: None,
            }],
            criteria: vec!["System design".into(), "Communication".into()],
        }
    }

    #[test]
    fn creates_private_interview_bundle_with_explicit_defaults() {
        let db = setup();
        let bundle = create_interview_impl(db.conn(), &request(), NOW).unwrap();
        assert_eq!(bundle.session.status, "draft");
        assert_eq!(bundle.participants.len(), 1);
        assert_eq!(bundle.criteria.len(), 2);
        assert!(!bundle.participants[0].consent_transcription);
        assert!(bundle.session.sentinel_enabled);
    }

    #[test]
    fn transcript_requires_active_participant_consent() {
        let db = setup();
        let bundle = create_interview_impl(db.conn(), &request(), NOW).unwrap();
        let participant = &bundle.participants[0];
        let req = AppendTranscriptRequest {
            session_id: bundle.session.id.clone(),
            participant_id: participant.id.clone(),
            speaker_label: participant.display_name.clone(),
            text: "We improved the service.".into(),
            start_ms: 2_000,
            end_ms: 3_000,
            is_final: true,
            confidence: Some(0.9),
            source: "local_stt".into(),
        };
        let error = append_transcript_impl(db.conn(), &req, NOW).unwrap_err();
        assert!(error.contains("has not consented"));

        db.conn()
            .execute(
                "UPDATE interview_participants SET consent_transcription = 1, consented_at = ?2
                 WHERE id = ?1",
                params![participant.id, NOW],
            )
            .unwrap();
        let segment = append_transcript_impl(db.conn(), &req, NOW).unwrap();
        assert_eq!(segment.speaker_label, "Candidate A");
    }

    #[test]
    fn transcript_requires_attribution_and_uses_the_participant_label() {
        let db = setup();
        let bundle = create_interview_impl(db.conn(), &request(), NOW).unwrap();
        let participant = &bundle.participants[0];
        db.conn()
            .execute(
                "UPDATE interview_participants SET consent_transcription = 1 WHERE id = ?1",
                params![participant.id],
            )
            .unwrap();
        let mut req = AppendTranscriptRequest {
            session_id: bundle.session.id.clone(),
            participant_id: String::new(),
            speaker_label: "Spoofed label".into(),
            text: "I designed the rollback plan.".into(),
            start_ms: 2_000,
            end_ms: 3_000,
            is_final: true,
            confidence: None,
            source: "manual".into(),
        };
        let error = append_transcript_impl(db.conn(), &req, NOW).unwrap_err();
        assert!(error.contains("attributed participant"));

        req.participant_id = participant.id.clone();
        let segment = append_transcript_impl(db.conn(), &req, NOW).unwrap();
        assert_eq!(segment.speaker_label, participant.display_name);
    }

    #[test]
    fn recommender_targets_vague_claims_and_uncovered_criteria() {
        let db = setup();
        let bundle = create_interview_impl(db.conn(), &request(), NOW).unwrap();
        let participant = &bundle.participants[0];
        db.conn()
            .execute(
                "UPDATE interview_participants SET consent_transcription = 1 WHERE id = ?1",
                params![participant.id],
            )
            .unwrap();
        let segment = append_transcript_impl(
            db.conn(),
            &AppendTranscriptRequest {
                session_id: bundle.session.id.clone(),
                participant_id: participant.id.clone(),
                speaker_label: participant.display_name.clone(),
                text: "We improved the database.".into(),
                start_ms: 5_000,
                end_ms: 6_000,
                is_final: true,
                confidence: None,
                source: "manual".into(),
            },
            NOW,
        )
        .unwrap();
        let suggestions =
            recommend_followups_impl(db.conn(), &bundle.session.id, Some(&segment.id), NOW)
                .unwrap();
        assert_eq!(suggestions.len(), 3);
        assert!(suggestions
            .iter()
            .any(|item| item.question.contains("baseline")));
        assert!(suggestions
            .iter()
            .any(|item| item.question.contains("specifically yours")));
    }

    #[test]
    fn summary_contains_timestamped_attributed_evidence() {
        let db = setup();
        let bundle = create_interview_impl(db.conn(), &request(), NOW).unwrap();
        let participant = &bundle.participants[0];
        db.conn()
            .execute(
                "UPDATE interview_participants SET consent_transcription = 1 WHERE id = ?1",
                params![participant.id],
            )
            .unwrap();
        append_transcript_impl(
            db.conn(),
            &AppendTranscriptRequest {
                session_id: bundle.session.id.clone(),
                participant_id: participant.id.clone(),
                speaker_label: participant.display_name.clone(),
                text: "I tested the failure path before rollout.".into(),
                start_ms: 65_000,
                end_ms: 70_000,
                is_final: true,
                confidence: None,
                source: "manual".into(),
            },
            NOW,
        )
        .unwrap();
        let summary = generate_summary_impl(db.conn(), &bundle.session.id).unwrap();
        assert!(summary.contains("[01:05] Candidate A"));
        assert!(summary.contains("failure path"));
    }
}
