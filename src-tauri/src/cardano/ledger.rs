//! The `Ledger` port: the chain operations credential anchoring needs.
//!
//! Anchoring talks to Cardano only through this trait. Production uses the
//! Blockfrost client; tests use an in-memory ledger. Replacing Blockfrost with
//! direct relay submission and Mithril-certified reads means adding another
//! implementation, not changing the anchorer.

use std::future::Future;
use std::sync::OnceLock;

use alexandria_verify::vc::anchor::IntegrityAnchor;
use pallas_primitives::Metadatum;
use pallas_traverse::MultiEraTx;

use super::blockfrost::{BlockfrostClient, TransactionReceipt};
use super::script_refs::ALEXANDRIA_ANCHOR_LABEL;
use super::types::{ProtocolParameters, UTxO};

/// Version of the label-1697 anchor record this build writes and reads.
pub const ANCHOR_RECORD_VERSION: i64 = 2;

/// Chain reads and writes for anchoring. Every future is `Send` so a
/// background pass can hold one across `.await`.
pub trait Ledger: Sync {
    fn utxos(&self, address: &str) -> impl Future<Output = Result<Vec<UTxO>, String>> + Send;
    fn protocol_params(&self) -> impl Future<Output = Result<ProtocolParameters, String>> + Send;
    fn tip_slot(&self) -> impl Future<Output = Result<u64, String>> + Send;
    /// Execution units per redeemer, `(mem, steps)`.
    fn evaluate(&self, tx: &[u8]) -> impl Future<Output = Result<Vec<(u64, u64)>, String>> + Send;
    /// Submit signed bytes; the ledger's transaction id on acceptance.
    fn submit(&self, tx: &[u8]) -> impl Future<Output = Result<String, String>> + Send;
    /// Inclusion receipt; `None` while the transaction is not on chain.
    fn receipt(
        &self,
        tx_hash: &str,
    ) -> impl Future<Output = Result<Option<TransactionReceipt>, String>> + Send;
    /// The signed bytes of an included transaction.
    fn tx_cbor(
        &self,
        tx_hash: &str,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, String>> + Send;
}

/// The tip as one background pass sees it: requested from the ledger the
/// first time a step needs it, then shared by every later step of the pass.
/// A pass that needs no tip makes no request.
pub(crate) struct PassTip<'a, L> {
    ledger: &'a L,
    slot: OnceLock<Result<u64, String>>,
}

impl<'a, L: Ledger> PassTip<'a, L> {
    pub(crate) fn new(ledger: &'a L) -> Self {
        Self {
            ledger,
            slot: OnceLock::new(),
        }
    }

    /// The pass's tip, or the error its one request returned.
    pub(crate) async fn get(&self) -> Result<u64, String> {
        if let Some(slot) = self.slot.get() {
            return slot.clone();
        }
        let read = self.ledger.tip_slot().await;
        self.slot.get_or_init(|| read).clone()
    }
}

impl Ledger for BlockfrostClient {
    async fn utxos(&self, address: &str) -> Result<Vec<UTxO>, String> {
        match self.get_utxos(address).await {
            Ok(utxos) => Ok(utxos),
            // Blockfrost answers 404 for an address that never held funds.
            Err(super::blockfrost::BlockfrostError::Api { status: 404, .. }) => Ok(Vec::new()),
            Err(e) => Err(e.to_string()),
        }
    }
    async fn protocol_params(&self) -> Result<ProtocolParameters, String> {
        self.get_protocol_params().await.map_err(|e| e.to_string())
    }
    async fn tip_slot(&self) -> Result<u64, String> {
        self.get_tip_slot().await.map_err(|e| e.to_string())
    }
    async fn evaluate(&self, tx: &[u8]) -> Result<Vec<(u64, u64)>, String> {
        self.evaluate_tx(tx).await.map_err(|e| e.to_string())
    }
    async fn submit(&self, tx: &[u8]) -> Result<String, String> {
        self.submit_tx(tx).await.map_err(|e| e.to_string())
    }
    async fn receipt(&self, tx_hash: &str) -> Result<Option<TransactionReceipt>, String> {
        self.get_transaction_receipt(tx_hash)
            .await
            .map_err(|e| e.to_string())
    }
    async fn tx_cbor(&self, tx_hash: &str) -> Result<Option<Vec<u8>>, String> {
        match self.get_tx_cbor(tx_hash).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(super::blockfrost::BlockfrostError::Api { status: 404, .. }) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Observe an anchor on the ledger: the transaction must be included with a
/// valid script outcome, and its bytes must carry a version-2 anchor record.
/// `Ok(None)` when it is not (yet) on chain or is not an anchor.
pub async fn observe_anchor<L: Ledger>(
    ledger: &L,
    tx_hash: &str,
) -> Result<Option<IntegrityAnchor>, String> {
    let Some(receipt) = ledger.receipt(tx_hash).await? else {
        return Ok(None);
    };
    if !receipt.valid_contract {
        return Ok(None);
    }
    let Some(cbor) = ledger.tx_cbor(tx_hash).await? else {
        return Ok(None);
    };
    let anchored_at = receipt.block_time.and_then(posix_secs_rfc3339);
    Ok(parse_anchor_tx(&cbor, tx_hash, anchored_at))
}

fn posix_secs_rfc3339(secs: u64) -> Option<String> {
    chrono::DateTime::from_timestamp(i64::try_from(secs).ok()?, 0)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Read a version-2 anchor record out of signed transaction bytes. The bytes
/// must hash to `tx_hash`, so an observation is always of the named
/// transaction. `anchored_at` is when the ledger confirmed it, when known.
pub fn parse_anchor_tx(
    cbor: &[u8],
    tx_hash: &str,
    anchored_at: Option<String>,
) -> Option<IntegrityAnchor> {
    let tx = MultiEraTx::decode(cbor).ok()?;
    if !tx.hash().to_string().eq_ignore_ascii_case(tx_hash) {
        return None;
    }
    let Metadatum::Map(fields) = tx.metadata().find(ALEXANDRIA_ANCHOR_LABEL)?.clone() else {
        return None;
    };
    let text = |name: &str| {
        fields.iter().find_map(|(k, v)| match (k, v) {
            (Metadatum::Text(k), Metadatum::Text(v)) if k == name => Some(v.clone()),
            _ => None,
        })
    };
    let int = |name: &str| {
        fields.iter().find_map(|(k, v)| match (k, v) {
            (Metadatum::Text(k), Metadatum::Int(v)) if k == name => {
                i64::try_from(i128::from(*v)).ok()
            }
            _ => None,
        })
    };
    if int("v")? != ANCHOR_RECORD_VERSION {
        return None;
    }
    let required_signers = tx
        .required_signers()
        .as_alonzo()
        .map(|signers| signers.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    Some(IntegrityAnchor {
        root: text("root")?,
        leaf_count: u64::try_from(int("leaf_count")?).ok()?,
        issuer_did: text("issuer_did")?,
        required_signers,
        tx_hash: tx_hash.to_ascii_lowercase(),
        anchored_at,
    })
}
