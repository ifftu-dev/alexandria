//! Talking xAPI to a learning record store the learner named.
//!
//! - **Where it is.** [`resolve`] classifies the address the learner typed as
//!   on this device, on a private network or on the public internet, using
//!   the same address rules as model connections
//!   ([`alexandria_ai::endpoint`]). A public store must use HTTPS. Host names
//!   are resolved once and requests are pinned to those addresses; redirects
//!   are never followed. Whether a private or local address may be used at
//!   all is the caller's decision: the learner confirms it.
//! - **Which version.** [`negotiate`] reads the store's `about` resource and
//!   picks the newest of [`XAPI_VERSIONS`] it speaks. Every built statement
//!   is valid in both, so only the header changes.
//! - **Sending.** [`Lrs::post_statements`] posts a batch with the learner's
//!   own credentials (HTTP Basic, as stores issue them).
//!
//! No Tauri and no database: the app's outbox drives this (ADR-0004).

use std::fmt;
use std::time::Duration;

use alexandria_ai::endpoint::{self, Endpoint, Location};
use serde::Deserialize;
use url::Url;

use super::{Statement, XAPI_VERSIONS};

/// The header naming the xAPI version of a request.
pub const VERSION_HEADER: &str = "X-Experience-API-Version";

/// How long one request to a store may take.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Longest part of a store's error body kept for the learner to read.
const DETAIL_LIMIT: usize = 200;

/// Why a store could not be used. The text before the colon is a stable code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LrsError {
    #[error("invalid_address: {0}")]
    InvalidAddress(String),
    #[error("https_required: a store on the public internet must use an https address")]
    HttpsRequired,
    /// Nothing reached the store: no connection, or its name did not resolve.
    #[error("unreachable: {0}")]
    Unreachable(String),
    #[error("timeout: the store did not answer in time")]
    Timeout,
    #[error("unsupported_version: the store speaks xAPI {0}, not 2.0.0 or 1.0.3")]
    UnsupportedVersion(String),
    /// The store answered and said no (bad credentials, a refused statement).
    #[error("refused: the store answered HTTP {status}{detail}")]
    Refused { status: u16, detail: String },
    /// The store answered but is busy or broken; worth trying later.
    #[error("unavailable: the store answered HTTP {status}")]
    Unavailable { status: u16 },
    #[error("invalid_response: {0}")]
    InvalidResponse(String),
}

impl LrsError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidAddress(_) => "invalid_address",
            Self::HttpsRequired => "https_required",
            Self::Unreachable(_) => "unreachable",
            Self::Timeout => "timeout",
            Self::UnsupportedVersion(_) => "unsupported_version",
            Self::Refused { .. } => "refused",
            Self::Unavailable { .. } => "unavailable",
            Self::InvalidResponse(_) => "invalid_response",
        }
    }

    /// True when nothing reached the store, so nothing was sent.
    pub fn is_unreachable(&self) -> bool {
        matches!(self, Self::Unreachable(_))
    }
}

/// Where a store is, as [`resolve`] found it.
#[derive(Debug, Clone)]
pub struct StoreAddress {
    endpoint: Endpoint,
}

impl StoreAddress {
    /// The xAPI base address, ending in `/`.
    pub fn base(&self) -> &Url {
        self.endpoint.base()
    }

    pub fn location(&self) -> Location {
        self.endpoint.location()
    }

    fn client(&self) -> Result<reqwest::Client, LrsError> {
        self.endpoint
            .http_client(Some(TIMEOUT))
            .map_err(|e| LrsError::InvalidAddress(e.to_string()))
    }

    fn join(&self, resource: &str) -> Result<Url, LrsError> {
        self.base()
            .join(resource)
            .map_err(|e| LrsError::InvalidAddress(e.to_string()))
    }
}

/// Classifies and pins the store's address. A public address must be HTTPS;
/// a private-network or on-device address may be HTTP.
pub async fn resolve(raw: &str) -> Result<StoreAddress, LrsError> {
    let url = Url::parse(raw.trim()).map_err(|_| {
        LrsError::InvalidAddress("enter the store's full xAPI address, such as https://…".into())
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(LrsError::InvalidAddress(
            "a store's address starts with https://".into(),
        ));
    }
    let mut refusal = None;
    for location in [Location::Remote, Location::PrivateNetwork, Location::Device] {
        match endpoint::resolve(raw, location).await {
            Ok(endpoint) => return Ok(StoreAddress { endpoint }),
            Err(alexandria_ai::Error::Unreachable) => {
                return Err(LrsError::Unreachable(
                    "the store's name could not be looked up".into(),
                ))
            }
            Err(error) => refusal = Some(error),
        }
    }
    if url.scheme() == "http" {
        return Err(LrsError::HttpsRequired);
    }
    Err(LrsError::InvalidAddress(refusal.map_or_else(
        || "the address cannot be used".into(),
        |e| e.to_string(),
    )))
}

/// The newest version both sides speak, from the versions a store's `about`
/// resource lists.
pub fn choose_version(offered: &[String]) -> Option<&'static str> {
    XAPI_VERSIONS
        .iter()
        .copied()
        .find(|ours| offered.iter().any(|theirs| theirs == ours))
}

