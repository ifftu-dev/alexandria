//! Content identifier detection and parsing.
//!
//! Alexandria addresses content two ways:
//!   - **BLAKE3 hex** (64-char hex) — native iroh content hashes
//!   - **Public URL** — HTTP(S) source for seeded/imported media, cached
//!     into iroh on first fetch
//!
//! This module detects which format an identifier uses.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CidError {
    #[error("unrecognised content identifier format: {0}")]
    Unrecognised(String),
}

/// A parsed content identifier — either a BLAKE3 hex hash or a public URL.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentId {
    /// 64-char lowercase hex-encoded BLAKE3 hash (from iroh).
    Blake3Hex(String),
    /// Public HTTPS/HTTP URL.
    Url(String),
}

impl ContentId {
    /// Return the raw string regardless of variant.
    pub fn as_str(&self) -> &str {
        match self {
            ContentId::Blake3Hex(s) => s,
            ContentId::Url(s) => s,
        }
    }
}

/// Parse a content identifier string into its typed form.
///
/// Detection rules:
///   - 64-char hex string → `Blake3Hex`
///   - Starts with `http://` or `https://` → `Url`
///   - Otherwise → error
pub fn parse_content_id(id: &str) -> Result<ContentId, CidError> {
    let id = id.trim();

    if is_blake3_hex(id) {
        return Ok(ContentId::Blake3Hex(id.to_lowercase()));
    }

    if is_http_url(id) {
        return Ok(ContentId::Url(id.to_string()));
    }

    Err(CidError::Unrecognised(id.to_string()))
}

/// Check if the string is an HTTP(S) URL.
pub fn is_http_url(s: &str) -> bool {
    s.starts_with("https://") || s.starts_with("http://")
}

/// Check if the string looks like a 64-char hex-encoded BLAKE3 hash.
pub fn is_blake3_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_blake3_hex() {
        let hash = "a".repeat(64);
        assert!(is_blake3_hex(&hash));
        assert_eq!(
            parse_content_id(&hash).unwrap(),
            ContentId::Blake3Hex(hash.clone())
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_content_id("hello world").is_err());
        assert!(parse_content_id("").is_err());
        assert!(parse_content_id("abc123").is_err());
    }

    #[test]
    fn rejects_ipfs_cid() {
        // Legacy IPFS CIDs are no longer a recognised addressing scheme.
        assert!(parse_content_id("QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG").is_err());
        assert!(
            parse_content_id("bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi")
                .is_err()
        );
    }

    #[test]
    fn blake3_normalises_to_lowercase() {
        let hash = "A".repeat(64);
        match parse_content_id(&hash).unwrap() {
            ContentId::Blake3Hex(h) => assert_eq!(h, "a".repeat(64)),
            _ => panic!("expected Blake3Hex"),
        }
    }

    #[test]
    fn content_id_as_str() {
        let hash = "b".repeat(64);
        let id = ContentId::Blake3Hex(hash.clone());
        assert_eq!(id.as_str(), hash);

        let url = "https://example.org/file.bin";
        let id = ContentId::Url(url.to_string());
        assert_eq!(id.as_str(), url);
    }

    #[test]
    fn detects_http_url() {
        let url = "https://example.org/media/file";
        assert!(is_http_url(url));
        assert_eq!(
            parse_content_id(url).unwrap(),
            ContentId::Url(url.to_string())
        );
    }
}
