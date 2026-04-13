//! Verify a signed VC per spec §13.2 and §22.1.
//!
//! Procedure:
//! 1. Canonicalize the envelope with `proof.jws = ""`.
//! 2. Resolve the issuer DID. For `did:key` this is self-resolving;
//!    if the current DID document doesn't match (e.g. the key was
//!    rotated), fall back to the `key_registry` via `resolve_key_at`.
//! 3. Verify the detached JWS signature.
//! 4. Check subject binding (subject.id is a well-formed DID).
//! 5. Check expiration against `verification_time`.
//! 6. Emit a `VerificationResult`.
//!
//! Revocation (§11.2) is handled by the PR 5 status-list layer;
//! until then `revoked = false`. Integrity anchor (§12.3) is
//! handled by PR 8; until then `integrity_anchored = false`.

use ed25519_dalek::{Signature, VerifyingKey};
use rusqlite::Connection;

use super::sign::{b64url_decode, canonicalize_credential};
use super::{AcceptanceDecision, VerifiableCredential, VerificationPolicy, VerificationResult};
use crate::crypto::did::{parse_did_key, resolve_did_key, resolve_key_at, Did};

/// Verification algorithm per spec §13.2, steps 1–10.
pub fn verify_credential(
    db: &Connection,
    credential: &VerifiableCredential,
    verification_time: &str,
    policy: &VerificationPolicy,
) -> VerificationResult {
    // Default: all flags negative; promote to true as checks pass.
    let mut result = VerificationResult {
        credential_id: credential.id.clone(),
        valid_signature: false,
        issuer_resolved: false,
        revoked: false,
        expired: false,
        subject_bound: false,
        integrity_anchored: false,
        verification_time: verification_time.to_string(),
        acceptance_decision: AcceptanceDecision::Reject,
    };

    // -- subject binding ----------------------------------------------------
    // Subject MUST have a DID identifier. §10: semantic non-transferability.
    result.subject_bound = credential
        .credential_subject
        .id
        .as_str()
        .starts_with("did:");

    // -- expiration ---------------------------------------------------------
    // String comparison on ISO 8601 is well-defined (strings sort lexically
    // the same way the times sort chronologically when all are UTC 'Z').
    if let Some(exp) = &credential.expiration_date {
        if exp.as_str() < verification_time {
            result.expired = true;
        }
    }

    // -- revocation via credential_status -----------------------------------
    // §11.2: each revocable credential MUST provide a resolvable status
    // reference. We look up the referenced status list locally (remote
    // lists land via the PR 9 P2P sync path) and check the bit. If the
    // list isn't known yet, we leave `revoked = false` — strict-mode
    // verifiers can reject on `credentialStatus` presence alone via a
    // future policy flag. This is the conservative default.
    if let Some(status) = &credential.credential_status {
        if let Ok(idx) = status.status_list_index.parse::<i64>() {
            if let Some(bits) = lookup_status_list_bits(db, &status.status_list_credential) {
                let byte = (idx / 8) as usize;
                let bit = (idx % 8) as u8;
                if byte < bits.len() && (bits[byte] & (1 << bit)) != 0 {
                    result.revoked = true;
                }
            }
        }
    }

    // -- issuer resolution --------------------------------------------------
    let issuer_pk = match resolve_issuer_key(db, &credential.issuer, verification_time) {
        Some(pk) => {
            result.issuer_resolved = true;
            pk
        }
        None => return finalize(result, policy),
    };

    // -- signature verification --------------------------------------------
    // Rebuild the canonical bytes with jws emptied, reconstruct signing
    // input, verify.
    result.valid_signature = verify_detached_jws(&issuer_pk, credential).unwrap_or(false);

    finalize(result, policy)
}

