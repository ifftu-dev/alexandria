//! Issuing a credential: build the envelope, take a status-list slot, sign
//! it, store it, and gate it on the bound integrity session when there is
//! one.

use rusqlite::{params, Connection, OptionalExtension};

// Imported rather than redeclared. These were local copies, and a duplicated
// constant is exactly how the issuance path came to emit the v1 context while
// the rest of the codebase had moved to v2.
use alexandria_verify::vc::context::W3C_VC_V2;

use super::evidence::{EvidenceBinding, EvidenceSource};
use super::{status_list, CredentialError, Issuer, SupersessionRequires};
use crate::crypto::did::Did;
use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
use crate::domain::vc::{
    integrity_hash_of, Claim, CredentialType, IntegrityAssertion, ProbeSummary, Proof,
    VerifiableCredential,
};

/// Issuance-time integrity gate (§ Integrity→VC bridge). When an
/// `IssueCredentialRequest` carries both an `integrity_session_id` and an
/// `IssuancePolicy`, the credential must satisfy every set bound or issuance
/// is refused, so a "trusted" credential is only minted when the assessment
/// that backed it passed the sponsor's integrity rules. The policy is judged
/// by the verifier library's shared rule ([`alexandria_verify::policy`]),
/// the one the hosted service applies, over the credential's assertion and
/// its attestation outcome; this module adds only the checks that need the
/// session ([`SessionFacts::floor_agrees`]).
pub use alexandria_verify::policy::IssuancePolicy;

/// Judge `policy` for a credential about to be signed: the shared rule over
/// its integrity assertion and the outcome of its attached attestation
/// evidence checked against `trust`, then the agreement of the floor judged
/// at attempt start with the one judged from the session now.
fn gate(
    policy: &IssuancePolicy,
    vc: &VerifiableCredential,
    facts: &SessionFacts,
    trust: &alexandria_verify::attestation::AttestationTrust,
) -> Result<(), CredentialError> {
    let Some(assertion) = &vc.integrity else {
        return Err(CredentialError::PolicyWithoutSession);
    };
    judge_shared_rule(policy, assertion, vc, trust)?;
    if let Some(floor) = policy.min_trust_profile {
        if !facts.floor_agrees(floor) {
            return Err(CredentialError::PolicyFloorDisagrees {
                required: floor.as_str().to_string(),
            });
        }
    }
    Ok(())
}

/// The shared rule over a credential's integrity `assertion`: a role
/// credential issued on an assessed credential is judged by what was
/// verified about that credential ([`alexandria_verify::policy::
/// evaluate_assessed`]); one issued from a hosted attempt by what the issuer
/// found ([`alexandria_verify::policy::evaluate_hosted`]), as the hosted
/// service judged it; any other by its own attestation evidence checked
/// against `trust`. Used at issuance and by the credential exchange's picker.
pub(crate) fn judge_shared_rule(
    policy: &IssuancePolicy,
    assertion: &IntegrityAssertion,
    vc: &VerifiableCredential,
    trust: &alexandria_verify::attestation::AttestationTrust,
) -> Result<(), CredentialError> {
    let envelope = |e: &dyn std::fmt::Display| CredentialError::Envelope(e.to_string());
    if let Some(assessed) = alexandria_verify::assessed::AssessedCredentialEvidence::find(vc)
        .map_err(|e| envelope(&e))?
    {
        alexandria_verify::policy::evaluate_assessed(policy, assertion, &assessed)?;
    } else if let Some(hosted) =
        alexandria_verify::custody::HostedAttemptEvidence::find(vc).map_err(|e| envelope(&e))?
    {
        alexandria_verify::policy::evaluate_hosted(policy, assertion, &hosted)?;
    } else {
        let attestation = alexandria_verify::attestation::verify_evidence(vc, trust);
        alexandria_verify::policy::evaluate(policy, assertion, &attestation)?;
    }
    Ok(())
}

