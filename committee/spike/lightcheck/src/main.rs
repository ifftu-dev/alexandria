use std::fs;

use cometbft::block::signed_header::SignedHeader;
use cometbft::block::Block;
use cometbft::hash::Hash;
use cometbft::merkle::simple_hash_from_byte_vectors;
use cometbft::tx::Proof as TxProof;
use cometbft::validator::{Info, Set};
use cometbft_light_client_verifier::types::UntrustedBlockState;
use cometbft_light_client_verifier::{ProdVerifier, Verdict};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct RpcEnvelope<T> {
    result: T,
}

#[derive(Deserialize)]
struct CommitResult {
    signed_header: SignedHeader,
}

#[derive(Deserialize)]
struct TrustedGenesis {
    chain_id: cometbft::chain::Id,
    validators: Vec<Info>,
}

#[derive(Deserialize)]
struct BlockResult {
    block: Block,
}

#[derive(Deserialize)]
struct TxResult {
    tx_result: ExecutionResult,
    proof: TxProof,
}

#[derive(Deserialize)]
struct BlockResults {
    height: String,
    txs_results: Vec<ExecutionResult>,
}

#[derive(Deserialize)]
struct ExecutionResult {
    code: u32,
    data: Option<String>,
    gas_wanted: String,
    gas_used: String,
    events: Vec<serde_json::Value>,
    codespace: String,
}

impl ExecutionResult {
    fn is_accepted_default(&self) -> bool {
        self.code == 0
            && self.data.is_none()
            && self.gas_wanted == "0"
            && self.gas_used == "0"
            && self.events.is_empty()
            && self.codespace.is_empty()
    }
}

