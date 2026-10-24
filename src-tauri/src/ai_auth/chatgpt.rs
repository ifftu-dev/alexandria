//! Sign in with ChatGPT, using the user's ChatGPT plan for requests.
//!
//! This follows OpenAI's open-source plan-usage flow
//! (developers.openai.com/siwc, "ChatGPT plan usage"): first-time
//! registration with `client_id=dynamic_agent_client`, a stable per-device
//! `ext_agent_host_id`, authorization code with PKCE (S256) and a loopback
//! redirect on `127.0.0.1` at `/auth/callback`, no client secret. The ID
//! token is verified against OpenAI's JWKS, and plan use is accepted only
//! when the grant includes `chatgpt.tokens.use.direct`. Access tokens last
//! an hour and are refreshed with the rotating refresh token; signing out
//! revokes the refresh token.
//!
//! Tokens are kept in the profile's encrypted database (`studio_secrets`),
//! never in logs, URLs, or the webview.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

use super::{
    http_client, jwt, now_secs, open_browser, pkce, random_token, DonePage, Loopback, SignInError,
};

pub const API_BASE: &str = "https://api.openai.com/v1";
pub const MANAGE_USAGE_URL: &str = "https://chatgpt.com/settings/usage";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
const REGISTER_CLIENT: &str = "dynamic_agent_client";
const APP_NAME: &str = "Alexandria";
const CALLBACK_PATH: &str = "/auth/callback";

/// Refresh this many seconds before the access token expires.
pub const REFRESH_MARGIN: i64 = 120;

/// OpenAI's sign-in endpoints. Tests point these at local servers.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    pub jwks: String,
    pub discovery: String,
    pub issuer: String,
    pub resource: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            authorize: "https://auth.openai.com/api/accounts/authorize".into(),
            token: "https://auth.openai.com/api/accounts/oauth/token".into(),
            jwks: "https://auth.openai.com/.well-known/jwks.json".into(),
            discovery: "https://auth.openai.com/.well-known/openid-configuration".into(),
            issuer: "https://auth.openai.com".into(),
            resource: API_BASE.into(),
        }
    }
}

/// One signed-in ChatGPT account on this device, as stored.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    /// The client ID OpenAI issued at registration (`oaiapp_...`).
    pub client_id: String,
    /// The verified account identity.
    pub sub: String,
    #[serde(default)]
    pub email: Option<String>,
    /// This device's stable host identifier.
    pub host_id: String,
    pub access_token: String,
    pub refresh_token: String,
    /// Retained for `id_token_hint` on re-authorization.
    pub id_token: String,
    /// Unix seconds when the access token expires.
    pub expires_at: i64,
    pub scopes: Vec<String>,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("client_id", &self.client_id)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Credential {
    pub fn parse(stored: &str) -> Result<Self, SignInError> {
        serde_json::from_str(stored)
            .map_err(|_| SignInError::Failed("the saved sign-in is unreadable".into()))
    }

    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("credential serializes")
    }

    pub fn needs_refresh(&self, now: i64) -> bool {
        self.expires_at - now <= REFRESH_MARGIN
    }
}

/// A new stable host identifier for this device.
pub fn new_host_id() -> String {
    format!("urn:uuid:{}", uuid::Uuid::new_v4())
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

async fn post_form(
    client: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
) -> Result<Result<TokenResponse, String>, SignInError> {
    let response = client
        .post(url)
        .form(form)
        .send()
        .await
        .map_err(|_| SignInError::Unreachable)?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        return serde_json::from_value(body)
            .map(Ok)
            .map_err(|_| SignInError::Failed("the provider answered unexpectedly".into()));
    }
    if status.is_server_error() {
        return Err(SignInError::Unreachable);
    }
    let code = body
        .get("error")
        .and_then(|e| e.as_str().or_else(|| e.get("code").and_then(Value::as_str)))
        .unwrap_or("error")
        .to_owned();
    Ok(Err(code))
}

