//! Verify a signed VC per §22.1. Stub — implementation in PR 4.

use rusqlite::Connection;

use super::{VerifiableCredential, VerificationPolicy, VerificationResult};

/// Verification algorithm per spec §13.2, steps 1–10. The DB handle is
/// used to look up the issuer's key registry (§5.3 historical keys)
/// and status lists (§11).
pub fn verify_credential(
    _db: &Connection,
    _credential: &VerifiableCredential,
    _verification_time: &str,
    _policy: &VerificationPolicy,
) -> VerificationResult {
    unimplemented!("PR 4 — verify credential")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::{Did, VerificationMethodRef};
    use crate::db::Database;
    use crate::domain::vc::{
        AcceptanceDecision, Claim, CredentialSubject, Proof, SkillClaim, VerifiableCredential,
    };

    fn skeleton(expiration: Option<String>, jws: &str) -> VerifiableCredential {
        VerifiableCredential {
            context: vec!["https://www.w3.org/2018/credentials/v1".into()],
            id: "urn:uuid:verify-unit-test".into(),
            type_: vec!["VerifiableCredential".into(), "FormalCredential".into()],
            issuer: Did("did:key:zIssuerTest".into()),
            issuance_date: "2026-01-01T00:00:00Z".into(),
            expiration_date: expiration,
            credential_subject: CredentialSubject {
                id: Did("did:key:zSubjectTest".into()),
                claim: Claim::Skill(SkillClaim {
                    skill_id: "skill_x".into(),
                    level: 3,
                    score: 0.65,
                    evidence_refs: vec![],
                    rubric_version: None,
                    assessment_method: None,
                }),
            },
            credential_status: None,
            terms_of_use: None,
            proof: Proof {
                type_: "Ed25519Signature2020".into(),
                created: "2026-01-01T00:00:00Z".into(),
                verification_method: VerificationMethodRef("did:key:z...#key-1".into()),
                proof_purpose: "assertionMethod".into(),
                jws: jws.into(),
            },
        }
    }

    #[test]
    #[ignore = "pending PR 4 — verify credential"]
    fn result_echoes_credential_id_and_verification_time() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let vc = skeleton(None, "fake-sig");
        let result = verify_credential(
            db.conn(),
            &vc,
            "2026-04-13T00:00:00Z",
            &VerificationPolicy::default(),
        );
        assert_eq!(result.credential_id, vc.id);
        assert_eq!(result.verification_time, "2026-04-13T00:00:00Z");
    }

    #[test]
    #[ignore = "pending PR 4 — verify credential"]
    fn bad_signature_short_circuits_to_reject() {
        // Acceptance predicate (§13.3): S(c)=0 ⇒ Accept=0 regardless of
        // any other flag. No DB lookups happen after the signature
        // check fails, but the decision MUST be Reject.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let vc = skeleton(None, "invalid-base64-jws");
        let result = verify_credential(
            db.conn(),
            &vc,
            "2026-04-13T00:00:00Z",
            &VerificationPolicy::default(),
        );
        assert!(!result.valid_signature);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    #[ignore = "pending PR 4 — verify credential"]
    fn expired_credential_under_strict_policy_is_rejected() {
        // §11.1: default policy rejects expired credentials.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let vc = skeleton(Some("2026-01-02T00:00:00Z".into()), "fake-sig");
        let strict = VerificationPolicy {
            reject_expired: true,
            ..Default::default()
        };
        let result = verify_credential(db.conn(), &vc, "2026-04-13T00:00:00Z", &strict);
        assert!(result.expired);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    #[ignore = "pending PR 4 — verify credential"]
    fn expired_credential_under_permissive_policy_may_accept() {
        // §11.1 also allows policy to downgrade rather than reject —
        // when reject_expired = false, an otherwise-valid expired cred
        // MUST still have expired=true in the result, but Accept=1 is
        // allowed. We assert only the expired flag here — signature
        // status still drives the final decision.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let vc = skeleton(Some("2026-01-02T00:00:00Z".into()), "fake-sig");
        let permissive = VerificationPolicy {
            reject_expired: false,
            ..Default::default()
        };
        let result = verify_credential(db.conn(), &vc, "2026-04-13T00:00:00Z", &permissive);
        assert!(result.expired);
    }
}
