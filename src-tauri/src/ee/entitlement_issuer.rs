// SPDX-License-Identifier: LicenseRef-IFFTU-Enterprise
//! Mint `EntitlementCredential`s with the IFFTU issuer key.
//!
//! ## Why this is enterprise, not MIT
//!
//! The boundary doc's graph-integrity test asks "is it part of how a
//! credential is produced?" — and everything that produces a *skill* claim is
//! permanently MIT. An entitlement is deliberately outside that graph: it
//! carries no capability claim about a learner and has aggregation weight
//! `0.00`, so it can never affect a skill score. What it does carry is org,
//! plan, and seats. That is seat accounting and billing, which the boundary
//! doc places squarely in EE.
//!
//! Verification stays MIT (`domain/vc/entitlement.rs`), so a customer can
//! always audit a credential they were issued without enterprise sources.
//! Only minting one is restricted.
//!
//! ## Key custody
//!
//! Follows the `cardano::operator` precedent exactly: the signing key is
//! configured by environment variable on the issuing deployment and is absent
//! everywhere else. There is no "gate" to bypass — on a node without the key,
//! [`load_issuer_key`] returns `None` and issuance is simply not available.
//!
//!   * `ENTITLEMENT_ISSUER_SKEY_PATH` — path to a file holding the raw 32-byte
//!     Ed25519 seed as hex.
//!   * `ENTITLEMENT_ISSUER_SKEY_HEX` — the seed inline as hex (CI / tests).
//!
//! The DID derived from this key must appear in
//! [`TRUSTED_ENTITLEMENT_ISSUERS`] for the resulting credential to be honoured
//! by any client. Minting with an unlisted key produces a credential that
//! verifies and grants nothing — deliberately, so a leaked or rotated-out key
//! cannot unlock anything.
//!
//! [`TRUSTED_ENTITLEMENT_ISSUERS`]: crate::domain::vc::entitlement::TRUSTED_ENTITLEMENT_ISSUERS

use ed25519_dalek::SigningKey;

use crate::crypto::did::{derive_did_key, Did, VerificationMethodRef};
use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
use crate::domain::vc::{
    CredentialStatus, CredentialSubject, CredentialType, EntitlementClaim, Proof,
    VerifiableCredential,
};

/// What an entitlement is being issued for.
#[derive(Debug, Clone)]
pub struct IssueEntitlementRequest {
    /// DID of the organisation the entitlement is sold to. This is the
    /// credential subject, so the entitlement is bound to the org the same way
    /// a skill credential is bound to a learner.
    pub org_did: Did,
    /// Organisation identifier in the billing system.
    pub org_id: String,
    /// Plan identifier (e.g. `"team"`, `"enterprise"`).
    pub plan: String,
    /// Seats purchased. Advisory on the client; metered server-side.
    pub seats: u32,
    /// Feature keys this plan unlocks.
    pub features: Vec<String>,
    /// RFC 3339 instant the entitlement stops being valid.
    ///
    /// Required, not optional. An entitlement is a paid term, and an
    /// open-ended one would survive the end of the contract with revocation as
    /// the only remedy — which needs the client to have seen a status list.
    /// A fixed term expires correctly even on a device that has been offline
    /// since issuance.
    pub valid_until: String,
    /// Status-list entry backing revocation, so a term can be cut short.
    /// Reuses the ordinary VC status-list path rather than inventing a
    /// separate entitlement revocation mechanism.
    pub status: Option<CredentialStatus>,
}

/// Load the entitlement issuer key, or `None` when this node is not an
/// issuer — which is every node but IFFTU's.
pub fn load_issuer_key() -> Option<SigningKey> {
    let hex_str = if let Ok(path) = std::env::var("ENTITLEMENT_ISSUER_SKEY_PATH") {
        std::fs::read_to_string(&path).ok()?.trim().to_string()
    } else {
        std::env::var("ENTITLEMENT_ISSUER_SKEY_HEX").ok()?
    };

    let bytes = hex::decode(hex_str.trim()).ok()?;
    let seed: [u8; 32] = bytes.try_into().ok()?;
    Some(SigningKey::from_bytes(&seed))
}

/// True if this node holds an entitlement issuer key.
pub fn is_issuer() -> bool {
    std::env::var("ENTITLEMENT_ISSUER_SKEY_PATH").is_ok()
        || std::env::var("ENTITLEMENT_ISSUER_SKEY_HEX").is_ok()
}

