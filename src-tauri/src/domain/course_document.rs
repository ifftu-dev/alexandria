//! Course document types for IPFS publication.
//!
//! A course document is a signed JSON blob stored on iroh (or fetched
//! from IPFS gateways) that contains the full course structure:
//! metadata, chapters, elements, and asset references.
//!
//! The document format mirrors v1's protobuf structure but uses JSON
//! for universal parseability and human readability.

use serde::{Deserialize, Serialize};

/// The unsigned course document payload (everything that gets signed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseDocumentPayload {
    /// Document format version (currently 1).
    pub version: u32,
    /// Deterministic course ID: blake2b(author_address + title + timestamp).
    pub course_id: String,
    /// Cardano stake address of the author.
    pub author_address: String,
    /// Course title.
    pub title: String,
    /// Course description.
    pub description: Option<String>,
    /// BLAKE3 hash of the thumbnail image (if any).
    pub thumbnail_hash: Option<String>,
    /// Tags for discovery.
    pub tags: Vec<String>,
    /// Skill IDs this course covers.
    pub skill_ids: Vec<String>,
    /// Ordered list of chapters with nested elements.
    pub chapters: Vec<DocumentChapter>,
    /// Unix timestamp of course creation.
    pub created_at: i64,
    /// Unix timestamp of this publication.
    pub updated_at: i64,
}

/// A chapter in the course document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChapter {
    /// Chapter ID.
    pub id: String,
    /// Display position (0-indexed).
    pub position: i64,
    /// Chapter title.
    pub title: String,
    /// Chapter description.
    pub description: Option<String>,
    /// Ordered list of learning elements.
    pub elements: Vec<DocumentElement>,
}

/// A learning element in the course document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentElement {
    /// Element ID.
    pub id: String,
    /// Display position (0-indexed).
    pub position: i64,
    /// Element title.
    pub title: String,
    /// Element type: video, text, quiz, interactive, assessment.
    pub element_type: String,
    /// BLAKE3 hash of the element content blob (video, PDF, etc.).
    pub content_hash: Option<String>,
    /// Duration in seconds (for video/audio elements).
    pub duration_seconds: Option<i64>,
}

/// A signed course document (payload + Ed25519 signature).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedCourseDocument {
    // -- Payload fields (flattened) --
    pub version: u32,
    pub course_id: String,
    pub author_address: String,
    pub title: String,
    pub description: Option<String>,
    pub thumbnail_hash: Option<String>,
    pub tags: Vec<String>,
    pub skill_ids: Vec<String>,
    pub chapters: Vec<DocumentChapter>,
    pub created_at: i64,
    pub updated_at: i64,

    // -- Cryptographic fields --
    /// Ed25519 signature over the payload JSON (hex-encoded, 128 chars).
    pub signature: String,
    /// Ed25519 public key of the signer (hex-encoded, 64 chars).
    pub public_key: String,
}

impl SignedCourseDocument {
    /// Extract the unsigned payload for signature verification.
    pub fn payload(&self) -> CourseDocumentPayload {
        CourseDocumentPayload {
            version: self.version,
            course_id: self.course_id.clone(),
            author_address: self.author_address.clone(),
            title: self.title.clone(),
            description: self.description.clone(),
            thumbnail_hash: self.thumbnail_hash.clone(),
            tags: self.tags.clone(),
            skill_ids: self.skill_ids.clone(),
            chapters: self.chapters.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Result of publishing a course document to iroh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishCourseResult {
    /// BLAKE3 hash of the stored document (64-char hex).
    pub content_hash: String,
    /// Size of the document in bytes.
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_signed_course_doc() -> SignedCourseDocument {
        SignedCourseDocument {
            version: 1,
            course_id: "course1".into(),
            author_address: "stake_test1u123".into(),
            title: "Intro to Rust".into(),
            description: Some("Learn Rust".into()),
            thumbnail_hash: None,
            tags: vec!["rust".into(), "programming".into()],
            skill_ids: vec!["sk1".into()],
            chapters: vec![DocumentChapter {
                id: "ch1".into(),
                position: 0,
                title: "Getting Started".into(),
                description: None,
                elements: vec![DocumentElement {
                    id: "el1".into(),
                    position: 0,
                    title: "Install Rust".into(),
                    element_type: "text".into(),
                    content_hash: Some("hash1".into()),
                    duration_seconds: None,
                }],
            }],
            created_at: 1700000000,
            updated_at: 1700100000,
            signature: "deadbeef".into(),
            public_key: "cafebabe".into(),
        }
    }

    #[test]
    fn signed_course_document_payload_extracts() {
        let signed = sample_signed_course_doc();
        let payload = signed.payload();

        assert_eq!(payload.version, 1);
        assert_eq!(payload.course_id, "course1");
        assert_eq!(payload.author_address, "stake_test1u123");
        assert_eq!(payload.title, "Intro to Rust");
        assert_eq!(payload.tags.len(), 2);
        assert_eq!(payload.chapters.len(), 1);
        assert_eq!(payload.chapters[0].elements.len(), 1);
    }

    #[test]
    fn signed_course_document_serde_roundtrip() {
        let signed = sample_signed_course_doc();
        let json = serde_json::to_string(&signed).unwrap();
        let parsed: SignedCourseDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.signature, "deadbeef");
        assert_eq!(parsed.chapters.len(), 1);
        assert_eq!(parsed.chapters[0].elements[0].title, "Install Rust");
    }

    #[test]
    fn course_document_payload_serde_roundtrip() {
        let payload = CourseDocumentPayload {
            version: 1,
            course_id: "c1".into(),
            author_address: "addr1".into(),
            title: "Test".into(),
            description: None,
            thumbnail_hash: None,
            tags: vec![],
            skill_ids: vec![],
            chapters: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let json = serde_json::to_string(&payload).unwrap();
        let parsed: CourseDocumentPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.course_id, "c1");
        assert!(parsed.chapters.is_empty());
    }

    #[test]
    fn document_chapter_with_elements_serde() {
        let chapter = DocumentChapter {
            id: "ch1".into(),
            position: 0,
            title: "Chapter 1".into(),
            description: Some("First chapter".into()),
            elements: vec![
                DocumentElement {
                    id: "el1".into(),
                    position: 0,
                    title: "Video".into(),
                    element_type: "video".into(),
                    content_hash: Some("hash".into()),
                    duration_seconds: Some(300),
                },
                DocumentElement {
                    id: "el2".into(),
                    position: 1,
                    title: "Quiz".into(),
                    element_type: "assessment".into(),
                    content_hash: None,
                    duration_seconds: None,
                },
            ],
        };
        let json = serde_json::to_string(&chapter).unwrap();
        let parsed: DocumentChapter = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.elements.len(), 2);
        assert_eq!(parsed.elements[0].duration_seconds, Some(300));
        assert_eq!(parsed.elements[1].content_hash, None);
    }
}
