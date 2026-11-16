//! What the app's credential views state about a credential's trust.
//!
//! Mirrors `CredentialTrustView` and its parts in `src/types/index.ts`. Built
//! by [`crate::credentials::trust::trust_view`]; the frontend only renders it.

use serde::{Deserialize, Serialize};

pub use alexandria_verify::attestation::{AttestationOutcome, Platform};
use alexandria_verify::did::Did;
use alexandria_verify::org::OrganisationBacking;
use alexandria_verify::trust::CredentialTrust;
use alexandria_verify::vc::VerificationPendingReason;

use crate::sentinel::scoring::CoverageStatement;

/// A credential's trust, as the credential detail and bundle views state it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CredentialTrustView {
    /// The classification under the default verification policy, which
    /// rejects a suspended credential.
    pub trust: CredentialTrust,
    /// The one plain-words provenance. Judged with suspension set aside, so a
    /// suspended credential still says where it came from; `None` while the
    /// credential is invalid for another reason or pending.
    pub provenance: Option<Provenance>,
    /// Who signed the credential, when that is not its subject.
    pub issuer: Option<Did>,
    /// The organisation backing of the issuer key, when it was judged.
    pub organisation: Option<OrganisationBacking>,
    /// Whether the credential is suspended, from its suspension list.
    pub suspension: SuspensionState,
    /// The coverage statement the integrity assertion names, resolved from
    /// the verifier library's registry; `None` when it names none or this
    /// build cannot resolve it.
    pub coverage_statement: Option<CoverageStatement>,
}

/// Exactly one of the three things a reader can rely on for a verified
/// credential, strongest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Provenance {
    /// An organisation's domain lists the key that signed the credential.
    Organisation { name: String, domain: String },
    /// Platform attestation evidence bound to this credential verified.
    DeviceAttested { platform: Platform },
    /// Nothing beyond the signature backs the figures. `attestation` says
    /// what the credential's evidence, if any, did prove, so the view can say
    /// why it reads as self-reported.
    SelfReported { attestation: AttestationOutcome },
}

/// A credential's suspension, read only from its published suspension list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SuspensionState {
    /// The suspension list was read and the credential's bit is clear.
    NotSuspended,
    /// The suspension list was read and the credential's bit is set.
    Suspended,
    /// The suspension list could not be read, so suspension is unknown.
    Unknown { reason: VerificationPendingReason },
    /// The credential names no suspension list, so no verifier can see it
    /// suspended.
    NoList,
}