/// Reads the store's `about` resource and picks the version to speak.
pub async fn negotiate(address: &StoreAddress) -> Result<&'static str, LrsError> {
    #[derive(Deserialize)]
    struct About {
        version: Vec<String>,
    }
    let response = address
        .client()?
        .get(address.join("about")?)
        .send()
        .await
        .map_err(transport_error)?;
    let response = check(response).await?;
    let about: About = response
        .json()
        .await
        .map_err(|e| LrsError::InvalidResponse(format!("the store's about resource: {e}")))?;
    choose_version(&about.version)
        .ok_or_else(|| LrsError::UnsupportedVersion(about.version.join(", ")))
}

/// The credentials a store issued to this learner.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub secret: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("secret", &"..")
            .finish()
    }
}

/// A store, its version and the learner's credentials for it.
#[derive(Debug, Clone)]
pub struct Lrs {
    client: reqwest::Client,
    address: StoreAddress,
    credentials: Credentials,
    version: String,
}

impl Lrs {
    pub fn new(
        address: StoreAddress,
        credentials: Credentials,
        version: impl Into<String>,
    ) -> Result<Self, LrsError> {
        Ok(Self {
            client: address.client()?,
            address,
            credentials,
            version: version.into(),
        })
    }

    /// Posts `statements` in one request. Statements keep their own ids, so a
    /// repeated batch is the same statements and stores ignore the repeat.
    pub async fn post_statements(&self, statements: &[Statement]) -> Result<(), LrsError> {
        let response = self
            .client
            .post(self.address.join("statements")?)
            .header(VERSION_HEADER, &self.version)
            .basic_auth(&self.credentials.username, Some(&self.credentials.secret))
            .json(statements)
            .send()
            .await
            .map_err(transport_error)?;
        check(response).await.map(|_| ())
    }
}

fn transport_error(error: reqwest::Error) -> LrsError {
    if error.is_timeout() {
        LrsError::Timeout
    } else if error.is_connect() {
        LrsError::Unreachable("could not connect to the store".into())
    } else {
        LrsError::Unreachable(format!("the request did not complete: {error}"))
    }
}

async fn check(response: reqwest::Response) -> Result<reqwest::Response, LrsError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let code = status.as_u16();
    if code == 429 || status.is_server_error() {
        return Err(LrsError::Unavailable { status: code });
    }
    let body = response.text().await.unwrap_or_default();
    let detail: String = body
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(DETAIL_LIMIT)
        .collect();
    Err(LrsError::Refused {
        status: code,
        detail: if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered(versions: &[&str]) -> Vec<String> {
        versions.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn the_newest_shared_version_is_chosen() {
        assert_eq!(choose_version(&offered(&["1.0.3", "2.0.0"])), Some("2.0.0"));
        assert_eq!(
            choose_version(&offered(&["1.0.0", "1.0.1", "1.0.2", "1.0.3"])),
            Some("1.0.3")
        );
        assert_eq!(choose_version(&offered(&["0.95", "1.0.1"])), None);
    }

    #[tokio::test]
    async fn a_public_store_must_use_https() {
        assert_eq!(
            resolve("http://203.0.113.9/xapi").await.unwrap_err(),
            LrsError::HttpsRequired
        );
    }

    #[tokio::test]
    async fn local_and_private_addresses_are_told_apart_from_public_ones() {
        let local = resolve("http://127.0.0.1:8080/xapi").await.unwrap();
        assert_eq!(local.location(), Location::Device);
        assert_eq!(local.base().as_str(), "http://127.0.0.1:8080/xapi/");
        let private = resolve("http://192.168.1.20/xapi/").await.unwrap();
        assert_eq!(private.location(), Location::PrivateNetwork);
        let public = resolve("https://203.0.113.9/xapi").await.unwrap();
        assert_eq!(public.location(), Location::Remote);
    }

    #[tokio::test]
    async fn an_address_with_credentials_in_it_is_refused() {
        assert_eq!(
            resolve("https://user:pw@203.0.113.9/xapi")
                .await
                .unwrap_err()
                .code(),
            "invalid_address"
        );
        assert_eq!(
            resolve("not an address").await.unwrap_err().code(),
            "invalid_address"
        );
    }
}
