//! Course document publishing and resolution via iroh.
//!
//! Handles the full lifecycle of IPFS-backed course documents:
//! 1. Build a CourseDocumentPayload from local SQLite data
//! 2. Sign it with the author's Ed25519 key
//! 3. Store the signed JSON as an iroh blob
//! 4. Resolve (fetch + verify) course documents by BLAKE3 hash

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use thiserror::Error;

use crate::domain::course_document::{
    CourseDocumentPayload, PublishCourseResult, SignedCourseDocument,
};
use crate::ipfs::content;
use crate::ipfs::node::ContentNode;

#[derive(Error, Debug)]
pub enum CourseDocError {
    #[error("serialization failed: {0}")]
    Serialization(String),
    #[error("signing failed: {0}")]
    Signing(String),
    #[error("content store error: {0}")]
    Store(String),
    #[error("document not found: {0}")]
    NotFound(String),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("deserialization failed: {0}")]
    Deserialization(String),
}

/// Sign a course document payload with the given Ed25519 signing key.
///
/// The signature covers the canonical JSON serialization of the payload.
pub fn sign_course_document(
    payload: &CourseDocumentPayload,
    key: &SigningKey,
) -> Result<SignedCourseDocument, CourseDocError> {
    let payload_json = serde_json::to_vec(payload)
        .map_err(|e| CourseDocError::Serialization(e.to_string()))?;

    let signature = key.sign(&payload_json);
    let public_key = key.verifying_key();

    Ok(SignedCourseDocument {
        version: payload.version,
        course_id: payload.course_id.clone(),
        author_address: payload.author_address.clone(),
        title: payload.title.clone(),
        description: payload.description.clone(),
        thumbnail_hash: payload.thumbnail_hash.clone(),
        tags: payload.tags.clone(),
        skill_ids: payload.skill_ids.clone(),
        chapters: payload.chapters.clone(),
        created_at: payload.created_at,
        updated_at: payload.updated_at,
        signature: hex::encode(signature.to_bytes()),
        public_key: hex::encode(public_key.to_bytes()),
    })
}

/// Verify a signed course document.
///
/// Checks that the Ed25519 signature over the payload is valid for
/// the included public key.
pub fn verify_course_document(signed: &SignedCourseDocument) -> Result<(), CourseDocError> {
    let payload = signed.payload();
    let payload_json = serde_json::to_vec(&payload)
        .map_err(|e| CourseDocError::Serialization(e.to_string()))?;

    let sig_bytes: [u8; 64] = hex::decode(&signed.signature)
        .map_err(|e| CourseDocError::InvalidPublicKey(format!("bad signature hex: {e}")))?
        .try_into()
        .map_err(|_| CourseDocError::InvalidSignature)?;

    let pub_bytes: [u8; 32] = hex::decode(&signed.public_key)
        .map_err(|e| CourseDocError::InvalidPublicKey(format!("bad public key hex: {e}")))?
        .try_into()
        .map_err(|_| CourseDocError::InvalidPublicKey("wrong length".into()))?;

    let verifying_key = VerifyingKey::from_bytes(&pub_bytes)
        .map_err(|e| CourseDocError::InvalidPublicKey(e.to_string()))?;

    let signature = Signature::from_bytes(&sig_bytes);
    verifying_key
        .verify(&payload_json, &signature)
        .map_err(|_| CourseDocError::InvalidSignature)
}

/// Publish a signed course document to the iroh blob store.
///
/// Serializes the signed document to JSON and stores it. Returns the
/// BLAKE3 hash for future retrieval.
pub async fn publish_course_document(
    node: &ContentNode,
    signed: &SignedCourseDocument,
) -> Result<PublishCourseResult, CourseDocError> {
    let doc_json = serde_json::to_vec(signed)
        .map_err(|e| CourseDocError::Serialization(e.to_string()))?;

    let result = content::add_bytes(node, &doc_json)
        .await
        .map_err(|e| CourseDocError::Store(e.to_string()))?;

    log::info!(
        "published course document '{}' ({} bytes, hash: {})",
        signed.title,
        result.size,
        result.hash
    );

    Ok(PublishCourseResult {
        content_hash: result.hash,
        size: result.size,
    })
}

