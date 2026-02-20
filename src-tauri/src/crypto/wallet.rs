use bip39::{Language, Mnemonic};
use ed25519_dalek::SigningKey;
use hmac::{Hmac, Mac};
use sha2::Sha512;
use thiserror::Error;

use super::hash::blake2b_256;

type HmacSha512 = Hmac<Sha512>;

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("invalid mnemonic phrase: {0}")]
    InvalidMnemonic(String),
    #[error("key derivation failed: {0}")]
    DerivationFailed(String),
    #[error("invalid seed length")]
    InvalidSeedLength,
}

/// A Cardano wallet derived from a BIP-39 mnemonic.
///
/// Key derivation follows CIP-1852 (purpose=1852, coin_type=1815):
///   m / 1852' / 1815' / 0' / role / index
///
/// We derive:
///   - Payment key: m/1852'/1815'/0'/0/0 (for receiving NFTs)
///   - Stake key:   m/1852'/1815'/0'/2/0 (persistent identity)
///   - Signing key: m/1852'/1815'/0'/0/0 (for Ed25519 signatures)
#[derive(Debug, Clone)]
pub struct Wallet {
    /// The BIP-39 mnemonic phrase (24 words).
    pub mnemonic: String,
    /// Ed25519 signing key derived at m/1852'/1815'/0'/0/0.
    pub signing_key: SigningKey,
    /// The Cardano stake address (bech32, starts with "stake1...").
    /// Derived from the stake key at m/1852'/1815'/0'/2/0.
    pub stake_address: String,
    /// The Cardano payment address (bech32, starts with "addr1...").
    pub payment_address: String,
}

/// Extended private key for BIP32-Ed25519 derivation.
/// Cardano uses a non-standard BIP32 variant (Icarus/Byron-era derivation).
#[derive(Clone)]
struct ExtendedKey {
    /// The 64-byte extended secret key (left 32 = scalar, right 32 = chain extension).
    secret: [u8; 64],
    /// The 32-byte chain code.
    chain_code: [u8; 32],
}

/// Generate a new wallet with a fresh 24-word mnemonic.
pub fn generate_wallet() -> Result<Wallet, WalletError> {
    let mnemonic = Mnemonic::generate_in(Language::English, 24)
        .map_err(|e| WalletError::InvalidMnemonic(e.to_string()))?;
    wallet_from_mnemonic(&mnemonic.to_string())
}

/// Restore a wallet from an existing mnemonic phrase.
pub fn wallet_from_mnemonic(phrase: &str) -> Result<Wallet, WalletError> {
    let mnemonic = Mnemonic::parse_in(Language::English, phrase)
        .map_err(|e| WalletError::InvalidMnemonic(e.to_string()))?;

    // Derive the master key using Icarus-style derivation:
    // PBKDF2-HMAC-SHA512 with password="" and the entropy as the seed.
    // For simplicity in Phase 1, we use the BIP-39 seed directly with
    // HMAC-SHA512 (ed25519 seed) — this will be upgraded to full
    // Icarus/CIP-1852 derivation when the Cardano integration matures.
    let entropy = mnemonic.to_entropy();
    let master = derive_master_key(&entropy)?;

    // Derive payment key: m/1852'/1815'/0'/0/0
    let account = derive_hardened(&master, &[1852, 1815, 0])?;
    let payment_key = derive_soft(&account, &[0, 0])?;

    // Derive stake key: m/1852'/1815'/0'/2/0
    let stake_key = derive_soft(&account, &[2, 0])?;

    // Extract the Ed25519 signing key (left 32 bytes of extended secret)
    let signing_bytes: [u8; 32] = payment_key.secret[..32]
        .try_into()
        .map_err(|_| WalletError::DerivationFailed("signing key extraction failed".into()))?;
    let signing_key = SigningKey::from_bytes(&signing_bytes);

    // Compute addresses from public key hashes
    let payment_pub = signing_key.verifying_key().to_bytes();
    let stake_pub_bytes: [u8; 32] = {
        let sk: [u8; 32] = stake_key.secret[..32]
            .try_into()
            .map_err(|_| WalletError::DerivationFailed("stake key extraction failed".into()))?;
        let sk = SigningKey::from_bytes(&sk);
        sk.verifying_key().to_bytes()
    };

    // Cardano addresses use Blake2b-224 hashes of public keys.
    // For Phase 1, we use a simplified hex representation.
    // Full bech32 encoding will be added with the Cardano integration.
    let payment_hash = blake2b_256(&payment_pub);
    let stake_hash = blake2b_256(&stake_pub_bytes);

    // Placeholder address format — will be replaced with proper bech32
    // (addr1... / stake1...) when pallas or cardano-serialization-lib
    // is integrated in Phase 2.
    let payment_address = format!("addr1_{}", hex::encode(&payment_hash[..28]));
    let stake_address = format!("stake1_{}", hex::encode(&stake_hash[..28]));

    Ok(Wallet {
        mnemonic: mnemonic.to_string(),
        signing_key,
        stake_address,
        payment_address,
    })
}

