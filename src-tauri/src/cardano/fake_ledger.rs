//! Test-only, in-memory [`Ledger`]: holds UTxOs per address and a tip,
//! records every submission, and includes transactions when told to.
//!
//! Like a node's mempool, it rejects a submission that spends an input of an
//! earlier accepted one that is still pending or included. Like a provider's
//! index, it keeps listing spent UTxOs until [`FakeLedger::catch_up`]. A
//! submitted transaction can be dropped so it never lands.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use pallas_primitives::conway::PseudoDatumOption;
use pallas_traverse::{MultiEraOutput, MultiEraTx};

use super::blockfrost::TransactionReceipt;
use super::ledger::Ledger;
use super::types::{AmountEntry, ProtocolParameters, UTxO};

type Outpoint = (String, u64);

#[derive(Default)]
struct State {
    utxos: HashMap<String, Vec<UTxO>>,
    tip: u64,
    submitted: Vec<(String, Vec<u8>)>,
    /// Submissions the mempool took, as opposed to rejected ones.
    accepted: HashSet<String>,
    /// tx hash → (slot, script succeeded)
    included: HashMap<String, (u64, bool)>,
    dropped: HashSet<String>,
    /// Included transactions already reflected in `utxos`.
    indexed: HashSet<String>,
    reject_submissions: Option<String>,
    evaluations: usize,
    tip_reads: usize,
}

/// What signed bytes spend, and until when they can land.
struct Spends {
    inputs: Vec<Outpoint>,
    collateral: Vec<Outpoint>,
    ttl: Option<u64>,
}

fn spends(bytes: &[u8]) -> Result<Spends, String> {
    let tx = MultiEraTx::decode(bytes).map_err(|e| e.to_string())?;
    let outpoints = |inputs: Vec<pallas_traverse::MultiEraInput<'_>>| {
        inputs
            .iter()
            .map(|input| (input.hash().to_string(), input.index()))
            .collect()
    };
    Ok(Spends {
        inputs: outpoints(tx.inputs()),
        collateral: outpoints(tx.collateral()),
        ttl: tx.ttl(),
    })
}

impl State {
    /// Outpoints the ledger or its mempool already consider spent: inputs of
    /// accepted transactions that are included or still pending (not
    /// dropped, tip before their bound), and collateral of included ones
    /// whose script failed.
    fn spent(&self) -> HashSet<Outpoint> {
        let mut spent = HashSet::new();
        for (hash, bytes) in &self.submitted {
            if !self.accepted.contains(hash) || self.dropped.contains(hash) {
                continue;
            }
            let Ok(tx) = spends(bytes) else { continue };
            match self.included.get(hash) {
                Some((_, true)) => spent.extend(tx.inputs),
                Some((_, false)) => spent.extend(tx.collateral),
                None if tx.ttl.is_none_or(|ttl| self.tip < ttl) => spent.extend(tx.inputs),
                None => {}
            }
        }
        spent
    }
}

