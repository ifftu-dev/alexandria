//! Issuing a credential: build the envelope, take a status-list slot, sign
//! it, store it, and gate it on the bound integrity session when there is
//! one.

use rusqlite::{params, Connection, OptionalExtension};

// Imported rather than redeclared. These were local copies, and a duplicated
// constant is exactly how the issuance path came to emit the v1 context while
// the rest of the codebase had moved to v2.
use alexandria_verify::vc::context::W3C_VC_V2;

use super::{status_list, CredentialError, Issuer, PolicyViolation, SupersessionRequires};
use crate::crypto::did::Did;
use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
use crate::domain::vc::{
    integrity_hash_of, Claim, CredentialStatus, CredentialType, IntegrityAssertion, ProbeSummary,
    Proof, VerifiableCredential,
};

/// Issuance-time integrity gate (§ Integrity→VC bridge). When an
/// `IssueCredentialRequest` carries both an `integrity_session_id` and
/// an `IssuancePolicy`, the session's terminal state must satisfy every
/// set bound or issuance is refused — so a "trusted" credential is only
/// minted when the assessment that backed it passed the sponsor's
/// integrity rules. All fields are optional/opt-in; an absent policy
/// means "embed the attestation but gate on nothing".
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct IssuancePolicy {
    /// Minimum final integrity score in `[0,1]`.
    #[serde(default)]
    pub min_integrity: Option<f64>,
    /// Maximum tolerated critical flags.
    #[serde(default)]
    pub max_critical: Option<i64>,
    /// Maximum tolerated warning flags.
    #[serde(default)]
    pub max_warning: Option<i64>,
    /// If true, the session status must be `completed` (clean) —
    /// `flagged` / `suspended` are rejected.
    #[serde(default)]
    pub require_clean: bool,
    /// If set, the assertion's `assurance_level` must equal this. Only
    /// `"local"` is currently achievable.
    #[serde(default)]
    pub required_assurance_level: Option<String>,
    /// Probe policy for attempts made for this role. Overrides the
    /// assessment's own policy when set to anything but `none`, and chooses
    /// which probes run after submission. At issuance, anything but `none`
    /// requires that the probe step ran under it and that no probe failed;
    /// a skipped probe never counts against the learner (require a satisfied
    /// one with `min_verification_rung: probed`).
    #[serde(default)]
    pub probe_policy: Option<crate::assessment::probes::ProbePolicy>,
    /// The model connection that writes generated probes under the `llm`
    /// policy: a saved model connection on the device that runs the attempt.
    /// Without one a generated probe is skipped. Ignored by every other
    /// policy and not judged at issuance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_connection_id: Option<String>,
    /// Minimum trust profile (the policy floor). Judged at attempt start and
    /// again here; issuance is refused unless the session meets it and the
    /// two judgements agree.
    #[serde(default)]
    pub min_trust_profile: Option<crate::sentinel::scoring::TrustProfile>,
    /// Minimum verification rung, judged at issuance from the session.
    #[serde(default)]
    pub min_verification_rung: Option<crate::sentinel::scoring::VerificationRung>,
}

impl IssuancePolicy {
    /// The first bound the assertion violates, if any.
    fn evaluate(
        &self,
        a: &IntegrityAssertion,
        facts: &SessionFacts,
    ) -> Result<(), PolicyViolation> {
        if self.require_clean && a.status != "completed" {
            return Err(PolicyViolation::NotClean {
                status: a.status.clone(),
            });
        }
        if let Some(min) = self.min_integrity {
            match a.integrity_score {
                Some(score) if score >= min => {}
                Some(score) => return Err(PolicyViolation::IntegrityBelowMinimum { score, min }),
                None => return Err(PolicyViolation::NoIntegrityScore),
            }
        }
        if let Some(max) = self.max_critical {
            if a.critical_count > max {
                return Err(PolicyViolation::TooManyCritical {
                    count: a.critical_count,
                    max,
                });
            }
        }
        if let Some(max) = self.max_warning {
            if a.warning_count > max {
                return Err(PolicyViolation::TooManyWarnings {
                    count: a.warning_count,
                    max,
                });
            }
        }
        if let Some(req) = &self.required_assurance_level {
            if &a.assurance_level != req {
                return Err(PolicyViolation::AssuranceBelowRequired {
                    level: a.assurance_level.clone(),
                    required: req.clone(),
                });
            }
        }
        if let Some(floor) = self.min_trust_profile {
            use crate::sentinel::scoring::evaluate_floor;
            // Judged again from the session as it stands: the attestation
            // counts only if it verifies now, the camera only if its
            // coverage meets the profile's minimum.
            let now = evaluate_floor(facts.trust_profile, Some(floor), &facts.uncredited);
            if !now.satisfied {
                return Err(PolicyViolation::TrustProfileBelowFloor {
                    profile: facts
                        .trust_profile
                        .map_or("none", |p| p.as_str())
                        .to_string(),
                    required: floor.as_str().to_string(),
                });
            }
            // The learner was told the attempt-start judgement; issuance must
            // agree with it or refuse.
            if facts.recorded_floor.as_ref() != Some(&now) {
                return Err(PolicyViolation::PolicyFloorDisagrees {
                    required: floor.as_str().to_string(),
                });
            }
        }
        if let Some(required) = self.min_verification_rung {
            if facts.rung < required {
                return Err(PolicyViolation::RungBelowMinimum {
                    rung: facts.rung.as_str().to_string(),
                    required: required.as_str().to_string(),
                });
            }
        }
        if let Some(policy) = self
            .probe_policy
            .filter(|p| *p != crate::assessment::probes::ProbePolicy::None)
        {
            let name = policy.as_str().to_string();
            match &facts.probe {
                Some(p) if p.policies.contains(&name) => {
                    if p.failed > 0 {
                        return Err(PolicyViolation::ProbeFailed { policy: name });
                    }
                }
                _ => return Err(PolicyViolation::ProbePolicyNotRun { policy: name }),
            }
        }
        Ok(())
    }
}

