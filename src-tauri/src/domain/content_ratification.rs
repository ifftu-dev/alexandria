//! DAO ratification for community-contributed content — goal templates and
//! assessment question banks — mirroring the taxonomy flow
//! (`evidence::taxonomy`): **propose → vote → publish → apply**.
//!
//! - `propose` records a `governance_proposal` (with a kind-specific category)
//!   whose `content_cid` column holds the serialized change document, and
//!   allocates the next version number.
//! - Voting is the existing governance machinery (`resolve_proposal` flips the
//!   proposal to `approved`).
//! - `publish` (post-approval) hashes the change doc into a content CID,
//!   applies it into the local content tables, records a signed version row,
//!   and stamps the proposal.
//! - `apply_doc` upserts a received version document into the local tables
//!   (used by `publish` and by the gossip inbound handler) — idempotent.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::crypto::hash::entity_id;

/// The two community-content kinds ratified through this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    GoalTemplate,
    QuestionBank,
}

impl ContentKind {
    pub fn category(self) -> &'static str {
        match self {
            ContentKind::GoalTemplate => "goal_template_change",
            ContentKind::QuestionBank => "question_bank_change",
        }
    }
    pub fn versions_table(self) -> &'static str {
        match self {
            ContentKind::GoalTemplate => "goal_template_versions",
            ContentKind::QuestionBank => "question_bank_versions",
        }
    }
    pub fn from_category(cat: &str) -> Option<Self> {
        match cat {
            "goal_template_change" => Some(ContentKind::GoalTemplate),
            "question_bank_change" => Some(ContentKind::QuestionBank),
            _ => None,
        }
    }
}

// ---- change documents ---------------------------------------------------

/// A goal-template change: rows to upsert (ratified=1 on apply).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalTemplateDoc {
    pub templates: Vec<GoalTemplateRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalTemplateRow {
    pub id: String,
    pub kind: String,
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub board: Option<String>,
    #[serde(default)]
    pub grade: Option<String>,
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub taxonomy_version: Option<String>,
}

/// A question-bank change: banks + their questions (upserted on apply).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuestionBankDoc {
    pub banks: Vec<BankRow>,
    pub questions: Vec<BankQuestionRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankRow {
    pub id: String,
    pub skill_id: String,
    pub label: String,
    #[serde(default = "default_threshold")]
    pub pass_threshold: f64,
    #[serde(default = "default_draw")]
    pub draw_count: i64,
    #[serde(default)]
    pub taxonomy_version: Option<String>,
}
fn default_threshold() -> f64 {
    0.7
}
fn default_draw() -> i64 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankQuestionRow {
    pub id: String,
    pub bank_id: String,
    pub prompt: String,
    pub options: Vec<String>,
    pub correct_indices: Vec<usize>,
    #[serde(default = "default_difficulty")]
    pub difficulty: i64,
    #[serde(default = "default_points")]
    pub points: f64,
}
fn default_difficulty() -> i64 {
    2
}
fn default_points() -> f64 {
    1.0
}

/// A published, ratified version document (what travels over gossip).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDoc {
    pub kind: String, // category
    pub version: i64,
    pub previous_cid: Option<String>,
    pub ratified_by: Vec<String>,
    pub ratified_at: String,
    pub signature: String,
    pub taxonomy_version: Option<String>,
    /// The kind-specific change document (GoalTemplateDoc | QuestionBankDoc).
    pub content: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishResult {
    pub version: i64,
    pub content_cid: String,
    pub rows_applied: usize,
    /// The proposal's category (`goal_template_change` | `question_bank_change`)
    /// so callers know which gossip topic to announce on.
    pub category: String,
    /// The serialized signed [`VersionDoc`] to broadcast to peers.
    pub doc_json: String,
}

// ---- propose ------------------------------------------------------------

