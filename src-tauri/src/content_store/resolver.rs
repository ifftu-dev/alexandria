//! Content resolver with fallback chain.
//!
//! Resolves content by either BLAKE3 hash or public URL, with an ordered
//! resolution strategy:
//!
//!   1. **Local iroh store** — check if we already have the content
//!   2. **iroh peer fetch** — pull from known providers (pinners) over iroh
//!   3. **External URL mapping** — if we know a public URL for the content,
//!      re-fetch it over HTTP, store locally, record the mapping
//!
//! Content is addressed and verified end to end by its BLAKE3 hash. Public
//! URLs are only an origin of last resort for seeded / imported media; once
//! fetched, the content lives in the iroh store and is served by peers.

use std::sync::Arc;

use rusqlite::params;
use std::sync::Mutex;
use thiserror::Error;

use crate::content_store::cid::{self, is_http_url, ContentId};
use crate::content_store::content;
use crate::content_store::fetch;
use crate::content_store::http::HttpClient;
use crate::content_store::node::ContentNode;
use crate::db::Database;

#[derive(Error, Debug)]
pub enum ResolveError {
    #[error("content not found: {0}")]
    NotFound(String),
    #[error("invalid identifier: {0}")]
    InvalidId(String),
    #[error("local store error: {0}")]
    Store(String),
    #[error("fetch error: {0}")]
    Fetch(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("blocked URL: {0}")]
    BlockedUrl(String),
}

/// Reject URLs that would reach a private, loopback, link-local or otherwise
/// non-public address (SSRF defence).
///
/// The previous version split the URL string by hand and only rejected a host
/// that either literally read `localhost`/`0.0.0.0` or parsed as a private
/// `IpAddr`. Four standard bypasses walked through it:
///
/// - **Userinfo** — `http://example.com@127.0.0.1/` produced a "host" of
///   `example.com@127.0.0.1`, which is neither a blocked name nor a parseable
///   address, so it passed and reqwest connected to loopback.
/// - **Integer literals** — `http://2130706433/` is loopback to every resolver
///   but is not parseable as an `IpAddr`, so it passed.
/// - **Names that resolve privately** — `127.0.0.1.nip.io`,
///   `metadata.google.internal`, or any A record the attacker controls. The
///   check never resolved anything.
/// - **IPv4-mapped IPv6** — `[::ffff:127.0.0.1]` parses as V6, and
///   `Ipv6Addr::is_loopback()` is false for the mapped form.
///
/// So: parse with a real URL parser, refuse userinfo outright, then resolve
/// the host and check *every* address it resolves to. Resolution is what makes
/// this a real check rather than a syntactic one — but it also means a DNS
/// answer can change between here and the connection (a rebinding attack), so
/// the client is additionally pinned to a redirect policy that re-runs this on
/// every hop. See [`crate::content_store::http`].
///
/// Exposed as `url_is_publicly_routable` so the redirect policy can call it
/// with a plain `String` error, which is what `reqwest`'s policy hook wants.
pub fn url_is_publicly_routable(url: &str) -> Result<(), String> {
    reject_private_url(url).map_err(|e| e.to_string())
}

fn reject_private_url(url: &str) -> Result<(), ResolveError> {
    let parsed = parse_fetchable_url(url)?;
    let Some(name) = reject_private_host_without_dns(&parsed)? else {
        return Ok(());
    };

    // Resolve and check every address. `to_socket_addrs` needs a port;
    // the scheme's default is fine because the port does not affect
    // which addresses a name resolves to.
    //
    // This lookup blocks the calling thread and cannot be cancelled by a
    // `tokio::time::timeout`. Time-budgeted callers use
    // [`reject_unreviewable_https_source`] instead and rely on the client's
    // connect-time resolver, which applies the same address check.
    use std::net::ToSocketAddrs;
    let port = parsed.port_or_known_default().unwrap_or(80);
    let resolved = (name, port)
        .to_socket_addrs()
        .map_err(|e| ResolveError::BlockedUrl(format!("could not resolve '{name}': {e}")))?;

    let mut any = false;
    for addr in resolved {
        any = true;
        reject_if_not_global(addr.ip())?;
    }
    if !any {
        return Err(ResolveError::BlockedUrl(format!(
            "'{name}' resolved to no addresses"
        )));
    }
    Ok(())
}

