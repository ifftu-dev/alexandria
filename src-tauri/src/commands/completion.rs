//! Frontend-facing IPC for the completion-witness flow.
//!
//! * [`preview_completion_root`] — given an ordered set of element
//!   completions (element id, grader cid, submission hash, score,
//!   grader version), return the leaves + Merkle root the validator
//!   will require. The frontend uses this to confirm what it's about
//!   to submit before pulling the wallet.
//! * [`submit_completion_witness`] — derives the Merkle root, unlocks
//!   the vault, and submits the mint tx via the completion tx
//!   builder. Gated on `ALEXANDRIA_COMPLETION_POLICY_ID` + Blockfrost
//!   availability.
//!
//! These are the bridge between the plugin-reported completion state
//! and the on-chain witness the observer later ingests.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::cardano::{blockfrost::BlockfrostClient, completion_tx_builder};
use crate::crypto::wallet;
use crate::domain::completion::{element_leaf, merkle_root, ElementCompletion};
use crate::AppState;

#[derive(Debug, Clone, Deserialize)]
pub struct ElementCompletionInput {
    pub element_id: String,
    pub grader_cid: String,
    pub submission_hash: String,
    pub grader_version: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionRootPreview {
    /// Hex-encoded 32-byte leaves, one per element.
    pub leaves: Vec<String>,
    /// Hex-encoded 32-byte Merkle root.
    pub root: String,
}

fn compute_preview(inputs: &[ElementCompletionInput]) -> Result<CompletionRootPreview, String> {
    if inputs.is_empty() {
        return Err("course completion requires at least one element".into());
    }
    let leaves: Vec<[u8; 32]> = inputs
        .iter()
        .map(|e| {
            element_leaf(&ElementCompletion {
                element_id: &e.element_id,
                grader_cid: &e.grader_cid,
                submission_hash: &e.submission_hash,
                grader_version: &e.grader_version,
                score: e.score,
            })
        })
        .collect();
    let root = merkle_root(&leaves);
    Ok(CompletionRootPreview {
        leaves: leaves.iter().map(hex::encode).collect(),
        root: hex::encode(root),
    })
}

#[tauri::command]
pub async fn preview_completion_root(
    elements: Vec<ElementCompletionInput>,
) -> Result<CompletionRootPreview, String> {
    compute_preview(&elements)
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionWitnessResult {
    pub tx_hash: String,
    pub completion_root: String,
    pub leaves: Vec<String>,
}

#[tauri::command]
pub async fn submit_completion_witness(
    state: State<'_, AppState>,
    course_id: String,
    elements: Vec<ElementCompletionInput>,
    timestamp_ms: i64,
) -> Result<CompletionWitnessResult, String> {
    let preview = compute_preview(&elements)?;
    let leaves_hex = preview.leaves.clone();

    // Decode leaves to [u8; 32].
    let leaves: Vec<[u8; 32]> = leaves_hex
        .iter()
        .map(|h| {
            hex::decode(h)
                .ok()
                .and_then(|v| v.try_into().ok())
                .ok_or_else(|| format!("invalid leaf hex: {h}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let root_bytes: [u8; 32] = hex::decode(&preview.root)
        .map_err(|e| format!("invalid root hex: {e}"))?
        .try_into()
        .map_err(|_| "root must decode to 32 bytes".to_string())?;

    // Blockfrost — required for tx submission.
    let project_id = std::env::var("BLOCKFROST_PROJECT_ID").map_err(|_| {
        "BLOCKFROST_PROJECT_ID not set — cannot submit completion witness".to_string()
    })?;
    let bf = BlockfrostClient::new(project_id).map_err(|e| e.to_string())?;

    // Unlock the vault and derive the wallet.
    let wallet = {
        let ks_guard = state.keystore.lock().await;
        let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
        let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
        wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?
    };

    // Subject pubkey = learner Ed25519 verification key (32 bytes).
    let subject_pubkey: [u8; 32] = *wallet.signing_key.verifying_key().as_bytes();

    // Build + sign + submit.
    let built = completion_tx_builder::build_completion_mint_tx(
        &bf,
        &wallet.payment_address,
        &wallet.payment_key_hash,
        &wallet.payment_key_extended,
        &subject_pubkey,
        course_id.as_bytes(),
        &leaves,
        &root_bytes,
        timestamp_ms,
    )
    .await
    .map_err(|e| e.to_string())?;

    let submitted_hash = bf
        .submit_tx(&built.tx_cbor)
        .await
        .map_err(|e| e.to_string())?;

    Ok(CompletionWitnessResult {
        tx_hash: submitted_hash,
        completion_root: preview.root,
        leaves: leaves_hex,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_fails_on_empty_input() {
        assert!(compute_preview(&[]).is_err());
    }

    #[test]
    fn preview_matches_domain_merkle_root_for_single_element() {
        let inp = ElementCompletionInput {
            element_id: "el".into(),
            grader_cid: "cid".into(),
            submission_hash: "hash".into(),
            grader_version: "v".into(),
            score: 0.5,
        };
        let preview = compute_preview(std::slice::from_ref(&inp)).unwrap();
        let leaf = element_leaf(&ElementCompletion {
            element_id: &inp.element_id,
            grader_cid: &inp.grader_cid,
            submission_hash: &inp.submission_hash,
            grader_version: &inp.grader_version,
            score: inp.score,
        });
        assert_eq!(preview.root, hex::encode(leaf));
        assert_eq!(preview.leaves, vec![hex::encode(leaf)]);
    }

    #[test]
    fn preview_is_deterministic_across_calls() {
        let inputs = vec![
            ElementCompletionInput {
                element_id: "el_1".into(),
                grader_cid: "c1".into(),
                submission_hash: "h1".into(),
                grader_version: "v".into(),
                score: 0.8,
            },
            ElementCompletionInput {
                element_id: "el_2".into(),
                grader_cid: "c2".into(),
                submission_hash: "h2".into(),
                grader_version: "v".into(),
                score: 0.9,
            },
        ];
        let a = compute_preview(&inputs).unwrap();
        let b = compute_preview(&inputs).unwrap();
        assert_eq!(a.root, b.root);
        assert_eq!(a.leaves, b.leaves);
    }
}
