use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;

use super::types::{ChainTip, ProtocolParameters, UTxO};

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
/// `GET /txs/{hash}/utxos`. We only consume the outputs side — inline
/// datums and asset lists — but Blockfrost also returns inputs here.
#[derive(Debug, Clone, Deserialize)]
pub struct TxUtxos {
    #[serde(default)]
    pub outputs: Vec<TxOutput>,
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
}

/// Preprod base URL.
const PREPROD_BASE_URL: &str = "https://cardano-preprod.blockfrost.io/api/v0";

impl BlockfrostClient {
    /// Create a new client for preprod testnet.
    pub fn new(project_id: String) -> Result<Self, BlockfrostError> {
        if project_id.is_empty() {
            return Err(BlockfrostError::MissingProjectId);
        }
        let client = Client::builder().build().map_err(BlockfrostError::Http)?;

        Ok(Self {
            client,
            base_url: PREPROD_BASE_URL.to_string(),
            project_id,
        })
    }

    /// Create a client with a custom base URL (for testing).
    #[cfg(test)]
    pub fn with_base_url(project_id: String, base_url: String) -> Result<Self, BlockfrostError> {
        if project_id.is_empty() {
            return Err(BlockfrostError::MissingProjectId);
        }
        let client = Client::builder().build().map_err(BlockfrostError::Http)?;

        Ok(Self {
            client,
            base_url,
            project_id,
        })
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
    /// Returns `None` if no UTxO meets the threshold.
    pub fn select_utxo(utxos: &[UTxO], min_lovelace: u64) -> Option<&UTxO> {
        utxos.iter().find(|u| u.lovelace() >= min_lovelace)
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

    /// Check if a transaction has been confirmed on-chain.
    ///
    /// Queries `GET /txs/{hash}`. Returns `true` if Blockfrost returns 200
    /// (transaction exists on-chain), `false` for 404 (not yet confirmed).
    pub async fn is_tx_confirmed(&self, tx_hash: &str) -> Result<bool, BlockfrostError> {
        let url = format!("{}/txs/{}", self.base_url, tx_hash);
        let resp = self
            .client
            .get(&url)
            .header("project_id", &self.project_id)
            .send()
            .await?;

        match resp.status().as_u16() {
            200 => Ok(true),
            404 => Ok(false),
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
            },
            UTxO {
                tx_hash: "bbb".into(),
                tx_index: 1,
                amount: vec![AmountEntry {
                    unit: "lovelace".into(),
                    quantity: "10000000".into(),
                }],
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
        }];
        let selected = BlockfrostClient::select_utxo(&utxos, 5_000_000);
        assert!(selected.is_none());
    }
}
