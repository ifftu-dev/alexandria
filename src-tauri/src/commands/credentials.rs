//! IPC commands for Verifiable Credentials.
//!
//! The public `#[tauri::command]` handlers are thin adapters — they
//! unlock the keystore, derive the issuer's signing key, and delegate
//! to pure functions that take `&Connection` + `&SigningKey` +
//! `&Did`. This split keeps the business logic unit-testable without
//! constructing a full `State<AppState>`.

use crate::profile::scope::ProfileState as State;
use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};

use crate::crypto::did::{derive_did_key, Did};
use crate::crypto::wallet;
use crate::db::executor::DatabaseWorkload;
use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
use crate::domain::vc::{
    Claim, CredentialStatus, CredentialType, IntegrityAssertion, Proof, VerifiableCredential,
    VerificationPendingReason, VerificationResult,
};
use crate::AppState;

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

const STATUS_LIST_BITS: usize = alexandria_verify::vc::status::MIN_BITS; // 16 KiB bitmap per list
const STATUS_LIST_TYPE: &str = "BitstringStatusListEntry";
// Imported rather than redeclared. These were local copies, and a duplicated
// constant is exactly how the issuance path came to emit the v1 context while
// the rest of the codebase had moved to v2.
use alexandria_verify::vc::context::W3C_VC_V2;

