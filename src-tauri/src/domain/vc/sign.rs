//! Sign a VC payload with Ed25519Signature2020. Stub — implementation in PR 4.

use ed25519_dalek::SigningKey;

use super::{VcError, VerifiableCredential};
use crate::crypto::did::Did;

/// The unsigned portion of a VC — everything except `proof`.
///
/// The caller constructs this explicitly; `sign_credential` canonicalizes
/// the JCS bytes, produces an Ed25519 signature, and returns the full
/// signed `VerifiableCredential`.
#[derive(Debug, Clone)]
pub struct UnsignedCredential {
    pub credential: VerifiableCredential,
}

pub fn sign_credential(
    _unsigned: UnsignedCredential,
    _signing_key: &SigningKey,
    _issuer_did: &Did,
) -> Result<VerifiableCredential, VcError> {
    unimplemented!("PR 4 — Ed25519Signature2020")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::vc::{Claim, CredentialSubject, Proof, SkillClaim, VerifiableCredential};

    fn test_signing_key(role: &str) -> SigningKey {
        let mut bytes = [0u8; 32];
        let b = role.as_bytes();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = b[i % b.len().max(1)];
        }
        SigningKey::from_bytes(&bytes)
    }

    fn unsigned_skeleton(issuer: Did, subject: Did) -> UnsignedCredential {
        UnsignedCredential {
            credential: VerifiableCredential {
                context: vec!["https://www.w3.org/2018/credentials/v1".into()],
                id: "urn:uuid:sign-unit-test".into(),
                type_: vec!["VerifiableCredential".into(), "FormalCredential".into()],
                issuer,
                issuance_date: "2026-04-13T00:00:00Z".into(),
                expiration_date: None,
                credential_subject: CredentialSubject {
                    id: subject,
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
                    created: "2026-04-13T00:00:00Z".into(),
                    verification_method: crate::crypto::did::VerificationMethodRef(
                        "did:key:z...#key-1".into(),
                    ),
                    proof_purpose: "assertionMethod".into(),
                    jws: String::new(),
                },
            },
        }
    }

    #[test]
    #[ignore = "pending PR 4 — Ed25519Signature2020"]
    fn sign_credential_populates_proof_jws() {
        let key = test_signing_key("issuer");
        let issuer = Did("did:key:zIssuerTest".into());
        let subject = Did("did:key:zSubjectTest".into());
        let signed =
            sign_credential(unsigned_skeleton(issuer.clone(), subject), &key, &issuer).unwrap();
        // Proof JWS must be populated after signing (was empty pre-sign).
        assert!(!signed.proof.jws.is_empty());
        assert_eq!(signed.proof.type_, "Ed25519Signature2020");
        assert_eq!(signed.proof.proof_purpose, "assertionMethod");
    }

    #[test]
    #[ignore = "pending PR 4 — Ed25519Signature2020"]
    fn sign_credential_is_deterministic_for_same_inputs() {
        // Ed25519 signatures over the same JCS bytes with the same key
        // are deterministic — this is what lets us snapshot-test the
        // export bundle (§20.4 survivability).
        let key = test_signing_key("issuer");
        let issuer = Did("did:key:zIssuerTest".into());
        let subject = Did("did:key:zSubjectTest".into());
        let a = sign_credential(
            unsigned_skeleton(issuer.clone(), subject.clone()),
            &key,
            &issuer,
        )
        .unwrap();
        let b = sign_credential(unsigned_skeleton(issuer.clone(), subject), &key, &issuer).unwrap();
        assert_eq!(a.proof.jws, b.proof.jws);
    }

    #[test]
    #[ignore = "pending PR 4 — Ed25519Signature2020"]
    fn sign_credential_preserves_payload_fields() {
        // The signing path MUST NOT mutate non-proof fields — the
        // verifier re-canonicalizes the exact envelope the issuer
        // intended to bind.
        let key = test_signing_key("issuer");
        let issuer = Did("did:key:zIssuerTest".into());
        let subject = Did("did:key:zSubjectTest".into());
        let unsigned = unsigned_skeleton(issuer.clone(), subject.clone());
        let original_id = unsigned.credential.id.clone();
        let signed = sign_credential(unsigned, &key, &issuer).unwrap();
        assert_eq!(signed.id, original_id);
        assert_eq!(signed.issuer, issuer);
        assert_eq!(signed.credential_subject.id, subject);
    }
}
