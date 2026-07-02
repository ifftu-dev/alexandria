//! Alexandria treasury payer — funds on-chain transactions.
//!
//! Policy: all blockchain transactions are funded by the Alexandria
//! treasury. When the treasury env config is present, tx builders pull
//! spend inputs + collateral from the treasury wallet and return change
//! to it, while the learner still signs to satisfy validator identity
//! checks (e.g. the completion policy's `extra_signatories` rule).
//!
//! Configuration (both required, e.g. via the dev `.env`):
//!   * `ALEXANDRIA_TREASURY_SKEY` — path to a cardano-cli
//!     `PaymentSigningKeyShelley_ed25519` JSON envelope.
//!   * `ALEXANDRIA_TREASURY_ADDR` — the treasury's bech32 address, or a
//!     path to a file containing it (cardano-cli `.addr` output).
//!
//! Absent/invalid config degrades gracefully: `from_env()` returns
//! `None` and callers fall back to learner-funded transactions.

use pallas_wallet::PrivateKey;
use serde::Deserialize;

/// A wallet that pays for a transaction: address for input selection +
/// change, key for witnessing its spent inputs.
pub struct TreasuryPayer {
    pub address: String,
    pub key: PrivateKey,
}

/// cardano-cli signing-key JSON envelope.
#[derive(Deserialize)]
struct KeyEnvelope {
    #[serde(rename = "cborHex")]
    cbor_hex: String,
}

impl TreasuryPayer {
    /// Load the treasury payer from the environment. `None` (with a log
    /// line explaining why) when unset or unreadable — callers then fall
    /// back to learner-funded txs.
    pub fn from_env() -> Option<Self> {
        let skey_path = std::env::var("ALEXANDRIA_TREASURY_SKEY").ok()?;
        let addr_raw = std::env::var("ALEXANDRIA_TREASURY_ADDR").ok()?;

        let key = match load_signing_key(&skey_path) {
            Ok(k) => k,
            Err(e) => {
                log::warn!("treasury payer disabled — bad signing key at {skey_path}: {e}");
                return None;
            }
        };

        // Accept either the literal bech32 address or a path to a file
        // holding it.
        let address = if addr_raw.starts_with("addr") {
            addr_raw.trim().to_string()
        } else {
            match std::fs::read_to_string(&addr_raw) {
                Ok(s) => s.trim().to_string(),
                Err(e) => {
                    log::warn!("treasury payer disabled — cannot read address {addr_raw}: {e}");
                    return None;
                }
            }
        };
        if !address.starts_with("addr") {
            log::warn!("treasury payer disabled — {addr_raw} does not hold a bech32 address");
            return None;
        }

        Some(TreasuryPayer { address, key })
    }
}

/// Parse a cardano-cli `PaymentSigningKeyShelley_ed25519` envelope into
/// a pallas signing key. The `cborHex` is CBOR `bytes(32)`: `5820` + 32
/// key bytes.
fn load_signing_key(path: &str) -> Result<PrivateKey, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let envelope: KeyEnvelope = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let cbor = hex::decode(envelope.cbor_hex.trim()).map_err(|e| e.to_string())?;
    let seed: [u8; 32] = cbor
        .strip_prefix(&[0x58, 0x20])
        .ok_or("cborHex is not a 32-byte CBOR byte string")?
        .try_into()
        .map_err(|_| "key payload is not 32 bytes".to_string())?;
    let secret = pallas_crypto::key::ed25519::SecretKey::from(seed);
    Ok(PrivateKey::Normal(secret))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cardano_cli_envelope() {
        let dir = std::env::temp_dir();
        let path = dir.join("treasury-test.skey");
        std::fs::write(
            &path,
            r#"{"type":"PaymentSigningKeyShelley_ed25519","description":"","cborHex":"58200101010101010101010101010101010101010101010101010101010101010101"}"#,
        )
        .unwrap();
        let key = load_signing_key(path.to_str().unwrap()).unwrap();
        // Deterministic seed → deterministic pubkey.
        assert_eq!(key.public_key().as_ref().len(), 32);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn rejects_wrong_length_payload() {
        let dir = std::env::temp_dir();
        let path = dir.join("treasury-test-bad.skey");
        std::fs::write(
            &path,
            r#"{"type":"PaymentSigningKeyShelley_ed25519","description":"","cborHex":"41ff"}"#,
        )
        .unwrap();
        assert!(load_signing_key(path.to_str().unwrap()).is_err());
        std::fs::remove_file(path).ok();
    }
}
