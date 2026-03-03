//! IPFS gateway HTTP client.
//!
//! Fetches content from IPFS gateways by CID, with ordered fallback:
//!   1. Blockfrost IPFS gateway (primary — hosts all v1 content)
//!   2. Public IPFS gateways (ipfs.io, dweb.link)
//!
//! The gateway client is read-only: it fetches content but never pins
//! or uploads. Pinning in v2 is local (iroh store + SQLite `pins` table).

use std::time::Duration;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum GatewayError {
    #[error("all gateways failed for CID {cid}: {details}")]
    AllFailed { cid: String, details: String },
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("gateway returned non-success status {status} from {url}")]
    BadStatus { status: u16, url: String },
}

/// Configuration for the IPFS gateway client.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Ordered list of gateway base URLs to try.
    /// Each URL should accept `{base}/{cid}` format.
    /// Example: `https://ipfs.blockfrost.dev/ipfs`
    pub gateways: Vec<String>,
    /// Timeout per gateway attempt.
    pub timeout: Duration,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            gateways: vec![
                "https://ipfs.blockfrost.dev/ipfs".to_string(),
                "https://ipfs.io/ipfs".to_string(),
                "https://dweb.link/ipfs".to_string(),
            ],
            timeout: Duration::from_secs(30),
        }
    }
}

/// HTTP client that fetches IPFS content by CID from gateway endpoints.
pub struct GatewayClient {
    config: GatewayConfig,
    http: reqwest::Client,
}

impl GatewayClient {
    /// Create a new gateway client with the given configuration.
    pub fn new(config: GatewayConfig) -> Result<Self, GatewayError> {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| GatewayError::Http(e.to_string()))?;

        Ok(Self { config, http })
    }

    /// Create a gateway client with default configuration.
    pub fn with_defaults() -> Result<Self, GatewayError> {
        Self::new(GatewayConfig::default())
    }

    /// Fetch content from IPFS gateways by CID.
    ///
    /// Tries each configured gateway in order. Returns the raw bytes
    /// from the first successful response. Collects errors from all
    /// failed attempts for diagnostics.
    pub async fn fetch_by_cid(&self, cid: &str) -> Result<Vec<u8>, GatewayError> {
        let mut errors = Vec::new();

        for gateway in &self.config.gateways {
            let url = format!("{}/{}", gateway, cid);
            log::debug!("gateway: trying {}", url);

            match self.fetch_url(&url).await {
                Ok(bytes) => {
                    log::info!(
                        "gateway: fetched {} bytes for CID {} from {}",
                        bytes.len(),
                        cid,
                        gateway
                    );
                    return Ok(bytes);
                }
                Err(e) => {
                    log::warn!("gateway: {} failed: {}", gateway, e);
                    errors.push(format!("{}: {}", gateway, e));
                }
            }
        }

        Err(GatewayError::AllFailed {
            cid: cid.to_string(),
            details: errors.join("; "),
        })
    }

    /// Fetch content directly from an HTTP(S) URL.
    pub async fn fetch_by_url(&self, url: &str) -> Result<Vec<u8>, GatewayError> {
        self.fetch_url(url).await
    }

    /// Fetch raw bytes from a URL.
    async fn fetch_url(&self, url: &str) -> Result<Vec<u8>, GatewayError> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| GatewayError::Http(e.to_string()))?;

        let status = response.status().as_u16();
        if status < 200 || status >= 300 {
            return Err(GatewayError::BadStatus {
                status,
                url: url.to_string(),
            });
        }

        response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| GatewayError::Http(e.to_string()))
    }

    /// Get the list of configured gateway base URLs.
    pub fn gateways(&self) -> &[String] {
        &self.config.gateways
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_three_gateways() {
        let config = GatewayConfig::default();
        assert_eq!(config.gateways.len(), 3);
        assert!(config.gateways[0].contains("blockfrost"));
        assert!(config.gateways[1].contains("ipfs.io"));
        assert!(config.gateways[2].contains("dweb.link"));
    }

    #[test]
    fn client_creates_with_defaults() {
        let client = GatewayClient::with_defaults();
        assert!(client.is_ok());
        assert_eq!(client.unwrap().gateways().len(), 3);
    }

    #[test]
    fn client_creates_with_custom_config() {
        let config = GatewayConfig {
            gateways: vec!["https://example.com/ipfs".to_string()],
            timeout: Duration::from_secs(5),
        };
        let client = GatewayClient::new(config).unwrap();
        assert_eq!(client.gateways().len(), 1);
    }

    #[tokio::test]
    async fn fetch_nonexistent_cid_fails() {
        // Use a very short timeout to avoid slow tests
        let config = GatewayConfig {
            gateways: vec!["http://127.0.0.1:1".to_string()], // unreachable
            timeout: Duration::from_millis(100),
        };
        let client = GatewayClient::new(config).unwrap();
        let result = client.fetch_by_cid("QmInvalidCid12345678901234567890123456789012").await;
        assert!(result.is_err());
    }
}
