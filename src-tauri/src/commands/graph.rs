//! Skill-graph + learning-path commands.
//!
//! Three concerns:
//!   1. The owner's own graph (all earned skills + per-skill prefs) for
//!      the visibility editor — [`get_my_skill_graph`].
//!   2. Fetching another DID's *public* graph over the
//!      `/alexandria/graph-fetch/1.0` P2P protocol, with a same-DID
//!      loopback so a user can preview their own public graph and so
//!      the feature is exercisable on a single node — [`fetch_public_graph`].
//!   3. Computing a topo-ordered learning path from the local user's
//!      earned skills toward a set of goal skills, with per-skill course
//!      recommendations — [`compute_learning_path`].

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::crypto::did::Did;
use crate::p2p::graph_fetch::{
    build_skill_graph, GraphFetchRequest, GraphFetchResponse, PublicSkillGraph,
};
use crate::settings::{registry::keys, SettingsStore};
use crate::AppState;

/// The owner's full skill graph (including skills they've marked
/// private), used by the visibility editor. Returns an empty graph when
/// the vault is locked / no DID cached yet.
#[tauri::command]
pub async fn get_my_skill_graph(state: State<'_, AppState>) -> Result<PublicSkillGraph, String> {
    let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();
    let local_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
    if local_did.is_empty() {
        return Ok(PublicSkillGraph {
            subject_did: String::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
        });
    }
    build_skill_graph(conn, &local_did, true)
}

/// Fetch a DID's *public* skill graph.
///
/// If `did` is the local owner, the graph is built directly from the
/// local DB (public view). Otherwise we broadcast a graph-fetch request
/// to each connected peer and return the first `Ok` — the responding
/// peer answers only if it owns `did`.
#[tauri::command]
pub async fn fetch_public_graph(
    state: State<'_, AppState>,
    did: String,
) -> Result<PublicSkillGraph, String> {
    // Read the local DID, then drop the std lock before any await.
    let local_did = {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();
        let local_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
        if did == local_did {
            // Loopback: serve our own public graph directly.
            return build_skill_graph(conn, &local_did, false);
        }
        local_did
    };

    let requestor = Did(if local_did.is_empty() {
        "did:key:unknown".to_string()
    } else {
        local_did
    });
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_default();

    let node_guard = state.p2p_node.lock().await;
    let node = node_guard.as_ref().ok_or("P2P node not running")?;
    // Known peers = current connections + the Kademlia routing table.
    // Idle connections get reaped between UI actions, so broadcasting
    // only to live connections misses peers we can perfectly well
    // reach — request-response auto-dials table entries.
    let peers = node
        .known_peers()
        .await
        .map_err(|e| format!("failed to list peers: {e}"))?;
    if peers.is_empty() {
        return Err("no known peers to fetch graph from".to_string());
    }

    log::info!(
        "graph-fetch: broadcasting request for {did} to {} known peers: {peers:?}",
        peers.len()
    );

    let (mut not_owner, mut empty, mut unreachable) = (0u32, 0u32, 0u32);
    for peer_str in peers {
        let Ok(peer) = peer_str.parse::<libp2p::PeerId>() else {
            continue;
        };
        let req = GraphFetchRequest {
            subject_did: did.clone(),
            requestor: requestor.clone(),
            nonce: nonce.clone(),
        };
        match node.fetch_graph(peer, req).await {
            Ok(GraphFetchResponse::Ok(graph)) => return Ok(*graph),
            Ok(GraphFetchResponse::NotOwner) => not_owner += 1,
            Ok(GraphFetchResponse::Empty) => empty += 1,
            Err(e) => {
                log::info!("graph-fetch: peer {peer} unreachable: {e}");
                unreachable += 1;
            }
        }
    }

    if empty > 0 {
        // The owner's node answered — there's just nothing public yet.
        return Err(
            "the owner's node was reached but their graph has no public skills".to_string(),
        );
    }
    Err(format!(
        "graph not found: {not_owner} peer(s) answered not-owner, {unreachable} unreachable"
    ))
}

