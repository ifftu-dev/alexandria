//! Goal templates + goal → ideal-skill-graph resolution.
//!
//! Curated exam / curriculum / job-role templates map a goal to a set of
//! target skill IDs (DAO-ratified, seeded genesis set works offline). A
//! free-text job description is matched on-device against the taxonomy by
//! [`crate::goals::jd_parser`] and returned as *suggestions* the user
//! confirms. The resolved skill IDs then feed the existing learning-path
//! pipeline (`commands::graph::compute_path`) via the `learner.targets`
//! setting — this module only produces the IDs, it does not persist goals.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::goals::jd_parser::{extract_skills, SkillEntry};
use crate::AppState;

/// A curated goal → skill map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalTemplate {
    pub id: String,
    pub kind: String, // exam | curriculum | job_role
    pub key: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    pub skill_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taxonomy_version: Option<String>,
    pub ratified: bool,
}

/// The goal the learner is setting. Untagged variants keep the frontend
/// payload small (`{ kind: "exam", key: "jee_main" }`, etc.).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GoalInput {
    Exam { key: String },
    Curriculum { board: String, grade: String },
    JobRole { key: String },
    JdText { text: String },
    JdLink { url: String },
}

/// One extracted candidate skill for the confirm-suggestions step.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SkillSuggestion {
    pub skill_id: String,
    pub name: String,
    pub score: f64,
    pub matched: String,
}

/// Result of resolving a goal. For curated templates `goal_skill_ids` is the
/// authoritative target set. For a parsed JD it is empty and `suggestions`
/// carries the on-device matches — the user picks which become the goal.
#[derive(Debug, Clone, Serialize)]
pub struct GoalResolution {
    pub label: String,
    pub goal_skill_ids: Vec<String>,
    pub suggestions: Vec<SkillSuggestion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taxonomy_version: Option<String>,
    /// `template` when resolved from a curated map, `jd_parsed` for a JD.
    pub resolution_provenance: String,
}

// ---- pure helpers (conn-based, unit-testable) ---------------------------

fn parse_skill_ids(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

const TEMPLATE_COLS: &str =
    "id, kind, key, label, board, grade, skill_ids, taxonomy_version, ratified";

/// Build a `GoalTemplate` from a row selected with [`TEMPLATE_COLS`].
fn map_template_row(r: &rusqlite::Row) -> rusqlite::Result<GoalTemplate> {
    let skill_ids_json: String = r.get(6)?;
    Ok(GoalTemplate {
        id: r.get(0)?,
        kind: r.get(1)?,
        key: r.get(2)?,
        label: r.get(3)?,
        board: r.get(4)?,
        grade: r.get(5)?,
        skill_ids: parse_skill_ids(&skill_ids_json),
        taxonomy_version: r.get(7)?,
        ratified: r.get::<_, i64>(8)? != 0,
    })
}

pub fn list_goal_templates_impl(
    conn: &Connection,
    kind: Option<&str>,
) -> Result<Vec<GoalTemplate>, String> {
    let mut sql = format!("SELECT {TEMPLATE_COLS} FROM goal_templates");
    if kind.is_some() {
        sql.push_str(" WHERE kind = ?1");
    }
    sql.push_str(" ORDER BY label");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = if let Some(k) = kind {
        stmt.query_map(params![k], map_template_row)
    } else {
        stmt.query_map([], map_template_row)
    }
    .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

fn get_template_by_key(
    conn: &Connection,
    kind: &str,
    key: &str,
) -> Result<Option<GoalTemplate>, String> {
    let sql = format!("SELECT {TEMPLATE_COLS} FROM goal_templates WHERE kind = ?1 AND key = ?2");
    conn.query_row(&sql, params![kind, key], map_template_row)
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other.to_string()),
        })
}

/// Load every skill's matchable surface for on-device JD/document matching.
fn load_skill_entries(conn: &Connection) -> Result<Vec<SkillEntry>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name, synonyms FROM skills")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let id: String = r.get(0)?;
            let name: String = r.get(1)?;
            let syn: Option<String> = r.get(2)?;
            Ok((id, name, syn))
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

fn parse_jd_text(conn: &Connection, text: &str) -> Result<GoalResolution, String> {
    let entries = load_skill_entries(conn)?;
    let by_name: std::collections::HashMap<&str, &str> = entries
        .iter()
        .map(|e| (e.id.as_str(), e.name.as_str()))
        .collect();
    let suggestions = extract_skills(text, &entries)
        .into_iter()
        .map(|c| SkillSuggestion {
            name: by_name
                .get(c.skill_id.as_str())
                .copied()
                .unwrap_or("")
                .to_string(),
            skill_id: c.skill_id,
            score: c.score,
            matched: c.matched,
        })
        .collect();
    Ok(GoalResolution {
        label: "Custom role (from job description)".into(),
        goal_skill_ids: Vec::new(), // filled by the user's confirmation
        suggestions,
        taxonomy_version: None,
        resolution_provenance: "jd_parsed".into(),
    })
}

