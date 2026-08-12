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

pub mod did;
pub mod hash;
pub mod talent;
pub mod vc;

pub use did::{Did, DidError, KeyRegistryEntry, VerificationMethodRef};

/// Persistent state that verification consults.
///
/// Every method returns `Option` rather than `Result`: an absent record and a
/// failed lookup are treated alike, because verification is conservative by
/// design. A status list we cannot read is not evidence of revocation, and a
/// suspension we cannot read is not evidence of suspension. Implementations
/// should log their own failures — this layer deliberately cannot distinguish
/// "no" from "don't know", and must not reject on the difference.
///
/// [`NullStore`] implements it as "nothing is known", which is the correct
/// posture for verifying a self-contained credential with no local context.
pub trait VerificationStore {
    /// The issuer's registered key valid at `at`, if the registry holds one.
    ///
    /// Preferred over `did:key` self-resolution so that a credential signed
    /// before a key rotation still verifies afterwards (spec §5.3).
    fn key_at(&self, did: &Did, at: &str) -> Option<KeyRegistryEntry>;

    /// Raw bits of a known status list. `None` means the list is unknown,
    /// which callers treat as "not known to be revoked".
    fn status_list_bits(&self, list_id: &str) -> Option<Vec<u8>>;

    /// Local suspension state: `(suspended, suspended_until)`. A `None` inner
    /// value means suspended indefinitely.
    fn suspension(&self, credential_id: &str) -> Option<(bool, Option<String>)>;

    /// Whether a locally-held credential supersedes this one.
    fn is_superseded(&self, credential_id: &str) -> bool;
}

/// A store that knows nothing.
///
/// Use it to verify a credential purely on its own contents — the signature,
/// the expiry, and `did:key` self-resolution. Nothing is revoked, suspended or
/// superseded, because there is no local context in which it could be.
pub struct NullStore;

impl VerificationStore for NullStore {
    fn key_at(&self, _did: &Did, _at: &str) -> Option<KeyRegistryEntry> {
        None
    }
    fn status_list_bits(&self, _list_id: &str) -> Option<Vec<u8>> {
        None
    }
    fn suspension(&self, _credential_id: &str) -> Option<(bool, Option<String>)> {
        None
    }
    fn is_superseded(&self, _credential_id: &str) -> bool {
        false
    }
}