fn parse_fetchable_url(url: &str) -> Result<url::Url, ResolveError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| ResolveError::BlockedUrl(format!("unparseable URL: {e}")))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(ResolveError::BlockedUrl(format!(
                "scheme '{other}' is not fetchable"
            )))
        }
    }

    // Userinfo has no legitimate use here and is the oldest way to make a URL
    // read as one host and connect to another.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ResolveError::BlockedUrl(
            "URLs with embedded credentials are refused".into(),
        ));
    }
    Ok(parsed)
}

/// Every host check that needs no DNS. Returns the domain name that still has
/// to be resolved, or `None` for an acceptable IP literal.
fn reject_private_host_without_dns(parsed: &url::Url) -> Result<Option<&str>, ResolveError> {
    let host = parsed
        .host()
        .ok_or_else(|| ResolveError::BlockedUrl("URL has no host".into()))?;

    match host {
        url::Host::Ipv4(v4) => reject_if_not_global(std::net::IpAddr::V4(v4)).map(|()| None),
        url::Host::Ipv6(v6) => reject_if_not_global(std::net::IpAddr::V6(v6)).map(|()| None),
        url::Host::Domain(name) => {
            // A name that spells out loopback is refused before the resolver
            // is consulted, so a hostile DNS answer is not needed to catch it.
            let lowered = name.to_ascii_lowercase();
            if lowered == "localhost" || lowered.ends_with(".localhost") {
                return Err(ResolveError::BlockedUrl(
                    "URL points to loopback address".into(),
                ));
            }
            Ok(Some(name))
        }
    }
}

