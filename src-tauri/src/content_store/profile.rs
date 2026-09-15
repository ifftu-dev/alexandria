//! Profile publishing and resolution via iroh.
//!
//! Handles the full lifecycle of content-addressed user profiles:
//! 1. Build a ProfilePayload from the local identity
//! 2. Sign it with the wallet's Ed25519 key
//! 3. Store the signed JSON as an iroh blob
//! 4. Resolve (fetch + verify) profiles by BLAKE3 hash

use alexandria_verify::json::{decode_untrusted, JsonLimits, UntrustedJsonError};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use thiserror::Error;

use crate::content_store::content;
use crate::content_store::node::ContentNode;
use crate::domain::profile::{ProfilePayload, PublishProfileResult, SignedProfile};

#[derive(Error, Debug)]
pub enum ProfileError {
    #[error("serialization failed: {0}")]
    Serialization(String),
    #[error("signing failed: {0}")]
    Signing(String),
    #[error("content store error: {0}")]
    Store(String),
    #[error("profile not found: {0}")]
    NotFound(String),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("deserialization failed: {0}")]
    Deserialization(String),
    #[error("profile document JSON rejected: {0}")]
    UntrustedJson(UntrustedJsonError),
}

/// Structural limits for an untrusted signed profile document, checked before
/// typed decoding and signature verification. A profile is one flat object of
/// short fields; publication applies the same limits, so an owner cannot
/// publish a profile peers would refuse.
pub const PROFILE_DOCUMENT_JSON_LIMITS: JsonLimits = JsonLimits {
    max_bytes: 64 * 1024,
    max_depth: 2,
    max_array_len: 16,
    max_object_entries: 16,
    max_string_bytes: 16 * 1024,
};

/// Decode an untrusted signed profile document under
/// [`PROFILE_DOCUMENT_JSON_LIMITS`]. The result is not yet verified.
pub fn decode_profile_document(bytes: &[u8]) -> Result<SignedProfile, ProfileError> {
    decode_untrusted(bytes, &PROFILE_DOCUMENT_JSON_LIMITS).map_err(|error| match error {
        UntrustedJsonError::Invalid(message) => ProfileError::Deserialization(message),
        rejected => ProfileError::UntrustedJson(rejected),
    })
}

/// Sign a profile payload with the given Ed25519 signing key.
///
/// The signature covers the canonical JSON serialization of the payload.
pub fn sign_profile(
    payload: &ProfilePayload,
    key: &SigningKey,
) -> Result<SignedProfile, ProfileError> {
    // Canonical JSON serialization (serde_json with sorted keys would be
    // ideal, but the struct field order is deterministic, which is sufficient
    // for our use case since we always serialize the same struct).
    let payload_json =
        serde_json::to_vec(payload).map_err(|e| ProfileError::Serialization(e.to_string()))?;

    let signature = key.sign(&payload_json);
    let public_key = key.verifying_key();

    Ok(SignedProfile {
        version: payload.version,
        stake_address: payload.stake_address.clone(),
        name: payload.name.clone(),
        bio: payload.bio.clone(),
        avatar_hash: payload.avatar_hash.clone(),
        created_at: payload.created_at,
        updated_at: payload.updated_at,
        signature: hex::encode(signature.to_bytes()),
        public_key: hex::encode(public_key.to_bytes()),
    })
}

/// Verify a signed profile document.
///
/// Checks that the Ed25519 signature over the payload is valid for
/// the included public key.
pub fn verify_profile(signed: &SignedProfile) -> Result<(), ProfileError> {
    let payload = signed.payload();
    let payload_json =
        serde_json::to_vec(&payload).map_err(|e| ProfileError::Serialization(e.to_string()))?;

    let sig_bytes: [u8; 64] = hex::decode(&signed.signature)
        .map_err(|e| ProfileError::InvalidPublicKey(format!("bad signature hex: {e}")))?
        .try_into()
        .map_err(|_| ProfileError::InvalidSignature)?;

    let pub_bytes: [u8; 32] = hex::decode(&signed.public_key)
        .map_err(|e| ProfileError::InvalidPublicKey(format!("bad public key hex: {e}")))?
        .try_into()
        .map_err(|_| ProfileError::InvalidPublicKey("wrong length".into()))?;

    let verifying_key = VerifyingKey::from_bytes(&pub_bytes)
        .map_err(|e| ProfileError::InvalidPublicKey(e.to_string()))?;

    let signature = Signature::from_bytes(&sig_bytes);

    verifying_key
        .verify(&payload_json, &signature)
        .map_err(|_| ProfileError::InvalidSignature)
}

