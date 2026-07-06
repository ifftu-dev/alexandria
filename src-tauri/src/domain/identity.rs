use chrono::Datelike;
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

/// Account role chosen at onboarding.
pub const ACCOUNT_ROLES: &[&str] = &["learner", "instructor", "parent"];

/// Role + gating status surfaced to the frontend. Age is computed, never stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountStatus {
    pub role: String,
    /// ISO-8601 date (`YYYY-MM-DD`), learners only. Local-only: never published.
    pub birthdate: Option<String>,
    pub is_minor: bool,
    /// `"active"` or `"pending_guardian"` (minor awaiting guardian link).
    pub activation_state: String,
}

/// Whether a birthdate makes the holder a minor (< 18) as of `today`.
/// Minority is recomputed from the birthdate wherever it matters, so
/// turning 18 takes effect at the next check with no stored age to go stale.
/// An unparseable birthdate is treated as adult (birthdates are validated
/// on write; this only guards against hand-edited rows).
pub fn is_minor(birthdate: &str, today: chrono::NaiveDate) -> bool {
    match chrono::NaiveDate::parse_from_str(birthdate, "%Y-%m-%d") {
        Ok(born) => {
            let adult_at = match born.with_year(born.year() + 18) {
                Some(d) => d,
                // Feb 29 birthdate in a non-leap target year: adulthood on Mar 1.
                None => chrono::NaiveDate::from_ymd_opt(born.year() + 18, 3, 1)
                    .expect("Mar 1 always exists"),
            };
            today < adult_at
        }
        Err(_) => false,
    }
}

/// Birthdate validation shared by signup and the IPC layer: ISO date,
/// not in the future, not more than 120 years ago.
pub fn validate_birthdate(birthdate: &str, today: chrono::NaiveDate) -> Result<String, String> {
    let b = birthdate.trim().to_string();
    let born = chrono::NaiveDate::parse_from_str(&b, "%Y-%m-%d")
        .map_err(|_| "Birthdate must be a valid date (YYYY-MM-DD).".to_string())?;
    if born > today {
        return Err("Birthdate cannot be in the future.".to_string());
    }
    if born < today - chrono::Duration::days(120 * 366) {
        return Err("Birthdate is too far in the past.".to_string());
    }
    Ok(b)
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

    fn d(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn minor_boundaries() {
        // 18th birthday today → adult.
        assert!(!is_minor("2008-07-04", d("2026-07-04")));
        // Day before 18th birthday → minor.
        assert!(is_minor("2008-07-05", d("2026-07-04")));
        // Clearly adult / clearly minor.
        assert!(!is_minor("1990-01-01", d("2026-07-04")));
        assert!(is_minor("2015-03-10", d("2026-07-04")));
        // Feb 29 birthdate: adulthood lands on Mar 1 in a non-leap year.
        assert!(is_minor("2008-02-29", d("2026-02-28")));
        assert!(!is_minor("2008-02-29", d("2026-03-01")));
        // Garbage never gates a profile.
        assert!(!is_minor("not-a-date", d("2026-07-04")));
    }

    #[test]
    fn birthdate_validates() {
        let today = d("2026-07-04");
        assert_eq!(
            validate_birthdate(" 2010-05-01 ", today).unwrap(),
            "2010-05-01"
        );
        assert!(validate_birthdate("2027-01-01", today).is_err()); // future
        assert!(validate_birthdate("1890-01-01", today).is_err()); // >120y
        assert!(validate_birthdate("05/01/2010", today).is_err()); // format
    }

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