/// Names that never identify a public origin: single-label hosts and
/// special-use or private-use suffixes (RFC 6761, 6762, 7686, 8375, 9476, and
/// names ICANN withholds because they collide with private networks).
pub(crate) fn is_special_use_domain(name: &str) -> bool {
    const SPECIAL_USE_SUFFIXES: &[&str] = &[
        "localhost",
        "local",
        "internal",
        "intranet",
        "home.arpa",
        "arpa",
        "onion",
        "invalid",
        "test",
        "example",
        "alt",
        "lan",
        "home",
        "corp",
    ];
    let name = name.to_ascii_lowercase();
    !name.contains('.')
        || SPECIAL_USE_SUFFIXES.iter().any(|suffix| {
            name == *suffix
                || name
                    .strip_suffix(suffix)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
}

/// Decide, without DNS, whether `parsed` may be an explicitly reviewed
/// content source such as a governance genesis mirror.
///
/// Reviewed sources must be HTTPS on the default port, carry no userinfo, and
/// name a public DNS host: IP literals, trailing-dot hosts, and special-use
/// names are refused so every source has one reviewable spelling. The check
/// is synchronous but never blocks; the HTTP client's connect-time resolver
/// rejects any non-public address the name resolves to.
pub(crate) fn reject_unreviewable_https_source(parsed: &url::Url) -> Result<(), ResolveError> {
    if parsed.scheme() != "https" {
        return Err(ResolveError::BlockedUrl(
            "reviewed sources must use HTTPS".into(),
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ResolveError::BlockedUrl(
            "URLs with embedded credentials are refused".into(),
        ));
    }
    if parsed.port().is_some() {
        return Err(ResolveError::BlockedUrl(
            "reviewed sources must use the default HTTPS port".into(),
        ));
    }
    match reject_private_host_without_dns(parsed)? {
        None => Err(ResolveError::BlockedUrl(
            "reviewed sources must name a DNS host, not an IP address".into(),
        )),
        Some(name) if name.ends_with('.') || is_special_use_domain(name) => Err(
            ResolveError::BlockedUrl(format!("'{name}' is not a public DNS name")),
        ),
        Some(_) => Ok(()),
    }
}

/// Refuse anything that is not a globally routable unicast address.
///
/// An allowlist of "is this public" rather than a blocklist of known-bad
/// ranges: the blocklist shape is what let `2130706433` and `::ffff:127.0.0.1`
/// through, because both are only bad once you have normalised them.
pub(crate) fn reject_if_not_global(ip: std::net::IpAddr) -> Result<(), ResolveError> {
    // Normalise IPv4-*mapped* IPv6 down to its v4 form, so
    // `::ffff:127.0.0.1` is judged as 127.0.0.1.
    //
    // Deliberately not `to_ipv4()`, which also converts the deprecated
    // IPv4-*compatible* form `::a.b.c.d` — and `::1` is `::0.0.0.1` under that
    // reading, so loopback would normalise to the perfectly routable 0.0.0.1
    // and be allowed. The v6 predicates below judge `::1` correctly, so the
    // compatible form is left alone.
    //
    // Transition prefixes that a gateway translates to an embedded IPv4
    // address are judged by that address too: on an IPv6-only network with
    // DNS64/NAT64, `64:ff9b::c0a8:101` reaches 192.168.1.1.
    let ip = match ip {
        std::net::IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => std::net::IpAddr::V4(v4),
            None => {
                let s = v6.segments();
                let low32 = std::net::Ipv4Addr::new(
                    (s[6] >> 8) as u8,
                    s[6] as u8,
                    (s[7] >> 8) as u8,
                    s[7] as u8,
                );
                match s {
                    // NAT64 well-known prefix (RFC 6052), 64:ff9b::/96.
                    [0x64, 0xff9b, 0, 0, 0, 0, _, _] => std::net::IpAddr::V4(low32),
                    // 6to4 (RFC 3056), 2002:AABB:CCDD::/48.
                    [0x2002, hi, lo, ..] => std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                        (hi >> 8) as u8,
                        hi as u8,
                        (lo >> 8) as u8,
                        lo as u8,
                    )),
                    _ => std::net::IpAddr::V6(v6),
                }
            }
        },
        v4 => v4,
    };

    let blocked = match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                // "This network", 0.0.0.0/8 (not only 0.0.0.0).
                || v4.octets()[0] == 0
                || v4.is_multicast()
                || v4.is_documentation()
                // IETF protocol assignments (RFC 6890), 192.0.0.0/24.
                || (v4.octets()[..3] == [192, 0, 0])
                // Shared address space (RFC 6598, carrier-grade NAT).
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
                // Benchmarking (RFC 2544).
                || (v4.octets()[0] == 198 && (v4.octets()[1] & 0xfe) == 18)
                // Reserved / 240.0.0.0/4.
                || v4.octets()[0] >= 240
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // Unique local (fc00::/7).
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // Link-local unicast (fe80::/10).
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // Deprecated site-local (fec0::/10).
                || (v6.segments()[0] & 0xffc0) == 0xfec0
                // Deprecated IPv4-compatible (::/96) and IPv4-translated
                // (::ffff:0:0/96) forms; `::1` and `::` are caught above.
                || matches!(v6.segments(), [0, 0, 0, 0, 0, 0, _, _] | [0, 0, 0, 0, 0xffff, 0, _, _])
                // Local-use NAT64 (RFC 8215), 64:ff9b:1::/48.
                || matches!(v6.segments(), [0x64, 0xff9b, 1, ..])
                // Teredo (2001::/32) tunnels to an embedded, obfuscated IPv4
                // client address.
                || matches!(v6.segments(), [0x2001, 0, ..])
                // Documentation (2001:db8::/32).
                || matches!(v6.segments(), [0x2001, 0x0db8, ..])
                // Discard-only (100::/64).
                || matches!(v6.segments(), [0x0100, 0, 0, 0, ..])
        }
    };

    if blocked {
        return Err(ResolveError::BlockedUrl(format!(
            "URL resolves to the non-public address {ip}"
        )));
    }
    Ok(())
}

/// Where the content was resolved from.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ResolveSource {
    /// Found in the local iroh store.
    Local,
    /// Found via external-URL↔BLAKE3 mapping + local store.
    MappedLocal,
    /// Fetched from a public URL and cached locally.
    Url,
}

/// Result of resolving content.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolveResult {
    /// The raw content bytes.
    #[serde(skip)]
    pub bytes: Vec<u8>,
    /// BLAKE3 hash of the content (always available after resolution).
    pub blake3_hash: String,
    /// Public URL for the content if known (from input or mapping table).
    pub external_id: Option<String>,
    /// Where the content was resolved from.
    pub source: ResolveSource,
    /// Size in bytes.
    pub size: u64,
}

/// Content resolver with local cache, iroh peer fetch, and URL fallback.
#[derive(Clone)]
pub struct ContentResolver {
    node: Arc<ContentNode>,
    http: HttpClient,
    db: Arc<Mutex<Option<Database>>>,
    /// Optional iroh content-provider discovery. When present, a local
    /// cache-miss for a BLAKE3 hash is first served by fetching from known
    /// peers (pinners) over iroh, before falling back to the public URL.
    discovery: Option<Arc<super::discovery::ContentDiscovery>>,
}

