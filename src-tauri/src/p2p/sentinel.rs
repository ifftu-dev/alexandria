//! Sentinel gossip — inbound handler for `/alexandria/sentinel-priors/1.0`.
//!
//! Mirrors `p2p::governance::handle_governance_message` in structure.
//! Applies the authority gate (signer must be a Sentinel DAO committee
//! member) and the ordering invariant (the referenced governance
//! proposal must be locally known and `approved` before the
//! `sentinel_priors` row is mirrored) before persisting. Idempotent:
//! re-receiving the same announcement is a no-op.

use rusqlite::params;

use crate::db::Database;
use crate::domain::sentinel::SentinelPriorAnnouncement;
use crate::p2p::types::SignedGossipMessage;

const SENTINEL_DAO_ID: &str = "sentinel-dao";

/// Handle an incoming Sentinel prior announcement.
///
/// Returns the deserialized announcement on success even when the
/// persistence step is skipped (e.g. proposal not yet known locally),
/// mirroring `handle_governance_message`'s behavior so callers can log
/// meaningfully. Real validation failures (bad kind, unauthorized
/// signer) return `Err`.
pub fn handle_sentinel_prior_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<SentinelPriorAnnouncement, String> {
    let ann: SentinelPriorAnnouncement = serde_json::from_slice(&message.payload)
        .map_err(|e| format!("invalid sentinel prior announcement: {e}"))?;

    validate_announcement(&ann)?;

    // Authority gate: signer must be on the Sentinel DAO committee.
    // This is the same pattern used for taxonomy/committee updates —
    // we trust the local view of committee membership, which is
    // itself established via authorized governance gossip.
    if !is_sentinel_committee_member(db, &message.stake_address) {
        return Err(format!(
            "unauthorized sentinel prior from '{}': not on Sentinel DAO committee",
            message.stake_address
        ));
    }

    // Ordering invariant: the approving proposal must already be
    // locally known and status='approved'. If we haven't seen it yet,
    // skip rather than fail — governance gossip may arrive shortly
    // after, and a re-gossip of the prior will pick it up.
    let proposal_status: Option<String> = db
        .conn()
        .query_row(
            "SELECT status FROM governance_proposals WHERE id = ?1",
            params![ann.proposal_id],
            |row| row.get(0),
        )
        .ok();
    match proposal_status.as_deref() {
        Some("approved") => {}
        Some(other) => {
            return Err(format!(
                "sentinel prior references proposal '{}' in state '{}', expected 'approved'",
                ann.proposal_id, other
            ));
        }
        None => {
            log::debug!(
                "Sentinel: proposal '{}' not in local DB — deferring prior '{}'",
                ann.proposal_id,
                ann.prior_id,
            );
            return Ok(ann);
        }
    }

    // Idempotent upsert — same prior_id from the same CID must be a
    // no-op; a conflicting entry (shouldn't happen given the
    // deterministic id) is ignored in favor of the local row.
    db.conn()
        .execute(
            "INSERT OR IGNORE INTO sentinel_priors
                 (id, proposal_id, cid, model_kind, label, schema_version,
                  sample_count, notes, ratified_at, signature)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                ann.prior_id,
                ann.proposal_id,
                ann.cid,
                ann.model_kind,
                ann.label,
                ann.schema_version as i64,
                ann.sample_count,
                ann.notes,
                ann.ratified_at,
                ann.signature,
            ],
        )
        .map_err(|e| format!("failed to insert sentinel prior: {e}"))?;

    // Sync log — follows the same convention as governance.rs so
    // existing sync-log views show both inbound and outbound events.
    let signature_hex = hex::encode(&message.signature);
    db.conn()
        .execute(
            "INSERT INTO sync_log (entity_type, entity_id, direction, peer_id, signature)
             VALUES ('sentinel_prior', ?1, 'received', ?2, ?3)",
            params![ann.prior_id, message.stake_address, signature_hex],
        )
        .map_err(|e| format!("failed to record sync_log: {e}"))?;

    log::info!(
        "Sentinel: mirrored prior '{}' ({} / {}) from '{}'",
        ann.prior_id,
        ann.model_kind,
        ann.label,
        message.stake_address,
    );

    Ok(ann)
}

fn validate_announcement(ann: &SentinelPriorAnnouncement) -> Result<(), String> {
    if ann.prior_id.is_empty() {
        return Err("sentinel prior announcement missing prior_id".into());
    }
    if ann.proposal_id.is_empty() {
        return Err("sentinel prior announcement missing proposal_id".into());
    }
    if ann.cid.is_empty() {
        return Err("sentinel prior announcement missing cid".into());
    }
    if ann.label.trim().is_empty() {
        return Err("sentinel prior announcement missing label".into());
    }
    // Face kind is forbidden across the board — see decision 2 in
    // docs/sentinel-federation.md. Reject loudly on gossip too, so a
    // malicious peer can't sneak face data in via a drifted client.
    match ann.model_kind.as_str() {
        "keystroke" | "mouse" => Ok(()),
        "face" => Err(
            "face kind is forbidden for sentinel priors (see sentinel-federation.md decision 2)"
                .into(),
        ),
        other => Err(format!("unknown model_kind: {other}")),
    }
}

