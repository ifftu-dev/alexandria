//! The signed export of a learner's activity log.
//!
//! An export is two files:
//!
//! * **The statement file**: a JSON array of xAPI statements, readable by any
//!   tool that reads xAPI. This crate never parses it; what is signed is its
//!   exact bytes.
//! * **The signature file** ([`ExportSignature`], conventionally the statement
//!   file's name plus `.sig`): a small JSON object naming the format, the
//!   signer's `did:key`, the statement file's SHA-256 and an Ed25519
//!   signature by that key over `EXPORT_FORMAT || 0x00 || file bytes`.
//!
//! The xAPI per-statement signature (a JWS attachment) is deliberately not
//! used: the standard requires RS256, RS384 or RS512, which a learner's
//! Ed25519 key cannot produce.
//!
//! To check an export by hand: SHA-256 the statement file and compare it with
//! `sha256`; resolve `signer` as a `did:key` to its Ed25519 public key;
//! base64url-decode `signature` (no padding, 64 bytes) and verify it over the
//! ASCII bytes of `format`, one zero byte, then the statement file unchanged.
//! [`verify`] does exactly that, and returns the signer so the caller can
//! compare it with the learner it expects.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::did::{derive_did_key, resolve_did_key, Did};

/// The signature file's format tag, also the signed domain separator.
pub const EXPORT_FORMAT: &str = "alexandria-record-export/1";

/// The largest signature file [`verify`] reads.
pub const MAX_SIGNATURE_FILE_BYTES: usize = 4096;

/// The detached signature over a statement file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportSignature {
    /// [`EXPORT_FORMAT`].
    pub format: String,
    /// The learner's `did:key`.
    pub signer: String,
    /// Lower-case hex SHA-256 of the statement file.
    pub sha256: String,
    /// base64url (no padding) Ed25519 signature over [`signing_bytes`].
    pub signature: String,
}

/// Why an export did not verify. [`ExportError::code`] is stable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    #[error("malformed signature file: {0}")]
    Malformed(String),
    #[error("unknown signature format {0}")]
    UnknownFormat(String),
    #[error("signer is not a did:key")]
    InvalidSigner,
    /// The signature file names a different statement file: the file was
    /// changed, or the two files do not belong together.
    #[error("the statement file is not the one that was signed")]
    WrongFile,
    #[error("signature does not verify")]
    BadSignature,
}

impl ExportError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Malformed(_) => "malformed",
            Self::UnknownFormat(_) => "unknown_format",
            Self::InvalidSigner => "invalid_signer",
            Self::WrongFile => "wrong_file",
            Self::BadSignature => "bad_signature",
        }
    }
}

/// What the learner's key signs: `EXPORT_FORMAT || 0x00 || file`.
pub fn signing_bytes(file: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(EXPORT_FORMAT.len() + 1 + file.len());
    bytes.extend_from_slice(EXPORT_FORMAT.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(file);
    bytes
}

fn sha256_hex(file: &[u8]) -> String {
    hex::encode(Sha256::digest(file))
}

/// Signs the exact bytes of a statement file with the learner's key.
pub fn sign(file: &[u8], key: &ed25519_dalek::SigningKey) -> ExportSignature {
    use ed25519_dalek::Signer;
    ExportSignature {
        format: EXPORT_FORMAT.to_string(),
        signer: derive_did_key(key).as_str().to_string(),
        sha256: sha256_hex(file),
        signature: URL_SAFE_NO_PAD.encode(key.sign(&signing_bytes(file)).to_bytes()),
    }
}

/// The signature file's bytes: pretty JSON with a trailing newline.
pub fn signature_file(signature: &ExportSignature) -> Vec<u8> {
    let mut bytes =
        serde_json::to_vec_pretty(signature).expect("a struct of strings always serializes");
    bytes.push(b'\n');
    bytes
}

/// Checks a statement file against its signature file and returns the
/// signer. The caller decides whether that signer is the learner it expects.
pub fn verify(file: &[u8], signature_file: &[u8]) -> Result<Did, ExportError> {
    use ed25519_dalek::Verifier;
    if signature_file.len() > MAX_SIGNATURE_FILE_BYTES {
        return Err(ExportError::Malformed("larger than 4 KiB".into()));
    }
    let signature: ExportSignature = serde_json::from_slice(signature_file)
        .map_err(|e| ExportError::Malformed(e.to_string()))?;
    if signature.format != EXPORT_FORMAT {
        return Err(ExportError::UnknownFormat(signature.format));
    }
    let signer = Did(signature.signer);
    let key = resolve_did_key(&signer).map_err(|_| ExportError::InvalidSigner)?;
    if signature.sha256 != sha256_hex(file) {
        return Err(ExportError::WrongFile);
    }
    let bytes: [u8; 64] = URL_SAFE_NO_PAD
        .decode(&signature.signature)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| ExportError::Malformed("signature encoding".into()))?;
    key.verify(
        &signing_bytes(file),
        &ed25519_dalek::Signature::from_bytes(&bytes),
    )
    .map_err(|_| ExportError::BadSignature)?;
    Ok(signer)
}
