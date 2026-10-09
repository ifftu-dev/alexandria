//! Sign a credential with a W3C Data Integrity proof, cryptosuite
//! `eddsa-jcs-2022` (VC Data Integrity 1.0; VC-DI-EdDSA 1.0 §3.3).
//!
//! The suite is the standard one that uses JSON Canonicalization (RFC 8785)
//! rather than RDF Dataset Canonicalization, so verifying needs JCS, SHA-256
//! and Ed25519 and nothing else — no JSON-LD processor, no context fetching.
//!
//! Signing input, exactly as the specification states it:
//!
//! ```text
//! proofConfig   = proof options without proofValue, plus the document's @context
//! hashData      = SHA-256(JCS(proofConfig)) || SHA-256(JCS(document without proof))
//! proofValue    = multibase-base58btc( Ed25519-sign(hashData) )
//! ```
//!
//! `proof.verificationMethod` is the issuer's `did:key` with the key's own
//! multibase identifier as the fragment (`did:key:z6Mk…#z6Mk…`), which is the
//! verification method id a `did:key` DID document defines.

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use super::{canonicalize::canonicalize, VcError, VerifiableCredential, EDDSA_JCS_2022};
use crate::did::{derive_did_key, Did, VerificationMethodRef};

/// The unsigned portion of a VC — everything except a completed `proof`.
///
/// The caller constructs this explicitly; `sign_credential` fills the proof
/// options it owns (type, cryptosuite, verification method when blank) and
/// the proof value, and returns the full signed `VerifiableCredential`.
#[derive(Debug, Clone)]
pub struct UnsignedCredential {
    pub credential: VerifiableCredential,
}

/// The 64 bytes an `eddsa-jcs-2022` signature covers.
///
/// Shared with `verify.rs` so issuer and verifier agree bit for bit. The
/// `proofValue` present on `credential`, if any, is ignored: the proof
/// configuration is everything in `proof` except the value.
pub fn hash_data(credential: &VerifiableCredential) -> Result<[u8; 64], VcError> {
    hash_data_for(&serde_json::to_value(credential)?)
}

/// The `eddsa-jcs-2022` hash data for any secured JSON document — a
/// credential or a presentation — carrying `@context` and a `proof`.
pub fn hash_data_for(secured: &serde_json::Value) -> Result<[u8; 64], VcError> {
    let object = secured
        .as_object()
        .ok_or_else(|| VcError::InvalidCredential("document is not an object".into()))?;
    let mut config = object
        .get("proof")
        .cloned()
        .ok_or_else(|| VcError::InvalidCredential("document has no proof".into()))?;
    let options = config
        .as_object_mut()
        .ok_or_else(|| VcError::InvalidCredential("proof is not an object".into()))?;
    options.remove("proofValue");
    options.insert(
        "@context".into(),
        object
            .get("@context")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    );
    let canonical_config = canonicalize(&config)?;

    let mut document = secured.clone();
    document
        .as_object_mut()
        .expect("checked above")
        .remove("proof");
    let canonical_document = canonicalize(&document)?;

    let mut out = [0u8; 64];
    out[..32].copy_from_slice(&Sha256::digest(&canonical_config));
    out[32..].copy_from_slice(&Sha256::digest(&canonical_document));
    Ok(out)
}

/// Sign any JSON document whose `proof` holds complete options (type,
/// cryptosuite, created, verificationMethod, proofPurpose, and for a
/// presentation challenge/domain/expires). Sets `proof.proofValue`.
pub fn sign_document(
    secured: &mut serde_json::Value,
    signing_key: &SigningKey,
) -> Result<(), VcError> {
    if let Some(proof) = secured.get_mut("proof").and_then(|p| p.as_object_mut()) {
        proof.remove("proofValue");
    }
    let signature = signing_key.sign(&hash_data_for(secured)?);
    secured["proof"]["proofValue"] = multibase_base58btc(&signature.to_bytes()).into();
    Ok(())
}

