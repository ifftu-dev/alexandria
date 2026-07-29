// SPDX-License-Identifier: MIT
//! IPC for the talent-index consent and publish client.
//!
//! MIT and registered unconditionally. The index that *receives* a record is an
//! enterprise product, but choosing what to send it is a learner's decision
//! about their own data, and the code making that decision has to be readable
//! by the person it affects. See `docs/enterprise-boundary.md`.
//!
//! Nothing here publishes anything yet — there is no index to publish to. What
//! it does provide is the whole decision surface: the candidate skills, the
//! consent record, and a byte-exact preview of what would leave the device.
//! Building this first means the wire schema is fixed from the auditable side
//! rather than back-derived from whatever a server happens to accept.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::domain::talent_index::{
    CandidateSkill, ProfileFields, TalentIndexConsent, TalentIndexRecord,
};
use crate::settings::registry::{keys, JsonSetting};
use crate::settings::SettingsStore;
use crate::AppState;

/// Everything the consent UI needs in one call.
///
/// Candidates and the resulting record are returned together so the preview
/// can never disagree with the checkboxes above it — two round trips could
/// interleave with a consent change and show a record that was never real.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TalentIndexPreview {
    /// Every skill the learner could publish, consented or not.
    pub candidates: Vec<PreviewCandidate>,
    /// Current consent.
    pub consent: TalentIndexConsent,
    /// Exactly what would be published, or `null` when nothing would be.
    pub record: Option<TalentIndexRecord>,
}

/// A candidate skill plus whether consent currently covers it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCandidate {
    pub skill_id: String,
    pub name: String,
    pub level: u8,
    pub issuer_clusters: u32,
    /// Whether this skill is currently consented. Derived rather than stored,
    /// so it cannot drift from the consent record itself.
    pub consented: bool,
}

/// Read stored consent, falling back to "publish nothing".
///
/// A consent record that will not parse is not consent. Anything unreadable —
/// a partial write, a downgrade that wrote an older shape — resolves to the
/// default rather than to whatever the malformed value might have meant.
fn load_consent(conn: &Connection) -> TalentIndexConsent {
    let raw = SettingsStore::get(conn, keys::TALENT_INDEX_CONSENT).0;
    serde_json::from_value(raw).unwrap_or_default()
}

