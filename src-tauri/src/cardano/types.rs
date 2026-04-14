use serde::{Deserialize, Serialize};

/// A UTxO (Unspent Transaction Output) as returned by Blockfrost.
///
/// Blockfrost's live response includes BOTH `tx_index` and
/// `output_index` (identical values) for the same output slot —
/// a serde `alias` can't handle that, because `alias` means "one
/// of these spellings MAY appear" and with both present serde
/// raises a duplicate-field error. We deserialize via a shadow
/// struct that accepts either (or both) and prefers `tx_index`.
#[derive(Debug, Clone, Serialize)]
pub struct UTxO {
    /// Transaction hash (64-char hex).
    pub tx_hash: String,
    /// Output index within the transaction.
    pub tx_index: u64,
    /// Multi-asset amounts on this UTxO.
    pub amount: Vec<AmountEntry>,
}

impl<'de> Deserialize<'de> for UTxO {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Shadow {
            tx_hash: String,
            #[serde(default)]
            tx_index: Option<u64>,
            #[serde(default)]
            output_index: Option<u64>,
            amount: Vec<AmountEntry>,
        }
        let raw = Shadow::deserialize(d)?;
        let idx = raw
            .tx_index
            .or(raw.output_index)
            .ok_or_else(|| serde::de::Error::missing_field("tx_index"))?;
        Ok(UTxO {
            tx_hash: raw.tx_hash,
            tx_index: idx,
            amount: raw.amount,
        })
    }
}

/// A single asset amount within a UTxO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmountEntry {
    /// "lovelace" or "{policyId}{assetNameHex}".
    pub unit: String,
    /// String-encoded integer (e.g. "5000000").
    pub quantity: String,
}

impl UTxO {
    /// Extract the lovelace value from this UTxO.
    pub fn lovelace(&self) -> u64 {
        self.amount
            .iter()
            .find(|a| a.unit == "lovelace")
            .and_then(|a| a.quantity.parse::<u64>().ok())
            .unwrap_or(0)
    }

    /// Check if this UTxO holds a specific asset (policy_id + asset_name_hex).
    pub fn has_asset(&self, policy_id: &str, asset_name_hex: &str) -> bool {
        let full = format!("{policy_id}{asset_name_hex}");
        self.amount.iter().any(|a| a.unit == full)
    }
}

/// Protocol parameters from Blockfrost `/epochs/latest/parameters`.
/// Only the fields we need for fee calculation and min-UTxO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolParameters {
    /// Fee coefficient A (lovelace per byte).
    pub min_fee_a: u64,
    /// Fee constant B (base fee in lovelace).
    pub min_fee_b: u64,
    /// Maximum transaction size in bytes.
    pub max_tx_size: u64,
    /// Coins per UTxO byte (Conway-era).
    #[serde(default)]
    pub coins_per_utxo_size: Option<String>,
    /// Coins per UTxO word (Alonzo-era fallback).
    #[serde(default)]
    pub coins_per_utxo_word: Option<String>,
    /// Stake key registration deposit.
    #[serde(default)]
    pub key_deposit: Option<String>,
}

impl ProtocolParameters {
    /// Calculate the minimum fee for a transaction of the given byte size.
    /// Formula: `min_fee_a * tx_size + min_fee_b`
    pub fn calculate_min_fee(&self, tx_size_bytes: u64) -> u64 {
        self.min_fee_a * tx_size_bytes + self.min_fee_b
    }

    /// Get coins per UTxO byte (tries Conway-era field first, then Alonzo-era).
    pub fn coins_per_utxo_byte(&self) -> u64 {
        self.coins_per_utxo_size
            .as_deref()
            .or(self.coins_per_utxo_word.as_deref())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(4310) // sensible default
    }
}

/// Chain tip information from Blockfrost `/blocks/latest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainTip {
    pub slot: u64,
}

/// Result of a transaction submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxSubmissionResult {
    /// The transaction hash (64-char hex).
    pub tx_hash: String,
}

/// Result of minting a SkillProof NFT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintResult {
    /// Transaction hash of the mint.
    pub tx_hash: String,
    /// Policy ID (56-char hex, Blake2b-224 of the serialized NativeScript).
    pub policy_id: String,
    /// Asset name (UTF-8 string, e.g. "AlexProof12345678").
    pub asset_name: String,
}

/// Result of registering a course on-chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseRegistrationResult {
    /// Transaction hash of the registration.
    pub tx_hash: String,
    /// Policy ID of the course NFT.
    pub policy_id: String,
    /// Asset name of the course NFT.
    pub asset_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utxo_lovelace_extraction() {
        let utxo = UTxO {
            tx_hash: "abc123".into(),
            tx_index: 0,
            amount: vec![
                AmountEntry {
                    unit: "lovelace".into(),
                    quantity: "5000000".into(),
                },
                AmountEntry {
                    unit: "abc123def456".into(),
                    quantity: "1".into(),
                },
            ],
        };
        assert_eq!(utxo.lovelace(), 5_000_000);
    }

    #[test]
    fn utxo_no_lovelace() {
        let utxo = UTxO {
            tx_hash: "abc".into(),
            tx_index: 0,
            amount: vec![],
        };
        assert_eq!(utxo.lovelace(), 0);
    }

    #[test]
    fn protocol_params_fee_calculation() {
        let params = ProtocolParameters {
            min_fee_a: 44,
            min_fee_b: 155381,
            max_tx_size: 16384,
            coins_per_utxo_size: Some("4310".into()),
            coins_per_utxo_word: None,
            key_deposit: None,
        };
        // For a 300-byte tx: 44 * 300 + 155381 = 168581
        assert_eq!(params.calculate_min_fee(300), 168_581);
    }

    #[test]
    fn protocol_params_coins_per_utxo() {
        let params = ProtocolParameters {
            min_fee_a: 44,
            min_fee_b: 155381,
            max_tx_size: 16384,
            coins_per_utxo_size: Some("4310".into()),
            coins_per_utxo_word: Some("999".into()),
            key_deposit: None,
        };
        // Should prefer coins_per_utxo_size over coins_per_utxo_word
        assert_eq!(params.coins_per_utxo_byte(), 4310);

        let params_fallback = ProtocolParameters {
            min_fee_a: 44,
            min_fee_b: 155381,
            max_tx_size: 16384,
            coins_per_utxo_size: None,
            coins_per_utxo_word: Some("4310".into()),
            key_deposit: None,
        };
        assert_eq!(params_fallback.coins_per_utxo_byte(), 4310);
    }
}
