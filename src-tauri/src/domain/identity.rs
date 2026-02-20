use serde::{Deserialize, Serialize};

/// The local user's identity and profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub stake_address: String,
    pub payment_address: String,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar_cid: Option<String>,
    /// BLAKE3 hash of the published profile document on iroh.
    pub profile_hash: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Public-facing wallet info (no secrets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletInfo {
    pub stake_address: String,
    pub payment_address: String,
    pub has_mnemonic_backup: bool,
}

/// Profile update request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileUpdate {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar_cid: Option<String>,
}
