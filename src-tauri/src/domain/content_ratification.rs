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
use crate::domain::bloom::BloomLevel;

/// Minimum proficiency to author an assessment for a skill: a credential in
/// that skill at `analyze` or above. High enough to mean real competence, low
/// enough that many proven learners qualify.
const MIN_AUTHOR_LEVEL: BloomLevel = BloomLevel::Analyze;

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

    // Assessment contributions are community-driven but skill-reputation
    // gated: to author an assessment for a skill you must have proven that
    // skill, or be on the DAO committee that seeds a skill's first content.
    if kind == ContentKind::QuestionBank {
        gate_question_bank_authorship(conn, dao_id, proposer, change_json)?;
    }

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

/// Enforce the authorship gate for a question-bank change.
///
/// Every bank in the change must target a skill that (a) falls within the
/// proposing DAO's scope, and (b) the proposer is entitled to author for —
/// either by holding a credential in that skill at [`MIN_AUTHOR_LEVEL`] or
/// above, or by being on the DAO committee.
///
/// The committee bypass is what makes this non-deadlocking: the first
/// assessment for a skill cannot require a credential that only that
/// assessment could produce, so the elected committee seeds it. Once
/// assessments exist, proven peers carry the load.
fn gate_question_bank_authorship(
    conn: &Connection,
    dao_id: &str,
    proposer: &str,
    change_json: &str,
) -> Result<(), String> {
    let doc: QuestionBankDoc = serde_json::from_str(change_json)
        .map_err(|e| format!("invalid question-bank change document: {e}"))?;

    let is_committee = proposer_is_committee(conn, dao_id, proposer)?;

    // The proposer's own DID, for the proficiency lookup. Proposals are
    // authored locally, so this is the local identity.
    let author_did = crate::settings::SettingsStore::get(
        conn,
        crate::settings::registry::keys::IDENTITY_LOCAL_DID,
    );

    for bank in &doc.banks {
        if !skill_in_dao_scope(conn, dao_id, &bank.skill_id)? {
            return Err(format!(
                "skill '{}' is outside DAO '{dao_id}' scope — propose it to the DAO \
                 that governs its subject",
                bank.skill_id
            ));
        }

        // Committee members may seed any in-scope skill; everyone else must
        // have proven the specific skill they are authoring for.
        if is_committee {
            continue;
        }
        if !author_holds_skill(conn, &author_did, &bank.skill_id, MIN_AUTHOR_LEVEL)? {
            return Err(format!(
                "authoring an assessment for '{}' requires a credential in it at '{}' or above, \
                 or DAO committee membership",
                bank.skill_id,
                MIN_AUTHOR_LEVEL.as_str()
            ));
        }
    }
    Ok(())
}

