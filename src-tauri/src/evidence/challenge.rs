//! Credential challenge engine (VC-first rebuild).
//!
//! Core lifecycle operations for the challenge mechanism:
//!   * [`submit_challenge`] — validate params, create a pending record
//!     with a 30-day expiry, sign it with the challenger's local
//!     identity.
//!   * [`vote`] — committee member casts a single vote; first vote
//!     transitions the challenge to `reviewing`.
//!   * [`resolve`] — tally votes (2/3 supermajority required to
//!     uphold) and flip the credential's revocation bit on uphold.
//!   * [`expire_overdue`] — scheduled job: challenges past their
//!     deadline become `expired`.
//!
//! Revocation of an upheld challenge's credential uses the same path
//! as `commands::credentials::revoke_credential_impl` — we flip the
//! status-list bit and mark the `credentials` row revoked.

use rusqlite::{params, Connection, OptionalExtension};

use crate::crypto::hash::entity_id;
use crate::domain::challenge::{
    ChallengeResolution, ChallengeStatus, ChallengeVote, CredentialChallenge,
    SubmitCredentialChallengeParams, CHALLENGE_DEADLINE_DAYS, MIN_STAKE_LOVELACE,
};

/// Supermajority threshold for uphold resolution (2/3).
const SUPERMAJORITY_NUM: i64 = 2;
const SUPERMAJORITY_DEN: i64 = 3;

