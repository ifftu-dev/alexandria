//! Where the app's HTTP services are, and how it talks to them: the
//! Alexandria Cloud origin, the loopback exception to HTTPS, and the HTTP
//! client for directory and status-list requests.

use std::net::{Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

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
    is_allowed_origin(&origin).then_some(origin)
}

/// Loopback is allowed so somebody can run a directory locally and try this
/// without a certificate. Nothing else plaintext is.
///
/// The URL is parsed, and its host has to *be* loopback. A prefix check would
/// accept `http://127.0.0.1.example.com`, a domain somebody else controls
/// wearing a loopback address as a costume, and `http://127.0.0.1:80@evil`,
/// whose host is `evil`; either would receive a proof header over plaintext.
/// A URL with userinfo is refused outright.
pub fn is_loopback(url: &str) -> bool {
    let Some(url) = parse(url) else {
        return false;
    };
    url.scheme() == "http"
        && match url.host() {
            Some(Host::Domain(name)) => name == "localhost",
            Some(Host::Ipv4(ip)) => ip == Ipv4Addr::LOCALHOST,
            Some(Host::Ipv6(ip)) => ip == Ipv6Addr::LOCALHOST,
            None => false,
        }
}

/// Whether the app may send requests to `url`: `https`, or `http` on
/// loopback ([`is_loopback`]), with a host and no userinfo. A status list
/// named at any other origin could be swapped on the wire, and a verifier
/// would believe it.
pub fn is_allowed_origin(url: &str) -> bool {
    parse(url).is_some_and(|parsed| parsed.scheme() == "https" && parsed.host().is_some())
        || is_loopback(url)
}

/// `url` parsed, unless it carries userinfo.
fn parse(url: &str) -> Option<Url> {
    let url = Url::parse(url).ok()?;
    (url.username().is_empty() && url.password().is_none()).then_some(url)
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

    #[test]
    fn a_loopback_address_in_the_userinfo_is_not_loopback() {
        for url in [
            "http://127.0.0.1:80@attacker.example",
            "http://127.0.0.1@attacker.example/status-lists/x/1",
            "http://localhost:8787@attacker.example",
            "http://[::1]:80@attacker.example",
            "http://user:pass@127.0.0.1:8787",
        ] {
            assert!(!is_loopback(url), "{url}");
            assert!(!is_allowed_origin(url), "{url}");
        }
    }

    #[test]
    fn a_loopback_name_inside_a_longer_host_is_not_loopback() {
        for url in [
            "http://127.0.0.1.evil.com",
            "http://127.0.0.1.evil.com:8787/path",
            "http://localhost.evil.com",
            "http://localhost.evil.com:8787",
        ] {
            assert!(!is_loopback(url), "{url}");
            assert!(!is_allowed_origin(url), "{url}");
        }
    }

    #[test]
    fn ipv6_loopback_and_any_scheme_case_count() {
        assert!(is_loopback("http://[::1]"));
        assert!(is_loopback("http://[::1]:8787/status-lists"));
        assert!(is_loopback("HTTP://127.0.0.1:8787"));
        assert!(is_allowed_origin("HTTPS://cloud.example"));
        assert!(
            !is_loopback("https://127.0.0.1:8787"),
            "loopback is the http exception"
        );
    }

    #[test]
    fn an_allowed_origin_is_https_or_http_on_loopback() {
        assert!(is_allowed_origin("https://cloud.example"));
        assert!(is_allowed_origin("https://cloud.example/status-lists/x/1"));
        assert!(is_allowed_origin("http://127.0.0.1:8787"));
        assert!(is_allowed_origin("http://localhost"));
        for url in [
            "http://cloud.example",
            "ftp://cloud.example",
            "cloud.example",
            "",
            "https://user@cloud.example",
            "https://user:pass@cloud.example",
        ] {
            assert!(!is_allowed_origin(url), "{url:?}");
        }
    }

    /// Signed directory requests carry a proof header; a redirect must not
    /// carry it on to another host.
    #[tokio::test]
    async fn the_client_does_not_follow_redirects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 302 Found\r\nlocation: http://attacker.example/\r\n\
                      content-length: 0\r\nconnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        });
        let response = client().unwrap().get(&origin).send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
    }
}