/// Resolve a course document from the iroh blob store by BLAKE3 hash.
///
/// Fetches the blob, deserializes the signed document, verifies the
/// signature, and returns the verified document.
pub async fn resolve_course_document(
    node: &ContentNode,
    hash_hex: &str,
) -> Result<SignedCourseDocument, CourseDocError> {
    let bytes = content::get_bytes(node, hash_hex)
        .await
        .map_err(|e| match e {
            content::ContentError::NotFound(_) => CourseDocError::NotFound(hash_hex.to_string()),
            other => CourseDocError::Store(other.to_string()),
        })?;

    let signed: SignedCourseDocument = serde_json::from_slice(&bytes)
        .map_err(|e| CourseDocError::Deserialization(e.to_string()))?;

    verify_course_document(&signed)?;

    log::info!(
        "resolved and verified course document '{}' (hash: {})",
        signed.title,
        hash_hex
    );

    Ok(signed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::course_document::{DocumentChapter, DocumentElement};
    use ed25519_dalek::SigningKey;
    use tempfile::TempDir;

    fn make_signing_key() -> SigningKey {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        SigningKey::from_bytes(&bytes)
    }

    fn make_payload() -> CourseDocumentPayload {
        CourseDocumentPayload {
            version: 1,
            course_id: "test_course_001".to_string(),
            author_address: "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu85qsqfy".to_string(),
            title: "Algorithm Design and Analysis".to_string(),
            description: Some("A comprehensive course on algorithms".to_string()),
            thumbnail_hash: None,
            tags: vec!["algorithms".to_string(), "cs".to_string()],
            skill_ids: vec!["skill_001".to_string()],
            chapters: vec![DocumentChapter {
                id: "ch_001".to_string(),
                position: 0,
                title: "Introduction to Graphs".to_string(),
                description: Some("Foundations of graph theory".to_string()),
                elements: vec![
                    DocumentElement {
                        id: "el_001".to_string(),
                        position: 0,
                        title: "What is a Graph?".to_string(),
                        element_type: "video".to_string(),
                        content_hash: Some("a".repeat(64)),
                        duration_seconds: Some(1200),
                    },
                    DocumentElement {
                        id: "el_002".to_string(),
                        position: 1,
                        title: "Graph Terminology Quiz".to_string(),
                        element_type: "quiz".to_string(),
                        content_hash: None,
                        duration_seconds: None,
                    },
                ],
            }],
            created_at: 1700000000,
            updated_at: 1700100000,
        }
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let key = make_signing_key();
        let payload = make_payload();

        let signed = sign_course_document(&payload, &key).unwrap();
        assert_eq!(signed.title, payload.title);
        assert_eq!(signed.signature.len(), 128); // 64 bytes hex
        assert_eq!(signed.public_key.len(), 64); // 32 bytes hex

        verify_course_document(&signed).unwrap();
    }

    #[test]
    fn verify_rejects_tampered_title() {
        let key = make_signing_key();
        let payload = make_payload();

        let mut signed = sign_course_document(&payload, &key).unwrap();
        signed.title = "Tampered Title".to_string();

        assert!(matches!(
            verify_course_document(&signed),
            Err(CourseDocError::InvalidSignature)
        ));
    }

    #[test]
    fn verify_rejects_tampered_chapter() {
        let key = make_signing_key();
        let payload = make_payload();

        let mut signed = sign_course_document(&payload, &key).unwrap();
        signed.chapters[0].title = "Tampered Chapter".to_string();

        assert!(matches!(
            verify_course_document(&signed),
            Err(CourseDocError::InvalidSignature)
        ));
    }

    #[test]
    fn verify_rejects_wrong_key() {
        let key1 = make_signing_key();
        let key2 = make_signing_key();
        let payload = make_payload();

        let mut signed = sign_course_document(&payload, &key1).unwrap();
        // Replace public key with key2's
        signed.public_key = hex::encode(key2.verifying_key().to_bytes());

        assert!(matches!(
            verify_course_document(&signed),
            Err(CourseDocError::InvalidSignature)
        ));
    }

    #[test]
    fn payload_extraction_matches_original() {
        let key = make_signing_key();
        let payload = make_payload();

        let signed = sign_course_document(&payload, &key).unwrap();
        let extracted = signed.payload();

        assert_eq!(extracted.course_id, payload.course_id);
        assert_eq!(extracted.title, payload.title);
        assert_eq!(extracted.chapters.len(), payload.chapters.len());
        assert_eq!(
            extracted.chapters[0].elements.len(),
            payload.chapters[0].elements.len()
        );
    }

    #[test]
    fn signed_document_serializes_to_json() {
        let key = make_signing_key();
        let payload = make_payload();

        let signed = sign_course_document(&payload, &key).unwrap();
        let json = serde_json::to_string_pretty(&signed).unwrap();

        // Verify it round-trips through JSON
        let deserialized: SignedCourseDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, signed.title);
        assert_eq!(deserialized.signature, signed.signature);
        verify_course_document(&deserialized).unwrap();
    }

    #[tokio::test]
    async fn publish_and_resolve_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let node = ContentNode::new(tmp.path());
        node.start().await.unwrap();

        let key = make_signing_key();
        let payload = make_payload();
        let signed = sign_course_document(&payload, &key).unwrap();

        // Publish
        let result = publish_course_document(&node, &signed).await.unwrap();
        assert_eq!(result.content_hash.len(), 64);
        assert!(result.size > 0);

        // Resolve
        let resolved = resolve_course_document(&node, &result.content_hash)
            .await
            .unwrap();
        assert_eq!(resolved.title, signed.title);
        assert_eq!(resolved.course_id, signed.course_id);
        assert_eq!(resolved.chapters.len(), 1);
        assert_eq!(resolved.chapters[0].elements.len(), 2);

        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn publish_same_document_twice_gives_same_hash() {
        let tmp = TempDir::new().unwrap();
        let node = ContentNode::new(tmp.path());
        node.start().await.unwrap();

        let key = make_signing_key();
        let payload = make_payload();
        let signed = sign_course_document(&payload, &key).unwrap();

        let r1 = publish_course_document(&node, &signed).await.unwrap();
        let r2 = publish_course_document(&node, &signed).await.unwrap();
        assert_eq!(r1.content_hash, r2.content_hash);

        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn resolve_nonexistent_returns_not_found() {
        let tmp = TempDir::new().unwrap();
        let node = ContentNode::new(tmp.path());
        node.start().await.unwrap();

        let fake_hash = "0".repeat(64);
        let result = resolve_course_document(&node, &fake_hash).await;
        assert!(matches!(result, Err(CourseDocError::NotFound(_))));

        node.shutdown().await.unwrap();
    }
}
