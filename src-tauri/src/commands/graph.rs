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

use std::collections::HashSet;

use crate::profile::scope::ProfileState as State;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::crypto::did::Did;
use crate::db::executor::DatabaseWorkload;
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
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "graph.get_mine",
            |db| {
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
            },
        )
        .await
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
    let requested_did = did.clone();
    let (local_did, local_graph) = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "graph.fetch_preflight",
            move |db| {
                let conn = db.conn();
                let local_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
                let graph = if requested_did == local_did {
                    Some(build_skill_graph(conn, &local_did, false)?)
                } else {
                    None
                };
                Ok((local_did, graph))
            },
        )
        .await?;
    if let Some(graph) = local_graph {
        return Ok(graph);
    }

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
    // Discover + dial peers we haven't met yet (the graph owner may not
    // be in our routing table), then broadcast. Known peers = current
    // connections + Kademlia routing table; request-response auto-dials
    // table entries.
    let _ = node.discover_peers(std::time::Duration::from_secs(4)).await;
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

    // Broadcast concurrently and return the first owner that answers,
    // with a per-request cap so unreachable peers fail fast in parallel
    // instead of stacking their timeouts serially.
    use futures::stream::StreamExt;
    const PER_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    let mut inflight = futures::stream::FuturesUnordered::new();
    for peer_str in peers {
        let Ok(peer) = peer_str.parse::<libp2p::PeerId>() else {
            continue;
        };
        let req = GraphFetchRequest {
            subject_did: did.clone(),
            requestor: requestor.clone(),
            nonce: nonce.clone(),
        };
        inflight.push(async move {
            (
                peer,
                tokio::time::timeout(PER_REQUEST_TIMEOUT, node.fetch_graph(peer, req)).await,
            )
        });
    }

    let (mut not_owner, mut empty, mut unreachable) = (0u32, 0u32, 0u32);
    while let Some((peer, res)) = inflight.next().await {
        match res {
            Ok(Ok(GraphFetchResponse::Ok(graph))) => return Ok(*graph),
            Ok(Ok(GraphFetchResponse::NotOwner)) => not_owner += 1,
            Ok(Ok(GraphFetchResponse::Empty)) => empty += 1,
            Ok(Err(e)) => {
                log::info!("graph-fetch: peer {peer} unreachable: {e}");
                unreachable += 1;
            }
            Err(_) => {
                log::info!("graph-fetch: peer {peer} timed out");
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
    // One implementation, shared with the assistant broker, so a connected
    // assistant can never see a different order or a different status.
    let path = alexandria_studio::skills::learning_path(conn, goals, earned)
        .map_err(|error| error.to_string())?;
    Ok(LearningPath {
        goal_skill_ids: path.goal_skill_ids,
        total: path.total,
        earned_count: path.earned_count,
        steps: path
            .steps
            .into_iter()
            .map(|step| LearningPathStep {
                skill_id: step.skill_id,
                name: step.name,
                bloom_level: step.bloom_level,
                subject_name: step.subject_name,
                status: step.status,
                is_goal: step.is_goal,
                prerequisite_ids: step.prerequisite_ids,
                course_recs: step
                    .course_recs
                    .into_iter()
                    .map(|rec| CourseRec {
                        id: rec.course_id,
                        title: rec.title,
                    })
                    .collect(),
            })
            .collect(),
    })
}

/// Compute the local user's learning path toward `goal_skill_ids`.
#[tauri::command]
pub async fn compute_learning_path(
    state: State<'_, AppState>,
    goal_skill_ids: Vec<String>,
) -> Result<LearningPath, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "graph.compute_learning_path",
            move |db| compute_learning_path_db(db, &goal_skill_ids),
        )
        .await
}

fn compute_learning_path_db(
    db: &crate::db::Database,
    goal_skill_ids: &[String],
) -> Result<LearningPath, String> {
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

    compute_path(conn, goal_skill_ids, &earned)
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
