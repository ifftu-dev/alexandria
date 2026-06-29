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

use pallas_addresses::Address as PallasAddress;
use pallas_codec::utils::KeyValuePairs;
use pallas_crypto::hash::{Hash, Hasher};
use pallas_primitives::{Metadatum, MetadatumLabel};
use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
use rusqlite::{params, Connection};

use super::blockfrost::BlockfrostClient;
use super::operator::OperatorKey;
use super::tx_builder::{self, sign_raw_tx, TxBuildError, MIN_UTXO_LOVELACE, TTL_OFFSET};
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

/// Build the `{1697: {…}}` metadata for a resolved proposal: its id,
/// outcome, final tally, and an optional Merkle root committing to the
/// signed off-chain votes (so the tally is independently auditable).
/// Each `Metadatum::Text` stays within the 64-byte ledger limit (the id
/// and root are 64-hex / 64-byte values).
pub fn proposal_outcome_metadata(
    proposal_id: &str,
    status: &str,
    votes_for: i64,
    votes_against: i64,
    vote_merkle_root_hex: Option<String>,
) -> KeyValuePairs<MetadatumLabel, Metadatum> {
    let mut fields = vec![
        (
            Metadatum::Text("kind".into()),
            Metadatum::Text("proposal_outcome".into()),
        ),
        (
            Metadatum::Text("proposal_id".into()),
            Metadatum::Text(proposal_id.into()),
        ),
        (
            Metadatum::Text("status".into()),
            Metadatum::Text(status.into()),
        ),
        (
            Metadatum::Text("votes_for".into()),
            Metadatum::Int(votes_for.into()),
        ),
        (
            Metadatum::Text("votes_against".into()),
            Metadatum::Int(votes_against.into()),
        ),
        (Metadatum::Text("v".into()), Metadatum::Int(1.into())),
    ];
    if let Some(root) = vote_merkle_root_hex {
        fields.push((
            Metadatum::Text("vote_merkle_root".into()),
            Metadatum::Text(root),
        ));
    }
    KeyValuePairs::from(vec![(
        script_refs::ALEXANDRIA_ANCHOR_LABEL,
        Metadatum::Map(KeyValuePairs::from(fields)),
    )])
}

