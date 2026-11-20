//! The trust a credential view states: one plain-words provenance, the
//! suspension state and the coverage statement, from one verification.
//!
//! The view never verifies anything itself. It reads the verifier library's
//! result and classification, so the detail view and the bundle view say
//! exactly what a verifier with the same inputs would establish.

use alexandria_verify::attestation::{AttestationOutcome, AttestationTrust};
use alexandria_verify::org::{OrgDescriptor, OrganisationBacking};
use alexandria_verify::trust::{
    classify_credential_with_organisation, CourseEndorsementEvidence, CredentialTrust,
};
use alexandria_verify::vc::verify::verify_credential_with_attestation;
use alexandria_verify::vc::{
    VerifiableCredential, VerificationPendingReason, VerificationPolicy, VerificationResult,
    STATUS_PURPOSE_SUSPENSION,
};
use alexandria_verify::VerificationStore;

use crate::domain::credential_trust::{CredentialTrustView, Provenance, SuspensionState};

/// What one credential's views state, given the store holding its keys and
/// status lists, the attestation trust inputs, and, when the caller resolved
/// them, completion endorsement evidence and the issuer domain's descriptor.
///
/// The credential is verified once with suspension set aside. `trust` is the
/// classification under the default policy, which rejects a suspended
/// credential; `provenance` is classified with suspension set aside, so a
/// suspended credential still says where it came from while `suspension`
/// says it is suspended.
pub fn trust_view(
    store: &dyn VerificationStore,
    attestation: &AttestationTrust,
    credential: &VerifiableCredential,
    verification_time: &str,
    network_id: &str,
    endorsement: Option<CourseEndorsementEvidence<'_>>,
    descriptor: Option<&OrgDescriptor>,
) -> CredentialTrustView {
    let lenient = VerificationPolicy {
        reject_suspended: false,
        ..VerificationPolicy::default()
    };
    // Suspension only changes the acceptance decision, never another check,
    // so one result serves both classifications.
    let verification = verify_credential_with_attestation(
        store,
        credential,
        verification_time,
        &lenient,
        attestation,
    );
    let classify = |policy: &VerificationPolicy| {
        classify_credential_with_organisation(
            credential,
            &verification,
            policy,
            network_id,
            endorsement,
            descriptor,
        )
    };
    let trust = classify(&VerificationPolicy::default());
    let settled = classify(&lenient);
    let (issuer, organisation) = match &settled {
        CredentialTrust::VerifiedIssuerSigned {
            issuer,
            organisation,
            ..
        } => (Some(issuer.clone()), organisation.clone()),
        _ => (None, None),
    };
    CredentialTrustView {
        trust,
        provenance: provenance(&settled),
        issuer,
        organisation,
        suspension: suspension(credential, &verification),
        coverage_statement: credential
            .integrity
            .as_ref()
            .and_then(|integrity| integrity.coverage_statement_digest.as_deref())
            .and_then(crate::sentinel::scoring::resolve_coverage_statement),
    }
}

/// Organisation first, then credential-bound attestation, else self-reported.
fn provenance(trust: &CredentialTrust) -> Option<Provenance> {
    let (attestation, organisation) = match trust {
        CredentialTrust::VerifiedSelfClaim { attestation, .. }
        | CredentialTrust::VerifiedCourseEndorsement { attestation, .. } => (*attestation, None),
        CredentialTrust::VerifiedIssuerSigned {
            attestation,
            organisation,
            ..
        } => (*attestation, organisation.as_ref()),
        CredentialTrust::Invalid { .. } | CredentialTrust::Pending { .. } => return None,
    };
    Some(match (organisation, attestation) {
        (Some(OrganisationBacking::Verified { name, domain }), _) => Provenance::Organisation {
            name: name.clone(),
            domain: domain.clone(),
        },
        (_, AttestationOutcome::VerifiedCredentialBound { platform, .. }) => {
            Provenance::DeviceAttested { platform }
        }
        (_, attestation) => Provenance::SelfReported { attestation },
    })
}

fn suspension(
    credential: &VerifiableCredential,
    verification: &VerificationResult,
) -> SuspensionState {
    let unknown = verification.pending_reasons.iter().find(|reason| {
        matches!(
            reason,
            VerificationPendingReason::SuspensionListMissing
                | VerificationPendingReason::SuspensionListUnavailable
        )
    });
    if verification.suspended {
        SuspensionState::Suspended
    } else if let Some(reason) = unknown {
        SuspensionState::Unknown { reason: *reason }
    } else if credential
        .status_entries()
        .iter()
        .any(|entry| entry.status_purpose == STATUS_PURPOSE_SUSPENSION)
    {
        SuspensionState::NotSuspended
    } else {
        SuspensionState::NoList
    }
}
