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
//! [`build_record`] emits nothing that was not explicitly named.
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
pub use alexandria_verify::talent::*;

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
    pub bloom_level: u8,
    pub score: f64,
    pub confidence: f64,
    pub trust_score: f64,
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

/// Build the record that consent permits.
///
/// A free function rather than a constructor on the type: the record's wire
/// shape lives in `alexandria-verify` so that anyone can verify one, while the
/// rules deciding what a learner is willing to publish are this application's
/// business and stay here.
///
/// Returns `None` when nothing would be published. That is distinct from an
/// empty record: publishing a bare DID would still tell an index this
/// learner exists and is listed, which nobody consented to.
///
/// Skills not named in `consent` are dropped regardless of what
/// `candidates` contains. The caller is free to pass every earned skill —
/// filtering is this function's job, and centralising it is what makes the
/// guarantee testable.
pub fn build_record(
    subject_did: &str,
    candidates: &[CandidateSkill],
    profile: &ProfileFields,
    consent: &TalentIndexConsent,
    valid_until: &str,
) -> Option<TalentIndexRecord> {
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
            bloom_level: c.bloom_level,
        });
    }

    // Consent can name a skill the learner no longer has — a revoked
    // credential, a renamed taxonomy entry. Nothing is invented to fill the
    // gap; the record carries only what is currently backed by evidence.
    if skills.is_empty() {
        return None;
    }

    Some(TalentIndexRecord {
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
                bloom_level: 3,
                score: 0.82,
                confidence: 0.6,
                trust_score: 0.49,
            },
            CandidateSkill {
                skill_id: "skill_async".into(),
                name: "Async".into(),
                level: 2,
                issuer_clusters: 1,
                bloom_level: 3,
                score: 0.82,
                confidence: 0.6,
                trust_score: 0.49,
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
            build_record(DID, &candidates(), &profile(), &consent, TERM),
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
        let record = build_record(DID, &candidates(), &profile(), &consent, TERM).unwrap();

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
        let record = build_record(DID, &candidates(), &profile(), &consent, TERM).unwrap();
        assert_eq!(record.display_name, None);
        assert_eq!(record.bio, None);

        let named = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            display_name: true,
            bio: false,
        };
        let record = build_record(DID, &candidates(), &profile(), &named, TERM).unwrap();
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
        let record = build_record(DID, &candidates(), &empty_profile, &consent, TERM).unwrap();
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
            build_record(DID, &candidates(), &profile(), &consent, TERM),
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
        let before = build_record(DID, &candidates(), &profile(), &both, TERM).unwrap();
        assert_eq!(before.skills.len(), 2);

        let withdrawn = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            ..Default::default()
        };
        let after = build_record(DID, &candidates(), &profile(), &withdrawn, TERM).unwrap();
        assert_eq!(after.skills.len(), 1);
        assert_eq!(after.skills[0].skill_id, "skill_rust");
    }

    #[test]
    fn duplicate_consent_entries_do_not_duplicate_the_listing() {
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into(), "skill_rust".into()],
            ..Default::default()
        };
        let record = build_record(DID, &candidates(), &profile(), &consent, TERM).unwrap();
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
        let record = build_record(DID, &candidates(), &profile(), &consent, TERM).unwrap();
        let json = serde_json::to_value(&record).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "subjectDid": DID,
                "skills": [{
                    "skillId": "skill_rust",
                    "name": "Rust",
                    // Two levels, because they answer different questions:
                    // bloomLevel is the kind of thinking evidenced, level is
                    // how strongly it is evidenced.
                    "bloomLevel": 3,
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
        let record = build_record(DID, &candidates(), &profile(), &consent, TERM).unwrap();
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
            build_record(did.as_str(), &candidates(), &profile(), &consent, SIGN_TERM).unwrap();
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
        let record = build_record(
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
            bloom_level: 3,
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
            build_record(did.as_str(), &candidates(), &profile(), &consent, SIGN_TERM).unwrap();
        let signed = sign_record(&record, &key, SIGN_NOW).unwrap();
        assert_eq!(signed.record, record);
    }
}
