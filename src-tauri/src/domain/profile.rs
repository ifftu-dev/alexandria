//! Profile document types for IPFS-backed user profiles.
//!
//! A profile document is a signed JSON blob stored on iroh. It contains
//! the user's public identity information (name, bio, avatar) along with
//! a cryptographic signature proving the owner authored it.
//!
//! Architecture spec (v2, Section 5.2):
//! ```json
//! {
//!   "version": 1,
//!   "stake_address": "stake_test1...",
//!   "name": "Alice",
//!   "bio": "Studying computer science",
//!   "avatar_cid": "bafy...abc",
//!   "created_at": 1740000000,
//!   "updated_at": 1740100000,
//!   "signature": "ed25519:<hex>"
//! }
//! ```

use serde::{Deserialize, Serialize};

/// The profile document payload (unsigned).
///
/// This is the data that gets signed. The signature covers the
/// canonical JSON serialization of this struct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfilePayload {
    /// Document format version (currently 1).
    pub version: u32,
    /// The author's Cardano stake address (bech32).
    pub stake_address: String,
    /// Display name (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Short biography (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    /// BLAKE3 hash of the avatar image blob (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_hash: Option<String>,
    /// Unix timestamp when the profile was first created.
    pub created_at: i64,
    /// Unix timestamp of this version.
    pub updated_at: i64,
}

/// A signed profile document, ready for storage on iroh.
///
/// The `signature` field is an Ed25519 signature over the canonical
/// JSON serialization of the `ProfilePayload` fields (everything
/// except `signature` and `public_key`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedProfile {
    /// Document format version.
    pub version: u32,
    /// The author's Cardano stake address (bech32).
    pub stake_address: String,
    /// Display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Short biography.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    /// BLAKE3 hash of avatar image (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_hash: Option<String>,
    /// Unix timestamp when the profile was first created.
    pub created_at: i64,
    /// Unix timestamp of this version.
    pub updated_at: i64,
    /// Ed25519 signature over the payload JSON, hex-encoded.
    pub signature: String,
    /// Ed25519 public key of the signer, hex-encoded.
    pub public_key: String,
}

impl SignedProfile {
    /// Extract the unsigned payload for verification.
    pub fn payload(&self) -> ProfilePayload {
        ProfilePayload {
            version: self.version,
            stake_address: self.stake_address.clone(),
            name: self.name.clone(),
            bio: self.bio.clone(),
            avatar_hash: self.avatar_hash.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Result of publishing a profile to iroh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishProfileResult {
    /// The BLAKE3 hash of the stored profile document (hex).
    pub profile_hash: String,
    /// The profile that was published.
    pub profile: SignedProfile,
}