/// Resolve an issuer DID to a `VerifyingKey` valid at `at`.
///
/// For `did:key` we first try self-resolution — the current pubkey
/// embedded in the identifier. If the key registry has a historical
/// entry whose window contains `at`, prefer it so that credentials
/// signed under a pre-rotation key still verify after rotation
/// (§5.3).
fn resolve_issuer_key(db: &Connection, issuer: &Did, at: &str) -> Option<VerifyingKey> {
    // Prefer the time-anchored historical entry (§5.3) when present.
    if let Ok(Some(entry)) = resolve_key_at(db, issuer, at) {
        if let Ok(vk) = verifying_key_from_slice(&entry.public_key_bytes) {
            return Some(vk);
        }
    }
    // Fall back to did:key self-resolution. Parse first so an
    // unsupported method short-circuits cleanly.
    parse_did_key(issuer.as_str()).ok()?;
    resolve_did_key(issuer).ok()
}

/// Look up the raw bits of a locally-known status list. Returns
/// `None` if the list isn't in our `credential_status_lists` table —
/// callers treat absence as "not known to be revoked".
fn lookup_status_list_bits(db: &Connection, list_id: &str) -> Option<Vec<u8>> {
    db.query_row(
        "SELECT bits FROM credential_status_lists WHERE list_id = ?1",
        rusqlite::params![list_id],
        |r| r.get::<_, Vec<u8>>(0),
    )
    .ok()
}

