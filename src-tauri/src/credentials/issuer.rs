//! The identity that signs credentials: a signing key and the `did:key`
//! derived from it, carried as one value so a credential can never be
//! signed with one key and stamped with another DID.

use alexandria_verify::did::resolve_did_key;
use ed25519_dalek::SigningKey;
use rusqlite::Connection;

use crate::crypto::did::{derive_did_key, Did};
use crate::crypto::key_registry;
use crate::crypto::keystore::Keystore;
use crate::crypto::wallet::{self, Wallet};

/// A signing key and its DID. Built from a key (the DID is derived from
/// it), or for a rotated DID from a key the registry holds for it, so the
/// two always belong together.
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
    ) -> Result<Self, String> {
        let ks_guard = keystore.lock().await;
        let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
        let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
        drop(ks_guard);
        let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
        Ok(Self::from_wallet(&w))
    }

    /// The issuer for `did` after it rotated to `key`: the DID keeps its
    /// identity, so it is no longer derived from the key. Refused unless
    /// `key` is the DID's key at `at` (the issuance time): the key registry's
    /// window open then. Only a DID the registry has never seen falls back to
    /// the key its `did:key` embeds; one with history but no open window has
    /// no current key, so its retired original key is refused too.
    pub fn rotated(conn: &Connection, did: Did, key: SigningKey, at: &str) -> Result<Self, String> {
        let open = key_registry::resolve_key_at(conn, &did, at).map_err(|e| e.to_string())?;
        let current = match open {
            Some(entry) => Some(entry.public_key_bytes),
            None if has_history(conn, &did)? => None,
            None => Some(
                resolve_did_key(&did)
                    .map_err(|e| e.to_string())?
                    .as_bytes()
                    .to_vec(),
            ),
        };
        if current.as_deref() != Some(key.verifying_key().as_bytes().as_slice()) {
            return Err(format!(
                "the signing key is not this DID's current key: {}",
                did.as_str()
            ));
        }
        Ok(Self { key, did })
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

/// Whether the key registry holds any window, open or not, for `did`.
fn has_history(conn: &Connection, did: &Did) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM key_registry WHERE did = ?1)",
        [did.as_str()],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::fixtures::{test_key, NOW};

    #[tokio::test]
    async fn a_locked_vault_has_no_issuer() {
        let locked = tokio::sync::Mutex::new(None);
        let error = Issuer::from_keystore(&locked).await.err().unwrap();
        assert!(error.contains("vault is locked"), "{error}");
    }

    #[test]
    fn the_did_is_the_keys_own() {
        let key = test_key("issuer");
        let issuer = Issuer::from_key(key.clone());
        assert_eq!(issuer.did(), &derive_did_key(&key));
        assert_eq!(issuer.key().to_bytes(), key.to_bytes());
    }

    #[test]
    fn a_rotated_key_signs_for_its_did_only_once_the_registry_holds_it() {
        let (db, original, did, _) = crate::credentials::fixtures::setup();
        let rotated = test_key("issuer-rotated");
        // A rotation takes effect at the wall clock; issue after it.
        let now = crate::clock::now_rfc3339();
        let error = Issuer::rotated(db.conn(), did.clone(), rotated.clone(), &now)
            .err()
            .unwrap();
        assert!(error.contains("not this DID's current key"), "{error}");

        crate::crypto::key_registry::rotate_key(db.conn(), &did, &rotated).unwrap();
        let issuer = Issuer::rotated(db.conn(), did.clone(), rotated.clone(), &now).unwrap();
        assert_eq!(issuer.did(), &did);
        assert_eq!(issuer.key().to_bytes(), rotated.to_bytes());
        // The pre-rotation key is no longer the DID's current key.
        assert!(Issuer::rotated(db.conn(), did, original.key().clone(), &now).is_err());
    }

    #[test]
    fn a_retired_original_key_is_refused_when_no_window_is_open() {
        let (db, original, did, _) = crate::credentials::fixtures::setup();
        // The DID has history: its original key, retired, and no key open now.
        db.conn()
            .execute(
                "INSERT INTO key_registry \
                 (did, key_id, public_key_hex, valid_from, valid_until, rotated_by) \
                 VALUES (?1, 'key-1', ?2, '1970-01-01T00:00:00Z', '2000-01-01T00:00:00Z', NULL)",
                rusqlite::params![
                    did.as_str(),
                    hex::encode(original.key().verifying_key().as_bytes())
                ],
            )
            .unwrap();
        let error = Issuer::rotated(db.conn(), did, original.key().clone(), NOW)
            .err()
            .unwrap();
        assert!(error.contains("not this DID's current key"), "{error}");
    }
}
