//! Content identifier detection and parsing.
//!
//! Alexandria uses two addressing schemes:
//!   - **BLAKE3 hex** (64-char hex) — native iroh content hashes
//!   - **IPFS CID** (CIDv0 `Qm...` or CIDv1 `bafy...`) — v1 platform content
//!
//! This module detects which format an identifier uses and provides
//! utilities for working with both.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CidError {
    #[error("unrecognised content identifier format: {0}")]
    Unrecognised(String),
}

/// A parsed content identifier — either a BLAKE3 hex hash or an IPFS CID.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentId {
    /// 64-char lowercase hex-encoded BLAKE3 hash (from iroh).
    Blake3Hex(String),
    /// IPFS CID string (CIDv0 or CIDv1).
    IpfsCid(String),
}

impl ContentId {
    /// Return the raw string regardless of variant.
    pub fn as_str(&self) -> &str {
        match self {
            ContentId::Blake3Hex(s) => s,
            ContentId::IpfsCid(s) => s,
        }
    }
}

/// Parse a content identifier string into its typed form.
///
/// Detection rules:
///   - 64-char hex string → `Blake3Hex`
///   - Starts with `Qm` and is 46 chars (CIDv0 base58) → `IpfsCid`
///   - Starts with `bafy` (CIDv1 base32) → `IpfsCid`
///   - Starts with `bafk` (CIDv1 base32, dag-cbor) → `IpfsCid`
///   - Otherwise → error
pub fn parse_content_id(id: &str) -> Result<ContentId, CidError> {
    let id = id.trim();

    if is_blake3_hex(id) {
        return Ok(ContentId::Blake3Hex(id.to_lowercase()));
    }

    if is_ipfs_cid(id) {
        return Ok(ContentId::IpfsCid(id.to_string()));
    }

    Err(CidError::Unrecognised(id.to_string()))
}

/// Check if the string looks like a 64-char hex-encoded BLAKE3 hash.
pub fn is_blake3_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Check if the string looks like an IPFS CID (v0 or v1).
pub fn is_ipfs_cid(s: &str) -> bool {
    // CIDv0: starts with "Qm", exactly 46 chars, base58btc
    if s.starts_with("Qm") && s.len() == 46 && is_base58(s) {
        return true;
    }

    // CIDv1: starts with "bafy" (dag-pb) or "bafk" (dag-cbor), base32lower
    // Typically 59 chars but can vary, so just check prefix + min length
    if (s.starts_with("bafy") || s.starts_with("bafk")) && s.len() >= 50 {
        return true;
    }

    false
}

/// Check if all characters are valid base58btc (no 0, O, I, l).
fn is_base58(s: &str) -> bool {
    s.chars().all(|c| {
        matches!(c,
            '1'..='9' | 'A'..='H' | 'J'..='N' | 'P'..='Z' |
            'a'..='k' | 'm'..='z'
        )
    })
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
    fn detects_cidv0() {
        // Real CIDv0 (base58, 46 chars, starts with Qm)
        let cid = "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG";
        assert!(is_ipfs_cid(cid));
        assert_eq!(
            parse_content_id(cid).unwrap(),
            ContentId::IpfsCid(cid.to_string())
        );
    }

    #[test]
    fn detects_cidv1_dag_pb() {
        let cid = "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi";
        assert!(is_ipfs_cid(cid));
        assert_eq!(
            parse_content_id(cid).unwrap(),
            ContentId::IpfsCid(cid.to_string())
        );
    }

    #[test]
    fn detects_cidv1_dag_cbor() {
        let cid = "bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenesa7olm";
        assert!(is_ipfs_cid(cid));
        assert_eq!(
            parse_content_id(cid).unwrap(),
            ContentId::IpfsCid(cid.to_string())
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_content_id("hello world").is_err());
        assert!(parse_content_id("").is_err());
        assert!(parse_content_id("abc123").is_err());
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

        let cid = "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG";
        let id = ContentId::IpfsCid(cid.to_string());
        assert_eq!(id.as_str(), cid);
    }
}
