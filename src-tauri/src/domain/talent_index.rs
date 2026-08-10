// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a learner publishes to a talent index, and the consent that governs it.
//!
//! A talent index is an employer-facing directory: enterprises search it to
//! find people with particular verified skills. The index itself is a
//! multi-tenant service that holds data about other people, and lives outside
//! this repository. **This half — the wire schema and the consent rules — is
//! here**, because it decides what leaves a learner's device and they must be
//! able to read it. See `docs/enterprise-boundary.md`: the client is core, the
//! service is not.
//!
//! Nothing here is bound to that service. A record is a signed, self-describing
//! artifact; any index could consume it, including one a learner runs
//! themselves.
//!
//! ## Consent is opt-in, and separate from graph visibility
//!
//! The existing peer-to-peer skill graph (`p2p::graph_fetch`) defaults earned
//! skills to **public**, via `NodePref::default`. That is the right default
//! there: it is a learning network, and being findable as someone who knows
//! Rust is the point.
//!
//! It would be the wrong default here. An employer-facing commercial index is
//! a different audience with different stakes, and reusing the graph preference
//! would mean every existing user's earned skills appeared in it the moment the
//! feature shipped, with nobody having chosen that. So talent-index consent is
//! its own preference, it starts empty, and
//! [`TalentIndexRecord::build`] emits nothing that was not explicitly named.
//!
//! ## The record is signed, and why it is not a presentation
//!
//! A record names a `subjectDid`. Without a signature nothing binds it to the
//! holder of that DID, so anyone could submit one inventing skills for a real
//! person — and the person named would have no way to notice. So a record that
//! leaves the device is signed by the subject's key.
//!
//! It is deliberately **not** a Verifiable Presentation, even though that
//! machinery exists and is tempting. A presentation is *audience-bound*, which
//! is exactly right when showing credentials to one verifier and exactly wrong
//! for a directory listing: a record bound to one index could not be forwarded
//! to an employer. The threat a listing actually faces is an old record being
//! replayed after the learner withdrew consent, and audience binding does
//! nothing about that — [`TalentIndexRecord::valid_until`] does.
//!
//! It is also not a self-issued Verifiable Credential. That would put it in the
//! credentials table and hand it an aggregation weight (`SelfAssertion` carries
//! 0.25), letting a learner's own published claims feed back into their skill
//! scores. A published record must never influence the graph it describes.
//!
//! What it does reuse is the envelope: JCS canonicalization and the same
//! detached Ed25519 JWS shape as a credential proof, so the signature format is
//! one thing to audit rather than two.
//!
//! ## The record is derived, never accumulated
//!
//! The published record is rebuilt from consent every time rather than being
//! stored and amended. Withdrawing consent for a skill removes it from the next
//! record by construction, with no separate deletion path that could be
//! forgotten. Whether an index *honours* a withdrawal is the index's problem;
//! this side's obligation is to stop sending it.

use serde::{Deserialize, Serialize};

/// A learner's consent to appear in a talent index.
///
/// Every field is opt-in. The [`Default`] is "publish nothing", which is also
/// what an absent or unparseable stored value falls back to — a consent record
/// that cannot be read is not consent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TalentIndexConsent {
    /// Skill ids the learner has agreed to publish. A skill absent from this
    /// list is never published, whatever its graph visibility says.
    pub skills: Vec<String>,
    /// Publish the display name. Off by default: a skill list without a name
    /// is far less identifying, and that should be the easy choice.
    pub display_name: bool,
    /// Publish the profile bio.
    pub bio: bool,
}

impl TalentIndexConsent {
    /// Whether `skill_id` may be published.
    pub fn allows_skill(&self, skill_id: &str) -> bool {
        self.skills.iter().any(|s| s == skill_id)
    }

    /// Whether anything at all would be published.
    ///
    /// A record carrying only a DID still discloses that this learner is
    /// *listed*, so "no skills consented" is treated as no publication rather
    /// than an empty listing.
    pub fn publishes_anything(&self) -> bool {
        !self.skills.is_empty()
    }
}

/// One skill as it would appear to an employer.
///
/// Carries the derived level and the evidence behind it, not the raw score.
/// The level is the unit the rest of the product already speaks in, and
/// exposing `raw_score`/`confidence` would invite employers to rank on numbers
/// whose derivation they cannot audit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedSkill {
    pub skill_id: String,
    /// Human-readable name at publication time. Denormalized deliberately: an
    /// index should not have to resolve a taxonomy to render a listing.
    pub name: String,
    /// Derived level, 1–5.
    pub level: u8,
    /// How many independent issuer clusters back this skill. An employer's
    /// most useful single signal, and it cannot be inflated by one issuer
    /// repeatedly attesting.
    pub issuer_clusters: u32,
}

