//! Plain HTTP(S) content fetcher.
//!
//! Fetches raw bytes from a public URL with a hard size cap. Used to pull
//! seeded / imported media into the local iroh store on first access, after
//! which the content is addressed and served purely by its BLAKE3 hash.
//!
//! Read-only: it fetches content but never pins or uploads.

use std::time::Duration;

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
        size: usize,
        max_bytes: usize,
    },
}

const MAX_FETCH_BYTES: usize = 64 * 1024 * 1024;

/// How many redirects a fetch may follow. Each one is re-checked against the
/// SSRF guard, so this is a loop bound rather than a security control.
const MAX_REDIRECTS: usize = 5;

/// HTTP client that fetches content bytes from a public URL.
#[derive(Clone)]
pub struct HttpClient {
    http: reqwest::Client,
}

impl HttpClient {
    /// Create a new HTTP client with the given per-request timeout.
    pub fn new(timeout: Duration) -> Result<Self, HttpError> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            // Redirects are checked, not followed blindly. `resolver`'s SSRF
            // guard runs on the URL the caller supplied; reqwest's default is
            // to follow up to ten hops, so a public URL answering `302
            // Location: http://169.254.169.254/` would reach cloud metadata
            // without the guard ever seeing that address. Every hop is put
            // back through the same check.
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    return attempt.error("too many redirects");
                }
                match crate::content_store::resolver::url_is_publicly_routable(
                    attempt.url().as_str(),
                ) {
                    Ok(()) => attempt.follow(),
                    Err(e) => attempt.error(e),
                }
            }))
            .build()
            .map_err(|e| HttpError::Http(e.to_string()))?;

        Ok(Self { http })
    }

    /// Create an HTTP client with the default 30s timeout.
    pub fn with_defaults() -> Result<Self, HttpError> {
        Self::new(Duration::from_secs(30))
    }

    /// Fetch raw bytes from an HTTP(S) URL, capped at [`MAX_FETCH_BYTES`].
    pub async fn fetch_by_url(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        let response = self
            .http
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
            if content_length > MAX_FETCH_BYTES as u64 {
                return Err(HttpError::TooLarge {
                    url: url.to_string(),
                    size: content_length as usize,
                    max_bytes: MAX_FETCH_BYTES,
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
            if new_len > MAX_FETCH_BYTES {
                return Err(HttpError::TooLarge {
                    url: url.to_string(),
                    size: new_len,
                    max_bytes: MAX_FETCH_BYTES,
                });
            }
            bytes.extend_from_slice(&chunk);
        }

        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_creates_with_defaults() {
        assert!(HttpClient::with_defaults().is_ok());
    }

    #[tokio::test]
    async fn fetch_from_unreachable_url_fails() {
        let client = HttpClient::new(Duration::from_millis(100)).unwrap();
        let result = client.fetch_by_url("http://127.0.0.1:1/nope").await;
        assert!(result.is_err());
    }
}
