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
