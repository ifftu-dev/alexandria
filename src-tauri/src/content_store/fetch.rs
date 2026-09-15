//! Peer-to-peer blob fetch over iroh.
//!
//! This is the piece that turns the iroh store from a *local cache* into a
//! *P2P storage layer*: content the local node does not have is pulled from
//! another iroh endpoint over the BLAKE3 blobs protocol, then tagged so it is
//! retained and re-servable. The blob stays BLAKE3-addressed end to end — no
//! external origin involved.
//!
//! Two entry points:
//!   - [`fetch_from_peer`] — fetch from one known provider ([`EndpointAddr`]).
//!     Used when we already know who holds the content: a PinBoard pinner, a
//!     `BlobTicket`, or a device in our own account.
//!   - [`fetch_from_any`] — try a list of candidate providers in order until
//!     one serves the blob. The list comes from discovery
//!     (`super::discovery`) or from PinBoard commitments.
//!
//! We connect directly and drive the fetch with `store.remote().fetch(..)`
//! rather than `iroh_blobs`' `Downloader`. The `Downloader` resolves providers
//! by `EndpointId` and needs the endpoint's address-lookup table primed with
//! each provider's addr; since discovery already hands us full
//! [`EndpointAddr`]s, a direct connect is simpler and lets us control fallback
//! order. Racing/among-provider range-splitting via `Downloader` is a possible
//! future optimization once discovery is a live address source.

use std::time::Duration;

use futures::StreamExt;
use iroh::{Endpoint, EndpointAddr};
use iroh_blobs::api::blobs::BlobStatus;
use iroh_blobs::api::remote::GetProgressItem;
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::{Hash, HashAndFormat};
use thiserror::Error;

use super::content::parse_hash;
use super::node::ContentNode;

/// How long a single connect+fetch attempt may run before giving up.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Error, Debug)]
pub enum FetchError {
    #[error("node not running")]
    NodeNotRunning,
    #[error("invalid hash: {0}")]
    InvalidHash(String),
    #[error("connect to provider failed: {0}")]
    Connect(String),
    #[error("blob fetch failed: {0}")]
    Fetch(String),
    #[error("fetch timed out")]
    Timeout,
    #[error("no provider served the blob (tried {tried})")]
    NoProvider { tried: usize },
    #[error("blob still missing locally after fetch: {0}")]
    MissingAfterFetch(String),
    #[error("blob is at least {size} bytes, exceeding the {max_bytes}-byte limit")]
    TooLarge { size: u64, max_bytes: usize },
}

/// Fetch a blob directly from a single known provider, then tag it locally.
///
/// On success the blob is present in the local `FsStore` under a named tag
/// (the hex hash), exactly as if it had been added locally — so it survives
/// restart, counts toward the store, and can be served to other peers.
pub async fn fetch_from_peer(
    node: &ContentNode,
    provider: EndpointAddr,
    hash: Hash,
) -> Result<(), FetchError> {
    let endpoint = node.endpoint().await.ok_or(FetchError::NodeNotRunning)?;
    // Clone the store handle out of the guard so we do not hold the node mutex
    // across the (potentially long) network fetch. `FsStore` is a cheap actor
    // handle.
    let store: FsStore = node
        .store()
        .await
        .map_err(|_| FetchError::NodeNotRunning)?
        .clone();

    fetch_into(&endpoint, &store, provider, hash, None).await?;

    // Confirm the blob really landed before we claim success.
    let present = store
        .has(hash)
        .await
        .map_err(|e| FetchError::Fetch(e.to_string()))?;
    if !present {
        return Err(FetchError::MissingAfterFetch(hash.to_hex().to_string()));
    }

    tag_retained(&store, hash).await?;
    Ok(())
}

/// Fetch at most `max_bytes` from one provider without creating a retention
/// tag. Used for preview flows where retrieval must not become an implicit
/// pin. Dropping the progress stream cancels an oversized transfer; any
/// partial untagged data remains eligible for the store's garbage collection.
pub async fn fetch_from_peer_unretained_bounded(
    node: &ContentNode,
    provider: EndpointAddr,
    hash: Hash,
    max_bytes: usize,
) -> Result<(), FetchError> {
    let endpoint = node.endpoint().await.ok_or(FetchError::NodeNotRunning)?;
    let store: FsStore = node
        .store()
        .await
        .map_err(|_| FetchError::NodeNotRunning)?
        .clone();

    if let BlobStatus::Complete { size } | BlobStatus::Partial { size: Some(size) } = store
        .status(hash)
        .await
        .map_err(|error| FetchError::Fetch(error.to_string()))?
    {
        if size > max_bytes as u64 {
            return Err(FetchError::TooLarge { size, max_bytes });
        }
    }

    fetch_into(&endpoint, &store, provider, hash, Some(max_bytes)).await?;

    match store
        .status(hash)
        .await
        .map_err(|error| FetchError::Fetch(error.to_string()))?
    {
        BlobStatus::Complete { size } if size <= max_bytes as u64 => Ok(()),
        BlobStatus::Complete { size } | BlobStatus::Partial { size: Some(size) }
            if size > max_bytes as u64 =>
        {
            Err(FetchError::TooLarge { size, max_bytes })
        }
        _ => Err(FetchError::MissingAfterFetch(hash.to_hex().to_string())),
    }
}

