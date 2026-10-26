//! Live calls — P2P video/audio sessions over iroh.
//!
//! Uses the `live` crate (Media over QUIC) for media transport and
//! `iroh-gossip` for room peer discovery. All traffic flows through the
//! same iroh `Endpoint` used for blob storage.
//!
//! Architecture:
//!   ContentNode.endpoint() + gossip() + live()
//!       → CallManager<PlatformMedia> (room lifecycle, chat, names, records)
//!           → MediaDevices adapter (camera, screen, codecs, audio route)
//!           → Room (gossip peer discovery + MoQ media)
//!               → PublishBroadcast (local camera/mic → H264/Opus → MoQ)
//!               → SubscribeBroadcast (remote MoQ → decode → frames)
//!
//! One call manager serves every platform; `media` holds the adapters:
//! desktop (nokhwa + ffmpeg + screen share), iOS (AVFoundation +
//! VideoToolbox + AVAudioSession), Android (NDK Camera2 over ffmpeg) and
//! synthetic (tests and builds without a video feature).

pub(crate) mod call_kind;
pub mod events;
pub mod manager;
pub mod media;
pub(crate) mod presence;
pub mod records;
mod tasks;

#[cfg(tutoring_video)]
mod preview_jpeg;

#[cfg(test)]
mod manager_tests;

pub use manager::CallManager;
pub use media::PlatformMedia;

/// The call manager over this build's media adapter, as `AppState` holds it.
pub type TutoringManager = CallManager<PlatformMedia>;
