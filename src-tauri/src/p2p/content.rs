//! Community-content gossip — `/alexandria/goal-templates/1.0` and
//! `/alexandria/question-banks/1.0`.
//!
//! Caller-declared goal-template and question-bank ratification is deleted;
//! the templates and banks are bundled built-in content (`db::bundled`). Both
//! topics stay subscribed until the coordinated wire-protocol removal, and
//! every inbound version document is rejected before any database access.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

pub const LEGACY_CONTENT_GOSSIP_DISABLED: &str =
    "content version gossip is retired; caller-declared goal-template and question-bank ratification is deleted";

/// Reject a received goal-template / question-bank version document.
pub fn handle_content_version_message(
    _db: &Database,
    _message: &SignedGossipMessage,
) -> Result<(), String> {
    Err(LEGACY_CONTENT_GOSSIP_DISABLED.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::types::{TOPIC_GOAL_TEMPLATES, TOPIC_QUESTION_BANKS};

    fn seeded_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        crate::db::bundled::install_bundled_data(db.conn()).expect("bundled data");
        db
    }

    fn message(topic: &str, doc: &serde_json::Value) -> SignedGossipMessage {
        SignedGossipMessage {
            topic: topic.into(),
            payload: serde_json::to_vec(doc).unwrap(),
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: "stake_test1registered".into(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        }
    }

    /// Rewrites the genesis JavaScript bank's answers and pass threshold.
    fn hostile_question_bank_doc() -> serde_json::Value {
        serde_json::json!({
            "kind": "question_bank_change",
            "version": 99,
            "previous_cid": null,
            "ratified_by": ["stake_test1registered"],
            "ratified_at": "2026-01-01T00:00:00Z",
            "signature": "self-declared",
            "taxonomy_version": null,
            "content": {
                "banks": [{"id": "qb_js", "skill_id": "skill_javascript",
                           "label": "JS", "pass_threshold": 0.0, "draw_count": 1}],
                "questions": [{"id": "bq_js1", "bank_id": "qb_js", "prompt": "?",
                               "options": ["a", "b"], "correct_indices": [0]}]
            },
        })
    }

    fn hostile_goal_template_doc() -> serde_json::Value {
        serde_json::json!({
            "kind": "goal_template_change",
            "version": 99,
            "previous_cid": null,
            "ratified_by": ["stake_test1registered"],
            "ratified_at": "2026-01-01T00:00:00Z",
            "signature": "self-declared",
            "taxonomy_version": null,
            "content": {
                "templates": [{"id": "gt_role_fe", "kind": "job_role",
                               "key": "frontend_engineer", "label": "Rewritten",
                               "skill_ids": []}]
            },
        })
    }

    fn content_state(db: &Database) -> Vec<String> {
        [
            "SELECT group_concat(id || ':' || pass_threshold || ':' || draw_count || ':' || ratified, '|') FROM question_banks",
            "SELECT group_concat(id || ':' || correct_indices, '|') FROM bank_questions",
            "SELECT group_concat(id || ':' || label || ':' || skill_ids, '|') FROM goal_templates",
            "SELECT COUNT(*) FROM question_bank_versions",
            "SELECT COUNT(*) FROM goal_template_versions",
            "SELECT COUNT(*) FROM sync_log",
        ]
        .iter()
        .map(|query| {
            db.conn()
                .query_row(query, [], |row| {
                    row.get::<_, Option<rusqlite::types::Value>>(0)
                })
                .map(|value| format!("{value:?}"))
                .unwrap()
        })
        .collect()
    }

    #[test]
    fn content_version_gossip_is_rejected_without_mutation() {
        let db = seeded_db();
        let before = content_state(&db);

        for (topic, doc) in [
            (TOPIC_QUESTION_BANKS, hostile_question_bank_doc()),
            (TOPIC_GOAL_TEMPLATES, hostile_goal_template_doc()),
        ] {
            let error = handle_content_version_message(&db, &message(topic, &doc)).unwrap_err();
            assert_eq!(error, LEGACY_CONTENT_GOSSIP_DISABLED);
        }

        assert_eq!(content_state(&db), before);
    }
}