/// Exactly what leaves the device.
///
/// Serialized shape is the wire format. It is intentionally small and flat:
/// everything here is something the learner ticked, and anything a learner
/// cannot see in a preview has no business being in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TalentIndexRecord {
    /// The learner's DID. Always present — a listing has to identify someone,
    /// and this is the identifier an employer later verifies credentials
    /// against.
    pub subject_did: String,
    /// Consented skills, in the order given by the source, deduplicated.
    pub skills: Vec<PublishedSkill>,
    /// Present only with explicit consent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Present only with explicit consent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    /// RFC 3339 instant after which this record must be ignored.
    ///
    /// Required, and inside the signed payload. This is the withdrawal
    /// mechanism: a learner who unticks everything simply stops republishing,
    /// and the record lapses without anyone having to honour a delete request.
    /// An index that ignores expiry is misbehaving in a way a signature cannot
    /// prevent — but a *stale* record cannot be passed off as current.
    pub valid_until: String,
}

/// Detached-JWS proof over a [`TalentIndexRecord`].
///
/// Same shape as a credential's proof so there is one signature format in the
/// system rather than two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordProof {
    #[serde(rename = "type")]
    pub type_: String,
    pub created: String,
    /// The key that signed, as `<did>#key-1`.
    pub verification_method: String,
    /// Detached compact JWS: `header..signature`.
    pub jws: String,
}

/// A record plus its signature — what actually leaves the device.
///
/// The proof sits alongside the payload rather than inside it, so signing needs
/// no "clear the signature field first" dance and the consent preview can show
/// the payload without a JWS blob in the middle of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedTalentIndexRecord {
    pub record: TalentIndexRecord,
    pub proof: RecordProof,
}

/// Why a signed record was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// `subjectDid` is not a resolvable `did:key`.
    UnresolvableSubject,
    /// Signature did not verify against the subject's key.
    BadSignature,
    /// Proof was not the expected shape.
    MalformedProof(String),
    /// The record's term has passed.
    Expired,
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnresolvableSubject => write!(f, "subject DID is not resolvable"),
            Self::BadSignature => write!(f, "signature does not match the subject"),
            Self::MalformedProof(why) => write!(f, "malformed proof: {why}"),
            Self::Expired => write!(f, "record has expired"),
        }
    }
}

/// JWS protected header. Fixed, so it canonicalizes identically for signer and
/// verifier — same constant as the credential path, for the same reason.
const JWS_HEADER_JSON: &str = r#"{"alg":"EdDSA","b64":false,"crit":["b64"]}"#;

/// Bytes that get signed: `header_b64 || '.' || JCS(record)`.
fn signing_input(record: &TalentIndexRecord, header_b64: &str) -> Result<Vec<u8>, String> {
    let value = serde_json::to_value(record).map_err(|e| e.to_string())?;
    let canonical = crate::domain::vc::canonicalize::canonicalize(&value)
        .map_err(|e| format!("canonicalize record: {e}"))?;
    let mut out = Vec::with_capacity(header_b64.len() + 1 + canonical.len());
    out.extend_from_slice(header_b64.as_bytes());
    out.push(b'.');
    out.extend_from_slice(&canonical);
    Ok(out)
}

/// Sign a record with the subject's key.
///
/// Refuses when the key does not derive the DID the record names. Signing a
/// record about somebody else would produce something that verifies against the
/// wrong key and fails at the far end for a confusing reason; better to fail
/// here, where the cause is obvious.
pub fn sign_record(
    record: &TalentIndexRecord,
    signing_key: &ed25519_dalek::SigningKey,
    now: &str,
) -> Result<SignedTalentIndexRecord, String> {
    use ed25519_dalek::Signer;

    let did = crate::crypto::did::derive_did_key(signing_key);
    if did.as_str() != record.subject_did {
        return Err(format!(
            "cannot sign a record for {} with the key for {}",
            record.subject_did,
            did.as_str()
        ));
    }

    let header_b64 = crate::domain::vc::sign::b64url(JWS_HEADER_JSON.as_bytes());
    let input = signing_input(record, &header_b64)?;
    let sig = signing_key.sign(&input);
    let sig_b64 = crate::domain::vc::sign::b64url(&sig.to_bytes());

    Ok(SignedTalentIndexRecord {
        record: record.clone(),
        proof: RecordProof {
            type_: "DataIntegrityProof".into(),
            created: now.to_string(),
            verification_method: format!("{}#key-1", did.as_str()),
            jws: format!("{header_b64}..{sig_b64}"),
        },
    })
}

