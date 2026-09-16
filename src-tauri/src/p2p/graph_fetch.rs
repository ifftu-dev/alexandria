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

/// Build the skill graph owned by `subject_did`.
///
/// `include_private` controls whether non-public earned skills are
/// included — `true` for the owner's own editor, `false` for anything
/// that leaves the device.
///
/// The implementation lives in `alexandria-studio` so that this P2P path,
/// the app's own editor and the assistant broker all build one graph from
/// one place.
pub fn build_skill_graph(
    conn: &Connection,
    subject_did: &str,
    include_private: bool,
) -> Result<PublicSkillGraph, String> {
    let graph = alexandria_studio::skills::skill_graph(conn, subject_did, include_private)
        .map_err(|error| error.to_string())?;
    Ok(PublicSkillGraph {
        subject_did: graph.subject_did,
        nodes: graph
            .nodes
            .into_iter()
            .map(|node| PublicGraphNode {
                id: node.skill_id,
                name: node.name,
                bloom_level: node.bloom_level,
                subject_name: node.subject_name,
                public: node.public,
                teaching: node.teaching,
            })
            .collect(),
        edges: graph
            .edges
            .into_iter()
            .map(|edge| PublicGraphEdge {
                skill_id: edge.skill_id,
                prerequisite_id: edge.prerequisite_id,
            })
            .collect(),
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