fn as_utxo(tx_hash: &str, index: usize, output: &MultiEraOutput<'_>) -> Option<(String, UTxO)> {
    let address = output.address().ok()?.to_bech32().ok()?;
    let value = output.value();
    let mut amount = vec![AmountEntry {
        unit: "lovelace".into(),
        quantity: value.coin().to_string(),
    }];
    for policy in value.assets() {
        for asset in policy.assets() {
            amount.push(AmountEntry {
                unit: format!("{}{}", policy.policy(), hex::encode(asset.name())),
                quantity: asset.output_coin().unwrap_or_default().to_string(),
            });
        }
    }
    let inline_datum = match output.datum() {
        Some(PseudoDatumOption::Data(data)) => Some(hex::encode(data.0.raw_cbor())),
        _ => None,
    };
    Some((
        address,
        UTxO {
            tx_hash: tx_hash.to_owned(),
            tx_index: index as u64,
            amount,
            reference_script_hash: None,
            inline_datum,
        },
    ))
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

    /// How many times the tip was requested.
    pub(crate) fn tip_reads(&self) -> usize {
        self.state.lock().unwrap().tip_reads
    }

    /// Include a submitted transaction at `slot`.
    pub(crate) fn include(&self, tx_hash: &str, slot: u64) {
        self.land(tx_hash, slot, true);
    }

    /// Include a transaction whose script failed (collateral taken).
    pub(crate) fn include_failed(&self, tx_hash: &str, slot: u64) {
        self.land(tx_hash, slot, false);
    }

    fn land(&self, tx_hash: &str, slot: u64, valid_contract: bool) {
        let mut state = self.state.lock().unwrap();
        assert!(
            !state.dropped.contains(tx_hash),
            "a dropped transaction never lands"
        );
        state
            .included
            .insert(tx_hash.to_owned(), (slot, valid_contract));
    }

    /// The transaction never lands: no receipt, and its inputs are free.
    pub(crate) fn drop_tx(&self, tx_hash: &str) {
        let mut state = self.state.lock().unwrap();
        assert!(
            !state.included.contains_key(tx_hash),
            "an included transaction cannot be dropped"
        );
        state.dropped.insert(tx_hash.to_owned());
    }

    /// Bring UTxO listings up to date with included transactions: drop the
    /// outpoints they spent and add the outputs they created. Until this is
    /// called, listings lag like a provider's index.
    pub(crate) fn catch_up(&self) {
        let mut state = self.state.lock().unwrap();
        let landed: Vec<(String, Vec<u8>, bool)> = state
            .submitted
            .iter()
            .filter(|(hash, _)| !state.indexed.contains(hash))
            .filter_map(|(hash, bytes)| {
                let (_, valid) = state.included.get(hash)?;
                Some((hash.clone(), bytes.clone(), *valid))
            })
            .collect();
        for (hash, bytes, valid) in landed {
            let tx = MultiEraTx::decode(&bytes).expect("submitted bytes decode");
            let consumed = spends(&bytes).expect("submitted bytes decode");
            let consumed: HashSet<Outpoint> = if valid {
                consumed.inputs.into_iter().collect()
            } else {
                consumed.collateral.into_iter().collect()
            };
            for utxos in state.utxos.values_mut() {
                utxos.retain(|u| !consumed.contains(&(u.tx_hash.clone(), u.tx_index)));
            }
            let produced: Vec<(usize, MultiEraOutput<'_>)> = if valid {
                tx.outputs().into_iter().enumerate().collect()
            } else {
                tx.collateral_return()
                    .into_iter()
                    .map(|output| (tx.outputs().len(), output))
                    .collect()
            };
            for (index, output) in produced {
                if let Some((address, utxo)) = as_utxo(&hash, index, &output) {
                    state.utxos.entry(address).or_default().push(utxo);
                }
            }
            state.indexed.insert(hash);
        }
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
        let mut state = self.state.lock().unwrap();
        state.tip_reads += 1;
        Ok(state.tip)
    }
    async fn evaluate(&self, _tx: &[u8]) -> Result<Vec<(u64, u64)>, String> {
        self.state.lock().unwrap().evaluations += 1;
        Ok(vec![(400_000, 150_000_000)])
    }
    async fn submit(&self, tx: &[u8]) -> Result<String, String> {
        let hash = super::tx_builder::compute_tx_hash(tx).map_err(|e| e.to_string())?;
        let wants = spends(tx)?;
        let mut state = self.state.lock().unwrap();
        let spent = state.spent();
        state.submitted.push((hash.clone(), tx.to_vec()));
        if let Some(error) = &state.reject_submissions {
            return Err(error.clone());
        }
        if let Some((tx_hash, index)) = wants
            .inputs
            .iter()
            .chain(&wants.collateral)
            .find(|outpoint| spent.contains(*outpoint))
        {
            return Err(format!(
                "transaction submit error: BadInputsUTxO {tx_hash}#{index}"
            ));
        }
        state.accepted.insert(hash.clone());
        Ok(hash)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cardano::tx_builder;
    use pallas_addresses::Address;
    use pallas_crypto::{hash::Hash, key::ed25519::SecretKey};
    use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
    use pallas_wallet::PrivateKey;

    const WALLET: &str = "addr_test1qz2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3jcu5d8ps7zex2k2xt3uqxgjqnnj83ws8lhrn648jjxtwq2ytjqp";
    const TTL: u64 = 1_000;

    /// Signed bytes spending `inputs`, pledging `collateral`, paying 2 ADA to
    /// [`WALLET`], valid until `ttl`.
    fn tx(inputs: &[(u8, u64)], collateral: &[(u8, u64)], ttl: u64) -> (String, Vec<u8>) {
        let mut staging = StagingTransaction::new()
            .output(Output::new(
                Address::from_bech32(WALLET).unwrap(),
                2_000_000,
            ))
            .fee(200_000)
            .invalid_from_slot(ttl)
            .network_id(0);
        for (byte, index) in inputs {
            staging = staging.input(Input::new(Hash::from([*byte; 32]), *index));
        }
        for (byte, index) in collateral {
            staging = staging.collateral_input(Input::new(Hash::from([*byte; 32]), *index));
        }
        let built = staging.build_conway_raw().unwrap();
        let signed = tx_builder::sign_raw_tx(
            &built.tx_bytes.0,
            &PrivateKey::Normal(SecretKey::from([7; 32])),
        )
        .unwrap();
        (tx_builder::compute_tx_hash(&signed).unwrap(), signed)
    }

    async fn listed(ledger: &FakeLedger, address: &str) -> Vec<(String, u64, u64)> {
        let mut listed: Vec<_> = ledger
            .utxos(address)
            .await
            .unwrap()
            .iter()
            .map(|u| (u.tx_hash.clone(), u.tx_index, u.lovelace()))
            .collect();
        listed.sort();
        listed
    }

    fn is_bad_inputs(result: Result<String, String>) -> bool {
        result.is_err_and(|error| error.contains("BadInputsUTxO"))
    }

    #[tokio::test]
    async fn mempool_rejects_inputs_or_collateral_an_accepted_transaction_spends() {
        let ledger = FakeLedger::at_tip(0);
        let (first, bytes) = tx(&[(1, 0)], &[], TTL);
        assert_eq!(ledger.submit(&bytes).await, Ok(first));
        let (_, same_input) = tx(&[(1, 0)], &[], TTL);
        assert!(is_bad_inputs(ledger.submit(&same_input).await));
        let (_, as_collateral) = tx(&[(2, 0)], &[(1, 0)], TTL);
        assert!(is_bad_inputs(ledger.submit(&as_collateral).await));
        let (unrelated, bytes) = tx(&[(3, 0)], &[(4, 0)], TTL);
        assert_eq!(ledger.submit(&bytes).await, Ok(unrelated));
        // A rejected submission spends nothing.
        let (_, reuse_rejected) = tx(&[(2, 0)], &[], TTL);
        assert!(ledger.submit(&reuse_rejected).await.is_ok());
    }

    #[tokio::test]
    async fn inputs_free_up_when_a_pending_transaction_drops_or_expires_but_not_once_included() {
        let ledger = FakeLedger::at_tip(0);
        let (dropped, bytes) = tx(&[(1, 0)], &[], TTL);
        ledger.submit(&bytes).await.unwrap();
        ledger.drop_tx(&dropped);
        assert!(ledger.receipt(&dropped).await.unwrap().is_none());
        let (_, respend) = tx(&[(1, 0)], &[], TTL + 1);
        assert!(ledger.submit(&respend).await.is_ok());

        let (_, stale) = tx(&[(2, 0)], &[], TTL);
        ledger.submit(&stale).await.unwrap();
        let (included, bytes) = tx(&[(3, 0)], &[], TTL);
        ledger.submit(&bytes).await.unwrap();
        ledger.include(&included, TTL - 1);
        ledger.set_tip(TTL);
        let (_, after_expiry) = tx(&[(2, 0)], &[], TTL * 2);
        assert!(ledger.submit(&after_expiry).await.is_ok());
        let (_, spent) = tx(&[(3, 0)], &[], TTL * 2);
        assert!(is_bad_inputs(ledger.submit(&spent).await));
    }

    #[tokio::test]
    async fn index_lists_spent_utxos_until_it_catches_up() {
        let ledger = FakeLedger::at_tip(0);
        let funding = ada(1, 0, 10_000_000);
        let untouched = ada(5, 0, 3_000_000);
        ledger.set_utxos(WALLET, vec![funding.clone(), untouched.clone()]);
        let (spender, bytes) = tx(&[(1, 0)], &[], TTL);
        ledger.submit(&bytes).await.unwrap();
        ledger.include(&spender, 10);
        let (failed, bytes) = tx(&[(5, 0)], &[(6, 0)], TTL);
        ledger.submit(&bytes).await.unwrap();
        ledger.include_failed(&failed, 11);
        let (_, bytes) = tx(&[(7, 0)], &[], TTL);
        ledger.submit(&bytes).await.unwrap();
        assert!(listed(&ledger, WALLET)
            .await
            .contains(&(funding.tx_hash.clone(), 0, 10_000_000)));

        ledger.catch_up();
        // The included spend leaves and its output arrives; a script-failed
        // transaction keeps its inputs (only collateral goes) and pays
        // nothing; a pending one changes nothing.
        let mut expected = vec![(spender, 0, 2_000_000), (untouched.tx_hash, 0, 3_000_000)];
        expected.sort();
        assert_eq!(listed(&ledger, WALLET).await, expected);
    }

    #[tokio::test]
    #[should_panic(expected = "dropped")]
    async fn dropped_transaction_cannot_be_included() {
        let ledger = FakeLedger::at_tip(0);
        let (dropped, bytes) = tx(&[(1, 0)], &[], TTL);
        ledger.submit(&bytes).await.unwrap();
        ledger.drop_tx(&dropped);
        ledger.include(&dropped, 5);
    }
}