/// Verify a signed record against the subject DID it names.
///
/// The key comes from the record's own `subjectDid` via `did:key`
/// self-resolution, **not** from `proof.verificationMethod`. Trusting the proof
/// to name its own key would let a forger sign with any key and point the
/// verifier at it — the whole check would become "is this internally
/// consistent", which every forgery is.
pub fn verify_record(
    signed: &SignedTalentIndexRecord,
    verification_time: &str,
) -> Result<(), RecordError> {
    use ed25519_dalek::Verifier;

    if signed.record.valid_until.as_str() <= verification_time {
        return Err(RecordError::Expired);
    }

    let subject = crate::crypto::did::Did(signed.record.subject_did.clone());
    let key = crate::crypto::did::resolve_did_key(&subject)
        .map_err(|_| RecordError::UnresolvableSubject)?;

    let (header_b64, sig_b64) = signed
        .proof
        .jws
        .split_once("..")
        .ok_or_else(|| RecordError::MalformedProof("expected detached compact JWS".into()))?;

    let sig_bytes = crate::domain::vc::sign::b64url_decode(sig_b64)
        .ok_or_else(|| RecordError::MalformedProof("signature is not base64url".into()))?;
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| RecordError::MalformedProof("signature is not 64 bytes".into()))?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_array);

    let input = signing_input(&signed.record, header_b64).map_err(RecordError::MalformedProof)?;

    key.verify(&input, &sig)
        .map_err(|_| RecordError::BadSignature)
}

/// A skill the learner *could* publish, with everything the preview needs.
///
/// Assembled by the caller from local state; kept as a plain input struct so
/// this module has no database dependency and the consent rules can be tested
/// exhaustively without one.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateSkill {
    pub skill_id: String,
    pub name: String,
    pub level: u8,
    pub issuer_clusters: u32,
}

/// The learner's own details, supplied whether or not consent covers them.
///
/// Passed in unconditionally so the filtering happens in one place — here —
/// rather than being duplicated at every call site, where one caller would
/// eventually forget.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProfileFields {
    pub display_name: Option<String>,
    pub bio: Option<String>,
}

