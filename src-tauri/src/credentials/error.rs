//! What a credential operation refuses, and what fails beneath it.
//!
//! [`CredentialError`] is the error every function at the module's interface
//! returns. Its display is the text the credential commands returned before
//! it existed; the commands convert it to a string at their boundary.

use std::fmt;

use crate::crypto::did::Did;

/// A change to an issued credential's standing. Each is issuer-bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusChange {
    Revoke,
    Suspend,
    Reinstate,
}

impl fmt::Display for StatusChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Revoke => "revoke",
            Self::Suspend => "suspend",
            Self::Reinstate => "reinstate",
        })
    }
}

/// What a superseding credential must share with the one it replaces
/// (§11.4), so no issuer can retire another's credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupersessionRequires {
    SameIssuer,
    SameSubject,
    SameClaimKind,
}

impl fmt::Display for SupersessionRequires {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SameIssuer => "same issuer",
            Self::SameSubject => "same subject",
            Self::SameClaimKind => "same claim kind",
        })
    }
}

/// The first bound of an [`IssuancePolicy`](super::IssuancePolicy) the bound
/// integrity session broke.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PolicyViolation {
    #[error("issuance policy: session status is '{status}', requires 'completed'")]
    NotClean { status: String },
    #[error("issuance policy: integrity score {score:.3} below minimum {min:.3}")]
    IntegrityBelowMinimum { score: f64, min: f64 },
    #[error("issuance policy: minimum integrity required but session has no score")]
    NoIntegrityScore,
    #[error("issuance policy: {count} critical flags exceed maximum {max}")]
    TooManyCritical { count: i64, max: i64 },
    #[error("issuance policy: {count} warning flags exceed maximum {max}")]
    TooManyWarnings { count: i64, max: i64 },
    #[error("issuance policy: assurance level '{level}' does not meet required '{required}'")]
    AssuranceBelowRequired { level: String, required: String },
    #[error("issuance policy: camera coverage {coverage:.2} below minimum {min:.2}")]
    CameraCoverageBelowMinimum { coverage: f64, min: f64 },
    #[error("issuance policy: camera coverage required but session has no snapshots")]
    NoSnapshots,
    #[error("issuance policy: trust profile '{profile}' does not meet the '{required}' floor")]
    TrustProfileBelowFloor { profile: String, required: String },
    #[error(
        "issuance policy: the '{required}' floor judged before the first item does not match issuance"
    )]
    PolicyFloorDisagrees { required: String },
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    // --- The issuer ------------------------------------------------------
    #[error("vault is locked — unlock first")]
    VaultLocked,
    /// The vault or the wallet it holds could not give the issuer key.
    #[error("{0}")]
    Vault(String),
    /// A rotated DID offered a key that is not its key at issuance time.
    #[error("the signing key is not this DID's current key: {}", .0.as_str())]
    NotCurrentKey(Did),

    // --- Issuance --------------------------------------------------------
    #[error("subject MUST be a DID (§10 non-transferability)")]
    SubjectNotDid,
    #[error("integrity_policy requires integrity_session_id to evaluate against")]
    PolicyWithoutSession,
    #[error("integrity session {0} not found")]
    SessionNotFound(String),
    #[error(transparent)]
    Policy(#[from] PolicyViolation),
    #[error("supersedes target {0} not found locally")]
    SupersededNotFound(String),
    #[error("§11.4: supersession requires {0} as the prior credential")]
    Supersession(SupersessionRequires),
    /// Building the envelope's id or integrity hash failed.
    #[error("{0}")]
    Envelope(String),
    #[error("sign: {0}")]
    Sign(alexandria_verify::vc::VcError),

    // --- Status lists ----------------------------------------------------
    #[error("status list {0} is full")]
    StatusListFull(String),
    #[error("list is not served by a host")]
    ListNotHosted,
    #[error("list belongs to another issuer")]
    ListOfAnotherIssuer,
    #[error("sign status list {list_id}: {source}")]
    SignStatusList {
        list_id: String,
        #[source]
        source: alexandria_verify::vc::VcError,
    },
    /// The host answered the push with a failure status.
    #[error("{origin} returned {status}: {body}")]
    HostRefused {
        origin: String,
        status: reqwest::StatusCode,
        body: String,
    },
    /// The Cloud HTTP client could not be built.
    #[error("{0}")]
    Cloud(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// A profile-scoped database job failed: the executor's jobs return
    /// strings.
    #[error("{0}")]
    Profile(String),

    // --- Self-issuance for observed completions --------------------------
    #[error("completion observation missing")]
    ObservationMissing,
    /// The caller's observation, the stored one and the learner disagree.
    #[error("completion issuance does not match the stored observation and learner")]
    ObservationMismatch,
    #[error(transparent)]
    Completion(#[from] crate::cardano::completion::CompletionError),
    /// Recording the learner's "credential issued" notification failed.
    #[error(transparent)]
    Notification(#[from] crate::notifications::NotificationError),

    // --- Untrusted payloads and bundles ---------------------------------
    /// The payload broke the structural limits or is not JSON.
    #[error("not a valid credential payload: {0}")]
    InvalidPayload(alexandria_verify::json::UntrustedJsonError),
    /// A credential in a payload (`index` within a list) broke the
    /// single-credential limits.
    #[error(
        "credential{}: {source}",
        .index.map(|index| format!(" {index}")).unwrap_or_default()
    )]
    CredentialEntry {
        index: Option<usize>,
        #[source]
        source: alexandria_verify::json::UntrustedJsonError,
    },
    #[error("not recognised as a credential, a list of credentials, or a §20.4 bundle")]
    Unrecognised,
    #[error("parse bundle: not a §20.4 credential bundle")]
    NotABundle,
    #[error("unsupported bundle format_version: {0}")]
    UnsupportedBundleVersion(String),
    #[error("bundle credentials are not a list")]
    BundleCredentialsNotAList,
    #[error("status list credential has no id")]
    StatusListWithoutId,
    #[error("status list issuer: {0}")]
    StatusListIssuer(#[source] alexandria_verify::did::DidError),
    /// A signed status list in a bundle did not verify.
    #[error("status list {list_id}: {source}")]
    StatusListInvalid {
        list_id: String,
        #[source]
        source: alexandria_verify::vc::VcError,
    },
    #[error("decode list bits: {0}")]
    ListBits(#[from] base64::DecodeError),
    #[error("canonicalize bundle: {0}")]
    Canonicalize(#[source] serde_json::Error),
    #[error("read anchor receipt: {0}")]
    AnchorReceipt(String),

    // --- Status changes --------------------------------------------------
    /// The caller is not the credential's issuer (ADR 0003).
    #[error("only the credential issuer may {0} it")]
    NotIssuer(StatusChange),
    #[error("credential {0} not found")]
    NotFound(String),
    /// Revocation needs the credential and the status list it sits in.
    #[error("credential {0} not found or has no status list")]
    NotRevocable(String),

    // --- Failures beneath ------------------------------------------------
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    KeyRegistry(#[from] crate::crypto::key_registry::KeyRegistryError),
    /// A `did:key` did not resolve to a key.
    #[error(transparent)]
    Did(#[from] alexandria_verify::did::DidError),
    /// A database step named for the log ("insert credential").
    #[error("{step}: {source}")]
    DatabaseStep {
        step: &'static str,
        #[source]
        source: rusqlite::Error,
    },
    /// A status-list bit could not be set.
    #[error(transparent)]
    StatusBit(alexandria_verify::vc::VcError),
}

impl CredentialError {
    /// A [`CredentialError::DatabaseStep`] for `map_err`.
    pub(crate) fn step(step: &'static str) -> impl FnOnce(rusqlite::Error) -> Self {
        move |source| Self::DatabaseStep { step, source }
    }
}
