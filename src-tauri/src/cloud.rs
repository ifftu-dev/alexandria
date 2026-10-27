//! Where the app's HTTP services are, and how it talks to them: the
//! Alexandria Cloud origin, the loopback exception to HTTPS, and the HTTP
//! client for directory and status-list requests.

use crate::settings::registry::keys;
use crate::settings::SettingsStore;

/// The Cloud origin this app talks to, whether or not requests are switched
/// on: the developer override, else the network profile's. Only https, or
/// http on loopback, counts.
pub fn origin(conn: &rusqlite::Connection) -> Option<String> {
    let configured = SettingsStore::get(conn, keys::CLOUD_ORIGIN);
    let origin = if !configured.trim().is_empty() {
        configured.trim().to_string()
    } else {
        crate::network_profile::embedded_preprod()
            .ok()
            .and_then(|profile| profile.cloud_https_origin.clone())?
    };
    let origin = origin.trim_end_matches('/').to_string();
    (origin.starts_with("https://") || is_loopback(&origin)).then_some(origin)
}

/// Loopback is allowed so somebody can run a directory locally and try this
/// without a certificate. Nothing else plaintext is.
///
/// The host has to *end* there. A prefix check alone would accept
/// `http://127.0.0.1.example.com`, which is a domain somebody else controls
/// wearing a loopback address as a costume — and it would arrive over
/// plaintext carrying a proof header.
pub fn is_loopback(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    for host in ["127.0.0.1", "localhost", "[::1]"] {
        if let Some(after) = rest.strip_prefix(host) {
            if after.is_empty() || after.starts_with(':') || after.starts_with('/') {
                return true;
            }
        }
    }
    false
}

/// An HTTP client that follows no redirects and gives up after 15 seconds.
pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_is_refused_except_on_loopback() {
        assert!(is_loopback("http://127.0.0.1:8787"));
        assert!(is_loopback("http://localhost:8787"));
        assert!(!is_loopback("http://registry.example.com"));
        assert!(!is_loopback("http://127.0.0.1.example.com"));
    }
}