fn verify_signed_header(
    signed_header: &SignedHeader,
    validator_set: &Set,
    trusted_chain_id: &cometbft::chain::Id,
) -> Result<(), String> {
    if &signed_header.header.chain_id != trusted_chain_id {
        return Err("signed header chain ID does not match trusted genesis".to_owned());
    }
    let untrusted_block = UntrustedBlockState {
        signed_header,
        validators: validator_set,
        next_validators: Some(validator_set),
    };
    let verifier = ProdVerifier::default();
    let set_verdict = verifier.verify_validator_sets(&untrusted_block);
    let commit_verdict = verifier.verify_commit(&untrusted_block);
    if set_verdict != Verdict::Success || commit_verdict != Verdict::Success {
        return Err(format!(
            "verification failed: validator_set={set_verdict:?} commit={commit_verdict:?}"
        ));
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let commit_path = args.next().ok_or("missing commit JSON path")?;
    let post_state_commit_path = args.next().ok_or("missing post-state commit JSON path")?;
    let genesis_path = args.next().ok_or("missing trusted genesis JSON path")?;
    let block_path = args.next().ok_or("missing transaction block JSON path")?;
    let post_state_block_path = args.next().ok_or("missing post-state block JSON path")?;
    let block_results_path = args.next().ok_or("missing block results JSON path")?;
    let tx_path = args.next().ok_or("missing transaction proof JSON path")?;

    let commit: RpcEnvelope<CommitResult> =
        serde_json::from_slice(&fs::read(commit_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("parse commit response: {error}"))?;
    let post_state_commit: RpcEnvelope<CommitResult> = serde_json::from_slice(
        &fs::read(post_state_commit_path).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("parse post-state commit response: {error}"))?;
    let genesis: TrustedGenesis =
        serde_json::from_slice(&fs::read(genesis_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("parse trusted genesis: {error}"))?;
    let transaction_block: RpcEnvelope<BlockResult> =
        serde_json::from_slice(&fs::read(block_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("parse block response: {error}"))?;
    let post_state_block: RpcEnvelope<BlockResult> = serde_json::from_slice(
        &fs::read(post_state_block_path).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("parse post-state block response: {error}"))?;
    let block_results: RpcEnvelope<BlockResults> =
        serde_json::from_slice(&fs::read(block_results_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("parse block results response: {error}"))?;
    let tx: RpcEnvelope<TxResult> =
        serde_json::from_slice(&fs::read(tx_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("parse transaction response: {error}"))?;

    let validator_set = Set::without_proposer(genesis.validators);
    verify_signed_header(
        &commit.result.signed_header,
        &validator_set,
        &genesis.chain_id,
    )?;
    verify_signed_header(
        &post_state_commit.result.signed_header,
        &validator_set,
        &genesis.chain_id,
    )?;

    let proof = tx.result.proof;
    if proof.proof.total != 1 || proof.proof.index != 0 || !proof.proof.aunts.is_empty() {
        return Err("the G01 single-transaction fixture has an unexpected proof shape".to_owned());
    }
    if transaction_block.result.block.header != commit.result.signed_header.header {
        return Err("transaction block header does not match the signed header".to_owned());
    }
    if post_state_block.result.block.header != post_state_commit.result.signed_header.header {
        return Err("post-state block header does not match the signed header".to_owned());
    }
    if post_state_block.result.block.header.height
        != transaction_block.result.block.header.height.increment()
    {
        return Err("post-state commitment is not at transaction height H+1".to_owned());
    }
    let previous_block = post_state_block
        .result
        .block
        .header
        .last_block_id
        .ok_or("post-state block does not identify the transaction block")?;
    if previous_block.hash != transaction_block.result.block.header.hash() {
        return Err("post-state block is not linked to the transaction block".to_owned());
    }
    if transaction_block.result.block.data.as_slice() != [proof.data.as_slice()] {
        return Err("transaction proof data does not match the committed block data".to_owned());
    }
    // CometBFT commits a Merkle tree whose leaves are transaction hashes,
    // rather than the raw transaction bytes returned in the RPC proof.
    let transaction_hash = Sha256::digest(&proof.data);
    let leaf = simple_hash_from_byte_vectors::<cometbft::crypto::default::Sha256>(&[
        transaction_hash.as_slice(),
    ]);
    let leaf_hash = Hash::Sha256(leaf);
    if proof.proof.leaf_hash != leaf_hash || proof.root_hash != leaf_hash {
        return Err(format!(
            "single-transaction Merkle proof does not reconstruct its root: calculated={leaf_hash} leaf={} root={}",
            proof.proof.leaf_hash, proof.root_hash
        ));
    }
    if transaction_block.result.block.header.data_hash != Some(proof.root_hash) {
        return Err("transaction proof root does not match the signed block header".to_owned());
    }
    if !tx.result.tx_result.is_accepted_default()
        || block_results.result.height != transaction_block.result.block.header.height.to_string()
        || block_results.result.txs_results.len() != 1
        || !block_results.result.txs_results[0].is_accepted_default()
    {
        return Err("RPC execution result is not the accepted G01 result".to_owned());
    }
    // Every deterministic protobuf field in the accepted fixture result is its
    // default value, so its canonical encoding is empty.
    let results_root =
        simple_hash_from_byte_vectors::<cometbft::crypto::default::Sha256>(&[&[] as &[u8]]);
    if post_state_block.result.block.header.last_results_hash != Some(Hash::Sha256(results_root)) {
        return Err("execution result root does not match the signed H+1 header".to_owned());
    }

    let execution_height = i64::try_from(transaction_block.result.block.header.height.value())
        .map_err(|_| "execution height does not fit the state encoding")?;
    let mut state_hasher = Sha256::new();
    state_hasher.update(b"alexandria/g01-state/v1");
    state_hasher.update(execution_height.to_be_bytes());
    state_hasher.update(1_u64.to_be_bytes());
    let expected_app_hash = state_hasher.finalize();
    if post_state_block.result.block.header.app_hash.as_bytes() != expected_app_hash.as_slice() {
        return Err("post-state application hash does not match height=15 value=1".to_owned());
    }

    println!(
        "verified fixed-genesis commits at heights {} and {}, validators={}, transaction inclusion/result at height={}, and post-state value=1 at H+1",
        commit.result.signed_header.header.height,
        post_state_commit.result.signed_header.header.height,
        validator_set.validators().len(),
        transaction_block.result.block.header.height
    );
    Ok(())
}