/// Derive the master extended key from entropy using HMAC-SHA512.
fn derive_master_key(entropy: &[u8]) -> Result<ExtendedKey, WalletError> {
    let mut mac = HmacSha512::new_from_slice(b"ed25519 cardano seed")
        .map_err(|e| WalletError::DerivationFailed(e.to_string()))?;
    mac.update(entropy);
    let hmac_out: [u8; 64] = mac
        .finalize()
        .into_bytes()
        .as_slice()
        .try_into()
        .map_err(|_| WalletError::InvalidSeedLength)?;

    let mut secret = [0u8; 64];
    secret.copy_from_slice(&hmac_out);

    // Clamp the scalar (Cardano Ed25519-BIP32 requirement)
    secret[0] &= 0b1111_1000;
    secret[31] &= 0b0111_1111;
    secret[31] |= 0b0100_0000;

    // Chain code: HMAC-SHA512 with different key
    let mut cc_mac = HmacSha512::new_from_slice(b"ed25519 cardano chaincode")
        .map_err(|e| WalletError::DerivationFailed(e.to_string()))?;
    cc_mac.update(entropy);
    let cc_out: [u8; 64] = cc_mac
        .finalize()
        .into_bytes()
        .as_slice()
        .try_into()
        .map_err(|_| WalletError::InvalidSeedLength)?;

    let mut chain_code = [0u8; 32];
    chain_code.copy_from_slice(&cc_out[..32]);

    Ok(ExtendedKey { secret, chain_code })
}

/// Derive a hardened child key (index >= 2^31).
fn derive_hardened(parent: &ExtendedKey, indices: &[u32]) -> Result<ExtendedKey, WalletError> {
    let mut current = parent.clone();
    for &idx in indices {
        let hardened_idx = idx + 0x8000_0000;
        let mut data = Vec::with_capacity(1 + 64 + 4);
        data.push(0x00); // Hardened derivation prefix
        data.extend_from_slice(&current.secret);
        data.extend_from_slice(&hardened_idx.to_le_bytes());

        let mut mac = HmacSha512::new_from_slice(&current.chain_code)
            .map_err(|e| WalletError::DerivationFailed(e.to_string()))?;
        mac.update(&data);
        let hmac_bytes = mac.finalize().into_bytes();
        let result: &[u8] = hmac_bytes.as_slice();

        let mut child_secret = [0u8; 64];
        child_secret[..32].copy_from_slice(&result[..32]);
        child_secret[32..64].copy_from_slice(&current.secret[32..64]);

        // Clamp
        child_secret[0] &= 0b1111_1000;
        child_secret[31] &= 0b0111_1111;
        child_secret[31] |= 0b0100_0000;

        let mut child_cc = [0u8; 32];
        child_cc.copy_from_slice(&result[32..64]);

        current = ExtendedKey {
            secret: child_secret,
            chain_code: child_cc,
        };
    }
    Ok(current)
}

/// Derive a soft (non-hardened) child key (index < 2^31).
fn derive_soft(parent: &ExtendedKey, indices: &[u32]) -> Result<ExtendedKey, WalletError> {
    let mut current = parent.clone();
    for &idx in indices {
        // Soft derivation uses the public key
        let sk_bytes: [u8; 32] = current.secret[..32]
            .try_into()
            .map_err(|_| WalletError::DerivationFailed("key slice failed".into()))?;
        let sk = SigningKey::from_bytes(&sk_bytes);
        let pk = sk.verifying_key().to_bytes();

        let mut data = Vec::with_capacity(1 + 32 + 4);
        data.push(0x02); // Soft derivation prefix
        data.extend_from_slice(&pk);
        data.extend_from_slice(&idx.to_le_bytes());

        let mut mac = HmacSha512::new_from_slice(&current.chain_code)
            .map_err(|e| WalletError::DerivationFailed(e.to_string()))?;
        mac.update(&data);
        let hmac_bytes = mac.finalize().into_bytes();
        let result: &[u8] = hmac_bytes.as_slice();

        let mut child_secret = [0u8; 64];
        // Add parent scalar + derived scalar (mod L for Ed25519)
        // Simplified: wrapping_add for Phase 1, will use proper scalar
        // addition when pallas is integrated.
        for i in 0..32 {
            child_secret[i] = current.secret[i].wrapping_add(result[i]);
        }
        child_secret[32..64].copy_from_slice(&current.secret[32..64]);

        // Clamp
        child_secret[0] &= 0b1111_1000;
        child_secret[31] &= 0b0111_1111;
        child_secret[31] |= 0b0100_0000;

        let mut child_cc = [0u8; 32];
        child_cc.copy_from_slice(&result[32..64]);

        current = ExtendedKey {
            secret: child_secret,
            chain_code: child_cc,
        };
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_wallet_produces_valid_wallet() {
        let wallet = generate_wallet().expect("wallet generation failed");
        assert_eq!(wallet.mnemonic.split_whitespace().count(), 24);
        assert!(wallet.stake_address.starts_with("stake1_"));
        assert!(wallet.payment_address.starts_with("addr1_"));
    }

    #[test]
    fn wallet_from_mnemonic_is_deterministic() {
        let wallet1 = generate_wallet().expect("wallet generation failed");
        let wallet2 = wallet_from_mnemonic(&wallet1.mnemonic).expect("wallet restoration failed");

        assert_eq!(wallet1.stake_address, wallet2.stake_address);
        assert_eq!(wallet1.payment_address, wallet2.payment_address);
        assert_eq!(
            wallet1.signing_key.to_bytes(),
            wallet2.signing_key.to_bytes()
        );
    }

    #[test]
    fn different_mnemonics_produce_different_wallets() {
        let wallet1 = generate_wallet().expect("wallet generation failed");
        let wallet2 = generate_wallet().expect("wallet generation failed");
        assert_ne!(wallet1.stake_address, wallet2.stake_address);
    }

    #[test]
    fn mnemonic_roundtrip() {
        let wallet = generate_wallet().expect("wallet generation failed");
        let phrase = wallet.mnemonic.clone();

        // Verify the mnemonic is valid BIP-39
        let parsed = Mnemonic::parse_in(Language::English, &phrase);
        assert!(parsed.is_ok());
    }
}
