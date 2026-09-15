use rusqlite::{params, Transaction};

use crate::crypto::hash::entity_id;

#[derive(Debug, thiserror::Error)]
pub(crate) enum VoteWriteError {
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error("Nominee has not accepted their nomination")]
    NomineeNotAccepted,
    #[error("Governance tally is invalid or cannot be incremented without overflow")]
    InvalidTally,
}

/// Storage fields from a vote whose authorization has already been checked.
/// These fields are not a committee receipt or independent verification proof.
pub(crate) struct VoteEvidence<'a> {
    pub voter: &'a str,
    pub signature: &'a str,
    pub public_key: &'a str,
}

/// Persist a vote and its derived count in the caller's validation transaction.
/// Returns false for an already-recorded voter; other constraint failures are errors.
pub(crate) fn record_election_vote(
    tx: &Transaction<'_>,
    election_id: &str,
    nominee_id: &str,
    evidence: &VoteEvidence<'_>,
) -> Result<bool, VoteWriteError> {
    let accepted: bool = tx.query_row(
        "SELECT accepted FROM governance_election_nominees WHERE id = ?1 AND election_id = ?2",
        params![nominee_id, election_id],
        |row| row.get(0),
    )?;
    if !accepted {
        return Err(VoteWriteError::NomineeNotAccepted);
    }
    let inserted = tx.execute(
        "INSERT INTO governance_election_votes \
         (id, election_id, voter, nominee_id, signature, public_key) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(election_id, voter) DO NOTHING",
        params![
            entity_id(&[election_id, evidence.voter]), election_id, evidence.voter,
            nominee_id, evidence.signature, evidence.public_key,
        ],
    )?;
    if inserted == 0 {
        return Ok(false);
    }
    let updated = tx.execute(
        "UPDATE governance_election_nominees SET votes_received = votes_received + 1 \
         WHERE id = ?1 AND election_id = ?2 AND typeof(votes_received) = 'integer' \
         AND votes_received BETWEEN 0 AND 9223372036854775806",
        params![nominee_id, election_id],
    )?;
    if updated != 1 {
        return Err(VoteWriteError::InvalidTally);
    }
    Ok(true)
}

