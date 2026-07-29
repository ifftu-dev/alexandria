// SPDX-License-Identifier: MIT
//! What a learner publishes to a talent index, and the consent that governs it.
//!
//! A talent index is an employer-facing directory: enterprises search it to
//! find people with particular verified skills. The index itself is a
//! multi-tenant server-side product and therefore enterprise-licensed, but
//! **this half is deliberately MIT** — the wire schema and the consent rules
//! decide what leaves a learner's device, and a learner must be able to read
//! that without enterprise sources. See `docs/enterprise-boundary.md`:
//! *"Talent-index consent UI and publish client — MIT — learner must audit what
//! leaves their device."*
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DID: &str = "did:key:z6MkLearner";

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
            TalentIndexRecord::build(DID, &candidates(), &profile(), &consent),
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
        let record = TalentIndexRecord::build(DID, &candidates(), &profile(), &consent).unwrap();

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
        let record = TalentIndexRecord::build(DID, &candidates(), &profile(), &consent).unwrap();
        assert_eq!(record.display_name, None);
        assert_eq!(record.bio, None);

        let named = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            display_name: true,
            bio: false,
        };
        let record = TalentIndexRecord::build(DID, &candidates(), &profile(), &named).unwrap();
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
            TalentIndexRecord::build(DID, &candidates(), &empty_profile, &consent).unwrap();
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
            TalentIndexRecord::build(DID, &candidates(), &profile(), &consent),
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
        let before = TalentIndexRecord::build(DID, &candidates(), &profile(), &both).unwrap();
        assert_eq!(before.skills.len(), 2);

        let withdrawn = TalentIndexConsent {
            skills: vec!["skill_rust".into()],
            ..Default::default()
        };
        let after = TalentIndexRecord::build(DID, &candidates(), &profile(), &withdrawn).unwrap();
        assert_eq!(after.skills.len(), 1);
        assert_eq!(after.skills[0].skill_id, "skill_rust");
    }

    #[test]
    fn duplicate_consent_entries_do_not_duplicate_the_listing() {
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into(), "skill_rust".into()],
            ..Default::default()
        };
        let record = TalentIndexRecord::build(DID, &candidates(), &profile(), &consent).unwrap();
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
        let record = TalentIndexRecord::build(DID, &candidates(), &profile(), &consent).unwrap();
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
        let record = TalentIndexRecord::build(DID, &candidates(), &profile(), &consent).unwrap();
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
}
