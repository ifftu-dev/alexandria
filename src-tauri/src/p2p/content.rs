//! Inbound handler for community-content version documents received on
//! `/alexandria/goal-templates/1.0` and `/alexandria/question-banks/1.0`.
//! Both carry a signed [`VersionDoc`] whose `kind` self-identifies the content
//! type, so one handler serves both topics.
//!
//! **Authority**: the network layer only proves that a registered identity
//! signed the envelope. The document's `ratified_by` list and signature are
//! declared by that sender, so they are not evidence of DAO ratification.
//! Production rejects these documents until the handler consumes a verified
//! committee outcome certificate. The old apply path is available only in an
//! explicitly enabled development build.

use crate::db::Database;
#[cfg(any(test, all(debug_assertions, feature = "legacy-content-ratification")))]
use crate::domain::content_ratification::{apply_version_doc, VersionDoc};
use crate::p2p::types::SignedGossipMessage;

/// Handle a received goal-template / question-bank version document.
pub fn handle_content_version_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<usize, String> {
    #[cfg(not(all(debug_assertions, feature = "legacy-content-ratification")))]
    {
        let _ = (db, message);
        Err(crate::domain::content_ratification::LEGACY_CONTENT_RATIFICATION_DISABLED.into())
    }

    #[cfg(all(debug_assertions, feature = "legacy-content-ratification"))]
    {
        handle_legacy_content_version_message(db, message)
    }
}

#[cfg(any(test, all(debug_assertions, feature = "legacy-content-ratification")))]
fn handle_legacy_content_version_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<usize, String> {
    let doc: VersionDoc = serde_json::from_slice(&message.payload)
        .map_err(|e| format!("invalid content version doc: {e}"))?;
    apply_version_doc(db.conn(), &doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(all(debug_assertions, feature = "legacy-content-ratification")))]
    use crate::p2p::types::TOPIC_GOAL_TEMPLATES;
    use crate::p2p::types::TOPIC_QUESTION_BANKS;

    fn seeded_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        crate::db::seed::seed_if_empty(db.conn()).expect("seed");
        db
    }

    fn message(topic: &str, doc: &VersionDoc) -> SignedGossipMessage {
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
    fn hostile_question_bank_doc() -> VersionDoc {
        VersionDoc {
            kind: "question_bank_change".into(),
            version: 99,
            previous_cid: None,
            ratified_by: vec!["stake_test1registered".into()],
            ratified_at: "2026-01-01T00:00:00Z".into(),
            signature: "self-declared".into(),
            taxonomy_version: None,
            content: serde_json::json!({
                "banks": [{"id": "qb_js", "skill_id": "skill_javascript",
                           "label": "JS", "pass_threshold": 0.0, "draw_count": 1}],
                "questions": [{"id": "bq_js1", "bank_id": "qb_js", "prompt": "?",
                               "options": ["a", "b"], "correct_indices": [0]}]
            }),
        }
    }

    #[cfg(not(all(debug_assertions, feature = "legacy-content-ratification")))]
    fn hostile_goal_template_doc() -> VersionDoc {
        VersionDoc {
            kind: "goal_template_change".into(),
            version: 99,
            previous_cid: None,
            ratified_by: vec!["stake_test1registered".into()],
            ratified_at: "2026-01-01T00:00:00Z".into(),
            signature: "self-declared".into(),
            taxonomy_version: None,
            content: serde_json::json!({
                "templates": [{"id": "gt_role_fe", "kind": "job_role",
                               "key": "frontend_engineer", "label": "Rewritten",
                               "skill_ids": []}]
            }),
        }
    }

    #[cfg(not(all(debug_assertions, feature = "legacy-content-ratification")))]
    fn content_state(db: &Database) -> Vec<String> {
        [
            "SELECT group_concat(id || ':' || pass_threshold || ':' || draw_count || ':' || ratified, '|') FROM question_banks",
            "SELECT group_concat(id || ':' || correct_indices, '|') FROM bank_questions",
            "SELECT group_concat(id || ':' || label || ':' || skill_ids, '|') FROM goal_templates",
            "SELECT COUNT(*) FROM question_bank_versions",
            "SELECT COUNT(*) FROM goal_template_versions",
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

    #[cfg(not(all(debug_assertions, feature = "legacy-content-ratification")))]
    #[test]
    fn production_handler_rejects_content_version_gossip_without_mutation() {
        let db = seeded_db();
        let before = content_state(&db);

        for (topic, doc) in [
            (TOPIC_QUESTION_BANKS, hostile_question_bank_doc()),
            (TOPIC_GOAL_TEMPLATES, hostile_goal_template_doc()),
        ] {
            let error = handle_content_version_message(&db, &message(topic, &doc)).unwrap_err();
            assert!(error.contains("verified committee certificates"), "{error}");
        }

        assert_eq!(content_state(&db), before);
    }

    #[test]
    fn legacy_handler_still_applies_version_documents() {
        let db = seeded_db();
        let doc = hostile_question_bank_doc();

        handle_legacy_content_version_message(&db, &message(TOPIC_QUESTION_BANKS, &doc)).unwrap();

        let threshold: f64 = db
            .conn()
            .query_row(
                "SELECT pass_threshold FROM question_banks WHERE id = 'qb_js'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(threshold, 0.0);
    }
}
