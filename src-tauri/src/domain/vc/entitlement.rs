// SPDX-License-Identifier: MIT
//! Entitlement credential claims and issuer pinning.
//!
//! An `EntitlementCredential` says which enterprise features an organisation
//! has paid for. It rides as an ordinary Verifiable Credential, so validity —
//! signature, expiry, status-list revocation, suspension, supersession — is
//! decided by the same [`verify_credential`] path as any other VC, offline and
//! with no license server.
//!
//! ## Why verification alone is not enough
//!
//! [`verify_credential`] proves a credential was signed by whoever its issuer
//! DID *names*. It does not prove who that issuer *is*: issuer resolution falls
//! back to `did:key` self-resolution, and a `did:key` is self-describing — the
//! public key is derivable from the DID string itself.
//!
//! So anyone can generate a `did:key`, self-issue an entitlement granting every
//! feature, and have it verify perfectly: valid signature, issuer resolved,
//! subject bound, unexpired, unrevoked. Accepting a credential purely because
//! it verifies would make the commercial boundary meaningless.
//!
//! [`is_trusted_issuer`] closes that hole. A credential counts only when its
//! issuer is one this build was compiled to trust.
//!
//! ## Threat model
//!
//! This is not DRM. A local attacker can patch the binary, and on an
//! open-source codebase they have the source too. The goal is narrower: an
//! *unmodified* build must never silently unlock enterprise features, and a
//! self-issued credential must not unlock them either.
//!
//! Anything that actually matters — hosted endpoints, seat metering, bulk
//! operations — is enforced server-side. A client-side boolean gates UI, not
//! access.
//!
//! ## Why this file is MIT
//!
//! A user must be able to read what gates the software they are running, and
//! to audit the wire shape of a credential they hold. Only the resolver that
//! consumes this is enterprise-licensed. See `docs/enterprise-boundary.md`.

use crate::crypto::did::Did;

use super::{CredentialType, EntitlementClaim, VerifiableCredential};

/// DIDs permitted to issue `EntitlementCredential`s.
///
/// An allowlist rather than a single constant so a signing key can be rotated
/// with an overlap window instead of invalidating every live entitlement the
/// moment it changes. Rotation still requires a release; that is accepted for
/// now and revisited if entitlements outlive release cadence.
///
/// Empty in this build: no IFFTU signing key has been minted yet, and an empty
/// allowlist fails closed — every entitlement is rejected, which is exactly the
/// community behaviour. Populating this is a deliberate, reviewable act.
pub const TRUSTED_ENTITLEMENT_ISSUERS: &[&str] = &[];

/// Whether `issuer` is permitted to issue entitlements for this build.
///
/// Compared as an exact string against [`TRUSTED_ENTITLEMENT_ISSUERS`]. No
/// prefix or wildcard matching: a partial match is how an allowlist quietly
/// stops being one.
pub fn is_trusted_issuer(issuer: &Did) -> bool {
    is_trusted_issuer_in(issuer, TRUSTED_ENTITLEMENT_ISSUERS)
}

/// [`is_trusted_issuer`] against an explicit allowlist.
///
/// The shipped allowlist is empty, so every accept path — feature union,
/// newest-wins org selection — would otherwise be unreachable and therefore
/// untested until the day a real issuer DID is added. Taking the list as a
/// parameter lets those paths be exercised now, so populating the constant
/// later is a one-line change to already-covered code rather than the first
/// time any of it runs.
pub fn is_trusted_issuer_in(issuer: &Did, trusted: &[&str]) -> bool {
    trusted.contains(&issuer.as_str())
}

/// Why an entitlement credential was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntitlementError {
    /// Not an `EntitlementCredential` at all.
    WrongType,
    /// Signed by a DID this build does not trust. The credential may be
    /// perfectly valid — it is simply not ours.
    UntrustedIssuer,
    /// Subject did not deserialize as an [`EntitlementClaim`] — the
    /// `entitlementPlan` marker is absent, or a required field is missing or
    /// the wrong type.
    Malformed,
}

impl std::fmt::Display for EntitlementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongType => write!(f, "not an EntitlementCredential"),
            Self::UntrustedIssuer => {
                write!(f, "entitlement issuer is not trusted by this build")
            }
            Self::Malformed => write!(f, "malformed entitlement subject"),
        }
    }
}

/// Read the entitlement claims out of a credential.
///
/// Checks the credential *is* an entitlement and that its issuer is trusted,
/// then reads the subject properties. It deliberately does **not** check
/// signature, expiry or revocation — that is [`verify_credential`]'s job, and
/// callers must run it too. Splitting them keeps this module free of
/// verification logic it would only duplicate.
pub fn parse_entitlement(
    credential: &VerifiableCredential,
) -> Result<EntitlementClaim, EntitlementError> {
    parse_entitlement_with(credential, TRUSTED_ENTITLEMENT_ISSUERS)
}

