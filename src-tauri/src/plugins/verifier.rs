//! Ed25519 signature verification + content-addressed plugin IDs.
//!
//! The signature is over the raw manifest bytes (not a canonical form —
//! the on-disk file *is* the canonical form). The plugin's identity is
//! `BLAKE3(manifest_bytes)` hex-encoded, making it a stable
//! content-addressed handle a course can pin forever.

use ed25519_dalek::Signature;

use crate::crypto::did::{parse_did_key, resolve_did_key};

/// Expected length of a raw Ed25519 signature.
pub const SIGNATURE_LEN: usize = 64;

/// Compute the content-addressed plugin CID from the raw manifest bytes.
///
/// The resulting hex-encoded BLAKE3 hash is what the `plugin_installed.plugin_cid`
/// column stores and what courses reference.
pub fn compute_plugin_cid(manifest_bytes: &[u8]) -> String {
    blake3::hash(manifest_bytes).to_hex().to_string()
}

/// Verify a detached Ed25519 signature on the manifest.
///
/// `sig_bytes` is the raw 64-byte signature (the on-disk `manifest.sig`).
/// The verifying key is resolved from the author's `did:key:z...`. The DID
/// is *supposed* to match `manifest.author_did` — the caller is responsible
/// for reading it from the manifest and passing it in so this function stays
/// narrow and testable without a full `PluginManifest` import.
pub fn verify_manifest_signature(
    manifest_bytes: &[u8],
    sig_bytes: &[u8],
    author_did: &str,
) -> Result<(), String> {
    if sig_bytes.len() != SIGNATURE_LEN {
        return Err(format!(
            "manifest signature must be {SIGNATURE_LEN} bytes, got {}",
            sig_bytes.len()
        ));
    }

    let sig_array: [u8; SIGNATURE_LEN] = sig_bytes
        .try_into()
        .map_err(|_| "manifest signature length check passed but conversion failed".to_string())?;
    let signature = Signature::from_bytes(&sig_array);

    let did = parse_did_key(author_did).map_err(|e| format!("invalid author DID: {e}"))?;
    let vkey = resolve_did_key(&did).map_err(|e| format!("cannot resolve author DID: {e}"))?;

    vkey.verify_strict(manifest_bytes, &signature)
        .map_err(|e| format!("manifest signature verification failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::did_from_verifying_key;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    #[test]
    fn cid_is_stable() {
        let a = compute_plugin_cid(b"hello");
        let b = compute_plugin_cid(b"hello");
        assert_eq!(a, b);
        assert_ne!(a, compute_plugin_cid(b"hello!"));
        assert_eq!(a.len(), 64); // blake3 hex
    }

    #[test]
    fn roundtrip_sign_verify() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let did = did_from_verifying_key(&vk);

        let manifest = br#"{"name":"test","author_did":"..."}"#;
        let sig = sk.sign(manifest);
        let sig_bytes = sig.to_bytes();

        verify_manifest_signature(manifest, &sig_bytes, did.as_str()).expect("should verify");
    }

    #[test]
    fn rejects_tampered_manifest() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let did = did_from_verifying_key(&vk);

        let manifest = br#"{"name":"test"}"#;
        let tampered = br#"{"name":"evil"}"#;
        let sig = sk.sign(manifest);

        assert!(verify_manifest_signature(tampered, &sig.to_bytes(), did.as_str()).is_err());
    }

    #[test]
    fn rejects_wrong_signer() {
        let author_sk = SigningKey::generate(&mut OsRng);
        let attacker_sk = SigningKey::generate(&mut OsRng);
        let author_did = did_from_verifying_key(&author_sk.verifying_key());

        let manifest = br#"{"name":"test"}"#;
        let attacker_sig = attacker_sk.sign(manifest);

        assert!(
            verify_manifest_signature(manifest, &attacker_sig.to_bytes(), author_did.as_str(),)
                .is_err()
        );
    }

    #[test]
    fn rejects_wrong_signature_length() {
        let did = "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK";
        assert!(verify_manifest_signature(b"x", &[0u8; 32], did).is_err());
        assert!(verify_manifest_signature(b"x", &[0u8; 128], did).is_err());
    }
}
