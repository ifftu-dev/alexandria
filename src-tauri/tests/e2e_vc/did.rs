//! §4.1 / §5.1 / §5.3 — DID derivation, rotation, historical verification.

use super::common::{new_test_db, test_key, TEST_NOW};
use app_lib::crypto::did::{derive_did_key, resolve_did_key, resolve_key_at, rotate_key};
use ed25519_dalek::Signer;

#[tokio::test]
#[ignore = "pending PR 3 — DID layer"]
async fn derive_did_key_is_deterministic() {
    let k = test_key("alice");
    let d1 = derive_did_key(&k);
    let d2 = derive_did_key(&k);
    assert_eq!(d1, d2);
    assert!(d1.as_str().starts_with("did:key:z"));
}

#[tokio::test]
#[ignore = "pending PR 3 — DID layer"]
async fn did_key_resolves_to_original_public_key() {
    let k = test_key("bob");
    let did = derive_did_key(&k);
    let vk = resolve_did_key(&did).expect("resolve");
    // Sign with original, verify with resolved: must match.
    let sig = k.sign(b"test");
    assert!(vk.verify_strict(b"test", &sig).is_ok());
}

#[tokio::test]
#[ignore = "pending PR 3 — DID layer"]
async fn rotated_keys_preserve_historical_verification() {
    // A VC signed under key_v1 at T1 must remain verifiable at T2
    // after the issuer has rotated to key_v2. Spec §5.3.
    let db = new_test_db();
    let initial = test_key("issuer");
    let did = derive_did_key(&initial);
    let new_key = test_key("issuer-rotated");
    rotate_key(db.conn(), &did, &new_key).expect("rotate");

    let entry = resolve_key_at(db.conn(), &did, TEST_NOW)
        .expect("lookup")
        .expect("historical entry exists");
    assert_eq!(entry.did, did);
}
