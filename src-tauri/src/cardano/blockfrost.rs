use std::time::Duration;

use reqwest::Client;
use rusqlite::Connection;
use serde::Deserialize;
use thiserror::Error;

use super::types::{ChainTip, ProtocolParameters, UTxO};
use crate::settings::registry::keys::CARDANO_BLOCKFROST_KEY;
use crate::settings::store::SettingsStore;

/// Resolve the Blockfrost project id from (in order):
/// 1. The per-device `cardano.blockfrost_project_id` setting, when a
///    DB handle is available and the value is non-empty.
/// 2. The `BLOCKFROST_PROJECT_ID` environment variable.
///
/// Returns `None` if neither source carries a value. Keeps the
/// settings-UI promise — "When set, overrides the
/// BLOCKFROST_PROJECT_ID env var" — true at every call site instead of
/// just where the env var was wired directly.
pub fn resolve_project_id(conn: Option<&Connection>) -> Option<String> {
    if let Some(conn) = conn {
        let from_setting = SettingsStore::get(conn, CARDANO_BLOCKFROST_KEY);
        let trimmed = from_setting.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    std::env::var("BLOCKFROST_PROJECT_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Minimal shape of an entry in `GET /assets/policy/{policy_id}`.
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyAsset {
    /// Concatenated `policy_id + asset_name_hex` (hex).
    pub asset: String,
    /// Current supply as a string-encoded integer.
    #[serde(default)]
    pub quantity: String,
}

/// Minimal shape of an entry in `GET /assets/{unit}/history`.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetHistoryEntry {
    pub tx_hash: String,
    /// `"minted"` or `"burned"`.
    pub action: Option<String>,
}

/// Outputs attached to a confirmed transaction, surfaced by
/// `GET /txs/{hash}/utxos`. We consume the outputs side — inline datums
/// and asset lists — and the inputs side (source addresses) for tx
/// provenance checks.
#[derive(Debug, Clone, Deserialize)]
pub struct TxUtxos {
    #[serde(default)]
    pub inputs: Vec<TxInput>,
    #[serde(default)]
    pub outputs: Vec<TxOutput>,
}

/// A single input from `GET /txs/{hash}/utxos`. Only the source address
/// is needed — verifying it proves who authored (signed) the tx.
#[derive(Debug, Clone, Deserialize)]
pub struct TxInput {
    pub address: String,
    /// Collateral inputs are also listed; skip them when checking
    /// authorship of the spend.
    #[serde(default)]
    pub collateral: bool,
}

/// One transaction carrying metadata under a queried label, from
/// `GET /metadata/txs/labels/{label}`.
#[derive(Debug, Clone, Deserialize)]
pub struct LabelMetadataTx {
    pub tx_hash: String,
    /// The metadata content under the label, as JSON.
    #[serde(default)]
    pub json_metadata: serde_json::Value,
}

/// A single output from `GET /txs/{hash}/utxos`.
#[derive(Debug, Clone, Deserialize)]
pub struct TxOutput {
    pub address: String,
    #[serde(default)]
    pub amount: Vec<super::types::AmountEntry>,
    /// Hex-encoded CBOR of the inline datum, when present.
    #[serde(default)]
    pub inline_datum: Option<String>,
    /// Hash of the inline datum (Blockfrost returns this even when the
    /// CBOR itself is absent on certain plans).
    #[serde(default)]
    pub data_hash: Option<String>,
    pub output_index: u32,
}

#[derive(Error, Debug)]
pub enum BlockfrostError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Blockfrost API error (status {status}): {body}")]
    Api { status: u16, body: String },
    #[error("deserialization failed: {0}")]
    Deserialize(String),
    #[error("missing Blockfrost project ID")]
    MissingProjectId,
    #[error("Blockfrost pagination limit reached for {endpoint} after {max_pages} pages")]
    PaginationLimit {
        endpoint: &'static str,
        max_pages: u32,
    },
}

/// Blockfrost REST API client for Cardano preprod testnet.
///
/// Provides the four endpoints needed for transaction construction:
/// - `GET /addresses/{addr}/utxos` — query UTxOs at an address
/// - `GET /epochs/latest/parameters` — current protocol parameters
/// - `GET /blocks/latest` — chain tip (current slot)
/// - `POST /tx/submit` — submit a signed CBOR transaction
#[derive(Debug, Clone)]
pub struct BlockfrostClient {
    client: Client,
    base_url: String,
    project_id: String,
    limits: RequestLimits,
}

/// Per-request deadlines for every chain-provider call, submissions and
/// queries alike. A timed-out submission is never treated as a rejection:
/// callers journal the signed bytes first and reconcile the same
/// transaction afterwards (D13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestLimits {
    /// Deadline for establishing the TCP/TLS connection.
    pub connect: Duration,
    /// Deadline for the whole request, from sending until the response
    /// body has been read.
    pub total: Duration,
}