/// Skills the learner could publish: their derived skill states, named.
///
/// Sourced from `derived_skill_states` rather than raw credentials because a
/// derived state is what the product means by "has this skill" — it already
/// folds in issuer diversity, revocation and evidence weighting. Publishing
/// straight from credentials would let one issuer's repeated attestations look
/// like corroboration.
fn candidate_skills(conn: &Connection, subject_did: &str) -> Result<Vec<CandidateSkill>, String> {
    let mut stmt = conn
        .prepare(
            // One row per skill. `derived_skill_states` is keyed by
            // (subject, skill, calculation_version), so a subject rescored
            // under a new version has several rows per skill and a naive
            // select would list the same skill twice. GROUP BY with MAX picks
            // the newest — SQLite guarantees the bare columns come from the
            // row that produced the max.
            "SELECT d.skill_id, COALESCE(s.name, d.skill_id), d.level, \
                    d.unique_issuer_clusters, MAX(d.computed_at) \
             FROM derived_skill_states d \
             LEFT JOIN skills s ON s.id = d.skill_id \
             WHERE d.subject_did = ?1 \
             GROUP BY d.skill_id \
             ORDER BY d.level DESC, d.skill_id ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([subject_did], |r| {
            Ok(CandidateSkill {
                skill_id: r.get(0)?,
                name: r.get(1)?,
                level: r.get::<_, i64>(2)? as u8,
                issuer_clusters: r.get::<_, i64>(3)? as u32,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// The learner's own profile fields, whether or not consent covers them.
///
/// Filtering happens in [`TalentIndexRecord::build`], in one place, rather than
/// here — a second filter would be a second thing to get wrong.
fn profile_fields(conn: &Connection) -> ProfileFields {
    conn.query_row(
        "SELECT display_name, bio FROM local_identity WHERE id = 1",
        [],
        |r| {
            Ok(ProfileFields {
                display_name: r.get(0)?,
                bio: r.get(1)?,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or_default()
}

pub fn get_talent_index_preview_impl(
    conn: &Connection,
    subject_did: &str,
) -> Result<TalentIndexPreview, String> {
    let consent = load_consent(conn);
    let candidates = candidate_skills(conn, subject_did)?;
    let record =
        TalentIndexRecord::build(subject_did, &candidates, &profile_fields(conn), &consent);

    Ok(TalentIndexPreview {
        candidates: candidates
            .iter()
            .map(|c| PreviewCandidate {
                skill_id: c.skill_id.clone(),
                name: c.name.clone(),
                level: c.level,
                issuer_clusters: c.issuer_clusters,
                consented: consent.allows_skill(&c.skill_id),
            })
            .collect(),
        consent,
        record,
    })
}

/// Replace stored consent wholesale.
///
/// Whole-record rather than per-field toggles: consent is a statement about
/// what the learner agrees to publish *now*, and a partial update leaves open
/// the question of what the unmentioned fields meant. The UI sends the full
/// picture it is showing.
pub fn set_talent_index_consent_impl(
    conn: &Connection,
    consent: &TalentIndexConsent,
) -> Result<(), String> {
    let value = serde_json::to_value(consent).map_err(|e| e.to_string())?;
    SettingsStore::set(conn, keys::TALENT_INDEX_CONSENT, JsonSetting(value))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_talent_index_preview(
    state: State<'_, AppState>,
) -> Result<TalentIndexPreview, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let subject_did = crate::commands::entitlements::local_did(db.conn())
        .ok_or("this profile has no identity yet")?;
    get_talent_index_preview_impl(db.conn(), &subject_did)
}

#[tauri::command]
pub async fn set_talent_index_consent(
    state: State<'_, AppState>,
    consent: TalentIndexConsent,
) -> Result<TalentIndexPreview, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let subject_did = crate::commands::entitlements::local_did(db.conn())
        .ok_or("this profile has no identity yet")?;

    set_talent_index_consent_impl(db.conn(), &consent)?;
    // Return the recomputed preview so the UI shows the record that consent
    // actually produced, rather than the one it predicted.
    get_talent_index_preview_impl(db.conn(), &subject_did)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    const DID: &str = "did:key:z6MkLearner";

    fn open_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    fn seed_skill(db: &Database, skill_id: &str, name: &str, level: i64, clusters: i64) {
        seed_state(
            db,
            skill_id,
            name,
            level,
            clusters,
            "v1",
            "2026-01-01T00:00:00Z",
        );
    }

    /// Insert a taxonomy skill and one derived state for it.
    ///
    /// `skills.subject_id` is a NOT NULL foreign key, so a subject row has to
    /// exist first — the talent index reads through that join for display
    /// names.
    fn seed_state(
        db: &Database,
        skill_id: &str,
        name: &str,
        level: i64,
        clusters: i64,
        version: &str,
        computed_at: &str,
    ) {
        // `skills.subject_id -> subjects.subject_field_id` is a NOT NULL FK
        // chain, so the whole spine has to exist. Unwrapped rather than
        // ignored: a silently failed seed makes the join miss and the display
        // name fall back to the raw id, which reads as a passing test of the
        // wrong thing.
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO subject_fields (id, name) VALUES ('sf_test', 'Field')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO subjects (id, name, subject_field_id) \
                 VALUES ('subj_test', 'Testing', 'sf_test')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO skills (id, name, subject_id) VALUES (?1, ?2, 'subj_test')",
                rusqlite::params![skill_id, name],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO derived_skill_states \
                 (subject_did, skill_id, raw_score, confidence, trust_score, level, \
                  evidence_mass, unique_issuer_clusters, active_evidence_count, \
                  calculation_version, state_json, computed_at) \
                 VALUES (?1, ?2, 0.8, 0.7, 0.56, ?3, 2.0, ?4, 2, ?5, '{}', ?6)",
                rusqlite::params![DID, skill_id, level, clusters, version, computed_at],
            )
            .unwrap();
    }

    /// A fresh profile publishes nothing. Every existing user is in this state
    /// when the feature ships, and none of them has been asked.
    #[test]
    fn a_profile_that_has_not_consented_publishes_nothing() {
        let db = open_db();
        seed_skill(&db, "skill_rust", "Rust", 3, 2);

        let preview = get_talent_index_preview_impl(db.conn(), DID).unwrap();
        assert_eq!(preview.candidates.len(), 1, "the skill is still offered");
        assert!(!preview.candidates[0].consented);
        assert!(
            preview.record.is_none(),
            "nothing may be published without consent"
        );
    }

    /// Consent is honoured, and only for what it names.
    #[test]
    fn consent_publishes_exactly_what_it_names() {
        let db = open_db();
        seed_skill(&db, "skill_rust", "Rust", 3, 2);
        seed_skill(&db, "skill_async", "Async", 2, 1);

        set_talent_index_consent_impl(
            db.conn(),
            &TalentIndexConsent {
                skills: vec!["skill_rust".into()],
                ..Default::default()
            },
        )
        .unwrap();

        let preview = get_talent_index_preview_impl(db.conn(), DID).unwrap();
        let record = preview.record.expect("consented, so a record exists");
        assert_eq!(record.skills.len(), 1);
        assert_eq!(record.skills[0].name, "Rust");

        let async_candidate = preview
            .candidates
            .iter()
            .find(|c| c.skill_id == "skill_async")
            .unwrap();
        assert!(!async_candidate.consented);
    }

    /// Consent survives a round trip through the settings store. It is stored
    /// as JSON, and a shape change that broke this would silently reset every
    /// learner's choices to "publish nothing" — safe, but a betrayal of the
    /// choice they made.
    #[test]
    fn consent_round_trips_through_storage() {
        let db = open_db();
        let consent = TalentIndexConsent {
            skills: vec!["skill_rust".into(), "skill_async".into()],
            display_name: true,
            bio: false,
        };
        set_talent_index_consent_impl(db.conn(), &consent).unwrap();
        assert_eq!(load_consent(db.conn()), consent);
    }

    /// Stored garbage must resolve to "publish nothing", never to "publish".
    #[test]
    fn unreadable_stored_consent_publishes_nothing() {
        let db = open_db();
        seed_skill(&db, "skill_rust", "Rust", 3, 2);
        SettingsStore::set(
            db.conn(),
            keys::TALENT_INDEX_CONSENT,
            JsonSetting(serde_json::json!("this is not a consent record")),
        )
        .unwrap();

        assert_eq!(load_consent(db.conn()), TalentIndexConsent::default());
        let preview = get_talent_index_preview_impl(db.conn(), DID).unwrap();
        assert!(preview.record.is_none());
    }

    /// Withdrawal takes effect immediately in the next preview — there is no
    /// separate deletion step that could be skipped.
    #[test]
    fn withdrawing_consent_empties_the_record() {
        let db = open_db();
        seed_skill(&db, "skill_rust", "Rust", 3, 2);

        set_talent_index_consent_impl(
            db.conn(),
            &TalentIndexConsent {
                skills: vec!["skill_rust".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert!(get_talent_index_preview_impl(db.conn(), DID)
            .unwrap()
            .record
            .is_some());

        set_talent_index_consent_impl(db.conn(), &TalentIndexConsent::default()).unwrap();
        assert!(get_talent_index_preview_impl(db.conn(), DID)
            .unwrap()
            .record
            .is_none());
    }

    /// The graph-visibility preference must not leak into this decision.
    ///
    /// `instructor.graph_prefs` defaults earned skills to public for
    /// peer-to-peer discovery. If that ever came to imply talent-index consent,
    /// every existing user would be published to employers without asking.
    #[test]
    fn public_graph_visibility_does_not_imply_talent_index_consent() {
        let db = open_db();
        seed_skill(&db, "skill_rust", "Rust", 3, 2);

        SettingsStore::set(
            db.conn(),
            keys::INSTRUCTOR_GRAPH_PREFS,
            JsonSetting(serde_json::json!({
                "skill_rust": { "public": true, "teaching": true }
            })),
        )
        .unwrap();

        let preview = get_talent_index_preview_impl(db.conn(), DID).unwrap();
        assert!(
            preview.record.is_none(),
            "a publicly visible skill is still not consented for the talent index"
        );
    }

    /// A subject rescored under a newer calculation version has several rows
    /// per skill. The listing must show the skill once, at its newest level —
    /// a duplicated entry would be visible to an employer and simply wrong.
    #[test]
    fn a_rescored_skill_is_listed_once_at_its_newest_level() {
        let db = open_db();
        seed_state(
            &db,
            "skill_rust",
            "Rust",
            2,
            1,
            "v1",
            "2026-01-01T00:00:00Z",
        );
        seed_state(
            &db,
            "skill_rust",
            "Rust",
            4,
            3,
            "v2",
            "2026-06-01T00:00:00Z",
        );

        let preview = get_talent_index_preview_impl(db.conn(), DID).unwrap();
        assert_eq!(preview.candidates.len(), 1, "one row per skill");
        assert_eq!(preview.candidates[0].level, 4, "the newest score wins");
        assert_eq!(preview.candidates[0].issuer_clusters, 3);
    }

    /// A skill with no taxonomy row still lists, under its id. Losing a skill
    /// from a listing because a name lookup missed would be worse than showing
    /// a raw id.
    #[test]
    fn a_skill_missing_from_the_taxonomy_falls_back_to_its_id() {
        let db = open_db();
        db.conn()
            .execute(
                "INSERT INTO derived_skill_states \
                 (subject_did, skill_id, raw_score, confidence, trust_score, level, \
                  evidence_mass, unique_issuer_clusters, active_evidence_count, \
                  calculation_version, state_json, computed_at) \
                 VALUES (?1, 'skill_orphan', 0.8, 0.7, 0.56, 3, 2.0, 1, 2, 'v1', '{}', \
                         '2026-01-01T00:00:00Z')",
                rusqlite::params![DID],
            )
            .unwrap();

        let preview = get_talent_index_preview_impl(db.conn(), DID).unwrap();
        assert_eq!(preview.candidates.len(), 1);
        assert_eq!(preview.candidates[0].name, "skill_orphan");
    }

    /// Another learner's skills are never candidates.
    #[test]
    fn only_this_subjects_skills_are_offered() {
        let db = open_db();
        seed_skill(&db, "skill_rust", "Rust", 3, 2);

        let preview = get_talent_index_preview_impl(db.conn(), "did:key:z6MkSomeoneElse").unwrap();
        assert!(preview.candidates.is_empty());
        assert!(preview.record.is_none());
    }
}
