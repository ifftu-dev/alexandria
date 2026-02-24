pub mod attestation;
#[cfg(feature = "desktop")]
pub mod cardano;
pub mod catalog;
pub mod chapters;
pub mod courses;
pub mod elements;
pub mod enrollment;
pub mod evidence;
pub mod governance;
pub mod health;
pub mod integrity;
pub mod reputation;
pub mod snapshot;
pub mod taxonomy;

// Desktop-only command modules (require iroh, stronghold, libp2p)
#[cfg(feature = "desktop")]
pub mod challenge;
#[cfg(feature = "desktop")]
pub mod content;
#[cfg(feature = "desktop")]
pub mod identity;
#[cfg(feature = "desktop")]
pub mod p2p;
#[cfg(feature = "desktop")]
pub mod sync;