impl RequestLimits {
    /// Approved production limits: 10-second connection, 30-second request.
    pub const PRODUCTION: Self = Self {
        connect: Duration::from_secs(10),
        total: Duration::from_secs(30),
    };
}

/// Minimal ledger receipt from GET /txs/{hash}; inclusion is not synonymous
/// with successful Plutus execution. See Blockfrost's `tx_content` schema.
#[derive(Debug, Clone, Deserialize)]
pub struct TransactionReceipt {
    pub hash: String,
    pub slot: u64,
    pub valid_contract: bool,
}

/// Preprod base URL.
const PREPROD_BASE_URL: &str = "https://cardano-preprod.blockfrost.io/api/v0";
const MAX_POLICY_ASSET_PAGES: u32 = 100;

impl BlockfrostClient {
    /// Create a new client for preprod testnet with the production
    /// [`RequestLimits`].
    pub fn new(project_id: String) -> Result<Self, BlockfrostError> {
        Self::build(
            project_id,
            PREPROD_BASE_URL.to_string(),
            RequestLimits::PRODUCTION,
        )
    }

    /// Create a client with a custom base URL (for testing).
    #[cfg(test)]
    pub fn with_base_url(project_id: String, base_url: String) -> Result<Self, BlockfrostError> {
        Self::build(project_id, base_url, RequestLimits::PRODUCTION)
    }

    /// Create a client with a custom base URL and shorter deadlines (for testing).
    #[cfg(test)]
    pub fn with_limits(
        project_id: String,
        base_url: String,
        limits: RequestLimits,
    ) -> Result<Self, BlockfrostError> {
        Self::build(project_id, base_url, limits)
    }

    fn build(
        project_id: String,
        base_url: String,
        limits: RequestLimits,
    ) -> Result<Self, BlockfrostError> {
        if project_id.is_empty() {
            return Err(BlockfrostError::MissingProjectId);
        }
        // Every request made through this client inherits both deadlines.
        let client = Client::builder()
            .connect_timeout(limits.connect)
            .timeout(limits.total)
            .build()
            .map_err(BlockfrostError::Http)?;

        Ok(Self {
            client,
            base_url,
            project_id,
            limits,
        })
    }

    /// Deadlines applied to every request made through this client.
    pub fn request_limits(&self) -> RequestLimits {
        self.limits
    }

    /// Fetch all UTxOs at the given bech32 address.
    pub async fn get_utxos(&self, address: &str) -> Result<Vec<UTxO>, BlockfrostError> {
        let url = format!("{}/addresses/{}/utxos", self.base_url, address);
        let resp = self
            .client
            .get(&url)
            .header("project_id", &self.project_id)
            .header("Content-Type", "application/json")
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status == 404 {
            // Address has no UTxOs (never funded)
            return Ok(vec![]);
        }
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(BlockfrostError::Api { status, body });
        }

