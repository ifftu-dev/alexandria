//! Operator-signed governance on-chain builders for the lean model.
//!
//! Under lean 2′ only three governance facts touch the chain, all signed
//! by the operator key (3-A):
//!   * `publish_finalized_election` — a plain-create UTxO at the election
//!     script in the `Finalized` phase; `install_committee` references it
//!     to prove an election concluded for the DAO.
//!   * `install_committee` — spends the DAO state UTxO and recreates it
//!     with the new committee (winners), referencing the finalized
//!     election.
//!   * proposal outcomes are anchored as metadata (see `anchor_proposal`).
//!
//! Votes, nominations and intermediate phases stay off-chain (signed
//! gossip + an anchored Merkle root). These builders return signed CBOR;
//! the caller submits.

use pallas_crypto::hash::{Hash, Hasher};
use rusqlite::{params, Connection};

use super::blockfrost::BlockfrostClient;
use super::operator::OperatorKey;
use super::tx_builder::{sign_raw_tx, TxBuildError};
use super::{gov_tx_builder, plutus_data, plutus_spend, script_refs};

const MIN_SCRIPT_UTXO_LOVELACE: u64 = 3_000_000;

fn e2s(e: TxBuildError) -> String {
    e.to_string()
}

/// Resolve a member's on-chain payment key hash (28 bytes) from their
/// stake address via the stake-pubkey registry: the registered gossip
/// pubkey is the wallet's payment key, so its blake2b-224 is the VKH the
/// validators use for committee membership. `None` if unregistered.
pub fn stake_to_vkh(conn: &Connection, stake_address: &str, now_secs: i64) -> Option<[u8; 28]> {
    let pubkey_hex: String = conn
        .query_row(
            "SELECT public_key_hex FROM stake_pubkey_registry \
             WHERE stake_address = ?1 AND valid_from <= ?2 \
               AND (valid_until IS NULL OR valid_until > ?2) \
             ORDER BY valid_from DESC LIMIT 1",
            params![stake_address, now_secs],
            |r| r.get(0),
        )
        .ok()?;
    let bytes = hex::decode(pubkey_hex).ok()?;
    Some(*Hasher::<224>::hash(&bytes))
}

/// Plain-create a `Finalized` election UTxO at the election script,
/// operator-signed. Returns signed CBOR. `nominees`/`membership_subjects`
/// are left empty — `install_committee` only checks the phase + DAO link,
/// not the nominee set (the vote process is attested off-chain).
#[allow(clippy::too_many_arguments)]
pub async fn publish_finalized_election(
    bf: &BlockfrostClient,
    op: &OperatorKey,
    dao_policy: &[u8; 28],
    dao_token_name: &[u8],
    reputation_policy: &[u8; 28],
    seats: i64,
    nomination_end_ms: i64,
    voting_end_ms: i64,
) -> Result<Vec<u8>, String> {
    let vote_policy =
        gov_tx_builder::hash_from_hex_pub(script_refs::VOTE_MINTING_SCRIPT_HASH).map_err(e2s)?;
    let datum = plutus_data::encode_election_datum(
        dao_policy,
        dao_token_name,
        0, // election_id: not validated on-chain by install
        "finalized",
        seats,
        "remember",
        "remember",
        &[],
        reputation_policy,
        &[],
        nomination_end_ms,
        voting_end_ms,
        &vote_policy,
    )
    .map_err(e2s)?;
    let unsigned = gov_tx_builder::build_plain_create_unsigned(
        bf,
        &op.address,
        script_refs::ELECTION_SCRIPT_HASH,
        MIN_SCRIPT_UTXO_LOVELACE,
        &datum,
    )
    .await
    .map_err(e2s)?;
    sign_raw_tx(&unsigned, &op.private_key).map_err(e2s)
}

/// Parameters to install a new committee (read from the DB row set).
pub struct InstallParams<'a> {
    /// The live DAO state UTxO to spend (`txhash`, index) + its lovelace.
    pub dao_state_utxo: (&'a str, u64),
    pub dao_state_lovelace: u64,
    pub dao_policy: [u8; 28],
    /// DAO state-token asset-name bytes (`"dao" ++ scope_id`).
    pub dao_token_name: Vec<u8>,
    pub scope_type: String,
    pub scope_id: Vec<u8>,
    pub reputation_policy: [u8; 28],
    pub committee_size: i64,
    pub election_interval_ms: i64,
    /// The finalized-election UTxO referenced as proof (`txhash`, index).
    pub election_ref: (&'a str, u64),
    /// The new committee (winners) payment key hashes.
    pub committee_vkhs: Vec<[u8; 28]>,
    /// Term start (POSIX ms); `term_end = term_start + interval`.
    pub term_start_ms: i64,
}

/// Spend the DAO state UTxO and recreate it with the new committee,
/// referencing the finalized election. Operator-signed. Returns signed
/// CBOR.
pub async fn install_committee(
    bf: &BlockfrostClient,
    op: &OperatorKey,
    p: &InstallParams<'_>,
) -> Result<Vec<u8>, String> {
    let committee_refs: Vec<&[u8; 28]> = p.committee_vkhs.iter().collect();
    let new_datum = plutus_data::encode_dao_datum(
        &p.scope_type,
        &p.scope_id,
        &p.reputation_policy,
        &[],
        "remember",
        &p.dao_policy,
        &committee_refs,
        p.committee_size,
        p.election_interval_ms,
        p.term_start_ms,
        p.term_start_ms + p.election_interval_ms,
    )
    .map_err(e2s)?;

    let election_ref_hash =
        hex::decode(p.election_ref.0).map_err(|e| format!("election ref hash: {e}"))?;
    let redeemer = plutus_data::encode_dao_redeemer(
        "install_committee",
        Some((&election_ref_hash, p.election_ref.1)),
    )
    .map_err(e2s)?;

    let assets = [(
        Hash::<28>::from(p.dao_policy),
        p.dao_token_name.clone(),
        1i64,
    )];
    let signers = [op.payment_key_hash()];
    let refs = vec![script_refs::DAO_REGISTRY_REF_UTXO, p.election_ref];

    let unsigned = plutus_spend::build_spend_unsigned(
        bf,
        &plutus_spend::SpendScript {
            payment_address: &op.address,
            payment_key_extended: &[0u8; 64],
            required_signers: &signers,
            script_input: p.dao_state_utxo,
            script_input_lovelace: p.dao_state_lovelace,
            spend_redeemer: redeemer,
            continuing_address: gov_tx_builder::script_address(
                script_refs::DAO_REGISTRY_SCRIPT_HASH,
            )
            .map_err(e2s)?,
            continuing_lovelace: p.dao_state_lovelace,
            continuing_datum: new_datum,
            continuing_assets: &assets,
            reference_inputs: &refs,
            mint: None,
            invalid_from_slot: None,
            valid_from_slot: None,
        },
    )
    .await
    .map_err(e2s)?;
    sign_raw_tx(&unsigned, &op.private_key).map_err(e2s)
}
