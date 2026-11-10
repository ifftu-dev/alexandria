//! Test-only, in-memory [`Ledger`]: holds UTxOs per address and a tip,
//! records every submission, and includes transactions when told to.

use std::collections::HashMap;
use std::sync::Mutex;

use super::blockfrost::TransactionReceipt;
use super::ledger::Ledger;
use super::types::{AmountEntry, ProtocolParameters, UTxO};

#[derive(Default)]
struct State {
    utxos: HashMap<String, Vec<UTxO>>,
    tip: u64,
    submitted: Vec<(String, Vec<u8>)>,
    /// tx hash → (slot, script succeeded)
    included: HashMap<String, (u64, bool)>,
    reject_submissions: Option<String>,
    evaluations: usize,
}

pub(crate) struct FakeLedger {
    state: Mutex<State>,
}

impl FakeLedger {
    pub(crate) fn at_tip(tip: u64) -> Self {
        Self {
            state: Mutex::new(State {
                tip,
                ..State::default()
            }),
        }
    }

    pub(crate) fn set_tip(&self, tip: u64) {
        self.state.lock().unwrap().tip = tip;
    }

    pub(crate) fn set_utxos(&self, address: &str, utxos: Vec<UTxO>) {
        self.state
            .lock()
            .unwrap()
            .utxos
            .insert(address.to_owned(), utxos);
    }

    /// Every submission so far: `(tx hash, signed bytes)`.
    pub(crate) fn submissions(&self) -> Vec<(String, Vec<u8>)> {
        self.state.lock().unwrap().submitted.clone()
    }

    pub(crate) fn evaluations(&self) -> usize {
        self.state.lock().unwrap().evaluations
    }

    /// Include a submitted transaction at `slot`.
    pub(crate) fn include(&self, tx_hash: &str, slot: u64) {
        self.state
            .lock()
            .unwrap()
            .included
            .insert(tx_hash.to_owned(), (slot, true));
    }

    /// Include a transaction whose script failed (collateral taken).
    pub(crate) fn include_failed(&self, tx_hash: &str, slot: u64) {
        self.state
            .lock()
            .unwrap()
            .included
            .insert(tx_hash.to_owned(), (slot, false));
    }

    /// Answer every later submission with this error.
    pub(crate) fn reject_submissions(&self, error: &str) {
        self.state.lock().unwrap().reject_submissions = Some(error.to_owned());
    }
}

/// A lovelace-only UTxO.
pub(crate) fn ada(tx_hash_byte: u8, index: u64, lovelace: u64) -> UTxO {
    UTxO {
        tx_hash: format!("{tx_hash_byte:02x}").repeat(32),
        tx_index: index,
        amount: vec![AmountEntry {
            unit: "lovelace".into(),
            quantity: lovelace.to_string(),
        }],
        reference_script_hash: None,
        inline_datum: None,
    }
}

impl Ledger for FakeLedger {
    async fn utxos(&self, address: &str) -> Result<Vec<UTxO>, String> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .utxos
            .get(address)
            .cloned()
            .unwrap_or_default())
    }
    async fn protocol_params(&self) -> Result<ProtocolParameters, String> {
        serde_json::from_value(serde_json::json!({
            "min_fee_a": 44, "min_fee_b": 155_381, "max_tx_size": 16_384
        }))
        .map_err(|e| e.to_string())
    }
    async fn tip_slot(&self) -> Result<u64, String> {
        Ok(self.state.lock().unwrap().tip)
    }
    async fn evaluate(&self, _tx: &[u8]) -> Result<Vec<(u64, u64)>, String> {
        self.state.lock().unwrap().evaluations += 1;
        Ok(vec![(400_000, 150_000_000)])
    }
    async fn submit(&self, tx: &[u8]) -> Result<String, String> {
        let hash = super::tx_builder::compute_tx_hash(tx).map_err(|e| e.to_string())?;
        let mut state = self.state.lock().unwrap();
        state.submitted.push((hash.clone(), tx.to_vec()));
        match &state.reject_submissions {
            Some(error) => Err(error.clone()),
            None => Ok(hash),
        }
    }
    async fn receipt(&self, tx_hash: &str) -> Result<Option<TransactionReceipt>, String> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .included
            .get(tx_hash)
            .map(|(slot, ok)| TransactionReceipt {
                hash: tx_hash.to_owned(),
                slot: *slot,
                valid_contract: *ok,
                block_time: None,
            }))
    }
    async fn tx_cbor(&self, tx_hash: &str) -> Result<Option<Vec<u8>>, String> {
        let state = self.state.lock().unwrap();
        if !state.included.contains_key(tx_hash) {
            return Ok(None);
        }
        Ok(state
            .submitted
            .iter()
            .find(|(hash, _)| hash == tx_hash)
            .map(|(_, bytes)| bytes.clone()))
    }
}