// ---------------------------------------------------------------------------
// Learning path
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CourseRec {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LearningPathStep {
    pub skill_id: String,
    pub name: String,
    pub bloom_level: String,
    pub subject_name: Option<String>,
    /// `"earned" | "available" | "locked"`.
    pub status: String,
    /// `true` if this skill is one of the requested goals.
    pub is_goal: bool,
    pub prerequisite_ids: Vec<String>,
    pub course_recs: Vec<CourseRec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LearningPath {
    pub goal_skill_ids: Vec<String>,
    pub steps: Vec<LearningPathStep>,
    pub total: usize,
    pub earned_count: usize,
}

/// Compute a learning path toward `goals` given the set of skills the
/// learner has already `earned`. Pure over the connection so it can be
/// unit-tested against an in-memory DB.
///
/// The relevant set is the goals plus the transitive closure of their
/// prerequisites. Each relevant skill is ordered by its longest
/// prerequisite chain (so prerequisites always precede dependents) and
/// labelled:
///   - `earned`    — already proven,
///   - `available` — every direct prerequisite is earned (unlocked next),
///   - `locked`    — at least one prerequisite is still unearned.
pub fn compute_path(
    conn: &Connection,
    goals: &[String],
    earned: &HashSet<String>,
) -> Result<LearningPath, String> {
    // Direct prerequisites: skill_id -> [prerequisite_id].
    let mut prereqs: HashMap<String, Vec<String>> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT skill_id, prerequisite_id FROM skill_prerequisites")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for r in rows {
            let (skill, prereq) = r.map_err(|e| e.to_string())?;
            prereqs.entry(skill).or_default().push(prereq);
        }
    }

    // Transitive closure of goals over prerequisites.
    let mut relevant: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = goals.to_vec();
    while let Some(id) = stack.pop() {
        if !relevant.insert(id.clone()) {
            continue;
        }
        if let Some(ps) = prereqs.get(&id) {
            for p in ps {
                if !relevant.contains(p) {
                    stack.push(p.clone());
                }
            }
        }
    }

    // Longest-prerequisite-chain depth for ordering (cycle-protected).
    fn depth(
        id: &str,
        prereqs: &HashMap<String, Vec<String>>,
        relevant: &HashSet<String>,
        memo: &mut HashMap<String, usize>,
        visiting: &mut HashSet<String>,
    ) -> usize {
        if let Some(d) = memo.get(id) {
            return *d;
        }
        if !visiting.insert(id.to_string()) {
            return 0; // cycle guard
        }
        let d = prereqs
            .get(id)
            .map(|ps| {
                ps.iter()
                    .filter(|p| relevant.contains(*p))
                    .map(|p| 1 + depth(p, prereqs, relevant, memo, visiting))
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        visiting.remove(id);
        memo.insert(id.to_string(), d);
        d
    }

    let goal_set: HashSet<&String> = goals.iter().collect();
    let mut memo = HashMap::new();
    let mut visiting = HashSet::new();
    let mut ordered: Vec<String> = relevant.iter().cloned().collect();
    ordered.sort_by(|a, b| {
        let da = depth(a, &prereqs, &relevant, &mut memo, &mut visiting);
        let db = depth(b, &prereqs, &relevant, &mut memo, &mut visiting);
        da.cmp(&db).then_with(|| a.cmp(b))
    });

    let mut steps = Vec::new();
    let mut earned_count = 0;
    for skill_id in &ordered {
        let row = conn
            .query_row(
                "SELECT sk.name, sk.bloom_level, s.name
                 FROM skills sk
                 LEFT JOIN subjects s ON sk.subject_id = s.id
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
            .ok();
        let Some((name, bloom_level, subject_name)) = row else {
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

        // Recommend up to 3 published courses tagged with this skill.
        let course_recs = if is_earned {
            Vec::new()
        } else {
            recommend_courses(conn, skill_id).unwrap_or_default()
        };

        steps.push(LearningPathStep {
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

/// Published courses tagged with `skill_id` (matched against the JSON
/// `skill_ids` array column), capped at 3.
fn recommend_courses(conn: &Connection, skill_id: &str) -> Result<Vec<CourseRec>, String> {
    let pattern = format!("%\"{skill_id}\"%");
    let mut stmt = conn
        .prepare(
            "SELECT id, title FROM courses
             WHERE status = 'published' AND skill_ids LIKE ?1
             ORDER BY title LIMIT 3",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([pattern], |row| {
            Ok(CourseRec {
                id: row.get(0)?,
                title: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Compute the local user's learning path toward `goal_skill_ids`.
#[tauri::command]
pub async fn compute_learning_path(
    state: State<'_, AppState>,
    goal_skill_ids: Vec<String>,
) -> Result<LearningPath, String> {
    let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let local_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
    let earned: HashSet<String> = if local_did.is_empty() {
        HashSet::new()
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT skill_id FROM credentials
                 WHERE subject_did = ?1 AND skill_id IS NOT NULL AND revoked = 0",
            )
            .map_err(|e| e.to_string())?;
        let set = stmt
            .query_map([&local_did], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<HashSet<_>, _>>()
            .map_err(|e| e.to_string())?;
        set
    };

    compute_path(conn, &goal_skill_ids, &earned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE subjects (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE skills (id TEXT PRIMARY KEY, name TEXT NOT NULL,
                 bloom_level TEXT NOT NULL DEFAULT 'apply', subject_id TEXT);
             CREATE TABLE skill_prerequisites (skill_id TEXT NOT NULL,
                 prerequisite_id TEXT NOT NULL, PRIMARY KEY (skill_id, prerequisite_id));
             CREATE TABLE courses (id TEXT PRIMARY KEY, title TEXT NOT NULL,
                 status TEXT NOT NULL, skill_ids TEXT);",
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO subjects (id, name) VALUES ('sub1', 'Math');
             INSERT INTO skills (id, name, bloom_level, subject_id) VALUES
                 ('s_lin', 'Linear Algebra', 'apply', 'sub1'),
                 ('s_prob', 'Probability', 'apply', 'sub1'),
                 ('s_opt', 'Convex Optimization', 'analyze', 'sub1'),
                 ('s_grad', 'Gradient Methods', 'analyze', 'sub1'),
                 ('s_ml',  'ML Theory', 'evaluate', 'sub1');
             INSERT INTO skill_prerequisites (skill_id, prerequisite_id) VALUES
                 ('s_opt', 's_lin'),
                 ('s_grad', 's_opt'),
                 ('s_ml',  's_grad'),
                 ('s_ml',  's_prob');
             INSERT INTO courses (id, title, status, skill_ids) VALUES
                 ('crs1', 'Convex Opt 101', 'published', '[\"s_opt\"]'),
                 ('crs2', 'Draft Course', 'draft', '[\"s_opt\"]');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn path_orders_prereqs_before_goal() {
        let conn = setup();
        let earned: HashSet<String> = ["s_lin", "s_prob"].iter().map(|s| s.to_string()).collect();
        let path = compute_path(&conn, &["s_ml".to_string()], &earned).unwrap();

        // Relevant = s_ml + closure {s_grad, s_opt, s_lin, s_prob}.
        assert_eq!(path.total, 5);
        assert_eq!(path.earned_count, 2);

        let order: Vec<&str> = path.steps.iter().map(|s| s.skill_id.as_str()).collect();
        let pos = |id: &str| order.iter().position(|x| *x == id).unwrap();
        assert!(pos("s_lin") < pos("s_opt"));
        assert!(pos("s_opt") < pos("s_grad"));
        assert!(pos("s_grad") < pos("s_ml"));
    }

    #[test]
    fn status_and_goal_flags() {
        let conn = setup();
        let earned: HashSet<String> = ["s_lin", "s_prob"].iter().map(|s| s.to_string()).collect();
        let path = compute_path(&conn, &["s_ml".to_string()], &earned).unwrap();
        let by = |id: &str| path.steps.iter().find(|s| s.skill_id == id).unwrap();

        assert_eq!(by("s_lin").status, "earned");
        assert_eq!(by("s_opt").status, "available"); // its only prereq s_lin is earned
        assert_eq!(by("s_grad").status, "locked"); // needs s_opt (unearned)
        assert_eq!(by("s_ml").status, "locked");
        assert!(by("s_ml").is_goal);
        assert!(!by("s_opt").is_goal);
    }

    #[test]
    fn recommends_only_published_courses() {
        let conn = setup();
        let earned = HashSet::new();
        let path = compute_path(&conn, &["s_opt".to_string()], &earned).unwrap();
        let opt = path.steps.iter().find(|s| s.skill_id == "s_opt").unwrap();
        assert_eq!(opt.course_recs.len(), 1);
        assert_eq!(opt.course_recs[0].id, "crs1");
    }

    #[test]
    fn earned_skills_get_no_course_recs() {
        let conn = setup();
        let earned: HashSet<String> = ["s_opt"].iter().map(|s| s.to_string()).collect();
        let path = compute_path(&conn, &["s_opt".to_string()], &earned).unwrap();
        let opt = path.steps.iter().find(|s| s.skill_id == "s_opt").unwrap();
        assert_eq!(opt.status, "earned");
        assert!(opt.course_recs.is_empty());
    }
}
