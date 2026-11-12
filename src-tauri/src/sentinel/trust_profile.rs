//! The session's trust profile: chosen by the backend at session start,
//! locked for the session, and folded into the commitment chain before any
//! snapshot.
//!
//! Selection order is the spec's: the session's stored attestation result,
//! the capability report, the device benchmark against each camera
//! profile's budget, then the policy floor and per-capability consent (see
//! [`crate::sentinel::scoring::select_profile`]). Afterwards the profile
//! never changes: losing a signal it needs is a degradation flag
//! ([`shape_to_profile`]), never a downgrade.
//!
//! The policy floor is judged when the profile is chosen (interview-check
//! acceptance), again at attempt start ([`record_policy_floor`]), and once
//! more at issuance, which refuses unless the last recorded judgement and
//! its own agree.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

pub use crate::sentinel::camera_budget::{
    benchmark, BenchmarkResult, CameraBudget, CAMERA_PROFILE_BUDGET, FULL_PROFILE_BUDGET,
};
pub use crate::sentinel::scoring::{
    evaluate_floor, select_profile, BudgetFit, Capabilities, PolicyFloor, ProfileSelection,
    ProfileSignal, SelectionInputs, TrustProfile, WindowFeatures,
};

/// The flag a window carries when its locked profile runs the camera and
/// the window has no camera checks (camera covered or revoked, inference
/// stopped). Never a downgrade: the profile stands and the flag is counted.
pub const CAMERA_DEGRADED: &str = "camera_degraded";

/// What the client reports when asking for a profile.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileRequest {
    /// What the device can provide.
    pub capabilities: Capabilities,
    /// What the learner consented to for this session.
    pub consent: Capabilities,
    /// The sponsor role the attempt is for, whose policy sets the floor.
    /// Assessments only.
    pub role_assessment_id: Option<String>,
}

impl ProfileRequest {
    /// Whether selection needs the device benchmark: only a camera the
    /// learner consented to can make a camera profile selectable.
    pub fn needs_benchmark(&self) -> bool {
        self.capabilities.camera && self.consent.camera
    }
}

/// The stored selection: the choice, its inputs, and the measurement and
/// budgets it was judged against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectionRecord {
    #[serde(flatten)]
    pub selection: ProfileSelection,
    pub benchmark: Option<BenchmarkResult>,
    pub camera_budget: CameraBudget,
    pub full_budget: CameraBudget,
}

/// The reply to the client: the recorded selection plus the policy floor as
/// judged now, so the learner is told before the first item.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SelectedProfile {
    #[serde(flatten)]
    pub record: SelectionRecord,
    pub policy_floor: PolicyFloor,
}