/// Mint and sign an entitlement credential.
///
/// `now` is the RFC 3339 issuance instant, passed in rather than read from the
/// clock so issuance is reproducible in tests.
///
/// The signature is produced by the MIT `sign_credential` primitive — this
/// module contributes the *shape* and the *key*, not any new cryptography.
pub fn issue_entitlement(
    signing_key: &SigningKey,
    req: &IssueEntitlementRequest,
    credential_id: &str,
    now: &str,
) -> Result<VerifiableCredential, String> {
    if req.valid_until.as_str() <= now {
        return Err("entitlement would be expired at issuance".into());
    }

    let issuer = derive_did_key(signing_key);
    let claim = EntitlementClaim {
        org_id: req.org_id.clone(),
        entitlement_plan: req.plan.clone(),
        seats: req.seats,
        features: req.features.clone(),
    };

    let credential = VerifiableCredential {
        context: vec!["https://www.w3.org/ns/credentials/v2".into()],
        id: Some(credential_id.to_string()),
        type_: vec![
            "VerifiableCredential".into(),
            CredentialType::EntitlementCredential.as_str().to_string(),
        ],
        issuer: issuer.clone(),
        valid_from: now.to_string(),
        valid_until: Some(req.valid_until.clone()),
        credential_subject: CredentialSubject {
            id: req.org_did.clone(),
            properties: claim.into_properties(),
        },
        credential_status: req.status.clone(),
        terms_of_use: None,
        witness: None,
        integrity: None,
        proof: Proof {
            type_: "Ed25519Signature2020".into(),
            created: now.to_string(),
            verification_method: VerificationMethodRef(format!("{}#key-1", issuer.as_str())),
            proof_purpose: "assertionMethod".into(),
            jws: String::new(),
        },
    };

    sign_credential(UnsignedCredential { credential }, signing_key, &issuer)
        .map_err(|e| format!("sign entitlement: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::vc::entitlement::{is_trusted_issuer, parse_entitlement, EntitlementError};
    use crate::domain::vc::verify::verify_credential;
    use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

    const NOW: &str = "2026-04-13T00:00:00Z";
    const LATER: &str = "2027-04-13T00:00:00Z";

    fn issuer_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn request() -> IssueEntitlementRequest {
        IssueEntitlementRequest {
            org_did: Did("did:key:z6MkOrgSubject".into()),
            org_id: "org-1".into(),
            plan: "enterprise".into(),
            seats: 25,
            features: vec!["talent_index".into(), "employer_console".into()],
            valid_until: LATER.into(),
            status: None,
        }
    }

    fn policy() -> VerificationPolicy {
        VerificationPolicy {
            allowed_types: vec![CredentialType::EntitlementCredential],
            ..VerificationPolicy::default()
        }
    }

    /// A minted entitlement must satisfy the ordinary MIT verification path —
    /// issuance introduces no private envelope shape.
    #[test]
    fn a_minted_entitlement_verifies_through_the_mit_path() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();

        let vc = issue_entitlement(&issuer_key(), &request(), "urn:test:ent:1", NOW).unwrap();
        let result = verify_credential(db.conn(), &vc, NOW, &policy());
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
        assert!(result.valid_signature);
    }

    /// Holding the signing key is not the same as being trusted. Until the
    /// issuer DID is added to the compiled-in allowlist, a credential this
    /// module mints grants nothing — which is exactly what a leaked or
    /// rotated-out key must do.
    #[test]
    fn minting_with_an_unlisted_key_grants_nothing() {
        let vc = issue_entitlement(&issuer_key(), &request(), "urn:test:ent:1", NOW).unwrap();
        assert!(!is_trusted_issuer(&vc.issuer));
        assert_eq!(
            parse_entitlement(&vc),
            Err(EntitlementError::UntrustedIssuer)
        );
    }

    /// The subject is the organisation, and the claim round-trips intact.
    #[test]
    fn the_subject_is_the_org_and_the_claim_survives_signing() {
        let req = request();
        let vc = issue_entitlement(&issuer_key(), &req, "urn:test:ent:1", NOW).unwrap();

        assert_eq!(vc.credential_subject.id, req.org_did);
        let claim = EntitlementClaim::extract(&vc.credential_subject).unwrap();
        assert_eq!(claim.org_id, "org-1");
        assert_eq!(claim.entitlement_plan, "enterprise");
        assert_eq!(claim.seats, 25);
        assert!(claim.grants("talent_index"));
        assert!(claim.grants("employer_console"));
    }

    /// A term is mandatory and must be in the future — issuing something
    /// already expired is a billing bug, not a credential to store.
    #[test]
    fn an_already_expired_term_is_refused() {
        let mut req = request();
        req.valid_until = "2020-01-01T00:00:00Z".into();
        assert!(issue_entitlement(&issuer_key(), &req, "urn:test:ent:1", NOW).is_err());
    }

    /// The expiry actually bites at verification time, offline, with no
    /// status list involved.
    #[test]
    fn the_term_expires_without_needing_revocation() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();

        let vc = issue_entitlement(&issuer_key(), &request(), "urn:test:ent:1", NOW).unwrap();
        let after_term = "2028-01-01T00:00:00Z";
        let result = verify_credential(db.conn(), &vc, after_term, &policy());
        assert!(result.expired);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    /// Absent configuration, a node is not an issuer. Asserted without
    /// touching the environment so the test cannot race a sibling.
    #[test]
    fn is_issuer_requires_explicit_configuration() {
        if std::env::var("ENTITLEMENT_ISSUER_SKEY_PATH").is_err()
            && std::env::var("ENTITLEMENT_ISSUER_SKEY_HEX").is_err()
        {
            assert!(!is_issuer());
            assert!(load_issuer_key().is_none());
        }
    }
}
