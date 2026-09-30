use ed25519_dalek::{SigningKey, VerifyingKey};
use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    canonical, decode, validate_challenge, Policy, Receipt, SignedChallenge, VerifiedSubmission,
};

pub const SCHEMA: &str = include_str!("schema.sql");
pub type StoreResult<T> = std::result::Result<T, String>;

pub fn prepare(db: &Connection, session: &str, challenge: &SignedChallenge) -> StoreResult<()> {
    let c = &challenge.body;
    let issued = i64::try_from(c.issued_at).map_err(|e| e.to_string())?;
    let expires = i64::try_from(c.expires_at).map_err(|e| e.to_string())?;
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let recent: i64 = tx
        .query_row(
            "SELECT count(*) FROM personhood_private_challenges WHERE created_at >= ?1",
            [issued.saturating_sub(60)],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if recent >= 5 {
        return Err("Too many receipt attempts; wait a minute".into());
    }
    let count: i64 = tx
        .query_row(
            "SELECT count(*) FROM personhood_private_challenges WHERE state = 'consumed'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if count >= 100 {
        return Err("This developer profile has reached its 100 receipt limit".into());
    }
    tx.execute(
        "DELETE FROM personhood_private_challenges WHERE state != 'consumed' AND expires_at < ?1",
        [issued.saturating_sub(60)],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE personhood_private_challenges SET state = 'cancelled' WHERE state = 'pending'",
        [],
    )
    .map_err(|e| e.to_string())?;
    let encoded = String::from_utf8(canonical(challenge).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO personhood_private_challenges(nonce, session_id, subject_did, network_id, challenge_json, created_at, expires_at, state) VALUES (?1,?2,?3,?4,?5,?6,?7,'pending')", params![c.nonce,session,c.subject_did,c.network_id,encoded,issued,expires]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn pending(
    db: &Connection,
    session: &str,
    nonce: &str,
    policy: &Policy,
    account: &VerifyingKey,
    now: u64,
) -> StoreResult<SignedChallenge> {
    let encoded: Option<String> = db.query_row("SELECT challenge_json FROM personhood_private_challenges WHERE nonce = ?1 AND session_id = ?2 AND state = 'pending'", params![nonce,session], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
    let challenge: SignedChallenge = decode(
        encoded
            .ok_or("Challenge is cancelled, consumed, or belongs to another session")?
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    validate_challenge(policy, &challenge, account, now).map_err(|e| e.to_string())?;
    Ok(challenge)
}

pub fn cancel(db: &Connection, session: &str, nonce: &str) -> StoreResult<()> {
    db.execute("UPDATE personhood_private_challenges SET state = 'cancelled' WHERE nonce = ?1 AND session_id = ?2 AND state = 'pending'", params![nonce,session]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn existing(
    db: &Connection,
    session: &str,
    nonce: &str,
    account: &VerifyingKey,
    policy: &Policy,
) -> StoreResult<Option<Receipt>> {
    let subject = alexandria_verify::did::did_from_verifying_key(account).0;
    let encoded: Option<String> = db.query_row("SELECT receipt_json FROM personhood_private_challenges WHERE nonce = ?1 AND session_id = ?2 AND subject_did = ?3 AND network_id = ?4 AND state = 'consumed'", params![nonce,session,subject,policy.network_id], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
    encoded
        .map(|json| {
            let receipt: Receipt = decode(json.as_bytes()).map_err(|e| e.to_string())?;
            crate::verify_receipt(
                &receipt,
                &crate::key(&policy.verifier_public_key).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if receipt.body.id != nonce
                || receipt.body.subject_did != subject
                || receipt.body.network_id != policy.network_id
            {
                return Err("Stored receipt context mismatch".into());
            }
            Ok(receipt)
        })
        .transpose()
}

// Caller owns the transaction and commits under its profile-session admission gate.
pub fn consume(
    db: &rusqlite::Transaction<'_>,
    session: &str,
    verified: &VerifiedSubmission,
    policy: &Policy,
    account: &VerifyingKey,
    verifier: &SigningKey,
    now: u64,
) -> StoreResult<Receipt> {
    if let Some(receipt) = existing(
        db,
        session,
        &verified.challenge().body.nonce,
        account,
        policy,
    )? {
        if receipt.body.submission_digest
            != verified.submission_digest().map_err(|e| e.to_string())?
        {
            return Err("Challenge was consumed by a different submission".into());
        }
        return Ok(receipt);
    }
    let challenge = pending(
        db,
        session,
        &verified.challenge().body.nonce,
        policy,
        account,
        now,
    )?;
    if &challenge != verified.challenge() {
        return Err("Stored challenge changed".into());
    }
    let receipt = verified
        .receipt(policy, account, verifier, now)
        .map_err(|e| e.to_string())?;
    let json = String::from_utf8(canonical(&receipt).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let changed = db.execute("UPDATE personhood_private_challenges SET state = 'consumed', submission_digest = ?1, receipt_json = ?2 WHERE nonce = ?3 AND session_id = ?4 AND state = 'pending'", params![receipt.body.submission_digest,json,challenge.body.nonce,session]).map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("Challenge already consumed".into());
    }
    Ok(receipt)
}

pub fn receipts(db: &Connection, subject: &str, network: &str) -> StoreResult<Vec<Receipt>> {
    let mut query = db.prepare("SELECT receipt_json FROM personhood_private_challenges WHERE state = 'consumed' AND subject_did = ?1 AND network_id = ?2 ORDER BY created_at DESC LIMIT 20").map_err(|e| e.to_string())?;
    let rows = query
        .query_map(params![subject, network], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|row| decode(row.map_err(|e| e.to_string())?.as_bytes()).map_err(|e| e.to_string()))
        .collect()
}
