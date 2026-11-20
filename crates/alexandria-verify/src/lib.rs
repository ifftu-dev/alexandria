//! Alexandria credential verification.
//!
//! Everything needed to decide whether an Alexandria credential is genuine:
//! the VC envelope types, JCS canonicalization, the Data Integrity proof
//! (`DataIntegrityProof`, cryptosuite `eddsa-jcs-2022`), `did:key`
//! resolution, and Bitstring Status List revocation and suspension. Around
//! them sit the checks a reader makes of what a credential says: device
//! attestation evidence ([`attestation`]), organisation backing ([`org`]),
//! the coverage statement registry ([`coverage`]), the issuance policy
//! rule an issuer and a hosted service share ([`policy`]), the evidence
//! entry naming the credential a role credential was issued on
//! ([`assessed`]), and answer custody with the evidence entry naming a hosted
//! attempt ([`custody`]).
//!
//! The crate performs no I/O. Verification needs to consult persistent state
//! in four places — the key registered for an issuer whose DID is not
//! self-resolving, a status list's bits (revocation and suspension alike),
//! whether something supersedes the credential, and an observed ledger anchor
//! of its hash — and each of those arrives through
//! [`VerificationStore`] rather than a database handle or a chain client. Callers implement the trait over whatever they have:
//! SQLite in the app, a credential bundle in the CLI, Postgres on a server.
//!
//! That indirection is the point. A verifier should link a signature checker,
//! not an application.

pub mod access;
pub mod assessed;
pub mod attestation;
pub mod course;
pub mod coverage;
pub mod custody;
pub mod did;
pub mod exchange;
pub mod governance;
pub mod hash;
pub mod integrity;
pub mod json;
pub mod org;
pub mod policy;
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
    /// The key registered for an issuer whose DID method is not
    /// self-resolving, if the registry holds one.
    ///
    /// Never consulted for a `did:key`: that resolves to the key in its
    /// identifier, and no registry state can change it.
    fn registered_key(&self, did: &Did) -> StoreLookup<KeyRegistryEntry>;

    /// Raw bits of a known status list, whatever its purpose. Suspension is
    /// read from here too: a credential's suspension entry names a list of
    /// purpose `suspension`, and its bit is the only suspension state
    /// verification reads.
    fn status_list_bits(&self, list_id: &str) -> StoreLookup<Vec<u8>>;

    /// Whether a locally-held credential supersedes this one.
    fn is_superseded(&self, credential_id: &str) -> StoreLookup<bool>;

    /// The anchor receipt for `credential_hash` and the caller's ledger
    /// observation of the transaction it names (spec §11.3.1), if both are
    /// known. The verifier folds the receipt from the recomputed hash and
    /// checks root, transaction, issuer and required signer before trusting
    /// it.
    ///
    /// Stores that know nothing about anchors keep the default: `Missing`, so
    /// `integrityAnchored` is false.
    fn integrity_anchor(&self, _credential_hash: &str) -> StoreLookup<vc::anchor::AnchorEvidence> {
        StoreLookup::Missing
    }
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
    fn registered_key(&self, _did: &Did) -> StoreLookup<KeyRegistryEntry> {
        StoreLookup::Missing
    }
    fn status_list_bits(&self, _list_id: &str) -> StoreLookup<Vec<u8>> {
        StoreLookup::Missing
    }
    fn is_superseded(&self, _credential_id: &str) -> StoreLookup<bool> {
        StoreLookup::Missing
    }
}
