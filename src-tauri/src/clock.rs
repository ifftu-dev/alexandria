//! The current time as the app stores it.

/// Now in UTC as RFC 3339 with whole seconds and a `Z` suffix, the format
/// of credential timestamps and most stored times. Chat stores
/// milliseconds instead ([`crate::ai_chat::timestamp`]).
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
