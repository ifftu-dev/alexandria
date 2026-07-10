//! Bootstrap a learner's current skill graph from uploaded documents.
//!
//! A new user uploads a resume / academic transcript / credential; the text is
//! matched on-device against the taxonomy (reusing the JD parser) and the user
//! confirms which skills to claim. Each confirmed skill becomes a *self-issued*
//! `SelfAssertion` VC carrying a provenance tier — a bare resume yields
//! `DocumentBacked`, an accredited-institution document yields
//! `AccreditedDocument` — so the aggregation confidence (Phase A) ranks
//! accredited evidence above self-made resumes. The uploaded file's content
//! hash is stored in `evidence_refs`.
//!
//! These are low-confidence starting points; dynamic assessments (Phase D)
//! raise them by issuing higher-weight `AssessmentCredential`s.

use rusqlite::Connection;
use serde::Deserialize;
use tauri::State;

use crate::commands::credentials::load_issuer_key;
use crate::commands::goal_templates::SkillSuggestion;
use crate::domain::vc::{Claim, CredentialType, ProvenanceTier, SkillClaim};
use crate::goals::jd_parser::{extract_skills, SkillEntry};
use crate::AppState;

/// What kind of document the skills were extracted from — sets the provenance
/// tier of the resulting claims.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocType {
    /// Self-authored resume/CV — a document, but self-made.
    Resume,
    /// Academic transcript from an accredited school/university.
    Transcript,
    /// Other accredited-institution credential document.
    AccreditedCredential,
}

impl DocType {
    fn provenance(self) -> ProvenanceTier {
        match self {
            DocType::Resume => ProvenanceTier::DocumentBacked,
            DocType::Transcript | DocType::AccreditedCredential => {
                ProvenanceTier::AccreditedDocument
            }
        }
    }
}

/// Self-asserted claims from an uploaded document start at a modest,
/// explicitly-unverified score. The provenance tier + SelfAssertion type
/// weight keep the aggregated confidence low until an assessment verifies it.
const BOOTSTRAP_SCORE: f64 = 0.5;
const BOOTSTRAP_LEVEL: u8 = 2;

fn load_skill_entries(conn: &Connection) -> Result<Vec<SkillEntry>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name, synonyms FROM skills")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name, syn) = row.map_err(|e| e.to_string())?;
        let synonyms = syn
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        out.push(SkillEntry { id, name, synonyms });
    }
    Ok(out)
}

/// Pure: match document text against the taxonomy, returning suggestions with
/// display names. Shared shape with the JD resolver's suggestions.
fn suggest_from_text(conn: &Connection, text: &str) -> Result<Vec<SkillSuggestion>, String> {
    let entries = load_skill_entries(conn)?;
    let names: std::collections::HashMap<&str, &str> = entries
        .iter()
        .map(|e| (e.id.as_str(), e.name.as_str()))
        .collect();
    Ok(extract_skills(text, &entries)
        .into_iter()
        .map(|c| SkillSuggestion {
            name: names
                .get(c.skill_id.as_str())
                .copied()
                .unwrap_or("")
                .to_string(),
            skill_id: c.skill_id,
            score: c.score,
            matched: c.matched,
        })
        .collect())
}

// ---- Tauri commands -----------------------------------------------------

/// Extract plain text from an uploaded document's bytes for skill matching.
/// PDFs are parsed on-device; other bytes are treated as UTF-8 text. Operates
/// only on bytes the caller already holds — it is not a file-read primitive.
#[tauri::command]
pub async fn bootstrap_extract_text(data: Vec<u8>) -> Result<String, String> {
    if data.starts_with(b"%PDF") {
        // Text-based PDFs extract cleanly; scanned/image PDFs yield little —
        // the user can still paste in that case.
        pdf_extract::extract_text_from_mem(&data)
            .map_err(|e| format!("couldn't read text from this PDF: {e}"))
    } else {
        Ok(String::from_utf8_lossy(&data).into_owned())
    }
}

/// Extract candidate skills from document text (the frontend supplies the text
/// — pasted, or read from a `.txt`/`.md` file). Suggestions only; nothing is
/// claimed until `bootstrap_confirm`.
#[tauri::command]
pub async fn bootstrap_extract(
    state: State<'_, AppState>,
    text: String,
) -> Result<Vec<SkillSuggestion>, String> {
    let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    suggest_from_text(db.conn(), &text)
}

/// Issue self-asserted skill VCs for the confirmed skills, tagged with the
/// document's provenance tier and the uploaded file's content hash, then
/// recompute derived skill states. Returns the number of skills claimed.
#[tauri::command]
pub async fn bootstrap_confirm(
    state: State<'_, AppState>,
    skill_ids: Vec<String>,
    doc_type: DocType,
    content_hash: Option<String>,
) -> Result<u32, String> {
    if skill_ids.is_empty() {
        return Ok(0);
    }
    let (signing_key, issuer_did) = load_issuer_key(&state).await?;
    let now = crate::commands::credentials::now_rfc3339();
    let provenance = doc_type.provenance();
    let evidence_refs: Vec<String> = content_hash.into_iter().collect();

    let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let mut claimed = 0u32;
    for skill_id in skill_ids {
        let claim = SkillClaim {
            skill_id,
            level: BOOTSTRAP_LEVEL,
            score: BOOTSTRAP_SCORE,
            evidence_refs: evidence_refs.clone(),
            rubric_version: None,
            assessment_method: Some("document_bootstrap".into()),
            provenance: Some(provenance),
        };
        let req = crate::commands::credentials::IssueCredentialRequest {
            credential_type: CredentialType::SelfAssertion,
            subject: issuer_did.clone(), // self-issued
            claim: Claim::Skill(claim),
            evidence_refs: evidence_refs.clone(),
            expiration_date: None,
            supersedes: None,
            integrity_session_id: None,
            integrity_policy: None,
        };
        match crate::commands::credentials::issue_credential_impl(
            conn,
            &signing_key,
            &issuer_did,
            &req,
            &now,
        ) {
            Ok(_) => claimed += 1,
            Err(e) => log::warn!("bootstrap: skipping skill claim: {e}"),
        }
    }

    // Refresh derived confidence so the new (low-tier) skills appear.
    let _ = crate::commands::aggregation::recompute_all_impl(conn, &now);
    Ok(claimed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE skills (id TEXT PRIMARY KEY, name TEXT, synonyms TEXT);
             INSERT INTO skills VALUES ('skill_js','JavaScript','js,node.js');
             INSERT INTO skills VALUES ('skill_sql','SQL','postgres');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn doc_type_maps_to_provenance_tier() {
        assert_eq!(DocType::Resume.provenance(), ProvenanceTier::DocumentBacked);
        assert_eq!(
            DocType::Transcript.provenance(),
            ProvenanceTier::AccreditedDocument
        );
        assert!(DocType::Transcript.provenance() > DocType::Resume.provenance());
    }

    #[test]
    fn suggests_skills_from_document_text() {
        let conn = setup();
        let s = suggest_from_text(&conn, "Built services in JavaScript and Postgres.").unwrap();
        let ids: Vec<_> = s.iter().map(|x| x.skill_id.as_str()).collect();
        assert!(ids.contains(&"skill_js") && ids.contains(&"skill_sql"));
        assert!(s.iter().any(|x| x.name == "JavaScript"));
    }
}