fn is_sentinel_committee_member(db: &Database, stake_address: &str) -> bool {
    db.conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM governance_dao_members
             WHERE dao_id = ?1 AND stake_address = ?2 AND role IN ('committee', 'chair')",
            params![SENTINEL_DAO_ID, stake_address],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn committee_setup(db: &Database, signer: &str) {
        // Migration 037 seeded the Sentinel DAO row; add the signer
        // as a committee member.
        db.conn()
            .execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role)
                 VALUES ('sentinel-dao', ?1, 'committee')",
                params![signer],
            )
            .unwrap();
    }

    fn approved_proposal(db: &Database, proposal_id: &str) {
        db.conn()
            .execute(
                "INSERT INTO governance_proposals
                    (id, dao_id, title, category, proposer, status)
                 VALUES (?1, 'sentinel-dao', 'test', 'sentinel_prior', 'stake_test1', 'approved')",
                params![proposal_id],
            )
            .unwrap();
    }

    fn make_message(ann: &SentinelPriorAnnouncement, signer: &str) -> SignedGossipMessage {
        SignedGossipMessage {
            topic: crate::p2p::types::TOPIC_SENTINEL_PRIORS.into(),
            payload: serde_json::to_vec(ann).unwrap(),
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: signer.into(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        }
    }

    fn make_ann(proposal_id: &str, prior_id: &str, kind: &str) -> SentinelPriorAnnouncement {
        SentinelPriorAnnouncement {
            prior_id: prior_id.into(),
            proposal_id: proposal_id.into(),
            cid: "cid-123".into(),
            model_kind: kind.into(),
            label: "paste_macro".into(),
            schema_version: 1,
            sample_count: 42,
            notes: None,
            signature: "deadbeef".into(),
            ratified_at: "2026-04-18T00:00:00Z".into(),
        }
    }

    #[test]
    fn mirrors_approved_prior_from_committee_signer() {
        let db = test_db();
        committee_setup(&db, "stake_test1signer");
        approved_proposal(&db, "prop1");
        let ann = make_ann("prop1", "prior1", "keystroke");

        let result = handle_sentinel_prior_message(&db, &make_message(&ann, "stake_test1signer"));
        assert!(result.is_ok(), "got {result:?}");

        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sentinel_priors WHERE id = 'prior1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn rejects_non_committee_signer() {
        let db = test_db();
        approved_proposal(&db, "prop1");
        let ann = make_ann("prop1", "prior1", "keystroke");

        let err =
            handle_sentinel_prior_message(&db, &make_message(&ann, "stake_attacker")).unwrap_err();
        assert!(err.contains("unauthorized"), "got {err}");

        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM sentinel_priors", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn rejects_face_kind() {
        let db = test_db();
        committee_setup(&db, "stake_test1signer");
        approved_proposal(&db, "prop1");
        let ann = make_ann("prop1", "prior1", "face");

        let err = handle_sentinel_prior_message(&db, &make_message(&ann, "stake_test1signer"))
            .unwrap_err();
        assert!(err.contains("face"), "got {err}");
    }

    #[test]
    fn defers_when_proposal_unknown_locally() {
        let db = test_db();
        committee_setup(&db, "stake_test1signer");
        // No proposal inserted — ordering invariant kicks in.
        let ann = make_ann("prop-unknown", "prior1", "mouse");

        let result = handle_sentinel_prior_message(&db, &make_message(&ann, "stake_test1signer"));
        assert!(result.is_ok(), "defer should return Ok, got {result:?}");

        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM sentinel_priors", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn rejects_unapproved_proposal() {
        let db = test_db();
        committee_setup(&db, "stake_test1signer");
        db.conn()
            .execute(
                "INSERT INTO governance_proposals
                    (id, dao_id, title, category, proposer, status)
                 VALUES ('prop1', 'sentinel-dao', 'test', 'sentinel_prior',
                         'stake_test1', 'rejected')",
                [],
            )
            .unwrap();
        let ann = make_ann("prop1", "prior1", "mouse");

        let err = handle_sentinel_prior_message(&db, &make_message(&ann, "stake_test1signer"))
            .unwrap_err();
        assert!(err.contains("expected 'approved'"), "got {err}");
    }

    #[test]
    fn mirror_is_idempotent() {
        let db = test_db();
        committee_setup(&db, "stake_test1signer");
        approved_proposal(&db, "prop1");
        let ann = make_ann("prop1", "prior1", "keystroke");

        handle_sentinel_prior_message(&db, &make_message(&ann, "stake_test1signer")).unwrap();
        handle_sentinel_prior_message(&db, &make_message(&ann, "stake_test1signer")).unwrap();

        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sentinel_priors WHERE id = 'prior1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