/// [`parse_entitlement`] against an explicit allowlist. See
/// [`is_trusted_issuer_in`] for why this exists.
pub fn parse_entitlement_with(
    credential: &VerifiableCredential,
    trusted: &[&str],
) -> Result<EntitlementClaim, EntitlementError> {
    if !credential
        .type_
        .iter()
        .any(|t| t == CredentialType::EntitlementCredential.as_str())
    {
        return Err(EntitlementError::WrongType);
    }

    if !is_trusted_issuer_in(&credential.issuer, trusted) {
        return Err(EntitlementError::UntrustedIssuer);
    }

    EntitlementClaim::extract(&credential.credential_subject).ok_or(EntitlementError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn credential_with(issuer: &str, subject_props: serde_json::Value) -> VerifiableCredential {
        let props = subject_props
            .as_object()
            .expect("test props must be an object")
            .clone();
        VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some("urn:test:entitlement:1".into()),
            type_: vec![
                "VerifiableCredential".into(),
                CredentialType::EntitlementCredential.as_str().to_string(),
            ],
            issuer: Did(issuer.to_string()),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_until: None,
            credential_subject: super::super::CredentialSubject {
                id: Did("did:key:z6MkOrgSubject".into()),
                properties: props,
            },
            credential_status: None,
            terms_of_use: None,
            witness: None,
            integrity: None,
            proof: super::super::Proof {
                type_: "DataIntegrityProof".into(),
                created: "2026-01-01T00:00:00Z".into(),
                verification_method: crate::crypto::did::VerificationMethodRef(format!(
                    "{issuer}#key-1"
                )),
                proof_purpose: "assertionMethod".into(),
                jws: String::new(),
            },
        }
    }

    /// A well-formed entitlement subject. `entitlementPlan` is the marker
    /// `EntitlementClaim::extract` keys on, so it is never optional here.
    fn entitlement_props(org: &str, features: &[&str]) -> serde_json::Value {
        json!({
            "orgId": org,
            "entitlementPlan": "enterprise",
            "seats": 10,
            "features": features,
        })
    }

    /// The whole reason this module exists: a credential that would pass
    /// `verify_credential` in every respect is still refused when this build
    /// does not trust its issuer.
    #[test]
    fn self_issued_credential_is_rejected_even_though_it_would_verify() {
        let vc = credential_with(
            "did:key:z6MkAttackerGeneratedKey",
            entitlement_props("org-evil", &["talent_index"]),
        );
        assert_eq!(
            parse_entitlement(&vc),
            Err(EntitlementError::UntrustedIssuer)
        );
    }

    /// An empty allowlist must fail closed, not open. This is the shipped
    /// configuration, so the guarantee is worth asserting directly.
    #[test]
    fn empty_allowlist_trusts_nobody() {
        assert!(TRUSTED_ENTITLEMENT_ISSUERS.is_empty());
        assert!(!is_trusted_issuer(&Did("did:key:z6MkAnyone".into())));
    }

    /// Exact match only — a DID that merely starts with a trusted prefix is
    /// not trusted.
    #[test]
    fn issuer_match_is_exact_not_prefix() {
        const TRUSTED: &str = "did:key:z6MkTrusted";
        let suffixed = Did(format!("{TRUSTED}extra"));
        assert_eq!(
            TRUSTED_ENTITLEMENT_ISSUERS.contains(&suffixed.as_str()),
            false
        );
    }

    #[test]
    fn wrong_credential_type_is_rejected() {
        let mut vc = credential_with("did:key:z6MkWhoever", entitlement_props("org-1", &[]));
        vc.type_ = vec![
            "VerifiableCredential".into(),
            CredentialType::AssessmentCredential.as_str().to_string(),
        ];
        assert_eq!(parse_entitlement(&vc), Err(EntitlementError::WrongType));
    }

    /// Type is checked before issuer trust, so a non-entitlement reports the
    /// more specific reason.
    #[test]
    fn type_is_checked_before_issuer() {
        let mut vc = credential_with("did:key:z6MkUntrusted", entitlement_props("org-1", &[]));
        vc.type_ = vec!["VerifiableCredential".into()];
        assert_eq!(parse_entitlement(&vc), Err(EntitlementError::WrongType));
    }

    /// A subject missing the `entitlementPlan` marker is not an entitlement
    /// subject, even on a credential typed as one by a trusted issuer.
    #[test]
    fn subject_without_plan_marker_is_malformed() {
        const TRUSTED: &str = "did:key:z6MkTestTrustedIssuer";
        let vc = credential_with(TRUSTED, json!({ "orgId": "org-1", "seats": 5 }));
        // Prove the failure is the subject shape, not the issuer pin.
        assert!(!is_trusted_issuer(&vc.issuer));
        let claim = EntitlementClaim::extract(&vc.credential_subject);
        assert_eq!(claim, None);
    }

    /// Parsing is a pure read of the subject — it must not invent, drop or
    /// reorder feature keys, including ones this build does not know.
    #[test]
    fn features_pass_through_unfiltered_including_unknown_keys() {
        let vc = credential_with(
            "did:key:z6MkWhoever",
            entitlement_props("org-1", &["talent_index", "not_a_feature_this_build_knows"]),
        );
        let claim =
            EntitlementClaim::extract(&vc.credential_subject).expect("subject is well-formed");
        assert_eq!(
            claim.features,
            vec![
                "talent_index".to_string(),
                "not_a_feature_this_build_knows".to_string()
            ]
        );
        assert_eq!(claim.org_id, "org-1");
        assert_eq!(claim.seats, 10);
        assert!(claim.grants("talent_index"));
        assert!(!claim.grants("employer_console"));
    }
}