pub fn propose(
    conn: &Connection,
    kind: ContentKind,
    dao_id: &str,
    title: &str,
    description: Option<&str>,
    change_json: &str,
    proposer: &str,
) -> Result<String, String> {
    let dao_status: String = conn
        .query_row(
            "SELECT status FROM governance_daos WHERE id = ?1",
            params![dao_id],
            |r| r.get(0),
        )
        .map_err(|e| format!("DAO not found: {e}"))?;
    if dao_status != "active" {
        return Err(format!("DAO is not active (status: {dao_status})"));
    }
    // Validate the change doc parses for this kind.
    validate_change(kind, change_json)?;

    let next_version: i64 = conn
        .query_row(
            &format!(
                "SELECT COALESCE(MAX(version), 0) + 1 FROM {}",
                kind.versions_table()
            ),
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    let proposal_id = entity_id(&[dao_id, kind.category(), title, proposer]);
    conn.execute(
        "INSERT INTO governance_proposals \
         (id, dao_id, title, description, category, status, proposer, content_cid, taxonomy_version) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'draft', ?6, ?7, ?8)",
        params![
            proposal_id,
            dao_id,
            title,
            description,
            kind.category(),
            proposer,
            change_json,
            next_version,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(proposal_id)
}

fn validate_change(kind: ContentKind, change_json: &str) -> Result<(), String> {
    match kind {
        ContentKind::GoalTemplate => {
            serde_json::from_str::<GoalTemplateDoc>(change_json)
                .map_err(|e| format!("invalid goal-template change: {e}"))?;
        }
        ContentKind::QuestionBank => {
            serde_json::from_str::<QuestionBankDoc>(change_json)
                .map_err(|e| format!("invalid question-bank change: {e}"))?;
        }
    }
    Ok(())
}

// ---- publish ------------------------------------------------------------

pub fn publish(
    conn: &Connection,
    proposal_id: &str,
    ratified_by: &[String],
    signature: &str,
) -> Result<PublishResult, String> {
    let (category, status, change_json, version): (String, String, Option<String>, Option<i64>) =
        conn.query_row(
            "SELECT category, status, content_cid, taxonomy_version \
             FROM governance_proposals WHERE id = ?1",
            params![proposal_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| format!("proposal not found: {e}"))?;

    let kind = ContentKind::from_category(&category)
        .ok_or_else(|| format!("proposal category '{category}' is not community content"))?;
    if status != "approved" {
        return Err(format!(
            "proposal must be approved to publish (status: {status})"
        ));
    }
    let change_json = change_json.ok_or("proposal has no change document")?;
    let version = version.unwrap_or(1);

    let previous_cid: Option<String> = conn
        .query_row(
            &format!(
                "SELECT content_cid FROM {} WHERE version = ?1",
                kind.versions_table()
            ),
            params![version - 1],
            |r| r.get(0),
        )
        .ok();

    let now = chrono::Utc::now().to_rfc3339();
    let doc = VersionDoc {
        kind: category.clone(),
        version,
        previous_cid: previous_cid.clone(),
        ratified_by: ratified_by.to_vec(),
        ratified_at: now.clone(),
        signature: signature.into(),
        taxonomy_version: None,
        content: serde_json::from_str(&change_json).map_err(|e| e.to_string())?,
    };
    let doc_json = serde_json::to_string(&doc).map_err(|e| e.to_string())?;
    let content_cid = hex::encode(crate::crypto::hash::blake2b_256(doc_json.as_bytes()));

    let rows_applied = apply_change(conn, kind, &change_json)?;

    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO {} \
             (version, content_cid, previous_cid, ratified_by, signature, published_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            kind.versions_table()
        ),
        params![
            version,
            content_cid,
            previous_cid,
            serde_json::to_string(ratified_by).unwrap_or_default(),
            signature,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE governance_proposals SET content_cid = ?1 WHERE id = ?2",
        params![content_cid, proposal_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(PublishResult {
        version,
        content_cid,
        rows_applied,
        category,
        doc_json,
    })
}

// ---- apply --------------------------------------------------------------

/// Apply a received/ratified [`VersionDoc`] (from gossip) into local tables +
/// record the version. Idempotent — INSERT OR REPLACE throughout.
pub fn apply_version_doc(conn: &Connection, doc: &VersionDoc) -> Result<usize, String> {
    let kind = ContentKind::from_category(&doc.kind)
        .ok_or_else(|| format!("unknown content kind '{}'", doc.kind))?;
    let change_json = serde_json::to_string(&doc.content).map_err(|e| e.to_string())?;
    let doc_json = serde_json::to_string(doc).map_err(|e| e.to_string())?;
    let content_cid = hex::encode(crate::crypto::hash::blake2b_256(doc_json.as_bytes()));
    let rows = apply_change(conn, kind, &change_json)?;
    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO {} \
             (version, content_cid, previous_cid, ratified_by, signature, published_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            kind.versions_table()
        ),
        params![
            doc.version,
            content_cid,
            doc.previous_cid,
            serde_json::to_string(&doc.ratified_by).unwrap_or_default(),
            doc.signature,
            doc.ratified_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn apply_change(conn: &Connection, kind: ContentKind, change_json: &str) -> Result<usize, String> {
    match kind {
        ContentKind::GoalTemplate => {
            let doc: GoalTemplateDoc =
                serde_json::from_str(change_json).map_err(|e| e.to_string())?;
            let mut n = 0;
            for t in &doc.templates {
                conn.execute(
                    "INSERT OR REPLACE INTO goal_templates \
                     (id, kind, key, label, board, grade, skill_ids, taxonomy_version, ratified, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, datetime('now'))",
                    params![
                        t.id, t.kind, t.key, t.label, t.board, t.grade,
                        serde_json::to_string(&t.skill_ids).unwrap_or_default(),
                        t.taxonomy_version,
                    ],
                )
                .map_err(|e| e.to_string())?;
                n += 1;
            }
            Ok(n)
        }
        ContentKind::QuestionBank => {
            let doc: QuestionBankDoc =
                serde_json::from_str(change_json).map_err(|e| e.to_string())?;
            let mut n = 0;
            for b in &doc.banks {
                conn.execute(
                    "INSERT OR REPLACE INTO question_banks \
                     (id, skill_id, label, pass_threshold, draw_count, taxonomy_version, ratified) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
                    params![
                        b.id,
                        b.skill_id,
                        b.label,
                        b.pass_threshold,
                        b.draw_count,
                        b.taxonomy_version
                    ],
                )
                .map_err(|e| e.to_string())?;
                n += 1;
            }
            for q in &doc.questions {
                conn.execute(
                    "INSERT OR REPLACE INTO bank_questions \
                     (id, bank_id, prompt, options, correct_indices, difficulty, points) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        q.id,
                        q.bank_id,
                        q.prompt,
                        serde_json::to_string(&q.options).unwrap_or_default(),
                        serde_json::to_string(&q.correct_indices).unwrap_or_default(),
                        q.difficulty,
                        q.points,
                    ],
                )
                .map_err(|e| e.to_string())?;
                n += 1;
            }
            Ok(n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE governance_daos (id TEXT PRIMARY KEY, status TEXT);
             CREATE TABLE governance_proposals (id TEXT PRIMARY KEY, dao_id TEXT, title TEXT,
                description TEXT, category TEXT, status TEXT, proposer TEXT, content_cid TEXT,
                taxonomy_version INTEGER, votes_for INTEGER DEFAULT 0, votes_against INTEGER DEFAULT 0);
             CREATE TABLE goal_templates (id TEXT PRIMARY KEY, kind TEXT, key TEXT, label TEXT,
                board TEXT, grade TEXT, skill_ids TEXT, taxonomy_version TEXT, ratified INTEGER, updated_at TEXT);
             CREATE UNIQUE INDEX idx_gt_key ON goal_templates(kind, key);
             CREATE TABLE goal_template_versions (version INTEGER PRIMARY KEY, content_cid TEXT,
                previous_cid TEXT, ratified_by TEXT, signature TEXT, published_at TEXT);
             CREATE TABLE question_banks (id TEXT PRIMARY KEY, skill_id TEXT, label TEXT,
                pass_threshold REAL, draw_count INTEGER, taxonomy_version TEXT, ratified INTEGER);
             CREATE TABLE bank_questions (id TEXT PRIMARY KEY, bank_id TEXT, prompt TEXT,
                options TEXT, correct_indices TEXT, difficulty INTEGER, points REAL);
             CREATE TABLE question_bank_versions (version INTEGER PRIMARY KEY, content_cid TEXT,
                previous_cid TEXT, ratified_by TEXT, signature TEXT, published_at TEXT);
             INSERT INTO governance_daos VALUES ('dao1','active');",
        )
        .unwrap();
        conn
    }

    fn approve(conn: &Connection, id: &str) {
        conn.execute(
            "UPDATE governance_proposals SET status='approved' WHERE id=?1",
            params![id],
        )
        .unwrap();
    }

    #[test]
    fn full_goal_template_ratification_cycle() {
        let conn = setup();
        let change = r#"{"templates":[{"id":"gt_x","kind":"job_role","key":"data_scientist",
            "label":"Data Scientist","skill_ids":["skill_stats","skill_ml"]}]}"#;
        let pid = propose(
            &conn,
            ContentKind::GoalTemplate,
            "dao1",
            "Add DS role",
            None,
            change,
            "stakeX",
        )
        .unwrap();
        // not yet applied (still draft)
        let cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM goal_templates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt, 0);
        // publish before approval fails
        assert!(publish(&conn, &pid, &["m1".into()], "sig").is_err());
        approve(&conn, &pid);
        let res = publish(&conn, &pid, &["m1".into(), "m2".into()], "sig").unwrap();
        assert_eq!(res.rows_applied, 1);
        assert_eq!(res.version, 1);
        // applied + ratified
        let (key, ratified): (String, i64) = conn
            .query_row(
                "SELECT key, ratified FROM goal_templates WHERE id='gt_x'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(key, "data_scientist");
        assert_eq!(ratified, 1);
        // version recorded
        let v: i64 = conn
            .query_row("SELECT COUNT(*) FROM goal_template_versions", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(v, 1);
    }

    #[test]
    fn apply_version_doc_is_idempotent_for_question_banks() {
        let conn = setup();
        let content: serde_json::Value = serde_json::from_str(
            r#"{"banks":[{"id":"qb_x","skill_id":"skill_js","label":"JS","pass_threshold":0.7,"draw_count":3}],
                "questions":[{"id":"q1","bank_id":"qb_x","prompt":"?","options":["a","b"],"correct_indices":[0],"difficulty":1,"points":1.0}]}"#,
        ).unwrap();
        let doc = VersionDoc {
            kind: "question_bank_change".into(),
            version: 1,
            previous_cid: None,
            ratified_by: vec!["m1".into()],
            ratified_at: "2026-01-01T00:00:00Z".into(),
            signature: "sig".into(),
            taxonomy_version: None,
            content,
        };
        apply_version_doc(&conn, &doc).unwrap();
        apply_version_doc(&conn, &doc).unwrap(); // idempotent
        let banks: i64 = conn
            .query_row("SELECT COUNT(*) FROM question_banks", [], |r| r.get(0))
            .unwrap();
        let qs: i64 = conn
            .query_row("SELECT COUNT(*) FROM bank_questions", [], |r| r.get(0))
            .unwrap();
        assert_eq!((banks, qs), (1, 1));
    }
}