/// Runs the browser sign-in. `existing` re-authorizes a saved account on this
/// device; otherwise a new registration is made.
pub async fn sign_in(
    endpoints: &Endpoints,
    existing: Option<&Credential>,
    host_id: &str,
    page: &DonePage,
    cancel: &mut oneshot::Receiver<()>,
    open: impl FnOnce(&str) -> Result<(), SignInError>,
) -> Result<Credential, SignInError> {
    let loopback = Loopback::bind().await?;
    let redirect_uri = format!("http://127.0.0.1:{}{CALLBACK_PATH}", loopback.port());
    let (verifier, challenge) = pkce();
    let state = random_token(24);
    let nonce = random_token(24);
    let mut url = url::Url::parse(&endpoints.authorize)
        .map_err(|_| SignInError::Failed("bad sign-in address".into()))?;
    {
        let mut q = url.query_pairs_mut();
        match existing {
            Some(saved) => {
                q.append_pair("client_id", &saved.client_id);
                q.append_pair("id_token_hint", &saved.id_token);
            }
            None => {
                q.append_pair("client_id", REGISTER_CLIENT);
                q.append_pair("agent_name_hint", APP_NAME);
            }
        }
        q.append_pair("ext_agent_host_id", host_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", SCOPES)
            .append_pair("resource", &endpoints.resource)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &challenge);
    }
    open(url.as_str())?;
    let query = loopback
        .wait(CALLBACK_PATH, &state, page, cancel, super::SIGN_IN_TIMEOUT)
        .await?;
    if query.contains_key("error") {
        return Err(SignInError::Declined);
    }
    let code = query.get("code").ok_or(SignInError::Declined)?;
    let client_id = match (existing, query.get("client_id")) {
        (Some(saved), Some(issued)) if issued != &saved.client_id => {
            return Err(SignInError::DifferentAccount)
        }
        (Some(saved), _) => saved.client_id.clone(),
        (None, Some(issued)) if issued != REGISTER_CLIENT && !issued.is_empty() => issued.clone(),
        (None, _) => return Err(SignInError::Failed("no client was registered".into())),
    };

    let client = http_client()?;
    let tokens = post_form(
        &client,
        &endpoints.token,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", &client_id),
            ("code", code),
            ("code_verifier", &verifier),
            ("redirect_uri", &redirect_uri),
            ("resource", &endpoints.resource),
        ],
    )
    .await?
    .map_err(|_| SignInError::Failed("the sign-in could not be completed; try again".into()))?;

    let id_token = tokens
        .id_token
        .ok_or_else(|| SignInError::Failed("no identity was returned".into()))?;
    let jwks: Value = client
        .get(&endpoints.jwks)
        .send()
        .await
        .map_err(|_| SignInError::Unreachable)?
        .json()
        .await
        .map_err(|_| SignInError::Unreachable)?;
    let identity = jwt::verify(
        &id_token,
        &jwks,
        &endpoints.issuer,
        &client_id,
        &nonce,
        now_secs(),
    )
    .map_err(SignInError::Failed)?;
    if existing.is_some_and(|saved| saved.sub != identity.sub) {
        return Err(SignInError::DifferentAccount);
    }
    let scopes: Vec<String> = tokens
        .scope
        .as_deref()
        .or(query.get("scope").map(String::as_str))
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if !scopes.iter().any(|s| s == PLAN_SCOPE) {
        return Err(SignInError::PlanNotGranted);
    }
    let refresh_token = tokens
        .refresh_token
        .ok_or_else(|| SignInError::Failed("no refresh token was returned".into()))?;
    Ok(Credential {
        client_id,
        sub: identity.sub,
        email: identity.email,
        host_id: host_id.to_owned(),
        access_token: tokens.access_token,
        refresh_token,
        id_token,
        expires_at: now_secs() + tokens.expires_in.unwrap_or(3600),
        scopes,
    })
}

/// Why a refresh failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshError {
    /// The refresh token is no longer usable; the user must sign in again.
    SignInAgain,
    /// A temporary failure; keep the credential.
    Temporary,
}

/// Exchanges the refresh token for a new access token. The rotating refresh
/// token, expiry, and scopes are replaced together.
pub async fn refresh(
    endpoints: &Endpoints,
    saved: &Credential,
) -> Result<Credential, RefreshError> {
    let client = http_client().map_err(|_| RefreshError::Temporary)?;
    let result = post_form(
        &client,
        &endpoints.token,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &saved.client_id),
            ("refresh_token", &saved.refresh_token),
            ("resource", &endpoints.resource),
        ],
    )
    .await
    .map_err(|_| RefreshError::Temporary)?;
    let tokens = result.map_err(|code| match code.as_str() {
        "invalid_grant"
        | "invalid_refresh_token"
        | "token_expired"
        | "refresh_token_expired"
        | "refresh_token_invalidated"
        | "refresh_token_reused"
        | "invalid_client" => RefreshError::SignInAgain,
        _ => RefreshError::Temporary,
    })?;
    let mut next = saved.clone();
    next.access_token = tokens.access_token;
    if let Some(rotated) = tokens.refresh_token {
        next.refresh_token = rotated;
    }
    if let Some(id_token) = tokens.id_token {
        next.id_token = id_token;
    }
    if let Some(scope) = tokens.scope {
        next.scopes = scope.split_whitespace().map(str::to_owned).collect();
    }
    next.expires_at = now_secs() + tokens.expires_in.unwrap_or(3600);
    if !next.scopes.iter().any(|s| s == PLAN_SCOPE) {
        return Err(RefreshError::SignInAgain);
    }
    Ok(next)
}

