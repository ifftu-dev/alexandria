//! The identity that signs credentials: a signing key and the `did:key`
//! derived from it, carried as one value so a credential can never be
//! signed with one key and stamped with another DID.

use ed25519_dalek::SigningKey;

use super::CredentialError;
use crate::crypto::did::{derive_did_key, Did};
use crate::crypto::keystore::Keystore;
use crate::crypto::wallet::{self, Wallet};

/// A signing key and its DID. Only ever built from a key, the DID derived
/// from it, so the two always belong together: a `did:key` is its key.
#[derive(Clone)]
pub struct Issuer {
    key: SigningKey,
    did: Did,
}

impl Issuer {
    /// The issuer behind `key`; its DID is derived from the key.
    pub fn from_key(key: SigningKey) -> Self {
        let did = derive_did_key(&key);
        Self { key, did }
    }

    /// The issuer behind an unlocked wallet.
    pub fn from_wallet(wallet: &Wallet) -> Self {
        // `Wallet` implements `Drop` (zeroize) so we can't move out — clone
        // the signing key bytes instead.
        Self::from_key(SigningKey::from_bytes(&wallet.signing_key.to_bytes()))
    }

    /// The profile's issuer, read from its keystore. Refused while the
    /// vault is locked. The keystore lock is released before the wallet is
    /// derived.
    pub async fn from_keystore(
        keystore: &tokio::sync::Mutex<Option<Keystore>>,
    ) -> Result<Self, CredentialError> {
        let ks_guard = keystore.lock().await;
        let ks = ks_guard.as_ref().ok_or(CredentialError::VaultLocked)?;
        let mnemonic = ks
            .retrieve_mnemonic()
            .map_err(|e| CredentialError::Vault(e.to_string()))?;
        drop(ks_guard);
        let w = wallet::wallet_from_mnemonic(&mnemonic)
            .map_err(|e| CredentialError::Vault(e.to_string()))?;
        Ok(Self::from_wallet(&w))
    }

    pub fn key(&self) -> &SigningKey {
        &self.key
    }

    pub fn did(&self) -> &Did {
        &self.did
    }

    /// The key and DID apart, for signing something other than a
    /// credential (a directory proof, an organisation response).
    pub fn into_parts(self) -> (SigningKey, Did) {
        (self.key, self.did)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::fixtures::test_key;
    use crate::credentials::CredentialError;

    #[tokio::test]
    async fn a_locked_vault_has_no_issuer() {
        let locked = tokio::sync::Mutex::new(None);
        let error = Issuer::from_keystore(&locked).await.err().unwrap();
        assert!(matches!(error, CredentialError::VaultLocked), "{error:?}");
        assert_eq!(error.to_string(), "vault is locked — unlock first");
    }

    #[test]
    fn the_did_is_the_keys_own() {
        let key = test_key("issuer");
        let issuer = Issuer::from_key(key.clone());
        assert_eq!(issuer.did(), &derive_did_key(&key));
        assert_eq!(issuer.key().to_bytes(), key.to_bytes());
    }
}
