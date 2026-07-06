pub mod content_crypto;
pub mod did;
pub mod group_key;
pub mod guardian;
pub mod hash;
// Desktop uses the IOTA Stronghold-backed keystore; mobile (iOS/Android)
// swaps in the portable AES-256-GCM + Argon2id keystore. Same `Keystore`
// API either way. Keeping the switch here means this single module list
// serves both platforms — see the unconditional `pub mod crypto;` in
// `lib.rs`.
#[cfg(desktop)]
pub mod keystore;
#[cfg(mobile)]
#[path = "keystore_portable.rs"]
pub mod keystore;
pub mod pairing;
pub mod shamir;
pub mod signing;
pub mod wallet;