pub(crate) fn record_proposal_vote(
    tx: &Transaction<'_>,
    proposal_id: &str,
    in_favor: bool,
    evidence: &VoteEvidence<'_>,
) -> Result<bool, VoteWriteError> {
    let inserted = tx.execute(
        "INSERT INTO governance_proposal_votes \
         (id, proposal_id, voter, in_favor, signature, public_key) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(proposal_id, voter) DO NOTHING",
        params![
            entity_id(&[proposal_id, evidence.voter]),
            proposal_id,
            evidence.voter,
            in_favor,
            evidence.signature,
            evidence.public_key,
        ],
    )?;
    if inserted == 0 {
        return Ok(false);
    }
    // Fixed SQL keeps both choices on the same storage path without a dynamic
    // column name, and rejects overflow instead of letting SQLite promote to REAL.
    let updated = tx.execute(
        "UPDATE governance_proposals \
         SET votes_for = votes_for + ?2, votes_against = votes_against + (1 - ?2) \
         WHERE id = ?1 AND votes_for >= 0 AND votes_against >= 0 \
         AND typeof(votes_for) = 'integer' AND typeof(votes_against) = 'integer' \
         AND ((?2 = 1 AND votes_for < 9223372036854775807) \
           OR (?2 = 0 AND votes_against < 9223372036854775807))",
        params![proposal_id, in_favor],
    )?;
    if updated != 1 {
        return Err(VoteWriteError::InvalidTally);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use rusqlite::TransactionBehavior;

    fn setup() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO governance_daos (id, name, scope_type, scope_id, status) \
             VALUES ('dao', 'DAO', 'subject', 'subject', 'active'); \
             INSERT INTO governance_proposals (id, dao_id, title, category, proposer, status) \
             VALUES ('proposal', 'dao', 'Proposal', 'general', 'author', 'published'); \
             INSERT INTO governance_elections (id, dao_id, title, phase, seats) \
             VALUES ('election', 'dao', 'Election', 'voting', 7), \
                    ('other-election', 'dao', 'Other', 'voting', 7); \
             INSERT INTO governance_election_nominees (id, election_id, stake_address, accepted) \
             VALUES ('nominee', 'election', 'candidate', 1);",
            )
            .unwrap();
        db
    }

    fn evidence(voter: &str) -> VoteEvidence<'_> {
        VoteEvidence {
            voter,
            signature: "verified-signature",
            public_key: "verified-key",
        }
    }

    fn proposal_vote(db: &Database, voter: &str, in_favor: bool) -> Result<bool, VoteWriteError> {
        let tx = Transaction::new_unchecked(db.conn(), TransactionBehavior::Immediate)?;
        let inserted = record_proposal_vote(&tx, "proposal", in_favor, &evidence(voter))?;
        tx.commit()?;
        Ok(inserted)
    }

    fn election_vote(db: &Database, election_id: &str) -> Result<bool, VoteWriteError> {
        let tx = Transaction::new_unchecked(db.conn(), TransactionBehavior::Immediate)?;
        let inserted = record_election_vote(&tx, election_id, "nominee", &evidence("voter"))?;
        tx.commit()?;
        Ok(inserted)
    }

    #[test]
    fn proposal_tally_failure_rolls_back_vote_and_retry_counts_once() {
        let db = setup();
        db.conn()
            .execute_batch(
                "CREATE TRIGGER fail_tally BEFORE UPDATE OF votes_for ON governance_proposals \
             BEGIN SELECT RAISE(ABORT, 'injected tally failure'); END;",
            )
            .unwrap();
        assert!(proposal_vote(&db, "yes", true).is_err());
        assert_eq!(
            db.conn()
                .query_row("SELECT COUNT(*) FROM governance_proposal_votes", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
        db.conn().execute_batch("DROP TRIGGER fail_tally").unwrap();
        assert!(proposal_vote(&db, "yes", true).unwrap());
        assert!(!proposal_vote(&db, "yes", true).unwrap());
        assert!(proposal_vote(&db, "no", false).unwrap());
        assert_eq!(db.conn().query_row("SELECT votes_for, votes_against FROM governance_proposals WHERE id = 'proposal'", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).unwrap(), (1, 1));
    }

    #[test]
    fn election_tally_failure_rolls_back_vote_and_retry_counts_once() {
        let db = setup();
        db.conn().execute_batch(
            "CREATE TRIGGER fail_tally BEFORE UPDATE OF votes_received ON governance_election_nominees \
             BEGIN SELECT RAISE(ABORT, 'injected tally failure'); END;",
        ).unwrap();
        assert!(election_vote(&db, "election").is_err());
        assert_eq!(
            db.conn()
                .query_row("SELECT COUNT(*) FROM governance_election_votes", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
        db.conn().execute_batch("DROP TRIGGER fail_tally").unwrap();
        assert!(election_vote(&db, "election").unwrap());
        assert!(!election_vote(&db, "election").unwrap());
        assert_eq!(
            db.conn()
                .query_row(
                    "SELECT votes_received FROM governance_election_nominees WHERE id = 'nominee'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn nominee_from_another_election_cannot_receive_the_vote() {
        let db = setup();
        assert!(election_vote(&db, "other-election").is_err());
        assert_eq!(
            db.conn()
                .query_row("SELECT COUNT(*) FROM governance_election_votes", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }

    #[test]
    fn tally_overflow_does_not_leave_a_vote_without_a_count() {
        let db = setup();
        db.conn()
            .execute_batch(
                "UPDATE governance_proposals SET votes_for = 9223372036854775807; \
             UPDATE governance_election_nominees SET votes_received = 9223372036854775807;",
            )
            .unwrap();
        assert!(proposal_vote(&db, "yes", true).is_err());
        assert!(election_vote(&db, "election").is_err());
        assert_eq!(
            db.conn()
                .query_row("SELECT COUNT(*) FROM governance_proposal_votes", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
        assert_eq!(
            db.conn()
                .query_row("SELECT COUNT(*) FROM governance_election_votes", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }
}