/// Facts about the bound session that the assertion does not carry, for the
/// checks only the issuing device can make. Derived from the session at
/// issuance.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionFacts {
    /// The session's locked trust profile, if one was chosen.
    pub trust_profile: Option<crate::sentinel::scoring::TrustProfile>,
    /// Signals the profile runs that do not count towards a floor now.
    pub uncredited: Vec<crate::sentinel::scoring::ProfileSignal>,
    /// The policy floor as judged at attempt start or interview-check
    /// acceptance.
    pub recorded_floor: Option<crate::sentinel::scoring::PolicyFloor>,
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
            // An unreadable judgement is no judgement: issuance then refuses.
            recorded_floor: floor.and_then(|j| serde_json::from_str(&j).ok()),
        })
    }

    /// Whether `floor`, judged again from the session as it stands (the
    /// attestation counts only if it verifies now, the camera only if its
    /// coverage meets the profile's minimum), agrees with the judgement the
    /// learner was told at attempt start. A session never judged disagrees.
    fn floor_agrees(&self, floor: crate::sentinel::scoring::TrustProfile) -> bool {
        let now = crate::sentinel::scoring::evaluate_floor(
            self.trust_profile,
            Some(floor),
            &self.uncredited,
        );
        self.recorded_floor.as_ref() == Some(&now)
    }
}

