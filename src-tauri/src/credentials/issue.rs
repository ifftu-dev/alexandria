//! Issuing a credential: build the envelope, take a status-list slot, sign
//! it, store it, and gate it on the bound integrity session when there is
//! one.

use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};

// Imported rather than redeclared. These were local copies, and a duplicated
// constant is exactly how the issuance path came to emit the v1 context while
// the rest of the codebase had moved to v2.
use alexandria_verify::vc::context::W3C_VC_V2;

use super::status_list;
use crate::crypto::did::Did;
use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
use crate::domain::vc::{
    integrity_hash_of, Claim, CredentialStatus, CredentialType, IntegrityAssertion, Proof,
    VerifiableCredential,
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
    /// Minimum fraction of snapshots (in `[0,1]`) captured with the camera
    /// opted in. `Some(1.0)` means the camera was on for the whole
    /// session; a session with no snapshots never satisfies it. Evaluated
    /// against the stored snapshots, not the embedded assertion, so the VC
    /// schema is unchanged.
    #[serde(default)]
    pub min_camera_coverage: Option<f64>,
}

impl IssuancePolicy {
    /// Returns `Err(reason)` for the first bound the assertion violates.
    fn evaluate(&self, a: &IntegrityAssertion, facts: &SessionFacts) -> Result<(), String> {
        if self.require_clean && a.status != "completed" {
            return Err(format!(
                "issuance policy: session status is '{}', requires 'completed'",
                a.status
            ));
        }
        if let Some(min) = self.min_integrity {
            match a.integrity_score {
                Some(score) if score >= min => {}
                Some(score) => {
                    return Err(format!(
                        "issuance policy: integrity score {score:.3} below minimum {min:.3}"
                    ))
                }
                None => {
                    return Err(
                        "issuance policy: minimum integrity required but session has no score"
                            .into(),
                    )
                }
            }
        }
        if let Some(max) = self.max_critical {
            if a.critical_count > max {
                return Err(format!(
                    "issuance policy: {} critical flags exceed maximum {max}",
                    a.critical_count
                ));
            }
        }
        if let Some(max) = self.max_warning {
            if a.warning_count > max {
                return Err(format!(
                    "issuance policy: {} warning flags exceed maximum {max}",
                    a.warning_count
                ));
            }
        }
        if let Some(req) = &self.required_assurance_level {
            if &a.assurance_level != req {
                return Err(format!(
                    "issuance policy: assurance level '{}' does not meet required '{req}'",
                    a.assurance_level
                ));
            }
        }
        if let Some(min) = self.min_camera_coverage {
            match facts.camera_coverage {
                Some(cov) if cov >= min => {}
                Some(cov) => {
                    return Err(format!(
                        "issuance policy: camera coverage {cov:.2} below minimum {min:.2}"
                    ))
                }
                None => {
                    return Err(
                        "issuance policy: camera coverage required but session has no snapshots"
                            .into(),
                    )
                }
            }
        }
        Ok(())
    }
}

/// Facts about the bound session that the assertion does not carry but a
/// policy may gate on. Read from the session's snapshots at issuance.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionFacts {
    /// `camera snapshots / all snapshots`, or `None` without snapshots.
    pub camera_coverage: Option<f64>,
}

impl SessionFacts {
    fn load(conn: &Connection, session_id: &str) -> Result<Self, String> {
        let (total, with_camera): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN camera_score IS NOT NULL THEN 1 ELSE 0 END), 0)
                 FROM integrity_snapshots WHERE session_id = ?1",
                params![session_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        Ok(Self {
            camera_coverage: (total > 0).then(|| with_camera as f64 / total as f64),
        })
    }
}