impl TalentIndexRecord {
    /// Build the record that consent permits.
    ///
    /// Returns `None` when nothing would be published. That is distinct from an
    /// empty record: publishing a bare DID would still tell an index this
    /// learner exists and is listed, which nobody consented to.
    ///
    /// Skills not named in `consent` are dropped regardless of what
    /// `candidates` contains. The caller is free to pass every earned skill —
    /// filtering is this function's job, and centralising it is what makes the
    /// guarantee testable.
    pub fn build(
        subject_did: &str,
        candidates: &[CandidateSkill],
        profile: &ProfileFields,
        consent: &TalentIndexConsent,
        valid_until: &str,
    ) -> Option<Self> {
        if !consent.publishes_anything() {
            return None;
        }

        let mut skills: Vec<PublishedSkill> = Vec::new();
        for c in candidates {
            if !consent.allows_skill(&c.skill_id) {
                continue;
            }
            // Consent naming the same skill twice must not duplicate a listing.
            if skills.iter().any(|s| s.skill_id == c.skill_id) {
                continue;
            }
            skills.push(PublishedSkill {
                skill_id: c.skill_id.clone(),
                name: c.name.clone(),
                level: c.level,
                issuer_clusters: c.issuer_clusters,
            });
        }

        // Consent can name a skill the learner no longer has — a revoked
        // credential, a renamed taxonomy entry. Nothing is invented to fill the
        // gap; the record carries only what is currently backed by evidence.
        if skills.is_empty() {
            return None;
        }

        Some(Self {
            subject_did: subject_did.to_string(),
            skills,
            display_name: consent
                .display_name
                .then(|| profile.display_name.clone())
                .flatten(),
            bio: consent.bio.then(|| profile.bio.clone()).flatten(),
            valid_until: valid_until.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DID: &str = "did:key:z6MkLearner";
    /// Well past every `NOW` used below, so expiry never accidentally drives a
    /// consent test.
    const TERM: &str = "2027-01-01T00:00:00Z";

    fn candidates() -> Vec<CandidateSkill> {
        vec![
            CandidateSkill {
                skill_id: "skill_rust".into(),
                name: "Rust".into(),
                level: 3,
                issuer_clusters: 2,
            },
            CandidateSkill {
                skill_id: "skill_async".into(),
                name: "Async".into(),
                level: 2,
                issuer_clusters: 1,
            },
        ]
    }

    fn profile() -> ProfileFields {
        ProfileFields {
            display_name: Some("Ada".into()),
            bio: Some("Builds things".into()),
        }
    }

    /// The default must publish nothing. This is the guarantee that makes
    /// shipping the feature safe for every existing user, none of whom has
    /// been asked yet.
    #[test]
    fn default_consent_publishes_nothing() {
        let consent = TalentIndexConsent::default();
        assert!(!consent.publishes_anything());
        assert_eq!(
            TalentIndexRecord::build(DID, &candidates(), &profile(), &consent, TERM),
            None
        );
    }

    /// The core rule: a skill the learner did not tick never leaves the device,
    /// even though it is sitting right there in the candidate list.
    #[test]
    fn only_consented_skills_are_published() {
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            ..Default::default()
        };
        let record =
            TalentIndexRecord::build(DID, &candidates(), &profile(), &consent, TERM).unwrap();

        assert_eq!(record.skills.len(), 1);
        assert_eq!(record.skills[0].skill_id, "skill_rust");
        assert!(
            !record.skills.iter().any(|s| s.skill_id == "skill_async"),
            "an unconsented skill must never appear"
        );
    }

    /// Identity beyond the DID is opt-in separately from skills. Someone may
    /// reasonably want to be findable by capability without being named.
    #[test]
    fn profile_fields_require_their_own_consent() {
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            ..Default::default()
        };
        let record =
            TalentIndexRecord::build(DID, &candidates(), &profile(), &consent, TERM).unwrap();
        assert_eq!(record.display_name, None);
        assert_eq!(record.bio, None);

        let named = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            display_name: true,
            bio: false,
        };
        let record =
            TalentIndexRecord::build(DID, &candidates(), &profile(), &named, TERM).unwrap();
        assert_eq!(record.display_name.as_deref(), Some("Ada"));
        assert_eq!(record.bio, None, "bio was not consented");
    }