/// Verify the `proofValue` of any secured JSON document against `key`.
/// `Ok(false)` for a malformed or wrong signature.
pub fn verify_document(
    secured: &serde_json::Value,
    key: &ed25519_dalek::VerifyingKey,
) -> Result<bool, VcError> {
    let Some(value) = secured
        .get("proof")
        .and_then(|p| p.get("proofValue"))
        .and_then(|v| v.as_str())
    else {
        return Ok(false);
    };
    let Some(bytes) = multibase_base58btc_decode(value) else {
        return Ok(false);
    };
    let Ok(signature) = ed25519_dalek::Signature::from_slice(&bytes) else {
        return Ok(false);
    };
    Ok(key
        .verify_strict(&hash_data_for(secured)?, &signature)
        .is_ok())
}

/// The verification method for `signing_key` under `issuer`: the issuer's
/// DID with the key's own multibase identifier as the fragment. For an
/// unrotated `did:key` that is `did:key:z6Mk…#z6Mk…`, the method id its DID
/// document defines; for a rotated registry key the controller stays the
/// issuer and the fragment names the new key.
pub fn did_key_verification_method(
    issuer: &Did,
    signing_key: &SigningKey,
) -> VerificationMethodRef {
    let key_did = derive_did_key(signing_key);
    let fragment = key_did.as_str().trim_start_matches("did:key:");
    VerificationMethodRef(format!("{}#{fragment}", issuer.as_str()))
}

/// Sign the unsigned credential with `signing_key`.
///
/// `issuer_did` is embedded as-is: callers are expected to pass the DID
/// that `signing_key` controls (directly, or through a key-registry rotation
/// whose `verificationMethod` they set themselves). When the proof's
/// verification method is blank it is set to the `did:key` method of
/// `signing_key`; when it is already set it is kept, which is what a rotated
/// registry key needs.
pub fn sign_credential(
    mut unsigned: UnsignedCredential,
    signing_key: &SigningKey,
    issuer_did: &Did,
) -> Result<VerifiableCredential, VcError> {
    let vc = &mut unsigned.credential;
    vc.issuer = issuer_did.clone();
    vc.proof.type_ = super::DATA_INTEGRITY_PROOF.into();
    vc.proof.cryptosuite = EDDSA_JCS_2022.into();
    if vc.proof.proof_purpose.is_empty() {
        vc.proof.proof_purpose = "assertionMethod".into();
    }
    // A preset method is kept only when the issuer controls it (a rotated
    // registry key); anything else is replaced by the key's own method.
    let controlled = vc
        .proof
        .verification_method
        .0
        .strip_prefix(issuer_did.as_str())
        .is_some_and(|rest| rest.len() > 1 && rest.starts_with('#'));
    if !controlled {
        vc.proof.verification_method = did_key_verification_method(issuer_did, signing_key);
    }
    vc.proof.proof_value.clear();

    let signature = signing_key.sign(&hash_data(vc)?);
    vc.proof.proof_value = multibase_base58btc(&signature.to_bytes());
    Ok(unsigned.credential)
}

/// Multibase, base58btc alphabet: the `z` prefix the Data Integrity
/// specifications use for `proofValue` and `did:key` identifiers.
pub fn multibase_base58btc(bytes: &[u8]) -> String {
    format!("z{}", bs58::encode(bytes).into_string())
}

/// Decode a multibase base58btc string. `None` for any other multibase
/// prefix or for bytes that are not base58.
pub fn multibase_base58btc_decode(s: &str) -> Option<Vec<u8>> {
    let body = s.strip_prefix('z')?;
    bs58::decode(body).into_vec().ok()
}

