//! Catalog domain types for the distributed course catalog.
//!
//! The catalog has two representations:
//!
//! - **`CatalogAnnouncement`**: The lightweight gossip payload broadcast on
//!   `/alexandria/catalog/1.0`. Contains just enough for discovery (title,
//!   author, content CID, skill tags, version). Peers use the `content_cid`
//!   to fetch the full `SignedCourseDocument` from iroh/IPFS.
//!
//! - **`CatalogEntry`**: A row from the local `catalog` SQLite table.
//!   Includes metadata about when/how the entry was received.

use serde::{Deserialize, Serialize};

/// A course announcement broadcast on the catalog gossip topic.
///
/// This is the inner payload of a `SignedGossipMessage` on
/// `/alexandria/catalog/1.0`. It is a lightweight summary — peers
/// fetch the full course document from iroh using `content_cid`.
///
/// Per architecture spec §6.1:
/// ```json
/// {
///     "course_id":    "blake2b(stake_address + root_cid)",
///     "title":        "Algorithm Design and Analysis",
///     "root_cid":     "bafy...xyz",
///     "author":       "stake1u8...",
///     "skill_tags":   ["skill_graph_traversal", "dynamic_programming"],
///     "version":      1,
///     "published_at": "<unix_ts>"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogAnnouncement {
    /// Deterministic course ID: `blake2b(author_address + content_cid)`.
    pub course_id: String,
    /// Course title.
    pub title: String,
    /// Optional short description.
    pub description: Option<String>,
    /// BLAKE3 hash of the full course document on iroh.
    /// Peers use this to fetch the `SignedCourseDocument`.
    pub content_cid: String,
    /// Author's Cardano stake address (bech32).
    pub author_address: String,
    /// Optional thumbnail BLAKE3 hash.
    pub thumbnail_cid: Option<String>,
    /// Tags for discovery.
    pub tags: Vec<String>,
    /// Skill IDs this course covers.
    pub skill_ids: Vec<String>,
    /// Course version (monotonically increasing).
    pub version: i64,
    /// Unix timestamp of publication.
    pub published_at: i64,
    /// Discriminator: `"course"` (default) or `"tutorial"`. The default
    /// preserves compatibility with announcements from older nodes that
    /// predate the tutorials feature.
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "course".to_string()
}

/// A catalog entry as stored in the local `catalog` SQLite table.
///
/// Combines the announcement data with local metadata (when received,
/// pin status, on-chain registration).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub course_id: String,
    pub title: String,
    pub description: Option<String>,
    pub author_address: String,
    pub content_cid: String,
    pub thumbnail_cid: Option<String>,
    pub tags: Option<Vec<String>>,
    pub skill_ids: Option<Vec<String>>,
    pub version: i64,
    pub published_at: String,
    pub received_at: String,
    pub pinned: bool,
    pub on_chain_tx: Option<String>,
    /// `"course"` or `"tutorial"`.
    #[serde(default = "default_kind")]
    pub kind: String,
}