/// Load an integrity session and summarise it as an `IntegrityAssertion`
/// for embedding at issuance, with the facts a policy gates on.
/// `assurance_level` is `local` unless the session's stored device
/// attestation verifies right now against `trust` (see
/// `sentinel::attestation::effective_assurance`); the stored level is never
/// copied. The profile, signals, rung, probe summary and coverage statement
/// are derived from the session (`sentinel::surface`). Answer custody is
/// always `device`: the answers were on the device that ran the attempt, and
/// only an issuer whose answers never left it may state `issuer`. No anchor
/// reference is embedded.
pub(crate) fn build_integrity_assertion(
    conn: &Connection,
    session_id: &str,
    now: &str,
    trust: &crate::sentinel::attestation::TrustConfig,
) -> Result<(IntegrityAssertion, SessionFacts), CredentialError> {
    use crate::sentinel::attestation::{
        credits_attestation_signal, effective_assurance, SessionAttestationFacts,
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
        credits_attestation_signal(&assurance_level),
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
        camera_coverage: surface.camera_coverage,
        answer_custody: Some(alexandria_verify::integrity::AnswerCustody::Device),
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

/// The `relatedResource` entry naming the coverage statement `integrity`
/// names by digest: at the profile's status host when one is configured,
/// else by URN (as status lists fall back). Empty when there is no assertion
/// or it names no statement.
fn coverage_related_resource(
    conn: &Connection,
    integrity: Option<&IntegrityAssertion>,
) -> Result<Vec<alexandria_verify::vc::RelatedResource>, CredentialError> {
    let Some(digest) = integrity.and_then(|a| a.coverage_statement_digest.as_deref()) else {
        return Ok(Vec::new());
    };
    let statement =
        crate::sentinel::scoring::resolve_coverage_statement(digest).ok_or_else(|| {
            CredentialError::Envelope(format!("coverage statement {digest} does not resolve"))
        })?;
    let host = status_list::host(conn);
    Ok(vec![alexandria_verify::coverage::related_resource(
        &statement,
        host.as_deref(),
    )])
}

/// A signed credential and what attestation evidence it carries.
#[derive(Debug, Clone)]
pub struct Issued {
    pub credential: VerifiableCredential,
    pub evidence: EvidenceBinding,
}

/// [`issue_with`] this build's trust inputs and this device's own platform
/// as the evidence source ([`EvidenceSource::native`]).
pub fn issue(
    conn: &Connection,
    issuer: &Issuer,
    req: &IssueCredentialRequest,
    now: &str,
) -> Result<VerifiableCredential, CredentialError> {
    issue_with(conn, issuer, req, now, &EvidenceSource::native()).map(|i| i.credential)
}

/// Pure-function issuance pipeline. Allocates the next status-list
/// slot, builds the VC envelope, embeds the bound session's attestation
/// evidence (obtained through `evidence`), judges the issuance policy over
/// that envelope, signs it, persists both the signed VC and its status-list
/// slot, and returns the signed credential.
pub fn issue_with(
    conn: &Connection,
    issuer: &Issuer,
    req: &IssueCredentialRequest,
    now: &str,
    evidence: &EvidenceSource<'_>,
) -> Result<Issued, CredentialError> {
    issue_with_evidence(conn, issuer, req, now, evidence, Vec::new())
}

/// [`issue_with`] for a credential that also carries `entries` in its
/// `evidence` beside its attestation evidence, such as the
/// `AssessedCredentialEvidence` naming the credential a role credential is
/// issued on. The issuance policy sees them.
pub fn issue_with_evidence(
    conn: &Connection,
    issuer: &Issuer,
    req: &IssueCredentialRequest,
    now: &str,
    evidence: &EvidenceSource<'_>,
    entries: Vec<alexandria_verify::vc::EvidenceEntry>,
) -> Result<Issued, CredentialError> {
    let (issuer_key, issuer_did) = (issuer.key(), issuer.did());
    if !req.subject.as_str().starts_with("did:") {
        return Err(CredentialError::SubjectNotDid);
    }

    // The revocation list's slot is the credential's slot in both lists.
    let lists = status_list::ensure(conn, issuer_did)?;
    let list_id = lists.revocation.clone();
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

    // § Integrity→VC bridge: summarise the bound session and embed the
    // assertion in the envelope; the optional issuance gate is judged on the
    // envelope before signing. A policy without a session is a caller error;
    // a session without a policy embeds the assertion but gates on nothing.
    let (integrity, session_facts) = match &req.integrity_session_id {
        Some(session_id) => {
            let (assertion, facts) =
                build_integrity_assertion(conn, session_id, now, &evidence.trust)?;
            (Some(assertion), Some(facts))
        }
        None => {
            if req.integrity_policy.is_some() {
                return Err(CredentialError::PolicyWithoutSession);
            }
            (None, None)
        }
    };

    let mut vc = VerifiableCredential {
        context: vec![W3C_VC_V2.into()],
        id: Some(credential_id.clone()),
        type_: vec!["VerifiableCredential".into(), type_name.to_string()],
        issuer: issuer_did.clone().into(),
        valid_from: now.to_string(),
        valid_until: req.expiration_date.clone(),
        credential_subject: claim.into_subject(req.subject.clone()),
        credential_status: Some(lists.entries(index)),
        terms_of_use: None,
        witness: None,
        related_resource: coverage_related_resource(conn, integrity.as_ref())?,
        integrity,
        evidence: entries,
        proof: Proof::unsigned(now.to_string()),
    };
    // Attestation evidence over the finished content, then the policy over
    // the envelope with that evidence, both before signing: a refusal still
    // mints nothing, and the verified assurance level the policy judges is
    // the one a verifier will see.
    let evidence_binding = super::evidence::attach(conn, &mut vc, evidence)?;
    if let (Some(policy), Some(facts)) = (&req.integrity_policy, &session_facts) {
        gate(policy, &vc, facts, &evidence.trust.inputs())?;
    }
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

    Ok(Issued {
        credential: signed,
        evidence: evidence_binding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::fixtures::{
        assessed, sample_request, scored_with_history, session_on_profile, setup, test_key,
        verified, NOW,
    };
    use crate::credentials::{CredentialError, PolicyRefusal, SupersessionRequires};

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

    /// The wire form omits an empty `evidence`, so credentials issued without
    /// attestation evidence keep the shape they had before it existed.
    fn json_has_no_evidence(vc: &VerifiableCredential) -> bool {
        serde_json::to_value(vc).unwrap().get("evidence").is_none()
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

    /// A role credential from a hosted attempt is judged by what its
    /// `HostedAttemptEvidence` entry records, the issuer's own findings, as
    /// the hosted service judged it: not at `local` with the probe summary
    /// the device reported.
    #[test]
    fn a_role_credential_from_a_hosted_attempt_is_judged_from_its_entry() {
        use alexandria_verify::attestation::{AttestationOutcome, Platform};
        use alexandria_verify::custody::{AnswerCustody, HostedAttemptEvidence};
        use alexandria_verify::vc::ProbeSummary;
        let satisfied = ProbeSummary {
            outcome: "satisfied".into(),
            policies: vec!["authored".into()],
            satisfied: 1,
            failed: 0,
            skipped: 0,
        };
        let raw: serde_json::Value = serde_json::from_str(include_str!(
            "../../../crates/alexandria-verify/tests/vectors/01-valid.json"
        ))
        .unwrap();
        let mut vc: VerifiableCredential =
            serde_json::from_value(raw["credential"].clone()).unwrap();
        let assertion = IntegrityAssertion {
            session_id: "isess_hosted".into(),
            status: "completed".into(),
            integrity_score: Some(0.9),
            critical_count: 0,
            warning_count: 0,
            assurance_level: "device_attested".into(),
            commitment_root: None,
            anchor_ref: None,
            generated_at: NOW.into(),
            trust_profile: Some("attested".into()),
            verification_rung: Some("probed".into()),
            coverage_statement_digest: None,
            signals_run: vec!["core".into(), "device_attestation".into()],
            signals_degraded: vec![],
            signals_not_run: vec!["camera".into()],
            camera_coverage: None,
            // The device reported a failed probe; the issuer's step is the one judged.
            probe: Some(ProbeSummary {
                outcome: "failed".into(),
                failed: 1,
                satisfied: 0,
                ..satisfied.clone()
            }),
            answer_custody: Some(AnswerCustody::Issuer),
        };
        vc.integrity = Some(assertion.clone());
        vc.evidence = vec![HostedAttemptEvidence::new(
            "hatt",
            Some(satisfied),
            AttestationOutcome::VerifiedCredentialBound {
                platform: Platform::Ios,
                companion: false,
            },
        )
        .into_entry()];
        let policy = IssuancePolicy {
            required_answer_custody: Some(AnswerCustody::Issuer),
            required_assurance_level: Some("device_attested".into()),
            min_verification_rung: Some(alexandria_verify::integrity::VerificationRung::Probed),
            probe_policy: Some(alexandria_verify::policy::ProbePolicy::Authored),
            ..Default::default()
        };
        let trust = crate::sentinel::attestation::TrustConfig::default().inputs();
        judge_shared_rule(&policy, &assertion, &vc, &trust).expect("met on the entry");
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
        // An unattested session carries no evidence: the attestation is not
        // supplied, the verified level is local, and the credential is
        // accepted as before.
        assert_eq!(
            verified.attestation,
            alexandria_verify::attestation::AttestationOutcome::NotSupplied
        );
        assert_eq!(
            verified.verified_assurance_level,
            alexandria_verify::attestation::VerifiedAssuranceLevel::Local
        );
        assert_eq!(
            verified.acceptance_decision,
            crate::domain::vc::AcceptanceDecision::Accept
        );
        assert!(json_has_no_evidence(&vc));
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
            (statement.profile.as_str(), statement.rung.as_str()),
            (
                TrustProfile::Core.as_str(),
                VerificationRung::Process.as_str()
            )
        );
        assert_eq!(statement.version, "3", "new credentials name version 3");
        assert!(statement.certification.is_none());
        assert!(statement
            .not_checked
            .iter()
            .any(|line| line.contains("second person")));
    }

    /// The `relatedResource` entry of `vc`, as a verifier receives it.
    #[test]
    fn a_self_issued_credential_states_device_custody_and_the_library_accepts_it() {
        use crate::domain::vc::{verify_credential_db, VerificationPolicy};
        let (db, key, issuer, _) = setup();
        assessed(db.conn(), "mine", false, 2, 2);
        // Self-issued: the learner signs about themselves.
        let mut req = sample_request(issuer);
        req.integrity_session_id = Some("mine".into());
        let vc = issue(db.conn(), &key, &req, NOW).unwrap();
        let json = serde_json::to_value(&vc).unwrap();
        assert_eq!(
            json.pointer("/integrity/answerCustody"),
            Some(&"device".into()),
            "the custody is written, not left to the missing-field default"
        );
        let stored: VerifiableCredential = serde_json::from_value(json).unwrap();
        let verified =
            verify_credential_db(db.conn(), &stored, NOW, &VerificationPolicy::default());
        assert!(verified.custody_defects.is_empty());
        assert_eq!(
            verified.acceptance_decision,
            crate::domain::vc::AcceptanceDecision::Accept
        );
    }

    fn related_resources(vc: &VerifiableCredential) -> Vec<serde_json::Value> {
        serde_json::to_value(vc).unwrap()["relatedResource"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn a_credential_with_an_integrity_assertion_names_its_statement_by_related_resource() {
        use crate::domain::vc::{verify_credential_db, VerificationPolicy};
        use crate::sentinel::scoring::resolve_coverage_statement;
        use base64::Engine as _;
        let (db, key, _, subject) = setup();
        assessed(db.conn(), "named", false, 3, 3);
        let mut req = sample_request(subject.clone());
        req.integrity_session_id = Some("named".into());
        let vc = issue(db.conn(), &key, &req, NOW).unwrap();

        let digest = vc
            .integrity
            .as_ref()
            .and_then(|a| a.coverage_statement_digest.clone())
            .expect("the assertion keeps coverageStatementDigest");
        let related = related_resources(&vc);
        assert_eq!(related.len(), 1);
        assert_eq!(
            related[0]["id"],
            format!("urn:alexandria:coverage-statement:{digest}"),
            "no status host: the URN form"
        );
        let statement = resolve_coverage_statement(&digest).unwrap();
        let sha256 = <sha2::Sha256 as sha2::Digest>::digest(statement.bytes());
        assert_eq!(
            related[0]["digestSRI"],
            format!(
                "sha256-{}",
                base64::engine::general_purpose::STANDARD.encode(sha256)
            )
        );
        let verified = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(verified.valid_signature);
        assert!(!verified.coverage_statement_mismatch);
    }

    #[test]
    fn with_a_status_host_the_statement_is_named_at_that_host() {
        use crate::settings::{registry::keys, SettingsStore};
        let (db, key, _, subject) = setup();
        SettingsStore::set(
            db.conn(),
            keys::CREDENTIAL_STATUS_HOST,
            "https://cloud.example/".to_string(),
        )
        .unwrap();
        assessed(db.conn(), "hosted", false, 3, 3);
        let mut req = sample_request(subject.clone());
        req.integrity_session_id = Some("hosted".into());
        let vc = issue(db.conn(), &key, &req, NOW).unwrap();
        let related = related_resources(&vc);
        let digest = vc.integrity.unwrap().coverage_statement_digest.unwrap();
        assert_eq!(
            related[0]["id"],
            format!("https://cloud.example/coverage-statements/{digest}")
        );
    }

    #[test]
    fn a_credential_without_an_integrity_assertion_names_no_related_resource() {
        let (db, key, _, subject) = setup();
        let vc = issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        assert!(vc.integrity.is_none());
        assert!(related_resources(&vc).is_empty());
        assert!(!serde_json::to_string(&vc)
            .unwrap()
            .contains("relatedResource"));
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

    #[test]
    fn satisfied_probes_raise_the_rung_and_skipped_ones_leave_it_at_process() {
        let (db, key, _, subject) = setup();
        assessed(db.conn(), "probed", false, 2, 2);
        verified(db.conn(), "probed", "probe", "authored", "satisfied");
        verified(db.conn(), "probed", "probe", "authored", "satisfied");
        let a = assertion_for(db.conn(), &key, &subject, "probed");
        assert_eq!(a.verification_rung.as_deref(), Some("probed"));
        assert_eq!(
            a.probe,
            Some(ProbeSummary {
                outcome: "satisfied".into(),
                policies: vec!["authored".into()],
                satisfied: 2,
                failed: 0,
                skipped: 0,
            })
        );

        // A probe that timed out never counts against the learner, but the
        // probe policy was not satisfied either.
        assessed(db.conn(), "timeout", false, 2, 2);
        verified(db.conn(), "timeout", "probe", "authored", "satisfied");
        verified(db.conn(), "timeout", "probe", "authored", "skipped");
        let a = assertion_for(db.conn(), &key, &subject, "timeout");
        assert_eq!(a.verification_rung.as_deref(), Some("process"));
        assert_eq!(a.probe.as_ref().map(|p| p.skipped), Some(1));
    }

    #[test]
    fn a_satisfied_reseed_probe_is_reported_but_leaves_the_rung_at_process() {
        let (db, key, _, subject) = setup();
        assessed(db.conn(), "again", false, 2, 2);
        verified(db.conn(), "again", "probe", "reseed", "satisfied");
        verified(db.conn(), "again", "probe", "reseed", "satisfied");
        let a = assertion_for(db.conn(), &key, &subject, "again");
        assert_eq!(a.verification_rung.as_deref(), Some("process"));
        assert_eq!(
            a.probe,
            Some(ProbeSummary {
                outcome: "satisfied".into(),
                policies: vec!["reseed".into()],
                satisfied: 2,
                failed: 0,
                skipped: 0,
            }),
            "the probe still ran and is reported as it went"
        );
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
                CredentialError::Policy(PolicyRefusal::RungBelowMinimum { rung, required })
                    if rung == "process" && required == "probed"
            ),
            "{err:?}"
        );
        assert_eq!(
            err.to_string(),
            "issuance policy: verification rung 'process' does not meet the 'probed' minimum"
        );

        // The same item asked again does not lift the rung; a new question
        // does.
        verified(db.conn(), "process", "probe", "reseed", "satisfied");
        assert!(issue(db.conn(), &key, &req, NOW).is_err());
        verified(db.conn(), "process", "probe", "authored", "satisfied");
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
            CredentialError::Policy(PolicyRefusal::ProbePolicyNotRun { .. })
        ));

        // Probes ran, but under another policy.
        assessed(db.conn(), "other", false, 2, 2);
        verified(db.conn(), "other", "probe", "authored", "satisfied");
        assert!(matches!(
            refusal(&req, "other").unwrap_err(),
            CredentialError::Policy(PolicyRefusal::ProbePolicyNotRun { .. })
        ));

        // A probe that failed is a failed verification.
        assessed(db.conn(), "failed", false, 2, 2);
        verified(db.conn(), "failed", "probe", "reseed", "failed");
        let err = refusal(&req, "failed").unwrap_err();
        assert!(
            matches!(
                err,
                CredentialError::Policy(PolicyRefusal::ProbeFailed { .. })
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
            CredentialError::PolicyFloorDisagrees { .. }
        ));

        // A session whose profile does not run the camera does not meet it.
        session_on_profile(db.conn(), "core", false, false, Some(TrustProfile::Camera));
        req.integrity_session_id = Some("core".into());
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyRefusal::TrustProfileBelowFloor { .. })
        ));
    }

    #[test]
    fn a_camera_floor_is_judged_from_the_coverage_the_assertion_states() {
        use crate::sentinel::scoring::TrustProfile;
        let (db, key, _, subject) = setup();
        let mut req = sample_request(subject.clone());
        req.integrity_policy = Some(floor_policy(TrustProfile::Camera));

        // Every window carried camera checks: the floor is met.
        session_on_profile(db.conn(), "whole", false, true, Some(TrustProfile::Camera));
        camera_windows(db.conn(), "whole", 10, 0);
        req.integrity_session_id = Some("whole".into());
        let a = issue(db.conn(), &key, &req, NOW)
            .unwrap()
            .integrity
            .unwrap();
        assert!(a.signals_degraded.is_empty());
        assert_eq!(a.camera_coverage, Some(1.0));

        // One window in twelve without camera checks: degraded, and said so,
        // but the stated coverage (11/12) is at or above the minimum, so the
        // shared rule credits the camera and the credential issues.
        session_on_profile(db.conn(), "brief", false, true, Some(TrustProfile::Camera));
        camera_windows(db.conn(), "brief", 12, 1);
        req.integrity_session_id = Some("brief".into());
        let a = issue(db.conn(), &key, &req, NOW)
            .unwrap()
            .integrity
            .unwrap();
        assert_eq!(a.trust_profile.as_deref(), Some("camera"));
        assert_eq!(a.signals_run, vec!["core", "camera"]);
        assert_eq!(a.signals_degraded, vec!["camera"]);
        assert_eq!(a.signals_not_run, vec!["device_attestation"]);
        assert_eq!(a.camera_coverage, Some(11.0 / 12.0));

        // One window in ten: exactly at the minimum, still met.
        session_on_profile(db.conn(), "edge", false, true, Some(TrustProfile::Camera));
        camera_windows(db.conn(), "edge", 10, 1);
        req.integrity_session_id = Some("edge".into());
        let a = issue(db.conn(), &key, &req, NOW)
            .unwrap()
            .integrity
            .unwrap();
        assert_eq!(a.camera_coverage, Some(0.9));

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
            CredentialError::Policy(PolicyRefusal::TrustProfileBelowFloor { .. })
        ));
        let a = assertion_for(db.conn(), &key, &subject, "covered");
        assert_eq!(a.camera_coverage, Some(0.5));

        // A camera session with no committed windows has no coverage at all.
        session_on_profile(db.conn(), "empty", false, true, Some(TrustProfile::Camera));
        req.integrity_session_id = Some("empty".into());
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyRefusal::TrustProfileBelowFloor { .. })
        ));
        // Without a floor it still issues, and the assertion says it degraded
        // and states a coverage of 0.
        let a = assertion_for(db.conn(), &key, &subject, "empty");
        assert_eq!(a.signals_degraded, vec!["camera"]);
        assert_eq!(a.camera_coverage, Some(0.0));
    }

    #[test]
    fn an_assertion_without_the_camera_states_no_coverage_and_keeps_its_wire_shape() {
        let (db, key, _, subject) = setup();
        // A profile without the camera: the field is left out of the wire
        // form, so such credentials keep the bytes they had before it existed.
        session_on_profile(db.conn(), "core", false, false, None);
        let vc = {
            let mut req = sample_request(subject);
            req.integrity_session_id = Some("core".into());
            issue(db.conn(), &key, &req, NOW).unwrap()
        };
        let a = vc.integrity.as_ref().unwrap();
        assert_eq!(a.trust_profile.as_deref(), Some("core"));
        assert_eq!(a.camera_coverage, None);
        let wire = serde_json::to_value(&vc).unwrap();
        assert!(wire["integrity"].get("cameraCoverage").is_none());
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
            CredentialError::Policy(PolicyRefusal::TrustProfileBelowFloor { .. })
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

        // `device_attested` is met only by verified credential-bound
        // evidence, which this credential does not carry.
        for required in ["device_attested", "anchored", "high_assurance"] {
            req.integrity_policy = Some(IssuancePolicy {
                required_assurance_level: Some(required.into()),
                ..Default::default()
            });
            let error = issue(db.conn(), &key, &req, NOW).unwrap_err();
            assert!(
                matches!(
                    &error,
                    CredentialError::Policy(PolicyRefusal::AssuranceBelowRequired { level, required: wanted })
                        if level == "local" && wanted == required
                ),
                "{error:?}"
            );
        }

        // A required level is a minimum: `local` is met by every credential.
        req.integrity_policy = Some(IssuancePolicy {
            required_assurance_level: Some("local".into()),
            ..Default::default()
        });
        assert!(issue(db.conn(), &key, &req, NOW).is_ok());

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
                CredentialError::Policy(PolicyRefusal::NotClean { status }) if status == "suspended"
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
            CredentialError::Policy(PolicyRefusal::IntegrityBelowMinimum { .. })
        ));
        req.integrity_policy = Some(IssuancePolicy {
            max_critical: Some(1),
            ..Default::default()
        });
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyRefusal::TooManyCritical { count: 2, max: 1 })
        ));
        req.integrity_policy = Some(IssuancePolicy {
            max_warning: Some(0),
            ..Default::default()
        });
        assert!(matches!(
            issue(db.conn(), &key, &req, NOW).unwrap_err(),
            CredentialError::Policy(PolicyRefusal::TooManyWarnings { count: 1, max: 0 })
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
        // Two entries: revocation and suspension, each a Bitstring Status
        // List entry into its own list, at the same index.
        let json = serde_json::to_value(&vc).unwrap();
        let entries = json["credentialStatus"]
            .as_array()
            .expect("a list of entries");
        let purposes: Vec<_> = entries.iter().map(|e| e["statusPurpose"].clone()).collect();
        assert_eq!(purposes, ["revocation", "suspension"]);
        let issuer = vc.issuer.as_str();
        for (entry, number) in entries.iter().zip([1, 2]) {
            assert_eq!(entry["type"], "BitstringStatusListEntry");
            assert_eq!(entry["statusListIndex"], "0");
            assert_eq!(
                entry["statusListCredential"],
                format!("urn:alexandria:status-list:{issuer}:{number}")
            );
        }
    }

    #[test]
    fn issue_credential_allocates_sequential_indices() {
        // Each new credential from the same issuer gets the next bit
        // in the status list, never reusing an index even after revoke.
        let (db, key, _, subject) = setup();
        let a = issue(db.conn(), &key, &sample_request(subject.clone()), NOW).unwrap();
        let b = issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        for (vc, index) in [(a, "0"), (b, "1")] {
            assert!(vc
                .status_entries()
                .iter()
                .all(|e| e.status_list_index == index));
        }
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