/// Facts about the bound session that the assertion does not carry but a
/// policy may gate on. Derived from the session at issuance.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionFacts {
    /// The session's locked trust profile, if one was chosen.
    pub trust_profile: Option<crate::sentinel::scoring::TrustProfile>,
    /// Signals the profile runs that do not count towards a floor now.
    pub uncredited: Vec<crate::sentinel::scoring::ProfileSignal>,
    /// The policy floor as judged at attempt start or interview-check
    /// acceptance.
    pub recorded_floor: Option<crate::sentinel::scoring::PolicyFloor>,
    /// The verification rung the session reached.
    pub rung: crate::sentinel::scoring::VerificationRung,
    /// The probe step's results, when it ran.
    pub probe: Option<crate::sentinel::surface::ProbeOutcome>,
}

impl SessionFacts {
    fn load(
        conn: &Connection,
        session_id: &str,
        surface: &crate::sentinel::surface::SessionSurface,
    ) -> Result<Self, CredentialError> {
        let floor: Option<String> = conn.query_row(
            "SELECT policy_floor_json FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| r.get(0),
        )?;
        Ok(Self {
            trust_profile: surface.trust_profile,
            uncredited: surface.uncredited.clone(),
            rung: surface.rung,
            probe: surface.probe.clone(),
            // An unreadable judgement is no judgement: issuance then refuses.
            recorded_floor: floor.and_then(|j| serde_json::from_str(&j).ok()),
        })
    }
}