/// Pure-function issuance pipeline. Allocates the next status-list
/// slot, builds the VC envelope, signs it, persists both the signed
/// VC and its status-list slot, and returns the signed credential.
pub fn issue_credential_impl(
    conn: &Connection,
    issuer_key: &SigningKey,
    issuer_did: &Did,
    req: &IssueCredentialRequest,
    now: &str,
) -> Result<VerifiableCredential, String> {
    if !req.subject.as_str().starts_with("did:") {
        return Err("subject MUST be a DID (§10 non-transferability)".into());
    }

    let list_id = ensure_status_list(conn, issuer_did)?;
    let index = allocate_status_index(conn, &list_id)?;

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
            type_: STATUS_LIST_TYPE.into(),
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

/// Flip the revocation bit in the issuer's status list and mark the
/// local `credentials` row as revoked. Idempotent — calling it twice
/// leaves the bit set and the row flagged.
pub fn revoke_credential_impl(
    conn: &Connection,
    issuer_did: &Did,
    credential_id: &str,
    reason: &str,
    now: &str,
) -> Result<(), String> {
    let transaction = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let row: Option<(String, String, i64, String)> = transaction
        .query_row(
            "SELECT c.issuer_did, c.status_list_id, c.status_list_index, s.issuer_did \
             FROM credentials c \
             JOIN credential_status_lists s ON s.list_id = c.status_list_id \
             WHERE c.id = ?1",
            params![credential_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .and_then(
            |(credential_issuer, list_id, index, list_issuer)| match (list_id, index) {
                (Some(list_id), Some(index)) => {
                    Some((credential_issuer, list_id, index, list_issuer))
                }
                _ => None,
            },
        );

    let (credential_issuer, list_id, index, list_issuer) =
        row.ok_or_else(|| format!("credential {credential_id} not found or has no status list"))?;
    if credential_issuer != issuer_did.as_str() || list_issuer != issuer_did.as_str() {
        return Err("only the credential issuer may revoke it".to_string());
    }

    // Read current bits, set the revocation bit, write back + bump version.
    let mut bits: Vec<u8> = transaction
        .query_row(
            "SELECT bits FROM credential_status_lists WHERE list_id = ?1",
            params![list_id],
            |r| r.get(0),
        )
        .map_err(|e| format!("load status list: {e}"))?;
    // Bitstring Status List: index 0 is the most significant bit of byte 0.
    alexandria_verify::vc::status::set_bit(&mut bits, index as usize, true)
        .map_err(|e| e.to_string())?;

    transaction
        .execute(
            "UPDATE credential_status_lists \
         SET bits = ?2, version = version + 1, updated_at = ?3 \
         WHERE list_id = ?1 AND issuer_did = ?4",
            params![list_id, bits, now, issuer_did.as_str()],
        )
        .map_err(|e| format!("update status list: {e}"))?;

    transaction
        .execute(
            "UPDATE credentials \
         SET revoked = 1, revoked_at = ?2, revocation_reason = ?3 \
         WHERE id = ?1 AND issuer_did = ?4",
            params![credential_id, now, reason, issuer_did.as_str()],
        )
        .map_err(|e| format!("update credential: {e}"))?;

    transaction.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// §11.3 suspension. Set `suspended = 1` plus optional
/// `suspended_until` for automatic reinstatement at verify time.
/// Idempotent — re-suspending updates the until window.
pub fn suspend_credential_impl(
    conn: &Connection,
    issuer_did: &Did,
    credential_id: &str,
    until: Option<&str>,
    reason: Option<&str>,
    now: &str,
) -> Result<(), String> {
    let updated = conn
        .execute(
            "UPDATE credentials \
             SET suspended = 1, suspended_at = ?2, \
                 suspended_until = ?3, suspended_reason = ?4 \
             WHERE id = ?1 AND issuer_did = ?5",
            params![credential_id, now, until, reason, issuer_did.as_str()],
        )
        .map_err(|e| format!("suspend credential: {e}"))?;
    if updated == 0 {
        return Err(format!(
            "credential {credential_id} not found or caller is not its issuer"
        ));
    }
    Ok(())
}

/// §11.3 reinstatement — clear the suspension flag. Idempotent.
pub fn reinstate_credential_impl(
    conn: &Connection,
    issuer_did: &Did,
    credential_id: &str,
) -> Result<(), String> {
    let updated = conn
        .execute(
            "UPDATE credentials \
         SET suspended = 0, suspended_at = NULL, \
             suspended_until = NULL, suspended_reason = NULL \
         WHERE id = ?1 AND issuer_did = ?2",
            params![credential_id, issuer_did.as_str()],
        )
        .map_err(|e| format!("reinstate credential: {e}"))?;
    if updated == 0 {
        return Err(format!(
            "credential {credential_id} not found or caller is not its issuer"
        ));
    }
    Ok(())
}

pub fn get_credential_impl(
    conn: &Connection,
    credential_id: &str,
) -> Result<Option<VerifiableCredential>, String> {
    let json: Option<String> = conn
        .query_row(
            "SELECT signed_vc_json FROM credentials WHERE id = ?1",
            params![credential_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match json {
        Some(s) => Ok(Some(serde_json::from_str(&s).map_err(|e| e.to_string())?)),
        None => Ok(None),
    }
}

pub fn list_credentials_impl(
    conn: &Connection,
    subject_did: Option<&str>,
    skill_id: Option<&str>,
) -> Result<Vec<VerifiableCredential>, String> {
    let mut sql = String::from("SELECT signed_vc_json FROM credentials WHERE 1=1");
    let mut args: Vec<String> = Vec::new();
    if let Some(s) = subject_did {
        sql.push_str(" AND subject_did = ?");
        args.push(s.to_string());
    }
    if let Some(k) = skill_id {
        sql.push_str(" AND skill_id = ?");
        args.push(k.to_string());
    }
    sql.push_str(" ORDER BY received_at DESC");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args.iter()), |r| {
            r.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        let s = r.map_err(|e| e.to_string())?;
        out.push(serde_json::from_str(&s).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

// --- internal helpers -----------------------------------------------------

/// The origin this identity's status lists are served from, if any.
///
/// The `credentials.status_host` setting wins; the network profile's cloud
/// origin, then the first configured directory, stand in when it is empty.
/// Only `https`, or `http` on loopback, counts — a list named at a plain
/// `http` host could be swapped on the wire, and a verifier would believe it.
pub(crate) fn status_list_host(conn: &Connection) -> Option<String> {
    use crate::settings::{registry::keys, SettingsStore};
    let configured = SettingsStore::get(conn, keys::CREDENTIAL_STATUS_HOST);
    let candidate = if !configured.trim().is_empty() {
        Some(configured.trim().to_string())
    } else if let Some(origin) = crate::network_profile::embedded_preprod()
        .ok()
        .and_then(|profile| profile.cloud_https_origin.clone())
    {
        Some(origin)
    } else {
        super::holder_pull::directories_db(conn)
            .ok()
            .and_then(|directories| directories.into_iter().next().map(|d| d.url))
    };
    candidate
        .map(|origin| origin.trim_end_matches('/').to_string())
        .filter(|origin| {
            (origin.starts_with("https://") || super::holder_pull::is_loopback(origin))
                && alexandria_verify::vc::status::parse_list_url(
                    &alexandria_verify::vc::status::list_url(
                        origin,
                        &Did("did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK".into()),
                        1,
                    ),
                )
                .is_some()
        })
}

/// The list a credential issued now by `issuer_did` belongs to.
///
/// One list per issuer per host. With a host configured the id is the URL the
/// host serves it at (§14.11.2), so any verifier can fetch it; without one it
/// is a URN and the list travels only in exported bundles. A host that
/// changes later starts a fresh list — ids already written into credentials
/// are never rewritten.
pub(crate) fn ensure_status_list(conn: &Connection, issuer_did: &Did) -> Result<String, String> {
    let list_id = match status_list_host(conn) {
        Some(origin) => alexandria_verify::vc::status::list_url(&origin, issuer_did, 1),
        None => format!("urn:alexandria:status-list:{}:1", issuer_did.as_str()),
    };
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM credential_status_lists WHERE list_id = ?1",
            params![list_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        let bits = vec![0u8; STATUS_LIST_BITS / 8];
        conn.execute(
            "INSERT INTO credential_status_lists \
             (list_id, issuer_did, version, status_purpose, bits, bit_length) \
             VALUES (?1, ?2, 1, 'revocation', ?3, ?4)",
            params![list_id, issuer_did.as_str(), bits, STATUS_LIST_BITS as i64],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(list_id)
}

/// A URL-addressed list whose host has not yet seen its current version.
#[derive(Debug, Clone)]
pub(crate) struct PendingStatusList {
    pub list_id: String,
    pub status_purpose: String,
    pub bits: Vec<u8>,
    pub version: i64,
}

/// Lists this issuer owns that are ahead of what their host acknowledged.
///
/// URN-named lists are never pending: nothing serves them.
pub(crate) fn pending_status_lists(
    conn: &Connection,
    issuer_did: &Did,
) -> Result<Vec<PendingStatusList>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT list_id, status_purpose, bits, version FROM credential_status_lists \
             WHERE issuer_did = ?1 AND version > published_version ORDER BY list_id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![issuer_did.as_str()], |r| {
            Ok(PendingStatusList {
                list_id: r.get(0)?,
                status_purpose: r.get(1)?,
                bits: r.get(2)?,
                version: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut pending = Vec::new();
    for row in rows {
        let list = row.map_err(|e| e.to_string())?;
        if alexandria_verify::vc::status::parse_list_url(&list.list_id).is_some() {
            pending.push(list);
        }
    }
    Ok(pending)
}

/// The signed `BitstringStatusListCredential` a host serves for `list`.
pub(crate) fn signed_status_list(
    list: &PendingStatusList,
    key: &SigningKey,
    issuer_did: &Did,
    now: &str,
) -> Result<VerifiableCredential, String> {
    alexandria_verify::vc::status::status_list_credential(
        &list.list_id,
        issuer_did,
        &list.status_purpose,
        &list.bits,
        now,
        key,
    )
    .map_err(|e| format!("sign status list {}: {e}", list.list_id))
}

/// Record that a host acknowledged `version` of each list.
pub(crate) fn mark_status_lists_published(
    conn: &Connection,
    published: &[(String, i64)],
) -> Result<(), String> {
    for (list_id, version) in published {
        conn.execute(
            "UPDATE credential_status_lists SET published_version = ?2 \
             WHERE list_id = ?1 AND published_version < ?2",
            params![list_id, version],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// What one publication pass did.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StatusPublishReport {
    /// List ids the host now serves at their current version.
    pub published: Vec<String>,
    /// One line per list that could not be pushed; the list stays pending.
    pub errors: Vec<String>,
}

/// Push each pending list to its host, signed as of `now`.
///
/// Returns the `(list_id, version)` pairs the host accepted, so the caller
/// can mark them, and the failures, which stay pending for the next pass.
pub(crate) async fn push_status_lists(
    pending: &[PendingStatusList],
    key: &SigningKey,
    issuer_did: &Did,
    now: &str,
) -> (Vec<(String, i64)>, Vec<String>) {
    let mut accepted = Vec::new();
    let mut errors = Vec::new();
    for list in pending {
        match push_status_list(list, key, issuer_did, now).await {
            Ok(()) => accepted.push((list.list_id.clone(), list.version)),
            Err(e) => errors.push(format!("{}: {e}", list.list_id)),
        }
    }
    (accepted, errors)
}

async fn push_status_list(
    list: &PendingStatusList,
    key: &SigningKey,
    issuer_did: &Did,
    now: &str,
) -> Result<(), String> {
    let location = alexandria_verify::vc::status::parse_list_url(&list.list_id)
        .ok_or("list is not served by a host")?;
    if &location.issuer != issuer_did {
        return Err("list belongs to another issuer".into());
    }
    let document = signed_status_list(list, key, issuer_did, now)?;
    let response = super::exchange::client()?
        .put(&list.list_id)
        .json(&document)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "{} returned {status}: {}",
            location.origin,
            body.chars().take(200).collect::<String>()
        ));
    }
    Ok(())
}

/// Publish every pending list this identity owns, and record what landed.
///
/// Called after anything that creates or changes a list, and on demand. A
/// failed push is reported and left pending; the local revocation already
/// happened and stands regardless.
pub async fn publish_status_lists_for(
    state: &State<'_, AppState>,
) -> Result<StatusPublishReport, String> {
    let (key, issuer_did) = load_issuer_key(state).await?;
    let issuer_for_read = issuer_did.clone();
    let pending = state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "credentials.status_lists.pending",
            move |db| pending_status_lists(db.conn(), &issuer_for_read),
        )
        .await?;
    if pending.is_empty() {
        return Ok(StatusPublishReport::default());
    }
    let (accepted, errors) = push_status_lists(&pending, &key, &issuer_did, &now_rfc3339()).await;
    let published = accepted.iter().map(|(id, _)| id.clone()).collect();
    if !accepted.is_empty() {
        state
            .db_executor
            .execute(
                DatabaseWorkload::Instructor,
                state.profile_lease(),
                "credentials.status_lists.mark",
                move |db| mark_status_lists_published(db.conn(), &accepted),
            )
            .await?;
    }
    Ok(StatusPublishReport { published, errors })
}

/// Best-effort publication after a command that touched a list. The command's
/// own result is already decided; a host that is down is a warning here and a
/// pending list for the next pass.
async fn publish_status_lists_quietly(state: &State<'_, AppState>) {
    match publish_status_lists_for(state).await {
        Ok(report) if report.errors.is_empty() => {}
        Ok(report) => log::warn!("status list publication: {:?}", report.errors),
        Err(e) => log::warn!("status list publication: {e}"),
    }
}

fn allocate_status_index(conn: &Connection, list_id: &str) -> Result<i64, String> {
    // Next free index = max allocated + 1. We read from `credentials`
    // rather than scanning the bitmap because gaps from revocations
    // shouldn't be reused (the revoked state is permanent evidence).
    let next: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(status_list_index), -1) + 1 FROM credentials \
             WHERE status_list_id = ?1",
            params![list_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if next >= STATUS_LIST_BITS as i64 {
        return Err(format!("status list {list_id} is full"));
    }
    Ok(next)
}

pub(crate) fn integrity_hash_of(vc: &VerifiableCredential) -> Result<String, String> {
    let mut clone = vc.clone();
    clone.proof.proof_value.clear();
    let value = serde_json::to_value(&clone).map_err(|e| e.to_string())?;
    let bytes = serde_json_canonicalizer::to_vec(&value).map_err(|e| e.to_string())?;
    Ok(hex::encode(blake3::hash(&bytes).as_bytes()))
}

pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub(crate) async fn load_issuer_key(
    state: &State<'_, AppState>,
) -> Result<(SigningKey, Did), String> {
    let ks_guard = state.keystore.lock().await;
    let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(ks_guard);
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    // `Wallet` implements `Drop` (zeroize) so we can't move out — clone
    // the signing key bytes instead.
    let signing_key = SigningKey::from_bytes(&w.signing_key.to_bytes());
    let issuer_did = derive_did_key(&signing_key);
    Ok((signing_key, issuer_did))
}

// --- tauri command handlers ----------------------------------------------

#[tauri::command]
pub async fn issue_credential(
    state: State<'_, AppState>,
    req: IssueCredentialRequest,
) -> Result<VerifiableCredential, String> {
    let (signing_key, issuer_did) = load_issuer_key(&state).await?;
    let now = now_rfc3339();
    let issued = state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "credentials.issue",
            move |db| issue_credential_impl(db.conn(), &signing_key, &issuer_did, &req, &now),
        )
        .await?;
    publish_status_lists_quietly(&state).await;
    Ok(issued)
}

#[tauri::command]
pub async fn list_credentials(
    state: State<'_, AppState>,
    subject: Option<String>,
    skill_id: Option<String>,
) -> Result<Vec<VerifiableCredential>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.list",
            move |db| list_credentials_impl(db.conn(), subject.as_deref(), skill_id.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn get_credential(
    state: State<'_, AppState>,
    credential_id: String,
) -> Result<Option<VerifiableCredential>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.get",
            move |db| get_credential_impl(db.conn(), &credential_id),
        )
        .await
}

#[tauri::command]
pub async fn revoke_credential(
    state: State<'_, AppState>,
    credential_id: String,
    reason: String,
) -> Result<(), String> {
    let (_, issuer_did) = load_issuer_key(&state).await?;
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "credentials.revoke",
            move |db| revoke_credential_impl(db.conn(), &issuer_did, &credential_id, &reason, &now),
        )
        .await?;
    publish_status_lists_quietly(&state).await;
    Ok(())
}

/// Push every status list that is ahead of what its host serves.
#[tauri::command]
pub async fn publish_status_lists(
    state: State<'_, AppState>,
) -> Result<StatusPublishReport, String> {
    publish_status_lists_for(&state).await
}

#[tauri::command]
pub async fn suspend_credential(
    state: State<'_, AppState>,
    credential_id: String,
    until: Option<String>,
    reason: Option<String>,
) -> Result<(), String> {
    let (_, issuer_did) = load_issuer_key(&state).await?;
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "credentials.suspend",
            move |db| {
                suspend_credential_impl(
                    db.conn(),
                    &issuer_did,
                    &credential_id,
                    until.as_deref(),
                    reason.as_deref(),
                    &now,
                )
            },
        )
        .await
}

#[tauri::command]
pub async fn reinstate_credential(
    state: State<'_, AppState>,
    credential_id: String,
) -> Result<(), String> {
    let (_, issuer_did) = load_issuer_key(&state).await?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "credentials.reinstate",
            move |db| reinstate_credential_impl(db.conn(), &issuer_did, &credential_id),
        )
        .await
}

/// Add a (credential_id, requestor_did) entry to the per-credential
/// vc-fetch allowlist. Pass the literal string `"public"` to mark
/// the credential as world-fetchable.
#[tauri::command]
pub async fn allow_credential_fetch(
    state: State<'_, AppState>,
    credential_id: String,
    requestor_did: String,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.allow_fetch",
            move |db| crate::p2p::vc_fetch::allow_fetch(db.conn(), &credential_id, &requestor_did),
        )
        .await
}

/// Remove a (credential_id, requestor_did) entry from the
/// allowlist. Idempotent.
#[tauri::command]
pub async fn disallow_credential_fetch(
    state: State<'_, AppState>,
    credential_id: String,
    requestor_did: String,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.disallow_fetch",
            move |db| {
                crate::p2p::vc_fetch::disallow_fetch(db.conn(), &credential_id, &requestor_did)
            },
        )
        .await
}

#[tauri::command]
pub async fn verify_credential_cmd(
    state: State<'_, AppState>,
    credential: VerifiableCredential,
) -> Result<VerificationResult, String> {
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.verify",
            move |db| {
                Ok(crate::domain::vc::verify_credential_db(
                    db.conn(),
                    &credential,
                    &now,
                    &crate::domain::vc::VerificationPolicy::default(),
                ))
            },
        )
        .await
}

// ---------------------------------------------------------------------------
// Survivability — credential bundle export + offline verification (§20.4).
//
// The export bundle is a single JSON document carrying everything a
// third-party verifier needs to re-check the credentials without any
// Alexandria infrastructure: the signed VCs themselves, the historical
// key registry, and the revocation status lists.
//
// Determinism comes from JCS canonicalization — same inputs ⇒
// byte-identical bundle, which is what the survivability tests assert
// and what archival storage relies on for content-addressing.
// ---------------------------------------------------------------------------

/// Bundle wire shape. Keys sort under JCS so the canonical bytes are
/// stable across implementations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CredentialBundle {
    pub format_version: String,
    pub credentials: Vec<VerifiableCredential>,
    pub key_registry: Vec<KeyRegistryRow>,
    pub status_lists: Vec<StatusListRow>,
    /// The same lists as signed `BitstringStatusListCredential` documents,
    /// for the lists this node issues. A verifier that trusts nothing about
    /// the bundle's author can still check these against the issuer's key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub status_list_credentials: Vec<VerifiableCredential>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyRegistryRow {
    pub did: String,
    pub key_id: String,
    pub public_key_hex: String,
    pub valid_from: String,
    pub valid_until: Option<String>,
    pub rotated_by: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StatusListRow {
    pub list_id: String,
    pub issuer_did: String,
    pub version: i64,
    pub status_purpose: String,
    /// Base64-encoded bitmap.
    pub bits_b64: String,
    pub bit_length: i64,
}

const BUNDLE_FORMAT_VERSION: &str = "alexandria-credential-bundle/1.0";

/// Structural limits for an untrusted credential payload: a bundle, a list of
/// credentials, or one credential. A bundle's status lists carry base64
/// bitmaps up to the status-list bitmap cap, so its strings may be longer than
/// a credential's; every credential entry is still held to
/// [`alexandria_verify::vc::CREDENTIAL_JSON_LIMITS`].
pub const CREDENTIAL_PAYLOAD_JSON_LIMITS: alexandria_verify::json::JsonLimits =
    alexandria_verify::json::JsonLimits {
        max_bytes: 16 * 1024 * 1024,
        max_depth: 32,
        max_array_len: 4096,
        max_object_entries: 256,
        max_string_bytes: 4 * crate::p2p::vc_status::MAX_BITS_BYTES.div_ceil(3),
    };

/// Parse an untrusted credential payload under
/// [`CREDENTIAL_PAYLOAD_JSON_LIMITS`] before any typed decoding.
pub(crate) fn parse_credential_payload(payload: &str) -> Result<serde_json::Value, String> {
    alexandria_verify::json::parse_untrusted(payload.as_bytes(), &CREDENTIAL_PAYLOAD_JSON_LIMITS)
        .map_err(|error| format!("not a valid credential payload: {error}"))
}

/// Decode one credential entry of a parsed payload under the single-credential
/// limits, so a bundle or list cannot carry a credential a direct import would
/// refuse.
pub(crate) fn credential_entry(
    entry: &serde_json::Value,
) -> Result<VerifiableCredential, alexandria_verify::json::UntrustedJsonError> {
    let bytes = serde_json::to_vec(entry)
        .map_err(|error| alexandria_verify::json::UntrustedJsonError::Invalid(error.to_string()))?;
    alexandria_verify::vc::decode_credential(&bytes)
}

fn bounded_credentials(entries: &[serde_json::Value]) -> Result<Vec<VerifiableCredential>, String> {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            credential_entry(entry).map_err(|error| format!("credential {index}: {error}"))
        })
        .collect()
}

