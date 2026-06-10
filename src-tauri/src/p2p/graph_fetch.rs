//! Pull-based public skill-graph fetch over `/alexandria/graph-fetch/1.0`.
//!
//! A node serves *its own owner's* skill graph. The graph nodes are
//! every skill the owner has earned a (non-revoked) credential for;
//! the owner opts each skill in/out of public visibility and marks the
//! subset they actively teach. Requests carry the `subject_did` of the
//! graph being asked for — a node answers `Ok` only if it owns that DID
//! (looked up via the device-local `identity.local_did` setting), and
//! otherwise returns `NotOwner` so a broadcast caller can move on to the
//! next connected peer.
//!
//! Visibility model:
//!   - Default for an earned skill is **public** (so a fresh graph is
//!     useful immediately). The owner can flip individual skills private.
//!   - `teaching` defaults to `false` and is a pure highlight flag.
//!
//! Mirrors the request-response wiring of [`super::vc_fetch`].

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::crypto::did::Did;
use crate::settings::{registry::keys, SettingsStore};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphFetchRequest {
    /// DID whose public graph is being requested.
    pub subject_did: String,
    /// DID of the requesting node (for future rate-limiting / allowlists).
    pub requestor: Did,
    /// Replay-protection nonce.
    pub nonce: String,
}