impl ContentResolver {
    /// Create a new resolver without peer discovery (URL-only fallback).
    pub fn new(node: Arc<ContentNode>, http: HttpClient, db: Arc<Mutex<Option<Database>>>) -> Self {
        Self {
            node,
            http,
            db,
            discovery: None,
        }
    }

    /// Create a resolver that fetches cache-misses from iroh peers (via
    /// `discovery`) before falling back to the public URL.
    pub fn with_discovery(
        node: Arc<ContentNode>,
        http: HttpClient,
        db: Arc<Mutex<Option<Database>>>,
        discovery: Arc<super::discovery::ContentDiscovery>,
    ) -> Self {
        Self {
            node,
            http,
            db,
            discovery: Some(discovery),
        }
    }

    /// Resolve content by any supported identifier (BLAKE3 hex or public URL).
    ///
    /// Resolution chain:
    ///   1. Parse identifier type
    ///   2. Try local iroh store directly
    ///   3. Fetch from iroh peers if providers are known
    ///   4. If a public URL is known, fetch over HTTP, cache locally, map it
    ///   5. If nothing works, return NotFound
    pub async fn resolve(&self, identifier: &str) -> Result<ResolveResult, ResolveError> {
        let content_id = cid::parse_content_id(identifier)
            .map_err(|e| ResolveError::InvalidId(e.to_string()))?;

        match &content_id {
            ContentId::Blake3Hex(hash) => self.resolve_blake3(hash).await,
            ContentId::Url(url) => self.resolve_url(url).await,
        }
    }

    /// Resolve bytes for an explicit preview without retaining newly fetched
    /// content. Local content may be reused, but peer fetches receive no named
    /// retention tag and URL responses are neither cached nor mapped. Every
    /// allocation/network attempt is bounded by `max_bytes`.
    pub async fn resolve_preview_bounded(
        &self,
        identifier: &str,
        max_bytes: usize,
    ) -> Result<ResolveResult, ResolveError> {
        let content_id = cid::parse_content_id(identifier)
            .map_err(|error| ResolveError::InvalidId(error.to_string()))?;
        match content_id {
            ContentId::Blake3Hex(hash) => {
                self.resolve_blake3_preview_bounded(&hash, max_bytes).await
            }
            ContentId::Url(url) => self.fetch_url_preview_bounded(&url, max_bytes).await,
        }
    }

    async fn resolve_blake3_preview_bounded(
        &self,
        hash: &str,
        max_bytes: usize,
    ) -> Result<ResolveResult, ResolveError> {
        match content::get_bytes_bounded(&self.node, hash, max_bytes).await {
            Ok(bytes) => {
                let size = bytes.len() as u64;
                return Ok(ResolveResult {
                    bytes,
                    blake3_hash: hash.to_string(),
                    external_id: self.lookup_external_for_blake3(hash).await,
                    source: ResolveSource::Local,
                    size,
                });
            }
            Err(content::ContentError::NotFound(_)) => {}
            Err(error) => return Err(ResolveError::Store(error.to_string())),
        }

        if let Some(discovery) = &self.discovery {
            if let Ok(parsed) = content::parse_hash(hash) {
                let providers = discovery.find_providers(parsed).await;
                if !providers.is_empty()
                    && fetch::fetch_from_any_unretained_bounded(
                        &self.node, &providers, parsed, max_bytes,
                    )
                    .await
                    .is_ok()
                {
                    let bytes = content::get_bytes_bounded(&self.node, hash, max_bytes)
                        .await
                        .map_err(|error| ResolveError::Store(error.to_string()))?;
                    let size = bytes.len() as u64;
                    return Ok(ResolveResult {
                        bytes,
                        blake3_hash: hash.to_string(),
                        external_id: self.lookup_external_for_blake3(hash).await,
                        source: ResolveSource::Local,
                        size,
                    });
                }
            }
        }

        // Deliberately no mapped-URL fallback: a preview fetches only the
        // identifiers its caller reviewed, and a mapping learned from other
        // content is neither reviewed nor necessarily HTTPS.
        Err(ResolveError::NotFound(format!("blake3:{hash}")))
    }

