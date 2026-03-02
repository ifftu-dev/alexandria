//! Content resolver with fallback chain.
//!
//! Transparently resolves content by either BLAKE3 hash or IPFS CID,
//! with an ordered resolution strategy:
//!
//!   1. **Local iroh store** — check if we already have the content
//!   2. **CID↔BLAKE3 mapping table** — if we know the other identifier,
//!      check local store under that
//!   3. **IPFS gateway** — fetch from Blockfrost / public gateways,
//!      store locally, record the mapping
//!
//! The resolver bridges the gap between iroh's BLAKE3 addressing and
//! IPFS's SHA-256 CIDs. Content fetched from gateways is automatically
//! cached in the local iroh store and mapped for future lookups.

use std::sync::Arc;

use rusqlite::params;
use thiserror::Error;
use std::sync::Mutex;

use crate::db::Database;
use crate::ipfs::cid::{self, ContentId};
use crate::ipfs::content;
use crate::ipfs::gateway::GatewayClient;
use crate::ipfs::node::ContentNode;

#[derive(Error, Debug)]
pub enum ResolveError {
    #[error("content not found: {0}")]
    NotFound(String),
    #[error("invalid identifier: {0}")]
    InvalidId(String),
    #[error("local store error: {0}")]
    Store(String),
    #[error("gateway error: {0}")]
    Gateway(String),
    #[error("database error: {0}")]
    Database(String),
}

/// Where the content was resolved from.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ResolveSource {
    /// Found in the local iroh store.
    Local,
    /// Found via CID↔BLAKE3 mapping + local store.
    MappedLocal,
    /// Fetched from an IPFS gateway and cached locally.
    Gateway,
}

/// Result of resolving content.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolveResult {
    /// The raw content bytes.
    #[serde(skip)]
    pub bytes: Vec<u8>,
    /// BLAKE3 hash of the content (always available after resolution).
    pub blake3_hash: String,
    /// IPFS CID if known (from input or mapping table).
    pub ipfs_cid: Option<String>,
    /// Where the content was resolved from.
    pub source: ResolveSource,
    /// Size in bytes.
    pub size: u64,
}

/// Content resolver with local cache and gateway fallback.
pub struct ContentResolver {
    node: Arc<ContentNode>,
    gateway: GatewayClient,
        db: Arc<Mutex<Database>>,
}

impl ContentResolver {
    /// Create a new resolver.
    pub fn new(
        node: Arc<ContentNode>,
        gateway: GatewayClient,
    db: Arc<Mutex<Database>>,
    ) -> Self {
        Self { node, gateway, db }
    }

    /// Resolve content by any supported identifier (BLAKE3 hex or IPFS CID).
    ///
    /// Resolution chain:
    ///   1. Parse identifier type
    ///   2. Try local iroh store directly
    ///   3. Check CID↔BLAKE3 mapping table, try mapped hash locally
    ///   4. If IPFS CID, fetch from gateway, cache locally, record mapping
    ///   5. If nothing works, return NotFound
    pub async fn resolve(&self, identifier: &str) -> Result<ResolveResult, ResolveError> {
        let content_id = cid::parse_content_id(identifier)
            .map_err(|e| ResolveError::InvalidId(e.to_string()))?;

        match &content_id {
            ContentId::Blake3Hex(hash) => self.resolve_blake3(hash).await,
            ContentId::IpfsCid(cid_str) => self.resolve_cid(cid_str).await,
        }
    }

    /// Resolve by BLAKE3 hash: try local, then check if we have a CID mapping.
    async fn resolve_blake3(&self, hash: &str) -> Result<ResolveResult, ResolveError> {
        // Step 1: Try local store directly
        match content::get_bytes(&self.node, hash).await {
            Ok(bytes) => {
                let size = bytes.len() as u64;
                let ipfs_cid = self.lookup_cid_for_blake3(hash).await;
                return Ok(ResolveResult {
                    bytes,
                    blake3_hash: hash.to_string(),
                    ipfs_cid,
                    source: ResolveSource::Local,
                    size,
                });
            }
            Err(content::ContentError::NotFound(_)) => {
                // Continue to mapping lookup
            }
            Err(e) => return Err(ResolveError::Store(e.to_string())),
        }

        // Step 2: Check if we have a CID mapping for this BLAKE3 hash,
        // and if so, try fetching by that CID from gateways
        if let Some(mapped_cid) = self.lookup_cid_for_blake3(hash).await {
            if let Ok(result) = self.fetch_and_cache(&mapped_cid).await {
                return Ok(result);
            }
        }

        Err(ResolveError::NotFound(format!("blake3:{}", hash)))
    }

