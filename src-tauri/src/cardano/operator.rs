//! Operator signing key for governance admin/committee transactions.
//!
//! Under the 3-A signing model the platform operator holds a single
//! Cardano key (the treasury key baked into the deployed validators as
//! `authorized_admin` / `authorized_minter`). Governance admin actions —
//! DAO create, publishing a finalized election, committee install,
//! proposal-outcome anchors — are built unsigned by the queue and signed
//! with this key, then submitted. User wallets sign only their own
//! actions (votes, self-nominations).
//!
//! The key is a NORMAL Ed25519 key (not the BIP32-extended key the app
//! derives for user wallets), so it signs via `PrivateKey::Normal`. Only
//! the operator's deployment configures it; on every other node the key
//! is absent and the queue simply skips admin actions.
//!
//! Configuration (checked in order):
//!   * `OPERATOR_SKEY_PATH` — path to a cardano-cli signing-key file
//!     (`PaymentSigningKeyShelley_ed25519`, a JSON envelope whose
//!     `cborHex` wraps the 32-byte key).
//!   * `OPERATOR_SKEY_HEX` — the raw 32-byte key as hex (CI / tests).
//!
//! `OPERATOR_ADDRESS` (bech32) is the wallet the operator funds txs from
//! and receives change at; required alongside the key.

use pallas_crypto::hash::Hasher;
use pallas_crypto::key::ed25519::SecretKey;
use pallas_wallet::PrivateKey;

use super::tx_builder::TxBuildError;

/// A loaded operator identity: the signing key plus the funding address.
pub struct OperatorKey {
    pub private_key: PrivateKey,
    /// Bech32 payment address the operator funds from / receives change at.
    pub address: String,
}

impl OperatorKey {
    /// 28-byte payment key hash (blake2b-224 of the public key) — used as
    /// the `authorized_admin` signer the deployed validators check.
    pub fn payment_key_hash(&self) -> [u8; 28] {
        let pubkey = self.private_key.public_key();
        *Hasher::<224>::hash(pubkey.as_ref())
    }
}

/// Decode the 32-byte raw key out of a cardano-cli signing-key envelope's
/// `cborHex` field (a CBOR byte string: `0x58 0x20 || 32 bytes`).
fn key_bytes_from_cli_cbor_hex(cbor_hex: &str) -> Result<[u8; 32], TxBuildError> {
    let raw = hex::decode(cbor_hex.trim())
        .map_err(|e| TxBuildError::Cbor(format!("operator skey hex: {e}")))?;
    // Accept either a bare 32-byte key or the CBOR-wrapped form.
    let key: &[u8] = match raw.as_slice() {
        [0x58, 0x20, rest @ ..] if rest.len() == 32 => rest,
        b if b.len() == 32 => b,
        _ => {
            return Err(TxBuildError::Cbor(
                "operator skey cborHex is not a 32-byte ed25519 key".into(),
            ))
        }
    };
    let mut out = [0u8; 32];
    out.copy_from_slice(key);
    Ok(out)
}

/// Load the operator key from the environment, or `None` when this node
/// is not configured as an operator (the common case).
pub fn load_operator_key() -> Option<OperatorKey> {
    let address = std::env::var("OPERATOR_ADDRESS").ok()?;

    let key_bytes = if let Ok(path) = std::env::var("OPERATOR_SKEY_PATH") {
        let contents = std::fs::read_to_string(&path).ok()?;
        let envelope: serde_json::Value = serde_json::from_str(&contents).ok()?;
        let cbor_hex = envelope.get("cborHex")?.as_str()?;
        key_bytes_from_cli_cbor_hex(cbor_hex).ok()?
    } else if let Ok(hex_str) = std::env::var("OPERATOR_SKEY_HEX") {
        key_bytes_from_cli_cbor_hex(&hex_str).ok()?
    } else {
        return None;
    };

    let secret = SecretKey::from(key_bytes);
    Some(OperatorKey {
        private_key: PrivateKey::Normal(secret),
        address,
    })
}

/// True if this node has an operator key configured (i.e. may build +
/// sign governance admin/committee transactions).
pub fn is_operator() -> bool {
    std::env::var("OPERATOR_ADDRESS").is_ok()
        && (std::env::var("OPERATOR_SKEY_PATH").is_ok()
            || std::env::var("OPERATOR_SKEY_HEX").is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway, non-secret test key (all 0x11 bytes).
    const TEST_KEY: [u8; 32] = [0x11; 32];

    #[test]
    fn parses_bare_and_wrapped_cbor_hex() {
        let bare = hex::encode(TEST_KEY);
        assert_eq!(key_bytes_from_cli_cbor_hex(&bare).unwrap(), TEST_KEY);

        let wrapped = format!("5820{}", hex::encode(TEST_KEY));
        assert_eq!(key_bytes_from_cli_cbor_hex(&wrapped).unwrap(), TEST_KEY);
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(key_bytes_from_cli_cbor_hex("abcd").is_err());
    }

    /// Loads the configured operator key and prints its payment key hash.
    /// Run against the real treasury key to confirm it matches the
    /// `authorized_admin` baked into the deployed validators
    /// (6056de43cda476fa7c496bf58b30f89c9945fc7a68b4134d6ef38333):
    ///   OPERATOR_SKEY_PATH=/…/keys/treasury.skey \
    ///   OPERATOR_ADDRESS=addr_test1q… cargo test -p alexandria-node \
    ///     live_operator_key_pkh -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_operator_key_pkh() {
        let key = load_operator_key().expect("operator key configured");
        println!("OPERATOR_PKH:{}", hex::encode(key.payment_key_hash()));
        println!("OPERATOR_ADDRESS:{}", key.address);
    }

    #[test]
    fn payment_key_hash_is_stable_28_bytes() {
        let key = OperatorKey {
            private_key: PrivateKey::Normal(SecretKey::from(TEST_KEY)),
            address: "addr_test1xyz".into(),
        };
        let pkh = key.payment_key_hash();
        assert_eq!(pkh.len(), 28);
        // Deterministic: same key → same hash.
        let key2 = OperatorKey {
            private_key: PrivateKey::Normal(SecretKey::from(TEST_KEY)),
            address: "addr_test1xyz".into(),
        };
        assert_eq!(pkh, key2.payment_key_hash());
    }
}
