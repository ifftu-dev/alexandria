//! Content storage and addressing.
//!
//! Content is stored and addressed with [iroh](https://iroh.computer) blobs
//! over BLAKE3 — not Kubo, not bitswap, not the public IPFS DHT. Peer
//! discovery runs on Alexandria's own private Kademlia DHT
//! (`/alexandria/kad/1.0`, see `crate::p2p::network`).
//!
//! [`http`] is a plain HTTP(S) fetcher used only to pull seeded / imported
//! media into the store on first access; after that, content is served by
//! BLAKE3 hash over iroh.
//!
//! [`node`] owns a single QUIC endpoint shared by three ALPNs:
//! `iroh-blobs` (content), `iroh-gossip` (room discovery), and MoQ via
//! `iroh-live` (tutoring media).

pub mod cid;
pub mod content;
pub mod course;
pub mod discovery;
pub mod fetch;
pub mod http;
pub mod node;
pub mod pinboard;
pub mod profile;
pub mod resolver;
pub mod storage;