/// Decode a parsed payload as a §20.4 bundle. `None` means the payload is not
/// a bundle; an unsupported format version or an over-limit credential entry
/// is an error.
fn bundle_from_payload(value: &serde_json::Value) -> Result<Option<CredentialBundle>, String> {
    let Ok(mut bundle) = serde_json::from_value::<CredentialBundle>(value.clone()) else {
        return Ok(None);
    };
    if bundle.format_version != BUNDLE_FORMAT_VERSION {
        return Err(format!(
            "unsupported bundle format_version: {}",
            bundle.format_version
        ));
    }
    let entries = value
        .get("credentials")
        .and_then(serde_json::Value::as_array)
        .ok_or("bundle credentials are not a list")?;
    bundle.credentials = bounded_credentials(entries)?;
    Ok(Some(bundle))
}

/// Build a JCS-canonical export bundle of every credential, key
/// registry row, and status list known to this node.
pub fn export_bundle_impl(conn: &Connection) -> Result<String, String> {
    export_bundle_signed_impl(conn, None)
}

/// As [`export_bundle_impl`], and when `signer` is this node's issuer key,
/// also carry each list this node issues as a signed
/// `BitstringStatusListCredential`.
pub fn export_bundle_signed_impl(
    conn: &Connection,
    signer: Option<(&SigningKey, &Did)>,
) -> Result<String, String> {
    use base64::Engine;

    // Credentials, ordered deterministically by id so ad-hoc ordering
    // in the credentials table doesn't leak into the bundle.
    let mut stmt = conn
        .prepare("SELECT signed_vc_json FROM credentials ORDER BY id")
        .map_err(|e| e.to_string())?;
    let cred_rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut credentials = Vec::new();
    for r in cred_rows {
        let json = r.map_err(|e| e.to_string())?;
        credentials.push(serde_json::from_str(&json).map_err(|e| e.to_string())?);
    }

    let mut stmt = conn
        .prepare(
            "SELECT did, key_id, public_key_hex, valid_from, valid_until, rotated_by \
             FROM key_registry ORDER BY did, key_id",
        )
        .map_err(|e| e.to_string())?;
    let key_rows = stmt
        .query_map([], |r| {
            Ok(KeyRegistryRow {
                did: r.get(0)?,
                key_id: r.get(1)?,
                public_key_hex: r.get(2)?,
                valid_from: r.get(3)?,
                valid_until: r.get(4)?,
                rotated_by: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut key_registry = Vec::new();
    for r in key_rows {
        key_registry.push(r.map_err(|e| e.to_string())?);
    }

    let mut stmt = conn
        .prepare(
            "SELECT list_id, issuer_did, version, status_purpose, bits, bit_length \
             FROM credential_status_lists ORDER BY list_id",
        )
        .map_err(|e| e.to_string())?;
    let list_rows = stmt
        .query_map([], |r| {
            let bits: Vec<u8> = r.get(4)?;
            Ok(StatusListRow {
                list_id: r.get(0)?,
                issuer_did: r.get(1)?,
                version: r.get(2)?,
                status_purpose: r.get(3)?,
                bits_b64: base64::engine::general_purpose::STANDARD.encode(&bits),
                bit_length: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut status_lists = Vec::new();
    for r in list_rows {
        status_lists.push(r.map_err(|e| e.to_string())?);
    }
    let mut status_list_credentials = Vec::new();
    if let Some((key, did)) = signer {
        let created = now_rfc3339();
        for list in status_lists.iter().filter(|l| l.issuer_did == did.as_str()) {
            let bits = base64::engine::general_purpose::STANDARD
                .decode(list.bits_b64.as_bytes())
                .map_err(|e| format!("decode list bits: {e}"))?;
            status_list_credentials.push(
                alexandria_verify::vc::status::status_list_credential(
                    &list.list_id,
                    did,
                    &list.status_purpose,
                    &bits,
                    &created,
                    key,
                )
                .map_err(|e| format!("sign status list: {e}"))?,
            );
        }
    }

    let bundle = CredentialBundle {
        format_version: BUNDLE_FORMAT_VERSION.into(),
        credentials,
        key_registry,
        status_lists,
        status_list_credentials,
    };
    serde_json_canonicalizer::to_string(&bundle).map_err(|e| format!("canonicalize bundle: {e}"))
}

/// Verify a bundle with no dependence on the calling node's state —
/// loads the bundle into a fresh in-memory DB and runs each VC
/// through the full §13.2 verification pipeline. Returns
/// `(accepted, total)`.
///
/// This is the in-process analogue of "shell out to digitalbazaar/
/// vc-js" — same offline guarantee, no Alexandria infrastructure
/// required, except the verifier itself.
/// What a piece of JSON handed to the offline verifier turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OfflineSource {
    /// A §20.4 survivability bundle, carrying its own keys and status lists.
    Bundle,
    /// One bare credential.
    Credential,
    /// A JSON array of bare credentials.
    Credentials,
}

/// Outcome of verifying whatever was handed in.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineVerification {
    pub source: OfflineSource,
    pub total: u32,
    pub accepted: u32,
    /// Per-credential detail, so a rejection can say which check failed
    /// instead of only that the count came up short.
    pub results: Vec<VerificationResult>,
    /// True when verification had no revocation context: a bare credential
    /// carries no status list, so "not revoked" means "not known to be
    /// revoked". A bundle carries its own, and is therefore conclusive.
    pub revocation_unknown: bool,
}

/// Why a credential was rejected, in terms a reader can act on.
///
/// Order matters. When the issuer cannot be resolved, `verify_credential`
/// returns before the signature is ever checked, so `valid_signature` is false
/// because nothing was verified — not because a signature failed. Listing both
/// would report two independent failures where only one thing happened, and
/// would send the reader looking at the signature when the problem is the DID.
pub fn rejection_reasons(result: &VerificationResult) -> Vec<&'static str> {
    if result.acceptance_decision == crate::domain::vc::AcceptanceDecision::Pending {
        return result
            .pending_reasons
            .iter()
            .map(|reason| match reason {
                VerificationPendingReason::IssuerKeyMissing => "issuer key is missing",
                VerificationPendingReason::IssuerKeyUnavailable => {
                    "issuer key lookup is unavailable"
                }
                VerificationPendingReason::StatusListMissing => "status list is missing",
                VerificationPendingReason::StatusListUnavailable => {
                    "status list lookup is unavailable"
                }
                VerificationPendingReason::SuspensionStateUnavailable => {
                    "suspension lookup is unavailable"
                }
                VerificationPendingReason::SupersessionStateUnavailable => {
                    "supersession lookup is unavailable"
                }
            })
            .collect();
    }
    if !result.issuer_resolved {
        return vec!["issuer DID could not be resolved — signature not checked"];
    }

    let mut reasons = Vec::new();
    if !result.valid_signature {
        reasons.push("signature does not match the issuer key");
    }
    if !result.subject_bound {
        reasons.push("subject is not a DID");
    }
    if result.expired {
        reasons.push("expired");
    }
    if result.revoked {
        reasons.push("revoked");
    }
    if !result.status_valid {
        reasons.push("invalid status reference");
    }
    if result.suspended {
        reasons.push("suspended");
    }
    if result.superseded {
        reasons.push("superseded");
    }
    reasons
}

/// Verify a bundle, a single credential, or an array of credentials.
///
/// Callers should not have to know which shape they were sent. A bare
/// credential still verifies meaningfully offline — the signature checks
/// against the issuer's `did:key`, which embeds the public key, and expiry and
/// subject binding are properties of the document — so refusing it merely
/// because it is not wrapped in a bundle would be an artificial limitation.
///
/// What a bare credential cannot tell you is revocation, suspension, or
/// supersession: those are facts held elsewhere. [`NullStore`] reports them as
/// "not known", and `revocation_unknown` says so rather than letting a caller
/// read the result as a clean bill of health.
pub fn verify_offline_impl(json: &str, now: &str) -> Result<OfflineVerification, String> {
    use crate::domain::vc::{verify, AcceptanceDecision, NullStore, VerificationPolicy};

    let policy = VerificationPolicy::default();
    let tally = |results: Vec<VerificationResult>, source, revocation_unknown| {
        let total = results.len() as u32;
        let accepted = results
            .iter()
            .filter(|r| r.acceptance_decision == AcceptanceDecision::Accept)
            .count() as u32;
        OfflineVerification {
            source,
            total,
            accepted,
            results,
            revocation_unknown,
        }
    };

    // Structural limits, duplicate keys and unsafe numbers are checked once,
    // before any shape is tried.
    let value = parse_credential_payload(json)?;

    // A bundle is the most specific shape, so it is tried first: it has a
    // `format_version` that neither of the others carries.
    if let Some(bundle) = bundle_from_payload(&value)? {
        let store = BundleStore::new(&bundle)?;
        let results = bundle
            .credentials
            .iter()
            .map(|vc| verify::verify_credential(&store, vc, now, &policy))
            .collect();
        return Ok(tally(results, OfflineSource::Bundle, false));
    }

    if serde_json::from_value::<VerifiableCredential>(value.clone()).is_ok() {
        let vc = credential_entry(&value).map_err(|error| format!("credential: {error}"))?;
        let results = vec![verify::verify_credential(&NullStore, &vc, now, &policy)];
        return Ok(tally(results, OfflineSource::Credential, true));
    }

    if let Some(entries) = value
        .as_array()
        .filter(|_| serde_json::from_value::<Vec<VerifiableCredential>>(value.clone()).is_ok())
    {
        let list = bounded_credentials(entries)?;
        let results = list
            .iter()
            .map(|vc| verify::verify_credential(&NullStore, vc, now, &policy))
            .collect();
        return Ok(tally(results, OfflineSource::Credentials, true));
    }

    // Say what was expected rather than echoing whichever parse failed last —
    // the bundle error ("missing field `format_version`") is confusing when
    // the caller handed over a perfectly good credential.
    Err("not recognised as a credential, a list of credentials, or a §20.4 bundle".into())
}

pub fn verify_bundle_offline_impl(
    bundle_json: &str,
    verification_time: &str,
) -> Result<(u32, u32), String> {
    use crate::domain::vc::{verify, AcceptanceDecision, VerificationPolicy};

    let value = parse_credential_payload(bundle_json).map_err(|e| format!("parse bundle: {e}"))?;
    let bundle = bundle_from_payload(&value)?
        .ok_or_else(|| "parse bundle: not a §20.4 credential bundle".to_string())?;

    let store = BundleStore::new(&bundle)?;

    let total = bundle.credentials.len() as u32;
    let mut accepted = 0u32;
    let policy = VerificationPolicy::default();
    for vc in &bundle.credentials {
        let result = verify::verify_credential(&store, vc, verification_time, &policy);
        if result.acceptance_decision == AcceptanceDecision::Accept {
            accepted += 1;
        }
    }
    Ok((accepted, total))
}

/// A [`VerificationStore`] over a credential bundle's own contents.
///
/// A bundle is self-contained: it carries the key registry entries and status
/// lists needed to check the credentials inside it, and nothing else. So the
/// answer to "is this suspended?" or "is this superseded?" is always no — those
/// are facts about a local collection, and a bundle is not one.
///
/// This used to be done by opening an in-memory SQLite database, running the
/// entire application migration suite against it, and INSERTing the bundle's
/// rows — solely because `verify_credential` demanded a `&Connection`. It never
/// needed a database; it needed four lookups.
struct BundleStore {
    keys: Vec<KeyRegistryRow>,
    status_lists: Vec<(String, Vec<u8>)>,
}

impl BundleStore {
    /// Status-list bits are decoded up front so a corrupt bundle fails loudly
    /// here rather than being silently read as "nothing is revoked" later.
    fn new(bundle: &CredentialBundle) -> Result<Self, String> {
        use base64::Engine;
        let mut status_lists = Vec::with_capacity(bundle.status_lists.len());
        // A signed BitstringStatusListCredential is evidence on its own
        // terms: it is checked against the issuer's key and, when it
        // verifies, wins over the raw row for the same list id.
        for list_vc in &bundle.status_list_credentials {
            let Some(id) = list_vc.id.as_deref() else {
                return Err("status list credential has no id".into());
            };
            let key = alexandria_verify::did::resolve_did_key(&list_vc.issuer)
                .map_err(|e| format!("status list issuer: {e}"))?;
            let bits = alexandria_verify::vc::status::verify_status_list_credential(list_vc, &key)
                .map_err(|e| format!("status list {id}: {e}"))?;
            status_lists.push((id.to_string(), bits));
        }
        for list in &bundle.status_lists {
            if status_lists.iter().any(|(id, _)| id == &list.list_id) {
                continue;
            }
            let bits = base64::engine::general_purpose::STANDARD
                .decode(list.bits_b64.as_bytes())
                .map_err(|e| format!("decode list bits: {e}"))?;
            status_lists.push((list.list_id.clone(), bits));
        }
        Ok(Self {
            keys: bundle.key_registry.clone(),
            status_lists,
        })
    }
}

impl crate::domain::vc::VerificationStore for BundleStore {
    /// Mirrors the registry query in `crypto::key_registry::resolve_key_at`:
    /// the entry whose `[valid_from, valid_until)` window contains `at`, latest
    /// `valid_from` first.
    fn key_at(
        &self,
        did: &alexandria_verify::did::Did,
        at: &str,
    ) -> crate::domain::vc::StoreLookup<alexandria_verify::did::KeyRegistryEntry> {
        self.keys
            .iter()
            .filter(|e| {
                e.did == did.as_str()
                    && e.valid_from.as_str() <= at
                    && e.valid_until.as_deref().is_none_or(|u| u > at)
            })
            .max_by(|a, b| a.valid_from.cmp(&b.valid_from))
            .and_then(|e| {
                Some(alexandria_verify::did::KeyRegistryEntry {
                    did: did.clone(),
                    key_id: e.key_id.clone(),
                    public_key_bytes: hex::decode(&e.public_key_hex).ok()?,
                    valid_from: e.valid_from.clone(),
                    valid_until: e.valid_until.clone(),
                    rotated_by: e.rotated_by.clone(),
                })
            })
            .map(crate::domain::vc::StoreLookup::Found)
            .unwrap_or(crate::domain::vc::StoreLookup::Missing)
    }

    fn status_list_bits(&self, list_id: &str) -> crate::domain::vc::StoreLookup<Vec<u8>> {
        self.status_lists
            .iter()
            .find(|(id, _)| id == list_id)
            .map(|(_, bits)| bits.clone())
            .map(crate::domain::vc::StoreLookup::Found)
            .unwrap_or(crate::domain::vc::StoreLookup::Missing)
    }

    fn suspension(
        &self,
        _credential_id: &str,
    ) -> crate::domain::vc::StoreLookup<(bool, Option<String>)> {
        crate::domain::vc::StoreLookup::Missing
    }

    fn is_superseded(&self, _credential_id: &str) -> crate::domain::vc::StoreLookup<bool> {
        crate::domain::vc::StoreLookup::Missing
    }
}

#[tauri::command]
pub async fn export_credentials_bundle(state: State<'_, AppState>) -> Result<String, String> {
    let (key, did) = load_issuer_key(&state).await?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.export-bundle",
            move |db| export_bundle_signed_impl(db.conn(), Some((&key, &did))),
        )
        .await
}

// ---------------------------------------------------------------------------
// Tests.
//
// Unit-test the pure `*_impl` functions against an in-memory DB — the
// tauri handlers are thin wrappers around the same business logic, so the
// command-level behaviour is fully covered.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::domain::vc::{AcceptanceDecision, SkillClaim};

    const NOW: &str = "2026-04-13T00:00:00Z";

    fn test_key(role: &str) -> SigningKey {
        let mut bytes = [0u8; 32];
        let b = role.as_bytes();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = b[i % b.len().max(1)];
        }
        SigningKey::from_bytes(&bytes)
    }

    fn setup() -> (Database, SigningKey, Did, Did) {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let issuer_key = test_key("issuer");
        let issuer = derive_did_key(&issuer_key);
        let subject = derive_did_key(&test_key("subject"));
        (db, issuer_key, issuer, subject)
    }

    fn sample_request(subject: Did) -> IssueCredentialRequest {
        IssueCredentialRequest {
            credential_type: CredentialType::FormalCredential,
            subject,
            claim: Claim::Skill(SkillClaim {
                skill_id: "skill_test".into(),
                level: 4,
                score: 0.82,
                evidence_refs: vec![],
                rubric_version: Some("v1".into()),
                assessment_method: Some("exam".into()),
                provenance: None,
            }),
            evidence_refs: vec!["urn:uuid:e1".into()],
            expiration_date: None,
            supersedes: None,
            integrity_session_id: None,
            integrity_policy: None,
        }
    }

    // ---- Shape-agnostic offline verification ---------------------------

    #[test]
    fn verify_offline_marks_a_status_bearing_bare_credential_pending() {
        // Reported as a bug: pasting a single credential into offline verify
        // failed with "missing field `format_version`", because only the
        // bundle shape was accepted. A credential signed by a `did:key` issuer
        // is verifiable on its own — the DID embeds the public key.
        let (db, issuer_key, issuer, subject) = setup();
        let vc = issue_credential_impl(
            db.conn(),
            &issuer_key,
            &issuer,
            &sample_request(subject),
            NOW,
        )
        .unwrap();

        let json = serde_json::to_string(&vc).unwrap();
        let report = verify_offline_impl(&json, NOW).unwrap();

        assert_eq!(report.source, OfflineSource::Credential);
        assert_eq!((report.accepted, report.total), (0, 1));
        assert!(report.results[0].valid_signature);
        assert!(report.results[0].issuer_resolved, "did:key self-resolution");
        assert_eq!(
            report.results[0].acceptance_decision,
            AcceptanceDecision::Pending
        );
        assert_eq!(
            report.results[0].pending_reasons,
            vec![VerificationPendingReason::StatusListMissing]
        );
        // A bare credential carries no status list, so this must not read as
        // a clean bill of health.
        assert!(report.revocation_unknown);
    }

    #[test]
    fn credential_payloads_are_bounded_before_verification() {
        let (db, issuer_key, issuer, subject) = setup();
        issue_credential_impl(
            db.conn(),
            &issuer_key,
            &issuer,
            &sample_request(subject),
            NOW,
        )
        .unwrap();
        let bundle = export_bundle_impl(db.conn()).unwrap();

        let max = CREDENTIAL_PAYLOAD_JSON_LIMITS.max_bytes;
        let mut exact = bundle.clone();
        exact.push_str(&" ".repeat(max - bundle.len()));
        assert_eq!(verify_bundle_offline_impl(&exact, NOW).unwrap(), (1, 1));
        exact.push(' ');
        let error = verify_bundle_offline_impl(&exact, NOW).unwrap_err();
        assert!(error.contains("exceeds"), "{error}");

        let duplicated = format!("{{\"format_version\":\"forged\",{}", &bundle[1..]);
        let error = verify_offline_impl(&duplicated, NOW).unwrap_err();
        assert!(error.contains("duplicate"), "{error}");

        // The bundle fits, but one credential in it exceeds what a direct
        // import of that credential would accept.
        let mut value: serde_json::Value = serde_json::from_str(&bundle).unwrap();
        value["credentials"][0]["credentialSubject"]["note"] = serde_json::Value::String(
            "a".repeat(alexandria_verify::vc::CREDENTIAL_JSON_LIMITS.max_string_bytes + 1),
        );
        let error = verify_offline_impl(&value.to_string(), NOW).unwrap_err();
        assert!(error.contains("credential 0"), "{error}");
    }

    #[test]
    fn verify_offline_still_accepts_a_bundle_and_knows_the_difference() {
        let (db, issuer_key, issuer, subject) = setup();
        issue_credential_impl(
            db.conn(),
            &issuer_key,
            &issuer,
            &sample_request(subject),
            NOW,
        )
        .unwrap();
        let bundle = export_bundle_impl(db.conn()).unwrap();

        let report = verify_offline_impl(&bundle, NOW).unwrap();
        assert_eq!(report.source, OfflineSource::Bundle);
        assert_eq!((report.accepted, report.total), (1, 1));
        // A bundle carries its own status lists, so revocation is conclusive.
        assert!(!report.revocation_unknown);
    }

    #[test]
    fn verify_offline_marks_status_bearing_credential_arrays_pending() {
        let (db, issuer_key, issuer, subject) = setup();
        let vc = issue_credential_impl(
            db.conn(),
            &issuer_key,
            &issuer,
            &sample_request(subject),
            NOW,
        )
        .unwrap();

        let json = serde_json::to_string(&vec![vc.clone(), vc]).unwrap();
        let report = verify_offline_impl(&json, NOW).unwrap();
        assert_eq!(report.source, OfflineSource::Credentials);
        assert_eq!((report.accepted, report.total), (0, 2));
        assert!(report
            .results
            .iter()
            .all(|result| result.acceptance_decision == AcceptanceDecision::Pending));
    }

    #[test]
    fn verify_offline_detects_a_tampered_credential() {
        // The point of the whole exercise: a modified claim must fail the
        // signature check rather than being waved through.
        let (db, issuer_key, issuer, subject) = setup();
        let vc = issue_credential_impl(
            db.conn(),
            &issuer_key,
            &issuer,
            &sample_request(subject),
            NOW,
        )
        .unwrap();

        let mut doc: serde_json::Value = serde_json::to_value(&vc).unwrap();
        doc["credentialSubject"]["level"] = serde_json::json!(99);
        let report = verify_offline_impl(&doc.to_string(), NOW).unwrap();

        assert_eq!(report.accepted, 0, "a tampered credential must not verify");
        assert!(!report.results[0].valid_signature);
    }

    #[test]
    fn verify_offline_explains_unrecognised_json() {
        // The old message leaked the bundle parser's complaint about
        // `format_version`, which made no sense to someone holding a
        // credential.
        let err = verify_offline_impl(r#"{"hello":"world"}"#, NOW).unwrap_err();
        assert!(err.contains("credential"), "got: {err}");
        assert!(err.contains("bundle"), "got: {err}");
        assert!(
            !err.contains("format_version"),
            "leaked parser detail: {err}"
        );
    }

    #[test]
    fn an_unresolvable_issuer_is_not_reported_as_a_bad_signature() {
        // Seed/demo credentials carry a placeholder issuer DID and a
        // placeholder JWS. verify_credential returns before checking the
        // signature, so saying "bad signature" would describe a check that
        // never ran and point the reader at the wrong field.
        let placeholder = r#"{
            "@context":["https://www.w3.org/ns/credentials/v2"],
            "id":"urn:uuid:demo",
            "type":["VerifiableCredential","AssessmentCredential"],
            "issuer":"did:key:z6MkSeedAuthor5CivicsInstructorXXXXXXXXXXXXXXX",
            "validFrom":"2026-04-08T11:15:00Z",
            "credentialSubject":{"id":"did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX"},
            "proof":{"type": "DataIntegrityProof", "cryptosuite": "eddsa-jcs-2022","created":"2026-04-08T11:15:00Z",
                     "verificationMethod":"did:key:z6MkSeedAuthor5CivicsInstructorXXXXXXXXXXXXXXX#key-1",
                     "proofPurpose":"assertionMethod","proofValue": "zseed..signature"}
        }"#;

        let report = verify_offline_impl(placeholder, NOW).unwrap();
        assert_eq!(
            report.accepted, 0,
            "a placeholder credential must not verify"
        );

        let reasons = rejection_reasons(&report.results[0]);
        assert_eq!(
            reasons.len(),
            1,
            "one failure happened, not two: {reasons:?}"
        );
        assert!(reasons[0].contains("issuer DID"), "got: {reasons:?}");
        assert!(
            !reasons
                .iter()
                .any(|r| r.contains("signature does not match")),
            "the signature was never checked: {reasons:?}"
        );
    }

    #[test]
    fn a_resolvable_issuer_with_a_broken_signature_says_so() {
        // The other side of the same coin: when the DID does resolve, a
        // signature failure is a real finding and must be named.
        let (db, issuer_key, issuer, subject) = setup();
        let vc = issue_credential_impl(
            db.conn(),
            &issuer_key,
            &issuer,
            &sample_request(subject),
            NOW,
        )
        .unwrap();
        let mut doc: serde_json::Value = serde_json::to_value(&vc).unwrap();
        doc["credentialSubject"]["level"] = serde_json::json!(99);

        let report = verify_offline_impl(&doc.to_string(), NOW).unwrap();
        let reasons = rejection_reasons(&report.results[0]);
        assert!(report.results[0].issuer_resolved);
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("signature does not match")),
            "got: {reasons:?}"
        );
    }

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
        assert!(issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).is_ok());

        req.integrity_policy = Some(IssuancePolicy {
            min_camera_coverage: Some(0.9),
            ..Default::default()
        });
        let err = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
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
        let err = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(err.contains("no snapshots"), "{err}");

        // Without the camera bound, the same session issues fine.
        req.integrity_policy = Some(IssuancePolicy::default());
        assert!(issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).is_ok());
    }

    #[test]
    fn integrity_assertion_embedded_when_session_bound() {
        let (db, key, issuer, subject) = setup();
        seed_session(db.conn(), "sess_ok", "completed", Some(0.91), 0, 1);
        let mut req = sample_request(subject);
        req.integrity_session_id = Some("sess_ok".into());
        let vc = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap();
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
            let error = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
            assert!(error.contains("does not meet required"), "{error}");
        }

        req.integrity_policy = None;
        let vc = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap();
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
        let err = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
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
        let vc = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap();
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
        let err = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(err.contains("requires integrity_session_id"), "got: {err}");
    }

    #[test]
    fn a_configured_host_makes_status_lists_url_addressed_and_pending() {
        use crate::settings::{registry::keys, SettingsStore};
        use alexandria_verify::vc::status;
        let (db, key, issuer, subject) = setup();
        SettingsStore::set(
            db.conn(),
            keys::CREDENTIAL_STATUS_HOST,
            "http://127.0.0.1:8080/".to_string(),
        )
        .unwrap();
        assert_eq!(
            status_list_host(db.conn()).as_deref(),
            Some("http://127.0.0.1:8080")
        );

        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        let reference = vc.credential_status.expect("status attached");
        let expected = status::list_url("http://127.0.0.1:8080", &issuer, 1);
        assert_eq!(reference.status_list_credential, expected);
        assert_eq!(reference.id, format!("{expected}#0"));

        // A new list is version 1 and nothing has served it: pending.
        let pending = pending_status_lists(db.conn(), &issuer).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].version, 1);
        let signed = signed_status_list(&pending[0], &key, &issuer, NOW).unwrap();
        let bits = status::verify_fetched_list(&signed, &expected, &issuer, "revocation").unwrap();
        assert_eq!(status::get_bit(&bits, 0), Some(false));

        mark_status_lists_published(db.conn(), &[(expected.clone(), 1)]).unwrap();
        assert!(pending_status_lists(db.conn(), &issuer).unwrap().is_empty());

        // Revocation bumps the version past what the host has.
        revoke_credential_impl(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();
        let pending = pending_status_lists(db.conn(), &issuer).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].version, 2);
        let signed = signed_status_list(&pending[0], &key, &issuer, NOW).unwrap();
        let bits = status::verify_fetched_list(&signed, &expected, &issuer, "revocation").unwrap();
        assert_eq!(status::get_bit(&bits, 0), Some(true));

        // An older acknowledgement never moves the mark backwards.
        mark_status_lists_published(db.conn(), &[(expected.clone(), 2)]).unwrap();
        mark_status_lists_published(db.conn(), &[(expected, 1)]).unwrap();
        assert!(pending_status_lists(db.conn(), &issuer).unwrap().is_empty());
    }

    #[test]
    fn only_https_or_loopback_hosts_count() {
        use crate::settings::{registry::keys, SettingsStore};
        let (db, _, _, _) = setup();
        for bad in ["http://cloud.example", "ftp://x", "cloud.example", "   "] {
            SettingsStore::set(db.conn(), keys::CREDENTIAL_STATUS_HOST, bad.to_string()).unwrap();
            assert!(status_list_host(db.conn()).is_none(), "{bad} accepted");
        }
        SettingsStore::set(
            db.conn(),
            keys::CREDENTIAL_STATUS_HOST,
            "https://cloud.example/".to_string(),
        )
        .unwrap();
        assert_eq!(
            status_list_host(db.conn()).as_deref(),
            Some("https://cloud.example")
        );
    }

    #[test]
    fn without_a_host_lists_are_urns_and_never_pending() {
        let (db, key, issuer, subject) = setup();
        assert!(status_list_host(db.conn()).is_none());
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        revoke_credential_impl(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();
        assert!(pending_status_lists(db.conn(), &issuer).unwrap().is_empty());
    }

    #[test]
    fn the_first_directory_stands_in_for_an_unset_host() {
        use crate::settings::{
            registry::{keys, JsonSetting},
            SettingsStore,
        };
        let (db, _, _, _) = setup();
        SettingsStore::set(
            db.conn(),
            keys::HOLDER_DIRECTORIES,
            JsonSetting(serde_json::json!([
                {"name": "Demo", "url": "http://localhost:8080"},
                {"name": "Other", "url": "https://other.example"}
            ])),
        )
        .unwrap();
        assert_eq!(
            status_list_host(db.conn()).as_deref(),
            Some("http://localhost:8080")
        );
    }

    /// A one-request HTTP host: records the PUT it receives and answers 200.
    async fn one_shot_host(
        expected_path: String,
    ) -> (String, tokio::sync::oneshot::Receiver<serde_json::Value>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw).to_string();
                if let Some(split) = text.find("\r\n\r\n") {
                    let head = &text[..split];
                    let length: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if raw.len() >= split + 4 + length {
                        let first = head.lines().next().unwrap().to_string();
                        assert_eq!(first, format!("PUT {expected_path} HTTP/1.1"));
                        let body: serde_json::Value =
                            serde_json::from_slice(&raw[split + 4..split + 4 + length]).unwrap();
                        tx.send(body).unwrap();
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\nconnection: close\r\n\r\n{\"ok\":true}")
                .await
                .unwrap();
        });
        (origin, rx)
    }

    #[tokio::test]
    async fn pending_lists_are_put_to_their_host_as_signed_credentials() {
        use alexandria_verify::vc::status;
        let key = test_key("issuer");
        let issuer = derive_did_key(&key);
        let (origin, received) =
            one_shot_host(format!("/status-lists/{}/1", issuer.as_str())).await;
        let list_id = status::list_url(&origin, &issuer, 1);
        let mut bits = vec![0u8; status::MIN_BITS / 8];
        status::set_bit(&mut bits, 5, true).unwrap();
        let pending = vec![PendingStatusList {
            list_id: list_id.clone(),
            status_purpose: "revocation".into(),
            bits,
            version: 3,
        }];
        let (accepted, errors) = push_status_lists(&pending, &key, &issuer, NOW).await;
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(accepted, vec![(list_id.clone(), 3)]);
        let document: VerifiableCredential =
            serde_json::from_value(received.await.unwrap()).unwrap();
        let bits = status::verify_fetched_list(&document, &list_id, &issuer, "revocation").unwrap();
        assert_eq!(status::get_bit(&bits, 5), Some(true));
    }

    #[tokio::test]
    async fn a_host_that_refuses_leaves_the_list_pending() {
        use alexandria_verify::vc::status;
        let key = test_key("issuer");
        let issuer = derive_did_key(&key);
        let other = derive_did_key(&test_key("other"));
        // Nothing listens here; and the second list is not ours to push.
        let pending = vec![
            PendingStatusList {
                list_id: status::list_url("http://127.0.0.1:9", &issuer, 1),
                status_purpose: "revocation".into(),
                bits: vec![0; 16],
                version: 1,
            },
            PendingStatusList {
                list_id: status::list_url("http://127.0.0.1:9", &other, 1),
                status_purpose: "revocation".into(),
                bits: vec![0; 16],
                version: 1,
            },
        ];
        let (accepted, errors) = push_status_lists(&pending, &key, &issuer, NOW).await;
        assert!(accepted.is_empty());
        assert_eq!(errors.len(), 2);
        assert!(errors[1].contains("another issuer"), "{errors:?}");
    }

    /// Writes `scripts/demo/fixtures/status-list-bundle.json`: a signed export
    /// whose list is URL-addressed, with one revoked and one active
    /// credential. The stdlib verifier's fetch test serves it over HTTP.
    ///
    ///   ALEXANDRIA_REGENERATE_FIXTURES=1 cargo test --lib status_list_fixture
    #[test]
    fn status_list_fixture_is_current() {
        use crate::settings::{registry::keys, SettingsStore};
        let (db, key, issuer, subject) = setup();
        SettingsStore::set(
            db.conn(),
            keys::CREDENTIAL_STATUS_HOST,
            "https://cloud.example".to_string(),
        )
        .unwrap();
        let first = issue_credential_impl(
            db.conn(),
            &key,
            &issuer,
            &sample_request(subject.clone()),
            NOW,
        )
        .unwrap();
        issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        revoke_credential_impl(
            db.conn(),
            &issuer,
            first.id.as_deref().unwrap(),
            "fixture",
            NOW,
        )
        .unwrap();
        let bundle = export_bundle_signed_impl(db.conn(), Some((&key, &issuer))).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&bundle).unwrap();
        value["origin"] = serde_json::Value::String("https://cloud.example".into());
        let pretty = serde_json::to_string_pretty(&value).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../scripts/demo/fixtures/status-list-bundle.json");
        if std::env::var("ALEXANDRIA_REGENERATE_FIXTURES").is_ok() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &pretty).unwrap();
        }
        // The list credential is signed as of export time, so the file is not
        // byte-stable; what must hold is that the fixture on disk is a bundle
        // this code still produces and still verifies: the same credentials,
        // referencing a URL-addressed list that is signed by their issuer and
        // marks exactly the first one revoked.
        let on_disk = std::fs::read_to_string(&path).expect("fixture exists; regenerate it");
        let on_disk: CredentialBundle = serde_json::from_str(&on_disk).unwrap();
        let fresh: CredentialBundle = serde_json::from_value(value).unwrap();
        assert_eq!(
            serde_json::to_value(&on_disk.credentials).unwrap(),
            serde_json::to_value(&fresh.credentials).unwrap(),
            "fixture is stale: ALEXANDRIA_REGENERATE_FIXTURES=1 cargo test --lib status_list_fixture"
        );
        let list = &on_disk.status_list_credentials[0];
        let list_id = list.id.clone().unwrap();
        assert_eq!(
            list_id,
            alexandria_verify::vc::status::list_url("https://cloud.example", &issuer, 1)
        );
        let bits = alexandria_verify::vc::status::verify_fetched_list(
            list,
            &list_id,
            &issuer,
            "revocation",
        )
        .unwrap();
        assert_eq!(alexandria_verify::vc::status::get_bit(&bits, 0), Some(true));
        assert_eq!(
            alexandria_verify::vc::status::get_bit(&bits, 1),
            Some(false)
        );
        for credential in &on_disk.credentials {
            assert_eq!(
                credential
                    .credential_status
                    .as_ref()
                    .map(|s| s.status_list_credential.as_str()),
                Some(list_id.as_str())
            );
        }
    }

    #[test]
    fn issue_credential_returns_signed_vc_with_status_slot() {
        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
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
        let a = issue_credential_impl(
            db.conn(),
            &key,
            &issuer,
            &sample_request(subject.clone()),
            NOW,
        )
        .unwrap();
        let b =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        assert_eq!(a.credential_status.unwrap().status_list_index, "0");
        assert_eq!(b.credential_status.unwrap().status_list_index, "1");
    }

    #[test]
    fn issue_rejects_non_did_subject() {
        let (db, key, issuer, _) = setup();
        let req = sample_request(Did("alice@example.com".into()));
        let err = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(err.contains("DID"), "got {err}");
    }

    #[test]
    fn revoke_sets_bit_and_marks_row_revoked() {
        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        revoke_credential_impl(
            db.conn(),
            &issuer,
            vc.id.as_deref().unwrap(),
            "superseded",
            NOW,
        )
        .unwrap();

        let revoked: i64 = db
            .conn()
            .query_row(
                "SELECT revoked FROM credentials WHERE id = ?1",
                params![vc.id.as_deref().unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(revoked, 1);

        // Bit 0 must be flipped in the status list.
        let bits: Vec<u8> = db
            .conn()
            .query_row(
                "SELECT bits FROM credential_status_lists WHERE issuer_did = ?1",
                params![issuer.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bits[0] & 0x80, 0x80, "index 0 is the most significant bit");
    }

    #[test]
    fn revoke_is_idempotent() {
        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        revoke_credential_impl(db.conn(), &issuer, vc.id.as_deref().unwrap(), "r1", NOW).unwrap();
        revoke_credential_impl(db.conn(), &issuer, vc.id.as_deref().unwrap(), "r2", NOW).unwrap();
        // One bit set; not doubled up.
        let bits: Vec<u8> = db
            .conn()
            .query_row(
                "SELECT bits FROM credential_status_lists WHERE issuer_did = ?1",
                params![issuer.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bits[0], 0x80, "index 0 is the most significant bit");
    }

    #[test]
    fn one_local_issuer_cannot_revoke_another_issuers_credential() {
        let (db, _, local_issuer, subject) = setup();
        let other_key = test_key("other-issuer");
        let other_issuer = derive_did_key(&other_key);
        let credential = issue_credential_impl(
            db.conn(),
            &other_key,
            &other_issuer,
            &sample_request(subject),
            NOW,
        )
        .unwrap();

        let error = revoke_credential_impl(
            db.conn(),
            &local_issuer,
            credential.id.as_deref().unwrap(),
            "not mine",
            NOW,
        )
        .unwrap_err();
        assert!(error.contains("only the credential issuer"));

        let revoked: i64 = db
            .conn()
            .query_row(
                "SELECT revoked FROM credentials WHERE id = ?1",
                params![credential.id.as_deref().unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revoked, 0);
    }

    #[test]
    fn one_local_issuer_cannot_suspend_or_reinstate_another_issuers_credential() {
        let (db, _, local_issuer, subject) = setup();
        let other_key = test_key("other-suspension-issuer");
        let other_issuer = derive_did_key(&other_key);
        let credential = issue_credential_impl(
            db.conn(),
            &other_key,
            &other_issuer,
            &sample_request(subject),
            NOW,
        )
        .unwrap();
        let credential_id = credential.id.as_deref().unwrap();

        let suspend_error = suspend_credential_impl(
            db.conn(),
            &local_issuer,
            credential_id,
            None,
            Some("not mine"),
            NOW,
        )
        .unwrap_err();
        assert!(suspend_error.contains("caller is not its issuer"));

        suspend_credential_impl(
            db.conn(),
            &other_issuer,
            credential_id,
            None,
            Some("issuer review"),
            NOW,
        )
        .unwrap();
        let reinstate_error =
            reinstate_credential_impl(db.conn(), &local_issuer, credential_id).unwrap_err();
        assert!(reinstate_error.contains("caller is not its issuer"));

        let suspended: i64 = db
            .conn()
            .query_row(
                "SELECT suspended FROM credentials WHERE id = ?1",
                params![credential_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(suspended, 1);
    }

    #[test]
    fn get_credential_returns_none_for_unknown_id() {
        let (db, _, _, _) = setup();
        let got = get_credential_impl(db.conn(), "urn:uuid:missing").unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn list_credentials_filters_by_subject_and_skill() {
        let (db, key, issuer, subject) = setup();
        issue_credential_impl(
            db.conn(),
            &key,
            &issuer,
            &sample_request(subject.clone()),
            NOW,
        )
        .unwrap();
        // Different skill
        let mut req2 = sample_request(subject.clone());
        if let Claim::Skill(ref mut s) = req2.claim {
            s.skill_id = "other_skill".into();
        }
        issue_credential_impl(db.conn(), &key, &issuer, &req2, NOW).unwrap();

        let all = list_credentials_impl(db.conn(), Some(subject.as_str()), None).unwrap();
        assert_eq!(all.len(), 2);
        let one =
            list_credentials_impl(db.conn(), Some(subject.as_str()), Some("other_skill")).unwrap();
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn a_signed_export_carries_the_status_list_as_a_bitstring_credential() {
        // The bundle a learner exports must verify with nothing from this
        // node: the list it names is inside it, as a credential signed by
        // the list's issuer, and a revocation set here reads as revoked
        // from that document alone.
        let (db, key, issuer, subject) = setup();
        let vc = issue_credential_impl(
            db.conn(),
            &key,
            &issuer,
            &sample_request(subject.clone()),
            NOW,
        )
        .unwrap();
        let json = export_bundle_signed_impl(db.conn(), Some((&key, &issuer))).unwrap();
        let bundle: CredentialBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(bundle.status_list_credentials.len(), 1);
        let list = &bundle.status_list_credentials[0];
        assert_eq!(
            list.id.as_deref(),
            vc.credential_status
                .as_ref()
                .map(|s| s.status_list_credential.as_str())
        );
        assert!(list
            .type_
            .iter()
            .any(|t| t == "BitstringStatusListCredential"));
        assert_eq!(list.proof.cryptosuite, "eddsa-jcs-2022");
        let bits = alexandria_verify::vc::status::verify_status_list_credential(
            list,
            &key.verifying_key(),
        )
        .unwrap();
        assert_eq!(bits.len() * 8, alexandria_verify::vc::status::MIN_BITS);
        let unsigned = export_bundle_impl(db.conn()).unwrap();
        assert!(
            !unsigned.contains("status_list_credentials"),
            "no signer, no list credentials"
        );

        revoke_credential_impl(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();
        let json = export_bundle_signed_impl(db.conn(), Some((&key, &issuer))).unwrap();
        let mut bundle: CredentialBundle = serde_json::from_str(&json).unwrap();
        // Drop the raw rows: the signed list credential alone must carry the revocation.
        bundle.status_lists.clear();
        let json = serde_json_canonicalizer::to_string(&bundle).unwrap();
        let (accepted, total) = verify_bundle_offline_impl(&json, NOW).unwrap();
        assert_eq!(
            accepted, 0,
            "a revoked credential must not verify from the signed list"
        );
        assert_eq!(total, 1);
    }

    #[test]
    fn revoked_credential_fails_verification() {
        // End-to-end within this test: issue → verify (accept) →
        // revoke → verify (reject) under default policy. This is what
        // PR 5.3 wires into verify_credential; locking it in here lets
        // verify.rs's test module stay focused on sign/verify only.
        use crate::domain::vc::verify_credential_db;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();

        let accepted = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert_eq!(accepted.acceptance_decision, AcceptanceDecision::Accept);
        assert!(!accepted.revoked);

        revoke_credential_impl(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();

        let rejected = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(rejected.revoked, "revocation bit must propagate to verify");
        assert_eq!(rejected.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    fn export_bundle_is_deterministic_for_same_inputs() {
        // §20.4: same credential set + same fixed clock + same key
        // ⇒ byte-identical bundle. This is what lets the bundle
        // round-trip through content-addressed archival.
        let (db, key, issuer, subject) = setup();
        let _ =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        let a = export_bundle_impl(db.conn()).unwrap();
        let b = export_bundle_impl(db.conn()).unwrap();
        assert_eq!(a, b, "bundle MUST be byte-identical");
    }

    #[test]
    fn export_bundle_includes_credentials_and_status_lists() {
        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        let json = export_bundle_impl(db.conn()).unwrap();
        let bundle: CredentialBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(bundle.format_version, BUNDLE_FORMAT_VERSION);
        assert_eq!(bundle.credentials.len(), 1);
        assert_eq!(bundle.credentials[0].id, vc.id);
        assert_eq!(bundle.status_lists.len(), 1);
        assert_eq!(bundle.status_lists[0].issuer_did, issuer.as_str());
    }

    #[test]
    fn offline_verifier_accepts_a_well_signed_bundle() {
        // §20: bundle survives Alexandria shutdown — verify uses an
        // ephemeral DB with no shared state.
        let (db, key, issuer, subject) = setup();
        let _ =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        let json = export_bundle_impl(db.conn()).unwrap();
        let (accepted, total) = verify_bundle_offline_impl(&json, NOW).unwrap();
        assert_eq!(total, 1);
        assert_eq!(accepted, 1, "round-tripped credential must verify");
    }

    #[test]
    fn offline_verifier_rejects_revoked_credential_in_bundle() {
        // The status list inside the bundle carries the revocation
        // bit, so the offline verifier sees the same Reject as the
        // local one.
        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        revoke_credential_impl(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();
        let json = export_bundle_impl(db.conn()).unwrap();
        let (accepted, total) = verify_bundle_offline_impl(&json, NOW).unwrap();
        assert_eq!(total, 1);
        assert_eq!(accepted, 0, "revoked VC must not be accepted offline");
    }

    #[test]
    fn offline_verifier_rejects_unsupported_format_version() {
        let bundle = serde_json::json!({
            "format_version": "alexandria-credential-bundle/0.0",
            "credentials": [],
            "key_registry": [],
            "status_lists": []
        });
        assert!(
            verify_bundle_offline_impl(&bundle.to_string(), NOW).is_err(),
            "must reject unknown format_version"
        );
    }

    // ---- §11.3 suspension --------------------------------------------------

    #[test]
    fn suspension_round_trip_flips_verify_decision() {
        use crate::domain::vc::verify_credential_db;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();

        // Pre-suspension: accepted.
        let pre = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert_eq!(pre.acceptance_decision, AcceptanceDecision::Accept);
        assert!(!pre.suspended);

        // Suspend with no upper bound — indefinite suspension.
        suspend_credential_impl(
            db.conn(),
            &issuer,
            vc.id.as_deref().unwrap(),
            None,
            Some("under review"),
            NOW,
        )
        .unwrap();
        let mid = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(mid.suspended);
        assert_eq!(mid.acceptance_decision, AcceptanceDecision::Reject);

        // Reinstate.
        reinstate_credential_impl(db.conn(), &issuer, vc.id.as_deref().unwrap()).unwrap();
        let after = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(!after.suspended);
        assert_eq!(after.acceptance_decision, AcceptanceDecision::Accept);
    }

    #[test]
    fn suspension_with_until_in_past_is_no_longer_active() {
        use crate::domain::vc::verify_credential_db;
        use crate::domain::vc::VerificationPolicy;

        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();

        // Suspended until a time before NOW — verifier sees the
        // suspension as auto-expired and treats the credential as
        // active again.
        suspend_credential_impl(
            db.conn(),
            &issuer,
            vc.id.as_deref().unwrap(),
            Some("2026-01-01T00:00:00Z"),
            None,
            NOW,
        )
        .unwrap();
        let result = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(!result.suspended);
    }

    #[test]
    fn permissive_policy_can_accept_suspended() {
        use crate::domain::vc::verify_credential_db;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        suspend_credential_impl(
            db.conn(),
            &issuer,
            vc.id.as_deref().unwrap(),
            None,
            None,
            NOW,
        )
        .unwrap();

        let permissive = VerificationPolicy {
            reject_suspended: false,
            ..Default::default()
        };
        let result = verify_credential_db(db.conn(), &vc, NOW, &permissive);
        assert!(result.suspended);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
    }

    // ---- §11.4 supersession ------------------------------------------------

    #[test]
    fn supersession_marks_old_credential_superseded() {
        use crate::domain::vc::verify_credential_db;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, issuer, subject) = setup();
        let old = issue_credential_impl(
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
        let _new = issue_credential_impl(db.conn(), &key, &issuer, &new_req, NOW).unwrap();

        let result = verify_credential_db(db.conn(), &old, NOW, &VerificationPolicy::default());
        assert!(result.superseded);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    fn supersession_rejects_cross_issuer() {
        // §11.4: same subject, claim kind, and issuer required.
        let (db, key, issuer, subject) = setup();
        let old = issue_credential_impl(
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
        let err =
            issue_credential_impl(db.conn(), &other_key, &other_issuer, &bad_req, NOW).unwrap_err();
        assert!(err.contains("issuer"), "got {err}");
    }

    #[test]
    fn supersession_rejects_unknown_prior_id() {
        let (db, key, issuer, subject) = setup();
        let mut req = sample_request(subject);
        req.supersedes = Some("urn:uuid:does-not-exist".into());
        let err = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(err.contains("not found"), "got {err}");
    }
}