/// Open a new challenge. The challenger is identified by the local
/// node's stake address (read from `local_identity`). The signature
/// is a placeholder for now; gossip validation checks it against the
/// author address of the envelope.
pub fn submit_challenge(
    conn: &Connection,
    params: &SubmitCredentialChallengeParams,
    challenger: &str,
    signature_hex: &str,
) -> Result<CredentialChallenge, String> {
    if params.reason.trim().len() < 10 {
        return Err("reason must be at least 10 characters".into());
    }
    if (params.stake_lovelace as u64) < MIN_STAKE_LOVELACE {
        return Err(format!(
            "stake must be at least {MIN_STAKE_LOVELACE} lovelace, got {}",
            params.stake_lovelace
        ));
    }

    // Reject challenges on unknown or already-revoked credentials —
    // once a cred is revoked, nothing more to do.
    let (_exists, revoked): (i64, i64) = conn
        .query_row(
            "SELECT 1, revoked FROM credentials WHERE id = ?1",
            params![params.credential_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("credential not found: {}", params.credential_id))?;
    if revoked == 1 {
        return Err(format!(
            "credential {} is already revoked",
            params.credential_id
        ));
    }

    let id = entity_id(&[challenger, &params.credential_id, &params.reason]);
    let expires_at = chrono::Utc::now() + chrono::Duration::days(CHALLENGE_DEADLINE_DAYS);

    conn.execute(
        "INSERT INTO credential_challenges \
         (id, challenger, credential_id, reason, stake_lovelace, \
          stake_tx_hash, status, dao_id, signature, expires_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'pending', ?6, ?7, ?8)",
        params![
            id,
            challenger,
            params.credential_id,
            params.reason,
            params.stake_lovelace,
            params.dao_id,
            signature_hex,
            expires_at.to_rfc3339(),
        ],
    )
    .map_err(|e| e.to_string())?;

    get_challenge(conn, &id)?.ok_or_else(|| "challenge vanished after insert".into())
}

/// Cast a committee vote. First vote flips status to `reviewing`;
/// duplicate votes from the same voter are rejected.
pub fn vote(
    conn: &Connection,
    challenge_id: &str,
    voter: &str,
    upheld: bool,
    reason: Option<&str>,
) -> Result<ChallengeVote, String> {
    let status = load_status(conn, challenge_id)?;
    match status {
        ChallengeStatus::Pending | ChallengeStatus::Reviewing => {}
        other => {
            return Err(format!(
                "cannot vote on a challenge in state {}",
                other.as_str()
            ));
        }
    }

    let vote_id = entity_id(&[challenge_id, voter]);
    let insert = conn.execute(
        "INSERT INTO credential_challenge_votes \
         (id, challenge_id, voter, upheld, reason) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(challenge_id, voter) DO NOTHING",
        params![vote_id, challenge_id, voter, upheld as i64, reason],
    );
    match insert {
        Ok(0) => return Err(format!("voter {voter} already voted on this challenge")),
        Ok(_) => {}
        Err(e) => return Err(e.to_string()),
    }

    // First vote flips to reviewing.
    if matches!(status, ChallengeStatus::Pending) {
        conn.execute(
            "UPDATE credential_challenges SET status = 'reviewing' WHERE id = ?1",
            params![challenge_id],
        )
        .map_err(|e| e.to_string())?;
    }

    conn.query_row(
        "SELECT id, challenge_id, voter, upheld, reason, voted_at \
         FROM credential_challenge_votes WHERE id = ?1",
        params![vote_id],
        |row| {
            let upheld_i: i64 = row.get(3)?;
            Ok(ChallengeVote {
                id: row.get(0)?,
                challenge_id: row.get(1)?,
                voter: row.get(2)?,
                upheld: upheld_i == 1,
                reason: row.get(4)?,
                voted_at: row.get(5)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

/// Tally and resolve a challenge. Requires a 2/3 supermajority of
/// upholds to revoke the credential; otherwise the challenge is
/// marked rejected.
///
/// Flips the target credential's revocation bit when upheld — the
/// status list bytes are updated in place so the existing
/// RevocationList2020 workflow serves the revocation to external
/// verifiers without any extra wiring.
pub fn resolve(conn: &Connection, challenge_id: &str) -> Result<ChallengeResolution, String> {
    let status = load_status(conn, challenge_id)?;
    if !matches!(
        status,
        ChallengeStatus::Reviewing | ChallengeStatus::Pending
    ) {
        return Err(format!(
            "challenge already resolved (state: {})",
            status.as_str()
        ));
    }

    let (uphold, reject): (i64, i64) = conn
        .query_row(
            "SELECT \
                COALESCE(SUM(CASE WHEN upheld = 1 THEN 1 ELSE 0 END), 0), \
                COALESCE(SUM(CASE WHEN upheld = 0 THEN 1 ELSE 0 END), 0) \
             FROM credential_challenge_votes WHERE challenge_id = ?1",
            params![challenge_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let total = uphold + reject;
    let upheld = total > 0 && SUPERMAJORITY_DEN * uphold >= SUPERMAJORITY_NUM * total;

    let credential_revoked = if upheld {
        let credential_id: String = conn
            .query_row(
                "SELECT credential_id FROM credential_challenges WHERE id = ?1",
                params![challenge_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        revoke_credential(conn, &credential_id)?;
        true
    } else {
        false
    };

    let new_status = if upheld { "upheld" } else { "rejected" };
    conn.execute(
        "UPDATE credential_challenges \
         SET status = ?1, resolved_at = datetime('now') \
         WHERE id = ?2",
        params![new_status, challenge_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(ChallengeResolution {
        challenge_id: challenge_id.to_string(),
        status: new_status.to_string(),
        votes_for_uphold: uphold,
        votes_for_reject: reject,
        credential_revoked,
    })
}

/// Mark any reviewing challenge past its deadline as `expired`. Does
/// not revoke anything — the challenger simply forfeits the stake by
/// inaction (that's governance's problem, not ours).
pub fn expire_overdue(conn: &Connection) -> Result<usize, String> {
    let rows = conn
        .execute(
            "UPDATE credential_challenges \
             SET status = 'expired', resolved_at = datetime('now') \
             WHERE status IN ('pending', 'reviewing') \
               AND expires_at IS NOT NULL \
               AND expires_at < datetime('now')",
            [],
        )
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Fetch a single challenge row.
pub fn get_challenge(
    conn: &Connection,
    challenge_id: &str,
) -> Result<Option<CredentialChallenge>, String> {
    conn.query_row(
        "SELECT id, challenger, credential_id, reason, stake_lovelace, \
                stake_tx_hash, status, dao_id, resolution_tx, signature, \
                created_at, resolved_at, expires_at \
         FROM credential_challenges WHERE id = ?1",
        params![challenge_id],
        row_to_challenge,
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// List challenges. Filters are optional; ordering is newest first.
pub fn list_challenges(
    conn: &Connection,
    status_filter: Option<&str>,
    credential_id: Option<&str>,
) -> Result<Vec<CredentialChallenge>, String> {
    let mut conditions: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut i = 1;
    if let Some(s) = status_filter {
        conditions.push(format!("status = ?{i}"));
        values.push(Box::new(s.to_string()));
        i += 1;
    }
    if let Some(c) = credential_id {
        conditions.push(format!("credential_id = ?{i}"));
        values.push(Box::new(c.to_string()));
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT id, challenger, credential_id, reason, stake_lovelace, \
                stake_tx_hash, status, dao_id, resolution_tx, signature, \
                created_at, resolved_at, expires_at \
         FROM credential_challenges {where_clause} \
         ORDER BY created_at DESC"
    );
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_ref.as_slice(), row_to_challenge)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

// ---------- internal ----------

fn load_status(conn: &Connection, challenge_id: &str) -> Result<ChallengeStatus, String> {
    let status_str: String = conn
        .query_row(
            "SELECT status FROM credential_challenges WHERE id = ?1",
            params![challenge_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("challenge not found: {e}"))?;
    ChallengeStatus::from_str(&status_str)
        .ok_or_else(|| format!("unexpected status in DB: {status_str}"))
}

/// Flip the credential's status-list bit to mark it revoked.
/// Minimal-viable version — mirrors what
/// `commands::credentials::revoke_credential_impl` does for the
/// in-scope columns. A future refactor could share a single helper.
fn revoke_credential(conn: &Connection, credential_id: &str) -> Result<(), String> {
    let (list_id, index): (Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT status_list_id, status_list_index FROM credentials WHERE id = ?1",
            params![credential_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE credentials \
         SET revoked = 1, \
             revoked_at = datetime('now'), \
             revocation_reason = 'challenge_upheld' \
         WHERE id = ?1",
        params![credential_id],
    )
    .map_err(|e| e.to_string())?;

    if let (Some(list_id), Some(index)) = (list_id, index) {
        // Load the bitmap, flip the bit, write back.
        let (mut bits, bit_length): (Vec<u8>, i64) = conn
            .query_row(
                "SELECT bits, bit_length FROM credential_status_lists \
                 WHERE list_id = ?1",
                params![list_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if index < 0 || index >= bit_length {
            return Err(format!(
                "status_list_index {index} out of range for list {list_id}"
            ));
        }
        let byte = (index / 8) as usize;
        let bit = (index % 8) as u8;
        if byte < bits.len() {
            bits[byte] |= 1 << bit;
        }
        conn.execute(
            "UPDATE credential_status_lists \
             SET bits = ?1, version = version + 1, updated_at = datetime('now') \
             WHERE list_id = ?2",
            params![bits, list_id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn row_to_challenge(row: &rusqlite::Row) -> rusqlite::Result<CredentialChallenge> {
    Ok(CredentialChallenge {
        id: row.get(0)?,
        challenger: row.get(1)?,
        credential_id: row.get(2)?,
        reason: row.get(3)?,
        stake_lovelace: row.get(4)?,
        stake_tx_hash: row.get(5)?,
        status: row.get(6)?,
        dao_id: row.get(7)?,
        resolution_tx: row.get(8)?,
        signature: row.get(9)?,
        created_at: row.get(10)?,
        resolved_at: row.get(11)?,
        expires_at: row.get(12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        // Seed a status list + a credential to challenge.
        db.conn()
            .execute(
                "INSERT INTO credential_status_lists \
                   (list_id, issuer_did, version, status_purpose, bits, bit_length) \
                 VALUES ('list:1', 'did:issuer', 1, 'revocation', zeroblob(2048), 16384)",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO credentials ( \
                   id, issuer_did, subject_did, credential_type, claim_kind, \
                   issuance_date, signed_vc_json, integrity_hash, \
                   status_list_id, status_list_index, revoked \
                 ) VALUES ('cred_a', 'did:issuer', 'did:subject', \
                           'FormalCredential', 'skill', datetime('now'), \
                           '{}', 'h', 'list:1', 5, 0)",
                [],
            )
            .unwrap();
        db
    }

    fn submit(
        db: &Database,
        challenger: &str,
        credential_id: &str,
        stake: i64,
    ) -> CredentialChallenge {
        submit_challenge(
            db.conn(),
            &SubmitCredentialChallengeParams {
                credential_id: credential_id.into(),
                reason: "the grader deterministically accepted an empty submission".into(),
                stake_lovelace: stake,
                dao_id: "dao_cs".into(),
            },
            challenger,
            "sig_hex",
        )
        .unwrap()
    }

    #[test]
    fn rejects_short_reason() {
        let db = test_db();
        let err = submit_challenge(
            db.conn(),
            &SubmitCredentialChallengeParams {
                credential_id: "cred_a".into(),
                reason: "nope".into(),
                stake_lovelace: 5_000_000,
                dao_id: "d".into(),
            },
            "stake_challenger",
            "sig",
        )
        .unwrap_err();
        assert!(err.contains("reason"));
    }

    #[test]
    fn rejects_insufficient_stake() {
        let db = test_db();
        let err = submit_challenge(
            db.conn(),
            &SubmitCredentialChallengeParams {
                credential_id: "cred_a".into(),
                reason: "long enough reason here".into(),
                stake_lovelace: 1_000_000,
                dao_id: "d".into(),
            },
            "stake_challenger",
            "sig",
        )
        .unwrap_err();
        assert!(err.contains("stake"));
    }

    #[test]
    fn rejects_already_revoked_credential() {
        let db = test_db();
        db.conn()
            .execute("UPDATE credentials SET revoked = 1 WHERE id = 'cred_a'", [])
            .unwrap();
        let err = submit_challenge(
            db.conn(),
            &SubmitCredentialChallengeParams {
                credential_id: "cred_a".into(),
                reason: "long enough reason here".into(),
                stake_lovelace: 5_000_000,
                dao_id: "d".into(),
            },
            "stake_challenger",
            "sig",
        )
        .unwrap_err();
        assert!(err.contains("already revoked"));
    }

    #[test]
    fn vote_flips_to_reviewing_and_prevents_duplicates() {
        let db = test_db();
        let ch = submit(&db, "stake_c1", "cred_a", 5_000_000);
        vote(db.conn(), &ch.id, "voter_a", true, None).unwrap();
        // Now in reviewing.
        let row = get_challenge(db.conn(), &ch.id).unwrap().unwrap();
        assert_eq!(row.status, "reviewing");
        // Duplicate vote rejected.
        let err = vote(db.conn(), &ch.id, "voter_a", false, None).unwrap_err();
        assert!(err.contains("already voted"));
    }

    #[test]
    fn supermajority_upheld_revokes_credential() {
        let db = test_db();
        let ch = submit(&db, "c1", "cred_a", 5_000_000);
        // 2 uphold, 1 reject → 2/3 uphold → upheld.
        vote(db.conn(), &ch.id, "v1", true, None).unwrap();
        vote(db.conn(), &ch.id, "v2", true, None).unwrap();
        vote(db.conn(), &ch.id, "v3", false, None).unwrap();
        let outcome = resolve(db.conn(), &ch.id).unwrap();
        assert_eq!(outcome.status, "upheld");
        assert!(outcome.credential_revoked);

        // Credential is now marked revoked AND the status-list bit is set.
        let (revoked, list_id, index): (i64, String, i64) = db
            .conn()
            .query_row(
                "SELECT revoked, status_list_id, status_list_index \
                 FROM credentials WHERE id = 'cred_a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(revoked, 1);

        let bits: Vec<u8> = db
            .conn()
            .query_row(
                "SELECT bits FROM credential_status_lists WHERE list_id = ?1",
                params![list_id],
                |row| row.get(0),
            )
            .unwrap();
        let byte = (index / 8) as usize;
        let bit = (index % 8) as u8;
        assert_eq!(bits[byte] & (1 << bit), 1 << bit);
    }

    #[test]
    fn majority_not_supermajority_is_rejected() {
        let db = test_db();
        let ch = submit(&db, "c1", "cred_a", 5_000_000);
        // 1 uphold, 1 reject → 1/2 < 2/3 → rejected.
        vote(db.conn(), &ch.id, "v1", true, None).unwrap();
        vote(db.conn(), &ch.id, "v2", false, None).unwrap();
        let outcome = resolve(db.conn(), &ch.id).unwrap();
        assert_eq!(outcome.status, "rejected");
        assert!(!outcome.credential_revoked);
    }

    #[test]
    fn expire_overdue_only_touches_reviewing() {
        let db = test_db();
        let ch = submit(&db, "c1", "cred_a", 5_000_000);
        // Retroactively push expires_at into the past.
        db.conn()
            .execute(
                "UPDATE credential_challenges \
                 SET expires_at = datetime('now', '-40 days') WHERE id = ?1",
                params![ch.id],
            )
            .unwrap();
        let count = expire_overdue(db.conn()).unwrap();
        assert_eq!(count, 1);
        let row = get_challenge(db.conn(), &ch.id).unwrap().unwrap();
        assert_eq!(row.status, "expired");

        // Calling again on a terminal state does nothing.
        let next = expire_overdue(db.conn()).unwrap();
        assert_eq!(next, 0);
    }
}