/// Revokes the refresh token. Best effort: returns whether OpenAI confirmed.
pub async fn revoke(endpoints: &Endpoints, saved: &Credential) -> bool {
    let Ok(client) = http_client() else {
        return false;
    };
    let Ok(discovery) = client.get(&endpoints.discovery).send().await else {
        return false;
    };
    let Ok(discovery) = discovery.json::<Value>().await else {
        return false;
    };
    let Some(url) = discovery.get("revocation_endpoint").and_then(Value::as_str) else {
        return false;
    };
    client
        .post(url)
        .form(&[
            ("token", saved.refresh_token.as_str()),
            ("token_type_hint", "refresh_token"),
            ("client_id", saved.client_id.as_str()),
        ])
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
}

/// Opens the page where the user manages ChatGPT plan usage.
pub fn open_manage_usage() -> Result<(), SignInError> {
    open_browser(MANAGE_USAGE_URL)
}

#[cfg(test)]
mod tests {
    use super::super::jwt::test_key;
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A fake OpenAI auth server: token exchange, refresh, JWKS, discovery.
    async fn auth_server(
        scope: &'static str,
        sub: &'static str,
        seen: Arc<Mutex<Vec<String>>>,
    ) -> (Endpoints, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let nonce = Arc::new(Mutex::new(String::new()));
        let nonce_for_server = nonce.clone();
        let issuer = base.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = vec![0u8; 16384];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                seen.lock().unwrap().push(req.clone());
                let line = req.lines().next().unwrap_or_default().to_owned();
                let body = if line.starts_with("GET /jwks") {
                    test_key::jwks().to_string()
                } else if line.starts_with("GET /discovery") {
                    json!({"revocation_endpoint": format!("{issuer}/revoke")}).to_string()
                } else if line.starts_with("POST /revoke") {
                    String::new()
                } else if req.contains("grant_type=refresh_token") {
                    if req.contains("refresh_token=dead") {
                        let resp = "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 25\r\nConnection: close\r\n\r\n{\"error\":\"invalid_grant\"}";
                        let _ = s.write_all(resp.as_bytes()).await;
                        continue;
                    }
                    json!({"access_token": "at-2", "refresh_token": "rt-2", "expires_in": 3600, "scope": scope}).to_string()
                } else {
                    let id_token = test_key::sign(&json!({
                        "iss": issuer, "aud": "oaiapp_9", "exp": now_secs() + 600, "iat": now_secs(),
                        "nonce": nonce_for_server.lock().unwrap().clone(), "sub": sub, "email": "a@example.com"
                    }));
                    json!({"access_token": "at-1", "refresh_token": "rt-1", "id_token": id_token,
                           "expires_in": 3600, "scope": scope, "token_type": "Bearer"})
                    .to_string()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });
        (
            Endpoints {
                authorize: format!("{base}/authorize"),
                token: format!("{base}/token"),
                jwks: format!("{base}/jwks"),
                discovery: format!("{base}/discovery"),
                issuer: base.clone(),
                resource: API_BASE.into(),
            },
            nonce,
        )
    }

    /// Plays the browser: reads the authorize URL, then calls the loopback
    /// redirect with a code and the issued client.
    fn browser(
        nonce: Arc<Mutex<String>>,
        opened: Arc<Mutex<String>>,
        callback_extra: &'static str,
    ) -> impl FnOnce(&str) -> Result<(), SignInError> {
        move |url: &str| {
            *opened.lock().unwrap() = url.to_owned();
            let parsed = url::Url::parse(url).unwrap();
            let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
            *nonce.lock().unwrap() = q["nonce"].clone();
            let redirect = format!(
                "{}?code=c1&state={}{callback_extra}",
                q["redirect_uri"], q["state"]
            );
            tokio::spawn(async move {
                let target = url::Url::parse(&redirect).unwrap();
                let mut s = tokio::net::TcpStream::connect(("127.0.0.1", target.port().unwrap()))
                    .await
                    .unwrap();
                let path = format!("{}?{}", target.path(), target.query().unwrap());
                s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                let mut out = String::new();
                let _ = s.read_to_string(&mut out).await;
            });
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_first_sign_in_registers_verifies_and_requires_the_plan_scope() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (endpoints, nonce) = auth_server(SCOPES, "user-1", seen.clone()).await;
        let opened = Arc::new(Mutex::new(String::new()));
        let (_tx, mut cancel) = oneshot::channel();
        let cred = sign_in(
            &endpoints,
            None,
            "urn:uuid:host",
            &DonePage::default(),
            &mut cancel,
            browser(nonce, opened.clone(), "&client_id=oaiapp_9"),
        )
        .await
        .unwrap();
        assert_eq!(
            (cred.client_id.as_str(), cred.sub.as_str()),
            ("oaiapp_9", "user-1")
        );
        assert_eq!(
            (cred.access_token.as_str(), cred.refresh_token.as_str()),
            ("at-1", "rt-1")
        );
        let url = opened.lock().unwrap().clone();
        for part in [
            "client_id=dynamic_agent_client",
            "agent_name_hint=Alexandria",
            "ext_agent_host_id=urn%3Auuid%3Ahost",
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A",
            "%2Fauth%2Fcallback",
            "code_challenge_method=S256",
            "chatgpt.tokens.use.direct",
            "resource=https%3A%2F%2Fapi.openai.com%2Fv1",
        ] {
            assert!(url.contains(part), "{part} in {url}");
        }
        let exchange = seen
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.contains("grant_type=authorization_code"))
            .cloned()
            .unwrap();
        assert!(exchange.contains("client_id=oaiapp_9") && exchange.contains("code_verifier="));
        assert!(!exchange.contains("client_secret"));

        // Refresh rotates the token; a dead refresh token means signing in again.
        let next = refresh(&endpoints, &cred).await.unwrap();
        assert_eq!(
            (next.access_token.as_str(), next.refresh_token.as_str()),
            ("at-2", "rt-2")
        );
        let mut dead = cred.clone();
        dead.refresh_token = "dead".into();
        assert_eq!(
            refresh(&endpoints, &dead).await.unwrap_err(),
            RefreshError::SignInAgain
        );
        assert!(revoke(&endpoints, &cred).await);
    }

    #[tokio::test]
    async fn declined_plan_use_and_another_account_are_refused() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (endpoints, nonce) =
            auth_server("openid profile email offline_access", "user-1", seen).await;
        let (_tx, mut cancel) = oneshot::channel();
        let err = sign_in(
            &endpoints,
            None,
            "h",
            &DonePage::default(),
            &mut cancel,
            browser(nonce, Arc::default(), "&client_id=oaiapp_9"),
        )
        .await
        .unwrap_err();
        assert_eq!(err, SignInError::PlanNotGranted);

        let seen = Arc::new(Mutex::new(Vec::new()));
        let (endpoints, nonce) = auth_server(SCOPES, "user-2", seen).await;
        let saved = Credential {
            client_id: "oaiapp_9".into(),
            sub: "user-1".into(),
            email: None,
            host_id: "h".into(),
            access_token: "a".into(),
            refresh_token: "r".into(),
            id_token: "i".into(),
            expires_at: 0,
            scopes: vec![],
        };
        let (_tx, mut cancel) = oneshot::channel();
        let err = sign_in(
            &endpoints,
            Some(&saved),
            "h",
            &DonePage::default(),
            &mut cancel,
            browser(nonce, Arc::default(), ""),
        )
        .await
        .unwrap_err();
        assert_eq!(err, SignInError::DifferentAccount);

        let (_tx, mut cancel) = oneshot::channel();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (endpoints, nonce) = auth_server(SCOPES, "user-1", seen).await;
        let err = sign_in(
            &endpoints,
            None,
            "h",
            &DonePage::default(),
            &mut cancel,
            browser(nonce, Arc::default(), "&error=access_denied"),
        )
        .await
        .unwrap_err();
        assert_eq!(err, SignInError::Declined);
    }

    #[test]
    fn debug_output_never_shows_tokens() {
        let cred = Credential {
            client_id: "oaiapp_9".into(),
            sub: "s".into(),
            email: None,
            host_id: "h".into(),
            access_token: "secret-at".into(),
            refresh_token: "secret-rt".into(),
            id_token: "secret-id".into(),
            expires_at: 10,
            scopes: vec![],
        };
        let printed = format!("{cred:?}");
        assert!(!printed.contains("secret"));
        assert!(cred.needs_refresh(10 - REFRESH_MARGIN + 1));
        assert!(!cred.needs_refresh(10 - REFRESH_MARGIN - 1));
        assert_eq!(Credential::parse(&cred.encode()).unwrap(), cred);
    }
}