/// Try each candidate provider in order until one serves the blob.
///
/// Returns `Ok(EndpointAddr)` of the provider that succeeded. Errors from
/// individual providers are logged and swallowed; only if *every* candidate
/// fails do we return [`FetchError::NoProvider`].
pub async fn fetch_from_any(
    node: &ContentNode,
    providers: &[EndpointAddr],
    hash: Hash,
) -> Result<EndpointAddr, FetchError> {
    if providers.is_empty() {
        return Err(FetchError::NoProvider { tried: 0 });
    }
    for provider in providers {
        match fetch_from_peer(node, provider.clone(), hash).await {
            Ok(()) => return Ok(provider.clone()),
            Err(e) => {
                log::warn!(
                    "p2p fetch of {} from {} failed: {e}",
                    hash.to_hex(),
                    provider.id.fmt_short()
                );
            }
        }
    }
    Err(FetchError::NoProvider {
        tried: providers.len(),
    })
}

/// Try candidate providers without retaining the fetched blob and stop each
/// attempt once the caller's byte budget is crossed.
pub async fn fetch_from_any_unretained_bounded(
    node: &ContentNode,
    providers: &[EndpointAddr],
    hash: Hash,
    max_bytes: usize,
) -> Result<EndpointAddr, FetchError> {
    if providers.is_empty() {
        return Err(FetchError::NoProvider { tried: 0 });
    }
    for provider in providers {
        match fetch_from_peer_unretained_bounded(node, provider.clone(), hash, max_bytes).await {
            Ok(()) => return Ok(provider.clone()),
            Err(error) => {
                log::warn!(
                    "bounded P2P fetch of {} from {} failed: {error}",
                    hash.to_hex(),
                    provider.id.fmt_short()
                );
            }
        }
    }
    Err(FetchError::NoProvider {
        tried: providers.len(),
    })
}

/// Convenience wrapper: fetch by hex hash string from a single provider.
pub async fn fetch_hex_from_peer(
    node: &ContentNode,
    provider: EndpointAddr,
    hash_hex: &str,
) -> Result<(), FetchError> {
    let hash = parse_hash(hash_hex).map_err(|e| FetchError::InvalidHash(e.to_string()))?;
    fetch_from_peer(node, provider, hash).await
}

/// Core connect + fetch, bounded by [`FETCH_TIMEOUT`]. Does not tag.
async fn fetch_into(
    endpoint: &Endpoint,
    store: &FsStore,
    provider: EndpointAddr,
    hash: Hash,
    max_bytes: Option<usize>,
) -> Result<(), FetchError> {
    let fut = async {
        let conn = endpoint
            .connect(provider, iroh_blobs::ALPN)
            .await
            .map_err(|e| FetchError::Connect(e.to_string()))?;
        // `Remote::fetch` takes the locally-available ranges into account and
        // downloads only what is missing, writing into the store.
        let stream = store.remote().fetch(conn, hash).stream();
        tokio::pin!(stream);
        while let Some(item) = stream.next().await {
            match item {
                GetProgressItem::Progress(size)
                    if max_bytes.is_some_and(|limit| size > limit as u64) =>
                {
                    return Err(FetchError::TooLarge {
                        size,
                        max_bytes: max_bytes.expect("checked as some"),
                    });
                }
                GetProgressItem::Progress(_) => {}
                GetProgressItem::Done(_) => return Ok(()),
                GetProgressItem::Error(error) => {
                    return Err(FetchError::Fetch(error.to_string()));
                }
            }
        }
        Err(FetchError::Fetch(
            "provider closed the transfer without a result".to_string(),
        ))
    };
    match tokio::time::timeout(FETCH_TIMEOUT, fut).await {
        Ok(res) => res,
        Err(_) => Err(FetchError::Timeout),
    }
}

/// Set a named tag (hex hash) on a raw blob so the store retains it and the
/// eviction layer can find it — same tagging scheme as `content::add_bytes`.
async fn tag_retained(store: &FsStore, hash: Hash) -> Result<(), FetchError> {
    let hash_hex = hash.to_hex().to_string();
    store
        .tags()
        .set(&hash_hex, HashAndFormat::raw(hash))
        .await
        .map_err(|e| FetchError::Fetch(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn bounded_unretained_fetch_stops_oversized_peer_blob() {
        let provider_dir = TempDir::new().expect("provider temp dir");
        let provider = ContentNode::new(provider_dir.path());
        provider.start(None).await.expect("start provider");
        let bytes = vec![0x5a; 512 * 1024];
        let added = super::super::content::add_bytes_unencrypted(&provider, &bytes)
            .await
            .expect("add provider blob");
        let hash = super::super::content::parse_hash(&added.hash).expect("parse blob hash");

        let receiver_dir = TempDir::new().expect("receiver temp dir");
        let receiver = ContentNode::new(receiver_dir.path());
        receiver.start(None).await.expect("start receiver");
        let result = fetch_from_peer_unretained_bounded(
            &receiver,
            provider.endpoint_addr().await.expect("provider address"),
            hash,
            256 * 1024,
        )
        .await;
        assert!(matches!(result, Err(FetchError::TooLarge { .. })));

        receiver.shutdown().await.expect("shutdown receiver");
        provider.shutdown().await.expect("shutdown provider");
    }
}