    /// Fetch one reviewed HTTPS source inside the caller's time budget.
    ///
    /// No synchronous DNS pre-check runs here: a blocking lookup cannot be
    /// pre-empted by `tokio::time::timeout`. Hostnames are resolved by the
    /// HTTP client's async resolver, which refuses non-public answers at
    /// connect time, and redirects are not followed.
    async fn fetch_url_preview_bounded(
        &self,
        url: &str,
        max_bytes: usize,
    ) -> Result<ResolveResult, ResolveError> {
        let parsed = parse_fetchable_url(url)?;
        reject_unreviewable_https_source(&parsed)?;
        let bytes = self
            .http
            .fetch_exact_url_with_limit(url, max_bytes)
            .await
            .map_err(|error| ResolveError::Fetch(error.to_string()))?;
        let size = bytes.len() as u64;
        Ok(ResolveResult {
            blake3_hash: blake3::hash(&bytes).to_hex().to_string(),
            external_id: Some(url.to_string()),
            source: ResolveSource::Url,
            bytes,
            size,
        })
    }

    /// Resolve by BLAKE3 hash: local store, then peers, then mapped URL.
    async fn resolve_blake3(&self, hash: &str) -> Result<ResolveResult, ResolveError> {
        // Step 1: Try local store directly
        match content::get_bytes(&self.node, hash).await {
            Ok(bytes) => {
                let size = bytes.len() as u64;
                let external_id = self.lookup_external_for_blake3(hash).await;
                return Ok(ResolveResult {
                    bytes,
                    blake3_hash: hash.to_string(),
                    external_id,
                    source: ResolveSource::Local,
                    size,
                });
            }
            Err(content::ContentError::NotFound(_)) => {
                // Continue to peer fetch, then mapping lookup
            }
            Err(e) => return Err(ResolveError::Store(e.to_string())),
        }

        // Step 2: iroh peer fetch. If discovery knows providers for this hash
        // (e.g. PinBoard pinners), pull it directly over iroh before touching
        // the network origin. This is the decentralized path — content served
        // by peers, BLAKE3-verified end to end.
        if let Some(discovery) = &self.discovery {
            if let Ok(parsed) = content::parse_hash(hash) {
                let providers = discovery.find_providers(parsed).await;
                if !providers.is_empty() {
                    match fetch::fetch_from_any(&self.node, &providers, parsed).await {
                        Ok(_provider) => {
                            if let Ok(bytes) = content::get_bytes(&self.node, hash).await {
                                let size = bytes.len() as u64;
                                let external_id = self.lookup_external_for_blake3(hash).await;
                                return Ok(ResolveResult {
                                    bytes,
                                    blake3_hash: hash.to_string(),
                                    external_id,
                                    source: ResolveSource::Local,
                                    size,
                                });
                            }
                        }
                        Err(e) => {
                            log::debug!("resolver: p2p fetch of {hash} failed: {e}; trying URL");
                        }
                    }
                }
            }
        }

        // Step 3: If we know a public URL for this BLAKE3 hash, re-fetch it.
        if let Some(mapped_url) = self.lookup_external_for_blake3(hash).await {
            if is_http_url(&mapped_url) {
                if let Ok(result) = self.fetch_url_and_cache(&mapped_url).await {
                    return Ok(result);
                }
            }
        }

        Err(ResolveError::NotFound(format!("blake3:{}", hash)))
    }

    /// Resolve by URL: mapping -> local first, then fetch URL and cache.
    async fn resolve_url(&self, url: &str) -> Result<ResolveResult, ResolveError> {
        if let Some(mapped_hash) = self.lookup_blake3_for_external(url).await {
            match content::get_bytes(&self.node, &mapped_hash).await {
                Ok(bytes) => {
                    let size = bytes.len() as u64;
                    return Ok(ResolveResult {
                        bytes,
                        blake3_hash: mapped_hash,
                        external_id: Some(url.to_string()),
                        source: ResolveSource::MappedLocal,
                        size,
                    });
                }
                Err(content::ContentError::NotFound(_)) => {}
                Err(e) => return Err(ResolveError::Store(e.to_string())),
            }
        }

        self.fetch_url_and_cache(url).await
    }

    /// Fetch content from a public URL, store in iroh, record mapping.
    async fn fetch_url_and_cache(&self, url: &str) -> Result<ResolveResult, ResolveError> {
        reject_private_url(url)?;

        let bytes = self
            .http
            .fetch_by_url(url)
            .await
            .map_err(|e| ResolveError::Fetch(e.to_string()))?;

        let add_result = content::add_bytes(&self.node, &bytes)
            .await
            .map_err(|e| ResolveError::Store(e.to_string()))?;

        self.save_mapping(url, &add_result.hash, add_result.size)
            .await;

        log::info!(
            "cached URL {} → blake3:{} ({} bytes)",
            url,
            add_result.hash,
            add_result.size
        );

        Ok(ResolveResult {
            bytes,
            blake3_hash: add_result.hash,
            external_id: Some(url.to_string()),
            source: ResolveSource::Url,
            size: add_result.size,
        })
    }

