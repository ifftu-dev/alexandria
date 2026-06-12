use serde::{Deserialize, Serialize};

/// The local user's identity and profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub stake_address: String,
    pub payment_address: String,
    /// Stable @handle, set at signup. Lowercase `[a-z0-9_]{3,32}`.
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar_cid: Option<String>,
    /// `"public"` (default) or `"private"`. Private profiles answer
    /// the profile-fetch protocol with no fields.
    pub visibility: Option<String>,
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
    /// `"public"` or `"private"`.
    pub visibility: Option<String>,
}

/// Username validation shared by signup paths and the IPC layer.
/// Lowercase letters, digits, underscore; 3–32 chars. Returns the
/// normalized (lowercased, trimmed) handle.
pub fn validate_username(username: &str) -> Result<String, String> {
    let u = username.trim().to_lowercase();
    if u.len() < 3 || u.len() > 32 {
        return Err("Username must be 3–32 characters.".to_string());
    }
    if !u
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err("Username may only contain letters, numbers, and underscores.".to_string());
    }
    Ok(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_normalizes_and_validates() {
        assert_eq!(validate_username("  Ada_99 ").unwrap(), "ada_99");
        assert!(validate_username("ab").is_err()); // too short
        assert!(validate_username(&"x".repeat(33)).is_err()); // too long
        assert!(validate_username("has space").is_err());
        assert!(validate_username("emoji🙂").is_err());
        assert!(validate_username("dash-ed").is_err());
    }
}
