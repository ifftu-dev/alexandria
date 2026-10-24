//! Signing in to an AI provider in the browser instead of pasting a key.
//!
//! Only providers whose own terms offer this to third-party apps are
//! supported (see `alexandria_ai::terms` for the ones that forbid it):
//!
//! - ChatGPT: OpenAI's "Sign in with ChatGPT" plan-usage flow, offered to
//!   open-source and locally hosted apps. Requests then count against the
//!   user's ChatGPT plan ([`chatgpt`]).
//! - OpenRouter: its OAuth PKCE flow issues an API key tied to the user's own
//!   OpenRouter account ([`openrouter`]).
//!
//! Both flows open the system browser at the provider's own sign-in page and
//! receive the result on a one-shot HTTP listener bound to the loopback
//! interface. The listener answers only the expected path with the expected
//! `state`; anything else is ignored. Alexandria never sees a password.

pub mod chatgpt;
pub mod credential;
pub mod jwt;
pub mod openrouter;

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

/// How long a sign-in waits for the browser before giving up.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Why a sign-in did not finish. Display strings start with a stable code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignInError {
    #[error("signin_cancelled: sign-in was cancelled")]
    Cancelled,
    #[error("signin_timeout: sign-in took too long; try again")]
    Timeout,
    #[error("signin_declined: access was not granted")]
    Declined,
    #[error("plan_not_granted: ChatGPT plan use was not allowed for Alexandria")]
    PlanNotGranted,
    #[error(
        "different_account: that is a different ChatGPT account; disconnect the current one first"
    )]
    DifferentAccount,
    #[error("unreachable: the provider could not be reached")]
    Unreachable,
    #[error("code_rejected: the provider did not accept that code; start again")]
    CodeRejected,
    #[error("signin_expired: that sign-in expired; start again")]
    Expired,
    #[error("signin_failed: {0}")]
    Failed(String),
}

/// A fresh random value, base64url without padding.
pub fn random_token(bytes: usize) -> String {
    use rand::RngCore;
    let mut buf = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// A PKCE verifier and its S256 challenge.
pub fn pkce() -> (String, String) {
    let verifier = random_token(48);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// The HTTP client for provider sign-in endpoints: TLS, no redirects, short
/// timeouts.
pub fn http_client() -> Result<reqwest::Client, SignInError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| SignInError::Failed("could not start a secure connection".into()))
}

/// Opens a provider page in the system browser from the app's main thread,
/// which iOS requires. Works on every platform; phones use it for the
/// paste-the-code sign-in and provider pages.
pub async fn open_browser_on_main(app: &tauri::AppHandle, url: String) -> Result<(), SignInError> {
    let failed = || SignInError::Failed("could not open the browser".into());
    let (tx, rx) = oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(webbrowser::open(&url).is_ok());
    })
    .map_err(|_| failed())?;
    match tokio::time::timeout(Duration::from_secs(10), rx).await {
        Ok(Ok(true)) => Ok(()),
        _ => Err(failed()),
    }
}

/// Opens a provider page in the system browser.
pub fn open_browser(url: &str) -> Result<(), SignInError> {
    if cfg!(mobile) {
        return Err(SignInError::Failed(
            "sign-in in the browser is available on desktop".into(),
        ));
    }
    webbrowser::open(url).map_err(|_| SignInError::Failed("could not open the browser".into()))
}

/// What the browser page shows once Alexandria has the result. The text is
/// supplied by the frontend in the user's language.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct DonePage {
    pub title: String,
    pub body: String,
}

impl DonePage {
    fn html(&self) -> String {
        let esc = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
        };
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>{t}</title></head>\
<body style=\"font-family:system-ui,sans-serif;display:grid;place-items:center;min-height:90vh;margin:0;background:#f7f6f3;color:#1c1b19\">\
<main style=\"max-width:28rem;padding:2rem;text-align:center\"><h1 style=\"font-size:1.25rem\">{t}</h1><p style=\"color:#55524c\">{b}</p></main></body></html>",
            t = esc(&self.title),
            b = esc(&self.body)
        )
    }
}

/// A one-shot callback listener on the loopback interface. It binds IPv4
/// and, on the same port when possible, IPv6, because a browser may resolve
/// `localhost` to either.
pub struct Loopback {
    v4: TcpListener,
    v6: Option<TcpListener>,
    port: u16,
}