/// Whether `stake_address` is on the DAO's committee (or its chair).
fn proposer_is_committee(
    conn: &Connection,
    dao_id: &str,
    stake_address: &str,
) -> Result<bool, String> {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM governance_dao_members \
          WHERE dao_id = ?1 AND stake_address = ?2 AND role IN ('committee', 'chair')",
        params![dao_id, stake_address],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

/// Whether `subject_did` holds a non-revoked skill credential for `skill_id`
/// at or above `min_level`.
///
/// Reads the Bloom rank from the credential's `credentialSubject.level`,
/// which serialises as an integer 0..=5 matching [`BloomLevel::rank`].
fn author_holds_skill(
    conn: &Connection,
    subject_did: &str,
    skill_id: &str,
    min_level: BloomLevel,
) -> Result<bool, String> {
    if subject_did.is_empty() {
        return Ok(false);
    }
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM credentials \
          WHERE subject_did = ?1 AND skill_id = ?2 AND claim_kind = 'skill' AND revoked = 0 \
            AND CAST(json_extract(signed_vc_json, '$.credentialSubject.level') AS INTEGER) >= ?3",
        params![subject_did, skill_id, min_level.rank() as i64],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

/// Whether `skill_id` falls within the DAO's governed scope. DAOs are scoped
/// to a subject field or a subject, never a single skill, so this walks
/// skill → subject → subject_field to match. A `sentinel`-scoped DAO governs
/// no skills and so authors no assessments.
fn skill_in_dao_scope(conn: &Connection, dao_id: &str, skill_id: &str) -> Result<bool, String> {
    let (scope_type, scope_id): (String, String) = conn
        .query_row(
            "SELECT scope_type, scope_id FROM governance_daos WHERE id = ?1",
            params![dao_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| format!("DAO not found: {e}"))?;

    match scope_type.as_str() {
        "subject" => conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM skills WHERE id = ?1 AND subject_id = ?2",
                params![skill_id, scope_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string()),
        "subject_field" => conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM skills sk \
                 JOIN subjects sub ON sub.id = sk.subject_id \
                 WHERE sk.id = ?1 AND sub.subject_field_id = ?2",
                params![skill_id, scope_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string()),
        // `sentinel` (or anything unrecognised) governs no skills.
        _ => Ok(false),
    }
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

/// The skill-reputation authorship gate for question-bank contributions.
///
/// Uses a full migrated database rather than the minimal `tests::setup`
/// schema, because the gate reads governance scope, committee membership,
/// credentials, and the local identity setting — tables the light fixture
/// does not create.
#[cfg(test)]
mod authorship_gate_tests {
    use super::*;
    use crate::db::Database;
    use crate::settings::{registry::keys, SettingsStore};

    const AUTHOR_DID: &str = "did:key:zAuthor";
    const AUTHOR_STAKE: &str = "stake_author";

    /// A DAO scoped to subject `sub_cs`, one skill in scope and one out, plus
    /// the local identity pointing at AUTHOR_DID. No committee membership and
    /// no credentials by default — the strictest starting point.
    fn base() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO governance_daos (id, name, scope_type, scope_id, status)
                 VALUES ('dao_cs', 'CS DAO', 'subject', 'sub_cs', 'active');
                 INSERT INTO subject_fields (id, name) VALUES ('sf', 'Field');
                 INSERT INTO subjects (id, name, subject_field_id) VALUES
                   ('sub_cs', 'CS', 'sf'), ('sub_bio', 'Bio', 'sf');
                 INSERT INTO skills (id, name, subject_id, bloom_level) VALUES
                   ('skill_rust', 'Rust', 'sub_cs', 'apply'),
                   ('skill_dna', 'DNA', 'sub_bio', 'apply');",
            )
            .unwrap();
        SettingsStore::set(db.conn(), keys::IDENTITY_LOCAL_DID, AUTHOR_DID.to_string()).unwrap();
        db
    }

    fn bank_change(skill_id: &str) -> String {
        format!(r#"{{"banks":[{{"id":"b1","skill_id":"{skill_id}","label":"L"}}],"questions":[]}}"#)
    }

    fn grant_credential(db: &Database, subject_did: &str, skill_id: &str, level: u8) {
        let vc = format!(r#"{{"credentialSubject":{{"level":{level}}}}}"#);
        db.conn()
            .execute(
                "INSERT INTO credentials
                   (id, issuer_did, subject_did, credential_type, claim_kind, skill_id,
                    issuance_date, signed_vc_json, integrity_hash, revoked)
                 VALUES (?1, 'did:key:zIssuer', ?2, 'AssessmentCredential', 'skill', ?3,
                         '2026-01-01T00:00:00Z', ?4, 'hash', 0)",
                params![
                    format!("cred_{skill_id}_{level}"),
                    subject_did,
                    skill_id,
                    vc
                ],
            )
            .unwrap();
    }

    fn make_committee(db: &Database) {
        db.conn()
            .execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role)
                 VALUES ('dao_cs', ?1, 'committee')",
                params![AUTHOR_STAKE],
            )
            .unwrap();
    }

    fn try_propose(db: &Database, skill_id: &str) -> Result<String, String> {
        propose(
            db.conn(),
            ContentKind::QuestionBank,
            "dao_cs",
            "New bank",
            None,
            &bank_change(skill_id),
            AUTHOR_STAKE,
        )
    }

    #[test]
    fn an_unproven_non_committee_author_is_refused() {
        // The gate's whole point: you cannot author an assessment for a skill
        // you have not demonstrated.
        let db = base();
        let err = try_propose(&db, "skill_rust").unwrap_err();
        assert!(err.contains("requires a credential"), "unexpected: {err}");
    }

    #[test]
    fn proving_the_skill_at_analyze_unlocks_authoring() {
        let db = base();
        grant_credential(&db, AUTHOR_DID, "skill_rust", 3); // analyze
        assert!(try_propose(&db, "skill_rust").is_ok());
    }

    #[test]
    fn a_credential_below_analyze_is_not_enough() {
        let db = base();
        grant_credential(&db, AUTHOR_DID, "skill_rust", 2); // apply
        let err = try_propose(&db, "skill_rust").unwrap_err();
        assert!(err.contains("requires a credential"), "unexpected: {err}");
    }

    #[test]
    fn a_credential_above_analyze_also_works() {
        let db = base();
        grant_credential(&db, AUTHOR_DID, "skill_rust", 5); // create
        assert!(try_propose(&db, "skill_rust").is_ok());
    }

    #[test]
    fn a_committee_member_may_seed_a_skill_they_have_not_proven() {
        // The bootstrap escape: the first assessment for a skill cannot
        // require a credential only that assessment could produce.
        let db = base();
        make_committee(&db);
        assert!(
            try_propose(&db, "skill_rust").is_ok(),
            "committee should be able to seed content without a credential"
        );
    }

    #[test]
    fn a_revoked_credential_does_not_authorize() {
        let db = base();
        grant_credential(&db, AUTHOR_DID, "skill_rust", 4);
        db.conn()
            .execute("UPDATE credentials SET revoked = 1", [])
            .unwrap();
        assert!(try_propose(&db, "skill_rust").is_err());
    }

    #[test]
    fn a_credential_in_a_different_skill_does_not_transfer() {
        // Proving Rust does not authorize authoring for DNA.
        let db = base();
        grant_credential(&db, AUTHOR_DID, "skill_rust", 5);
        // skill_dna is out of this DAO's scope anyway, so also assert scope.
        let err = try_propose(&db, "skill_dna").unwrap_err();
        assert!(err.contains("outside DAO"), "unexpected: {err}");
    }

    #[test]
    fn a_skill_outside_the_dao_scope_is_refused_even_when_proven() {
        // Scope is checked before proficiency: a proven skill in the wrong
        // DAO still cannot be authored there.
        let db = base();
        grant_credential(&db, AUTHOR_DID, "skill_dna", 5);
        make_committee(&db); // even a committee member cannot cross scope
        let err = try_propose(&db, "skill_dna").unwrap_err();
        assert!(err.contains("outside DAO"), "unexpected: {err}");
    }

    #[test]
    fn a_credential_held_by_someone_else_does_not_authorize_me() {
        // The gate must key on the proposer's own DID, not any credential in
        // the local database.
        let db = base();
        grant_credential(&db, "did:key:zSomeoneElse", "skill_rust", 5);
        assert!(try_propose(&db, "skill_rust").is_err());
    }

    #[test]
    fn goal_template_proposals_are_not_subject_to_the_skill_gate() {
        // The gate is specific to assessments. A goal-template proposal must
        // still go through unimpeded.
        let db = base();
        let change = r#"{"templates":[{"id":"t1","kind":"job_role","key":"k","label":"L","skill_ids":["skill_rust"]}]}"#;
        let r = propose(
            db.conn(),
            ContentKind::GoalTemplate,
            "dao_cs",
            "T",
            None,
            change,
            AUTHOR_STAKE,
        );
        assert!(
            r.is_ok(),
            "goal templates should not hit the skill gate: {r:?}"
        );
    }
}
