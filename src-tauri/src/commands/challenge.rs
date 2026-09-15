//! IPC for the credential-challenge mechanism (VC-first rebuild).

use crate::profile::scope::ProfileState as State;

use crate::cardano::escrow_recovery::{self, EscrowAction, EscrowContext};
use crate::cardano::submission::{self, Operation};
use crate::cardano::{blockfrost::BlockfrostClient, challenge_escrow_tx_builder};
use crate::crypto::hash::blake2b_256;
use crate::db::{executor::DatabaseWorkload, Database};
use crate::domain::challenge::{
    ChallengeResolution, ChallengeVote, CredentialChallenge, SubmitCredentialChallengeParams,
};
use crate::evidence::challenge as challenge_logic;
use crate::AppState;

async fn challenge_db<T, F>(
    state: &State<'_, AppState>,
    workload: DatabaseWorkload,
    label: &'static str,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
{
    state
        .db_executor
        .execute(workload, state.profile_lease(), label, operation)
        .await
}

fn challenge_transaction<T>(
    db: &Database,
    operation: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    let transaction =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
    let result = operation(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

fn local_stake_address(connection: &rusqlite::Connection) -> Result<String, String> {
    connection
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("local identity not initialized: {error}"))
}

/// Parse a hex-encoded 28-byte payment key hash.
fn parse_key_hash(hex_str: &str) -> Result<[u8; 28], String> {
    let bytes = hex::decode(hex_str.trim()).map_err(|e| format!("invalid key hash hex: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| "key hash must be 28 bytes".to_string())
}

#[tauri::command]
pub async fn submit_credential_challenge(
    state: State<'_, AppState>,
    params: SubmitCredentialChallengeParams,
) -> Result<CredentialChallenge, String> {
    challenge_db(
        &state,
        DatabaseWorkload::Instructor,
        "challenge.submit",
        move |db| {
            challenge_transaction(db, |connection| {
                let challenger = local_stake_address(connection)?;
                // The stored signature remains legacy metadata. Production
                // authority must come from the replacement committee protocol.
                challenge_logic::submit_challenge(
                    connection,
                    &params,
                    &challenger,
                    "ipc-placeholder-sig",
                )
            })
        },
    )
    .await
}

#[tauri::command]
pub async fn vote_on_credential_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
    upheld: bool,
    reason: Option<String>,
) -> Result<ChallengeVote, String> {
    challenge_db(
        &state,
        DatabaseWorkload::Instructor,
        "challenge.vote",
        move |db| {
            challenge_transaction(db, |connection| {
                let voter = local_stake_address(connection)?;
                challenge_logic::vote(connection, &challenge_id, &voter, upheld, reason.as_deref())
            })
        },
    )
    .await
}

#[tauri::command]
pub async fn resolve_credential_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
) -> Result<ChallengeResolution, String> {
    challenge_db(
        &state,
        DatabaseWorkload::Instructor,
        "challenge.resolve",
        move |db| {
            challenge_transaction(db, |connection| {
                challenge_logic::resolve(connection, &challenge_id)
            })
        },
    )
    .await
}

#[tauri::command]
pub async fn list_credential_challenges(
    state: State<'_, AppState>,
    status: Option<String>,
    credential_id: Option<String>,
) -> Result<Vec<CredentialChallenge>, String> {
    challenge_db(
        &state,
        DatabaseWorkload::Learner,
        "challenge.list",
        move |db| {
            challenge_logic::list_challenges(db.conn(), status.as_deref(), credential_id.as_deref())
        },
    )
    .await
}

#[tauri::command]
pub async fn get_credential_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
) -> Result<Option<CredentialChallenge>, String> {
    challenge_db(
        &state,
        DatabaseWorkload::Learner,
        "challenge.get",
        move |db| challenge_logic::get_challenge(db.conn(), &challenge_id),
    )
    .await
}

#[tauri::command]
pub async fn expire_overdue_credential_challenges(
    state: State<'_, AppState>,
) -> Result<usize, String> {
    challenge_db(
        &state,
        DatabaseWorkload::Background,
        "challenge.expire-overdue",
        move |db| challenge_logic::expire_overdue(db.conn()),
    )
    .await
}

/// Build helper: derive the local wallet from the unlocked vault and a
/// Blockfrost client from the environment. Shared by stake lock/settle.
async fn wallet_and_blockfrost(
    state: &State<'_, AppState>,
) -> Result<(crate::crypto::wallet::Wallet, BlockfrostClient), String> {
    let ks_guard = state.keystore.lock().await;
    let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(ks_guard);
    let wallet =
        crate::crypto::wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    let project_id = challenge_db(
        state,
        DatabaseWorkload::Learner,
        "challenge.cardano-project-id",
        move |db| {
            Ok(crate::cardano::blockfrost::resolve_project_id(Some(
                db.conn(),
            )))
        },
    )
    .await?
    .ok_or(
        "Blockfrost project id not configured \
         (set in Settings → Cardano, or export BLOCKFROST_PROJECT_ID)",
    )?;
    let bf = BlockfrostClient::new(project_id).map_err(|e| e.to_string())?;
    Ok((wallet, bf))
}

/// Lock the challenger's stake at the escrow script for a challenge.
///
/// Works today on preprod (a plain pay-to-script); spending it later
/// needs the escrow validator deployed. `treasury_key_hash` and
/// `dao_authority_key_hash` (hex, 28 bytes) are stored in the escrow
/// datum; both default to the challenger's own key when omitted (solo
/// operator / testing).
#[tauri::command]
pub async fn lock_challenge_stake(
    state: State<'_, AppState>,
    challenge_id: String,
    treasury_key_hash: Option<String>,
    dao_authority_key_hash: Option<String>,
) -> Result<String, String> {
    let operation = Operation {
        kind: escrow_recovery::LOCK_KIND,
        id: &challenge_id,
    };
    let challenge_id_for_read = challenge_id.clone();
    let (existing_response, stake) = challenge_db(
        &state,
        DatabaseWorkload::Learner,
        "challenge.lock-stake-read",
        move |db| {
            let operation = Operation {
                kind: escrow_recovery::LOCK_KIND,
                id: &challenge_id_for_read,
            };
            let existing = escrow_recovery::existing_response(db.conn(), operation)?;
            let stake = existing
                .is_none()
                .then(|| challenge_logic::get_stake_info(db.conn(), &challenge_id_for_read))
                .transpose()?;
            Ok((existing, stake))
        },
    )
    .await?;
    if let Some(hash) = existing_response {
        return Ok(hash);
    }
    let stake = stake.ok_or("challenge stake lookup produced no result")?;
    if stake.stake_status != "none" {
        return Err(format!(
            "challenge stake already {} — cannot re-lock",
            stake.stake_status
        ));
    }

    let (wallet, bf) = wallet_and_blockfrost(&state).await?;
    let treasury = match treasury_key_hash {
        Some(h) => parse_key_hash(&h)?,
        None => wallet.payment_key_hash,
    };
    let authority = match dao_authority_key_hash {
        Some(h) => parse_key_hash(&h)?,
        None => wallet.payment_key_hash,
    };
    let challenge_id_hash = blake2b_256(challenge_id.as_bytes());
    let stake_lovelace =
        u64::try_from(stake.stake_lovelace).map_err(|_| "negative escrow stake")?;

    let result = challenge_escrow_tx_builder::build_lock_tx(
        &bf,
        &wallet.payment_address,
        &wallet.payment_key_hash,
        &wallet.payment_key_extended,
        &treasury,
        &authority,
        &challenge_id_hash,
        stake_lovelace,
    )
    .await
    .map_err(|e| e.to_string())?;

    let context = EscrowContext {
        version: 1,
        challenge_id: challenge_id.clone(),
        stake_lovelace,
        action: EscrowAction::Lock {
            challenger_pkh: hex::encode(wallet.payment_key_hash),
            treasury_pkh: hex::encode(treasury),
            authority_pkh: hex::encode(authority),
        },
    };
    let context_json = serde_json::to_string(&context).map_err(|e| e.to_string())?;
    let submitted =
        submission::submit_once(&state.db, &bf, operation, &result.tx_cbor, &context_json).await?;
    let challenge_id_for_projection = challenge_id.clone();
    let submitted_for_projection = submitted.clone();
    challenge_db(
        &state,
        DatabaseWorkload::Instructor,
        "challenge.lock-stake-project",
        move |db| {
            escrow_recovery::project(
                db.conn(),
                Operation {
                    kind: escrow_recovery::LOCK_KIND,
                    id: &challenge_id_for_projection,
                },
                &submitted_for_projection,
            )
        },
    )
    .await?;
    escrow_recovery::response(submitted)
}

/// Settle a resolved challenge's escrowed stake. The DAO authority
/// spends the escrow UTxO, refunding the challenger (upheld) or
/// forfeiting to the treasury (rejected). Gated on the escrow validator
/// being deployed. The destination is read from what the lock tx recorded
/// (challenger pkh on refund, treasury pkh on forfeit), never from the caller.
#[tauri::command]
pub async fn settle_challenge_stake(
    state: State<'_, AppState>,
    challenge_id: String,
    escrow_tx_hash: String,
    escrow_index: u64,
) -> Result<String, String> {
    let operation = Operation {
        kind: escrow_recovery::SETTLE_KIND,
        id: &challenge_id,
    };
    let challenge_id_for_read = challenge_id.clone();
    let (existing_response, stake) = challenge_db(
        &state,
        DatabaseWorkload::Learner,
        "challenge.settle-stake-read",
        move |db| {
            let operation = Operation {
                kind: escrow_recovery::SETTLE_KIND,
                id: &challenge_id_for_read,
            };
            let existing = escrow_recovery::existing_response(db.conn(), operation)?;
            let stake = existing
                .is_none()
                .then(|| challenge_logic::get_stake_info(db.conn(), &challenge_id_for_read))
                .transpose()?;
            Ok((existing, stake))
        },
    )
    .await?;
    if let Some(hash) = existing_response {
        return Ok(hash);
    }
    let stake = stake.ok_or("challenge stake lookup produced no result")?;
    if stake.stake_status != "locked" {
        return Err(format!(
            "challenge stake is '{}', expected 'locked'",
            stake.stake_status
        ));
    }
    if stake.lock_tx_hash.as_deref() != Some(escrow_tx_hash.as_str()) || escrow_index != 0 {
        return Err("settlement must spend output 0 of the recorded lock transaction".into());
    }
    let refund = match stake.challenge_status.as_str() {
        "upheld" => true,
        "rejected" => false,
        other => {
            return Err(format!(
                "challenge not resolved (status: {other}) — settle after resolution"
            ))
        }
    };

    // The recipient is whatever the escrow datum says it is, and the datum
    // was written by *this* app at lock time — so read it back rather than
    // asking the frontend to repeat it. The on-chain validator would reject a
    // wrong recipient anyway, but a settle that cannot be misdirected is
    // better than one the ledger has to catch.
    let recipient_hex = if refund {
        stake.escrow_challenger_pkh.as_deref()
    } else {
        stake.escrow_treasury_pkh.as_deref()
    }
    .ok_or(
        "this stake was locked before its escrow recipients were recorded — \
         re-lock it, or settle it with a manually built transaction",
    )?;
    let recipient = parse_key_hash(recipient_hex)?;
    let (wallet, bf) = wallet_and_blockfrost(&state).await?;
    let stake_lovelace =
        u64::try_from(stake.stake_lovelace).map_err(|_| "negative escrow stake")?;

    let result = challenge_escrow_tx_builder::build_settle_tx(
        &bf,
        &wallet.payment_address,
        &wallet.payment_key_hash,
        &wallet.payment_key_extended,
        (&escrow_tx_hash, escrow_index),
        &recipient,
        stake_lovelace,
        refund,
    )
    .await
    .map_err(|e| e.to_string())?;

    let context = EscrowContext {
        version: 1,
        challenge_id: challenge_id.clone(),
        stake_lovelace,
        action: EscrowAction::Settle {
            escrow_tx_hash,
            escrow_index,
            recipient_pkh: hex::encode(recipient),
            refund,
        },
    };
    let context_json = serde_json::to_string(&context).map_err(|e| e.to_string())?;
    let submitted =
        submission::submit_once(&state.db, &bf, operation, &result.tx_cbor, &context_json).await?;
    let challenge_id_for_projection = challenge_id.clone();
    let submitted_for_projection = submitted.clone();
    challenge_db(
        &state,
        DatabaseWorkload::Instructor,
        "challenge.settle-stake-project",
        move |db| {
            escrow_recovery::project(
                db.conn(),
                Operation {
                    kind: escrow_recovery::SETTLE_KIND,
                    id: &challenge_id_for_projection,
                },
                &submitted_for_projection,
            )
        },
    )
    .await?;
    escrow_recovery::response(submitted)
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use super::*;
    use crate::domain::challenge::MIN_STAKE_LOVELACE;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open database");
        db.run_migrations().expect("run migrations");
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test', 'addr_test')",
                [],
            )
            .expect("seed identity");
        db.conn()
            .execute(
                "INSERT INTO credential_status_lists \
                   (list_id, issuer_did, version, status_purpose, bits, bit_length) \
                 VALUES ('list:test', 'did:issuer', 1, 'revocation', zeroblob(8), 64)",
                [],
            )
            .expect("seed status list");
        db.conn()
            .execute(
                "INSERT INTO credentials ( \
                   id, issuer_did, subject_did, credential_type, claim_kind, \
                   issuance_date, signed_vc_json, integrity_hash, \
                   status_list_id, status_list_index, revoked \
                 ) VALUES ('cred_test', 'did:issuer', 'did:subject', \
                           'FormalCredential', 'skill', datetime('now'), \
                           '{}', 'integrity', 'list:test', 5, 0)",
                [],
            )
            .expect("seed credential");
        db
    }

    fn submit(db: &Database) -> CredentialChallenge {
        let request = SubmitCredentialChallengeParams {
            credential_id: "cred_test".into(),
            reason: "the submitted evidence is inconsistent with the result".into(),
            stake_lovelace: i64::try_from(MIN_STAKE_LOVELACE).expect("stake fits SQLite"),
            dao_id: "dao_test".into(),
        };
        challenge_transaction(db, |connection| {
            challenge_logic::submit_challenge(
                connection,
                &request,
                "stake_test",
                "legacy-test-signature",
            )
        })
        .expect("submit challenge")
    }

    #[test]
    fn vote_and_status_transition_roll_back_together() {
        let db = test_db();
        let challenge = submit(&db);
        db.conn()
            .execute_batch(
                "CREATE TRIGGER fail_challenge_reviewing \
                 BEFORE UPDATE OF status ON credential_challenges \
                 WHEN NEW.status = 'reviewing' \
                 BEGIN SELECT RAISE(ABORT, 'injected status failure'); END;",
            )
            .expect("install failure trigger");

        let error = challenge_transaction(&db, |connection| {
            challenge_logic::vote(connection, &challenge.id, "voter_test", true, None)
        })
        .expect_err("status failure must abort vote");
        assert!(error.contains("injected status failure"));

        let vote_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credential_challenge_votes WHERE challenge_id = ?1",
                [&challenge.id],
                |row| row.get(0),
            )
            .expect("count votes");
        assert_eq!(vote_count, 0);
        assert_eq!(
            challenge_logic::get_challenge(db.conn(), &challenge.id)
                .expect("load challenge")
                .expect("challenge exists")
                .status,
            "pending"
        );
    }

    #[test]
    fn credential_revocation_and_resolution_roll_back_together() {
        let db = test_db();
        let challenge = submit(&db);
        for voter in ["voter_a", "voter_b"] {
            challenge_transaction(&db, |connection| {
                challenge_logic::vote(connection, &challenge.id, voter, true, None)
            })
            .expect("record vote");
        }
        db.conn()
            .execute_batch(
                "CREATE TRIGGER fail_challenge_resolution \
                 BEFORE UPDATE OF status ON credential_challenges \
                 WHEN NEW.status = 'upheld' \
                 BEGIN SELECT RAISE(ABORT, 'injected resolution failure'); END;",
            )
            .expect("install failure trigger");

        let error = challenge_transaction(&db, |connection| {
            challenge_logic::resolve(connection, &challenge.id)
        })
        .expect_err("resolution failure must roll back revocation");
        assert!(error.contains("injected resolution failure"));

        let revoked: i64 = db
            .conn()
            .query_row(
                "SELECT revoked FROM credentials WHERE id = 'cred_test'",
                [],
                |row| row.get(0),
            )
            .expect("load credential status");
        assert_eq!(revoked, 0);
        let bits: Vec<u8> = db
            .conn()
            .query_row(
                "SELECT bits FROM credential_status_lists WHERE list_id = 'list:test'",
                [],
                |row| row.get(0),
            )
            .expect("load status list");
        assert_eq!(bits[0] & (1 << 5), 0);
        let status: String = db
            .conn()
            .query_row(
                "SELECT status FROM credential_challenges WHERE id = ?1",
                params![challenge.id],
                |row| row.get(0),
            )
            .expect("load challenge status");
        assert_eq!(status, "reviewing");
    }
}
