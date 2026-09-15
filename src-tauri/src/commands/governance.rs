//! IPC commands for DAO governance.
//!
//! Exposes the full governance lifecycle to the frontend:
//!   - DAO management (list, create, get with members)
//!   - Election lifecycle (open, nominate, accept, start voting, vote, finalize, install)
//!   - Proposal lifecycle (submit, approve, cancel, vote, resolve)
//!
//! Port of `api/internal/handler/governance.go` (20 endpoints) adapted for
//! local-first operation. Committee/admin checks use local identity.

use crate::profile::scope::ProfileState as State;
use rusqlite::{params, Transaction, TransactionBehavior};

use std::str::FromStr;

use crate::cardano::onchain_queue;
use crate::crypto::did::{derive_did_key, Did};
use crate::crypto::hash::entity_id;
use crate::crypto::wallet;
use crate::db::executor::DatabaseWorkload;
use crate::db::governance::{record_election_vote, record_proposal_vote, VoteEvidence};
use crate::domain::bloom::BloomLevel;
use crate::domain::governance::{
    DaoInfo, DaoMember, Election, ElectionNominee, ElectionVote, GovernanceAnnouncement,
    GovernanceEventType, OpenElectionParams, Proposal, ProposalVote, SubmitProposalParams,
};
use crate::AppState;