    /// Resolve by IPFS CID: check mapping → local, then gateway fallback.
    async fn resolve_cid(&self, cid_str: &str) -> Result<ResolveResult, ResolveError> {
        // Step 1: Check if we have a BLAKE3 mapping for this CID
        if let Some(mapped_hash) = self.lookup_blake3_for_cid(cid_str).await {
            match content::get_bytes(&self.node, &mapped_hash).await {
                Ok(bytes) => {
                    let size = bytes.len() as u64;
                    return Ok(ResolveResult {
                        bytes,
                        blake3_hash: mapped_hash,
                        ipfs_cid: Some(cid_str.to_string()),
                        source: ResolveSource::MappedLocal,
                        size,
                    });
                }
                Err(content::ContentError::NotFound(_)) => {
                    // Mapping exists but content was evicted — re-fetch
                }
                Err(e) => return Err(ResolveError::Store(e.to_string())),
            }
        }

        // Step 2: Fetch from IPFS gateway
        self.fetch_and_cache(cid_str).await
    }

    /// Fetch content from IPFS gateways, store in iroh, record mapping.
    async fn fetch_and_cache(&self, cid_str: &str) -> Result<ResolveResult, ResolveError> {
        let bytes = self
            .gateway
            .fetch_by_cid(cid_str)
            .await
            .map_err(|e| ResolveError::Gateway(e.to_string()))?;

        // Store in iroh
        let add_result = content::add_bytes(&self.node, &bytes)
            .await
            .map_err(|e| ResolveError::Store(e.to_string()))?;

        // Record the CID↔BLAKE3 mapping
        self.save_mapping(cid_str, &add_result.hash, add_result.size)
            .await;

        log::info!(
            "cached CID {} → blake3:{} ({} bytes)",
            cid_str,
            add_result.hash,
            add_result.size
        );

        Ok(ResolveResult {
            bytes,
            blake3_hash: add_result.hash,
            ipfs_cid: Some(cid_str.to_string()),
            source: ResolveSource::Gateway,
            size: add_result.size,
        })
    }

    /// Look up the BLAKE3 hash for a given IPFS CID in the mapping table.
    async fn lookup_blake3_for_cid(&self, cid_str: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.conn()
            .query_row(
                "SELECT blake3_hash FROM content_mappings WHERE ipfs_cid = ?1",
                params![cid_str],
                |row| row.get(0),
            )
            .ok()
    }

    /// Look up the IPFS CID for a given BLAKE3 hash in the mapping table.
    async fn lookup_cid_for_blake3(&self, blake3_hash: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.conn()
            .query_row(
                "SELECT ipfs_cid FROM content_mappings WHERE blake3_hash = ?1",
                params![blake3_hash],
                |row| row.get(0),
            )
            .ok()
    }