impl Loopback {
    pub async fn bind() -> Result<Self, SignInError> {
        let v4 = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|_| SignInError::Failed("could not listen for the browser".into()))?;
        let port = v4
            .local_addr()
            .map_err(|_| SignInError::Failed("could not listen for the browser".into()))?
            .port();
        let v6 = TcpListener::bind(("::1", port)).await.ok();
        Ok(Self { v4, v6, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Waits for `GET {path}?...&state={state}` and returns its query. Requests
    /// to other paths, or with another state, are answered and ignored.
    pub async fn wait(
        &self,
        path: &str,
        state: &str,
        page: &DonePage,
        cancel: &mut oneshot::Receiver<()>,
        timeout: Duration,
    ) -> Result<HashMap<String, String>, SignInError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let accepted = tokio::select! {
                _ = &mut *cancel => return Err(SignInError::Cancelled),
                _ = tokio::time::sleep_until(deadline) => return Err(SignInError::Timeout),
                a = self.v4.accept() => a,
                a = accept(self.v6.as_ref()) => a,
            };
            let Ok((stream, _)) = accepted else { continue };
            if let Some(query) = answer(stream, path, state, page).await {
                return Ok(query);
            }
        }
    }
}

async fn accept(
    listener: Option<&TcpListener>,
) -> std::io::Result<(TcpStream, std::net::SocketAddr)> {
    match listener {
        Some(l) => l.accept().await,
        None => std::future::pending().await,
    }
}

/// Reads one request and answers it. Returns the query when it is the
/// callback carrying the expected state.
async fn answer(
    mut stream: TcpStream,
    path: &str,
    state: &str,
    page: &DonePage,
) -> Option<HashMap<String, String>> {
    let mut buf = vec![0u8; 8192];
    let mut len = 0;
    let read = tokio::time::timeout(Duration::from_secs(5), async {
        while len < buf.len() {
            let n = stream.read(&mut buf[len..]).await.ok()?;
            if n == 0 {
                break;
            }
            len += n;
            if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        Some(())
    })
    .await;
    if !matches!(read, Ok(Some(()))) {
        return None;
    }
    let head = String::from_utf8_lossy(&buf[..len]);
    let line = head.lines().next().unwrap_or_default();
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let (route, query) = target.split_once('?').unwrap_or((target, ""));
    let query: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let matched =
        method == "GET" && route == path && query.get("state").map(String::as_str) == Some(state);
    let (status, body) = if matched {
        ("200 OK", page.html())
    } else {
        ("404 Not Found", String::new())
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
    matched.then_some(query)
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    async fn get(port: u16, target: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[test]
    fn pkce_challenge_is_the_s256_of_the_verifier() {
        let (verifier, challenge) = pkce();
        assert!(verifier.len() >= 43);
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, expected);
        assert_ne!(pkce().0, verifier, "fresh every time");
    }

    #[tokio::test]
    async fn the_listener_ignores_strays_and_returns_the_matching_callback() {
        let loopback = Loopback::bind().await.unwrap();
        let port = loopback.port();
        let page = DonePage {
            title: "Signed <in>".into(),
            body: "Close this tab".into(),
        };
        let (_tx, mut cancel) = oneshot::channel();
        let browser = tokio::spawn(async move {
            let stray = get(port, "/favicon.ico").await;
            let forged = get(port, "/auth/callback?code=evil&state=wrong").await;
            let real = get(port, "/auth/callback?code=abc&state=s1&client_id=oaiapp_1").await;
            (stray, forged, real)
        });
        let query = loopback
            .wait(
                "/auth/callback",
                "s1",
                &page,
                &mut cancel,
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert_eq!(query["code"], "abc");
        assert_eq!(query["client_id"], "oaiapp_1");
        let (stray, forged, real) = browser.await.unwrap();
        assert!(stray.starts_with("HTTP/1.1 404"));
        assert!(forged.starts_with("HTTP/1.1 404"));
        assert!(real.starts_with("HTTP/1.1 200"));
        assert!(real.contains("Signed &lt;in&gt;"), "page text is escaped");
    }

    #[tokio::test]
    async fn waiting_can_be_cancelled_or_time_out() {
        let loopback = Loopback::bind().await.unwrap();
        let (tx, mut cancel) = oneshot::channel();
        tx.send(()).unwrap();
        let err = loopback
            .wait(
                "/cb",
                "s",
                &DonePage::default(),
                &mut cancel,
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
        assert_eq!(err, SignInError::Cancelled);
        let (_tx, mut cancel) = oneshot::channel();
        let err = loopback
            .wait(
                "/cb",
                "s",
                &DonePage::default(),
                &mut cancel,
                Duration::from_millis(50),
            )
            .await
            .unwrap_err();
        assert_eq!(err, SignInError::Timeout);
    }
}
