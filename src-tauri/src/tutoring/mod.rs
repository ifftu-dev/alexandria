//! Live tutoring module — P2P video/audio sessions over iroh.
//!
//! Uses `iroh-live` (Media over QUIC) for media transport and
//! `iroh-gossip` for room peer discovery. All traffic flows
//! through the same iroh `Endpoint` used for blob storage.
//!
//! Architecture:
//!   ContentNode.endpoint() + gossip() + live()
//!       → TutoringManager (room lifecycle, session DB records)
//!           → Room (gossip peer discovery + MoQ media)
//!               → PublishBroadcast (local camera/mic → H264/Opus → MoQ)
//!               → SubscribeBroadcast (remote MoQ → decode → frames)
//!
//! Desktop: full video + audio + screen share.
//! iOS: full video + audio via platform camera + VideoToolbox.
//! Android: real room/audio/chat flow via the shared manager path, but
//! tutoring video is still disabled in the current Android build.

#[cfg(any(desktop, target_os = "android"))]
pub mod manager;

#[cfg(target_os = "ios")]
pub mod manager_mobile;

#[cfg(target_os = "android")]
pub mod manager_android;

#[cfg(any(desktop, target_os = "android"))]
pub use manager::TutoringManager;

#[cfg(target_os = "ios")]
pub use manager_mobile::TutoringManager;