    /// Look up the BLAKE3 hash for a given external URL in the mapping table.
    async fn lookup_blake3_for_external(&self, external_id: &str) -> Option<String> {
        let guard = self.db.lock().ok()?;
        let db = guard.as_ref()?;
        db.conn()
            .query_row(
                "SELECT blake3_hash FROM content_mappings WHERE external_id = ?1",
                params![external_id],
                |row| row.get(0),
            )
            .ok()
    }

    /// Look up the external URL for a given BLAKE3 hash in the mapping table.
    async fn lookup_external_for_blake3(&self, blake3_hash: &str) -> Option<String> {
        let guard = self.db.lock().ok()?;
        let db = guard.as_ref()?;
        db.conn()
            .query_row(
                "SELECT external_id FROM content_mappings WHERE blake3_hash = ?1",
                params![blake3_hash],
                |row| row.get(0),
            )
            .ok()
    }

    /// Save an external-URL↔BLAKE3 mapping to the database.
    async fn save_mapping(&self, external_id: &str, blake3_hash: &str, size: u64) {
        let Ok(guard) = self.db.lock() else {
            log::warn!("database lock poisoned — skipping content mapping save");
            return;
        };
        let Some(db) = guard.as_ref() else {
            return;
        };
        if let Err(e) = db.conn().execute(
            "INSERT OR REPLACE INTO content_mappings (external_id, blake3_hash, size_bytes) VALUES (?1, ?2, ?3)",
            params![external_id, blake3_hash, size as i64],
        ) {
            log::warn!("failed to save content mapping: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    async fn make_resolver() -> (ContentResolver, TempDir) {
        let tmp = TempDir::new().expect("create temp dir");

        // Set up database
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrations");
        let db = Arc::new(Mutex::new(Some(db)));

        // Set up iroh node
        let node = Arc::new(ContentNode::new(tmp.path()));
        node.start(None).await.expect("start node");

        // Short timeout — tests never reach a real HTTP origin.
        let http = HttpClient::new(Duration::from_millis(50)).expect("http client");

        let resolver = ContentResolver::new(node, http, db);
        (resolver, tmp)
    }

    #[tokio::test]
    async fn resolve_blake3_from_local_store() {
        let (resolver, _tmp) = make_resolver().await;

        // Add content directly to iroh
        let data = b"test content for resolver";
        let add = content::add_bytes(&resolver.node, data).await.expect("add");

        // Resolve by BLAKE3 hash
        let result = resolver.resolve(&add.hash).await.expect("resolve");
        assert_eq!(result.bytes, data);
        assert_eq!(result.blake3_hash, add.hash);
        assert_eq!(result.source, ResolveSource::Local);
        assert_eq!(result.size, data.len() as u64);

        resolver.node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn preview_resolution_enforces_exact_local_byte_limit() {
        let (resolver, _tmp) = make_resolver().await;
        let data = b"bounded governance preview";
        let add = content::add_bytes_unencrypted(&resolver.node, data)
            .await
            .expect("add");

        assert!(matches!(
            resolver
                .resolve_preview_bounded(&add.hash, data.len() - 1)
                .await,
            Err(ResolveError::Store(message)) if message.contains("exceeding")
        ));
        assert_eq!(
            resolver
                .resolve_preview_bounded(&add.hash, data.len())
                .await
                .expect("resolve at exact limit")
                .bytes,
            data
        );

        resolver.node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn resolve_blake3_fetches_from_peer_before_url() {
        use crate::content_store::discovery::ContentDiscovery;

        // Provider node holds the content.
        let provider_tmp = TempDir::new().expect("provider temp");
        let provider = ContentNode::new(provider_tmp.path());
        provider.start(None).await.expect("start provider");
        let data = b"resolver p2p path: served by a peer, not the origin";
        let add = content::add_bytes(&provider, data).await.expect("add");
        let provider_addr = provider.endpoint_addr().await.expect("provider addr");

        // Resolver's own node starts empty; seed discovery with the provider.
        let (resolver_base, _tmp) = make_resolver().await;
        let discovery = Arc::new(ContentDiscovery::new());
        let parsed = content::parse_hash(&add.hash).expect("hash");
        discovery.seed(parsed, provider_addr).await;
        let resolver = ContentResolver::with_discovery(
            resolver_base.node.clone(),
            resolver_base.http.clone(),
            resolver_base.db.clone(),
            discovery,
        );

        // No reachable URL origin; success proves the peer path served it.
        let result = resolver.resolve(&add.hash).await.expect("resolve via peer");
        assert_eq!(result.bytes, data);
        assert_eq!(result.source, ResolveSource::Local);

        resolver.node.shutdown().await.expect("shutdown resolver");
        provider.shutdown().await.expect("shutdown provider");
    }

    #[tokio::test]
    async fn resolve_url_via_mapping_table() {
        let (resolver, _tmp) = make_resolver().await;

        // Add content to iroh
        let data = b"mapped content";
        let add = content::add_bytes(&resolver.node, data).await.expect("add");

        // Insert a URL→BLAKE3 mapping
        let url = "https://example.org/media/mapped.bin";
        resolver.save_mapping(url, &add.hash, add.size).await;

        // Resolve by URL should find it via mapping (no network fetch)
        let result = resolver.resolve(url).await.expect("resolve");
        assert_eq!(result.bytes, data);
        assert_eq!(result.blake3_hash, add.hash);
        assert_eq!(result.external_id.as_deref(), Some(url));
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
        let add = content::add_bytes(&resolver.node, data).await.expect("add");

        let url = "https://example.org/media/bidi.bin";
        resolver.save_mapping(url, &add.hash, add.size).await;

        // Lookup URL → BLAKE3
        let hash = resolver.lookup_blake3_for_external(url).await;
        assert_eq!(hash.as_deref(), Some(add.hash.as_str()));

        // Lookup BLAKE3 → URL
        let external = resolver.lookup_external_for_blake3(&add.hash).await;
        assert_eq!(external.as_deref(), Some(url));

        resolver.node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn resolve_blake3_includes_external_when_mapped() {
        let (resolver, _tmp) = make_resolver().await;

        let data = b"content with mapping";
        let add = content::add_bytes(&resolver.node, data).await.expect("add");

        // Without mapping: no external URL
        let result = resolver.resolve(&add.hash).await.expect("resolve");
        assert!(result.external_id.is_none());

        // Add mapping
        let url = "https://example.org/media/withmap.bin";
        resolver.save_mapping(url, &add.hash, add.size).await;

        // With mapping: external URL is included
        let result = resolver.resolve(&add.hash).await.expect("resolve 2");
        assert_eq!(result.external_id.as_deref(), Some(url));

        resolver.node.shutdown().await.expect("shutdown");
    }

    /// Each of these walked through the previous string-splitting guard.
    #[test]
    fn the_standard_ssrf_bypasses_are_refused() {
        // Userinfo: reads as example.com, connects to loopback.
        assert!(reject_private_url("http://example.com@127.0.0.1/x").is_err());
        assert!(reject_private_url("http://user:pass@10.0.0.1/x").is_err());

        // Integer literal for 127.0.0.1.
        assert!(reject_private_url("http://2130706433/").is_err());

        // IPv4-mapped IPv6 — Ipv6Addr::is_loopback() is false for this form.
        assert!(reject_private_url("http://[::ffff:127.0.0.1]/").is_err());

        // Cloud metadata, the usual target.
        assert!(reject_private_url("http://169.254.169.254/latest/meta-data/").is_err());

        // A name that spells out loopback, caught without consulting DNS.
        assert!(reject_private_url("http://localhost:8080/x").is_err());
        assert!(reject_private_url("http://anything.localhost/x").is_err());
    }

    #[test]
    fn plain_private_ranges_are_still_refused() {
        for url in [
            "http://127.0.0.1/",
            "http://10.1.2.3/",
            "http://192.168.1.1/",
            "http://172.16.0.1/",
            "http://0.0.0.0/",
            "http://[::1]/",
            // The IPv4-compatible spelling of loopback, which normalising with
            // `to_ipv4()` would have turned into the routable 0.0.0.1.
            "http://[::0.0.0.1]/",
            "http://[fc00::1]/",
            "http://[fe80::1]/",
            // Carrier-grade NAT and reserved space, which the old check missed.
            "http://100.64.0.1/",
            "http://240.0.0.1/",
        ] {
            assert!(
                reject_private_url(url).is_err(),
                "{url} should have been refused"
            );
        }
    }

    #[test]
    fn transition_and_reserved_addresses_are_refused() {
        for url in [
            // The rest of 0.0.0.0/8 and IETF protocol assignments.
            "http://0.1.2.3/",
            "http://192.0.0.8/",
            // NAT64 to a private and a metadata address; a DNS64 network
            // translates these back to IPv4 on the way out.
            "http://[64:ff9b::c0a8:101]/",
            "http://[64:ff9b::a9fe:a9fe]/",
            "http://[64:ff9b:1::1]/",
            // 6to4 wrapping 192.168.1.1 and 127.0.0.1.
            "http://[2002:c0a8:101::1]/",
            "http://[2002:7f00:1::1]/",
            // Teredo, IPv4-compatible, IPv4-translated, site-local,
            // documentation and discard-only.
            "http://[2001:0:4136:e378:8000:63bf:3fff:fdd2]/",
            "http://[::c0a8:101]/",
            "http://[::ffff:0:c0a8:101]/",
            "http://[fec0::1]/",
            "http://[2001:db8::1]/",
            "http://[100::1]/",
        ] {
            assert!(
                reject_private_url(url).is_err(),
                "{url} should have been refused"
            );
        }
    }

    #[test]
    fn transition_addresses_wrapping_public_ipv4_are_allowed() {
        // NAT64 and 6to4 are judged by the IPv4 address they carry.
        assert!(reject_private_url("http://[64:ff9b::808:808]/").is_ok());
        assert!(reject_private_url("http://[2002:808:808::1]/").is_ok());
    }

    #[test]
    fn non_http_schemes_are_refused() {
        assert!(reject_private_url("file:///etc/passwd").is_err());
        assert!(reject_private_url("gopher://example.com/").is_err());
    }

    #[test]
    fn a_public_address_is_allowed() {
        // Literal addresses, so this does not depend on DNS in CI.
        assert!(reject_private_url("https://8.8.8.8/x").is_ok());
        assert!(reject_private_url("http://93.184.216.34/").is_ok());
        assert!(reject_private_url("https://[2001:4860:4860::8888]/").is_ok());
    }

    #[test]
    fn reviewed_sources_are_public_https_dns_names() {
        for url in [
            "http://mirror.example.org/g",
            "https://user@mirror.example.org/g",
            "https://mirror.example.org:8443/g",
            "https://8.8.8.8/g",
            "https://[2001:4860:4860::8888]/g",
            "https://127.0.0.1/g",
            "https://2130706433/g",
            "https://localhost/g",
            "https://intranet/g",
            "https://mirror.example.org./g",
            "https://printer.local/g",
            "https://metadata.google.internal/g",
            "https://router.home.arpa/g",
            "https://mirror.example/g",
            "https://mirror.test/g",
            "https://example/g",
        ] {
            let parsed = url::Url::parse(url).unwrap();
            assert!(
                reject_unreviewable_https_source(&parsed).is_err(),
                "{url} should have been refused"
            );
        }
        for url in [
            "https://mirror.example.org/g",
            "https://MIRROR.example.org:443/g",
            "https://example.com/genesis.json?v=1",
            "https://notlocal.dev/g",
        ] {
            let parsed = url::Url::parse(url).unwrap();
            assert!(
                reject_unreviewable_https_source(&parsed).is_ok(),
                "{url} should have been allowed"
            );
        }
    }

    #[tokio::test]
    async fn preview_never_falls_back_to_an_unreviewed_mapped_url() {
        let (resolver, _tmp) = make_resolver().await;
        let missing = blake3::hash(b"never stored locally").to_hex().to_string();
        resolver
            .save_mapping("http://mapped.example.org/plain-http", &missing, 20)
            .await;

        assert!(matches!(
            resolver.resolve_preview_bounded(&missing, 1024).await,
            Err(ResolveError::NotFound(_))
        ));

        resolver.node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn preview_refuses_unreviewable_urls_before_any_lookup() {
        let (resolver, _tmp) = make_resolver().await;
        for url in [
            "http://mirror.example.org/genesis.json",
            "https://10.0.0.1/genesis.json",
            "https://metadata.google.internal/genesis.json",
        ] {
            assert!(
                matches!(
                    resolver.resolve_preview_bounded(url, 1024).await,
                    Err(ResolveError::BlockedUrl(_))
                ),
                "{url} should have been refused"
            );
        }

        resolver.node.shutdown().await.expect("shutdown");
    }
}
