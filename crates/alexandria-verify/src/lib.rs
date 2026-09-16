//! Alexandria credential verification.
//!
//! Everything needed to decide whether an Alexandria credential is genuine:
//! the VC envelope types, JCS canonicalization, detached Ed25519 JWS, and
//! `did:key` resolution.
//!
//! The crate performs no I/O. Verification needs to consult persistent state
//! in four places — the issuer's key at a point in time, a status list's bits,
//! a local suspension flag, and whether something supersedes the credential —
//! and each of those arrives through [`VerificationStore`] rather than a
//! database handle. Callers implement the trait over whatever they have:
//! SQLite in the app, a credential bundle in the CLI, Postgres on a server.
//!
//! That indirection is the point. A verifier should link a signature checker,
//! not an application.

pub mod course;
pub mod did;
pub mod governance;
pub mod hash;
pub mod json;
pub mod qualification;
pub mod talent;
pub mod trust;
pub mod vc;

pub use did::{Did, DidError, KeyRegistryEntry, VerificationMethodRef};

/// Persistent state that verification consults.
///
/// Every method distinguishes a known value, a confirmed missing value, and an
/// unavailable lookup. Verification must never turn missing status evidence or
/// a storage failure into a clean active credential.
pub trait VerificationStore {
    /// The issuer's registered key valid at `at`, if the registry holds one.
    ///
    /// Preferred over `did:key` self-resolution so that a credential signed
    /// before a key rotation still verifies afterwards (spec §5.3).
    fn key_at(&self, did: &Did, at: &str) -> StoreLookup<KeyRegistryEntry>;

    /// Raw bits of a known status list.
    fn status_list_bits(&self, list_id: &str) -> StoreLookup<Vec<u8>>;

    /// Local suspension state: `(suspended, suspended_until)`. A `None` inner
    /// value means suspended indefinitely.
    fn suspension(&self, credential_id: &str) -> StoreLookup<(bool, Option<String>)>;

    /// Whether a locally-held credential supersedes this one.
    fn is_superseded(&self, credential_id: &str) -> StoreLookup<bool>;
}

/// Result of consulting verification state supplied by a caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreLookup<T> {
    Found(T),
    Missing,
    Unavailable,
}

/// A store that knows nothing.
///
/// Use it to verify a credential purely on its own contents — the signature,
/// the expiry, and `did:key` self-resolution. A credential that references a
/// status list remains pending because this store cannot supply that list.
pub struct NullStore;

impl VerificationStore for NullStore {
    fn key_at(&self, _did: &Did, _at: &str) -> StoreLookup<KeyRegistryEntry> {
        StoreLookup::Missing
    }
    fn status_list_bits(&self, _list_id: &str) -> StoreLookup<Vec<u8>> {
        StoreLookup::Missing
    }
    fn suspension(&self, _credential_id: &str) -> StoreLookup<(bool, Option<String>)> {
        StoreLookup::Missing
    }
    fn is_superseded(&self, _credential_id: &str) -> StoreLookup<bool> {
        StoreLookup::Missing
    }
}
