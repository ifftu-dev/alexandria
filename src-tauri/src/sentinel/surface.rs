//! What a credential says about its integrity session, derived from the
//! profile database at issuance: the locked trust profile, the signals run,
//! degraded and not run, the verification rung, the probe summary and the
//! coverage statement for that profile and rung.
//!
//! Nothing here is copied from client text. The rung's evidence is read from
//! the session and its attempt:
//!
//! - `process`: every served item was observed by the response tracker
//!   (`integrity_sessions.response_items` ≥ items served), the session's
//!   `response_process` channel is not over threshold, and the attempt's
//!   items came from the backend's draw (every attempt's are).
//! - `probed`: probe results exist and all of them were satisfied.
//! - `consistent`: the attempt's outcome agrees with the learner's history
//!   ([`crate::sentinel::consistency::for_attempt`]).
//! - `live`: a satisfied [`STEP_INTERVIEW`] verification in the session.

use rusqlite::{params, OptionalExtension};

use crate::sentinel::scoring::{
    coverage_statement, decide,
    measurement::{Certification, CertifiedSignal},
    verification_rung, CoverageStatement, ProfileSignal, RatioManifest, RungEvidence,
    SessionEvidence, TrustProfile, VerificationRung, DEFAULT_PROFILE,
};
use crate::sentinel::trust_profile::CAMERA_DEGRADED;
use crate::sentinel::verification::{
    session_verification_summary, VerificationOutcome, VerificationResult, STEP_INTERVIEW,
    STEP_PROBE,
};

/// The fusion channel the response-process signals report on.
const RESPONSE_PROCESS_CHANNEL: &str = "response_process";

/// The probe step's results, summarised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    pub outcome: VerificationOutcome,
    pub policies: Vec<String>,
    pub satisfied: u32,
    pub failed: u32,
    pub skipped: u32,
}

/// Everything the assertion and the issuance policy read about a session.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionSurface {
    pub trust_profile: Option<TrustProfile>,
    pub evidence: RungEvidence,
    pub rung: VerificationRung,
    pub signals_run: Vec<String>,
    pub signals_degraded: Vec<String>,
    pub signals_not_run: Vec<String>,
    /// Signals the profile runs that do not count towards a policy floor now.
    pub uncredited: Vec<ProfileSignal>,
    pub probe: Option<ProbeOutcome>,
    /// `None` when the session never chose a profile.
    pub coverage: Option<CoverageStatement>,
}

/// Derive the surface of `session_id`. `attestation_credited` is whether
/// its device attestation verifies now.
pub(crate) fn session_surface(
    conn: &rusqlite::Connection,
    session_id: &str,
    attestation_credited: bool,
    manifest: &RatioManifest,
    certification: Option<(&Certification, &str)>,
) -> Result<SessionSurface, String> {
    let (profile, response_items, purpose): (Option<String>, i64, String) = conn
        .query_row(
            "SELECT trust_profile, response_items, purpose FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let profile = profile
        .map(|p| TrustProfile::parse(&p).ok_or(format!("unknown trust profile '{p}'")))
        .transpose()?;
    let windows = committed_windows(conn, session_id)?;
    let verifications = session_verification_summary(conn, session_id)?.results;

    // Device axis signals.
    let mut uncredited = Vec::new();
    let mut signals_degraded = Vec::new();
    if let Some(p) = profile {
        if p.includes(ProfileSignal::DeviceAttestation) && !attestation_credited {
            uncredited.push(ProfileSignal::DeviceAttestation);
            signals_degraded.push(ProfileSignal::DeviceAttestation.as_str().to_string());
        }
        if let Some(min) = p.min_camera_coverage() {
            let degraded = windows
                .iter()
                .filter(|w| w.iter().any(|f| f == CAMERA_DEGRADED))
                .count();
            // A session with no windows has no camera coverage at all.
            let coverage =
                (!windows.is_empty()).then(|| 1.0 - degraded as f64 / windows.len() as f64);
            if coverage.is_none_or(|c| c < min) {
                uncredited.push(ProfileSignal::Camera);
            }
            if coverage != Some(1.0) {
                signals_degraded.push(ProfileSignal::Camera.as_str().to_string());
            }
        }
    }
    let (signals_run, signals_not_run) = match profile {
        Some(p) => (
            std::iter::once("core")
                .chain(p.signals().iter().map(|s| s.as_str()))
                .map(str::to_string)
                .collect(),
            ProfileSignal::ALL
                .into_iter()
                .filter(|s| !p.includes(*s))
                .map(|s| s.as_str().to_string())
                .collect(),
        ),
        None => (Vec::new(), Vec::new()),
    };

    // Verification axis.
    let attempt = session_attempt(conn, session_id)?;
    let process = match &attempt {
        Some(a) => {
            let evidence = SessionEvidence {
                windows: windows.clone(),
                verification_failed: false,
                interview: purpose == "interview",
            };
            let profile_name = profile.map_or(DEFAULT_PROFILE, |p| p.as_str());
            let fusion = decide(&evidence, manifest, profile_name, true).summary;
            let anomalous = fusion
                .channels
                .iter()
                .any(|c| c.channel == RESPONSE_PROCESS_CHANNEL && c.over_threshold);
            a.served > 0 && response_items >= a.served && !anomalous
        }
        None => false,
    };
    let probe = probe_outcome(&verifications);
    let consistent = match &attempt {
        Some(Attempt {
            id,
            score: Some(score),
            ..
        }) => crate::sentinel::consistency::for_attempt(conn, id, *score)?.satisfied(),
        _ => false,
    };
    let live = verifications
        .iter()
        .any(|r| r.step == STEP_INTERVIEW && r.outcome == VerificationOutcome::Satisfied);
    let evidence = RungEvidence {
        process,
        probed: probe
            .as_ref()
            .is_some_and(|p| p.outcome == VerificationOutcome::Satisfied && p.skipped == 0),
        consistent,
        live,
    };
    let rung = verification_rung(&evidence);

    Ok(SessionSurface {
        trust_profile: profile,
        evidence,
        rung,
        signals_run,
        signals_degraded,
        signals_not_run,
        uncredited,
        probe,
        coverage: profile.map(|p| coverage_statement(p, rung, certification)),
    })
}

/// One signal a session raised, as a sponsor reviewing it sees it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ReviewSignal {
    pub signal: String,
    /// The independent evidence channel it reports on.
    pub channel: String,
    /// Committed windows it fired in.
    pub windows: u32,
    /// No measured ratio under the session's profile: it added nothing to
    /// the posterior and can send a session to review, never decide it.
    pub advisory: bool,
    /// The rates it was certified at, when the certification in force
    /// admits it to the session's profile.
    pub certified: Option<CertifiedSignal>,
}

