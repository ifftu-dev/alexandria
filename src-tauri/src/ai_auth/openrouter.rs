//! Sign in with OpenRouter.
//!
//! OpenRouter's OAuth PKCE flow lets a user authorize an app, which then
//! receives an API key tied to the user's own OpenRouter account and credits
//! (openrouter.ai/docs, "OAuth PKCE"). Localhost callbacks are accepted on
//! any port. The key is then used like any pasted key. OpenRouter requires
//! users to be 18 or older, which the host enforces before starting.
//!
//! Phones cannot keep a loopback listener alive while the browser is in
//! front, so they use OpenRouter's documented headless mode instead: the
//! authorize URL has no `callback_url`, OpenRouter shows the code on screen,
//! and the user pastes it into Alexandria, which exchanges it with the PKCE
//! verifier it kept ([`code_url`], [`exchange`]).

use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::{http_client, pkce, random_token, DonePage, Loopback, SignInError};

pub const API_BASE: &str = "https://openrouter.ai/api/v1";
/// OpenRouter's automatic router: picks a model per request, so a new
/// connection works before the user chooses one.
pub const DEFAULT_MODEL: &str = "openrouter/auto";
pub const KEYS_URL: &str = "https://openrouter.ai/settings/keys";
const CALLBACK_PATH: &str = "/callback";

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub authorize: String,
    pub keys: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            authorize: "https://openrouter.ai/auth".into(),
            keys: "https://openrouter.ai/api/v1/auth/keys".into(),
        }
    }
}

/// Runs the browser sign-in and returns the issued API key.
pub async fn sign_in(
    endpoints: &Endpoints,
    page: &DonePage,
    cancel: &mut oneshot::Receiver<()>,
    open: impl FnOnce(&str) -> Result<(), SignInError>,
) -> Result<String, SignInError> {
    let loopback = Loopback::bind().await?;
    let callback = format!("http://localhost:{}{CALLBACK_PATH}", loopback.port());
    let (verifier, challenge) = pkce();
    let state = random_token(24);
    let mut url = url::Url::parse(&endpoints.authorize)
        .map_err(|_| SignInError::Failed("bad sign-in address".into()))?;
    url.query_pairs_mut()
        .append_pair("callback_url", &callback)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("key_label", "Alexandria")
        .append_pair("state", &state);
    open(url.as_str())?;
    let query = loopback
        .wait(CALLBACK_PATH, &state, page, cancel, super::SIGN_IN_TIMEOUT)
        .await?;
    let code = query.get("code").ok_or(SignInError::Declined)?;
    exchange(endpoints, code, &verifier).await
}

/// The authorize URL for the paste-the-code flow: no callback, so OpenRouter
/// shows the code to the user. A PKCE challenge is required in this mode.
pub fn code_url(endpoints: &Endpoints, challenge: &str) -> Result<String, SignInError> {
    let mut url = url::Url::parse(&endpoints.authorize)
        .map_err(|_| SignInError::Failed("bad sign-in address".into()))?;
    url.query_pairs_mut()
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("key_label", "Alexandria");
    Ok(url.into())
}

/// Whether `code` looks like an authorization code. Pasted text is trimmed
/// first; anything else is refused before it is sent anywhere.
pub fn clean_code(code: &str) -> Option<&str> {
    let code = code.trim();
    (!code.is_empty()
        && code.len() <= 512
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')))
    .then_some(code)
}

/// Exchanges an authorization code and its PKCE verifier for the user's key.
pub async fn exchange(
    endpoints: &Endpoints,
    code: &str,
    verifier: &str,
) -> Result<String, SignInError> {
    let response = http_client()?
        .post(&endpoints.keys)
        .json(&json!({
            "code": code,
            "code_verifier": verifier,
            "code_challenge_method": "S256",
        }))
        .send()
        .await
        .map_err(|_| SignInError::Unreachable)?;
    if response.status().is_server_error() {
        return Err(SignInError::Unreachable);
    }
    if !response.status().is_success() {
        return Err(SignInError::CodeRejected);
    }
    let body: Value = response
        .json()
        .await
        .map_err(|_| SignInError::Failed("OpenRouter answered unexpectedly".into()))?;
    body.get("key")
        .and_then(Value::as_str)
        .filter(|k| !k.is_empty() && k.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| SignInError::Failed("OpenRouter returned no key".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn the_code_url_has_no_callback_and_pasted_codes_are_checked() {
        let url = code_url(&Endpoints::default(), "ch").unwrap();
        assert!(url.starts_with("https://openrouter.ai/auth?"));
        assert!(url.contains("code_challenge=ch") && url.contains("code_challenge_method=S256"));
        assert!(!url.contains("callback_url"));
        assert_eq!(clean_code("  abc-123_x.y \n"), Some("abc-123_x.y"));
        for bad in ["", "   ", "a b", "<script>", "a/b", &"x".repeat(513)] {
            assert_eq!(clean_code(bad), None, "{bad}");
        }
    }

    #[tokio::test]
    async fn a_refused_code_is_reported_as_such() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let keys = format!("http://{}/keys", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = s.read(&mut buf).await;
            let resp = "HTTP/1.1 403 Forbidden\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
            s.write_all(resp.as_bytes()).await.unwrap();
        });
        let endpoints = Endpoints {
            authorize: "https://openrouter.example/auth".into(),
            keys,
        };
        assert_eq!(
            exchange(&endpoints, "c1", "v").await.unwrap_err(),
            SignInError::CodeRejected
        );
    }

    #[tokio::test]
    async fn the_code_is_exchanged_for_a_key_with_the_verifier() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let keys = format!("http://{}/keys", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(String::new()));
        let seen_by_server = seen.clone();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).await.unwrap();
            *seen_by_server.lock().unwrap() = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = r#"{"key":"sk-or-v1-abc"}"#;
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            s.write_all(resp.as_bytes()).await.unwrap();
        });
        let endpoints = Endpoints {
            authorize: "https://openrouter.example/auth".into(),
            keys,
        };
        let opened = Arc::new(Mutex::new(String::new()));
        let opened_by_browser = opened.clone();
        let (_tx, mut cancel) = oneshot::channel();
        let key = sign_in(&endpoints, &DonePage::default(), &mut cancel, move |url| {
            *opened_by_browser.lock().unwrap() = url.to_owned();
            let parsed = url::Url::parse(url).unwrap();
            let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
            let callback = url::Url::parse(&q["callback_url"]).unwrap();
            let target = format!("{}?code=c1&state={}", callback.path(), q["state"]);
            tokio::spawn(async move {
                // A browser may resolve localhost to either address family.
                let mut s = tokio::net::TcpStream::connect(("127.0.0.1", callback.port().unwrap()))
                    .await
                    .unwrap();
                s.write_all(format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                let mut out = String::new();
                let _ = s.read_to_string(&mut out).await;
            });
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(key, "sk-or-v1-abc");
        let url = opened.lock().unwrap().clone();
        assert!(url.contains("callback_url=http%3A%2F%2Flocalhost%3A"));
        assert!(url.contains("code_challenge_method=S256"));
        let exchange = seen.lock().unwrap().clone();
        assert!(exchange.contains("\"code\":\"c1\"") && exchange.contains("\"code_verifier\""));
    }
}
