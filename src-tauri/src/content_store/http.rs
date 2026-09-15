//! Plain HTTP(S) content fetcher.
//!
//! Fetches raw bytes from a public URL with a hard size cap. Used to pull
//! seeded / imported media into the local iroh store on first access, after
//! which the content is addressed and served purely by its BLAKE3 hash.
//!
//! Read-only: it fetches content but never pins or uploads.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum HttpError {
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("URL returned non-success status {status} from {url}")]
    BadStatus { status: u16, url: String },
    #[error("response too large from {url}: {size} bytes exceeds {max_bytes}")]
    TooLarge {
        url: String,
        size: u64,
        max_bytes: usize,
    },
}

const MAX_FETCH_BYTES: usize = 64 * 1024 * 1024;

/// How many redirects a fetch may follow. Each one is re-checked against the
/// SSRF guard, so this is a loop bound rather than a security control.
const MAX_REDIRECTS: usize = 5;

#[derive(Debug)]
struct PublicDnsResolver;

impl reqwest::dns::Resolve for PublicDnsResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let resolved = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|error| boxed_dns_error(format!("could not resolve '{host}': {error}")))?;
            let addresses = public_dns_addresses(resolved).map_err(boxed_dns_error)?;
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

fn public_dns_addresses(
    addresses: impl IntoIterator<Item = SocketAddr>,
) -> Result<Vec<SocketAddr>, String> {
    let addresses = addresses.into_iter().collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err("DNS lookup returned no addresses".into());
    }
    for address in &addresses {
        crate::content_store::resolver::reject_if_not_global(address.ip())
            .map_err(|error| error.to_string())?;
    }
    Ok(addresses)
}

fn boxed_dns_error(message: String) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(std::io::Error::other(message))
}

/// HTTP client that fetches content bytes from a public URL.
#[derive(Clone)]
pub struct HttpClient {
    http: reqwest::Client,
    /// Same timeout and connect-time DNS filtering, but never follows a
    /// redirect. A redirect target is a source nobody reviewed, and the
    /// redirect policy hook can only run the synchronous SSRF check, which
    /// would block inside a caller's cancellable time budget.
    exact: reqwest::Client,
}

impl HttpClient {
    /// Create a new HTTP client with the given per-request timeout.
    pub fn new(timeout: Duration) -> Result<Self, HttpError> {
        Self::build(timeout, Arc::new(PublicDnsResolver), false)
    }