async fn governance_db<T, F>(
    state: &State<'_, AppState>,
    workload: DatabaseWorkload,
    label: &'static str,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&crate::db::Database) -> Result<T, String> + Send + 'static,
{
    state
        .db_executor
        .execute(workload, state.profile_lease(), label, operation)
        .await
}

fn governance_transaction<T>(
    db: &crate::db::Database,
    operation: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    let transaction = Transaction::new_unchecked(db.conn(), TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let result = operation(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

/// Sign a governance event with the local wallet and broadcast it on the
/// governance gossip topic (best-effort). Returns the signed envelope so
/// callers can persist its signature + public key alongside the local
/// row. Signing is synchronous; the actual broadcast is awaited after the
/// caller has dropped its DB lock.
fn sign_governance_event(
    w: &wallet::Wallet,
    dao_id: &str,
    event: GovernanceEventType,
) -> Result<crate::p2p::types::SignedGossipMessage, String> {
    let ann = GovernanceAnnouncement {
        event_type: event,
        dao_id: dao_id.to_string(),
        timestamp: chrono::Utc::now().timestamp(),
    };
    let payload = serde_json::to_vec(&ann).map_err(|e| e.to_string())?;
    Ok(crate::p2p::signing::sign_gossip_message(
        crate::p2p::types::TOPIC_GOVERNANCE,
        payload,
        &w.signing_key,
        &w.stake_address,
    ))
}

/// Broadcast an already-signed gossip message via the P2P node, if one is
/// running. Best-effort — never fails the parent command.
async fn broadcast_signed(state: &AppState, signed: &crate::p2p::types::SignedGossipMessage) {
    let node = state.p2p_node.lock().await;
    if let Some(ref n) = *node {
        if let Err(e) = n.publish_signed(signed).await {
            log::warn!("governance gossip broadcast failed: {e}");
        }
    }
}

/// Enforce that the local identity is a registered Alexandria user.
///
/// Participating in governance (opening elections, nominating, accepting,
/// starting/finalizing, voting) requires the wallet's stake address to be
/// bound to its signing pubkey in the `stake_pubkey_registry` — the same
/// binding peers check before accepting the gossiped action. Checking it
/// up-front gives a clear error instead of a silent drop on the receiver
/// side. Being a registered user is a hard requirement to act on anything
/// in Alexandria governance.
fn ensure_registered(conn: &rusqlite::Connection, w: &wallet::Wallet) -> Result<(), String> {
    let pubkey_hex = hex::encode(w.signing_key.verifying_key().to_bytes());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let registered = crate::p2p::registry::lookup(conn, &w.stake_address, &pubkey_hex, now)
        .map_err(|e| format!("registry lookup failed: {e}"))?;
    if !registered {
        return Err(
            "you must be a registered Alexandria user to participate in governance — \
             register this device's signing key (stake-pubkey registry) first"
                .into(),
        );
    }
    Ok(())
}

/// Load the local wallet from the unlocked vault (needed to sign
/// governance actions). Errors if the vault is locked.
async fn load_wallet(state: &AppState) -> Result<wallet::Wallet, String> {
    let keystore = state.keystore.lock().await;
    let ks = keystore.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(keystore);
    wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())
}

/// Default proposal voting deadline: 14 days from approval.
const DEFAULT_VOTING_DAYS: i64 = 14;

/// Evaluate the approved two-thirds proposal threshold exactly.
fn has_two_thirds(votes_for: i64, votes_against: i64) -> Result<bool, String> {
    if votes_for < 0 || votes_against < 0 {
        return Err("proposal tallies cannot be negative".into());
    }
    let votes_for = i128::from(votes_for);
    let total = votes_for + i128::from(votes_against);
    Ok(total > 0 && 3 * votes_for >= 2 * total)
}

fn select_unambiguous_winners(
    ranked_nominees: &[(String, i64)],
    seats: i64,
) -> Result<Vec<String>, String> {
    let seats =
        usize::try_from(seats).map_err(|_| "election seat count cannot be negative".to_string())?;
    if seats == 0 {
        return Err("election must have at least one seat".into());
    }
    if ranked_nominees.len() < seats {
        return Err(format!(
            "election has {} accepted nominees for {seats} seats",
            ranked_nominees.len()
        ));
    }
    if ranked_nominees
        .get(seats)
        .is_some_and(|next| next.1 == ranked_nominees[seats - 1].1)
    {
        return Err(
            "the final committee seat is tied; a certified runoff is required before finalization"
                .into(),
        );
    }
    Ok(ranked_nominees
        .iter()
        .take(seats)
        .map(|(id, _)| id.clone())
        .collect())
}

/// Check if the authenticated signer's DID has at least `min_level` proficiency for any
/// skill within the scope of the given DAO.
///
/// Returns Ok(()) if the check passes, or an Err with a human-readable
/// message if the user lacks sufficient proficiency.
fn check_proficiency(
    conn: &rusqlite::Connection,
    subject_did: &Did,
    dao_id: &str,
    min_level: &str,
) -> Result<(), String> {
    // Invalid configuration is not permission to use the weakest level.
    let min_idx = BloomLevel::from_str(min_level)
        .map_err(|_| format!("Invalid governance proficiency requirement: '{min_level}'"))?
        .rank();

    // Find the DAO's scope (subject_field or subject) to determine which
    // skills are in scope.
    let (scope_type, scope_id): (String, String) = conn
        .query_row(
            "SELECT scope_type, scope_id FROM governance_daos WHERE id = ?1",
            params![dao_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("DAO not found: {e}"))?;

    // Sentinel DAO (scope_type='sentinel') is open-proposal by design
    // (see docs/sentinel-federation.md §10 decision 4). Sybil is absorbed
    // by DAO ratification itself, so the proficiency gate does not apply.
    if scope_type == "sentinel" {
        return Ok(());
    }
    if scope_type != "subject_field" && scope_type != "subject" {
        return Err(format!("Unsupported governance scope: '{scope_type}'"));
    }

    // Post-migration 040: eligibility is derived from `credentials`
    // (skill-kind VCs) instead of `skill_proofs`. The proficiency
    // level on a SkillClaim serialises as an integer 0..=5 inside
    // `signed_vc_json` — we read it back via `json_extract` at query
    // time (no hoisted column needed; governance checks are low-
    // frequency). The integer maps 1:1 onto `BLOOM_ORDER`:
    //   0 = remember, 1 = understand, 2 = apply,
    //   3 = analyze,  4 = evaluate,   5 = create.
    let min_idx_i64 = i64::from(min_idx);
    let has_credential: bool = if scope_type == "subject_field" {
        conn.query_row(
            "SELECT COUNT(*) > 0 FROM credentials c \
             JOIN skills sk ON sk.id = c.skill_id \
             JOIN subjects sub ON sub.id = sk.subject_id \
             WHERE c.claim_kind = 'skill' AND c.revoked = 0 \
               AND c.subject_did = ?3 \
               AND json_extract(c.signed_vc_json, '$.credentialSubject.id') = ?3 \
               AND json_extract(c.signed_vc_json, '$.credentialSubject.skillId') = c.skill_id \
               AND sub.subject_field_id = ?1 \
               AND json_type(c.signed_vc_json, '$.credentialSubject.level') = 'integer' \
               AND json_extract(c.signed_vc_json, '$.credentialSubject.level') BETWEEN ?2 AND 5",
            params![scope_id, min_idx_i64, subject_did.as_str()],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to read governance eligibility evidence: {e}"))?
    } else {
        conn.query_row(
            "SELECT COUNT(*) > 0 FROM credentials c \
             JOIN skills sk ON sk.id = c.skill_id \
             WHERE c.claim_kind = 'skill' AND c.revoked = 0 \
               AND c.subject_did = ?3 \
               AND json_extract(c.signed_vc_json, '$.credentialSubject.id') = ?3 \
               AND json_extract(c.signed_vc_json, '$.credentialSubject.skillId') = c.skill_id \
               AND sk.subject_id = ?1 \
               AND json_type(c.signed_vc_json, '$.credentialSubject.level') = 'integer' \
               AND json_extract(c.signed_vc_json, '$.credentialSubject.level') BETWEEN ?2 AND 5",
            params![scope_id, min_idx_i64, subject_did.as_str()],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to read governance eligibility evidence: {e}"))?
    };

    if has_credential {
        Ok(())
    } else {
        Err(format!(
            "Insufficient proficiency: requires '{min_level}' in {scope_type} '{scope_id}' — no qualifying credentials found"
        ))
    }
}

/// Parse an optional ISO 8601 deadline string and check if it has passed.
/// Returns Ok(()) if no deadline is set, or the deadline has not passed.
fn check_before_deadline(deadline: Option<&str>, action: &str) -> Result<(), String> {
    if let Some(dl) = deadline {
        let parsed = chrono::DateTime::parse_from_rfc3339(dl)
            .map_err(|error| format!("Invalid deadline for {action}: {error}"))?;
        if chrono::Utc::now() >= parsed {
            return Err(format!("Deadline has passed for {action} (deadline: {dl})"));
        }
    }
    Ok(())
}

/// Parse an optional ISO 8601 deadline string and check if it has been reached.
/// Returns Ok(()) if no deadline is set, or the deadline has been reached.
fn check_after_deadline(deadline: Option<&str>, action: &str) -> Result<(), String> {
    if let Some(dl) = deadline {
        let parsed = chrono::DateTime::parse_from_rfc3339(dl)
            .map_err(|error| format!("Invalid deadline for {action}: {error}"))?;
        if chrono::Utc::now() < parsed {
            return Err(format!(
                "Deadline not yet reached for {action} (deadline: {dl})"
            ));
        }
    }
    Ok(())
}

/// Fire-and-forget enqueue for on-chain governance transactions.
/// Logs a warning if enqueue fails but never fails the parent command.
fn try_enqueue(db: &crate::db::Database, action: &str, target_table: &str, target_id: &str) {
    let payload = serde_json::json!({ "action": action, "target_id": target_id }).to_string();
    if let Err(e) = onchain_queue::enqueue(db, action, &payload, target_table, target_id) {
        log::warn!("Failed to enqueue on-chain tx for {action}: {e}");
    }
}

// ---- DAO Commands ----

/// List all DAOs, optionally filtered by scope_type and/or status.
#[tauri::command]
pub async fn list_daos(
    state: State<'_, AppState>,
    scope_type: Option<String>,
    status: Option<String>,
    search: Option<String>,
) -> Result<Vec<DaoInfo>, String> {
    governance_db(
        &state,
        DatabaseWorkload::Learner,
        "governance.list-daos",
        move |db| {
            let conn = db.conn();

            let mut conditions: Vec<String> = Vec::new();
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(ref st) = scope_type {
                conditions.push(format!("scope_type = ?{idx}"));
                param_values.push(Box::new(st.clone()));
                idx += 1;
            }
            if let Some(ref s) = status {
                conditions.push(format!("status = ?{idx}"));
                param_values.push(Box::new(s.clone()));
                idx += 1;
            }
            if let Some(ref q) = search {
                conditions.push(format!("(name LIKE ?{idx} OR description LIKE ?{idx})"));
                param_values.push(Box::new(format!("%{q}%")));
                idx += 1;
            }
            let _ = idx;

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let sql = format!(
                "SELECT id, name, description, icon_emoji, scope_type, scope_id, status, \
         committee_size, election_interval_days, on_chain_tx, created_at, updated_at \
         FROM governance_daos {where_clause} ORDER BY name ASC"
            );

            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|v| v.as_ref()).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

            let daos = stmt
                .query_map(params_ref.as_slice(), |row| {
                    Ok(DaoInfo {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        icon_emoji: row.get(3)?,
                        scope_type: row.get(4)?,
                        scope_id: row.get(5)?,
                        status: row.get(6)?,
                        committee_size: row.get(7)?,
                        election_interval_days: row.get(8)?,
                        on_chain_tx: row.get(9)?,
                        created_at: row.get(10)?,
                        updated_at: row.get(11)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;

            Ok(daos)
        },
    )
    .await
}

/// Get a DAO by ID, including its members.
#[tauri::command]
pub async fn get_dao(
    state: State<'_, AppState>,
    dao_id: String,
) -> Result<(DaoInfo, Vec<DaoMember>), String> {
    governance_db(
        &state,
        DatabaseWorkload::Learner,
        "governance.get-dao",
        move |db| {
            let conn = db.conn();

            let dao: DaoInfo = conn
                .query_row(
                    "SELECT id, name, description, icon_emoji, scope_type, scope_id, status, \
             committee_size, election_interval_days, on_chain_tx, created_at, updated_at \
             FROM governance_daos WHERE id = ?1",
                    params![dao_id],
                    |row| {
                        Ok(DaoInfo {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            description: row.get(2)?,
                            icon_emoji: row.get(3)?,
                            scope_type: row.get(4)?,
                            scope_id: row.get(5)?,
                            status: row.get(6)?,
                            committee_size: row.get(7)?,
                            election_interval_days: row.get(8)?,
                            on_chain_tx: row.get(9)?,
                            created_at: row.get(10)?,
                            updated_at: row.get(11)?,
                        })
                    },
                )
                .map_err(|e| format!("DAO not found: {e}"))?;

            let mut stmt = conn
                .prepare(
                    "SELECT dao_id, stake_address, role, joined_at \
             FROM governance_dao_members WHERE dao_id = ?1",
                )
                .map_err(|e| e.to_string())?;

            let members = stmt
                .query_map(params![dao_id], |row| {
                    Ok(DaoMember {
                        dao_id: row.get(0)?,
                        stake_address: row.get(1)?,
                        role: row.get(2)?,
                        joined_at: row.get(3)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;

            Ok((dao, members))
        },
    )
    .await
}

/// Legacy one-operator DAO bootstrap compatibility command.
///
/// The authority-bearing implementation is compiled only for debug builds
/// that explicitly enable `legacy-governance-bootstrap`. Every other build
/// retains the IPC name for compatibility but fails closed, pending the
/// founding-roster protocol.
#[tauri::command]
pub async fn create_dao(
    state: State<'_, AppState>,
    name: String,
    scope_type: String,
    scope_id: String,
    committee_size: Option<i64>,
    election_interval_days: Option<i64>,
) -> Result<DaoInfo, String> {
    #[cfg(not(all(debug_assertions, feature = "legacy-governance-bootstrap")))]
    {
        let _ = (
            state,
            name,
            scope_type,
            scope_id,
            committee_size,
            election_interval_days,
        );
        Err("legacy one-operator DAO creation is disabled; use the founding-roster protocol".into())
    }

    #[cfg(all(debug_assertions, feature = "legacy-governance-bootstrap"))]
    {
        create_dao_legacy(
            state,
            name,
            scope_type,
            scope_id,
            committee_size,
            election_interval_days,
        )
        .await
    }
}

#[cfg(all(debug_assertions, feature = "legacy-governance-bootstrap"))]
async fn create_dao_legacy(
    state: State<'_, AppState>,
    name: String,
    scope_type: String,
    scope_id: String,
    committee_size: Option<i64>,
    election_interval_days: Option<i64>,
) -> Result<DaoInfo, String> {
    use crate::cardano::{gov_tx_builder, plutus_data, plutus_mint, script_refs};

    let op = crate::cardano::operator::load_operator_key()
        .ok_or("this node is not configured as a governance operator (set OPERATOR_SKEY_*)")?;
    let op_pkh = op.payment_key_hash();

    let project_id = governance_db(
        &state,
        DatabaseWorkload::Learner,
        "governance.legacy-create-project-id",
        move |db| {
            Ok(crate::cardano::blockfrost::resolve_project_id(Some(
                db.conn(),
            )))
        },
    )
    .await?
    .ok_or("Blockfrost project id not configured")?;
    let bf =
        crate::cardano::blockfrost::BlockfrostClient::new(project_id).map_err(|e| e.to_string())?;

    let interval_days = election_interval_days.unwrap_or(30);
    let interval_ms = interval_days * 86_400_000;
    let committee_n = committee_size.unwrap_or(1);
    let now_ms = chrono::Utc::now().timestamp_millis();

    // DaoDatum: scope + policies + the operator as sole initial committee.
    let e2s = |e: crate::cardano::tx_builder::TxBuildError| e.to_string();
    let datum = plutus_data::encode_dao_datum(
        &scope_type,
        scope_id.as_bytes(),
        &gov_tx_builder::hash_from_hex_pub(script_refs::REPUTATION_MINTING_SCRIPT_HASH)
            .map_err(e2s)?,
        &[],
        "remember",
        &gov_tx_builder::hash_from_hex_pub(script_refs::DAO_MINTING_SCRIPT_HASH).map_err(e2s)?,
        &[&op_pkh],
        committee_n,
        interval_ms,
        now_ms,
        now_ms + interval_ms,
    )
    .map_err(e2s)?;
    let redeemer = plutus_data::encode_dao_redeemer("create", None).map_err(e2s)?;

    // State token name = "dao" ++ scope_id (matches dao_registry validator).
    let mut asset_name = b"dao".to_vec();
    asset_name.extend_from_slice(scope_id.as_bytes());

    let policy = pallas_crypto::hash::Hash::<28>::from(
        gov_tx_builder::hash_from_hex_pub(script_refs::DAO_MINTING_SCRIPT_HASH).map_err(e2s)?,
    );
    let unsigned = plutus_mint::build_mint_to_address_unsigned(
        &bf,
        &plutus_mint::MintToAddress {
            payment_address: &op.address,
            payment_key_extended: &[0u8; 64], // unused on the unsigned path
            required_signers: std::slice::from_ref(&op_pkh),
            policy_id: policy,
            asset_name: asset_name.clone(),
            mint_redeemer: redeemer,
            ref_script: script_refs::DAO_MINTING_REF_UTXO,
            recipient_address: gov_tx_builder::script_address(
                script_refs::DAO_REGISTRY_SCRIPT_HASH,
            )
            .map_err(e2s)?,
            recipient_lovelace: 3_000_000,
            recipient_datum: Some(datum),
        },
    )
    .await
    .map_err(e2s)?;

    let signed =
        crate::cardano::tx_builder::sign_raw_tx(&unsigned, &op.private_key).map_err(e2s)?;
    let tx_hash = bf
        .submit_tx(&signed)
        .await
        .map_err(|e| format!("DAO create submission failed: {e}"))?;

    // Persist the DAO + its on-chain links. The state token + datum land
    // at output #0 (the recipient), so that's the live state UTxO.
    let dao_id = entity_id(&[&scope_type, &scope_id, &name]);
    let asset_name_hex = hex::encode(&asset_name);
    let dao_state_utxo = format!("{tx_hash}#0");
    let persisted_dao_id = dao_id.clone();
    let persisted_name = name.clone();
    let persisted_scope_type = scope_type.clone();
    let persisted_scope_id = scope_id.clone();
    let persisted_tx_hash = tx_hash.clone();
    governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.legacy-create-persist",
        move |db| {
            db.conn()
                .execute(
                    "INSERT INTO governance_daos \
                 (id, name, scope_type, scope_id, status, committee_size, \
                  election_interval_days, on_chain_tx, state_token_policy, \
                  state_token_name, reputation_policy, dao_state_utxo) \
                 VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        persisted_dao_id,
                        persisted_name,
                        persisted_scope_type,
                        persisted_scope_id,
                        committee_n,
                        interval_days,
                        persisted_tx_hash,
                        script_refs::DAO_MINTING_SCRIPT_HASH,
                        asset_name_hex,
                        script_refs::REPUTATION_MINTING_SCRIPT_HASH,
                        dao_state_utxo,
                    ],
                )
                .map_err(|e| format!("failed to persist DAO: {e}"))?;
            Ok(())
        },
    )
    .await?;

    log::info!("Created DAO '{name}' on-chain (tx {tx_hash})");
    Ok(DaoInfo {
        id: dao_id,
        name,
        description: None,
        icon_emoji: None,
        scope_type,
        scope_id,
        status: "active".into(),
        committee_size: committee_n,
        election_interval_days: interval_days,
        on_chain_tx: Some(tx_hash),
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    })
}

// ---- Election Commands ----

/// Open a new election for a DAO.
#[tauri::command]
pub async fn open_election(
    state: State<'_, AppState>,
    params: OpenElectionParams,
) -> Result<Election, String> {
    let w = load_wallet(&state).await?;

    let (election, signed) = governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.open-election",
        move |db| {
            governance_transaction(db, |conn| {
                ensure_registered(conn, &w)?;

                // Verify DAO exists and is active
                let dao_status: String = conn
                    .query_row(
                        "SELECT status FROM governance_daos WHERE id = ?1",
                        params![params.dao_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("DAO not found: {e}"))?;

                if dao_status != "active" {
                    return Err(format!("DAO is not active (status: {dao_status})"));
                }

                let id = entity_id(&[
                    &params.dao_id,
                    &params.title,
                    &chrono::Utc::now().to_rfc3339(),
                ]);
                let seats = params.seats.unwrap_or(5);
                let nominee_prof = params
                    .nominee_min_proficiency
                    .clone()
                    .unwrap_or_else(|| "apply".into());
                let voter_prof = params
                    .voter_min_proficiency
                    .clone()
                    .unwrap_or_else(|| "remember".into());

                conn.execute(
                    "INSERT INTO governance_elections \
             (id, dao_id, title, description, phase, seats, \
              nominee_min_proficiency, voter_min_proficiency, \
              nomination_end, voting_end) \
             VALUES (?1, ?2, ?3, ?4, 'nomination', ?5, ?6, ?7, ?8, ?9)",
                    params![
                        id,
                        params.dao_id,
                        params.title,
                        params.description,
                        seats,
                        nominee_prof,
                        voter_prof,
                        params.nomination_end,
                        params.voting_end,
                    ],
                )
                .map_err(|e| e.to_string())?;

                let election = query_election(conn, &id)?;
                // No on-chain tx at open under the lean model: the election lives
                // off-chain (DB + gossip) until finalize publishes the finalized
                // election UTxO that committee-install references.

                let signed = sign_governance_event(
                    &w,
                    &params.dao_id,
                    GovernanceEventType::ElectionOpened {
                        election_id: id.clone(),
                        title: params.title.clone(),
                        seats,
                        nominee_min_proficiency: nominee_prof,
                        voter_min_proficiency: voter_prof,
                        nomination_end: params.nomination_end.clone(),
                        voting_end: params.voting_end.clone(),
                    },
                )?;
                Ok((election, signed))
            })
        },
    )
    .await?;

    broadcast_signed(&state, &signed).await;
    Ok(election)
}

/// List elections, optionally filtered by dao_id and/or phase.
#[tauri::command]
pub async fn list_elections(
    state: State<'_, AppState>,
    dao_id: Option<String>,
    phase: Option<String>,
) -> Result<Vec<Election>, String> {
    governance_db(
        &state,
        DatabaseWorkload::Learner,
        "governance.list-elections",
        move |db| {
            let conn = db.conn();

            let mut conditions: Vec<String> = Vec::new();
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(ref d) = dao_id {
                conditions.push(format!("dao_id = ?{idx}"));
                param_values.push(Box::new(d.clone()));
                idx += 1;
            }
            if let Some(ref p) = phase {
                conditions.push(format!("phase = ?{idx}"));
                param_values.push(Box::new(p.clone()));
                idx += 1;
            }
            let _ = idx;

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let sql = format!(
                "SELECT id, dao_id, title, description, phase, seats, \
         nominee_min_proficiency, voter_min_proficiency, \
         nomination_start, nomination_end, voting_end, on_chain_tx, \
         created_at, finalized_at \
         FROM governance_elections {where_clause} ORDER BY created_at DESC"
            );

            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|v| v.as_ref()).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

            let elections = stmt
                .query_map(params_ref.as_slice(), |row| {
                    Ok(Election {
                        id: row.get(0)?,
                        dao_id: row.get(1)?,
                        title: row.get(2)?,
                        description: row.get(3)?,
                        phase: row.get(4)?,
                        seats: row.get(5)?,
                        nominee_min_proficiency: row.get(6)?,
                        voter_min_proficiency: row.get(7)?,
                        nomination_start: row.get(8)?,
                        nomination_end: row.get(9)?,
                        voting_end: row.get(10)?,
                        on_chain_tx: row.get(11)?,
                        created_at: row.get(12)?,
                        finalized_at: row.get(13)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;

            Ok(elections)
        },
    )
    .await
}

/// Get an election by ID with its nominees.
#[tauri::command]
pub async fn get_election(
    state: State<'_, AppState>,
    election_id: String,
) -> Result<(Election, Vec<ElectionNominee>), String> {
    governance_db(
        &state,
        DatabaseWorkload::Learner,
        "governance.get-election",
        move |db| {
            let election = query_election(db.conn(), &election_id)?;
            let nominees = query_nominees(db.conn(), &election_id)?;
            Ok((election, nominees))
        },
    )
    .await
}

/// Nominate someone (or self) for an election.
#[tauri::command]
pub async fn nominate(
    state: State<'_, AppState>,
    election_id: String,
    stake_address: String,
) -> Result<ElectionNominee, String> {
    // Self-nomination: the nominee is the local wallet (it signs).
    let _ = stake_address;
    let w = load_wallet(&state).await?;
    let stake_address = w.stake_address.clone();

    let (nominee, signed) = governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.nominate",
        move |db| {
            governance_transaction(db, |conn| {
                ensure_registered(conn, &w)?;

                // Verify election is in nomination phase
                let phase: String = conn
                    .query_row(
                        "SELECT phase FROM governance_elections WHERE id = ?1",
                        params![election_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("election not found: {e}"))?;

                if phase != "nomination" {
                    return Err(format!(
                        "election is not in nomination phase (phase: {phase})"
                    ));
                }

                // Check nomination deadline
                let (nomination_end, dao_id_for_prof): (Option<String>, String) = conn
                    .query_row(
                        "SELECT nomination_end, dao_id FROM governance_elections WHERE id = ?1",
                        params![election_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .map_err(|e| e.to_string())?;
                check_before_deadline(nomination_end.as_deref(), "nomination")?;

                // Proficiency gate: nominee must have at least "remember" in DAO scope
                check_proficiency(
                    conn,
                    &derive_did_key(&w.signing_key),
                    &dao_id_for_prof,
                    "remember",
                )?;

                let id = entity_id(&[&election_id, &stake_address]);

                conn.execute(
                    "INSERT INTO governance_election_nominees \
             (id, election_id, stake_address) VALUES (?1, ?2, ?3)",
                    params![id, election_id, stake_address],
                )
                .map_err(|e| e.to_string())?;

                let signed = sign_governance_event(
                    &w,
                    &dao_id_for_prof,
                    GovernanceEventType::NomineeSubmitted {
                        election_id: election_id.clone(),
                        nominee_id: id.clone(),
                        nominee: stake_address.clone(),
                    },
                )?;

                let nominee = ElectionNominee {
                    id,
                    election_id: election_id.clone(),
                    stake_address: stake_address.clone(),
                    accepted: false,
                    votes_received: 0,
                    is_winner: false,
                    nominated_at: chrono::Utc::now().to_rfc3339(),
                };
                Ok((nominee, signed))
            })
        },
    )
    .await?;

    broadcast_signed(&state, &signed).await;
    Ok(nominee)
}

/// Accept a nomination (nominee confirms their candidacy).
#[tauri::command]
pub async fn accept_nomination(
    state: State<'_, AppState>,
    nominee_id: String,
) -> Result<(), String> {
    let w = load_wallet(&state).await?;

    let signed = governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.accept-nomination",
        move |db| {
            governance_transaction(db, |conn| {
                ensure_registered(conn, &w)?;

                // Look up the nominee's election context for proficiency + deadline checks
                let (stake_address, election_id): (String, String) = conn
            .query_row(
                "SELECT stake_address, election_id FROM governance_election_nominees WHERE id = ?1",
                params![nominee_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "nominee not found".to_string())?;

                // A nominee may only accept their OWN nomination.
                if stake_address != w.stake_address {
                    return Err("you can only accept your own nomination".into());
                }

                let (nominee_min_prof, nomination_end, dao_id): (String, Option<String>, String) =
                    conn.query_row(
                        "SELECT nominee_min_proficiency, nomination_end, dao_id \
                 FROM governance_elections WHERE id = ?1",
                        params![election_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .map_err(|e| e.to_string())?;

                // Deadline check: must be before nomination_end
                check_before_deadline(nomination_end.as_deref(), "accepting nomination")?;

                // Proficiency gate: nominee must meet nominee_min_proficiency
                check_proficiency(
                    conn,
                    &derive_did_key(&w.signing_key),
                    &dao_id,
                    &nominee_min_prof,
                )?;

                let affected = conn
                    .execute(
                        "UPDATE governance_election_nominees SET accepted = 1 WHERE id = ?1",
                        params![nominee_id],
                    )
                    .map_err(|e| e.to_string())?;

                if affected == 0 {
                    return Err("nominee not found".into());
                }

                sign_governance_event(
                    &w,
                    &dao_id,
                    GovernanceEventType::NomineeAccepted {
                        election_id,
                        nominee_id: nominee_id.clone(),
                    },
                )
            })
        },
    )
    .await?;

    broadcast_signed(&state, &signed).await;
    Ok(())
}

/// Transition an election from nomination to voting phase.
#[tauri::command]
pub async fn start_election_voting(
    state: State<'_, AppState>,
    election_id: String,
) -> Result<(), String> {
    let w = load_wallet(&state).await?;

    let signed = governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.start-election-voting",
        move |db| {
            governance_transaction(db, |conn| {
                ensure_registered(conn, &w)?;

                let (phase, dao_id): (String, String) = conn
                    .query_row(
                        "SELECT phase, dao_id FROM governance_elections WHERE id = ?1",
                        params![election_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .map_err(|e| format!("election not found: {e}"))?;

                if phase != "nomination" {
                    return Err(format!(
                        "election must be in nomination phase to start voting (phase: {phase})"
                    ));
                }

                // Deadline check: nomination period must have ended
                let nomination_end: Option<String> = conn
                    .query_row(
                        "SELECT nomination_end FROM governance_elections WHERE id = ?1",
                        params![election_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                check_after_deadline(nomination_end.as_deref(), "starting voting")?;

                // Verify at least `seats` accepted nominees
                let (seats, accepted_count): (i64, i64) = conn
                    .query_row(
                        "SELECT e.seats, \
                 (SELECT COUNT(*) FROM governance_election_nominees n \
                  WHERE n.election_id = e.id AND n.accepted = 1) \
                 FROM governance_elections e WHERE e.id = ?1",
                        params![election_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .map_err(|e| e.to_string())?;

                if accepted_count < seats {
                    return Err(format!(
                "need at least {seats} accepted nominees to start voting, have {accepted_count}"
            ));
                }

                conn.execute(
                    "UPDATE governance_elections SET phase = 'voting' WHERE id = ?1",
                    params![election_id],
                )
                .map_err(|e| e.to_string())?;

                sign_governance_event(
                    &w,
                    &dao_id,
                    GovernanceEventType::ElectionStarted {
                        election_id: election_id.clone(),
                    },
                )
            })
        },
    )
    .await?;

    broadcast_signed(&state, &signed).await;
    Ok(())
}

/// Cast a vote in an election (one vote per voter).
#[tauri::command]
pub async fn cast_election_vote(
    state: State<'_, AppState>,
    election_id: String,
    voter: String,
    nominee_id: String,
) -> Result<ElectionVote, String> {
    // A node can only cast its OWN vote — the wallet key signs it. The
    // signed identity is the local wallet's stake address; the incoming
    // `voter` argument is ignored in favour of it.
    let _ = voter;
    let w = load_wallet(&state).await?;
    let voter = w.stake_address.clone();

    let (vote, signed) = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "governance.cast_election_vote",
            move |db| {
                let tx = Transaction::new_unchecked(db.conn(), TransactionBehavior::Immediate)
                    .map_err(|e| e.to_string())?;
                let conn = &tx;

                // Must be a registered user to vote.
                ensure_registered(conn, &w)?;

                let phase: String = conn
                    .query_row(
                        "SELECT phase FROM governance_elections WHERE id = ?1",
                        params![election_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("election not found: {e}"))?;
                if phase != "voting" {
                    return Err(format!("election is not in voting phase (phase: {phase})"));
                }

                let (voting_end, voter_min_prof, dao_id_for_vote): (
                    Option<String>,
                    String,
                    String,
                ) = conn
                    .query_row(
                        "SELECT voting_end, voter_min_proficiency, dao_id \
                         FROM governance_elections WHERE id = ?1",
                        params![election_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .map_err(|e| e.to_string())?;
                check_before_deadline(voting_end.as_deref(), "voting")?;
                check_proficiency(
                    conn,
                    &derive_did_key(&w.signing_key),
                    &dao_id_for_vote,
                    &voter_min_prof,
                )?;

                let already_voted: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM governance_election_votes \
                         WHERE election_id = ?1 AND voter = ?2",
                        params![election_id, voter],
                        |row| Ok(row.get::<_, i64>(0)? > 0),
                    )
                    .map_err(|e| e.to_string())?;
                if already_voted {
                    return Err("already voted in this election".into());
                }

                let nominee_accepted: bool = conn
                    .query_row(
                        "SELECT accepted FROM governance_election_nominees \
                         WHERE id = ?1 AND election_id = ?2",
                        params![nominee_id, election_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("nominee not found: {e}"))?;
                if !nominee_accepted {
                    return Err("nominee has not accepted their nomination".into());
                }

                let signed = sign_governance_event(
                    &w,
                    &dao_id_for_vote,
                    GovernanceEventType::ElectionVoteRecorded {
                        election_id: election_id.clone(),
                        voter: voter.clone(),
                        nominee_id: nominee_id.clone(),
                    },
                )?;
                let signature_hex = hex::encode(&signed.signature);
                let public_key_hex = hex::encode(&signed.public_key);
                let vote_id = entity_id(&[&election_id, &voter]);
                if !record_election_vote(
                    &tx,
                    &election_id,
                    &nominee_id,
                    &VoteEvidence {
                        voter: &voter,
                        signature: &signature_hex,
                        public_key: &public_key_hex,
                    },
                )
                .map_err(|e| e.to_string())?
                {
                    return Err("already voted in this election".into());
                }

                let vote = ElectionVote {
                    id: vote_id,
                    election_id: election_id.clone(),
                    voter: voter.clone(),
                    nominee_id: nominee_id.clone(),
                    on_chain_tx: None,
                    voted_at: chrono::Utc::now().to_rfc3339(),
                };
                tx.commit().map_err(|e| e.to_string())?;
                Ok((vote, signed))
            },
        )
        .await?;

    // Votes are off-chain under the lean governance model: gossip the
    // signed vote so peers can build the tally; the operator commits a
    // Merkle root of the signed votes on-chain at finalize.
    broadcast_signed(&state, &signed).await;
    Ok(vote)
}

/// Finalize an election: determine winners and transition to finalized.
#[tauri::command]
pub async fn finalize_election(
    state: State<'_, AppState>,
    election_id: String,
) -> Result<Vec<ElectionNominee>, String> {
    let w = load_wallet(&state).await?;

    let (nominees, signed) = governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.finalize-election",
        move |db| {
            let target_id = election_id.clone();
            let result = governance_transaction(db, |conn| {
                ensure_registered(conn, &w)?;

                let (phase, seats, dao_id): (String, i64, String) = conn
                    .query_row(
                        "SELECT phase, seats, dao_id FROM governance_elections WHERE id = ?1",
                        params![election_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .map_err(|e| format!("election not found: {e}"))?;

                if phase != "voting" {
                    return Err(format!(
                        "election must be in voting phase to finalize (phase: {phase})"
                    ));
                }

                // Deadline check: voting period must have ended
                let voting_end: Option<String> = conn
                    .query_row(
                        "SELECT voting_end FROM governance_elections WHERE id = ?1",
                        params![election_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                check_after_deadline(voting_end.as_deref(), "finalizing election")?;

                // Resolve only an unambiguous top-N set. Crossing-seat ties need the
                // separately certified runoff approved for the replacement protocol.
                let ranked_nominees: Vec<(String, i64)> = {
                    let mut stmt = conn
                        .prepare(
                            "SELECT id, votes_received FROM governance_election_nominees \
                     WHERE election_id = ?1 AND accepted = 1 \
                     ORDER BY votes_received DESC, id ASC",
                        )
                        .map_err(|e| e.to_string())?;
                    let ids = stmt
                        .query_map(params![election_id], |row| Ok((row.get(0)?, row.get(1)?)))
                        .map_err(|e| e.to_string())?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| e.to_string())?;
                    ids
                };
                let winner_ids = select_unambiguous_winners(&ranked_nominees, seats)?;

                // Mark winners
                conn.execute(
                    "UPDATE governance_election_nominees SET is_winner = 0 WHERE election_id = ?1",
                    params![election_id],
                )
                .map_err(|e| e.to_string())?;
                for wid in &winner_ids {
                    conn.execute(
                        "UPDATE governance_election_nominees SET is_winner = 1 WHERE id = ?1",
                        params![wid],
                    )
                    .map_err(|e| e.to_string())?;
                }

                // Transition to finalized
                conn.execute(
            "UPDATE governance_elections SET phase = 'finalized', finalized_at = datetime('now') \
             WHERE id = ?1",
            params![election_id],
        )
        .map_err(|e| e.to_string())?;

                let nominees = query_nominees(conn, &election_id)?;
                let signed = sign_governance_event(
                    &w,
                    &dao_id,
                    GovernanceEventType::ElectionFinalized {
                        election_id: election_id.clone(),
                        winner_nominee_ids: winner_ids,
                    },
                )?;
                Ok((nominees, signed))
            })?;
            try_enqueue(db, "finalize_election", "governance_elections", &target_id);
            Ok(result)
        },
    )
    .await?;

    broadcast_signed(&state, &signed).await;
    Ok(nominees)
}

/// Install winners as the new DAO committee after a finalized election.
#[tauri::command]
pub async fn install_committee(
    state: State<'_, AppState>,
    election_id: String,
) -> Result<Vec<DaoMember>, String> {
    governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.install-committee",
        move |db| {
            let target_id = election_id.clone();
            let members =
                governance_transaction(db, |conn| replace_elected_committee(conn, &election_id))?;
            try_enqueue(db, "install_committee", "governance_elections", &target_id);
            Ok(members)
        },
    )
    .await
}

// ---- Proposal Commands ----

/// Submit a new proposal to a DAO.
#[tauri::command]
pub async fn submit_proposal(
    state: State<'_, AppState>,
    params: SubmitProposalParams,
) -> Result<Proposal, String> {
    let w = load_wallet(&state).await?;
    governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.submit-proposal",
        move |db| {
            governance_transaction(db, |conn| {
                // Verify DAO exists and is active
                let dao_status: String = conn
                    .query_row(
                        "SELECT status FROM governance_daos WHERE id = ?1",
                        params![params.dao_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("DAO not found: {e}"))?;

                if dao_status != "active" {
                    return Err(format!("DAO is not active (status: {dao_status})"));
                }

                ensure_registered(conn, &w)?;
                let proposer = w.stake_address.clone();

                // Proficiency gate: proposer must have at least "remember" in DAO scope
                check_proficiency(
                    conn,
                    &derive_did_key(&w.signing_key),
                    &params.dao_id,
                    "remember",
                )?;

                let id = entity_id(&[&params.dao_id, &params.title, &proposer]);
                let min_prof = params
                    .min_vote_proficiency
                    .unwrap_or_else(|| "remember".into());

                conn.execute(
                    "INSERT INTO governance_proposals \
         (id, dao_id, title, description, category, status, proposer, \
          min_vote_proficiency) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'draft', ?6, ?7)",
                    params![
                        id,
                        params.dao_id,
                        params.title,
                        params.description,
                        params.category,
                        proposer,
                        min_prof,
                    ],
                )
                .map_err(|e| e.to_string())?;

                let proposal = query_proposal(conn, &id)?;
                // Off-chain under the lean model — only the resolved outcome is
                // anchored on-chain (at resolve), not draft submission.
                Ok(proposal)
            })
        },
    )
    .await
}

/// List proposals, optionally filtered by dao_id, status, category.
#[tauri::command]
pub async fn list_proposals(
    state: State<'_, AppState>,
    dao_id: Option<String>,
    status: Option<String>,
    category: Option<String>,
) -> Result<Vec<Proposal>, String> {
    governance_db(
        &state,
        DatabaseWorkload::Learner,
        "governance.list-proposals",
        move |db| {
            let conn = db.conn();

            let mut conditions: Vec<String> = Vec::new();
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(ref d) = dao_id {
                conditions.push(format!("dao_id = ?{idx}"));
                param_values.push(Box::new(d.clone()));
                idx += 1;
            }
            if let Some(ref s) = status {
                conditions.push(format!("status = ?{idx}"));
                param_values.push(Box::new(s.clone()));
                idx += 1;
            }
            if let Some(ref c) = category {
                conditions.push(format!("category = ?{idx}"));
                param_values.push(Box::new(c.clone()));
                idx += 1;
            }
            let _ = idx;

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let sql = format!(
                "SELECT id, dao_id, title, description, category, status, proposer, \
         votes_for, votes_against, voting_deadline, min_vote_proficiency, \
         on_chain_tx, created_at, resolved_at \
         FROM governance_proposals {where_clause} ORDER BY created_at DESC"
            );

            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|v| v.as_ref()).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

            let proposals = stmt
                .query_map(params_ref.as_slice(), |row| {
                    Ok(Proposal {
                        id: row.get(0)?,
                        dao_id: row.get(1)?,
                        title: row.get(2)?,
                        description: row.get(3)?,
                        category: row.get(4)?,
                        status: row.get(5)?,
                        proposer: row.get(6)?,
                        votes_for: row.get(7)?,
                        votes_against: row.get(8)?,
                        voting_deadline: row.get(9)?,
                        min_vote_proficiency: row.get(10)?,
                        on_chain_tx: row.get(11)?,
                        created_at: row.get(12)?,
                        resolved_at: row.get(13)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;

            Ok(proposals)
        },
    )
    .await
}

/// Approve a proposal (draft → published), setting voting deadline.
#[tauri::command]
pub async fn approve_proposal(
    state: State<'_, AppState>,
    proposal_id: String,
) -> Result<Proposal, String> {
    governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.approve-proposal",
        move |db| {
            governance_transaction(db, |conn| {
                let status: String = conn
                    .query_row(
                        "SELECT status FROM governance_proposals WHERE id = ?1",
                        params![proposal_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("proposal not found: {e}"))?;

                if status != "draft" {
                    return Err(format!(
                        "proposal must be in draft status to approve (status: {status})"
                    ));
                }

                let deadline = chrono::Utc::now() + chrono::Duration::days(DEFAULT_VOTING_DAYS);

                conn.execute(
        "UPDATE governance_proposals SET status = 'published', voting_deadline = ?1 WHERE id = ?2",
        params![deadline.to_rfc3339(), proposal_id],
    )
    .map_err(|e| e.to_string())?;

                let proposal = query_proposal(conn, &proposal_id)?;
                // Off-chain (committee approval to open voting); no on-chain tx.
                Ok(proposal)
            })
        },
    )
    .await
}

/// Cancel a proposal (only if not already resolved).
#[tauri::command]
pub async fn cancel_proposal(
    state: State<'_, AppState>,
    proposal_id: String,
) -> Result<(), String> {
    governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.cancel-proposal",
        move |db| {
            governance_transaction(db, |conn| {
                let status: String = conn
                    .query_row(
                        "SELECT status FROM governance_proposals WHERE id = ?1",
                        params![proposal_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("proposal not found: {e}"))?;

                if status == "approved" || status == "rejected" {
                    return Err(format!(
                        "cannot cancel a resolved proposal (status: {status})"
                    ));
                }

                conn.execute(
                    "UPDATE governance_proposals SET status = 'cancelled' WHERE id = ?1",
                    params![proposal_id],
                )
                .map_err(|e| e.to_string())?;

                Ok(())
            })
        },
    )
    .await
}

/// Cast a vote on a proposal (one vote per voter).
#[tauri::command]
pub async fn cast_proposal_vote(
    state: State<'_, AppState>,
    proposal_id: String,
    voter: String,
    in_favor: bool,
) -> Result<ProposalVote, String> {
    // Only the local wallet's own vote can be signed; ignore `voter`.
    let _ = voter;
    let w = load_wallet(&state).await?;
    let voter = w.stake_address.clone();

    let (vote, signed) = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "governance.cast_proposal_vote",
            move |db| {
                let tx = Transaction::new_unchecked(db.conn(), TransactionBehavior::Immediate)
                    .map_err(|e| e.to_string())?;
                let conn = &tx;
                ensure_registered(conn, &w)?;

                let status: String = conn
                    .query_row(
                        "SELECT status FROM governance_proposals WHERE id = ?1",
                        params![proposal_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("proposal not found: {e}"))?;

                if status != "published" {
                    return Err(format!(
                        "proposal is not open for voting (status: {status})"
                    ));
                }

                // Deadline + proficiency checks
                let (voting_deadline, min_prof, dao_id_for_vote): (Option<String>, String, String) =
                    conn.query_row(
                        "SELECT voting_deadline, min_vote_proficiency, dao_id \
                         FROM governance_proposals WHERE id = ?1",
                        params![proposal_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .map_err(|e| e.to_string())?;
                check_before_deadline(voting_deadline.as_deref(), "proposal voting")?;
                check_proficiency(
                    conn,
                    &derive_did_key(&w.signing_key),
                    &dao_id_for_vote,
                    &min_prof,
                )?;

                // Check double-vote
                let already_voted: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM governance_proposal_votes \
                         WHERE proposal_id = ?1 AND voter = ?2",
                        params![proposal_id, voter],
                        |row| Ok(row.get::<_, i64>(0)? > 0),
                    )
                    .map_err(|e| e.to_string())?;

                if already_voted {
                    return Err("already voted on this proposal".into());
                }

                let signed = sign_governance_event(
                    &w,
                    &dao_id_for_vote,
                    GovernanceEventType::ProposalVoteRecorded {
                        proposal_id: proposal_id.clone(),
                        voter: voter.clone(),
                        in_favor,
                    },
                )?;
                let signature_hex = hex::encode(&signed.signature);
                let public_key_hex = hex::encode(&signed.public_key);

                let vote_id = entity_id(&[&proposal_id, &voter]);
                if !record_proposal_vote(
                    &tx,
                    &proposal_id,
                    in_favor,
                    &VoteEvidence {
                        voter: &voter,
                        signature: &signature_hex,
                        public_key: &public_key_hex,
                    },
                )
                .map_err(|e| e.to_string())?
                {
                    return Err("already voted on this proposal".into());
                }

                let vote = ProposalVote {
                    id: vote_id,
                    proposal_id: proposal_id.clone(),
                    voter: voter.clone(),
                    in_favor,
                    on_chain_tx: None,
                    voted_at: chrono::Utc::now().to_rfc3339(),
                };
                tx.commit().map_err(|e| e.to_string())?;
                Ok((vote, signed))
            },
        )
        .await?;

    broadcast_signed(&state, &signed).await;
    Ok(vote)
}

/// Resolve a proposal using supermajority (2/3 votes_for).
#[tauri::command]
pub async fn resolve_proposal(
    state: State<'_, AppState>,
    proposal_id: String,
) -> Result<Proposal, String> {
    governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.resolve-proposal",
        move |db| {
            let target_id = proposal_id.clone();
            let proposal = governance_transaction(db, |conn| {
                let (status, votes_for, votes_against): (String, i64, i64) = conn
        .query_row(
            "SELECT status, votes_for, votes_against FROM governance_proposals WHERE id = ?1",
            params![proposal_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| format!("proposal not found: {e}"))?;

                if status != "published" {
                    return Err(format!(
                        "proposal must be published to resolve (status: {status})"
                    ));
                }

                // Deadline check: voting period must have ended
                let voting_deadline: Option<String> = conn
                    .query_row(
                        "SELECT voting_deadline FROM governance_proposals WHERE id = ?1",
                        params![proposal_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                check_after_deadline(voting_deadline.as_deref(), "resolving proposal")?;

                if votes_for == 0 && votes_against == 0 {
                    return Err("cannot resolve proposal with no votes".into());
                }

                let new_status = if has_two_thirds(votes_for, votes_against)? {
                    "approved"
                } else {
                    "rejected"
                };

                conn.execute(
        "UPDATE governance_proposals SET status = ?1, resolved_at = datetime('now') WHERE id = ?2",
        params![new_status, proposal_id],
    )
    .map_err(|e| e.to_string())?;

                let proposal = query_proposal(conn, &proposal_id)?;
                // Anchor the resolved outcome on-chain (operator-signed metadata +
                // Merkle root of the signed votes). Processed by the queue on the
                // operator node.
                Ok(proposal)
            })?;
            try_enqueue(db, "resolve_proposal", "governance_proposals", &target_id);
            Ok(proposal)
        },
    )
    .await
}

// ---- On-Chain Queue Commands ----

/// Get the status of all on-chain governance transaction submissions.
#[tauri::command]
pub async fn get_onchain_queue_status(
    state: State<'_, AppState>,
) -> Result<Vec<crate::cardano::onchain_queue::QueueItem>, String> {
    governance_db(
        &state,
        DatabaseWorkload::Learner,
        "governance.onchain-queue-status",
        crate::cardano::onchain_queue::get_all,
    )
    .await
}

/// Retry a failed on-chain governance transaction.
#[tauri::command]
pub async fn retry_onchain_submission(
    state: State<'_, AppState>,
    queue_id: String,
) -> Result<(), String> {
    governance_db(
        &state,
        DatabaseWorkload::Instructor,
        "governance.retry-onchain-submission",
        move |db| crate::cardano::onchain_queue::retry_item(db, &queue_id),
    )
    .await
}

// ---- Internal Helpers ----

fn replace_elected_committee(
    conn: &rusqlite::Connection,
    election_id: &str,
) -> Result<Vec<DaoMember>, String> {
    let (phase, dao_id, seats): (String, String, i64) = conn
        .query_row(
            "SELECT phase, dao_id, seats FROM governance_elections WHERE id = ?1",
            params![election_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| format!("election not found: {e}"))?;

    if phase != "finalized" {
        return Err(format!(
            "election must be finalized to install committee (phase: {phase})"
        ));
    }

    let mut statement = conn
        .prepare(
            "SELECT stake_address FROM governance_election_nominees \
             WHERE election_id = ?1 AND is_winner = 1 ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let winners: Vec<String> = statement
        .query_map(params![election_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let expected =
        usize::try_from(seats).map_err(|_| "election seat count cannot be negative".to_string())?;
    if expected == 0 || winners.len() != expected {
        return Err(format!(
            "cannot install committee: expected {expected} certified winners, found {}",
            winners.len()
        ));
    }

    conn.execute(
        "DELETE FROM governance_dao_members WHERE dao_id = ?1 AND role = 'committee'",
        params![dao_id],
    )
    .map_err(|e| e.to_string())?;

    let now = chrono::Utc::now().to_rfc3339();
    for address in &winners {
        conn.execute(
            "INSERT OR REPLACE INTO governance_dao_members \
             (dao_id, stake_address, role, joined_at) VALUES (?1, ?2, 'committee', ?3)",
            params![dao_id, address, now],
        )
        .map_err(|e| e.to_string())?;
    }

    let mut statement = conn
        .prepare(
            "SELECT dao_id, stake_address, role, joined_at \
             FROM governance_dao_members WHERE dao_id = ?1 ORDER BY stake_address",
        )
        .map_err(|e| e.to_string())?;
    let members = statement
        .query_map(params![dao_id], |row| {
            Ok(DaoMember {
                dao_id: row.get(0)?,
                stake_address: row.get(1)?,
                role: row.get(2)?,
                joined_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(members)
}

fn query_election(conn: &rusqlite::Connection, election_id: &str) -> Result<Election, String> {
    conn.query_row(
        "SELECT id, dao_id, title, description, phase, seats, \
         nominee_min_proficiency, voter_min_proficiency, \
         nomination_start, nomination_end, voting_end, on_chain_tx, \
         created_at, finalized_at \
         FROM governance_elections WHERE id = ?1",
        params![election_id],
        |row| {
            Ok(Election {
                id: row.get(0)?,
                dao_id: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                phase: row.get(4)?,
                seats: row.get(5)?,
                nominee_min_proficiency: row.get(6)?,
                voter_min_proficiency: row.get(7)?,
                nomination_start: row.get(8)?,
                nomination_end: row.get(9)?,
                voting_end: row.get(10)?,
                on_chain_tx: row.get(11)?,
                created_at: row.get(12)?,
                finalized_at: row.get(13)?,
            })
        },
    )
    .map_err(|e| format!("election not found: {e}"))
}

fn query_nominees(
    conn: &rusqlite::Connection,
    election_id: &str,
) -> Result<Vec<ElectionNominee>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, election_id, stake_address, accepted, votes_received, is_winner, nominated_at \
             FROM governance_election_nominees WHERE election_id = ?1 \
             ORDER BY votes_received DESC",
        )
        .map_err(|e| e.to_string())?;

    let nominees = stmt
        .query_map(params![election_id], |row| {
            Ok(ElectionNominee {
                id: row.get(0)?,
                election_id: row.get(1)?,
                stake_address: row.get(2)?,
                accepted: row.get(3)?,
                votes_received: row.get(4)?,
                is_winner: row.get(5)?,
                nominated_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(nominees)
}

fn query_proposal(conn: &rusqlite::Connection, proposal_id: &str) -> Result<Proposal, String> {
    conn.query_row(
        "SELECT id, dao_id, title, description, category, status, proposer, \
         votes_for, votes_against, voting_deadline, min_vote_proficiency, \
         on_chain_tx, created_at, resolved_at \
         FROM governance_proposals WHERE id = ?1",
        params![proposal_id],
        |row| {
            Ok(Proposal {
                id: row.get(0)?,
                dao_id: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                category: row.get(4)?,
                status: row.get(5)?,
                proposer: row.get(6)?,
                votes_for: row.get(7)?,
                votes_against: row.get(8)?,
                voting_deadline: row.get(9)?,
                min_vote_proficiency: row.get(10)?,
                on_chain_tx: row.get(11)?,
                created_at: row.get(12)?,
                resolved_at: row.get(13)?,
            })
        },
    )
    .map_err(|e| format!("proposal not found: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn setup_db() -> Database {
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrations");

        // Create local identity
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test1uvoter', 'addr_test1q123')",
                [],
            )
            .unwrap();

        // Create subject field and subject for DAO scope
        db.conn()
            .execute(
                "INSERT INTO subject_fields (id, name) VALUES ('sf1', 'CS')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub1', 'Algo', 'sf1')",
                [],
            )
            .unwrap();

        // Create an active DAO
        db.conn()
            .execute(
                "INSERT INTO governance_daos (id, name, scope_type, scope_id, status) \
                 VALUES ('dao1', 'CS DAO', 'subject_field', 'sf1', 'active')",
                [],
            )
            .unwrap();

        db
    }

    fn issue_skill_for(conn: &rusqlite::Connection, subject: crate::crypto::did::Did) {
        use crate::commands::credentials::{issue_credential_impl, IssueCredentialRequest};
        use crate::domain::vc::{Claim, CredentialType, SkillClaim};

        conn.execute(
            "INSERT OR IGNORE INTO skills (id, name, subject_id) VALUES ('skill1', 'Sorting', 'sub1')",
            [],
        )
        .unwrap();
        let issuer_key = ed25519_dalek::SigningKey::from_bytes(&[73; 32]);
        let issuer = crate::crypto::did::derive_did_key(&issuer_key);
        let request = IssueCredentialRequest {
            credential_type: CredentialType::AssessmentCredential,
            subject,
            claim: Claim::Skill(SkillClaim {
                skill_id: "skill1".into(),
                level: 4,
                score: 0.9,
                evidence_refs: vec![],
                rubric_version: Some("v1".into()),
                assessment_method: Some("exam".into()),
                provenance: None,
            }),
            evidence_refs: vec![],
            expiration_date: None,
            supersedes: None,
            integrity_session_id: None,
            integrity_policy: None,
        };
        issue_credential_impl(conn, &issuer_key, &issuer, &request, "2026-09-01T00:00:00Z")
            .unwrap();
    }

    #[test]
    fn proficiency_does_not_borrow_another_subjects_credential() {
        let db = setup_db();
        let alice =
            crate::crypto::did::derive_did_key(&ed25519_dalek::SigningKey::from_bytes(&[1; 32]));
        let bob =
            crate::crypto::did::derive_did_key(&ed25519_dalek::SigningKey::from_bytes(&[2; 32]));
        issue_skill_for(db.conn(), alice.clone());
        for (scope_type, scope_id) in [("subject_field", "sf1"), ("subject", "sub1")] {
            db.conn()
                .execute(
                    "UPDATE governance_daos SET scope_type = ?1, scope_id = ?2 WHERE id = 'dao1'",
                    params![scope_type, scope_id],
                )
                .unwrap();
            assert!(check_proficiency(db.conn(), &alice, "dao1", "apply").is_ok());
            assert!(check_proficiency(db.conn(), &bob, "dao1", "apply").is_err());
        }
    }

    #[test]
    fn proficiency_rejects_invalid_requirement_instead_of_weakening_it() {
        let db = setup_db();
        let alice =
            crate::crypto::did::derive_did_key(&ed25519_dalek::SigningKey::from_bytes(&[1; 32]));
        issue_skill_for(db.conn(), alice.clone());
        assert!(check_proficiency(db.conn(), &alice, "dao1", "typo").is_err());
    }

    #[test]
    fn proficiency_requires_the_signed_subject_to_match_the_cached_subject() {
        let db = setup_db();
        let alice = derive_did_key(&ed25519_dalek::SigningKey::from_bytes(&[1; 32]));
        let bob = derive_did_key(&ed25519_dalek::SigningKey::from_bytes(&[2; 32]));
        issue_skill_for(db.conn(), alice);
        db.conn()
            .execute("UPDATE credentials SET subject_did = ?1", [bob.as_str()])
            .unwrap();
        assert!(check_proficiency(db.conn(), &bob, "dao1", "apply").is_err());
    }

    #[test]
    fn proficiency_requires_a_bounded_integer_level_without_sql_coercion() {
        let db = setup_db();
        let alice = derive_did_key(&ed25519_dalek::SigningKey::from_bytes(&[1; 32]));
        issue_skill_for(db.conn(), alice.clone());
        assert!(check_proficiency(db.conn(), &alice, "dao1", "evaluate").is_ok());
        assert!(check_proficiency(db.conn(), &alice, "dao1", "create").is_err());
        for invalid in [
            serde_json::json!("4"),
            serde_json::json!(4.5),
            serde_json::json!(99),
            serde_json::json!(-1),
        ] {
            db.conn().execute(
                "UPDATE credentials SET signed_vc_json = json_set(signed_vc_json, '$.credentialSubject.level', json(?1))",
                [invalid.to_string()],
            ).unwrap();
            assert!(check_proficiency(db.conn(), &alice, "dao1", "remember").is_err());
        }
    }

    #[test]
    fn proficiency_reports_database_failure_separately_from_missing_qualification() {
        let db = setup_db();
        let alice = derive_did_key(&ed25519_dalek::SigningKey::from_bytes(&[1; 32]));
        issue_skill_for(db.conn(), alice.clone());
        db.conn()
            .execute("UPDATE credentials SET signed_vc_json = 'malformed'", [])
            .unwrap();
        let error = check_proficiency(db.conn(), &alice, "dao1", "remember").unwrap_err();
        assert!(error.contains("Failed to read governance eligibility evidence"));
    }

    #[test]
    fn create_and_list_daos() {
        let db = setup_db();
        let conn = db.conn();

        // `setup_db` inserts one DAO (`dao1`). Migration 037 seeds the
        // Sentinel DAO on any fresh database, so the total count is 2.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM governance_daos", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);

        let dao1_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM governance_daos WHERE id = 'dao1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(dao1_exists);
    }

    #[test]
    fn election_lifecycle_nomination_to_finalized() {
        let db = setup_db();
        let conn = db.conn();

        // Open election
        let elec_id = entity_id(&["dao1", "test election", "now"]);
        conn.execute(
            "INSERT INTO governance_elections \
             (id, dao_id, title, phase, seats, nominee_min_proficiency, voter_min_proficiency) \
             VALUES (?1, 'dao1', 'Test Election', 'nomination', 3, 'apply', 'remember')",
            params![elec_id],
        )
        .unwrap();

        // Verify phase
        let phase: String = conn
            .query_row(
                "SELECT phase FROM governance_elections WHERE id = ?1",
                params![elec_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(phase, "nomination");

        // Add 3 nominees and accept them
        for i in 1..=3 {
            let nom_id = entity_id(&[&elec_id, &format!("nominee{i}")]);
            conn.execute(
                "INSERT INTO governance_election_nominees \
                 (id, election_id, stake_address, accepted) VALUES (?1, ?2, ?3, 1)",
                params![nom_id, elec_id, format!("stake_test1unominee{i}")],
            )
            .unwrap();
        }

        // Transition to voting
        let accepted: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM governance_election_nominees \
                 WHERE election_id = ?1 AND accepted = 1",
                params![elec_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, 3);

        conn.execute(
            "UPDATE governance_elections SET phase = 'voting' WHERE id = ?1",
            params![elec_id],
        )
        .unwrap();

        let phase: String = conn
            .query_row(
                "SELECT phase FROM governance_elections WHERE id = ?1",
                params![elec_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(phase, "voting");
    }

    #[test]
    fn double_vote_prevention_elections() {
        let db = setup_db();
        let conn = db.conn();

        let elec_id = entity_id(&["dao1", "elec", "now"]);
        conn.execute(
            "INSERT INTO governance_elections \
             (id, dao_id, title, phase, seats) VALUES (?1, 'dao1', 'Test', 'voting', 1)",
            params![elec_id],
        )
        .unwrap();

        let nom_id = entity_id(&[&elec_id, "nominee1"]);
        conn.execute(
            "INSERT INTO governance_election_nominees \
             (id, election_id, stake_address, accepted) VALUES (?1, ?2, 'stake_test1unom', 1)",
            params![nom_id, elec_id],
        )
        .unwrap();

        // First vote should succeed
        let vote1_id = entity_id(&[&elec_id, "voter1"]);
        conn.execute(
            "INSERT INTO governance_election_votes \
             (id, election_id, voter, nominee_id) VALUES (?1, ?2, 'voter1', ?3)",
            params![vote1_id, elec_id, nom_id],
        )
        .unwrap();

        // Second vote from same voter should fail (UNIQUE constraint)
        let vote2_id = entity_id(&[&elec_id, "voter1", "2"]);
        let result = conn.execute(
            "INSERT INTO governance_election_votes \
             (id, election_id, voter, nominee_id) VALUES (?1, ?2, 'voter1', ?3)",
            params![vote2_id, elec_id, nom_id],
        );
        assert!(
            result.is_err(),
            "double vote should be rejected by UNIQUE constraint"
        );
    }

    #[test]
    fn proposal_lifecycle_draft_to_approved() {
        let db = setup_db();
        let conn = db.conn();

        // Create proposal
        let prop_id = entity_id(&["dao1", "test prop", "proposer"]);
        conn.execute(
            "INSERT INTO governance_proposals \
             (id, dao_id, title, category, status, proposer, min_vote_proficiency) \
             VALUES (?1, 'dao1', 'Test Proposal', 'policy', 'draft', 'stake_test1uvoter', 'remember')",
            params![prop_id],
        )
        .unwrap();

        // Approve (draft -> published)
        conn.execute(
            "UPDATE governance_proposals SET status = 'published', \
             voting_deadline = datetime('now', '+14 days') WHERE id = ?1",
            params![prop_id],
        )
        .unwrap();

        // Cast votes: 3 for, 1 against → 75% > 66.7% → approved
        for i in 1..=3 {
            let vid = entity_id(&[&prop_id, &format!("voter{i}")]);
            conn.execute(
                "INSERT INTO governance_proposal_votes \
                 (id, proposal_id, voter, in_favor) VALUES (?1, ?2, ?3, 1)",
                params![vid, prop_id, format!("voter{i}")],
            )
            .unwrap();
        }
        let vid = entity_id(&[&prop_id, "voter4"]);
        conn.execute(
            "INSERT INTO governance_proposal_votes \
             (id, proposal_id, voter, in_favor) VALUES (?1, ?2, 'voter4', 0)",
            params![vid, prop_id],
        )
        .unwrap();

        // Update tally
        conn.execute(
            "UPDATE governance_proposals SET votes_for = 3, votes_against = 1 WHERE id = ?1",
            params![prop_id],
        )
        .unwrap();

        // Resolve: 3/4 = 0.75 >= 0.667 → approved
        let (vf, va): (i64, i64) = conn
            .query_row(
                "SELECT votes_for, votes_against FROM governance_proposals WHERE id = ?1",
                params![prop_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        let new_status = if has_two_thirds(vf, va).unwrap() {
            "approved"
        } else {
            "rejected"
        };

        assert_eq!(new_status, "approved");
    }

    #[test]
    fn proposal_supermajority_rejection() {
        // 1 for, 2 against → 33% < 67% → rejected
        assert!(!has_two_thirds(1, 2).unwrap());
    }

    #[test]
    fn proposal_supermajority_uses_exact_checked_integer_math() {
        assert!(has_two_thirds(2, 1).unwrap());
        assert!(!has_two_thirds(1, 1).unwrap());
        assert!(has_two_thirds(i64::MAX, i64::MAX / 2).unwrap());
        assert!(has_two_thirds(-1, 0).is_err());
        assert!(has_two_thirds(0, -1).is_err());
    }

    #[test]
    fn malformed_deadlines_fail_closed() {
        assert!(check_before_deadline(Some("not-a-date"), "voting").is_err());
        assert!(check_after_deadline(Some("not-a-date"), "voting").is_err());
    }

    #[test]
    fn crossing_seat_tie_requires_runoff() {
        let ranked = vec![
            ("first".to_string(), 9),
            ("tied-a".to_string(), 4),
            ("tied-b".to_string(), 4),
        ];
        let error = select_unambiguous_winners(&ranked, 2).unwrap_err();
        assert!(error.contains("runoff"));
        assert_eq!(
            select_unambiguous_winners(&ranked, 1).unwrap(),
            vec!["first"]
        );
    }

    #[test]
    fn committee_replacement_rolls_back_and_retries_atomically() {
        let db = setup_db();
        db.conn()
            .execute(
                "INSERT INTO governance_elections (id, dao_id, title, phase, seats) \
                 VALUES ('election_replace', 'dao1', 'Replace', 'finalized', 2)",
                [],
            )
            .unwrap();
        for (id, address) in [("winner_a", "stake_new_a"), ("winner_b", "stake_new_b")] {
            db.conn()
                .execute(
                    "INSERT INTO governance_election_nominees \
                     (id, election_id, stake_address, accepted, is_winner) \
                     VALUES (?1, 'election_replace', ?2, 1, 1)",
                    params![id, address],
                )
                .unwrap();
        }
        for address in ["stake_old_a", "stake_old_b"] {
            db.conn()
                .execute(
                    "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
                     VALUES ('dao1', ?1, 'committee')",
                    [address],
                )
                .unwrap();
        }
        db.conn()
            .execute_batch(
                "CREATE TRIGGER fail_second_committee_member \
                 BEFORE INSERT ON governance_dao_members \
                 WHEN NEW.stake_address = 'stake_new_b' \
                 BEGIN SELECT RAISE(ABORT, 'injected committee failure'); END;",
            )
            .unwrap();

        let error = governance_transaction(&db, |connection| {
            replace_elected_committee(connection, "election_replace")
        })
        .unwrap_err();
        assert!(error.contains("injected committee failure"));
        let addresses = committee_addresses(db.conn());
        assert_eq!(addresses, vec!["stake_old_a", "stake_old_b"]);

        db.conn()
            .execute_batch("DROP TRIGGER fail_second_committee_member")
            .unwrap();
        governance_transaction(&db, |connection| {
            replace_elected_committee(connection, "election_replace")
        })
        .unwrap();
        assert_eq!(
            committee_addresses(db.conn()),
            vec!["stake_new_a", "stake_new_b"]
        );
    }

    fn committee_addresses(conn: &rusqlite::Connection) -> Vec<String> {
        let mut statement = conn
            .prepare(
                "SELECT stake_address FROM governance_dao_members \
                 WHERE dao_id = 'dao1' AND role = 'committee' ORDER BY stake_address",
            )
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn proposal_double_vote_prevention() {
        let db = setup_db();
        let conn = db.conn();

        let prop_id = entity_id(&["dao1", "prop", "p"]);
        conn.execute(
            "INSERT INTO governance_proposals \
             (id, dao_id, title, category, status, proposer, min_vote_proficiency) \
             VALUES (?1, 'dao1', 'Test', 'policy', 'published', 'proposer', 'remember')",
            params![prop_id],
        )
        .unwrap();

        let vid1 = entity_id(&[&prop_id, "v1"]);
        conn.execute(
            "INSERT INTO governance_proposal_votes (id, proposal_id, voter, in_favor) \
             VALUES (?1, ?2, 'voter1', 1)",
            params![vid1, prop_id],
        )
        .unwrap();

        let vid2 = entity_id(&[&prop_id, "v1", "2"]);
        let result = conn.execute(
            "INSERT INTO governance_proposal_votes (id, proposal_id, voter, in_favor) \
             VALUES (?1, ?2, 'voter1', 0)",
            params![vid2, prop_id],
        );
        assert!(result.is_err(), "double vote should fail");
    }

    #[test]
    fn election_winners_are_top_n_by_votes() {
        let db = setup_db();
        let conn = db.conn();

        let elec_id = entity_id(&["dao1", "winner_test", "now"]);
        conn.execute(
            "INSERT INTO governance_elections \
             (id, dao_id, title, phase, seats) VALUES (?1, 'dao1', 'Winner Test', 'voting', 2)",
            params![elec_id],
        )
        .unwrap();

        // 3 nominees with different vote counts
        for (i, votes) in [(1, 10), (2, 5), (3, 8)] {
            let nom_id = entity_id(&[&elec_id, &format!("nom{i}")]);
            conn.execute(
                "INSERT INTO governance_election_nominees \
                 (id, election_id, stake_address, accepted, votes_received) \
                 VALUES (?1, ?2, ?3, 1, ?4)",
                params![nom_id, elec_id, format!("stake{i}"), votes],
            )
            .unwrap();
        }

        // Get top 2 by votes
        let mut stmt = conn
            .prepare(
                "SELECT stake_address, votes_received FROM governance_election_nominees \
                 WHERE election_id = ?1 AND accepted = 1 \
                 ORDER BY votes_received DESC LIMIT 2",
            )
            .unwrap();

        let winners: Vec<(String, i64)> = stmt
            .query_map(params![elec_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(winners.len(), 2);
        assert_eq!(winners[0].1, 10); // nominee1 with 10 votes
        assert_eq!(winners[1].1, 8); // nominee3 with 8 votes
    }
}
