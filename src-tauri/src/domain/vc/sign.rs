//! Sign a VC payload with Ed25519Signature2020 using detached JWS.
//!
//! Detached JWS is the encoding used by the W3C VC Data Integrity
//! proof suite for 2020-style `Ed25519Signature2020`: the payload is
//! *not* embedded in the JWS compact form — only the header and the
//! signature — so the signed JSON-LD stays human-readable. The
//! `jws` field looks like `BASE64URL(header)..BASE64URL(sig)` with
//! an empty middle segment (two consecutive dots).
//!
//! Signing input (per RFC 7797):
//!   `ASCII(BASE64URL(header)) || '.' || JCS_canonical_bytes(credential)`
//!
//! where `credential` is the full VC envelope with `proof.jws = ""`.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};

use super::{canonicalize::canonicalize, VcError, VerifiableCredential};
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

/// Protected JWS header for Ed25519 detached signatures.
/// Fixed JSON — no per-sign variation — so it canonicalizes
/// deterministically for both issuer and verifier.
const JWS_HEADER_JSON: &str = r#"{"alg":"EdDSA","b64":false,"crit":["b64"]}"#;

/// Sign the unsigned credential with `signing_key`. The produced
/// `jws` encodes the Ed25519 signature over the JCS-canonical bytes
/// of the full credential envelope with `proof.jws` held empty. The
/// `issuer_did` is embedded in the credential as-is — callers are
/// expected to ensure it matches `derive_did_key(signing_key)`,
/// though this function does not enforce that so that signing works
/// for test DIDs that are not self-resolving.
pub fn sign_credential(
    mut unsigned: UnsignedCredential,
    signing_key: &SigningKey,
    issuer_did: &Did,
) -> Result<VerifiableCredential, VcError> {
    // Normalise the envelope before canonicalization.
    unsigned.credential.issuer = issuer_did.clone();
    unsigned.credential.proof.jws.clear();

    let canonical_bytes = canonicalize_credential(&unsigned.credential)?;
    let header_b64 = b64url(JWS_HEADER_JSON.as_bytes());

    // Signing input: `header_b64 || '.' || canonical_bytes`.
    let mut signing_input = Vec::with_capacity(header_b64.len() + 1 + canonical_bytes.len());
    signing_input.extend_from_slice(header_b64.as_bytes());
    signing_input.push(b'.');
    signing_input.extend_from_slice(&canonical_bytes);
    let sig = signing_key.sign(&signing_input);
    let sig_b64 = b64url(&sig.to_bytes());

    // Detached compact JWS: middle segment is empty.
    unsigned.credential.proof.jws = format!("{header_b64}..{sig_b64}");
    Ok(unsigned.credential)
}

/// Render a VC to its JCS-canonical bytes. Shared with `verify.rs`
/// so issuer and verifier bit-for-bit agree.
pub(super) fn canonicalize_credential(vc: &VerifiableCredential) -> Result<Vec<u8>, VcError> {
    let value = serde_json::to_value(vc)?;
    canonicalize(&value)
}

/// Unpadded URL-safe base64 per RFC 7515.
pub(super) fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Decode an unpadded URL-safe base64 string. Returns `None` on any
/// decode failure — verifiers treat this as a signature mismatch.
pub(super) fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::derive_did_key;
    use crate::domain::vc::{Claim, Proof, SkillClaim, VerifiableCredential};

    fn test_signing_key(role: &str) -> SigningKey {
        let mut bytes = [0u8; 32];
        let b = role.as_bytes();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = b[i % b.len().max(1)];
        }
        SigningKey::from_bytes(&bytes)
    }

    fn unsigned_skeleton(issuer: Did, subject: Did) -> UnsignedCredential {
        let claim = Claim::Skill(SkillClaim {
            skill_id: "skill_x".into(),
            level: 3,
            score: 0.65,
            evidence_refs: vec![],
            rubric_version: None,
            assessment_method: None,
        });
        UnsignedCredential {
            credential: VerifiableCredential {
                context: vec!["https://www.w3.org/ns/credentials/v2".into()],
                id: Some("urn:uuid:sign-unit-test".into()),
                type_: vec!["VerifiableCredential".into(), "FormalCredential".into()],
                issuer,
                valid_from: "2026-04-13T00:00:00Z".into(),
                valid_until: None,
                credential_subject: claim.into_subject(subject),
                credential_status: None,
                terms_of_use: None,
                witness: None,
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
    fn sign_credential_populates_proof_jws() {
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let signed =
            sign_credential(unsigned_skeleton(issuer.clone(), subject), &key, &issuer).unwrap();
        // Proof JWS must be populated after signing (was empty pre-sign).
        assert!(!signed.proof.jws.is_empty());
        // Detached JWS: header..signature, middle segment empty.
        assert!(signed.proof.jws.contains(".."));
        assert_eq!(signed.proof.type_, "Ed25519Signature2020");
        assert_eq!(signed.proof.proof_purpose, "assertionMethod");
    }

    #[test]
    fn sign_credential_is_deterministic_for_same_inputs() {
        // Ed25519 signatures over the same JCS bytes with the same key
        // are deterministic — this is what lets us snapshot-test the
        // export bundle (§20.4 survivability).
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
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
    fn sign_credential_preserves_payload_fields() {
        // The signing path MUST NOT mutate non-proof fields — the
        // verifier re-canonicalizes the exact envelope the issuer
        // intended to bind.
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let unsigned = unsigned_skeleton(issuer.clone(), subject.clone());
        let original_id = unsigned.credential.id.clone();
        let signed = sign_credential(unsigned, &key, &issuer).unwrap();
        assert_eq!(signed.id, original_id);
        assert_eq!(signed.issuer, issuer);
        assert_eq!(signed.credential_subject.id, subject);
    }

    #[test]
    fn signed_credential_serializes_with_w3c_v2_field_names() {
        // The on-disk JSON MUST use W3C VC v2 keys: validFrom (not
        // issuance_date), credentialSubject (not credential_subject).
        // External verifiers parsing the bundle rely on this shape.
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let signed =
            sign_credential(unsigned_skeleton(issuer.clone(), subject), &key, &issuer).unwrap();
        let v = serde_json::to_value(&signed).unwrap();
        assert!(v.get("validFrom").is_some(), "missing validFrom in {v}");
        assert!(v.get("credentialSubject").is_some());
        assert!(v.get("issuance_date").is_none());
        assert!(v.get("credential_subject").is_none());
    }
}
