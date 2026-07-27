//! Live tutoring networking over iroh.
//!
//! Owned, lean replacement for the former vendored `iroh-live` crate. Provides:
//!
//! - [`Live`] — a thin MoQ (Media over QUIC) facade over [`iroh_moq::Moq`].
//! - [`rooms`] — a room actor built on raw [`iroh`] + [`iroh_gossip`]. Peer
//!   presence rides a gossip topic ([`rooms::RoomTicket::topic_id`]) as flooded
//!   `PeerAnnounce` messages; each advertised broadcast is subscribed over its
//!   own MoQ session with exponential-backoff retry and periodic reconciliation.
//!
//! Media capture/codecs (`media`) and the MoQ transport (`moq`, [`ALPN`]) are
//! re-exported from the sibling `moq-media` / `iroh-moq` crates, which are kept
//! as-is — only the room/session layer is owned here.

// The room actor's future-set type is intentionally explicit; allow the
// complexity lint rather than obscure it.
#![allow(clippy::type_complexity)]

mod live;
pub mod rooms;

pub use self::live::Live;

pub use iroh_moq as moq;
pub use iroh_moq::ALPN;

pub use moq_media as media;
