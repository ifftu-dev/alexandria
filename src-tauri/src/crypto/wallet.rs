use bip39::{Language, Mnemonic};
use ed25519_dalek::SigningKey;
use pallas_addresses::{
    Address, Network, ShelleyAddress, ShelleyDelegationPart, ShelleyPaymentPart, StakeAddress,
};
use pallas_crypto::hash::Hasher;
use pallas_crypto::key::ed25519::SecretKeyExtended;
use pallas_wallet::hd::Bip32PrivateKey;
use thiserror::Error;
use zeroize::Zeroize;

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("invalid mnemonic phrase: {0}")]
    InvalidMnemonic(String),
    #[error("key derivation failed: {0}")]
    DerivationFailed(String),
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
///
/// Uses pallas for proper Icarus-style BIP32-Ed25519 derivation and
/// bech32 address encoding (addr_test1... / stake_test1... for preprod).
/// Note: `Clone` intentionally not derived — prevents accidental duplication
/// of secret key material in memory. `Drop` zeros all sensitive fields.
#[derive(Debug)]
pub struct Wallet {
    /// The BIP-39 mnemonic phrase (24 words).
    pub mnemonic: String,
    /// Ed25519 signing key derived at m/1852'/1815'/0'/0/0.
    pub signing_key: SigningKey,
    /// Raw BIP32-Ed25519 extended payment key (64 bytes: 32-byte scalar + 32-byte extension).
    /// Needed by pallas-txbuilder for Cardano transaction signing (uses `PrivateKey::Extended`).
    pub payment_key_extended: [u8; 64],
    /// The Cardano stake address (bech32, starts with "stake_test1..." on preprod).
    /// Derived from the stake key at m/1852'/1815'/0'/2/0.
    pub stake_address: String,
    /// The Cardano payment address (bech32, starts with "addr_test1..." on preprod).
    pub payment_address: String,
    /// Blake2b-224 hash of the payment public key (28 bytes).
    /// Used for NativeScript policy creation and disclosed signers.
    pub payment_key_hash: [u8; 28],
}

impl Drop for Wallet {
    fn drop(&mut self) {
        self.mnemonic.zeroize();
        self.signing_key.zeroize();
        self.payment_key_extended.zeroize();
        self.payment_key_hash.zeroize();
    }
}

/// The Cardano network to use for address generation.
/// Preprod testnet for development, mainnet for production.
const CARDANO_NETWORK: Network = Network::Testnet;

/// CIP-1852 derivation path constants.
const PURPOSE: u32 = 1852;
const COIN_TYPE: u32 = 1815;
const ACCOUNT: u32 = 0;
const PAYMENT_ROLE: u32 = 0;
const STAKE_ROLE: u32 = 2;
const ADDRESS_INDEX: u32 = 0;

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

    let mnemonic_str = mnemonic.to_string();

    // Derive master key using Icarus-style derivation (PBKDF2-HMAC-SHA512).
    // pallas-wallet handles this correctly via from_bip39_mnenomic.
    let master = Bip32PrivateKey::from_bip39_mnenomic(mnemonic_str.clone(), String::new())
        .map_err(|e| WalletError::DerivationFailed(format!("master key derivation: {e}")))?;

    // CIP-1852 account key: m/1852'/1815'/0'
    let account = master
        .derive(harden(PURPOSE))
        .derive(harden(COIN_TYPE))
        .derive(harden(ACCOUNT));

    // Payment key: m/1852'/1815'/0'/0/0
    let payment_bip32 = account.derive(PAYMENT_ROLE).derive(ADDRESS_INDEX);

    // Stake key: m/1852'/1815'/0'/2/0
    let stake_bip32 = account.derive(STAKE_ROLE).derive(ADDRESS_INDEX);

    // Extract Ed25519 public keys and compute Blake2b-224 key hashes
    let payment_pub = payment_bip32.to_public().to_ed25519_pubkey();
    let stake_pub = stake_bip32.to_public().to_ed25519_pubkey();

    let payment_key_hash = Hasher::<224>::hash(payment_pub.as_ref());
    let stake_key_hash = Hasher::<224>::hash(stake_pub.as_ref());

    // Construct Shelley-era base address (payment + stake delegation)
    let shelley_addr = ShelleyAddress::new(
        CARDANO_NETWORK,
        ShelleyPaymentPart::key_hash(payment_key_hash),
        ShelleyDelegationPart::key_hash(stake_key_hash),
    );
    let payment_address = Address::from(shelley_addr.clone())
        .to_bech32()
        .map_err(|e| WalletError::DerivationFailed(format!("payment address bech32: {e}")))?;

    // Construct stake address via TryFrom<ShelleyAddress>
    let stake_addr: StakeAddress = shelley_addr
        .try_into()
        .map_err(|e| WalletError::DerivationFailed(format!("stake address conversion: {e}")))?;
    let stake_address = stake_addr
        .to_bech32()
        .map_err(|e| WalletError::DerivationFailed(format!("stake address bech32: {e}")))?;

    // Extract the raw extended key bytes (64 bytes) for pallas-txbuilder signing.
    let payment_key_extended = extract_extended_key_bytes(&payment_bip32)?;

    // Extract Ed25519 signing key (first 32 bytes of extended key) for
    // ed25519-dalek compatibility (used in our signing module).
    let signing_key = SigningKey::from_bytes(
        &payment_key_extended[..32]
            .try_into()
            .map_err(|_| WalletError::DerivationFailed("signing key slice failed".into()))?,
    );

    // Store the payment key hash (28 bytes) for policy/signer use.
    let payment_key_hash_pallas = payment_key_hash;
    let mut pkh_bytes = [0u8; 28];
    pkh_bytes.copy_from_slice(payment_key_hash_pallas.as_ref());

    Ok(Wallet {
        mnemonic: mnemonic_str,
        signing_key,
        payment_key_extended,
        stake_address,
        payment_address,
        payment_key_hash: pkh_bytes,
    })
}