/// Unpadded URL-safe base64 per RFC 7515.
///
/// Kept for the non-credential signatures that use a detached JWS shape
/// (`talent`, `course`): a second copy of signature encoding is exactly the
/// kind of thing that drifts apart.
pub fn b64url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Decode an unpadded URL-safe base64 string. Returns `None` on any
/// decode failure — verifiers treat this as a signature mismatch.
pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vc::{Claim, Proof, SkillClaim, VerifiableCredential, DATA_INTEGRITY_PROOF};

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
            provenance: None,
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
                integrity: None,
                proof: Proof::unsigned("2026-04-13T00:00:00Z"),
            },
        }
    }

    #[test]
    fn sign_credential_emits_a_data_integrity_proof() {
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let signed =
            sign_credential(unsigned_skeleton(issuer.clone(), subject), &key, &issuer).unwrap();
        assert_eq!(signed.proof.type_, DATA_INTEGRITY_PROOF);
        assert_eq!(signed.proof.cryptosuite, EDDSA_JCS_2022);
        assert_eq!(signed.proof.proof_purpose, "assertionMethod");
        assert!(signed.proof.proof_value.starts_with('z'));
        assert_eq!(
            multibase_base58btc_decode(&signed.proof.proof_value)
                .unwrap()
                .len(),
            64
        );
        // did:key:z6Mk…#z6Mk…
        let (did, fragment) = signed.proof.verification_method.0.split_once('#').unwrap();
        assert_eq!(did, issuer.as_str());
        assert_eq!(format!("did:key:{fragment}"), issuer.as_str());
    }

    #[test]
    fn the_proof_value_is_ed25519_over_the_two_sha256_hashes() {
        // Reproduce the specification's algorithm by hand so the shared
        // `hash_data` cannot quietly drift from it.
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let signed =
            sign_credential(unsigned_skeleton(issuer.clone(), subject), &key, &issuer).unwrap();
        let mut doc = serde_json::to_value(&signed).unwrap();
        let proof = doc.as_object_mut().unwrap().remove("proof").unwrap();
        let mut config = proof.clone();
        config.as_object_mut().unwrap().remove("proofValue");
        config["@context"] = doc["@context"].clone();
        let mut expected = Vec::new();
        expected.extend_from_slice(&Sha256::digest(canonicalize(&config).unwrap()));
        expected.extend_from_slice(&Sha256::digest(canonicalize(&doc).unwrap()));
        let signature = ed25519_dalek::Signature::from_slice(
            &multibase_base58btc_decode(proof["proofValue"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        key.verifying_key()
            .verify_strict(&expected, &signature)
            .expect("the published algorithm verifies what we signed");
    }

    #[test]
    fn sign_credential_is_deterministic_for_same_inputs() {
        // Ed25519 is deterministic, which is what lets the export bundle be
        // snapshot-tested (§20.4 survivability).
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
        assert_eq!(a.proof.proof_value, b.proof.proof_value);
    }

    #[test]
    fn a_preset_verification_method_is_kept_for_rotated_keys() {
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let mut unsigned = unsigned_skeleton(issuer.clone(), subject);
        unsigned.credential.proof.verification_method =
            VerificationMethodRef(format!("{}#rotated-2", issuer.as_str()));
        let signed = sign_credential(unsigned, &key, &issuer).unwrap();
        assert!(signed.proof.verification_method.0.ends_with("#rotated-2"));
    }

    #[test]
    fn signed_credential_serializes_with_w3c_v2_field_names() {
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let signed =
            sign_credential(unsigned_skeleton(issuer.clone(), subject), &key, &issuer).unwrap();
        let v = serde_json::to_value(&signed).unwrap();
        assert!(v.get("validFrom").is_some(), "missing validFrom in {v}");
        assert!(v.get("credentialSubject").is_some());
        assert_eq!(v["proof"]["type"], "DataIntegrityProof");
        assert_eq!(v["proof"]["cryptosuite"], "eddsa-jcs-2022");
        assert!(v["proof"].get("jws").is_none());
        assert!(v.get("issuance_date").is_none());
    }
}