    /// Test seam: replace connect-time DNS and ignore proxy settings, so a
    /// test controls every name lookup the client performs.
    #[cfg(test)]
    pub(crate) fn with_dns_resolver<R: reqwest::dns::Resolve + 'static>(
        timeout: Duration,
        resolver: Arc<R>,
    ) -> Result<Self, HttpError> {
        Self::build(timeout, resolver, true)
    }

    fn build<R: reqwest::dns::Resolve + 'static>(
        timeout: Duration,
        resolver: Arc<R>,
        no_proxy: bool,
    ) -> Result<Self, HttpError> {
        let builder = |redirect: reqwest::redirect::Policy| {
            let builder = reqwest::Client::builder()
                .timeout(timeout)
                .dns_resolver(resolver.clone())
                .redirect(redirect);
            let builder = if no_proxy {
                builder.no_proxy()
            } else {
                builder
            };
            builder.build().map_err(|e| HttpError::Http(e.to_string()))
        };

        // Redirects are checked, not followed blindly. `resolver`'s SSRF
        // guard runs on the URL the caller supplied; reqwest's default is
        // to follow up to ten hops, so a public URL answering `302
        // Location: http://169.254.169.254/` would reach cloud metadata
        // without the guard ever seeing that address. Every hop is put
        // back through the same check.
        let http = builder(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }
            match crate::content_store::resolver::url_is_publicly_routable(attempt.url().as_str()) {
                Ok(()) => attempt.follow(),
                Err(e) => attempt.error(e),
            }
        }))?;
        let exact = builder(reqwest::redirect::Policy::none())?;

        Ok(Self { http, exact })
    }

    /// Create an HTTP client with the default 30s timeout.
    pub fn with_defaults() -> Result<Self, HttpError> {
        Self::new(Duration::from_secs(30))
    }

    /// Fetch raw bytes from an HTTP(S) URL, capped at [`MAX_FETCH_BYTES`].
    pub async fn fetch_by_url(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        self.fetch_by_url_with_limit(url, MAX_FETCH_BYTES).await
    }

    /// Fetch raw bytes with a caller-specific cap that can only narrow the
    /// process-wide content limit.
    pub async fn fetch_by_url_with_limit(
        &self,
        url: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, HttpError> {
        fetch_bounded(&self.http, url, max_bytes).await
    }

    /// Fetch exactly `url` with a caller-specific cap. A redirect response is
    /// returned as a non-success status rather than followed.
    pub async fn fetch_exact_url_with_limit(
        &self,
        url: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, HttpError> {
        fetch_bounded(&self.exact, url, max_bytes).await
    }
}

async fn fetch_bounded(
    client: &reqwest::Client,
    url: &str,
    max_bytes: usize,
) -> Result<Vec<u8>, HttpError> {
    let max_bytes = max_bytes.min(MAX_FETCH_BYTES);
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| HttpError::Http(e.to_string()))?;

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(HttpError::BadStatus {
            status,
            url: url.to_string(),
        });
    }
    if let Some(content_length) = response.content_length() {
        if content_length > max_bytes as u64 {
            return Err(HttpError::TooLarge {
                url: url.to_string(),
                size: content_length,
                max_bytes,
            });
        }
    }

    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| HttpError::Http(e.to_string()))?
    {
        let new_len = bytes.len().saturating_add(chunk.len());
        if new_len > max_bytes {
            return Err(HttpError::TooLarge {
                url: url.to_string(),
                size: new_len as u64,
                max_bytes,
            });
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn serve_once(response: Vec<u8>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept test request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await.expect("read test request");
            stream
                .write_all(&response)
                .await
                .expect("write test response");
        });
        format!("http://{address}/content")
    }

    #[test]
    fn client_creates_with_defaults() {
        assert!(HttpClient::with_defaults().is_ok());
    }

    #[test]
    fn connector_dns_rejects_empty_private_and_mixed_answers() {
        assert!(public_dns_addresses([]).is_err());
        assert!(public_dns_addresses(["127.0.0.1:0".parse().unwrap()]).is_err());
        assert!(public_dns_addresses([
            "93.184.216.34:0".parse().unwrap(),
            "10.0.0.1:0".parse().unwrap(),
        ])
        .is_err());
        assert_eq!(
            public_dns_addresses([
                "93.184.216.34:0".parse().unwrap(),
                "[2001:4860:4860::8888]:0".parse().unwrap(),
            ])
            .unwrap()
            .len(),
            2
        );
    }

    #[tokio::test]
    async fn connector_resolver_rejects_localhost_before_connecting() {
        let name = "localhost".parse::<reqwest::dns::Name>().unwrap();
        let result = reqwest::dns::Resolve::resolve(&PublicDnsResolver, name).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fetch_from_unreachable_url_fails() {
        let client = HttpClient::new(Duration::from_millis(100)).unwrap();
        let result = client.fetch_by_url("http://127.0.0.1:1/nope").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn exact_fetch_reports_redirects_instead_of_following_them() {
        let url = serve_once(
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
        )
        .await;
        let client = HttpClient::new(Duration::from_secs(2)).expect("client");

        assert!(matches!(
            client.fetch_exact_url_with_limit(&url, 8).await,
            Err(HttpError::BadStatus { status: 302, .. })
        ));
    }

    #[tokio::test]
    async fn caller_limit_rejects_declared_length_before_reading_body() {
        let url = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Length: 999\r\nConnection: close\r\n\r\n".to_vec(),
        )
        .await;
        let client = HttpClient::new(Duration::from_secs(2)).expect("client");

        assert!(matches!(
            client.fetch_by_url_with_limit(&url, 8).await,
            Err(HttpError::TooLarge {
                size: 999,
                max_bytes: 8,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn caller_limit_rejects_chunked_body_without_declared_length() {
        let url = serve_once(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\nabcd\r\n5\r\nefghi\r\n0\r\n\r\n"
                .to_vec(),
        )
        .await;
        let client = HttpClient::new(Duration::from_secs(2)).expect("client");

        assert!(matches!(
            client.fetch_by_url_with_limit(&url, 8).await,
            Err(HttpError::TooLarge {
                size: 9,
                max_bytes: 8,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn caller_limit_accepts_exact_length() {
        let url = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n12345678".to_vec(),
        )
        .await;
        let client = HttpClient::new(Duration::from_secs(2)).expect("client");

        assert_eq!(
            client
                .fetch_by_url_with_limit(&url, 8)
                .await
                .expect("exact limit should pass"),
            b"12345678"
        );
    }
}