    /// Save a CID↔BLAKE3 mapping to the database.
    async fn save_mapping(&self, cid_str: &str, blake3_hash: &str, size: u64) {
        let db = self.db.lock().unwrap();
        if let Err(e) = db.conn().execute(
            "INSERT OR REPLACE INTO content_mappings (ipfs_cid, blake3_hash, size_bytes) VALUES (?1, ?2, ?3)",
            params![cid_str, blake3_hash, size as i64],
        ) {
            log::warn!("failed to save content mapping: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipfs::gateway::GatewayConfig;
    use std::time::Duration;
    use tempfile::TempDir;

    async fn make_resolver() -> (ContentResolver, TempDir) {
        let tmp = TempDir::new().expect("create temp dir");

        // Set up database
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrations");
        let db = Arc::new(Mutex::new(db));

        // Set up iroh node
        let node = Arc::new(ContentNode::new(tmp.path()));
        node.start().await.expect("start node");

        // Use unreachable gateway (tests don't need real HTTP)
        let config = GatewayConfig {
            gateways: vec!["http://127.0.0.1:1/ipfs".to_string()],
            timeout: Duration::from_millis(50),
        };
        let gateway = GatewayClient::new(config).expect("gateway");

        let resolver = ContentResolver::new(node, gateway, db);
        (resolver, tmp)
    }

    #[tokio::test]
    async fn resolve_blake3_from_local_store() {
        let (resolver, _tmp) = make_resolver().await;

        // Add content directly to iroh
        let data = b"test content for resolver";
        let add = content::add_bytes(&resolver.node, data)
            .await
            .expect("add");

        // Resolve by BLAKE3 hash
        let result = resolver.resolve(&add.hash).await.expect("resolve");
        assert_eq!(result.bytes, data);
        assert_eq!(result.blake3_hash, add.hash);
        assert_eq!(result.source, ResolveSource::Local);
        assert_eq!(result.size, data.len() as u64);

        resolver.node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn resolve_cid_returns_not_found_when_no_gateway() {
        let (resolver, _tmp) = make_resolver().await;

        // CID not in mapping table and gateway unreachable
        let result = resolver
            .resolve("QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG")
            .await;
        assert!(result.is_err());

        resolver.node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn resolve_cid_via_mapping_table() {
        let (resolver, _tmp) = make_resolver().await;

        // Add content to iroh
        let data = b"mapped content";
        let add = content::add_bytes(&resolver.node, data)
            .await
            .expect("add");

        // Insert a fake CID→BLAKE3 mapping
        let fake_cid = "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG";
        resolver
            .save_mapping(fake_cid, &add.hash, add.size)
            .await;

        // Resolve by CID should find it via mapping
        let result = resolver.resolve(fake_cid).await.expect("resolve");
        assert_eq!(result.bytes, data);
        assert_eq!(result.blake3_hash, add.hash);
        assert_eq!(result.ipfs_cid.as_deref(), Some(fake_cid));
        assert_eq!(result.source, ResolveSource::MappedLocal);

        resolver.node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn resolve_invalid_identifier() {
        let (resolver, _tmp) = make_resolver().await;

        let result = resolver.resolve("not-a-valid-id").await;
        assert!(matches!(result, Err(ResolveError::InvalidId(_))));

        resolver.node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn mapping_is_bidirectional() {
        let (resolver, _tmp) = make_resolver().await;

        let data = b"bidirectional mapping test";
        let add = content::add_bytes(&resolver.node, data)
            .await
            .expect("add");

        let fake_cid = "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG";
        resolver
            .save_mapping(fake_cid, &add.hash, add.size)
            .await;

        // Lookup CID → BLAKE3
        let hash = resolver.lookup_blake3_for_cid(fake_cid).await;
        assert_eq!(hash.as_deref(), Some(add.hash.as_str()));

        // Lookup BLAKE3 → CID
        let cid = resolver.lookup_cid_for_blake3(&add.hash).await;
        assert_eq!(cid.as_deref(), Some(fake_cid));

        resolver.node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn resolve_blake3_includes_cid_when_mapped() {
        let (resolver, _tmp) = make_resolver().await;

        let data = b"content with mapping";
        let add = content::add_bytes(&resolver.node, data)
            .await
            .expect("add");

        // Without mapping: no CID
        let result = resolver.resolve(&add.hash).await.expect("resolve");
        assert!(result.ipfs_cid.is_none());

        // Add mapping
        let fake_cid = "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG";
        resolver
            .save_mapping(fake_cid, &add.hash, add.size)
            .await;

        // With mapping: CID is included
        let result = resolver.resolve(&add.hash).await.expect("resolve 2");
        assert_eq!(result.ipfs_cid.as_deref(), Some(fake_cid));

        resolver.node.shutdown().await.expect("shutdown");
    }
}
