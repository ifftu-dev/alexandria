//! Content storage and addressing.
//!
//! **This module is not IPFS.** The name is legacy. Content is stored
//! and addressed with [iroh](https://iroh.computer) blobs over BLAKE3 —
//! not Kubo, not bitswap, not the public IPFS DHT. Peer discovery runs
//! on Alexandria's own private Kademlia DHT (`/alexandria/kad/1.0`, see
//! `crate::p2p::network`), deliberately isolated from the public IPFS
//! network.
//!
//! Only [`gateway`] and [`cid`] retain genuinely IPFS-shaped concepts.
//!
//! [`node`] owns a single QUIC endpoint shared by three ALPNs:
//! `iroh-blobs` (content), `iroh-gossip` (room discovery), and MoQ via
//! `iroh-live` (tutoring media).

pub mod cid;
pub mod content;
pub mod course;
pub mod discovery;
pub mod fetch;
pub mod gateway;
pub mod node;
pub mod pinboard;
pub mod profile;
pub mod resolver;
pub mod storage;
