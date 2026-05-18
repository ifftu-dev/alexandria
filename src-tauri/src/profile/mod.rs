//! Multi-user profile management.
//!
//! Each device may host multiple isolated user profiles. A profile owns its
//! own keystore (vault), SQLCipher database, iroh blob store, plugin bundle
//! directory, and video cache. Switching profiles tears down all per-profile
//! state and rebuilds it for the newly-unlocked profile.
//!
//! Layout on disk:
//!
//! ```text
//! <app_data>/
//!   profiles/
//!     <profile-uuid>/
//!       vault/           # stronghold (desktop) or portable vault (mobile)
//!       alexandria.db    # SQLCipher
//!       iroh/            # iroh blob store + per-profile node secret
//!       plugins/         # installed plugin bundles
//!       videocache/      # materialized video files
//!   profiles_index.json  # public sidecar — display names + avatars only
//! ```
//!
//! `profiles_index.json` is intentionally public (unencrypted) so the picker
//! can render tiles before any vault is unlocked. It only ever contains
//! display name, avatar, accent color, and timestamps — never keys, DIDs,
//! stake addresses, or other cryptographically identifying material.

pub mod index;
pub mod manager;
pub mod migration;

pub use index::{Avatar, ProfileIndex, ProfileSummary};
pub use manager::{ProfileError, ProfileId, ProfileManager, ProfilePaths};