/// A finished session as a sponsor's review queue ranks it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SessionReview {
    pub session_id: String,
    pub status: String,
    pub trust_profile: Option<TrustProfile>,
    pub rung: VerificationRung,
    /// Summed log-likelihood ratio of every channel: the session's posterior
    /// log-odds of misconduct over the prior. Zero while nothing is measured.
    pub posterior_llr: f64,
    /// The profile's operating threshold per channel; `None` uncertified.
    pub threshold: Option<f64>,
    pub manifest_version: String,
    /// Strongest first: by accumulated ratio, then by windows.
    pub signals: Vec<ReviewSignal>,
}

/// Rescore `session_id` from its committed windows for a sponsor's review:
/// the fusion posterior under `manifest`, and each raised signal with its
/// certified rates under `certification`, or marked advisory.
pub(crate) fn session_review(
    conn: &rusqlite::Connection,
    session_id: &str,
    manifest: &RatioManifest,
    certification: Option<&Certification>,
) -> Result<SessionReview, String> {
    let (status, assurance, purpose): (String, String, String) = conn
        .query_row(
            "SELECT status, assurance_level, purpose FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let surface = session_surface(
        conn,
        session_id,
        assurance == "device_attested",
        manifest,
        None,
    )?;
    let profile = surface
        .trust_profile
        .map_or(DEFAULT_PROFILE, |p| p.as_str());
    let windows = committed_windows(conn, session_id)?;
    let evidence = SessionEvidence {
        windows: windows.clone(),
        verification_failed: crate::sentinel::verification::has_failed_verification(
            conn, session_id,
        )?,
        interview: purpose == "interview",
    };
    let fusion = decide(&evidence, manifest, profile, true).summary;
    let ratios = manifest.profiles.get(profile);
    let certified = certification
        .filter(|c| !c.synthetic)
        .and_then(|c| c.profiles.get(profile));
    let mut signals: Vec<ReviewSignal> = fusion
        .channels
        .iter()
        .flat_map(|c| {
            c.signals.iter().map(|signal| ReviewSignal {
                signal: signal.clone(),
                channel: c.channel.clone(),
                windows: windows.iter().filter(|w| w.contains(signal)).count() as u32,
                advisory: !ratios.is_some_and(|p| p.ratios.contains_key(signal)),
                certified: certified.and_then(|p| p.signals.get(signal)).cloned(),
            })
        })
        .collect();
    let weight = |s: &ReviewSignal| {
        ratios
            .and_then(|p| p.ratios.get(&s.signal))
            .map_or(0.0, |llr| llr * f64::from(s.windows))
    };
    signals.sort_by(|a, b| {
        weight(b)
            .total_cmp(&weight(a))
            .then(b.windows.cmp(&a.windows))
            .then(a.signal.cmp(&b.signal))
    });
    Ok(SessionReview {
        session_id: session_id.to_string(),
        status,
        trust_profile: surface.trust_profile,
        rung: surface.rung,
        posterior_llr: fusion.channels.iter().fold(0.0, |sum, c| sum + c.llr),
        threshold: fusion.threshold,
        manifest_version: fusion.manifest_version,
        signals,
    })
}

