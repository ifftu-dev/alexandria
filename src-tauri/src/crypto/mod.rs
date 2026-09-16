pub mod content_crypto;
pub mod did;
pub mod group_key;
pub mod guardian;
pub mod hash;
pub mod key_registry;
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
// The portable keystore is what mobile builds ship, but the test job runs on
// desktop, where `cfg(mobile)` is false — so its refusal and wrong-password
// tests never compiled anywhere, and a mobile target check only type-checks
// production code. Compiling it a second time under test keeps those tests
// running. Nothing outside its own tests uses it here, hence the allow.
#[cfg(all(test, desktop))]
#[allow(dead_code)]
#[path = "keystore_portable.rs"]
mod keystore_portable_on_desktop;
pub mod pairing;
pub mod shamir;
pub mod signing;
pub mod wallet;
