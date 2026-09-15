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
//! 6. Resolve referenced status and local lifecycle state.
//! 7. Emit an accepted, pending, or rejected `VerificationResult`.
//!
//! Missing or unavailable issuer/status evidence produces a typed pending
//! result. A malformed reference or a failed cryptographic/policy check is a
//! rejection.

use ed25519_dalek::{Signature, VerifyingKey};

use super::sign::{b64url_decode, canonicalize_credential};
use super::{
    AcceptanceDecision, VerifiableCredential, VerificationPendingReason, VerificationPolicy,
    VerificationResult,
};
use crate::did::{parse_did_key, resolve_did_key, Did, DidError};
use crate::{StoreLookup, VerificationStore};

/// Verification algorithm per spec §13.2, steps 1–10.
pub fn verify_credential(
    db: &dyn VerificationStore,
    credential: &VerifiableCredential,
    verification_time: &str,
    policy: &VerificationPolicy,
) -> VerificationResult {
    // Default: all flags negative; promote to true as checks pass.
    // VC envelope `id` is optional in W3C VC v2 §4.3 — fall back to
    // empty string for the result echo when absent.
    let credential_id = credential.id.clone().unwrap_or_default();
    let mut result = VerificationResult {
        credential_id: credential_id.clone(),
        valid_signature: false,
        issuer_resolved: false,
        revoked: false,
        status_valid: true,
        expired: false,
        subject_bound: false,
        integrity_anchored: false,
        suspended: false,
        superseded: false,
        verification_time: verification_time.to_string(),
        pending_reasons: Vec::new(),
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
    if let Some(exp) = &credential.valid_until {
        if exp.as_str() < verification_time {
            result.expired = true;
        }
    }

    // -- revocation via credential_status -----------------------------------
    // §11.2: a status-bearing credential is conclusive only when the supplied
    // store can read the referenced bit. Missing/unavailable evidence is a
    // pending result, while malformed or out-of-range indices are invalid.
    if let Some(status) = &credential.credential_status {
        match status.status_list_index.parse::<usize>() {
            Ok(idx) => match db.status_list_bits(&status.status_list_credential) {
                StoreLookup::Found(bits) => {
                    let byte = idx / 8;
                    let bit = (idx % 8) as u8;
                    if byte >= bits.len() {
                        result.status_valid = false;
                    } else if (bits[byte] & (1 << bit)) != 0 {
                        result.revoked = true;
                    }
                }
                StoreLookup::Missing => result
                    .pending_reasons
                    .push(VerificationPendingReason::StatusListMissing),
                StoreLookup::Unavailable => result
                    .pending_reasons
                    .push(VerificationPendingReason::StatusListUnavailable),
            },
            Err(_) => result.status_valid = false,
        }
    }

    // -- suspension (§11.3) -------------------------------------------------
    // Local `credentials.suspended` flag set by `suspend_credential_impl`.
    // `suspended_until` NULL means "suspended indefinitely"; a set value
    // is compared against `verification_time` for automatic reinstatement.
    // Skipped when the envelope has no `id` — local revocation/suspension
    // state is keyed by id and an id-less VC can't have any.
    if !credential_id.is_empty() {
        match db.suspension(&credential_id) {
            StoreLookup::Found((sus_flag, sus_until)) => {
                if sus_flag {
                    let active = match sus_until {
                        Some(until) => until.as_str() > verification_time,
                        None => true,
                    };
                    if active {
                        result.suspended = true;
                    }
                }
            }
            StoreLookup::Missing => {}
            StoreLookup::Unavailable => result
                .pending_reasons
                .push(VerificationPendingReason::SuspensionStateUnavailable),
        }

        // -- supersession (§11.4) -------------------------------------------
        // A newer credential in the local store pointing to this one via
        // `supersedes` marks it as superseded. The match is on id only — the
        // stricter same-subject/same-claim/same-issuer invariant is enforced
        // at the INSERT site (see `supersede_credential_impl`).
        match db.is_superseded(&credential_id) {
            StoreLookup::Found(superseded) => result.superseded = superseded,
            StoreLookup::Missing => {}
            StoreLookup::Unavailable => result
                .pending_reasons
                .push(VerificationPendingReason::SupersessionStateUnavailable),
        }
    }

    // -- issuer resolution --------------------------------------------------
    let issuer_pk = match resolve_issuer_key(db, &credential.issuer, verification_time) {
        IssuerKeyResolution::Found(pk) => {
            result.issuer_resolved = true;
            pk
        }
        IssuerKeyResolution::FoundWithUnavailableStore(pk) => {
            result.issuer_resolved = true;
            result
                .pending_reasons
                .push(VerificationPendingReason::IssuerKeyUnavailable);
            pk
        }
        IssuerKeyResolution::Missing => {
            result
                .pending_reasons
                .push(VerificationPendingReason::IssuerKeyMissing);
            return finalize(result, credential, policy);
        }
        IssuerKeyResolution::Unavailable => {
            result
                .pending_reasons
                .push(VerificationPendingReason::IssuerKeyUnavailable);
            return finalize(result, credential, policy);
        }
        IssuerKeyResolution::Invalid => return finalize(result, credential, policy),
    };

    // -- signature verification --------------------------------------------
    // Rebuild the canonical bytes with jws emptied, reconstruct signing
    // input, verify.
    result.valid_signature = verify_detached_jws(&issuer_pk, credential).unwrap_or(false);

    finalize(result, credential, policy)
}

/// Resolve an issuer DID to a `VerifyingKey` valid at `at`.
///
/// The key registry is consulted first so a historical entry whose window
/// contains `at` can verify credentials across rotation (§5.3). When the
/// registry has no entry, `did:key` falls back to the public key embedded in
/// the identifier.
enum IssuerKeyResolution {
    Found(VerifyingKey),
    FoundWithUnavailableStore(VerifyingKey),
    Missing,
    Unavailable,
    Invalid,
}

fn resolve_issuer_key(db: &dyn VerificationStore, issuer: &Did, at: &str) -> IssuerKeyResolution {
    // Prefer the time-anchored historical entry (§5.3) when present.
    let store_unavailable = match db.key_at(issuer, at) {
        StoreLookup::Found(entry) => {
            return verifying_key_from_slice(&entry.public_key_bytes)
                .map(IssuerKeyResolution::Found)
                .unwrap_or(IssuerKeyResolution::Invalid);
        }
        StoreLookup::Missing => false,
        StoreLookup::Unavailable => true,
    };
    // Fall back to did:key self-resolution. An unsupported DID method can be
    // resolved later by acquiring its key binding; malformed did:key bytes are
    // permanently invalid.
    let resolved = match parse_did_key(issuer.as_str()) {
        Ok(_) => resolve_did_key(issuer),
        Err(DidError::UnsupportedMethod) if store_unavailable => {
            return IssuerKeyResolution::Unavailable;
        }
        Err(DidError::UnsupportedMethod) => return IssuerKeyResolution::Missing,
        Err(DidError::InvalidFormat(_)) => return IssuerKeyResolution::Invalid,
    };
    match resolved {
        Ok(key) if store_unavailable => IssuerKeyResolution::FoundWithUnavailableStore(key),
        Ok(key) => IssuerKeyResolution::Found(key),
        Err(_) => IssuerKeyResolution::Invalid,
    }
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

/// Whether `credential`'s `type` array names a class this policy accepts.
///
/// An empty `allowed_types` means "no type constraint" rather than "accept
/// nothing" — some callers verify a credential whose class they already know
/// from context and have no filtering to do.
///
/// The credential's `type` array always carries `"VerifiableCredential"` plus
/// its class, so this looks for *any* member that is an allowed class. A
/// credential naming several classes is accepted if any one is allowed, which
/// is the same permissive reading the rest of the type handling uses.
fn type_allowed(credential: &VerifiableCredential, policy: &VerificationPolicy) -> bool {
    if policy.allowed_types.is_empty() {
        return true;
    }
    credential
        .type_
        .iter()
        .any(|t| policy.allowed_types.iter().any(|a| a.as_str() == t))
}

/// Apply the acceptance predicate (§13.3) and return the result.
///
/// `allowed_types` is enforced here rather than as its own
/// [`VerificationResult`] field: a disallowed class is a policy refusal, not a
/// defect in the credential, and every per-check flag on the result stays
/// truthfully positive. The caller learns the outcome from
/// `acceptance_decision`.
fn finalize(
    mut result: VerificationResult,
    credential: &VerifiableCredential,
    policy: &VerificationPolicy,
) -> VerificationResult {
    let issuer_evidence_pending = result.pending_reasons.iter().any(|reason| {
        matches!(
            reason,
            VerificationPendingReason::IssuerKeyMissing
                | VerificationPendingReason::IssuerKeyUnavailable
        )
    });
    let valid_or_pending = (result.valid_signature && result.issuer_resolved)
        || (!result.issuer_resolved && issuer_evidence_pending);
    let accept = valid_or_pending
        && result.status_valid
        && result.subject_bound
        && type_allowed(credential, policy)
        && !result.revoked
        && (!policy.require_integrity_anchor || result.integrity_anchored)
        && !(policy.reject_expired && result.expired)
        && !(policy.reject_suspended && result.suspended)
        && !(policy.reject_superseded && result.superseded);
    result.acceptance_decision = if !accept {
        AcceptanceDecision::Reject
    } else if result.pending_reasons.is_empty() {
        AcceptanceDecision::Accept
    } else {
        AcceptanceDecision::Pending
    };
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::did::{derive_did_key, KeyRegistryEntry, VerificationMethodRef};
    use crate::vc::sign::{sign_credential, UnsignedCredential};
    use crate::vc::{
        Claim, CredentialStatus, CredentialType, Proof, SkillClaim, VerifiableCredential,
    };
    use crate::{NullStore, StoreLookup};

    const NOW: &str = "2026-04-13T00:00:00Z";
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
        let claim = Claim::Skill(SkillClaim {
            skill_id: "skill_x".into(),
            level: 3,
            score: 0.65,
            evidence_refs: vec![],
            rubric_version: None,
            assessment_method: None,
            provenance: None,
        });
        VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some("urn:uuid:verify-unit-test".into()),
            type_: vec!["VerifiableCredential".into(), "FormalCredential".into()],
            issuer,
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_until: expiration,
            credential_subject: claim.into_subject(subject),
            credential_status: None,
            terms_of_use: None,
            witness: None,
            integrity: None,
            proof: Proof {
                type_: "Ed25519Signature2020".into(),
                created: "2026-01-01T00:00:00Z".into(),
                verification_method: VerificationMethodRef("did:key:z...#key-1".into()),
                proof_purpose: "assertionMethod".into(),
                jws: String::new(),
            },
        }
    }

    fn signed(expiration: Option<String>) -> (NullStore, VerifiableCredential) {
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
        (NullStore, vc)
    }

    struct UnavailableStore;

    impl VerificationStore for UnavailableStore {
        fn key_at(&self, _did: &Did, _at: &str) -> StoreLookup<KeyRegistryEntry> {
            StoreLookup::Unavailable
        }

        fn status_list_bits(&self, _list_id: &str) -> StoreLookup<Vec<u8>> {
            StoreLookup::Unavailable
        }

        fn suspension(&self, _credential_id: &str) -> StoreLookup<(bool, Option<String>)> {
            StoreLookup::Unavailable
        }

        fn is_superseded(&self, _credential_id: &str) -> StoreLookup<bool> {
            StoreLookup::Unavailable
        }
    }

    /// The default policy omits `EntitlementCredential` on purpose — a
    /// commercial artifact must not pass a capability-credential check. That
    /// only holds if `allowed_types` is actually enforced, so assert it on a
    /// credential that is otherwise perfectly valid.
    #[test]
    fn entitlement_credential_is_refused_by_the_default_policy() {
        let (db, mut vc) = signed(None);
        vc.type_ = vec![
            "VerifiableCredential".into(),
            CredentialType::EntitlementCredential.as_str().to_string(),
        ];
        // Re-sign so the only thing standing between this VC and acceptance
        // is the policy's type list.
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let vc = sign_credential(UnsignedCredential { credential: vc }, &key, &issuer).unwrap();

        let result = verify_credential(&db, &vc, NOW, &VerificationPolicy::default());
        assert!(result.valid_signature, "signature must still be good");
        assert!(result.issuer_resolved);
        assert!(result.subject_bound);
        assert!(!result.revoked && !result.expired);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    fn required_integrity_anchor_is_enforced() {
        let (db, vc) = signed(None);
        let policy = VerificationPolicy {
            require_integrity_anchor: true,
            ..VerificationPolicy::default()
        };
        let result = verify_credential(&db, &vc, NOW, &policy);
        assert!(result.valid_signature);
        assert!(!result.integrity_anchored);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    /// The same credential is accepted when the caller explicitly asks for
    /// entitlements — enforcement is a policy choice, not a blanket ban.
    #[test]
    fn entitlement_credential_is_accepted_under_an_entitlement_policy() {
        let (db, mut vc) = signed(None);
        vc.type_ = vec![
            "VerifiableCredential".into(),
            CredentialType::EntitlementCredential.as_str().to_string(),
        ];
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let vc = sign_credential(UnsignedCredential { credential: vc }, &key, &issuer).unwrap();

        let policy = VerificationPolicy {
            allowed_types: vec![CredentialType::EntitlementCredential],
            ..VerificationPolicy::default()
        };
        let result = verify_credential(&db, &vc, NOW, &policy);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
    }

    /// An empty list means "no type constraint", not "accept nothing" —
    /// callers that already know the class from context rely on this.
    #[test]
    fn empty_allowed_types_imposes_no_type_constraint() {
        let (db, vc) = signed(None);
        let policy = VerificationPolicy {
            allowed_types: vec![],
            ..VerificationPolicy::default()
        };
        let result = verify_credential(&db, &vc, NOW, &policy);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
    }

    #[test]
    fn result_echoes_credential_id_and_verification_time() {
        let (db, vc) = signed(None);
        let result = verify_credential(
            &db,
            &vc,
            "2026-04-13T00:00:00Z",
            &VerificationPolicy::default(),
        );
        assert_eq!(Some(result.credential_id), vc.id);
        assert_eq!(result.verification_time, "2026-04-13T00:00:00Z");
    }

    #[test]
    fn well_signed_credential_is_accepted() {
        // Round-trip: sign → verify under default policy → Accept.
        let (db, vc) = signed(None);
        let result = verify_credential(
            &db,
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
    fn missing_status_list_is_pending_not_active() {
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let mut credential = skeleton(issuer.clone(), subject, None);
        credential.credential_status = Some(CredentialStatus {
            id: "urn:uuid:entry".into(),
            type_: "RevocationList2020Status".into(),
            status_purpose: "revocation".into(),
            status_list_index: "0".into(),
            status_list_credential: "urn:uuid:missing-list".into(),
        });
        let credential = sign_credential(UnsignedCredential { credential }, &key, &issuer).unwrap();

        let result = verify_credential(&NullStore, &credential, NOW, &Default::default());
        assert!(result.valid_signature);
        assert!(!result.revoked);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Pending);
        assert_eq!(
            result.pending_reasons,
            vec![VerificationPendingReason::StatusListMissing]
        );
    }

    #[test]
    fn unavailable_store_is_pending_even_when_did_key_signature_verifies() {
        let (_, credential) = signed(None);
        let result = verify_credential(
            &UnavailableStore,
            &credential,
            NOW,
            &VerificationPolicy::default(),
        );
        assert!(result.valid_signature);
        assert!(result.issuer_resolved);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Pending);
        assert!(result
            .pending_reasons
            .contains(&VerificationPendingReason::IssuerKeyUnavailable));
        assert!(result
            .pending_reasons
            .contains(&VerificationPendingReason::SuspensionStateUnavailable));
        assert!(result
            .pending_reasons
            .contains(&VerificationPendingReason::SupersessionStateUnavailable));
    }

    #[test]
    fn unavailable_external_issuer_lookup_is_distinct_from_a_missing_key() {
        let key = test_signing_key("issuer");
        let issuer = Did("did:web:issuer.example".into());
        let subject = derive_did_key(&test_signing_key("subject"));
        let credential = sign_credential(
            UnsignedCredential {
                credential: skeleton(issuer.clone(), subject, None),
            },
            &key,
            &issuer,
        )
        .unwrap();

        let result = verify_credential(
            &UnavailableStore,
            &credential,
            NOW,
            &VerificationPolicy::default(),
        );
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Pending);
        assert!(result
            .pending_reasons
            .contains(&VerificationPendingReason::IssuerKeyUnavailable));
        assert!(!result
            .pending_reasons
            .contains(&VerificationPendingReason::IssuerKeyMissing));
    }

    #[test]
    fn malformed_or_out_of_range_status_reference_is_rejected() {
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let mut credential = skeleton(issuer.clone(), subject, None);
        credential.credential_status = Some(CredentialStatus {
            id: "urn:uuid:entry".into(),
            type_: "RevocationList2020Status".into(),
            status_purpose: "revocation".into(),
            status_list_index: "9".into(),
            status_list_credential: "urn:uuid:short-list".into(),
        });
        let credential = sign_credential(UnsignedCredential { credential }, &key, &issuer).unwrap();
        struct ShortList;
        impl VerificationStore for ShortList {
            fn key_at(&self, _did: &Did, _at: &str) -> StoreLookup<KeyRegistryEntry> {
                StoreLookup::Missing
            }
            fn status_list_bits(&self, _list_id: &str) -> StoreLookup<Vec<u8>> {
                StoreLookup::Found(vec![0])
            }
            fn suspension(&self, _credential_id: &str) -> StoreLookup<(bool, Option<String>)> {
                StoreLookup::Missing
            }
            fn is_superseded(&self, _credential_id: &str) -> StoreLookup<bool> {
                StoreLookup::Found(false)
            }
        }
        let result = verify_credential(&ShortList, &credential, NOW, &Default::default());
        assert!(!result.status_valid);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    fn bad_signature_short_circuits_to_reject() {
        // Acceptance predicate (§13.3): S(c)=0 ⇒ Accept=0.
        let db = NullStore;
        let key = test_signing_key("issuer");
        let issuer = derive_did_key(&key);
        let subject = derive_did_key(&test_signing_key("subject"));
        let mut vc = skeleton(issuer, subject, None);
        vc.proof.jws = "invalid-not-a-jws".into();
        let result = verify_credential(
            &db,
            &vc,
            "2026-04-13T00:00:00Z",
            &VerificationPolicy::default(),
        );
        assert!(!result.valid_signature);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    fn tampered_payload_breaks_signature() {
        // Altering a claim property after signing must invalidate the
        // signature — the whole point of canonical signing is that any
        // change to non-proof fields breaks verification. The skill
        // claim is stored as inline subject properties, so tamper
        // there directly.
        let (db, mut vc) = signed(None);
        vc.credential_subject
            .properties
            .insert("score".into(), serde_json::json!(1.0));
        let result = verify_credential(
            &db,
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
        let result = verify_credential(&db, &vc, "2026-04-13T00:00:00Z", &strict);
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
        let result = verify_credential(&db, &vc, "2026-04-13T00:00:00Z", &permissive);
        assert!(result.expired);
        assert!(result.valid_signature);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
    }

    #[test]
    fn non_did_subject_is_not_subject_bound() {
        // §10: presenter identity must equal subject identity. A bare
        // string id that isn't a DID cannot be bound.
        let db = NullStore;
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
            &db,
            &vc,
            "2026-04-13T00:00:00Z",
            &VerificationPolicy::default(),
        );
        assert!(!result.subject_bound);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }
}