        resp.json::<Vec<UTxO>>()
            .await
            .map_err(|e| BlockfrostError::Deserialize(e.to_string()))
    }

    /// Fetch the current epoch's protocol parameters.
    pub async fn get_protocol_params(&self) -> Result<ProtocolParameters, BlockfrostError> {
        let url = format!("{}/epochs/latest/parameters", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("project_id", &self.project_id)
            .header("Content-Type", "application/json")
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(BlockfrostError::Api { status, body });
        }

        resp.json::<ProtocolParameters>()
            .await
            .map_err(|e| BlockfrostError::Deserialize(e.to_string()))
    }

    /// Fetch the chain tip (current slot number).
    pub async fn get_tip_slot(&self) -> Result<u64, BlockfrostError> {
        let url = format!("{}/blocks/latest", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("project_id", &self.project_id)
            .header("Content-Type", "application/json")
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(BlockfrostError::Api { status, body });
        }

        let tip: ChainTip = resp
            .json()
            .await
            .map_err(|e| BlockfrostError::Deserialize(e.to_string()))?;

        Ok(tip.slot)
    }

    /// Submit a signed transaction (raw CBOR bytes) to the network.
    ///
    /// Returns the transaction hash on success.
    pub async fn submit_tx(&self, tx_cbor: &[u8]) -> Result<String, BlockfrostError> {
        let url = format!("{}/tx/submit", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("project_id", &self.project_id)
            .header("Content-Type", "application/cbor")
            .body(tx_cbor.to_vec())
            .send()
            .await?;

        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();

        if status != 200 && status != 202 {
            return Err(BlockfrostError::Api { status, body });
        }

        // Blockfrost returns the tx hash as a JSON string (with quotes)
        let tx_hash = body.trim().trim_matches('"').to_string();
        Ok(tx_hash)
    }

    /// Select the first UTxO with at least `min_lovelace` from the list.
    ///
    /// This is a simple linear-scan coin selection matching v1 behavior.
    /// Returns `None` if no UTxO meets the threshold. UTxOs carrying a
    /// reference script are skipped — consuming one would destroy a
    /// deployed validator's reference script and add the Conway
    /// reference-script fee.
    pub fn select_utxo(utxos: &[UTxO], min_lovelace: u64) -> Option<&UTxO> {
        utxos
            .iter()
            .find(|u| u.lovelace() >= min_lovelace && !u.has_reference_script())
    }

    // ---- Governance-specific endpoints ----

    /// Fetch UTxOs at a script address. Used to find DAO/election/proposal
    /// state UTxOs holding governance state tokens.
    pub async fn get_script_utxos(&self, address: &str) -> Result<Vec<UTxO>, BlockfrostError> {
        // Same endpoint as get_utxos — script addresses are regular addresses
        self.get_utxos(address).await
    }

    /// Find the UTxO holding a specific asset (policy_id + hex asset_name).
    /// Used to locate the current state UTxO for a DAO/election/proposal.
    pub async fn get_utxo_by_asset(
        &self,
        policy_id: &str,
        asset_name_hex: &str,
    ) -> Result<Option<UTxO>, BlockfrostError> {
        let asset = format!("{policy_id}{asset_name_hex}");
        let url = format!("{}/assets/{}/addresses", self.base_url, asset);
        let resp = self
            .client
            .get(&url)
            .header("project_id", &self.project_id)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status == 404 {
            return Ok(None);
        }
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(BlockfrostError::Api { status, body });
        }

        #[derive(serde::Deserialize)]
        struct AssetAddress {
            address: String,
        }

        let addrs: Vec<AssetAddress> = resp
            .json()
            .await
            .map_err(|e| BlockfrostError::Deserialize(e.to_string()))?;

        if let Some(first) = addrs.first() {
            let utxos = self.get_utxos(&first.address).await?;
            // Find the specific UTxO holding this asset
            Ok(utxos
                .into_iter()
                .find(|u| u.has_asset(policy_id, asset_name_hex)))
        } else {
            Ok(None)
        }
    }

    /// Evaluate a transaction to get execution unit estimates for Plutus scripts.
    /// Calls Blockfrost's `/utils/txs/evaluate` endpoint with the unsigned tx CBOR.
    pub async fn evaluate_tx(&self, tx_cbor: &[u8]) -> Result<Vec<(u64, u64)>, BlockfrostError> {
        let url = format!("{}/utils/txs/evaluate", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("project_id", &self.project_id)
            .header("Content-Type", "application/cbor")
            .body(tx_cbor.to_vec())
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(BlockfrostError::Api { status, body });
        }

        // Blockfrost returns: { "result": { "EvaluationResult": { "spend:0": { "memory": N, "steps": N }, ... } } }
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| BlockfrostError::Deserialize(e.to_string()))?;

        let mut units = Vec::new();
        if let Some(result) = body.get("result").and_then(|r| r.get("EvaluationResult")) {
            if let Some(obj) = result.as_object() {
                for (_key, val) in obj {
                    let mem = val.get("memory").and_then(|v| v.as_u64()).unwrap_or(0);
                    let steps = val.get("steps").and_then(|v| v.as_u64()).unwrap_or(0);
                    units.push((mem, steps));
                }
            }
        }

        Ok(units)
    }

    /// List every asset ever minted under a policy ID.
    ///
    /// Used by the completion observer to discover new completion
    /// witnesses. Blockfrost paginates at 100 entries by default; we
    /// use `count=100` and iterate until an empty page comes back so
    /// the observer catches up after a long offline stretch.
    pub async fn list_policy_assets(
        &self,
        policy_id: &str,
    ) -> Result<Vec<PolicyAsset>, BlockfrostError> {
        self.list_policy_assets_with_page_limit(policy_id, MAX_POLICY_ASSET_PAGES)
            .await
    }

    async fn list_policy_assets_with_page_limit(
        &self,
        policy_id: &str,
        max_pages: u32,
    ) -> Result<Vec<PolicyAsset>, BlockfrostError> {
        if max_pages == 0 {
            return Err(BlockfrostError::PaginationLimit {
                endpoint: "policy assets",
                max_pages,
            });
        }
        let mut out: Vec<PolicyAsset> = Vec::new();
        let mut page: u32 = 1;
        loop {
            let url = format!(
                "{}/assets/policy/{}?count=100&page={}",
                self.base_url, policy_id, page
            );
            let resp = self
                .client
                .get(&url)
                .header("project_id", &self.project_id)
                .send()
                .await?;

            let status = resp.status().as_u16();
            if status == 404 {
                return Ok(out);
            }
            if status != 200 {
                let body = resp.text().await.unwrap_or_default();
                return Err(BlockfrostError::Api { status, body });
            }

            let batch: Vec<PolicyAsset> = resp
                .json()
                .await
                .map_err(|e| BlockfrostError::Deserialize(e.to_string()))?;

            if batch.is_empty() {
                return Ok(out);
            }
            let done = batch.len() < 100;
            out.extend(batch);
            if done {
                return Ok(out);
            }
            if page == max_pages {
                return Err(BlockfrostError::PaginationLimit {
                    endpoint: "policy assets",
                    max_pages,
                });
            }
            page += 1;
        }
    }

    /// Fetch the set of addresses a specific asset has ever been
    /// observed at, along with the tx hash where it was minted.
    ///
    /// The completion witness is a one-shot mint per (learner, course),
    /// so we expect one entry. The tx hash is what lands in the VC's
    /// `witness.tx_hash` field.
    pub async fn get_asset_history(
        &self,
        asset_unit: &str,
    ) -> Result<Option<AssetHistoryEntry>, BlockfrostError> {
        let url = format!("{}/assets/{}/history?order=asc", self.base_url, asset_unit);
        let resp = self
            .client
            .get(&url)
            .header("project_id", &self.project_id)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status == 404 {
            return Ok(None);
        }
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(BlockfrostError::Api { status, body });
        }

        let entries: Vec<AssetHistoryEntry> = resp
            .json()
            .await
            .map_err(|e| BlockfrostError::Deserialize(e.to_string()))?;

        Ok(entries
            .into_iter()
            .find(|e| e.action.as_deref() == Some("minted")))
    }

    /// Fetch tx outputs (including any inline datums) for a confirmed
    /// transaction. Used by the completion observer to read back the
    /// `CompletionDatum` carried on the witness output.
    pub async fn get_tx_utxos(&self, tx_hash: &str) -> Result<TxUtxos, BlockfrostError> {
        let url = format!("{}/txs/{}/utxos", self.base_url, tx_hash);
        let resp = self
            .client
            .get(&url)
            .header("project_id", &self.project_id)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(BlockfrostError::Api { status, body });
        }

        resp.json::<TxUtxos>()
            .await
            .map_err(|e| BlockfrostError::Deserialize(e.to_string()))
    }

    /// Fetch transactions carrying metadata under `label`, most recent
    /// first. Uses `GET /metadata/txs/labels/{label}?order=desc`. Returns
    /// up to the first page (100) — registry updates are rare, and only
    /// the latest valid entry is used. A 404 (no such label yet) maps to
    /// an empty list.
    pub async fn get_metadata_by_label(
        &self,
        label: u64,
    ) -> Result<Vec<LabelMetadataTx>, BlockfrostError> {
        let url = format!(
            "{}/metadata/txs/labels/{}?order=desc&count=100",
            self.base_url, label
        );
        let resp = self
            .client
            .get(&url)
            .header("project_id", &self.project_id)
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status == 404 {
            return Ok(vec![]);
        }
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(BlockfrostError::Api { status, body });
        }
        resp.json::<Vec<LabelMetadataTx>>()
            .await
            .map_err(|e| BlockfrostError::Deserialize(e.to_string()))
    }

    /// Fetch the raw CBOR of a confirmed transaction.
    ///
    /// Uses `GET /txs/{hash}/cbor`, which returns JSON of shape
    /// `{"cbor": "<hex>"}`. Returns the decoded CBOR bytes ready for
    /// `pallas_primitives::conway::Tx::decode`.
    pub async fn get_tx_cbor(&self, tx_hash: &str) -> Result<Vec<u8>, BlockfrostError> {
        let url = format!("{}/txs/{}/cbor", self.base_url, tx_hash);
        let resp = self
            .client
            .get(&url)
            .header("project_id", &self.project_id)
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(BlockfrostError::Api { status, body });
        }
        #[derive(serde::Deserialize)]
        struct CborWrapper {
            cbor: String,
        }
        let wrapped: CborWrapper = resp
            .json()
            .await
            .map_err(|e| BlockfrostError::Deserialize(e.to_string()))?;
        hex::decode(wrapped.cbor.trim()).map_err(|e| BlockfrostError::Deserialize(e.to_string()))
    }

    /// Check if a transaction has been confirmed on-chain.
    ///
    /// Queries `GET /txs/{hash}`. Returns `true` if Blockfrost returns 200
    /// (transaction exists on-chain), `false` for 404 (not yet confirmed).
    pub async fn is_tx_confirmed(&self, tx_hash: &str) -> Result<bool, BlockfrostError> {
        Ok(self
            .get_transaction_receipt(tx_hash)
            .await?
            .is_some_and(|r| r.valid_contract))
    }

    pub async fn get_transaction_receipt(
        &self,
        tx_hash: &str,
    ) -> Result<Option<TransactionReceipt>, BlockfrostError> {
        let url = format!("{}/txs/{}", self.base_url, tx_hash);
        let resp = self
            .client
            .get(&url)
            .header("project_id", &self.project_id)
            .send()
            .await?;

        match resp.status().as_u16() {
            200 => {
                let receipt: TransactionReceipt = resp
                    .json()
                    .await
                    .map_err(|e| BlockfrostError::Deserialize(e.to_string()))?;
                if receipt.hash != tx_hash {
                    return Err(BlockfrostError::Deserialize(
                        "transaction receipt hash mismatch".into(),
                    ));
                }
                Ok(Some(receipt))
            }
            404 => Ok(None),
            status => {
                let body = resp.text().await.unwrap_or_default();
                Err(BlockfrostError::Api { status, body })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_project_id_is_error() {
        let result = BlockfrostClient::new(String::new());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlockfrostError::MissingProjectId
        ));
    }

    #[test]
    fn valid_client_creation() {
        let result = BlockfrostClient::new("preprodABCDEF123456".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn production_client_uses_approved_connect_and_request_limits() {
        assert_eq!(
            RequestLimits::PRODUCTION,
            RequestLimits {
                connect: Duration::from_secs(10),
                total: Duration::from_secs(30),
            }
        );
        let client = BlockfrostClient::new("preprodABCDEF123456".to_string()).unwrap();
        assert_eq!(client.request_limits(), RequestLimits::PRODUCTION);
        let test_client =
            BlockfrostClient::with_base_url("test".into(), "http://127.0.0.1:1".into()).unwrap();
        assert_eq!(test_client.request_limits(), RequestLimits::PRODUCTION);
    }

    #[tokio::test]
    async fn total_request_limit_applies_to_submit_and_query() {
        let chain = crate::cardano::test_chain::FakeChain::start(|_, _, _| {
            crate::cardano::test_chain::Reply::Stall
        })
        .await;
        let limits = RequestLimits {
            connect: Duration::from_secs(2),
            total: Duration::from_millis(200),
        };
        let client = chain.client(limits);
        assert_eq!(client.request_limits(), limits);
        let started = std::time::Instant::now();
        let submit = client.submit_tx(&[0x84]).await.unwrap_err();
        assert!(
            matches!(&submit, BlockfrostError::Http(error) if error.is_timeout()),
            "{submit}"
        );
        let query = client
            .get_transaction_receipt(&"a".repeat(64))
            .await
            .unwrap_err();
        assert!(
            matches!(&query, BlockfrostError::Http(error) if error.is_timeout()),
            "{query}"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            chain.requests(),
            vec![
                "POST /tx/submit".to_string(),
                format!("GET /txs/{}", "a".repeat(64))
            ]
        );
    }

    #[tokio::test]
    async fn policy_asset_scan_stops_at_its_page_limit() {
        let page = serde_json::to_string(
            &(0..100)
                .map(|index| {
                    serde_json::json!({
                        "asset": format!("policy{index:02}"),
                        "quantity": "1"
                    })
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let chain = crate::cardano::test_chain::FakeChain::start(move |method, path, _| {
            assert_eq!(method, "GET");
            assert!(path.starts_with("/assets/policy/policy?count=100&page="));
            crate::cardano::test_chain::Reply::Json(200, page.clone())
        })
        .await;
        let client = chain.client(crate::cardano::test_chain::SHORT_LIMITS);

        let error = client
            .list_policy_assets_with_page_limit("policy", 2)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            BlockfrostError::PaginationLimit {
                endpoint: "policy assets",
                max_pages: 2
            }
        ));
        assert_eq!(chain.requests().len(), 2);
    }

    #[test]
    fn select_utxo_finds_adequate() {
        use super::super::types::AmountEntry;
        let utxos = vec![
            UTxO {
                tx_hash: "aaa".into(),
                tx_index: 0,
                amount: vec![AmountEntry {
                    unit: "lovelace".into(),
                    quantity: "2000000".into(),
                }],
                reference_script_hash: None,
            },
            UTxO {
                tx_hash: "bbb".into(),
                tx_index: 1,
                amount: vec![AmountEntry {
                    unit: "lovelace".into(),
                    quantity: "10000000".into(),
                }],
                reference_script_hash: None,
            },
        ];
        let selected = BlockfrostClient::select_utxo(&utxos, 5_000_000);
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().tx_hash, "bbb");
    }

    #[test]
    fn select_utxo_none_when_insufficient() {
        use super::super::types::AmountEntry;
        let utxos = vec![UTxO {
            tx_hash: "aaa".into(),
            tx_index: 0,
            amount: vec![AmountEntry {
                unit: "lovelace".into(),
                quantity: "1000000".into(),
            }],
            reference_script_hash: None,
        }];
        let selected = BlockfrostClient::select_utxo(&utxos, 5_000_000);
        assert!(selected.is_none());
    }
}