/// Publish a signed profile to the iroh content store.
///
/// Serializes the signed profile as JSON, stores it as a blob, and
/// returns the BLAKE3 hash.
pub async fn publish_profile(
    node: &ContentNode,
    signed: &SignedProfile,
) -> Result<PublishProfileResult, ProfileError> {
    let json = serde_json::to_vec_pretty(signed)
        .map_err(|e| ProfileError::Serialization(e.to_string()))?;
    decode_profile_document(&json)?;

    let result = content::add_bytes(node, &json)
        .await
        .map_err(|e| ProfileError::Store(e.to_string()))?;

    Ok(PublishProfileResult {
        profile_hash: result.hash,
        profile: signed.clone(),
    })
}

/// Resolve a profile from the iroh content store by BLAKE3 hash.
///
/// Fetches the blob, deserializes it as a SignedProfile, and verifies
/// the Ed25519 signature. Returns the verified profile.
pub async fn resolve_profile(
    node: &ContentNode,
    hash: &str,
) -> Result<SignedProfile, ProfileError> {
    let bytes = content::get_bytes(node, hash)
        .await
        .map_err(|e| ProfileError::NotFound(e.to_string()))?;

    let signed = decode_profile_document(&bytes)?;

    // Verify the signature before returning
    verify_profile(&signed)?;

    Ok(signed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_key() -> SigningKey {
        {
            let mut __s = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut __s);
            SigningKey::from_bytes(&__s)
        }
    }

    fn make_test_payload(stake_address: &str) -> ProfilePayload {
        ProfilePayload {
            version: 1,
            stake_address: stake_address.to_string(),
            name: Some("Alice".to_string()),
            bio: Some("Studying computer science".to_string()),
            avatar_hash: None,
            created_at: 1740000000,
            updated_at: 1740100000,
        }
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let key = make_test_key();
        let payload = make_test_payload("stake_test1abc");

        let signed = sign_profile(&payload, &key).expect("sign failed");
        assert!(verify_profile(&signed).is_ok());
    }

    #[test]
    fn verify_rejects_tampered_name() {
        let key = make_test_key();
        let payload = make_test_payload("stake_test1abc");

        let mut signed = sign_profile(&payload, &key).expect("sign failed");
        signed.name = Some("Eve".to_string()); // Tamper

        assert!(matches!(
            verify_profile(&signed),
            Err(ProfileError::InvalidSignature)
        ));
    }

    #[test]
    fn verify_rejects_tampered_stake_address() {
        let key = make_test_key();
        let payload = make_test_payload("stake_test1abc");

        let mut signed = sign_profile(&payload, &key).expect("sign failed");
        signed.stake_address = "stake_test1evil".to_string();

        assert!(matches!(
            verify_profile(&signed),
            Err(ProfileError::InvalidSignature)
        ));
    }

    #[test]
    fn verify_rejects_wrong_public_key() {
        let key1 = make_test_key();
        let key2 = make_test_key();
        let payload = make_test_payload("stake_test1abc");

        let mut signed = sign_profile(&payload, &key1).expect("sign failed");
        // Replace public key with a different one
        signed.public_key = hex::encode(key2.verifying_key().to_bytes());

        assert!(matches!(
            verify_profile(&signed),
            Err(ProfileError::InvalidSignature)
        ));
    }

    #[test]
    fn signed_profile_serializes_to_json() {
        let key = make_test_key();
        let payload = make_test_payload("stake_test1xyz");

        let signed = sign_profile(&payload, &key).expect("sign failed");
        let json = serde_json::to_string_pretty(&signed).expect("serialize");

        // Verify it contains expected fields
        assert!(json.contains("\"version\": 1"));
        assert!(json.contains("\"stake_address\": \"stake_test1xyz\""));
        assert!(json.contains("\"name\": \"Alice\""));
        assert!(json.contains("\"signature\""));
        assert!(json.contains("\"public_key\""));

        // Verify it round-trips
        let deserialized: SignedProfile = serde_json::from_str(&json).expect("deserialize");
        assert!(verify_profile(&deserialized).is_ok());
    }

    #[test]
    fn payload_extraction_matches_original() {
        let key = make_test_key();
        let payload = make_test_payload("stake_test1abc");

        let signed = sign_profile(&payload, &key).expect("sign failed");
        let extracted = signed.payload();

        assert_eq!(payload, extracted);
    }

    #[test]
    fn profile_documents_are_bounded_before_verification() {
        let signed =
            sign_profile(&make_test_payload("stake_test1limits"), &make_test_key()).expect("sign");
        let bytes = serde_json::to_vec(&signed).expect("serialize");
        let max = PROFILE_DOCUMENT_JSON_LIMITS.max_bytes;

        let mut exact = bytes.clone();
        exact.resize(max, b' ');
        verify_profile(&decode_profile_document(&exact).expect("exact boundary decodes"))
            .expect("exact boundary verifies");
        exact.push(b' ');
        assert!(matches!(
            decode_profile_document(&exact),
            Err(ProfileError::UntrustedJson(UntrustedJsonError::TooLarge { max: limit }))
                if limit == max
        ));

        let rest = std::str::from_utf8(&bytes)
            .expect("utf-8")
            .strip_prefix('{')
            .expect("object");
        let duplicated = format!("{{\"name\":\"Mallory\",{rest}");
        assert!(matches!(
            decode_profile_document(duplicated.as_bytes()),
            Err(ProfileError::UntrustedJson(UntrustedJsonError::DuplicateKey(field)))
                if field == "name"
        ));
    }

    #[tokio::test]
    async fn a_profile_over_the_limits_is_not_published() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let node = ContentNode::new(tmp.path());
        node.start(None).await.expect("start node");

        let mut payload = make_test_payload("stake_test1oversized");
        payload.bio = Some("a".repeat(PROFILE_DOCUMENT_JSON_LIMITS.max_string_bytes + 1));
        let signed = sign_profile(&payload, &make_test_key()).expect("sign");
        assert!(matches!(
            publish_profile(&node, &signed).await,
            Err(ProfileError::UntrustedJson(
                UntrustedJsonError::StringTooLong { .. }
            ))
        ));

        node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn publish_and_resolve_roundtrip() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let node = ContentNode::new(tmp.path());
        node.start(None).await.expect("start node");

        let key = make_test_key();
        let payload = make_test_payload("stake_test1roundtrip");

        let signed = sign_profile(&payload, &key).expect("sign");
        let result = publish_profile(&node, &signed).await.expect("publish");

        assert!(!result.profile_hash.is_empty());
        assert_eq!(result.profile_hash.len(), 64); // BLAKE3 hex

        // Resolve it back
        let resolved = resolve_profile(&node, &result.profile_hash)
            .await
            .expect("resolve");

        assert_eq!(resolved.stake_address, "stake_test1roundtrip");
        assert_eq!(resolved.name, Some("Alice".to_string()));
        assert_eq!(resolved.public_key, signed.public_key);

        node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn resolve_rejects_invalid_hash() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let node = ContentNode::new(tmp.path());
        node.start(None).await.expect("start node");

        let result = resolve_profile(&node, &"0".repeat(64)).await;
        assert!(matches!(result, Err(ProfileError::NotFound(_))));

        node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn publish_same_profile_twice_gives_same_hash() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let node = ContentNode::new(tmp.path());
        node.start(None).await.expect("start node");

        let key = make_test_key();
        let payload = make_test_payload("stake_test1deterministic");

        let signed = sign_profile(&payload, &key).expect("sign");
        let r1 = publish_profile(&node, &signed).await.expect("publish 1");
        let r2 = publish_profile(&node, &signed).await.expect("publish 2");

        assert_eq!(r1.profile_hash, r2.profile_hash);

        node.shutdown().await.expect("shutdown");
    }
}