/// One node in a skill graph. `public`/`teaching` reflect the owner's
/// per-skill preferences. Over the wire (a remote fetch) only public
/// nodes are ever sent, so `public` is always `true` there; the local
/// editor path requests private nodes too.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublicGraphNode {
    pub id: String,
    pub name: String,
    pub bloom_level: String,
    pub subject_name: Option<String>,
    pub public: bool,
    pub teaching: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublicGraphEdge {
    pub skill_id: String,
    pub prerequisite_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublicSkillGraph {
    pub subject_did: String,
    pub nodes: Vec<PublicGraphNode>,
    pub edges: Vec<PublicGraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphFetchResponse {
    Ok(Box<PublicSkillGraph>),
    /// This node does not own the requested `subject_did`.
    NotOwner,
    /// We own the DID but the graph is empty (no earned skills).
    Empty,
}

/// Per-skill visibility/teaching preference as stored in the
/// `instructor.graph_prefs` synced setting.
#[derive(Debug, Clone, Copy)]
struct NodePref {
    public: bool,
    teaching: bool,
}

impl Default for NodePref {
    fn default() -> Self {
        // Earned skills are public by default; teaching is opt-in.
        NodePref {
            public: true,
            teaching: false,
        }
    }
}

/// Parse the `instructor.graph_prefs` JSON object into a per-skill map.
/// Shape: `{ "<skill_id>": { "public": bool, "teaching": bool } }`.
/// Missing / malformed entries fall back to [`NodePref::default`].
fn load_prefs(conn: &Connection) -> HashMap<String, NodePref> {
    let raw = SettingsStore::get(conn, keys::INSTRUCTOR_GRAPH_PREFS).0;
    let mut out = HashMap::new();
    if let Some(obj) = raw.as_object() {
        for (skill_id, v) in obj {
            let public = v.get("public").and_then(|b| b.as_bool()).unwrap_or(true);
            let teaching = v.get("teaching").and_then(|b| b.as_bool()).unwrap_or(false);
            out.insert(skill_id.clone(), NodePref { public, teaching });
        }
    }
    out
}

/// Build the skill graph owned by `subject_did`.
///
/// `include_private` controls whether non-public earned skills are
/// included — `true` for the owner's own editor, `false` for anything
/// that leaves the device.
pub fn build_skill_graph(
    conn: &Connection,
    subject_did: &str,
    include_private: bool,
) -> Result<PublicSkillGraph, String> {
    // 1. Earned (non-revoked) skills for this subject.
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT skill_id FROM credentials
             WHERE subject_did = ?1 AND skill_id IS NOT NULL AND revoked = 0",
        )
        .map_err(|e| e.to_string())?;
    let earned: Vec<String> = stmt
        .query_map([subject_did], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let prefs = load_prefs(conn);

    // 2. Resolve each earned skill to a node, applying visibility.
    let mut nodes = Vec::new();
    let mut included: HashSet<String> = HashSet::new();
    for skill_id in earned {
        let pref = prefs.get(&skill_id).copied().unwrap_or_default();
        if !include_private && !pref.public {
            continue;
        }
        let row = conn
            .query_row(
                "SELECT sk.name, sk.bloom_level, s.name
                 FROM skills sk
                 LEFT JOIN subjects s ON sk.subject_id = s.id
                 WHERE sk.id = ?1",
                [&skill_id],
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
            // Earned a credential for a skill not in the local taxonomy
            // — skip rather than emit a nameless node.
            continue;
        };
        included.insert(skill_id.clone());
        nodes.push(PublicGraphNode {
            id: skill_id,
            name,
            bloom_level,
            subject_name,
            public: pref.public,
            teaching: pref.teaching,
        });
    }

    // 3. Edges among the included set only.
    let mut edge_stmt = conn
        .prepare("SELECT skill_id, prerequisite_id FROM skill_prerequisites")
        .map_err(|e| e.to_string())?;
    let edges: Vec<PublicGraphEdge> = edge_stmt
        .query_map([], |row| {
            Ok(PublicGraphEdge {
                skill_id: row.get(0)?,
                prerequisite_id: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|e| included.contains(&e.skill_id) && included.contains(&e.prerequisite_id))
        .collect();

    Ok(PublicSkillGraph {
        subject_did: subject_did.to_string(),
        nodes,
        edges,
    })
}

/// Handle an inbound graph-fetch request against the local DB.
///
/// Decision tree:
///   1. If `identity.local_did` is unset or differs from the requested
///      `subject_did` → `NotOwner`.
///   2. If we own it but have no public skills → `Empty`.
///   3. Otherwise → `Ok(public graph)`.
pub fn handle_graph_fetch_request(
    conn: &Connection,
    req: &GraphFetchRequest,
) -> Result<GraphFetchResponse, String> {
    let local_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
    if local_did.is_empty() || local_did != req.subject_did {
        return Ok(GraphFetchResponse::NotOwner);
    }
    let graph = build_skill_graph(conn, &local_did, false)?;
    if graph.nodes.is_empty() {
        return Ok(GraphFetchResponse::Empty);
    }
    Ok(GraphFetchResponse::Ok(Box::new(graph)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL,
                 updated_at TEXT, scope TEXT NOT NULL DEFAULT 'sync');
             CREATE TABLE subjects (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE skills (id TEXT PRIMARY KEY, name TEXT NOT NULL,
                 bloom_level TEXT NOT NULL DEFAULT 'apply', subject_id TEXT);
             CREATE TABLE skill_prerequisites (skill_id TEXT NOT NULL,
                 prerequisite_id TEXT NOT NULL, PRIMARY KEY (skill_id, prerequisite_id));
             CREATE TABLE credentials (id TEXT PRIMARY KEY, subject_did TEXT NOT NULL,
                 skill_id TEXT, revoked INTEGER NOT NULL DEFAULT 0);",
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO subjects (id, name) VALUES ('sub1', 'Math');
             INSERT INTO skills (id, name, bloom_level, subject_id) VALUES
                 ('s_lin', 'Linear Algebra', 'apply', 'sub1'),
                 ('s_opt', 'Convex Optimization', 'analyze', 'sub1'),
                 ('s_ml',  'ML Theory', 'evaluate', 'sub1');
             INSERT INTO skill_prerequisites (skill_id, prerequisite_id) VALUES
                 ('s_opt', 's_lin'),
                 ('s_ml',  's_opt');
             INSERT INTO credentials (id, subject_did, skill_id, revoked) VALUES
                 ('c1', 'did:key:alice', 's_lin', 0),
                 ('c2', 'did:key:alice', 's_opt', 0),
                 ('c3', 'did:key:alice', 's_ml',  0),
                 ('c4', 'did:key:alice', 's_ml',  1);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn build_graph_includes_all_earned_by_default() {
        let conn = setup();
        let g = build_skill_graph(&conn, "did:key:alice", false).unwrap();
        assert_eq!(
            g.nodes.len(),
            3,
            "all three earned skills public by default"
        );
        assert_eq!(g.edges.len(), 2, "both prereq edges between earned skills");
        assert!(g.nodes.iter().all(|n| n.public && !n.teaching));
    }

    #[test]
    fn private_skill_hidden_from_public_graph() {
        let conn = setup();
        SettingsStore::set(
            &conn,
            keys::INSTRUCTOR_GRAPH_PREFS,
            crate::settings::registry::JsonSetting(serde_json::json!({
                "s_opt": { "public": false, "teaching": false }
            })),
        )
        .unwrap();
        let public = build_skill_graph(&conn, "did:key:alice", false).unwrap();
        assert_eq!(public.nodes.len(), 2, "convex opt hidden");
        assert!(public.nodes.iter().all(|n| n.id != "s_opt"));
        // Edges touching the hidden node drop out.
        assert!(public.edges.is_empty());
        // The owner's own editor view still sees it.
        let private = build_skill_graph(&conn, "did:key:alice", true).unwrap();
        assert_eq!(private.nodes.len(), 3);
    }

    #[test]
    fn teaching_flag_surfaces() {
        let conn = setup();
        SettingsStore::set(
            &conn,
            keys::INSTRUCTOR_GRAPH_PREFS,
            crate::settings::registry::JsonSetting(serde_json::json!({
                "s_ml": { "public": true, "teaching": true }
            })),
        )
        .unwrap();
        let g = build_skill_graph(&conn, "did:key:alice", false).unwrap();
        let ml = g.nodes.iter().find(|n| n.id == "s_ml").unwrap();
        assert!(ml.teaching);
    }

    #[test]
    fn handler_rejects_non_owner() {
        let conn = setup();
        SettingsStore::set(&conn, keys::IDENTITY_LOCAL_DID, "did:key:alice".to_string()).unwrap();
        let req = GraphFetchRequest {
            subject_did: "did:key:bob".into(),
            requestor: Did("did:key:carol".into()),
            nonce: "n".into(),
        };
        assert!(matches!(
            handle_graph_fetch_request(&conn, &req).unwrap(),
            GraphFetchResponse::NotOwner
        ));
    }

    #[test]
    fn handler_serves_owner_graph() {
        let conn = setup();
        SettingsStore::set(&conn, keys::IDENTITY_LOCAL_DID, "did:key:alice".to_string()).unwrap();
        let req = GraphFetchRequest {
            subject_did: "did:key:alice".into(),
            requestor: Did("did:key:carol".into()),
            nonce: "n".into(),
        };
        match handle_graph_fetch_request(&conn, &req).unwrap() {
            GraphFetchResponse::Ok(g) => assert_eq!(g.nodes.len(), 3),
            other => panic!("expected Ok, got {other:?}"),
        }
    }
}