    /// Consenting to a name the profile does not have must not invent one.
    #[test]
    fn consent_without_a_value_publishes_nothing_for_that_field() {
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            display_name: true,
            bio: true,
        };
        let empty_profile = ProfileFields::default();
        let record =
            TalentIndexRecord::build(DID, &candidates(), &empty_profile, &consent, TERM).unwrap();
        assert_eq!(record.display_name, None);
        assert_eq!(record.bio, None);
    }

    /// Consent can outlive the evidence — a credential gets revoked, a
    /// taxonomy id is renamed. The record carries only what is currently
    /// backed, and never fabricates the missing entry.
    #[test]
    fn consent_for_a_skill_no_longer_held_publishes_nothing() {
        let consent = TalentIndexConsent {
            skills: vec!["skill_that_was_revoked".into()],
            display_name: true,
            ..Default::default()
        };
        assert_eq!(
            TalentIndexRecord::build(DID, &candidates(), &profile(), &consent, TERM),
            None,
            "a record with no skills must not be published just to carry a name"
        );
    }

    /// Withdrawal works by rebuilding, so there is no separate deletion path
    /// that could be forgotten.
    #[test]
    fn withdrawing_consent_removes_the_skill_from_the_next_record() {
        let both = TalentIndexConsent {
            skills: vec!["skill_rust".into(), "skill_async".into()],
            ..Default::default()
        };
        let before = TalentIndexRecord::build(DID, &candidates(), &profile(), &both, TERM).unwrap();
        assert_eq!(before.skills.len(), 2);

        let withdrawn = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            ..Default::default()
        };
        let after =
            TalentIndexRecord::build(DID, &candidates(), &profile(), &withdrawn, TERM).unwrap();
        assert_eq!(after.skills.len(), 1);
        assert_eq!(after.skills[0].skill_id, "skill_rust");
    }

    #[test]
    fn duplicate_consent_entries_do_not_duplicate_the_listing() {
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into(), "skill_rust".into()],
            ..Default::default()
        };
        let record =
            TalentIndexRecord::build(DID, &candidates(), &profile(), &consent, TERM).unwrap();
        assert_eq!(record.skills.len(), 1);
    }

    /// The wire shape is the contract an index reads. Pinned explicitly so a
    /// field rename is a deliberate, visible change rather than a silent one.
    #[test]
    fn the_wire_shape_is_stable_and_omits_unconsented_fields() {
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            ..Default::default()
        };
        let record =
            TalentIndexRecord::build(DID, &candidates(), &profile(), &consent, TERM).unwrap();
        let json = serde_json::to_value(&record).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "subjectDid": DID,
                "skills": [{
                    "skillId": "skill_rust",
                    "name": "Rust",
                    "level": 3,
                    "issuerClusters": 2,
                }],
                "validUntil": TERM,
            })
        );
        assert!(
            json.get("displayName").is_none(),
            "an unconsented field must be absent, not null — null still says \
             something about the learner"
        );
    }

    /// Raw scores and confidence must not reach the wire. Employers ranking on
    /// numbers whose derivation they cannot audit is the failure mode; the
    /// level and issuer-cluster count are the auditable summary.
    #[test]
    fn the_record_carries_no_raw_scores() {
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            ..Default::default()
        };
        let record =
            TalentIndexRecord::build(DID, &candidates(), &profile(), &consent, TERM).unwrap();
        let json = serde_json::to_string(&record).unwrap();
        for leaked in ["rawScore", "confidence", "trustScore", "evidenceMass"] {
            assert!(!json.contains(leaked), "{leaked} must not reach the wire");
        }
    }

    /// An unreadable stored consent value must fall back to publishing
    /// nothing, not to publishing everything.
    #[test]
    fn malformed_stored_consent_falls_back_to_publishing_nothing() {
        let parsed: TalentIndexConsent =
            serde_json::from_value(serde_json::json!({ "skills": "not-a-list" }))
                .unwrap_or_default();
        assert_eq!(parsed, TalentIndexConsent::default());
        assert!(!parsed.publishes_anything());
    }

    // ---- signing and verification ----------------------------------------

    const SIGN_NOW: &str = "2026-01-01T00:00:00Z";
    const SIGN_TERM: &str = "2026-02-01T00:00:00Z";

    fn learner_key(seed: u8) -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[seed; 32])
    }

    /// A record for whoever holds `key`, signed by it.
    fn signed_for(key: &ed25519_dalek::SigningKey) -> SignedTalentIndexRecord {
        let did = crate::crypto::did::derive_did_key(key);
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            ..Default::default()
        };
        let record =
            TalentIndexRecord::build(did.as_str(), &candidates(), &profile(), &consent, SIGN_TERM)
                .unwrap();
        sign_record(&record, key, SIGN_NOW).unwrap()
    }

    #[test]
    fn a_signed_record_verifies() {
        let signed = signed_for(&learner_key(1));
        assert_eq!(verify_record(&signed, SIGN_NOW), Ok(()));
    }

    /// The reason this module signs at all. Before signing, a record was a
    /// bare claim: anyone could assert skills for any DID and nothing
    /// contradicted them. Forging now requires the subject's private key.
    #[test]
    fn a_record_forged_for_someone_elses_did_does_not_verify() {
        let victim = crate::crypto::did::derive_did_key(&learner_key(1));
        let attacker = learner_key(2);

        // The attacker names the victim and signs with their own key. Signing
        // refuses outright, so they have to assemble it by hand.
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            ..Default::default()
        };
        let record = TalentIndexRecord::build(
            victim.as_str(),
            &candidates(),
            &profile(),
            &consent,
            SIGN_TERM,
        )
        .unwrap();
        assert!(
            sign_record(&record, &attacker, SIGN_NOW).is_err(),
            "signing a record for another DID must be refused at the source"
        );

        // Hand-built: the attacker's signature over the victim's record.
        let mut forged = signed_for(&attacker);
        forged.record.subject_did = victim.as_str().to_string();
        assert_eq!(
            verify_record(&forged, SIGN_NOW),
            Err(RecordError::BadSignature)
        );
    }

    /// The key comes from the record's own subjectDid, never from the proof.
    /// If the proof could name its own key, a forger would sign with any key,
    /// point the verifier at it, and every forgery would verify — the check
    /// would degrade to "is this internally consistent".
    #[test]
    fn a_proof_cannot_nominate_the_key_that_checks_it() {
        let victim = crate::crypto::did::derive_did_key(&learner_key(1));
        let attacker = learner_key(2);
        let attacker_did = crate::crypto::did::derive_did_key(&attacker);

        let mut forged = signed_for(&attacker);
        forged.record.subject_did = victim.as_str().to_string();
        // Proof still honestly points at the attacker's key…
        assert!(forged
            .proof
            .verification_method
            .contains(attacker_did.as_str()));
        // …and it makes no difference, because nothing reads it.
        assert_eq!(
            verify_record(&forged, SIGN_NOW),
            Err(RecordError::BadSignature)
        );
    }

    #[test]
    fn tampering_with_a_skill_breaks_the_signature() {
        let mut signed = signed_for(&learner_key(1));
        signed.record.skills[0].level = 5;
        assert_eq!(
            verify_record(&signed, SIGN_NOW),
            Err(RecordError::BadSignature)
        );
    }

    /// Adding a skill the learner never consented to must not survive either.
    #[test]
    fn appending_a_skill_breaks_the_signature() {
        let mut signed = signed_for(&learner_key(1));
        signed.record.skills.push(PublishedSkill {
            skill_id: "skill_never_consented".into(),
            name: "Not Mine".into(),
            level: 5,
            issuer_clusters: 9,
        });
        assert_eq!(
            verify_record(&signed, SIGN_NOW),
            Err(RecordError::BadSignature)
        );
    }

    /// Expiry is inside the signed payload, so it cannot be extended without
    /// the subject's key. That is what makes it a usable withdrawal mechanism:
    /// an index cannot keep a lapsed record alive by editing the date.
    #[test]
    fn the_term_cannot_be_extended_without_the_key() {
        let mut signed = signed_for(&learner_key(1));
        signed.record.valid_until = "2099-01-01T00:00:00Z".into();
        assert_eq!(
            verify_record(&signed, SIGN_NOW),
            Err(RecordError::BadSignature)
        );
    }

    #[test]
    fn an_expired_record_is_refused() {
        let signed = signed_for(&learner_key(1));
        let after = "2026-03-01T00:00:00Z";
        assert_eq!(verify_record(&signed, after), Err(RecordError::Expired));
    }

    /// Expiry is checked before the signature so a stale record reports the
    /// useful reason rather than passing crypto and failing later.
    #[test]
    fn expiry_is_reported_ahead_of_signature_problems() {
        let mut signed = signed_for(&learner_key(1));
        signed.record.skills[0].level = 5; // also breaks the signature
        let after = "2026-03-01T00:00:00Z";
        assert_eq!(verify_record(&signed, after), Err(RecordError::Expired));
    }

    #[test]
    fn a_malformed_proof_is_reported_as_such_not_as_a_bad_signature() {
        let mut signed = signed_for(&learner_key(1));
        signed.proof.jws = "not-a-jws".into();
        assert!(matches!(
            verify_record(&signed, SIGN_NOW),
            Err(RecordError::MalformedProof(_))
        ));

        let mut short = signed_for(&learner_key(1));
        let (h, _) = short.proof.jws.split_once("..").unwrap();
        short.proof.jws = format!("{h}..{}", crate::domain::vc::sign::b64url(&[0u8; 8]));
        assert!(matches!(
            verify_record(&short, SIGN_NOW),
            Err(RecordError::MalformedProof(_))
        ));
    }

    #[test]
    fn a_subject_that_is_not_a_resolvable_did_key_is_refused() {
        let mut signed = signed_for(&learner_key(1));
        signed.record.subject_did = "did:web:example.com".into();
        assert_eq!(
            verify_record(&signed, SIGN_NOW),
            Err(RecordError::UnresolvableSubject)
        );
    }

    /// Signing must not alter the payload — the bytes a learner previewed are
    /// the bytes that get signed.
    #[test]
    fn signing_leaves_the_payload_untouched() {
        let key = learner_key(1);
        let did = crate::crypto::did::derive_did_key(&key);
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            ..Default::default()
        };
        let record =
            TalentIndexRecord::build(did.as_str(), &candidates(), &profile(), &consent, SIGN_TERM)
                .unwrap();
        let signed = sign_record(&record, &key, SIGN_NOW).unwrap();
        assert_eq!(signed.record, record);
    }
}