fn verifying_key_from_slice(bytes: &[u8]) -> Result<VerifyingKey, String> {
    if bytes.len() != 32 {
        return Err(format!("expected 32 pubkey bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(bytes);
    VerifyingKey::from_bytes(&arr).map_err(|e| format!("bad ed25519 pk: {e}"))
}

/// Verify the detached JWS in `credential.proof.jws` against
/// the canonical bytes of the credential envelope (with proof.jws
/// emptied). Returns `Ok(true)` iff the signature is cryptographically
/// valid; any failure (parse, decode, verify) yields `Ok(false)` —
/// we don't distinguish failure modes at this layer. The `Err`
/// variant is reserved for canonicalization / serde failures on
/// otherwise well-formed inputs.
fn verify_detached_jws(
    issuer_pk: &VerifyingKey,
    credential: &VerifiableCredential,
) -> Result<bool, String> {
    let parts: Vec<&str> = credential.proof.jws.split('.').collect();
    // Detached JWS: header..signature → 3 segments, middle empty.
    if parts.len() != 3 || !parts[1].is_empty() {
        return Ok(false);
    }
    let sig_bytes = match b64url_decode(parts[2]) {
        Some(b) if b.len() == 64 => b,
        _ => return Ok(false),
    };
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let sig = Signature::from_bytes(&sig_arr);

    // Recompute canonical bytes with jws cleared.
    let mut clone = credential.clone();
    clone.proof.jws.clear();
    let canonical = canonicalize_credential(&clone).map_err(|e| format!("canonicalize: {e}"))?;

    // Signing input: header_b64 || '.' || canonical_bytes.
    let mut signing_input = Vec::with_capacity(parts[0].len() + 1 + canonical.len());
    signing_input.extend_from_slice(parts[0].as_bytes());
    signing_input.push(b'.');
    signing_input.extend_from_slice(&canonical);
    Ok(issuer_pk.verify_strict(&signing_input, &sig).is_ok())
}

/// Apply the acceptance predicate (§13.3) and return the result.
fn finalize(mut result: VerificationResult, policy: &VerificationPolicy) -> VerificationResult {
    let accept = result.valid_signature
        && result.issuer_resolved
        && result.subject_bound
        && !result.revoked
        && !(policy.reject_expired && result.expired);
    result.acceptance_decision = if accept {
        AcceptanceDecision::Accept
    } else {
        AcceptanceDecision::Reject
    };
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::{derive_did_key, VerificationMethodRef};
    use crate::db::Database;
    use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
    use crate::domain::vc::{Claim, CredentialSubject, Proof, SkillClaim, VerifiableCredential};
    use ed25519_dalek::SigningKey;

    fn test_signing_key(role: &str) -> SigningKey {
        let mut bytes = [0u8; 32];
        let b = role.as_bytes();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = b[i % b.len().max(1)];
        }
        SigningKey::from_bytes(&bytes)
    }

    fn skeleton(issuer: Did, subject: Did, expiration: Option<String>) -> VerifiableCredential {
        VerifiableCredential {
            context: vec!["https://www.w3.org/2018/credentials/v1".into()],
            id: "urn:uuid:verify-unit-test".into(),
            type_: vec!["VerifiableCredential".into(), "FormalCredential".into()],
            issuer,
            issuance_date: "2026-01-01T00:00:00Z".into(),
            expiration_date: expiration,
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
                created: "2026-01-01T00:00:00Z".into(),
                verification_method: VerificationMethodRef("did:key:z...#key-1".into()),
                proof_purpose: "assertionMethod".into(),
                jws: String::new(),
            },
        }
    }

    fn signed(expiration: Option<String>) -> (Database, VerifiableCredential) {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let vc = sign_credential(
            UnsignedCredential {
                credential: skeleton(issuer.clone(), subject, expiration),
            },
            &key,
            &issuer,
        )
        .unwrap();
        (db, vc)
    }

    #[test]
    fn result_echoes_credential_id_and_verification_time() {
        let (db, vc) = signed(None);
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
    fn well_signed_credential_is_accepted() {
        // Round-trip: sign → verify under default policy → Accept.
        let (db, vc) = signed(None);
        let result = verify_credential(
            db.conn(),
            &vc,
            "2026-04-13T00:00:00Z",
            &VerificationPolicy::default(),
        );
        assert!(result.valid_signature, "sig must verify");
        assert!(result.issuer_resolved);
        assert!(result.subject_bound);
        assert!(!result.expired);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
    }

    #[test]
    fn bad_signature_short_circuits_to_reject() {
        // Acceptance predicate (§13.3): S(c)=0 ⇒ Accept=0.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let mut vc = skeleton(issuer, subject, None);
        vc.proof.jws = "invalid-not-a-jws".into();
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
    fn tampered_payload_breaks_signature() {
        // Altering a claim after signing must invalidate the signature —
        // the whole point of canonical signing is that any change to
        // non-proof fields breaks verification.
        let (db, mut vc) = signed(None);
        if let Claim::Skill(ref mut s) = vc.credential_subject.claim {
            s.score = 1.0;
        }
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
    fn expired_credential_under_strict_policy_is_rejected() {
        let (db, vc) = signed(Some("2026-01-02T00:00:00Z".into()));
        let strict = VerificationPolicy {
            reject_expired: true,
            ..Default::default()
        };
        let result = verify_credential(db.conn(), &vc, "2026-04-13T00:00:00Z", &strict);
        assert!(result.expired);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    fn expired_credential_under_permissive_policy_may_accept() {
        // reject_expired=false: expired flag still set, but signature
        // verifies and decision is Accept.
        let (db, vc) = signed(Some("2026-01-02T00:00:00Z".into()));
        let permissive = VerificationPolicy {
            reject_expired: false,
            ..Default::default()
        };
        let result = verify_credential(db.conn(), &vc, "2026-04-13T00:00:00Z", &permissive);
        assert!(result.expired);
        assert!(result.valid_signature);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
    }

    #[test]
    fn non_did_subject_is_not_subject_bound() {
        // §10: presenter identity must equal subject identity. A bare
        // string id that isn't a DID cannot be bound.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let bogus_subject = Did("not-a-did".into());
        let vc = sign_credential(
            UnsignedCredential {
                credential: skeleton(issuer.clone(), bogus_subject, None),
            },
            &key,
            &issuer,
        )
        .unwrap();
        let result = verify_credential(
            db.conn(),
            &vc,
            "2026-04-13T00:00:00Z",
            &VerificationPolicy::default(),
        );
        assert!(!result.subject_bound);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }
}