/// Load an integrity session and summarise it as an `IntegrityAssertion`
/// for embedding at issuance. `assurance_level` is `local` unless the
/// session's stored device attestation verifies right now against `trust`
/// (see `sentinel::attestation::effective_assurance`); the stored level is
/// never copied. No anchor reference is embedded.
pub(crate) fn build_integrity_assertion(
    conn: &Connection,
    session_id: &str,
    now: &str,
    trust: &crate::sentinel::attestation::TrustConfig,
) -> Result<IntegrityAssertion, String> {
    use crate::sentinel::attestation::{effective_assurance, SessionAttestationFacts};
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
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("integrity session {session_id} not found"))?;
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
    };
    let (assurance_level, unverified) =
        effective_assurance(&facts, trust, chrono::Utc::now().timestamp());
    if let Some(why) = unverified {
        log::warn!(
            target: "sentinel",
            "session {session_id}: device attestation not credited: {why:?}"
        );
    }
    Ok(IntegrityAssertion {
        session_id: session_id.to_string(),
        status,
        integrity_score,
        critical_count,
        warning_count,
        assurance_level,
        commitment_root,
        anchor_ref: None,
        generated_at: now.to_string(),
    })
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
    issuer_key: &SigningKey,
    issuer_did: &Did,
    req: &IssueCredentialRequest,
    now: &str,
) -> Result<VerifiableCredential, String> {
    if !req.subject.as_str().starts_with("did:") {
        return Err("subject MUST be a DID (§10 non-transferability)".into());
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
    )?;

    // § Integrity→VC bridge: summarise the bound session, enforce the
    // optional issuance gate, and embed the attestation in the envelope.
    // A policy without a session is a caller error; a session without a
    // policy embeds the attestation but gates on nothing.
    let integrity = match &req.integrity_session_id {
        Some(session_id) => {
            let assertion = build_integrity_assertion(
                conn,
                session_id,
                now,
                &crate::sentinel::attestation::TrustConfig::from_profile(),
            )?;
            if let Some(policy) = &req.integrity_policy {
                let facts = SessionFacts::load(conn, session_id)?;
                policy.evaluate(&assertion, &facts)?;
            }
            Some(assertion)
        }
        None => {
            if req.integrity_policy.is_some() {
                return Err(
                    "integrity_policy requires integrity_session_id to evaluate against".into(),
                );
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
    .map_err(|e| format!("sign: {e}"))?;

    let signed_json = serde_json::to_string(&signed).map_err(|e| e.to_string())?;
    let integrity_hash = integrity_hash_of(&signed)?;

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
            .optional()
            .map_err(|e| e.to_string())?;
        let (prior_issuer, prior_subject, prior_kind) =
            prior.ok_or_else(|| format!("supersedes target {prior_id} not found locally"))?;
        if prior_issuer != issuer_did.as_str() {
            return Err("§11.4: supersession requires same issuer as the prior credential".into());
        }
        if prior_subject != req.subject.as_str() {
            return Err("§11.4: supersession requires same subject as the prior credential".into());
        }
        if prior_kind != claim_kind {
            return Err(
                "§11.4: supersession requires same claim kind as the prior credential".into(),
            );
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
    .map_err(|e| format!("insert credential: {e}"))?;

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
    use crate::crypto::did::derive_did_key;

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

    fn seed_snapshot(conn: &Connection, session: &str, id: &str, camera: Option<f64>) {
        conn.execute(
            "INSERT INTO integrity_snapshots (id, session_id, camera_score, composite_score, captured_at)
             VALUES (?1, ?2, ?3, 0.9, ?4)",
            params![id, session, camera, NOW],
        )
        .unwrap();
    }

    #[test]
    fn camera_coverage_gate_reads_snapshots() {
        let (db, key, issuer, subject) = setup();
        seed_session(db.conn(), "sess_cam", "completed", Some(0.9), 0, 0);
        seed_snapshot(db.conn(), "sess_cam", "snap_1", Some(0.8));
        seed_snapshot(db.conn(), "sess_cam", "snap_2", Some(0.7));
        seed_snapshot(db.conn(), "sess_cam", "snap_3", None);
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("sess_cam".into());

        // 2 of 3 snapshots had the camera: 0.66 coverage.
        req.integrity_policy = Some(IssuancePolicy {
            min_camera_coverage: Some(0.5),
            ..Default::default()
        });
        assert!(issue(db.conn(), &key, &issuer, &req, NOW).is_ok());

        req.integrity_policy = Some(IssuancePolicy {
            min_camera_coverage: Some(0.9),
            ..Default::default()
        });
        let err = issue(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(
            err.contains("camera coverage 0.67 below minimum 0.90"),
            "{err}"
        );
    }

    #[test]
    fn camera_coverage_gate_refuses_sessions_without_snapshots() {
        let (db, key, issuer, subject) = setup();
        seed_session(db.conn(), "sess_empty", "completed", Some(0.9), 0, 0);
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("sess_empty".into());
        req.integrity_policy = Some(IssuancePolicy {
            min_camera_coverage: Some(0.1),
            ..Default::default()
        });
        let err = issue(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(err.contains("no snapshots"), "{err}");

        // Without the camera bound, the same session issues fine.
        req.integrity_policy = Some(IssuancePolicy::default());
        assert!(issue(db.conn(), &key, &issuer, &req, NOW).is_ok());
    }

    #[test]
    fn integrity_assertion_embedded_when_session_bound() {
        let (db, key, issuer, subject) = setup();
        seed_session(db.conn(), "sess_ok", "completed", Some(0.91), 0, 1);
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("sess_ok".into());
        let vc = issue(db.conn(), &key, &issuer, &req, NOW).unwrap();
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
        let (db, key, issuer, subject) = setup();
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
            let error = issue(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
            assert!(error.contains("does not meet required"), "{error}");
        }

        req.integrity_policy = None;
        let vc = issue(db.conn(), &key, &issuer, &req, NOW).unwrap();
        let assertion = vc.integrity.as_ref().expect("integrity assertion embedded");
        assert_eq!(assertion.assurance_level, "local");
        assert_eq!(assertion.anchor_ref, None);
    }

    #[test]
    fn issuance_policy_blocks_when_session_fails_gate() {
        let (db, key, issuer, subject) = setup();
        seed_session(db.conn(), "sess_bad", "suspended", Some(0.30), 2, 1);
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("sess_bad".into());
        req.integrity_policy = Some(IssuancePolicy {
            min_integrity: Some(0.70),
            require_clean: true,
            ..Default::default()
        });
        let err = issue(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(err.contains("issuance policy"), "unexpected error: {err}");
    }

    #[test]
    fn issuance_policy_passes_when_session_meets_gate() {
        let (db, key, issuer, subject) = setup();
        seed_session(db.conn(), "sess_pass", "completed", Some(0.88), 0, 0);
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("sess_pass".into());
        req.integrity_policy = Some(IssuancePolicy {
            min_integrity: Some(0.70),
            max_critical: Some(0),
            require_clean: true,
            ..Default::default()
        });
        let vc = issue(db.conn(), &key, &issuer, &req, NOW).unwrap();
        assert!(vc.integrity.is_some());
        assert!(!vc.proof.proof_value.is_empty());
    }

    #[test]
    fn issuance_policy_without_session_is_rejected() {
        let (db, key, issuer, subject) = setup();
        let mut req = sample_request(subject);
        req.integrity_policy = Some(IssuancePolicy {
            require_clean: true,
            ..Default::default()
        });
        let err = issue(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(err.contains("requires integrity_session_id"), "got: {err}");
    }

    #[test]
    fn issue_credential_returns_signed_vc_with_status_slot() {
        let (db, key, issuer, subject) = setup();
        let vc = issue(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
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
        let (db, key, issuer, subject) = setup();
        let a = issue(
            db.conn(),
            &key,
            &issuer,
            &sample_request(subject.clone()),
            NOW,
        )
        .unwrap();
        let b = issue(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        assert_eq!(a.credential_status.unwrap().status_list_index, "0");
        assert_eq!(b.credential_status.unwrap().status_list_index, "1");
    }

    #[test]
    fn issue_rejects_non_did_subject() {
        let (db, key, issuer, _) = setup();
        let req = sample_request(Did("alice@example.com".into()));
        let err = issue(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(err.contains("DID"), "got {err}");
    }

    #[test]
    fn supersession_marks_old_credential_superseded() {
        use crate::domain::vc::verify_credential_db;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, issuer, subject) = setup();
        let old = issue(
            db.conn(),
            &key,
            &issuer,
            &sample_request(subject.clone()),
            NOW,
        )
        .unwrap();

        // Issue a newer credential that supersedes the old one.
        let mut new_req = sample_request(subject);
        new_req.supersedes = old.id.clone();
        let _new = issue(db.conn(), &key, &issuer, &new_req, NOW).unwrap();

        let result = verify_credential_db(db.conn(), &old, NOW, &VerificationPolicy::default());
        assert!(result.superseded);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    fn supersession_rejects_cross_issuer() {
        // §11.4: same subject, claim kind, and issuer required.
        let (db, key, issuer, subject) = setup();
        let old = issue(
            db.conn(),
            &key,
            &issuer,
            &sample_request(subject.clone()),
            NOW,
        )
        .unwrap();

        let other_key = test_key("other-issuer");
        let other_issuer = derive_did_key(&other_key);
        let mut bad_req = sample_request(subject);
        bad_req.supersedes = old.id.clone();
        let err = issue(db.conn(), &other_key, &other_issuer, &bad_req, NOW).unwrap_err();
        assert!(err.contains("issuer"), "got {err}");
    }

    #[test]
    fn supersession_rejects_unknown_prior_id() {
        let (db, key, issuer, subject) = setup();
        let mut req = sample_request(subject);
        req.supersedes = Some("urn:uuid:does-not-exist".into());
        let err = issue(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(err.contains("not found"), "got {err}");
    }
}
