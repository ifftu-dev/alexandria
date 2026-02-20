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