/// Load an integrity session and summarise it as an `IntegrityAssertion`
/// for embedding at issuance, with the facts a policy gates on.
/// `assurance_level` is `local` unless the session's stored device
/// attestation verifies right now against `trust` (see
/// `sentinel::attestation::effective_assurance`); the stored level is never
/// copied. The profile, signals, rung, probe summary and coverage statement
/// are derived from the session (`sentinel::surface`). No anchor reference
/// is embedded.
pub(crate) fn build_integrity_assertion(
    conn: &Connection,
    session_id: &str,
    now: &str,
    trust: &crate::sentinel::attestation::TrustConfig,
) -> Result<(IntegrityAssertion, SessionFacts), CredentialError> {
    use crate::sentinel::attestation::{
        effective_assurance, SessionAttestationFacts, ASSURANCE_DEVICE_ATTESTED,
    };
    use crate::sentinel::scoring::{active_certification, active_manifest};
    let row = conn
        .query_row(
            "SELECT status, integrity_score, critical_count, warning_count, commitment_root,
                    started_at, assurance_level, attestation_json
             FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<f64>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| CredentialError::SessionNotFound(session_id.to_string()))?;
    let (
        status,
        integrity_score,
        critical_count,
        warning_count,
        commitment_root,
        started_at,
        stored_assurance_level,
        attestation_json,
    ) = row;
    let facts = SessionAttestationFacts {
        session_id: session_id.to_string(),
        started_at,
        stored_assurance_level,
        attestation_json,
        commitment_root: commitment_root.clone(),
        learner_did: Some(crate::settings::SettingsStore::get(
            conn,
            crate::settings::registry::keys::IDENTITY_LOCAL_DID,
        ))
        .filter(|did| !did.is_empty()),
    };
    let (assurance_level, unverified) =
        effective_assurance(&facts, trust, chrono::Utc::now().timestamp());
    if let Some(why) = unverified {
        log::warn!(
            target: "sentinel",
            "session {session_id}: device attestation not credited: {why:?}"
        );
    }
    let certification = active_certification();
    let surface = crate::sentinel::surface::session_surface(
        conn,
        session_id,
        assurance_level == ASSURANCE_DEVICE_ATTESTED,
        &active_manifest(),
        certification.as_ref().map(|(c, d)| (c, d.as_str())),
    )
    .map_err(CredentialError::Profile)?;
    let session_facts = SessionFacts::load(conn, session_id, &surface)?;
    let assertion = IntegrityAssertion {
        session_id: session_id.to_string(),
        status,
        integrity_score,
        critical_count,
        warning_count,
        assurance_level,
        commitment_root,
        anchor_ref: None,
        generated_at: now.to_string(),
        trust_profile: surface.trust_profile.map(|p| p.as_str().to_string()),
        verification_rung: Some(surface.rung.as_str().to_string()),
        coverage_statement_digest: surface.coverage.as_ref().map(|c| c.digest()),
        signals_run: surface.signals_run,
        signals_degraded: surface.signals_degraded,
        signals_not_run: surface.signals_not_run,
        probe: surface.probe.map(|p| ProbeSummary {
            outcome: p.outcome.as_str().to_string(),
            policies: p.policies,
            satisfied: p.satisfied,
            failed: p.failed,
            skipped: p.skipped,
        }),
    };
    Ok((assertion, session_facts))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IssueCredentialRequest {
    pub credential_type: CredentialType,
    pub subject: Did,
    pub claim: Claim,
    pub evidence_refs: Vec<String>,
    pub expiration_date: Option<String>,
    /// §11.4 supersession: if set, the new credential declares that
    /// it replaces the credential with this id. The issuer/subject
    /// /claim-kind invariants from §11.4 are enforced at insert time.
    #[serde(default)]
    pub supersedes: Option<String>,
    /// § Integrity→VC bridge: if set, the credential is bound to this
    /// integrity session — its terminal state is summarised into an
    /// `IntegrityAssertion` and embedded in the signed envelope.
    #[serde(default)]
    pub integrity_session_id: Option<String>,
    /// Optional issuance gate evaluated against the bound session.
    /// Requires `integrity_session_id`. On violation, issuance is
    /// refused (no credential is minted).
    #[serde(default)]
    pub integrity_policy: Option<IssuancePolicy>,
}

/// Pure-function issuance pipeline. Allocates the next status-list
/// slot, builds the VC envelope, signs it, persists both the signed
/// VC and its status-list slot, and returns the signed credential.
pub fn issue(
    conn: &Connection,
    issuer: &Issuer,
    req: &IssueCredentialRequest,
    now: &str,
) -> Result<VerifiableCredential, CredentialError> {
    let (issuer_key, issuer_did) = (issuer.key(), issuer.did());
    if !req.subject.as_str().starts_with("did:") {
        return Err(CredentialError::SubjectNotDid);
    }

    let list_id = status_list::ensure(conn, issuer_did)?;
    let index = status_list::allocate(conn, &list_id)?;

    let type_name = req.credential_type.as_str();

    // Build the VC envelope; sign_credential will stamp proof.proofValue.
    // For skill claims we fold the request's evidence_refs into the
    // claim so the inline subject properties carry them.
    let mut claim = req.claim.clone();
    if let Claim::Skill(ref mut s) = claim {
        s.evidence_refs = req.evidence_refs.clone();
    }
    let claim_kind = claim.kind_str();
    let skill_id = claim.skill_id().map(str::to_string);
    // Denormalized provenance tier for fast filtering / UI badges; the
    // authoritative value lives inside the signed VC's SkillClaim.
    let provenance = match &claim {
        Claim::Skill(s) => s.provenance.map(|p| p.as_str().to_string()),
        _ => None,
    };

    // Deterministic VC id per spec §3.3 — hash of issuer/subject/claim/
    // validFrom + status-list slot. Must be derived AFTER evidence_refs
    // are folded in so the hash covers the final claim contents.
    let credential_id = crate::domain::vc::id::deterministic_credential_id(
        issuer_did,
        &req.subject,
        &claim,
        now,
        &list_id,
        index,
    )
    .map_err(CredentialError::Envelope)?;

    // § Integrity→VC bridge: summarise the bound session, enforce the
    // optional issuance gate, and embed the attestation in the envelope.
    // A policy without a session is a caller error; a session without a
    // policy embeds the attestation but gates on nothing.
    let integrity = match &req.integrity_session_id {
        Some(session_id) => {
            let (assertion, facts) = build_integrity_assertion(
                conn,
                session_id,
                now,
                &crate::sentinel::attestation::TrustConfig::from_profile(),
            )?;
            if let Some(policy) = &req.integrity_policy {
                policy.evaluate(&assertion, &facts)?;
            }
            Some(assertion)
        }
        None => {
            if req.integrity_policy.is_some() {
                return Err(CredentialError::PolicyWithoutSession);
            }
            None
        }
    };

    let vc = VerifiableCredential {
        context: vec![W3C_VC_V2.into()],
        id: Some(credential_id.clone()),
        type_: vec!["VerifiableCredential".into(), type_name.to_string()],
        issuer: issuer_did.clone(),
        valid_from: now.to_string(),
        valid_until: req.expiration_date.clone(),
        credential_subject: claim.into_subject(req.subject.clone()),
        credential_status: Some(CredentialStatus {
            id: format!("{list_id}#{index}"),
            type_: status_list::ENTRY_TYPE.into(),
            status_purpose: "revocation".into(),
            status_list_index: index.to_string(),
            status_list_credential: list_id.clone(),
        }),
        terms_of_use: None,
        witness: None,
        integrity,
        proof: Proof::unsigned(now.to_string()),
    };
    let signed = sign_credential(
        UnsignedCredential { credential: vc },
        issuer_key,
        issuer_did,
    )
    .map_err(CredentialError::Sign)?;

    let signed_json = serde_json::to_string(&signed)?;
    let integrity_hash = integrity_hash_of(&signed).map_err(CredentialError::Envelope)?;

    // §11.4 supersession invariants: a newer credential may
    // supersede an older only when the same subject, claim kind, and
    // issuer match (otherwise we'd let an arbitrary issuer "retire"
    // someone else's credentials). Enforce here at the insert site.
    if let Some(prior_id) = &req.supersedes {
        let prior: Option<(String, String, String)> = conn
            .query_row(
                "SELECT issuer_did, subject_did, claim_kind FROM credentials WHERE id = ?1",
                params![prior_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (prior_issuer, prior_subject, prior_kind) =
            prior.ok_or_else(|| CredentialError::SupersededNotFound(prior_id.clone()))?;
        if prior_issuer != issuer_did.as_str() {
            return Err(CredentialError::Supersession(
                SupersessionRequires::SameIssuer,
            ));
        }
        if prior_subject != req.subject.as_str() {
            return Err(CredentialError::Supersession(
                SupersessionRequires::SameSubject,
            ));
        }
        if prior_kind != claim_kind {
            return Err(CredentialError::Supersession(
                SupersessionRequires::SameClaimKind,
            ));
        }
    }

    conn.execute(
        "INSERT INTO credentials \
         (id, issuer_did, subject_did, credential_type, claim_kind, skill_id, \
          issuance_date, expiration_date, signed_vc_json, integrity_hash, \
          status_list_id, status_list_index, supersedes, provenance) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            credential_id,
            issuer_did.as_str(),
            req.subject.as_str(),
            type_name,
            claim_kind,
            skill_id,
            now,
            req.expiration_date,
            signed_json,
            integrity_hash,
            list_id,
            index,
            req.supersedes,
            provenance,
        ],
    )
    .map_err(CredentialError::step("insert credential"))?;

    // Auto-enqueue for §12.3 integrity anchoring. Failure here is
    // non-fatal — anchoring is a survivability convenience, not the
    // critical issuance path. A retry will re-enqueue on next issuance
    // or via an explicit IPC.
    if let Err(e) = crate::cardano::anchor_queue::enqueue(conn, &credential_id) {
        log::warn!("auto-enqueue anchor failed for {credential_id}: {e}");
    }

    // Reputation feedback. Skill-kind credentials feed the learner's
    // score on (skill, level); third-party-issued credentials also
    // credit the instructor. Soft-fail: reputation is a derived view.
    if let Err(e) = crate::evidence::reputation::on_credential_accepted(conn, &credential_id) {
        log::warn!("reputation update failed for {credential_id}: {e}");
    }

    Ok(signed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::fixtures::{sample_request, setup, test_key, NOW};
    use crate::credentials::{CredentialError, PolicyViolation, SupersessionRequires};

    /// Insert a terminal integrity session row for bridge tests.
    fn seed_session(
        conn: &Connection,
        id: &str,
        status: &str,
        score: Option<f64>,
        critical: i64,
        warning: i64,
    ) {
        conn.execute(
            "INSERT INTO integrity_sessions
                (id, enrollment_id, status, integrity_score, critical_count, warning_count, started_at)
             VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6)",
            params![id, status, score, critical, warning, NOW],
        )
        .unwrap();
    }

    /// `windows` committed windows for `session`, the first `degraded` of
    /// them without camera checks.
    fn camera_windows(conn: &Connection, session: &str, windows: usize, degraded: usize) {
        for i in 0..windows {
            let (camera, flags) = if i < degraded {
                (None, r#"["camera_degraded"]"#)
            } else {
                (Some(0.9), "[]")
            };
            conn.execute(
                "INSERT INTO integrity_snapshots
                     (id, session_id, camera_score, composite_score, captured_at, anomaly_flags)
                 VALUES (?1, ?2, ?3, 0.9, ?4, ?5)",
                params![format!("{session}-w{i}"), session, camera, NOW, flags],
            )
            .unwrap();
        }
    }

    /// A live session that selected `profile` (with the camera benchmark
    /// passing), then judged `floor` at attempt start and ended clean.
    fn session_on_profile(
        conn: &Connection,
        id: &str,
        attested: bool,
        camera: bool,
        floor: Option<crate::sentinel::scoring::TrustProfile>,
    ) {
        use crate::sentinel::trust_profile::{
            record_policy_floor, select_for_session, BenchmarkResult, Capabilities, ProfileRequest,
        };
        conn.execute(
            "INSERT INTO integrity_sessions (id, status, assurance_level, started_at)
             VALUES (?1, 'active', ?2, ?3)",
            params![id, if attested { "device_attested" } else { "local" }, NOW],
        )
        .unwrap();
        let caps = Capabilities {
            camera,
            ..Default::default()
        };
        let request = ProfileRequest {
            capabilities: caps,
            consent: caps,
            role_assessment_id: None,
        };
        let fast = BenchmarkResult {
            detect_ms: 9.0,
            liveness_ms: 7.0,
            track_ms: 1.0,
        };
        select_for_session(conn, id, &request, Some(&fast)).unwrap();
        record_policy_floor(conn, id, floor).unwrap();
        conn.execute(
            "UPDATE integrity_sessions SET status = 'completed', ended_at = ?2 WHERE id = ?1",
            params![id, NOW],
        )
        .unwrap();
    }

    /// A finished assessment session on a profile chosen from `camera`, whose
    /// attempt served `served` items, `observed` of which the response
    /// tracker saw the learner work through.
    fn assessed(conn: &Connection, id: &str, camera: bool, served: usize, observed: usize) {
        session_on_profile(conn, id, false, camera, None);
        conn.execute(
            "INSERT OR IGNORE INTO question_banks (id, skill_id, label) VALUES ('bank', 'skill_test', 'Test')",
            [],
        )
        .unwrap();
        let ids: Vec<String> = (0..served).map(|i| format!("q{i}")).collect();
        conn.execute(
            "INSERT INTO assessment_attempts
                 (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders,
                  integrity_session_id, started_at)
             VALUES (?1, ?5, 'bank', 'skill_test', 1, ?2, '[]', ?3, ?4)",
            params![
                format!("att-{id}"),
                serde_json::to_string(&ids).unwrap(),
                id,
                NOW,
                format!("did:key:{id}")
            ],
        )
        .unwrap();
        for item in ids.iter().take(observed) {
            conn.execute(
                "INSERT INTO integrity_observed_items (session_id, item_id) VALUES (?1, ?2)",
                params![id, item],
            )
            .unwrap();
        }
    }

    fn assertion_for(
        conn: &Connection,
        key: &Issuer,
        subject: &Did,
        session: &str,
    ) -> IntegrityAssertion {
        let mut req = sample_request(subject.clone());
        req.integrity_session_id = Some(session.into());
        issue(conn, key, &req, NOW)
            .unwrap()
            .integrity
            .expect("integrity assertion embedded")
    }

    #[test]
    fn the_assertion_names_profile_rung_and_signals_and_its_coverage_statement_resolves() {
        use crate::domain::vc::{verify_credential_db, VerificationPolicy};
        use crate::sentinel::scoring::{
            resolve_coverage_statement, TrustProfile, VerificationRung,
        };
        let (db, key, _, subject) = setup();
        assessed(db.conn(), "seen", false, 3, 3);
        let mut req = sample_request(subject.clone());
        req.integrity_session_id = Some("seen".into());
        let vc = issue(db.conn(), &key, &req, NOW).unwrap();
        // The new fields are inside the signed envelope: the stored JSON, as
        // a verifier receives it, still verifies.
        let stored: VerifiableCredential =
            serde_json::from_str(&serde_json::to_string(&vc).unwrap()).unwrap();
        let verified =
            verify_credential_db(db.conn(), &stored, NOW, &VerificationPolicy::default());
        assert!(verified.valid_signature);
        let json = serde_json::to_value(&vc).unwrap();
        assert_eq!(
            json.pointer("/integrity/trustProfile"),
            Some(&"core".into())
        );

        let a = vc.integrity.expect("integrity assertion embedded");
        assert_eq!(a.trust_profile.as_deref(), Some("core"));
        assert_eq!(a.verification_rung.as_deref(), Some("process"));
        assert_eq!(a.signals_run, vec!["core"]);
        assert_eq!(a.signals_not_run, vec!["device_attestation", "camera"]);
        assert!(a.signals_degraded.is_empty());
        assert_eq!(a.probe, None);

        let digest = a.coverage_statement_digest.expect("coverage digest");
        let statement = resolve_coverage_statement(&digest).expect("bundled statement");
        assert_eq!(
            (statement.profile, statement.rung),
            (TrustProfile::Core, VerificationRung::Process)
        );
        assert!(statement.certification.is_none());
        assert!(statement
            .not_checked
            .iter()
            .any(|line| line.contains("second person")));
    }

    #[test]
    fn a_session_whose_tracker_never_saw_the_learner_work_leaves_the_rung_at_none() {
        let (db, key, _, subject) = setup();
        assessed(db.conn(), "unseen", false, 3, 0);
        let a = assertion_for(db.conn(), &key, &subject, "unseen");
        assert_eq!(a.verification_rung.as_deref(), Some("none"));

        // A session with no attempt behind it has no response process either.
        session_on_profile(db.conn(), "bare", false, false, None);
        let a = assertion_for(db.conn(), &key, &subject, "bare");
        assert_eq!(a.verification_rung.as_deref(), Some("none"));
    }

    /// Record a verification result in a finished session, as if it had
    /// been recorded before monitoring ended.
    fn verified(conn: &Connection, session: &str, step: &str, policy: &str, outcome: &str) {
        use crate::sentinel::verification::{record_verification, NewVerification};
        let outcome = match outcome {
            "satisfied" => crate::sentinel::verification::VerificationOutcome::Satisfied,
            "failed" => crate::sentinel::verification::VerificationOutcome::Failed,
            _ => crate::sentinel::verification::VerificationOutcome::Skipped,
        };
        conn.execute(
            "UPDATE integrity_sessions SET ended_at = NULL WHERE id = ?1",
            [session],
        )
        .unwrap();
        record_verification(
            conn,
            &NewVerification {
                session_id: session,
                attempt_id: None,
                step,
                policy,
                outcome,
                reason: None,
                subject_ref: None,
                detail: serde_json::json!({}),
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE integrity_sessions SET ended_at = ?2 WHERE id = ?1",
            params![session, NOW],
        )
        .unwrap();
    }

    /// The session's attempt scored `score`, after `prior` graded items on
    /// the same skill scored `prior_score` a day before it started.
    fn scored_with_history(
        conn: &Connection,
        session: &str,
        score: f64,
        prior: usize,
        prior_score: f64,
    ) {
        conn.execute(
            "UPDATE assessment_attempts SET score = ?2 WHERE integrity_session_id = ?1",
            params![session, score],
        )
        .unwrap();
        let earlier = format!("prior-{session}");
        conn.execute(
            "INSERT INTO assessment_attempts
                 (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders, started_at)
             VALUES (?1, ?2, 'bank', 'skill_test', 2, '[]', '[]', '2026-04-11T00:00:00Z')",
            params![earlier, format!("did:key:{session}")],
        )
        .unwrap();
        for i in 0..prior {
            conn.execute(
                "INSERT INTO attempt_items (attempt_id, ordinal, item_id, score, graded_at)
                 VALUES (?1, ?2, ?3, ?4, '2026-04-12T00:00:00Z')",
                params![earlier, i as i64, format!("p{i}"), prior_score],
            )
            .unwrap();
        }
    }

    #[test]
    fn satisfied_probes_raise_the_rung_and_skipped_ones_leave_it_at_process() {
        let (db, key, _, subject) = setup();
        assessed(db.conn(), "probed", false, 2, 2);
        verified(db.conn(), "probed", "probe", "reseed", "satisfied");
        verified(db.conn(), "probed", "probe", "reseed", "satisfied");
        let a = assertion_for(db.conn(), &key, &subject, "probed");
        assert_eq!(a.verification_rung.as_deref(), Some("probed"));
        assert_eq!(
            a.probe,
            Some(ProbeSummary {
                outcome: "satisfied".into(),
                policies: vec!["reseed".into()],
                satisfied: 2,
                failed: 0,
                skipped: 0,
            })
        );

        // A probe that timed out never counts against the learner, but the
        // probe policy was not satisfied either.
        assessed(db.conn(), "timeout", false, 2, 2);
        verified(db.conn(), "timeout", "probe", "reseed", "satisfied");
        verified(db.conn(), "timeout", "probe", "reseed", "skipped");
        let a = assertion_for(db.conn(), &key, &subject, "timeout");
        assert_eq!(a.verification_rung.as_deref(), Some("process"));
        assert_eq!(a.probe.as_ref().map(|p| p.skipped), Some(1));
    }

    #[test]
    fn consistency_with_history_and_a_live_segment_climb_the_ladder_in_order() {
        let (db, key, _, subject) = setup();
        assessed(db.conn(), "steady", false, 2, 2);
        verified(db.conn(), "steady", "probe", "authored", "satisfied");
        scored_with_history(db.conn(), "steady", 0.9, 6, 1.0);
        let a = assertion_for(db.conn(), &key, &subject, "steady");
        assert_eq!(a.verification_rung.as_deref(), Some("consistent"));

        verified(db.conn(), "steady", "interview", "live", "satisfied");
        let a = assertion_for(db.conn(), &key, &subject, "steady");
        assert_eq!(a.verification_rung.as_deref(), Some("live"));

        // Too little history: consistency is not established, so a live
        // segment cannot stand on it.
        assessed(db.conn(), "new", false, 2, 2);
        verified(db.conn(), "new", "probe", "authored", "satisfied");
        verified(db.conn(), "new", "interview", "live", "satisfied");
        scored_with_history(db.conn(), "new", 0.9, 1, 1.0);
        let a = assertion_for(db.conn(), &key, &subject, "new");
        assert_eq!(a.verification_rung.as_deref(), Some("probed"));

        // History that fails the skill throughout does not agree with a pass.
        assessed(db.conn(), "odd", false, 2, 2);
        verified(db.conn(), "odd", "probe", "authored", "satisfied");
        scored_with_history(db.conn(), "odd", 1.0, 8, 0.0);
        let a = assertion_for(db.conn(), &key, &subject, "odd");
        assert_eq!(a.verification_rung.as_deref(), Some("probed"));
    }

    #[test]
    fn a_minimum_rung_refuses_issuance_below_it() {
        use crate::sentinel::scoring::VerificationRung;
        let (db, key, _, subject) = setup();
        let mut req = sample_request(subject);
        req.integrity_policy = Some(IssuancePolicy {
            min_verification_rung: Some(VerificationRung::Probed),
            ..Default::default()
        });
        assessed(db.conn(), "process", false, 2, 2);
        req.integrity_session_id = Some("process".into());
        let err = issue(db.conn(), &key, &req, NOW).unwrap_err();
        assert!(
            matches!(
                &err,
                CredentialError::Policy(PolicyViolation::RungBelowMinimum { rung, required })
                    if rung == "process" && required == "probed"
            ),
            "{err:?}"
        );
        assert_eq!(
            err.to_string(),
            "issuance policy: verification rung 'process' does not meet the 'probed' minimum"
        );

        verified(db.conn(), "process", "probe", "reseed", "satisfied");
        assert!(issue(db.conn(), &key, &req, NOW).is_ok());
    }

    #[test]
    fn a_role_probe_policy_must_have_run_and_must_not_have_failed() {
        use crate::assessment::probes::ProbePolicy;
        let (db, key, _, subject) = setup();
        let mut req = sample_request(subject);
        req.integrity_policy = Some(IssuancePolicy {
            probe_policy: Some(ProbePolicy::Reseed),
            ..Default::default()
        });
        let refusal = |req: &IssueCredentialRequest, session: &str| {
            let mut req = req.clone();
            req.integrity_session_id = Some(session.into());
            issue(db.conn(), &key, &req, NOW)
        };

        // The learner never reached the probes.
        assessed(db.conn(), "skipped_step", false, 2, 2);
        assert!(matches!(
            refusal(&req, "skipped_step").unwrap_err(),
            CredentialError::Policy(PolicyViolation::ProbePolicyNotRun { .. })
        ));

        // Probes ran, but under another policy.
        assessed(db.conn(), "other", false, 2, 2);
        verified(db.conn(), "other", "probe", "authored", "satisfied");
        assert!(matches!(
            refusal(&req, "other").unwrap_err(),
            CredentialError::Policy(PolicyViolation::ProbePolicyNotRun { .. })
        ));

        // A probe that failed is a failed verification.
        assessed(db.conn(), "failed", false, 2, 2);
        verified(db.conn(), "failed", "probe", "reseed", "failed");
        let err = refusal(&req, "failed").unwrap_err();
        assert!(
            matches!(
                err,
                CredentialError::Policy(PolicyViolation::ProbeFailed { .. })
            ),
            "{err:?}"
        );

        // Ran and timed out: never counts against the learner.
        assessed(db.conn(), "timed_out", false, 2, 2);
        verified(db.conn(), "timed_out", "probe", "reseed", "skipped");
        assert!(refusal(&req, "timed_out").is_ok());

        // `none` asks for nothing.
        req.integrity_policy = Some(IssuancePolicy {
            probe_policy: Some(ProbePolicy::None),
            ..Default::default()
        });
        assert!(refusal(&req, "skipped_step").is_ok());
    }

    fn floor_policy(floor: crate::sentinel::scoring::TrustProfile) -> IssuancePolicy {
        IssuancePolicy {
            min_trust_profile: Some(floor),
            ..Default::default()
        }
    }

    #[test]
    fn issuance_judges_the_policy_floor_again_and_needs_the_attempt_start_judgement() {
        use crate::sentinel::scoring::TrustProfile;
        let (db, key, _, subject) = setup();
        let mut req = sample_request(subject);
        req.integrity_policy = Some(floor_policy(TrustProfile::Camera));

        // Judged at attempt start, met, and met again now.
        session_on_profile(db.conn(), "met", false, true, Some(TrustProfile::Camera));
        camera_windows(db.conn(), "met", 3, 0);
        req.integrity_session_id = Some("met".into());
        assert!(issue(db.conn(), &key, &req, NOW).is_ok());

        // Never judged at attempt start: fail closed even though the session
        // runs the camera.
        session_on_profile(db.conn(), "unjudged", false, true, None);
        camera_windows(db.conn(), "unjudged", 3, 0);
        req.integrity_session_id = Some("unjudged".into());
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyViolation::PolicyFloorDisagrees { .. })
        ));

        // A session whose profile does not run the camera does not meet it.
        session_on_profile(db.conn(), "core", false, false, Some(TrustProfile::Camera));
        req.integrity_session_id = Some("core".into());
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyViolation::TrustProfileBelowFloor { .. })
        ));
    }

    #[test]
    fn a_camera_lost_for_too_much_of_the_session_no_longer_carries_a_camera_floor() {
        use crate::sentinel::scoring::TrustProfile;
        let (db, key, _, subject) = setup();
        let mut req = sample_request(subject.clone());
        req.integrity_policy = Some(floor_policy(TrustProfile::Camera));

        // One window in ten without camera checks: degraded, and said so,
        // but within the profile's coverage minimum.
        session_on_profile(db.conn(), "brief", false, true, Some(TrustProfile::Camera));
        camera_windows(db.conn(), "brief", 10, 1);
        req.integrity_session_id = Some("brief".into());
        let vc = issue(db.conn(), &key, &req, NOW).unwrap();
        let a = vc.integrity.unwrap();
        assert_eq!(a.trust_profile.as_deref(), Some("camera"));
        assert_eq!(a.signals_run, vec!["core", "camera"]);
        assert_eq!(a.signals_degraded, vec!["camera"]);
        assert_eq!(a.signals_not_run, vec!["device_attestation"]);

        // Half the session without the camera: the floor is not met.
        session_on_profile(
            db.conn(),
            "covered",
            false,
            true,
            Some(TrustProfile::Camera),
        );
        camera_windows(db.conn(), "covered", 4, 2);
        req.integrity_session_id = Some("covered".into());
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyViolation::TrustProfileBelowFloor { .. })
        ));

        // A camera session with no committed windows has no coverage at all.
        session_on_profile(db.conn(), "empty", false, true, Some(TrustProfile::Camera));
        req.integrity_session_id = Some("empty".into());
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyViolation::TrustProfileBelowFloor { .. })
        ));
        // Without a floor it still issues, and the assertion says it degraded.
        let a = assertion_for(db.conn(), &key, &subject, "empty");
        assert_eq!(a.signals_degraded, vec!["camera"]);
    }

    #[test]
    fn an_attestation_that_no_longer_verifies_at_issuance_fails_an_attested_floor() {
        use crate::sentinel::scoring::TrustProfile;
        let (db, key, _, subject) = setup();
        // The session was attested when the profile was chosen and the floor
        // judged; at issuance the stored attestation does not verify (there
        // is none), so the two judgements disagree.
        session_on_profile(db.conn(), "att", true, false, Some(TrustProfile::Attested));
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("att".into());
        req.integrity_policy = Some(floor_policy(TrustProfile::Attested));
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyViolation::TrustProfileBelowFloor { .. })
        ));
        // Without the floor the credential issues and names the loss.
        req.integrity_policy = None;
        let a = issue(db.conn(), &key, &req, NOW)
            .unwrap()
            .integrity
            .unwrap();
        assert_eq!(a.trust_profile.as_deref(), Some("attested"));
        assert_eq!(a.assurance_level, "local");
        assert_eq!(a.signals_degraded, vec!["device_attestation"]);
    }

    #[test]
    fn integrity_assertion_embedded_when_session_bound() {
        let (db, key, _, subject) = setup();
        seed_session(db.conn(), "sess_ok", "completed", Some(0.91), 0, 1);
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("sess_ok".into());
        let vc = issue(db.conn(), &key, &req, NOW).unwrap();
        let a = vc.integrity.as_ref().expect("integrity assertion embedded");
        assert_eq!(a.session_id, "sess_ok");
        assert_eq!(a.status, "completed");
        assert_eq!(a.integrity_score, Some(0.91));
        assert_eq!(a.warning_count, 1);
        assert_eq!(a.assurance_level, "local");
        // Assertion is inside the signed envelope.
        let v = serde_json::to_value(&vc).unwrap();
        assert!(v.get("integrity").is_some(), "integrity not serialized");
    }

    #[test]
    fn stored_assurance_claims_never_raise_the_embedded_level() {
        let (db, key, _, subject) = setup();
        seed_session(db.conn(), "sess_claimed", "completed", Some(0.95), 0, 0);
        db.conn()
            .execute(
                "UPDATE integrity_sessions
                 SET assurance_level = 'high_assurance', anchor_ref = 'dht:unverified'
                 WHERE id = 'sess_claimed'",
                [],
            )
            .unwrap();
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("sess_claimed".into());

        for required in ["anchored", "high_assurance"] {
            req.integrity_policy = Some(IssuancePolicy {
                required_assurance_level: Some(required.into()),
                ..Default::default()
            });
            let error = issue(db.conn(), &key, &req, NOW).unwrap_err();
            assert!(
                matches!(
                    &error,
                    CredentialError::Policy(PolicyViolation::AssuranceBelowRequired { level, required: wanted })
                        if level == "local" && wanted == required
                ),
                "{error:?}"
            );
        }

        req.integrity_policy = None;
        let vc = issue(db.conn(), &key, &req, NOW).unwrap();
        let assertion = vc.integrity.as_ref().expect("integrity assertion embedded");
        assert_eq!(assertion.assurance_level, "local");
        assert_eq!(assertion.anchor_ref, None);
    }

    #[test]
    fn issuance_policy_blocks_when_session_fails_gate() {
        let (db, key, _, subject) = setup();
        seed_session(db.conn(), "sess_bad", "suspended", Some(0.30), 2, 1);
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("sess_bad".into());
        req.integrity_policy = Some(IssuancePolicy {
            min_integrity: Some(0.70),
            require_clean: true,
            ..Default::default()
        });
        let err = issue(db.conn(), &key, &req, NOW).unwrap_err();
        assert!(
            matches!(
                &err,
                CredentialError::Policy(PolicyViolation::NotClean { status }) if status == "suspended"
            ),
            "{err:?}"
        );
        assert_eq!(
            err.to_string(),
            "issuance policy: session status is 'suspended', requires 'completed'"
        );

        req.integrity_policy = Some(IssuancePolicy {
            min_integrity: Some(0.70),
            ..Default::default()
        });
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyViolation::IntegrityBelowMinimum { .. })
        ));
        req.integrity_policy = Some(IssuancePolicy {
            max_critical: Some(1),
            ..Default::default()
        });
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyViolation::TooManyCritical { count: 2, max: 1 })
        ));
        req.integrity_policy = Some(IssuancePolicy {
            max_warning: Some(0),
            ..Default::default()
        });
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyViolation::TooManyWarnings { count: 1, max: 0 })
        ));
    }

    #[test]
    fn issuance_policy_passes_when_session_meets_gate() {
        let (db, key, _, subject) = setup();
        seed_session(db.conn(), "sess_pass", "completed", Some(0.88), 0, 0);
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("sess_pass".into());
        req.integrity_policy = Some(IssuancePolicy {
            min_integrity: Some(0.70),
            max_critical: Some(0),
            require_clean: true,
            ..Default::default()
        });
        let vc = issue(db.conn(), &key, &req, NOW).unwrap();
        assert!(vc.integrity.is_some());
        assert!(!vc.proof.proof_value.is_empty());
    }

    #[test]
    fn issuance_policy_without_session_is_rejected() {
        let (db, key, _, subject) = setup();
        let mut req = sample_request(subject);
        req.integrity_policy = Some(IssuancePolicy {
            require_clean: true,
            ..Default::default()
        });
        let err = issue(db.conn(), &key, &req, NOW).unwrap_err();
        assert!(
            matches!(err, CredentialError::PolicyWithoutSession),
            "{err:?}"
        );
    }

    #[test]
    fn an_unknown_integrity_session_is_refused() {
        let (db, key, _, subject) = setup();
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("sess_missing".into());
        let err = issue(db.conn(), &key, &req, NOW).unwrap_err();
        assert!(
            matches!(&err, CredentialError::SessionNotFound(id) if id == "sess_missing"),
            "{err:?}"
        );
    }

    #[test]
    fn issue_credential_returns_signed_vc_with_status_slot() {
        let (db, key, _, subject) = setup();
        let vc = issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        assert!(!vc.proof.proof_value.is_empty());
        assert!(vc.id.as_deref().unwrap().starts_with("urn:alexandria:vc:"));
        let status = vc.credential_status.expect("status attached");
        assert_eq!(status.status_list_index, "0");
        assert!(status
            .status_list_credential
            .starts_with("urn:alexandria:status-list:"));
    }

    #[test]
    fn issue_credential_allocates_sequential_indices() {
        // Each new credential from the same issuer gets the next bit
        // in the status list, never reusing an index even after revoke.
        let (db, key, _, subject) = setup();
        let a = issue(db.conn(), &key, &sample_request(subject.clone()), NOW).unwrap();
        let b = issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        assert_eq!(a.credential_status.unwrap().status_list_index, "0");
        assert_eq!(b.credential_status.unwrap().status_list_index, "1");
    }

    #[test]
    fn issue_rejects_non_did_subject() {
        let (db, key, _, _) = setup();
        let req = sample_request(Did("alice@example.com".into()));
        let err = issue(db.conn(), &key, &req, NOW).unwrap_err();
        assert!(matches!(err, CredentialError::SubjectNotDid), "{err:?}");
    }

    #[test]
    fn supersession_marks_old_credential_superseded() {
        use crate::domain::vc::verify_credential_db;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, _, subject) = setup();
        let old = issue(db.conn(), &key, &sample_request(subject.clone()), NOW).unwrap();

        // Issue a newer credential that supersedes the old one.
        let mut new_req = sample_request(subject);
        new_req.supersedes = old.id.clone();
        let _new = issue(db.conn(), &key, &new_req, NOW).unwrap();

        let result = verify_credential_db(db.conn(), &old, NOW, &VerificationPolicy::default());
        assert!(result.superseded);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    fn supersession_rejects_cross_issuer() {
        // §11.4: same subject, claim kind, and issuer required.
        let (db, key, _, subject) = setup();
        let old = issue(db.conn(), &key, &sample_request(subject.clone()), NOW).unwrap();

        let other_key = test_key("other-issuer");
        let mut bad_req = sample_request(subject);
        bad_req.supersedes = old.id.clone();
        let err = issue(
            db.conn(),
            &Issuer::from_key(other_key.clone()),
            &bad_req,
            NOW,
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                CredentialError::Supersession(SupersessionRequires::SameIssuer)
            ),
            "{err:?}"
        );
        assert_eq!(
            err.to_string(),
            "§11.4: supersession requires same issuer as the prior credential"
        );
    }

    #[test]
    fn supersession_rejects_unknown_prior_id() {
        let (db, key, _, subject) = setup();
        let mut req = sample_request(subject);
        req.supersedes = Some("urn:uuid:does-not-exist".into());
        let err = issue(db.conn(), &key, &req, NOW).unwrap_err();
        assert!(
            matches!(&err, CredentialError::SupersededNotFound(id) if id == "urn:uuid:does-not-exist"),
            "{err:?}"
        );
    }
}