/// Operator-signed metadata anchor: a plain self-payment tx carrying the
/// given auxiliary-data map. Used to record proposal outcomes on-chain
/// (lean model — proposals don't run a spend validator). Returns signed
/// CBOR.
pub async fn build_governance_anchor(
    bf: &BlockfrostClient,
    op: &OperatorKey,
    metadata: KeyValuePairs<MetadatumLabel, Metadatum>,
) -> Result<Vec<u8>, String> {
    let (utxos_res, params_res, tip_res) = tokio::join!(
        bf.get_utxos(&op.address),
        bf.get_protocol_params(),
        bf.get_tip_slot(),
    );
    let utxos = utxos_res.map_err(|e| e.to_string())?;
    let params = params_res.map_err(|e| e.to_string())?;
    let tip_slot = tip_res.map_err(|e| e.to_string())?;

    let selected = BlockfrostClient::select_utxo(&utxos, MIN_UTXO_LOVELACE)
        .ok_or("no operator UTxO with sufficient lovelace for anchor")?;
    let addr = PallasAddress::from_bech32(&op.address).map_err(|e| e.to_string())?;
    let fee = tx_builder::estimate_fee(&params, 1);
    let change = selected
        .lovelace()
        .checked_sub(fee)
        .ok_or("operator UTxO cannot cover anchor fee")?;
    let input_hash = tx_builder::parse_tx_hash(&selected.tx_hash).map_err(e2s)?;

    let staging = StagingTransaction::new()
        .input(Input::new(input_hash, selected.tx_index))
        .output(Output::new(addr, change))
        .disclosed_signer(Hash::<28>::from(op.payment_key_hash()))
        .fee(fee)
        .invalid_from_slot(tip_slot + TTL_OFFSET)
        .network_id(0);
    let built = staging.build_conway_raw().map_err(|e| e.to_string())?;
    let (with_meta, _) = tx_builder::inject_metadata(&built.tx_bytes.0, metadata).map_err(e2s)?;
    sign_raw_tx(&with_meta, &op.private_key).map_err(e2s)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Live preprod check for the operator-signed metadata anchor — the
    /// one new tx shape in the queue rewrite (finalize-publish reuses the
    /// verified plain-create; install reuses verified plutus_spend). Run:
    ///   BLOCKFROST_PROJECT_ID=… OPERATOR_SKEY_PATH=…/treasury.skey \
    ///   OPERATOR_ADDRESS=addr_test1q… cargo test -p alexandria-node \
    ///     live_governance_anchor -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_governance_anchor() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let op = super::super::operator::load_operator_key().expect("operator key");
        let meta = proposal_outcome_metadata(
            "testproposal000000000000000000000000000000000000000000000000abcd",
            "approved",
            3,
            1,
            Some("aa".repeat(32)),
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tx_hash = rt.block_on(async {
            let bf = BlockfrostClient::new(pid).unwrap();
            let signed = build_governance_anchor(&bf, &op, meta)
                .await
                .expect("build anchor");
            bf.submit_tx(&signed).await.expect("submit")
        });
        println!("ANCHOR_TX:{tx_hash}");
    }

    /// Live preprod END-TO-END of the SHIPPED on-chain bridge builders,
    /// chained against the existing `daoopx` DAO (tx 3a3e56b0, state UTxO
    /// #0): publish a finalized election → install the committee
    /// (spends the DAO state UTxO, references the finalized election) →
    /// anchor a proposal outcome. Each tx is operator-signed in-process
    /// and waited for confirmation. Run:
    ///   BLOCKFROST_PROJECT_ID=… OPERATOR_SKEY_PATH=…/treasury.skey \
    ///   OPERATOR_ADDRESS=addr_test1q… cargo test -p alexandria-node \
    ///     live_gov_onchain_e2e -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_gov_onchain_e2e() {
        use std::time::Duration;
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let op = super::super::operator::load_operator_key().expect("operator key");

        // The `daoopx` DAO created on preprod (tx 3a3e56b0). Its state
        // token "daoopx" sits at the registry at output #0 (3 ADA).
        let dao_state_tx = "3a3e56b070e5b3b582fe497c6d7a7bba3f40b3f18b3581549e288aba0be37e3c";
        let dao_policy =
            gov_tx_builder::hash_from_hex_pub(script_refs::DAO_MINTING_SCRIPT_HASH).unwrap();
        let rep_policy =
            gov_tx_builder::hash_from_hex_pub(script_refs::REPUTATION_MINTING_SCRIPT_HASH).unwrap();
        let dao_token = b"daoopx".to_vec();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let interval = 2_592_000_000i64;

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let bf = BlockfrostClient::new(pid).unwrap();

            async fn wait_conf(bf: &BlockfrostClient, tx: &str) {
                for _ in 0..50 {
                    if bf.is_tx_confirmed(tx).await.unwrap_or(false) {
                        return;
                    }
                    tokio::time::sleep(Duration::from_secs(6)).await;
                }
                panic!("timeout waiting for {tx}");
            }

            // 1. Publish a finalized-election UTxO (operator-signed).
            let signed_f = publish_finalized_election(
                &bf,
                &op,
                &dao_policy,
                &dao_token,
                &rep_policy,
                1,
                now - 172_800_000,
                now - 86_400_000,
            )
            .await
            .expect("build finalize");
            let tx_f = bf.submit_tx(&signed_f).await.expect("submit finalize");
            println!("E2E_FINALIZE_TX:{tx_f}");
            wait_conf(&bf, &tx_f).await;

            // 2. Install the committee: spend the DAO state UTxO,
            //    referencing the finalized election just published.
            let params = InstallParams {
                dao_state_utxo: (dao_state_tx, 0),
                dao_state_lovelace: 3_000_000,
                dao_policy,
                dao_token_name: dao_token.clone(),
                scope_type: "subject".into(),
                scope_id: b"opx".to_vec(),
                reputation_policy: rep_policy,
                committee_size: 1,
                election_interval_ms: interval,
                election_ref: (&tx_f, 0),
                committee_vkhs: vec![op.payment_key_hash()],
                term_start_ms: now,
            };
            let signed_i = install_committee(&bf, &op, &params)
                .await
                .expect("build install");
            let tx_i = bf.submit_tx(&signed_i).await.expect("submit install");
            println!("E2E_INSTALL_TX:{tx_i}");
            wait_conf(&bf, &tx_i).await;

            // 3. Anchor a resolved proposal outcome (operator metadata).
            let meta = proposal_outcome_metadata(
                "e2eproposal0000000000000000000000000000000000000000000000000abc",
                "approved",
                5,
                2,
                Some("bb".repeat(32)),
            );
            let signed_a = build_governance_anchor(&bf, &op, meta)
                .await
                .expect("build anchor");
            let tx_a = bf.submit_tx(&signed_a).await.expect("submit anchor");
            println!("E2E_ANCHOR_TX:{tx_a}");
            wait_conf(&bf, &tx_a).await;

            println!("E2E_ALL_CONFIRMED");
        });
    }
}
