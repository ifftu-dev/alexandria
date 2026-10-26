// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sitting on an organisation's interview panel, from inside Alexandria.
//!
//! An organisation schedules the interview in Alexandria Cloud and gives each
//! panelist a join link. Opening it here makes an ordinary local interview
//! session — the same consent gate, attributed transcript, criteria coverage,
//! follow-up suggestions, private notes and review the interview assistant
//! already has — linked to the Cloud interview. The call itself runs on a
//! tutoring room: the first panelist in creates it and hands Cloud the ticket,
//! everyone else (the candidate included) joins it.
//!
//! What goes back to the organisation is the scorecard the panelist chooses to
//! submit from the review. The transcript and notes stay on this device.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::interview::{
    create_interview_impl, CreateInterviewParticipant, CreateInterviewRequest, InterviewBundle,
};
use crate::clock::now_rfc3339;
use crate::commands::exchange::{client, response_json};
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// What a join link carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelLink {
    pub cloud: String,
    pub interview_id: String,
    pub panelist_id: String,
    pub token: String,
}

/// Read `alexandria://interview/join?cloud=…&id=…&panelist=…&token=…`.
///
/// The Cloud origin comes from the link, so it is held to the same rule as
/// every other Cloud address: https, or http on loopback for local testing.
/// Anything else would send the panelist's token over plaintext.
pub fn parse_panel_link(link: &str) -> Result<PanelLink, String> {
    let url = reqwest::Url::parse(link.trim()).map_err(|_| "not an interview link".to_string())?;
    if url.scheme() != "alexandria"
        || url.host_str() != Some("interview")
        || url.path().trim_end_matches('/') != "/join"
    {
        return Err("not an interview link".into());
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("the interview link has no {name}"))
    };
    let cloud = param("cloud")?.trim_end_matches('/').to_string();
    if !cloud.starts_with("https://") && !super::holder_pull::is_loopback(&cloud) {
        return Err("the interview link points at a service that is not https".into());
    }
    let interview_id = param("id")?;
    let panelist_id = param("panelist")?;
    for id in [&interview_id, &panelist_id] {
        uuid::Uuid::parse_str(id).map_err(|_| "the interview link is damaged".to_string())?;
    }
    let token = param("token")?;
    if token.len() < 32
        || token.len() > 128
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("the interview link is damaged".into());
    }
    Ok(PanelLink {
        cloud,
        interview_id,
        panelist_id,
        token,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelMember {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelCandidate {
    pub did: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelRoom {
    pub ticket: Option<String>,
    pub opened_at: Option<String>,
}

/// The interview as Cloud shows it to one panelist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelInterview {
    pub interview_id: String,
    pub organization: String,
    pub org_domain: Option<String>,
    pub role_label: String,
    pub status: String,
    pub chosen_slot: Option<i64>,
    pub candidate: PanelCandidate,
    pub criteria: Vec<String>,
    pub me: PanelMember,
    pub panel: Vec<PanelMember>,
    pub room: PanelRoom,
    #[serde(default)]
    pub candidate_evidence: Vec<Value>,
    /// Whether the organisation asked the candidate to run the integrity
    /// check, and each result the candidate's app sent. Passed through as
    /// Cloud shows it.
    #[serde(default)]
    pub integrity: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelSession {
    /// The local interview session this panel seat is attached to.
    pub session_id: String,
    pub interview: PanelInterview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rating {
    pub criterion: String,
    /// 1–4, or none when the panelist did not score it.
    pub rating: Option<u8>,
}

async fn panel_request(
    link: &PanelLink,
    method: reqwest::Method,
    suffix: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    let url = format!(
        "{}/api/panel/interviews/{}{suffix}",
        link.cloud, link.interview_id
    );
    let mut request = client()?
        .request(method, url)
        .header("authorization", format!("Panelist {}", link.token));
    if let Some(body) = body {
        request = request.json(body);
    }
    response_json(request.send().await.map_err(|e| e.to_string())?).await
}

async fn fetch(link: &PanelLink) -> Result<PanelInterview, String> {
    let value = panel_request(link, reqwest::Method::GET, "", None).await?;
    let interview: PanelInterview = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if interview.interview_id != link.interview_id || interview.me.id != link.panelist_id {
        return Err("Cloud answered for a different interview".into());
    }
    Ok(interview)
}

fn stored_link(conn: &Connection, session_id: &str) -> Result<PanelLink, String> {
    conn.query_row(
        "SELECT cloud_url, cloud_interview_id, cloud_panelist_id, cloud_panel_token
           FROM interview_sessions WHERE id = ?1",
        params![session_id],
        |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        },
    )
    .optional()
    .map_err(|e| e.to_string())?
    .and_then(|row| match row {
        (Some(cloud), Some(interview_id), Some(panelist_id), Some(token)) => Some(PanelLink {
            cloud,
            interview_id,
            panelist_id,
            token,
        }),
        _ => None,
    })
    .ok_or_else(|| "this interview is not linked to an organisation's panel".to_string())
}

/// The local session for this panel seat, creating it the first time.
pub fn attach_session(
    conn: &Connection,
    link: &PanelLink,
    interview: &PanelInterview,
    now: &str,
) -> Result<String, String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM interview_sessions WHERE cloud_interview_id = ?1 AND cloud_panelist_id = ?2",
            params![link.interview_id, link.panelist_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(id) = existing {
        // A link reopened later may carry a fresh token after a reissue.
        conn.execute(
            "UPDATE interview_sessions SET cloud_panel_token = ?2, cloud_url = ?3 WHERE id = ?1",
            params![id, link.token, link.cloud],
        )
        .map_err(|e| e.to_string())?;
        return Ok(id);
    }

    let candidate = interview
        .candidate
        .display_name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Candidate".to_string());
    let mut participants = vec![CreateInterviewParticipant {
        display_name: interview.me.name.clone(),
        role: "interviewer".into(),
        peer_id: None,
    }];
    participants.extend(
        interview
            .panel
            .iter()
            .filter(|p| p.id != interview.me.id)
            .map(|p| CreateInterviewParticipant {
                display_name: p.name.clone(),
                role: "interviewer".into(),
                peer_id: None,
            }),
    );
    participants.push(CreateInterviewParticipant {
        display_name: candidate.clone(),
        role: "candidate".into(),
        peer_id: None,
    });
    let bundle: InterviewBundle = create_interview_impl(
        conn,
        &CreateInterviewRequest {
            title: format!("{} · {candidate}", interview.role_label),
            objective: Some(format!("{} interview", interview.organization)),
            role_assessment_id: None,
            tutoring_session_id: None,
            duration_minutes: 45,
            retention_days: 30,
            record_audio: false,
            record_video: false,
            // Integrity monitoring belongs on the candidate's device, not a
            // panelist's; the panel seat never starts it.
            sentinel_enabled: false,
            participants,
            criteria: interview.criteria.clone(),
        },
        now,
    )?;
    conn.execute(
        "UPDATE interview_sessions
            SET cloud_url = ?2, cloud_interview_id = ?3, cloud_panelist_id = ?4,
                cloud_panel_token = ?5, cloud_org = ?6
          WHERE id = ?1",
        params![
            bundle.session.id,
            link.cloud,
            link.interview_id,
            link.panelist_id,
            link.token,
            interview.organization,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(bundle.session.id)
}

/// Coverage per criterion label, from the local session.
fn coverage(conn: &Connection, session_id: &str) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT label, status FROM interview_criteria WHERE session_id = ?1 ORDER BY position",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

/// The scorecard body Cloud expects, built from the local coverage and the
/// panelist's ratings.
pub fn scorecard_body(
    coverage: &[(String, String)],
    ratings: &[Rating],
    recommendation: &str,
    notes: &str,
) -> Result<Value, String> {
    if !matches!(recommendation, "strong_yes" | "yes" | "no" | "strong_no") {
        return Err("choose a recommendation".into());
    }
    if notes.chars().count() > 4000 {
        return Err("notes are limited to 4000 characters".into());
    }
    let mut out = Vec::new();
    for (label, status) in coverage {
        let rating = ratings
            .iter()
            .find(|r| &r.criterion == label)
            .and_then(|r| r.rating);
        if rating.is_some_and(|r| !(1..=4).contains(&r)) {
            return Err("ratings run from 1 to 4".into());
        }
        out.push(serde_json::json!({
            "criterion": label,
            "rating": rating,
            "coverage": status,
        }));
    }
    Ok(serde_json::json!({
        "ratings": out,
        "recommendation": recommendation,
        "notes": notes.trim(),
    }))
}

async fn link_for(state: &State<'_, AppState>, session_id: &str) -> Result<PanelLink, String> {
    let id = session_id.to_string();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "interview.panel.link",
            move |db| stored_link(db.conn(), &id),
        )
        .await
}

/// Open a panelist's join link: fetch the interview and attach a local session.
#[tauri::command]
pub async fn interview_panel_open(
    state: State<'_, AppState>,
    link: String,
) -> Result<PanelSession, String> {
    let link = parse_panel_link(&link)?;
    let interview = fetch(&link).await?;
    let (stored, panel) = (link.clone(), interview.clone());
    let session_id = state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "interview.panel.open",
            move |db| attach_session(db.conn(), &stored, &panel, &now_rfc3339()),
        )
        .await?;
    Ok(PanelSession {
        session_id,
        interview,
    })
}

/// The panel interview behind a local session, fresh from Cloud.
#[tauri::command]
pub async fn interview_panel_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<PanelInterview, String> {
    fetch(&link_for(&state, &session_id).await?).await
}

/// Hand Cloud the room ticket so the rest of the panel and the candidate can join.
#[tauri::command]
pub async fn interview_panel_set_room(
    state: State<'_, AppState>,
    session_id: String,
    ticket: String,
) -> Result<PanelInterview, String> {
    if ticket.trim().is_empty() || ticket.len() > 4096 {
        return Err("not a room ticket".into());
    }
    let link = link_for(&state, &session_id).await?;
    let value = panel_request(
        &link,
        reqwest::Method::POST,
        "/room",
        Some(&serde_json::json!({ "ticket": ticket })),
    )
    .await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// Send this panelist's scorecard to the organisation.
#[tauri::command]
pub async fn interview_panel_scorecard(
    state: State<'_, AppState>,
    session_id: String,
    ratings: Vec<Rating>,
    recommendation: String,
    notes: String,
) -> Result<Value, String> {
    let link = link_for(&state, &session_id).await?;
    let id = session_id.clone();
    let covered = state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "interview.panel.coverage",
            move |db| coverage(db.conn(), &id),
        )
        .await?;
    let body = scorecard_body(&covered, &ratings, &recommendation, &notes)?;
    panel_request(&link, reqwest::Method::POST, "/scorecard", Some(&body)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    const TOKEN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ";
    const ID: &str = "6f1b2640-0000-4000-8000-000000000001";
    const PID: &str = "6f1b2640-0000-4000-8000-000000000002";

    fn link(cloud: &str) -> String {
        format!(
            "alexandria://interview/join?cloud={}&id={ID}&panelist={PID}&token={TOKEN}",
            urlencoding(cloud)
        )
    }

    fn urlencoding(s: &str) -> String {
        s.replace(':', "%3A").replace('/', "%2F")
    }

    fn interview() -> PanelInterview {
        PanelInterview {
            interview_id: ID.into(),
            organization: "IFFTU".into(),
            org_domain: Some("alexandria.ifftu.dev".into()),
            role_label: "Engineering Manager".into(),
            status: "accepted".into(),
            chosen_slot: Some(1_790_000_000),
            candidate: PanelCandidate {
                did: "did:key:z6Mk".into(),
                display_name: Some("Demo Learner".into()),
            },
            criteria: vec!["Communication".into(), "Technical depth".into()],
            me: PanelMember {
                id: PID.into(),
                name: "Asha".into(),
            },
            panel: vec![
                PanelMember {
                    id: PID.into(),
                    name: "Asha".into(),
                },
                PanelMember {
                    id: "6f1b2640-0000-4000-8000-000000000003".into(),
                    name: "Ben".into(),
                },
            ],
            room: PanelRoom {
                ticket: None,
                opened_at: None,
            },
            candidate_evidence: Vec::new(),
            integrity: None,
        }
    }

    #[test]
    fn join_links_are_parsed_and_held_to_https() {
        let parsed = parse_panel_link(&link("https://cloud.example")).unwrap();
        assert_eq!(parsed.cloud, "https://cloud.example");
        assert_eq!(parsed.interview_id, ID);
        assert_eq!(parsed.token, TOKEN);
        assert!(parse_panel_link(&link("http://127.0.0.1:8787")).is_ok());
        assert!(parse_panel_link(&link("http://cloud.example")).is_err());
        assert!(parse_panel_link("alexandria://course/x").is_err());
        assert!(parse_panel_link(&link("https://cloud.example").replace(TOKEN, "short")).is_err());
        assert!(
            parse_panel_link(&link("https://cloud.example").replace(ID, "not-a-uuid")).is_err()
        );
    }

    #[test]
    fn opening_a_link_twice_reuses_the_same_session() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let parsed = parse_panel_link(&link("https://cloud.example")).unwrap();
        let first =
            attach_session(db.conn(), &parsed, &interview(), "2026-10-12T00:00:00Z").unwrap();
        let again =
            attach_session(db.conn(), &parsed, &interview(), "2026-10-12T00:05:00Z").unwrap();
        assert_eq!(first, again);
        let bundle = super::super::interview::get_interview_impl(db.conn(), &first)
            .unwrap()
            .unwrap();
        assert_eq!(bundle.criteria.len(), 2);
        // Me, the other panelist, and the candidate.
        assert_eq!(bundle.participants.len(), 3);
        assert!(!bundle.session.sentinel_enabled);
        assert_eq!(stored_link(db.conn(), &first).unwrap(), parsed);
    }

    #[test]
    fn a_scorecard_carries_local_coverage_and_checks_its_fields() {
        let coverage = vec![
            ("Communication".to_string(), "covered".to_string()),
            ("Technical depth".to_string(), "partial".to_string()),
        ];
        let ratings = vec![Rating {
            criterion: "Communication".into(),
            rating: Some(4),
        }];
        let body = scorecard_body(&coverage, &ratings, "yes", " Clear. ").unwrap();
        assert_eq!(body["ratings"][0]["rating"], 4);
        assert_eq!(body["ratings"][1]["rating"], Value::Null);
        assert_eq!(body["ratings"][1]["coverage"], "partial");
        assert_eq!(body["notes"], "Clear.");
        assert!(scorecard_body(&coverage, &ratings, "maybe", "").is_err());
        let wild = vec![Rating {
            criterion: "Communication".into(),
            rating: Some(9),
        }];
        assert!(scorecard_body(&coverage, &wild, "yes", "").is_err());
    }
}