struct Attempt {
    id: String,
    served: i64,
    score: Option<f64>,
}

/// The attempt the session monitored: the most recent one, preferring one
/// that has been scored. Items served are the fixed draw (`question_ids`) or,
/// for adaptive delivery, the items served one by one (`attempt_items`).
fn session_attempt(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Option<Attempt>, String> {
    conn.query_row(
        "SELECT a.id,
                MAX(json_array_length(a.question_ids),
                    (SELECT COUNT(*) FROM attempt_items i WHERE i.attempt_id = a.id)),
                a.score
           FROM assessment_attempts a
          WHERE a.integrity_session_id = ?1
          ORDER BY score IS NULL, started_at DESC, rowid DESC LIMIT 1",
        params![session_id],
        |r| {
            Ok(Attempt {
                id: r.get(0)?,
                served: r.get(1)?,
                score: r.get(2)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn committed_windows(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Vec<Vec<String>>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT anomaly_flags FROM integrity_snapshots WHERE session_id = ?1
             ORDER BY captured_at, rowid",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |r| r.get::<_, Option<String>>(0))
        .map_err(|e| e.to_string())?;
    let mut windows = Vec::new();
    for row in rows {
        let json = row.map_err(|e| e.to_string())?;
        windows.push(
            json.and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
        );
    }
    Ok(windows)
}

fn probe_outcome(results: &[VerificationResult]) -> Option<ProbeOutcome> {
    let probes: Vec<&VerificationResult> =
        results.iter().filter(|r| r.step == STEP_PROBE).collect();
    if probes.is_empty() {
        return None;
    }
    let count = |o: VerificationOutcome| probes.iter().filter(|r| r.outcome == o).count() as u32;
    let (satisfied, failed, skipped) = (
        count(VerificationOutcome::Satisfied),
        count(VerificationOutcome::Failed),
        count(VerificationOutcome::Skipped),
    );
    let outcome = if failed > 0 {
        VerificationOutcome::Failed
    } else if satisfied > 0 {
        VerificationOutcome::Satisfied
    } else {
        VerificationOutcome::Skipped
    };
    let mut policies: Vec<String> = Vec::new();
    for r in &probes {
        if !policies.contains(&r.policy) {
            policies.push(r.policy.clone());
        }
    }
    Some(ProbeOutcome {
        outcome,
        policies,
        satisfied,
        failed,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::sentinel::scoring::ProfileRatios;

    /// A finished `core` session whose attempt served two items, both
    /// observed, over two committed windows carrying `flags`.
    fn session(flags: &str) -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute_batch(&format!(
                "INSERT INTO question_banks (id, skill_id, label) VALUES ('bank', 'skill', 'Skill');
                 INSERT INTO integrity_sessions (id, status, trust_profile, response_items, ended_at)
                     VALUES ('s', 'completed', 'core', 2, '2026-04-13T00:00:00Z');
                 INSERT INTO assessment_attempts
                     (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders,
                      integrity_session_id, started_at)
                     VALUES ('a', 'did:key:l', 'bank', 'skill', 1, '[\"q1\",\"q2\"]', '[]', 's',
                             '2026-04-13T00:00:00Z');
                 INSERT INTO integrity_snapshots (id, session_id, composite_score, captured_at, anomaly_flags)
                     VALUES ('w1', 's', 0.9, '2026-04-13T00:00:00Z', '{flags}'),
                            ('w2', 's', 0.9, '2026-04-13T00:00:01Z', '{flags}');"
            ))
            .unwrap();
        db
    }

    /// Ratios under which an idle-then-burst answer in two windows puts the
    /// response-process channel over threshold.
    fn measured() -> RatioManifest {
        let mut manifest = RatioManifest::uncertified();
        manifest.version = "test-certified".into();
        manifest.profiles.insert(
            "core".into(),
            ProfileRatios {
                threshold: 2.0,
                ratios: [("response_idle_burst".to_string(), 1.5)].into(),
            },
        );
        manifest
    }

    #[test]
    fn a_measured_response_process_anomaly_keeps_the_session_off_the_process_rung() {
        let clean = session("[]");
        let surface = session_surface(clean.conn(), "s", false, &measured(), None).unwrap();
        assert_eq!(surface.rung, VerificationRung::Process);

        let bursts = session(r#"["response_idle_burst"]"#);
        let surface = session_surface(bursts.conn(), "s", false, &measured(), None).unwrap();
        assert_eq!(surface.rung, VerificationRung::None);

        // Unmeasured, the same signal is advisory and cannot hold the rung back.
        let surface = session_surface(
            bursts.conn(),
            "s",
            false,
            &RatioManifest::uncertified(),
            None,
        )
        .unwrap();
        assert_eq!(surface.rung, VerificationRung::Process);
    }
}