fn resolve_template(conn: &Connection, kind: &str, key: &str) -> Result<GoalResolution, String> {
    let tpl = get_template_by_key(conn, kind, key)?
        .ok_or_else(|| format!("no {kind} template for '{key}'"))?;
    Ok(GoalResolution {
        label: tpl.label,
        goal_skill_ids: tpl.skill_ids,
        suggestions: Vec::new(),
        taxonomy_version: tpl.taxonomy_version,
        resolution_provenance: "template".into(),
    })
}

/// Strip HTML tags to plain text for JD-link parsing (best-effort; the parser
/// only cares about word tokens, so exact structure doesn't matter).
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

// ---- Tauri commands -----------------------------------------------------

#[tauri::command]
pub async fn list_goal_templates(
    state: State<'_, AppState>,
    kind: Option<String>,
) -> Result<Vec<GoalTemplate>, String> {
    let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    list_goal_templates_impl(db.conn(), kind.as_deref())
}

#[tauri::command]
pub async fn get_goal_template(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<GoalTemplate>, String> {
    let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    let sql = format!("SELECT {TEMPLATE_COLS} FROM goal_templates WHERE id = ?1");
    db.conn()
        .query_row(&sql, params![id], map_template_row)
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other.to_string()),
        })
}

#[tauri::command]
pub async fn resolve_goal(
    state: State<'_, AppState>,
    input: GoalInput,
) -> Result<GoalResolution, String> {
    // JD-link fetch happens outside the DB lock (async network I/O).
    if let GoalInput::JdLink { url } = &input {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err("job-description link must be an http(s) URL".into());
        }
        let body = reqwest::get(url)
            .await
            .map_err(|e| format!("fetch JD: {e}"))?
            .text()
            .await
            .map_err(|e| format!("read JD: {e}"))?;
        let text = strip_html(&body);
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        return parse_jd_text(db.conn(), &text);
    }

    let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();
    match input {
        GoalInput::Exam { key } => resolve_template(conn, "exam", &key),
        GoalInput::JobRole { key } => resolve_template(conn, "job_role", &key),
        GoalInput::Curriculum { board, grade } => {
            // Curriculum templates key on `<board>.grade<grade>` (lowercased).
            let key = format!("{}.grade{}", board.to_lowercase(), grade);
            resolve_template(conn, "curriculum", &key)
        }
        GoalInput::JdText { text } => parse_jd_text(conn, &text),
        GoalInput::JdLink { .. } => unreachable!("handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE skills (id TEXT PRIMARY KEY, name TEXT, synonyms TEXT);
             CREATE TABLE goal_templates (id TEXT PRIMARY KEY, kind TEXT, key TEXT, label TEXT,
                board TEXT, grade TEXT, skill_ids TEXT, taxonomy_version TEXT, ratified INTEGER);
             INSERT INTO skills VALUES ('skill_js','JavaScript','js,node.js');
             INSERT INTO skills VALUES ('skill_sql','SQL','postgres');
             INSERT INTO skills VALUES ('skill_algo','Algorithms',NULL);
             INSERT INTO goal_templates VALUES ('t1','curriculum','cbse.grade10','CBSE — Grade 10',
                'CBSE','10','[\"skill_algo\",\"skill_js\"]','tax_v1',1);
             INSERT INTO goal_templates VALUES ('t2','job_role','engineering_manager',
                'Engineering Manager',NULL,NULL,'[\"skill_algo\"]','tax_v1',1);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn resolves_curriculum_template_to_skill_ids() {
        let conn = setup();
        let r = resolve_template(&conn, "curriculum", "cbse.grade10").unwrap();
        assert_eq!(r.goal_skill_ids, vec!["skill_algo", "skill_js"]);
        assert_eq!(r.resolution_provenance, "template");
        assert_eq!(r.taxonomy_version.as_deref(), Some("tax_v1"));
        assert!(r.suggestions.is_empty());
    }

    #[test]
    fn missing_template_errors() {
        let conn = setup();
        assert!(resolve_template(&conn, "exam", "nope").is_err());
    }

    #[test]
    fn jd_text_returns_suggestions_not_committed_goals() {
        let conn = setup();
        let r = parse_jd_text(&conn, "We use JavaScript and Postgres heavily.").unwrap();
        assert_eq!(r.resolution_provenance, "jd_parsed");
        assert!(
            r.goal_skill_ids.is_empty(),
            "JD goals must be user-confirmed"
        );
        let ids: Vec<_> = r.suggestions.iter().map(|s| s.skill_id.as_str()).collect();
        assert!(ids.contains(&"skill_js") && ids.contains(&"skill_sql"));
        // suggestions carry the display name for the confirm UI
        assert!(r.suggestions.iter().any(|s| s.name == "JavaScript"));
    }

    #[test]
    fn list_filters_by_kind() {
        let conn = setup();
        let jobs = list_goal_templates_impl(&conn, Some("job_role")).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].key, "engineering_manager");
        assert_eq!(list_goal_templates_impl(&conn, None).unwrap().len(), 2);
    }

    #[test]
    fn strip_html_extracts_text() {
        assert_eq!(
            strip_html("<p>Hello <b>world</b></p>").trim(),
            "Hello world"
        );
    }
}
