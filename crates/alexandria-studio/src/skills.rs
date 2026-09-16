//! Skill graph, learning path and goal resolution over the profile database.
//!
//! These reads are shared: the app's Tauri commands and P2P graph service call
//! them, and so does the assistant broker under `learning:read`. Keeping one
//! implementation means an assistant can never see a different graph, or a
//! different prerequisite order, from the one the app shows.
//!
//! Everything here is pure over a `&Connection`, including the two settings it
//! needs, so it can be unit-tested against an in-memory database.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::jd_parser::{extract_skills, SkillEntry};
use crate::{Error, Result};

/// Device-local cache of the active profile's `did:key`.
const LOCAL_DID_KEY: &str = "identity.local_did";
/// Per-skill `{public, teaching}` preferences, keyed by skill id.
const GRAPH_PREFS_KEY: &str = "instructor.graph_prefs";
const TEMPLATE_COLS: &str =
    "id, kind, key, label, board, grade, skill_ids, taxonomy_version, ratified";

pub const MAX_GOALS: usize = 50;
pub const MAX_TEXT_CHARS: usize = 100_000;
const MAX_TEMPLATES: usize = 500;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphNode {
    pub skill_id: String,
    pub name: String,
    pub bloom_level: String,
    pub subject_name: Option<String>,
    /// Whether the owner shares this skill with peers. Private skills are
    /// returned here only when the caller asked for the owner's own view.
    pub public: bool,
    pub teaching: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphEdge {
    pub skill_id: String,
    pub prerequisite_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SkillGraph {
    pub subject_did: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// True when private skills are included (the owner's own view).
    pub includes_private: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CourseRec {
    pub course_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PathStep {
    pub skill_id: String,
    pub name: String,
    pub bloom_level: String,
    pub subject_name: Option<String>,
    /// `earned` | `available` | `locked`.
    pub status: String,
    pub is_goal: bool,
    pub prerequisite_ids: Vec<String>,
    pub course_recs: Vec<CourseRec>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LearningPath {
    pub goal_skill_ids: Vec<String>,
    pub steps: Vec<PathStep>,
    pub total: usize,
    pub earned_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GoalTemplate {
    pub id: String,
    /// `exam` | `curriculum` | `job_role`.
    pub kind: String,
    pub key: String,
    pub label: String,
    pub board: Option<String>,
    pub grade: Option<String>,
    pub skill_ids: Vec<String>,
    pub taxonomy_version: Option<String>,
    pub ratified: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SkillSuggestion {
    pub skill_id: String,
    pub name: String,
    pub score: f64,
    pub matched: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GoalResolution {
    pub label: String,
    /// Authoritative targets for a curated template; empty for parsed text,
    /// where `suggestions` are for the learner to confirm.
    pub goal_skill_ids: Vec<String>,
    pub suggestions: Vec<SkillSuggestion>,
    pub taxonomy_version: Option<String>,
    /// `template` or `text_parsed`.
    pub resolution_provenance: String,
}

/// What a caller asked to resolve. There is deliberately no link variant:
/// fetching a URL is a network request, and nothing an assistant says should
/// make this device issue one.
pub enum Goal<'a> {
    Exam { key: &'a str },
    JobRole { key: &'a str },
    Curriculum { board: &'a str, grade: &'a str },
    Text { text: &'a str },
}

fn setting(conn: &Connection, key: &str) -> String {
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        [key],
        |row| row.get::<_, String>(0),
    )
    .unwrap_or_default()
}

struct NodePref {
    public: bool,
    teaching: bool,
}

impl Default for NodePref {
    fn default() -> Self {
        // An earned skill is public unless the owner says otherwise.
        Self {
            public: true,
            teaching: false,
        }
    }
}

fn load_prefs(conn: &Connection) -> HashMap<String, NodePref> {
    let raw: serde_json::Value =
        serde_json::from_str(&setting(conn, GRAPH_PREFS_KEY)).unwrap_or(serde_json::Value::Null);
    let mut out = HashMap::new();
    if let Some(object) = raw.as_object() {
        for (skill_id, value) in object {
            out.insert(
                skill_id.clone(),
                NodePref {
                    public: value
                        .get("public")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true),
                    teaching: value
                        .get("teaching")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                },
            );
        }
    }
    out
}

/// The active profile's DID, or empty when no profile identity is cached.
pub fn local_did(conn: &Connection) -> String {
    setting(conn, LOCAL_DID_KEY)
}

fn earned_skills(conn: &Connection, subject_did: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT skill_id FROM credentials
         WHERE subject_did = ?1 AND skill_id IS NOT NULL AND revoked = 0",
    )?;
    let rows = stmt.query_map([subject_did], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<HashSet<_>, _>>()?)
}

fn skill_row(conn: &Connection, skill_id: &str) -> Option<(String, String, Option<String>)> {
    conn.query_row(
        "SELECT sk.name, sk.bloom_level, s.name
         FROM skills sk LEFT JOIN subjects s ON sk.subject_id = s.id
         WHERE sk.id = ?1",
        [skill_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )
    .ok()
}

/// Every skill `subject_did` holds a live credential for, with prerequisite
/// edges among them. `include_private` is true only for the owner's own view;
/// anything that leaves the device passes false.
pub fn skill_graph(
    conn: &Connection,
    subject_did: &str,
    include_private: bool,
) -> Result<SkillGraph> {
    let prefs = load_prefs(conn);
    let fallback = NodePref::default();
    let mut nodes = Vec::new();
    let mut included: HashSet<String> = HashSet::new();
    let mut earned: Vec<String> = earned_skills(conn, subject_did)?.into_iter().collect();
    earned.sort();
    for skill_id in earned {
        let pref = prefs.get(&skill_id).unwrap_or(&fallback);
        if !include_private && !pref.public {
            continue;
        }
        // A credential for a skill outside the local taxonomy has no name to
        // show, so it is skipped rather than returned nameless.
        let Some((name, bloom_level, subject_name)) = skill_row(conn, &skill_id) else {
            continue;
        };
        included.insert(skill_id.clone());
        nodes.push(GraphNode {
            skill_id,
            name,
            bloom_level,
            subject_name,
            public: pref.public,
            teaching: pref.teaching,
        });
    }

    let mut stmt = conn.prepare("SELECT skill_id, prerequisite_id FROM skill_prerequisites")?;
    let edges = stmt
        .query_map([], |row| {
            Ok(GraphEdge {
                skill_id: row.get(0)?,
                prerequisite_id: row.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|edge| {
            included.contains(&edge.skill_id) && included.contains(&edge.prerequisite_id)
        })
        .collect();

    Ok(SkillGraph {
        subject_did: subject_did.to_string(),
        nodes,
        edges,
        includes_private: include_private,
    })
}

fn prerequisite_map(conn: &Connection) -> Result<HashMap<String, Vec<String>>> {
    let mut stmt = conn.prepare("SELECT skill_id, prerequisite_id FROM skill_prerequisites")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut prereqs: HashMap<String, Vec<String>> = HashMap::new();
    for row in rows {
        let (skill, prerequisite) = row?;
        prereqs.entry(skill).or_default().push(prerequisite);
    }
    Ok(prereqs)
}

/// Longest prerequisite chain below `id`, so prerequisites sort before what
/// depends on them. Cycles stop at the skill already being visited.
fn depth(
    id: &str,
    prereqs: &HashMap<String, Vec<String>>,
    relevant: &HashSet<String>,
    memo: &mut HashMap<String, usize>,
    visiting: &mut HashSet<String>,
) -> usize {
    if let Some(known) = memo.get(id) {
        return *known;
    }
    if !visiting.insert(id.to_string()) {
        return 0;
    }
    let deepest = prereqs
        .get(id)
        .map(|below| {
            below
                .iter()
                .filter(|p| relevant.contains(*p))
                .map(|p| 1 + depth(p, prereqs, relevant, memo, visiting))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    visiting.remove(id);
    memo.insert(id.to_string(), deepest);
    deepest
}

/// Published courses tagged with `skill_id`, capped at three.
fn recommend_courses(conn: &Connection, skill_id: &str) -> Result<Vec<CourseRec>> {
    let pattern = format!("%\"{skill_id}\"%");
    let mut stmt = conn.prepare(
        "SELECT id, title FROM courses
         WHERE status = 'published' AND skill_ids LIKE ?1
         ORDER BY title LIMIT 3",
    )?;
    let rows = stmt.query_map([pattern], |row| {
        Ok(CourseRec {
            course_id: row.get(0)?,
            title: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Order the goals and everything they require, labelling each skill
/// `earned`, `available` (every direct prerequisite earned) or `locked`.
pub fn learning_path(
    conn: &Connection,
    goals: &[String],
    earned: &HashSet<String>,
) -> Result<LearningPath> {
    if goals.len() > MAX_GOALS {
        return Err(Error::Invalid("too many goal skills".into()));
    }
    let prereqs = prerequisite_map(conn)?;

    let mut relevant: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = goals.to_vec();
    while let Some(id) = stack.pop() {
        if !relevant.insert(id.clone()) {
            continue;
        }
        if let Some(below) = prereqs.get(&id) {
            for prerequisite in below {
                if !relevant.contains(prerequisite) {
                    stack.push(prerequisite.clone());
                }
            }
        }
    }

    let goal_set: HashSet<&String> = goals.iter().collect();
    let mut memo = HashMap::new();
    let mut visiting = HashSet::new();
    let mut ordered: Vec<String> = relevant.iter().cloned().collect();
    ordered.sort_by(|a, b| {
        let a_depth = depth(a, &prereqs, &relevant, &mut memo, &mut visiting);
        let b_depth = depth(b, &prereqs, &relevant, &mut memo, &mut visiting);
        a_depth.cmp(&b_depth).then_with(|| a.cmp(b))
    });

    let mut steps = Vec::new();
    let mut earned_count = 0;
    for skill_id in &ordered {
        let Some((name, bloom_level, subject_name)) = skill_row(conn, skill_id) else {
            continue;
        };
        let direct = prereqs.get(skill_id).cloned().unwrap_or_default();
        let is_earned = earned.contains(skill_id);
        let status = if is_earned {
            earned_count += 1;
            "earned"
        } else if direct.iter().all(|p| earned.contains(p)) {
            "available"
        } else {
            "locked"
        };
        let course_recs = if is_earned {
            Vec::new()
        } else {
            recommend_courses(conn, skill_id).unwrap_or_default()
        };
        steps.push(PathStep {
            skill_id: skill_id.clone(),
            name,
            bloom_level,
            subject_name,
            status: status.to_string(),
            is_goal: goal_set.contains(skill_id),
            prerequisite_ids: direct,
            course_recs,
        });
    }

    let total = steps.len();
    Ok(LearningPath {
        goal_skill_ids: goals.to_vec(),
        steps,
        total,
        earned_count,
    })
}

/// The learner's own path: their earned skills toward `goals`.
pub fn my_learning_path(conn: &Connection, goals: &[String]) -> Result<LearningPath> {
    let did = local_did(conn);
    let earned = if did.is_empty() {
        HashSet::new()
    } else {
        earned_skills(conn, &did)?
    };
    learning_path(conn, goals, &earned)
}

fn template_row(row: &rusqlite::Row) -> rusqlite::Result<GoalTemplate> {
    let skill_ids: String = row.get(6)?;
    Ok(GoalTemplate {
        id: row.get(0)?,
        kind: row.get(1)?,
        key: row.get(2)?,
        label: row.get(3)?,
        board: row.get(4)?,
        grade: row.get(5)?,
        skill_ids: serde_json::from_str(&skill_ids).unwrap_or_default(),
        taxonomy_version: row.get(7)?,
        ratified: row.get::<_, i64>(8)? != 0,
    })
}

pub fn goal_templates(conn: &Connection, kind: Option<&str>) -> Result<Vec<GoalTemplate>> {
    let mut sql = format!("SELECT {TEMPLATE_COLS} FROM goal_templates");
    if kind.is_some() {
        sql.push_str(" WHERE kind = ?1");
    }
    sql.push_str(&format!(" ORDER BY label LIMIT {MAX_TEMPLATES}"));
    let mut stmt = conn.prepare(&sql)?;
    let rows = match kind {
        Some(kind) => stmt.query_map(params![kind], template_row)?,
        None => stmt.query_map([], template_row)?,
    };
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn template_by_key(conn: &Connection, kind: &str, key: &str) -> Result<Option<GoalTemplate>> {
    let sql = format!("SELECT {TEMPLATE_COLS} FROM goal_templates WHERE kind = ?1 AND key = ?2");
    // A missing template is not an error; anything else is.
    Ok(conn
        .query_row(&sql, params![kind, key], template_row)
        .optional()?)
}

/// Every skill's matchable surface, for on-device text matching.
pub fn skill_entries(conn: &Connection) -> Result<Vec<SkillEntry>> {
    let mut stmt = conn.prepare("SELECT id, name, synonyms FROM skills")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name, synonyms) = row?;
        out.push(SkillEntry {
            id,
            name,
            synonyms: synonyms
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        });
    }
    Ok(out)
}

/// Resolve a goal to target skills. Curated templates are authoritative;
/// free text produces suggestions the learner confirms, and never a saved
/// goal.
pub fn resolve_goal(conn: &Connection, goal: Goal<'_>) -> Result<GoalResolution> {
    let (kind, key) = match goal {
        Goal::Exam { key } => ("exam", key.to_string()),
        Goal::JobRole { key } => ("job_role", key.to_string()),
        Goal::Curriculum { board, grade } => (
            "curriculum",
            format!("{}.grade{}", board.to_lowercase(), grade),
        ),
        Goal::Text { text } => {
            if text.chars().count() > MAX_TEXT_CHARS {
                return Err(Error::Invalid("the text is too long to match".into()));
            }
            let entries = skill_entries(conn)?;
            let names: HashMap<&str, &str> = entries
                .iter()
                .map(|entry| (entry.id.as_str(), entry.name.as_str()))
                .collect();
            let suggestions = extract_skills(text, &entries)
                .into_iter()
                .map(|candidate| SkillSuggestion {
                    name: names
                        .get(candidate.skill_id.as_str())
                        .copied()
                        .unwrap_or_default()
                        .to_string(),
                    skill_id: candidate.skill_id,
                    score: candidate.score,
                    matched: candidate.matched,
                })
                .collect();
            return Ok(GoalResolution {
                label: "Custom goal (from the supplied text)".into(),
                goal_skill_ids: Vec::new(),
                suggestions,
                taxonomy_version: None,
                resolution_provenance: "text_parsed".into(),
            });
        }
    };
    let template = template_by_key(conn, kind, &key)?.ok_or(Error::NotFound)?;
    Ok(GoalResolution {
        label: template.label,
        goal_skill_ids: template.skill_ids,
        suggestions: Vec::new(),
        taxonomy_version: template.taxonomy_version,
        resolution_provenance: "template".into(),
    })
}

/// The learner's enrolments, newest activity first, with per-element
/// progress. `course_id` narrows it to one course.
pub fn learning_progress(conn: &Connection, course_id: Option<&str>) -> Result<serde_json::Value> {
    // Most recent lesson activity, falling back to when they enrolled.
    const LAST_ACTIVITY: &str = "COALESCE((SELECT MAX(ep.updated_at) FROM element_progress ep \
         WHERE ep.enrollment_id = e.id), e.enrolled_at)";
    let sql = format!(
        "SELECT e.id, e.course_id, c.title, e.status, e.enrolled_at, e.completed_at, e.updated_at
         FROM enrollments e LEFT JOIN courses c ON c.id = e.course_id
         WHERE (?1 = '' OR e.course_id = ?1)
         ORDER BY {LAST_ACTIVITY} DESC LIMIT 200"
    );
    let mut stmt = conn.prepare(&sql)?;
    let enrolments = stmt
        .query_map([course_id.unwrap_or_default()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                serde_json::json!({
                    "course_id": row.get::<_, String>(1)?,
                    "course_title": row.get::<_, Option<String>>(2)?,
                    "status": row.get::<_, String>(3)?,
                    "enrolled_at": row.get::<_, String>(4)?,
                    "completed_at": row.get::<_, Option<String>>(5)?,
                    "updated_at": row.get::<_, String>(6)?,
                }),
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut items = Vec::new();
    for (enrollment_id, mut enrolment) in enrolments {
        let mut stmt = conn.prepare(
            "SELECT p.element_id, e.title, e.element_type, p.status, p.score, p.time_spent, p.completed_at
             FROM element_progress p LEFT JOIN course_elements e ON e.id = p.element_id
             WHERE p.enrollment_id = ?1 ORDER BY p.element_id LIMIT 500",
        )?;
        let elements = stmt
            .query_map([&enrollment_id], |row| {
                Ok(serde_json::json!({
                    "element_id": row.get::<_, String>(0)?,
                    "title": row.get::<_, Option<String>>(1)?,
                    "element_type": row.get::<_, Option<String>>(2)?,
                    "status": row.get::<_, String>(3)?,
                    "score": row.get::<_, Option<f64>>(4)?,
                    "time_spent_seconds": row.get::<_, Option<i64>>(5)?,
                    "completed_at": row.get::<_, Option<String>>(6)?,
                }))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let completed = elements
            .iter()
            .filter(|element| element["status"] == "completed")
            .count();
        enrolment["elements_total"] = elements.len().into();
        enrolment["elements_completed"] = completed.into();
        enrolment["elements"] = serde_json::Value::Array(elements);
        items.push(enrolment);
    }
    Ok(serde_json::json!({ "enrolments": items }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE subjects (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE skills (id TEXT PRIMARY KEY, name TEXT NOT NULL,
                 bloom_level TEXT NOT NULL DEFAULT 'apply', subject_id TEXT, synonyms TEXT);
             CREATE TABLE skill_prerequisites (skill_id TEXT NOT NULL, prerequisite_id TEXT NOT NULL,
                 PRIMARY KEY (skill_id, prerequisite_id));
             CREATE TABLE credentials (id TEXT PRIMARY KEY, subject_did TEXT NOT NULL,
                 skill_id TEXT, revoked INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE courses (id TEXT PRIMARY KEY, title TEXT NOT NULL,
                 status TEXT NOT NULL DEFAULT 'draft', skill_ids TEXT);
             CREATE TABLE goal_templates (id TEXT PRIMARY KEY, kind TEXT NOT NULL, key TEXT NOT NULL,
                 label TEXT NOT NULL, board TEXT, grade TEXT, skill_ids TEXT NOT NULL,
                 taxonomy_version TEXT, ratified INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE enrollments (id TEXT PRIMARY KEY, course_id TEXT NOT NULL,
                 enrolled_at TEXT NOT NULL DEFAULT '2026-01-01', completed_at TEXT,
                 status TEXT NOT NULL DEFAULT 'active',
                 updated_at TEXT NOT NULL DEFAULT '2026-01-01');
             CREATE TABLE course_elements (id TEXT PRIMARY KEY, chapter_id TEXT NOT NULL,
                 title TEXT NOT NULL, element_type TEXT NOT NULL, position INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE element_progress (id TEXT PRIMARY KEY, enrollment_id TEXT NOT NULL,
                 element_id TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'not_started', score REAL,
                 time_spent INTEGER DEFAULT 0, completed_at TEXT,
                 updated_at TEXT NOT NULL DEFAULT '2026-01-02');
             INSERT INTO enrollments(id, course_id, status) VALUES ('e1', 'course-c', 'active');
             INSERT INTO course_elements(id, chapter_id, title, element_type, position) VALUES
                 ('g1', 'ch', 'Lesson one', 'text', 0),
                 ('g2', 'ch', 'Lesson two', 'text', 1);
             INSERT INTO element_progress(id, enrollment_id, element_id, status, score, time_spent) VALUES
                 ('p1', 'e1', 'g1', 'completed', 0.9, 120),
                 ('p2', 'e1', 'g2', 'in_progress', NULL, 30);
             INSERT INTO app_settings(key, value) VALUES ('identity.local_did', 'did:key:me');
             INSERT INTO subjects(id, name) VALUES ('sub', 'Computing');
             INSERT INTO skills(id, name, bloom_level, subject_id, synonyms) VALUES
                 ('a', 'Alpha', 'remember', 'sub', 'first'),
                 ('b', 'Beta', 'apply', 'sub', NULL),
                 ('c', 'Gamma', 'create', 'sub', NULL);
             INSERT INTO skill_prerequisites(skill_id, prerequisite_id) VALUES ('b','a'), ('c','b');
             INSERT INTO credentials(id, subject_did, skill_id, revoked) VALUES
                 ('v1', 'did:key:me', 'a', 0),
                 ('v2', 'did:key:me', 'b', 0),
                 ('v3', 'did:key:me', 'gone', 0),
                 ('v4', 'did:key:me', 'c', 1);
             INSERT INTO courses(id, title, status, skill_ids) VALUES
                 ('course-c', 'Gamma course', 'published', '[\"c\"]'),
                 ('draft-c', 'Gamma draft', 'draft', '[\"c\"]');
             INSERT INTO goal_templates(id, kind, key, label, skill_ids, ratified) VALUES
                 ('t1', 'exam', 'finals', 'Finals', '[\"c\"]', 1);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn the_owners_graph_keeps_private_skills_that_peers_never_see() {
        let conn = db();
        conn.execute(
            "INSERT INTO app_settings(key, value) VALUES ('instructor.graph_prefs', ?1)",
            [r#"{"b":{"public":false,"teaching":true}}"#],
        )
        .unwrap();

        let peers = skill_graph(&conn, "did:key:me", false).unwrap();
        assert_eq!(
            peers.nodes.iter().map(|n| &n.skill_id).collect::<Vec<_>>(),
            ["a"],
            "a private skill is not shared"
        );
        assert!(peers.edges.is_empty(), "edges need both ends included");

        let owner = skill_graph(&conn, "did:key:me", true).unwrap();
        assert_eq!(
            owner.nodes.iter().map(|n| &n.skill_id).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(owner.includes_private);
        assert!(!owner.nodes[1].public && owner.nodes[1].teaching);
        assert_eq!(owner.edges.len(), 1, "b requires a");
        // A revoked credential is not earned, and a skill outside the
        // taxonomy has no node.
        assert!(owner.nodes.iter().all(|n| n.skill_id != "c"));
        assert!(owner.nodes.iter().all(|n| n.skill_id != "gone"));
    }

    #[test]
    fn a_path_orders_prerequisites_first_and_recommends_only_published_courses() {
        let conn = db();
        let path = my_learning_path(&conn, &["c".to_string()]).unwrap();
        assert_eq!(
            path.steps.iter().map(|s| &s.skill_id).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        assert_eq!(
            path.steps
                .iter()
                .map(|s| s.status.as_str())
                .collect::<Vec<_>>(),
            ["earned", "earned", "available"]
        );
        assert_eq!(path.earned_count, 2);
        assert!(path.steps[2].is_goal);
        assert_eq!(
            path.steps[2].course_recs,
            vec![CourseRec {
                course_id: "course-c".into(),
                title: "Gamma course".into()
            }],
            "an unpublished course is never recommended"
        );
        assert!(
            path.steps[0].course_recs.is_empty(),
            "earned skills need no course"
        );
    }

    #[test]
    fn goals_resolve_from_templates_or_text_but_never_from_a_link() {
        let conn = db();
        let template = resolve_goal(&conn, Goal::Exam { key: "finals" }).unwrap();
        assert_eq!(template.goal_skill_ids, ["c"]);
        assert_eq!(template.resolution_provenance, "template");
        assert!(matches!(
            resolve_goal(&conn, Goal::Exam { key: "missing" }),
            Err(Error::NotFound)
        ));

        let parsed = resolve_goal(
            &conn,
            Goal::Text {
                text: "We need Alpha and first principles",
            },
        )
        .unwrap();
        assert_eq!(parsed.resolution_provenance, "text_parsed");
        assert!(parsed.goal_skill_ids.is_empty(), "text only suggests");
        assert_eq!(parsed.suggestions[0].skill_id, "a");
        assert_eq!(parsed.suggestions[0].name, "Alpha");
    }

    #[test]
    fn progress_reports_each_enrolment_with_its_observed_lessons() {
        let conn = db();
        let all = learning_progress(&conn, None).unwrap();
        let enrolment = &all["enrolments"][0];
        assert_eq!(enrolment["course_id"], "course-c");
        assert_eq!(enrolment["course_title"], "Gamma course");
        assert_eq!(enrolment["elements_total"], 2);
        assert_eq!(enrolment["elements_completed"], 1);
        assert_eq!(enrolment["elements"][0]["element_id"], "g1");
        assert_eq!(enrolment["elements"][0]["title"], "Lesson one");
        assert_eq!(enrolment["elements"][0]["score"], 0.9);
        assert_eq!(enrolment["elements"][0]["time_spent_seconds"], 120);
        assert_eq!(enrolment["elements"][1]["score"], serde_json::Value::Null);

        assert_eq!(
            learning_progress(&conn, Some("course-c")).unwrap()["enrolments"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            learning_progress(&conn, Some("elsewhere")).unwrap()["enrolments"]
                .as_array()
                .unwrap()
                .is_empty(),
            "filtering by another course returns nothing"
        );
    }

    #[test]
    fn oversized_input_is_refused() {
        let conn = db();
        let goals: Vec<String> = (0..MAX_GOALS + 1).map(|n| n.to_string()).collect();
        assert!(matches!(
            learning_path(&conn, &goals, &HashSet::new()),
            Err(Error::Invalid(_))
        ));
        let long = "x".repeat(MAX_TEXT_CHARS + 1);
        assert!(matches!(
            resolve_goal(&conn, Goal::Text { text: &long }),
            Err(Error::Invalid(_))
        ));
    }
}