/// The minimum profile a role's issuance policy asks for.
pub(crate) fn role_floor(
    conn: &rusqlite::Connection,
    role_assessment_id: &str,
) -> Result<Option<TrustProfile>, String> {
    let json: Option<String> = conn
        .query_row(
            "SELECT issuance_policy_json FROM role_assessments WHERE id = ?1",
            params![role_assessment_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("role assessment not found")?;
    let Some(json) = json else { return Ok(None) };
    // A policy that does not parse must not become "no floor".
    let policy: crate::credentials::IssuancePolicy =
        serde_json::from_str(&json).map_err(|e| format!("role policy unreadable: {e}"))?;
    Ok(policy.min_trust_profile)
}

/// Choose and lock the session's profile. Refused once a profile is chosen
/// or anything has been committed: the selection is the first link of the
/// commitment chain.
pub(crate) fn select_for_session(
    connection: &rusqlite::Connection,
    session_id: &str,
    req: &ProfileRequest,
    benchmark: Option<&BenchmarkResult>,
) -> Result<SelectedProfile, String> {
    let tx =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    let conn = &tx;
    type SessionRow = (
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
    );
    let (ended_at, locked, root, assurance, purpose): SessionRow = conn
        .query_row(
            "SELECT ended_at, trust_profile, commitment_root, assurance_level, purpose
               FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    if ended_at.is_some() {
        return Err("session has ended".into());
    }
    if let Some(profile) = locked {
        return Err(format!(
            "the trust profile is locked for this session ({profile})"
        ));
    }
    if root.is_some() {
        return Err("the trust profile is chosen before the first snapshot".into());
    }
    let floor = match (&req.role_assessment_id, purpose.as_str()) {
        (Some(_), "interview") => return Err("an interview check has no sponsor role".into()),
        (Some(role), _) => role_floor(conn, role)?,
        (None, _) => None,
    };
    let device_attested = assurance == crate::sentinel::attestation::ASSURANCE_DEVICE_ATTESTED;
    let inputs = SelectionInputs {
        device_attested,
        capabilities: req.capabilities,
        consent: req.consent,
        budget: benchmark.map(BenchmarkResult::fit),
        floor,
    };
    let record = SelectionRecord {
        selection: select_profile(&inputs),
        benchmark: benchmark.copied(),
        camera_budget: CAMERA_PROFILE_BUDGET,
        full_budget: FULL_PROFILE_BUDGET,
    };
    let profile = record.selection.profile;
    let record_json = serde_json::to_string(&record).map_err(|e| e.to_string())?;
    // Tagged so the selection never collides with a snapshot's canonical
    // form; it covers every persisted field of the choice.
    let canonical = format!("trust_profile|{session_id}|{record_json}");
    let root = crate::domain::integrity_commitment::fold_commitment("", canonical.as_bytes());
    let policy_floor = evaluate_floor(
        Some(profile),
        floor,
        &attestation_uncredited(device_attested),
    );
    let floor_json = serde_json::to_string(&policy_floor).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE integrity_sessions SET trust_profile = ?2, profile_selection_json = ?3,
                consent_camera = ?4, consent_microphone = ?5, consent_companion_phone = ?6,
                policy_floor_json = ?7, commitment_root = ?8
          WHERE id = ?1",
        params![
            session_id,
            profile.as_str(),
            record_json,
            req.consent.camera,
            req.consent.microphone,
            req.consent.companion_phone,
            floor_json,
            root,
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(SelectedProfile {
        record,
        policy_floor,
    })
}

/// The session's locked profile; `None` when none was chosen.
pub(crate) fn session_profile(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Option<TrustProfile>, String> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT trust_profile FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    stored
        .map(|p| TrustProfile::parse(&p).ok_or(format!("unknown trust profile '{p}'")))
        .transpose()
}

/// Judge `floor` against the session as it stands and record the result,
/// replacing the previous one. Used at attempt start.
pub(crate) fn record_policy_floor(
    conn: &rusqlite::Connection,
    session_id: &str,
    floor: Option<TrustProfile>,
) -> Result<PolicyFloor, String> {
    let profile = session_profile(conn, session_id)?;
    let assurance: String = conn
        .query_row(
            "SELECT assurance_level FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    // Nothing is committed yet at attempt start, so the camera still counts;
    // issuance judges it again against the committed windows.
    let result = evaluate_floor(
        profile,
        floor,
        &attestation_uncredited(
            assurance == crate::sentinel::attestation::ASSURANCE_DEVICE_ATTESTED,
        ),
    );
    let json = serde_json::to_string(&result).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE integrity_sessions SET policy_floor_json = ?2 WHERE id = ?1",
        params![session_id, json],
    )
    .map_err(|e| e.to_string())?;
    Ok(result)
}

/// The signals that do not count when the attestation is or is not credited.
pub(crate) fn attestation_uncredited(credited: bool) -> Vec<ProfileSignal> {
    if credited {
        Vec::new()
    } else {
        vec![ProfileSignal::DeviceAttestation]
    }
}

/// The policy floor last recorded for the session, if any.
#[cfg(test)]
pub(crate) fn recorded_policy_floor(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Option<PolicyFloor>, String> {
    let json: Option<String> = conn
        .query_row(
            "SELECT policy_floor_json FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    json.map(|j| serde_json::from_str(&j).map_err(|e| e.to_string()))
        .transpose()
}

/// Shape a window's features to the session's profile and return the
/// degradation flags it raises. A signal the profile does not run is
/// dropped ("not run"), so a `core` session never carries camera evidence;
/// a signal it runs that the window lacks is a degradation flag. A session
/// without a profile keeps the window as sent.
pub(crate) fn shape_to_profile(
    profile: Option<TrustProfile>,
    features: &mut WindowFeatures,
) -> Vec<String> {
    let Some(profile) = profile else {
        return Vec::new();
    };
    let mut flags = Vec::new();
    if profile.includes(ProfileSignal::Camera) {
        if features.camera.as_ref().is_none_or(|c| c.face_checks == 0) {
            flags.push(CAMERA_DEGRADED.to_string());
        }
    } else {
        features.camera = None;
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn session_db(purpose: &str) -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute(
                "INSERT INTO integrity_sessions (id, status, purpose) VALUES ('s', 'active', ?1)",
                [purpose],
            )
            .unwrap();
        db
    }

    #[test]
    fn a_device_without_a_camera_or_attestation_runs_core_and_the_choice_is_locked() {
        let db = session_db("assessment");
        let chosen = select_for_session(db.conn(), "s", &ProfileRequest::default(), None).unwrap();
        assert_eq!(chosen.record.selection.profile, TrustProfile::Core);
        assert_eq!(
            chosen.record.selection.not_run,
            vec![ProfileSignal::DeviceAttestation, ProfileSignal::Camera]
        );
        assert_eq!(
            session_profile(db.conn(), "s").unwrap(),
            Some(TrustProfile::Core)
        );

        // A second choice is a renegotiation: refused, profile unchanged.
        let camera = ProfileRequest {
            capabilities: Capabilities {
                camera: true,
                ..Default::default()
            },
            consent: Capabilities {
                camera: true,
                ..Default::default()
            },
            role_assessment_id: None,
        };
        let fast = BenchmarkResult {
            detect_ms: 1.0,
            liveness_ms: 1.0,
            track_ms: 1.0,
        };
        assert!(select_for_session(db.conn(), "s", &camera, Some(&fast))
            .unwrap_err()
            .contains("locked"));
        assert_eq!(
            session_profile(db.conn(), "s").unwrap(),
            Some(TrustProfile::Core)
        );
    }

    const FAST: BenchmarkResult = BenchmarkResult {
        detect_ms: 9.0,
        liveness_ms: 7.0,
        track_ms: 1.0,
    };
    /// Fits `camera` but not `full`.
    const MID: BenchmarkResult = BenchmarkResult {
        detect_ms: 200.0,
        liveness_ms: 30.0,
        track_ms: 1.0,
    };
    const SLOW: BenchmarkResult = BenchmarkResult {
        detect_ms: 900.0,
        liveness_ms: 300.0,
        track_ms: 1.0,
    };

    fn camera_consented() -> ProfileRequest {
        let camera = Capabilities {
            camera: true,
            ..Default::default()
        };
        ProfileRequest {
            capabilities: camera,
            consent: camera,
            role_assessment_id: None,
        }
    }

    fn attest(db: &Database) {
        db.conn()
            .execute(
                "UPDATE integrity_sessions SET assurance_level = 'device_attested' WHERE id = 's'",
                [],
            )
            .unwrap();
    }

    fn excluded(chosen: &SelectedProfile, profile: TrustProfile) -> Vec<ExclusionReason> {
        chosen
            .record
            .selection
            .excluded
            .iter()
            .find(|e| e.profile == profile)
            .map(|e| e.reasons.clone())
            .unwrap_or_default()
    }

    use crate::sentinel::scoring::ExclusionReason;

    #[test]
    fn an_attested_device_that_fits_the_budget_with_camera_consent_runs_full() {
        let db = session_db("assessment");
        attest(&db);
        let chosen = select_for_session(db.conn(), "s", &camera_consented(), Some(&FAST)).unwrap();
        assert_eq!(chosen.record.selection.profile, TrustProfile::Full);
        assert!(chosen.record.selection.not_run.is_empty());
        assert_eq!(chosen.record.benchmark, Some(FAST));

        // The choice and its inputs are the first link of the commitment
        // chain, and the consent is recorded on the session.
        let (root, json, consent_camera, consent_mic): (String, String, bool, bool) = db
            .conn()
            .query_row(
                "SELECT commitment_root, profile_selection_json, consent_camera, consent_microphone
                   FROM integrity_sessions WHERE id = 's'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert!(consent_camera);
        assert!(!consent_mic);
        assert_eq!(
            root,
            crate::domain::integrity_commitment::fold_commitment(
                "",
                format!("trust_profile|s|{json}").as_bytes()
            )
        );
        let stored: SelectionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(stored, chosen.record);
        assert!(stored.selection.inputs.device_attested);
    }

    #[test]
    fn a_device_over_budget_cannot_select_a_camera_profile() {
        let db = session_db("assessment");
        attest(&db);
        let chosen = select_for_session(db.conn(), "s", &camera_consented(), Some(&SLOW)).unwrap();
        assert_eq!(chosen.record.selection.profile, TrustProfile::Attested);
        assert_eq!(
            excluded(&chosen, TrustProfile::Camera),
            vec![ExclusionReason::OverBudget]
        );
        assert_eq!(
            excluded(&chosen, TrustProfile::Full),
            vec![ExclusionReason::OverBudget]
        );

        // A benchmark that never finished is not a pass either.
        let db = session_db("assessment");
        let chosen = select_for_session(db.conn(), "s", &camera_consented(), None).unwrap();
        assert_eq!(chosen.record.selection.profile, TrustProfile::Core);
        assert_eq!(
            excluded(&chosen, TrustProfile::Camera),
            vec![ExclusionReason::NotBenchmarked]
        );
    }

    #[test]
    fn a_camera_without_consent_is_never_used() {
        let db = session_db("assessment");
        let request = ProfileRequest {
            capabilities: Capabilities {
                camera: true,
                microphone: true,
                companion_phone: false,
            },
            ..Default::default()
        };
        assert!(!request.needs_benchmark());
        let chosen = select_for_session(db.conn(), "s", &request, Some(&FAST)).unwrap();
        assert_eq!(chosen.record.selection.profile, TrustProfile::Core);
        assert_eq!(
            excluded(&chosen, TrustProfile::Camera),
            vec![ExclusionReason::CameraNotConsented]
        );
        assert_eq!(
            excluded(&chosen, TrustProfile::Full),
            vec![
                ExclusionReason::NotAttested,
                ExclusionReason::CameraNotConsented
            ]
        );
    }

    #[test]
    fn the_floor_picks_the_highest_permitted_profile_that_meets_it() {
        let db = session_db("assessment");
        attest(&db);
        db.conn()
            .execute_batch(
                "INSERT INTO organizations (id, name, owner_address) VALUES ('org', 'Acme', 'stake');
                 INSERT INTO role_assessments (id, org_id, role_title, issuance_policy_json, status)
                 VALUES ('role', 'org', 'Dev', '{\"min_trust_profile\":\"attested\"}', 'published');",
            )
            .unwrap();
        // `camera` fits and outranks `attested`, but does not run the
        // attestation the floor asks for.
        let request = ProfileRequest {
            role_assessment_id: Some("role".into()),
            ..camera_consented()
        };
        let chosen = select_for_session(db.conn(), "s", &request, Some(&MID)).unwrap();
        assert_eq!(chosen.record.selection.profile, TrustProfile::Attested);
        assert!(chosen.policy_floor.satisfied);
        assert_eq!(chosen.policy_floor.required, Some(TrustProfile::Attested));
        assert_eq!(
            recorded_policy_floor(db.conn(), "s").unwrap(),
            Some(chosen.policy_floor)
        );
    }

    #[test]
    fn the_profile_is_chosen_before_the_first_snapshot_or_not_at_all() {
        let db = session_db("assessment");
        crate::sentinel::snapshots::persist_snapshot(
            db.conn(),
            &crate::sentinel::snapshots::SubmitSnapshotRequest::from_features(
                "s",
                "element",
                WindowFeatures::default(),
            ),
        )
        .unwrap();
        assert!(
            select_for_session(db.conn(), "s", &ProfileRequest::default(), None)
                .unwrap_err()
                .contains("before the first snapshot")
        );
        assert_eq!(session_profile(db.conn(), "s").unwrap(), None);
    }

    fn submit(
        db: &Database,
        features: WindowFeatures,
    ) -> crate::sentinel::snapshots::IntegritySnapshot {
        crate::sentinel::snapshots::persist_snapshot(
            db.conn(),
            &crate::sentinel::snapshots::SubmitSnapshotRequest::from_features(
                "s", "element", features,
            ),
        )
        .unwrap()
    }

    fn camera_window(face_checks: u32) -> WindowFeatures {
        WindowFeatures {
            camera: Some(crate::sentinel::scoring::CameraFeatures {
                face_present: Some(true),
                face_count: Some(1),
                face_checks,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn losing_the_camera_under_a_camera_profile_is_a_degradation_never_a_downgrade() {
        let db = session_db("assessment");
        select_for_session(db.conn(), "s", &camera_consented(), Some(&FAST)).unwrap();

        let healthy = submit(&db, camera_window(5));
        assert!(
            healthy.anomaly_flags.is_empty(),
            "{:?}",
            healthy.anomaly_flags
        );
        assert_eq!(healthy.session_status.as_deref(), Some("active"));

        // Camera revoked: the window arrives without camera features.
        let revoked = submit(&db, WindowFeatures::default());
        assert_eq!(revoked.anomaly_flags, vec![CAMERA_DEGRADED.to_string()]);
        assert_eq!(revoked.session_status.as_deref(), Some("review"));
        assert_eq!(revoked.fusion.unwrap().profile, "camera");

        // Inference stopped: camera group present, no checks ran.
        let stalled = submit(&db, camera_window(0));
        assert!(stalled.anomaly_flags.contains(&CAMERA_DEGRADED.to_string()));

        assert_eq!(
            session_profile(db.conn(), "s").unwrap(),
            Some(TrustProfile::Camera)
        );
    }

    #[test]
    fn a_profile_without_the_camera_records_no_camera_evidence() {
        let db = session_db("assessment");
        select_for_session(db.conn(), "s", &ProfileRequest::default(), None).unwrap();
        let window = submit(&db, camera_window(5));
        assert_eq!(window.camera_score, None, "camera is not run under core");
        assert!(window.anomaly_flags.is_empty());
        assert_eq!(window.fusion.unwrap().profile, "core");
    }

    #[test]
    fn an_interview_check_records_its_floor_at_acceptance_and_takes_no_role() {
        let db = session_db("interview");
        let request = ProfileRequest {
            role_assessment_id: Some("role".into()),
            ..Default::default()
        };
        assert!(select_for_session(db.conn(), "s", &request, None).is_err());
        let chosen = select_for_session(db.conn(), "s", &ProfileRequest::default(), None).unwrap();
        assert_eq!(
            recorded_policy_floor(db.conn(), "s").unwrap(),
            Some(PolicyFloor {
                required: None,
                profile: Some(TrustProfile::Core),
                satisfied: true,
                missing: vec![],
            })
        );
        assert_eq!(chosen.policy_floor.profile, Some(TrustProfile::Core));
    }
}