/// Apply hardened derivation offset to an index.
fn harden(index: u32) -> u32 {
    0x8000_0000 + index
}

/// Extract the raw 64-byte extended key from a pallas Bip32PrivateKey.
///
/// pallas stores an extended BIP32-Ed25519 key (64 bytes: 32-byte scalar + 32-byte extension).
/// Both halves are needed for pallas-txbuilder's `PrivateKey::Extended` signing.
/// The first 32 bytes alone serve as the Ed25519 scalar for ed25519-dalek.
fn extract_extended_key_bytes(bip32_key: &Bip32PrivateKey) -> Result<[u8; 64], WalletError> {
    let pallas_private = bip32_key.to_ed25519_private_key();

    match pallas_private {
        pallas_wallet::PrivateKey::Normal(sk) => {
            // Normal key is only 32 bytes — pad with zeros for the extension half.
            let bytes: [u8; 32] =
                unsafe { pallas_crypto::key::ed25519::SecretKey::leak_into_bytes(sk) };
            let mut extended = [0u8; 64];
            extended[..32].copy_from_slice(&bytes);
            Ok(extended)
        }
        pallas_wallet::PrivateKey::Extended(xsk) => {
            let bytes: [u8; SecretKeyExtended::SIZE] =
                unsafe { SecretKeyExtended::leak_into_bytes(xsk) };
            Ok(bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_wallet_produces_valid_wallet() {
        let wallet = generate_wallet().expect("wallet generation failed");
        assert_eq!(wallet.mnemonic.split_whitespace().count(), 24);
        // Preprod testnet addresses
        assert!(
            wallet.stake_address.starts_with("stake_test1"),
            "stake address should start with stake_test1, got: {}",
            wallet.stake_address
        );
        assert!(
            wallet.payment_address.starts_with("addr_test1"),
            "payment address should start with addr_test1, got: {}",
            wallet.payment_address
        );
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
        assert_ne!(wallet1.payment_address, wallet2.payment_address);
    }

    #[test]
    fn mnemonic_roundtrip() {
        let wallet = generate_wallet().expect("wallet generation failed");
        let phrase = wallet.mnemonic.clone();

        // Verify the mnemonic is valid BIP-39
        let parsed = Mnemonic::parse_in(Language::English, &phrase);
        assert!(parsed.is_ok());
    }

    #[test]
    fn payment_address_is_valid_bech32() {
        let wallet = generate_wallet().expect("wallet generation failed");
        // addr_test1 prefix = Shelley testnet base address (type 0x00)
        assert!(wallet.payment_address.starts_with("addr_test1"));
        // Base addresses are typically 98+ chars in bech32
        assert!(
            wallet.payment_address.len() > 50,
            "payment address too short: {}",
            wallet.payment_address
        );
    }

    #[test]
    fn stake_address_is_valid_bech32() {
        let wallet = generate_wallet().expect("wallet generation failed");
        // stake_test1 prefix = Shelley testnet stake/reward address
        assert!(wallet.stake_address.starts_with("stake_test1"));
        assert!(
            wallet.stake_address.len() > 40,
            "stake address too short: {}",
            wallet.stake_address
        );
    }

    #[test]
    fn signing_key_is_32_bytes() {
        let wallet = generate_wallet().expect("wallet generation failed");
        assert_eq!(wallet.signing_key.to_bytes().len(), 32);
    }

    #[test]
    fn signing_key_can_sign_and_verify() {
        use ed25519_dalek::Signer;
        let wallet = generate_wallet().expect("wallet generation failed");
        let message = b"hello cardano";
        let signature = wallet.signing_key.sign(message);

        let verifying_key = wallet.signing_key.verifying_key();
        use ed25519_dalek::Verifier;
        assert!(verifying_key.verify(message, &signature).is_ok());
    }

    #[test]
    fn payment_address_roundtrips_through_bech32_parsing() {
        let wallet = generate_wallet().expect("wallet generation failed");
        // Parse the bech32 address back and verify it's a valid Shelley address
        let parsed = Address::from_bech32(&wallet.payment_address)
            .expect("payment address should parse as bech32");
        assert!(
            matches!(parsed, Address::Shelley(_)),
            "payment address should be Shelley type"
        );
        // Re-encode and compare
        let re_encoded = parsed.to_bech32().expect("re-encoding should succeed");
        assert_eq!(wallet.payment_address, re_encoded);
    }

    #[test]
    fn stake_address_roundtrips_through_bech32_parsing() {
        let wallet = generate_wallet().expect("wallet generation failed");
        // Parse the bech32 address back and verify it's a valid Stake address
        let parsed = Address::from_bech32(&wallet.stake_address)
            .expect("stake address should parse as bech32");
        assert!(
            matches!(parsed, Address::Stake(_)),
            "stake address should be Stake type"
        );
        // Re-encode and compare
        let re_encoded = parsed.to_bech32().expect("re-encoding should succeed");
        assert_eq!(wallet.stake_address, re_encoded);
    }
}
