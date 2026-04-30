//! Auto-issuance pipeline: turn observed completion mints into VCs.
//!
//! Consumes [`CompletionObservation`] rows from `completion_observations`
//! and, for each pending row, signs a self-asserted Verifiable
//! Credential with the learner's own `did:key` as issuer. The signed
//! envelope embeds the on-chain `witness` block (tx hash + validator
//! script hash + validator name) so external verifiers can resolve
//! the tx on Cardano and confirm the witness was satisfied.
//!
//! ## Why self-asserted
//!
//! The completion validator enforces the authorization rules on-chain.
//! Whoever signed the tx that satisfied the validator is the learner;
//! off-chain signing by that same learner just binds the full VC
//! envelope (including claim details + witness block) under JWS so it
//! can travel between peers without being tampered with. The
//! verifier's trust chain runs: witness tx hash → on-chain script →
//! datum fields → VC envelope → JWS signature by the same key that
//! signed the tx. All of that is cryptographically linked.

use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::cardano::completion::{self, CompletionObservation};
use crate::crypto::did::{did_from_verifying_key, Did, VerificationMethodRef};
use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
use crate::domain::vc::{
    Claim, CredentialStatus, CustomClaim, Proof, VerifiableCredential, Witness,
};

const STATUS_LIST_BITS: usize = 16_384;
const STATUS_LIST_TYPE: &str = "RevocationList2020Status";
const W3C_VC_V1: &str = "https://www.w3.org/2018/credentials/v1";
const ALEXANDRIA_V1: &str = "https://alexandria.protocol/context/v1";

/// Outcome of a single auto-issuance pipeline run.
#[derive(Debug, Default)]
pub struct AutoIssuanceReport {
    /// Number of credentials successfully issued this tick.
    pub issued: usize,
    /// Number of pending observations skipped because they did not
    /// belong to the local learner (subject pubkey mismatch).
    pub skipped_foreign: usize,
    /// Number of pending observations skipped because the course's
    /// attestation requirement is not yet met.
    pub waiting_on_attestations: usize,
    /// Per-observation errors, recorded so the caller can surface them
    /// to the UI without stopping the loop.
    pub errors: Vec<String>,
}

/// Process every pending [`CompletionObservation`] by self-signing a
/// VC for it. Observations whose `subject_pubkey` doesn't match the
/// local learner's verifying key are skipped — we only sign our own.
pub fn tick(conn: &Connection, learner_key: &SigningKey) -> Result<AutoIssuanceReport, String> {
    let local_vk_hex = hex::encode(learner_key.verifying_key().as_bytes());
    let local_did = did_from_verifying_key(&learner_key.verifying_key());

    let pending = completion::pending_observations(conn).map_err(|e| e.to_string())?;

    let mut report = AutoIssuanceReport::default();
    for obs in pending {
        if obs.subject_pubkey != local_vk_hex {
            report.skipped_foreign += 1;
            continue;
        }

        // Attestation gate: if the course has an attestation
        // requirement configured, the witness tx must have gathered
        // enough validated signatures before we issue.
        //
        // `course_id` in the observation is a hex-encoded blob
        // (derived from the on-chain datum); we don't currently have
        // an authoritative mapping from that blob back to the local
        // course table, so the gate is checked against the raw hex
        // id. Callers who set a requirement keyed on that same hex
        // get the gate; everyone else proceeds immediately.
        match super::attestation::are_attestations_satisfied(
            conn,
            &obs.tx_hash,
            Some(&obs.course_id),
        ) {
            Ok(true) => {}
            Ok(false) => {
                report.waiting_on_attestations += 1;
                continue;
            }
            Err(e) => {
                report
                    .errors
                    .push(format!("attestation gate({}): {e}", obs.tx_hash));
                continue;
            }
        }

        match issue_for_observation(conn, learner_key, &local_did, &obs) {
            Ok(credential_id) => {
                if let Err(e) = completion::mark_issued(
                    conn,
                    &obs.policy_id,
                    &obs.asset_name_hex,
                    &credential_id,
                ) {
                    report
                        .errors
                        .push(format!("mark_issued({credential_id}): {e}"));
                } else {
                    report.issued += 1;
                }
            }
            Err(e) => {
                report.errors.push(format!("issue({}): {e}", obs.tx_hash));
            }
        }
    }

    Ok(report)
}

/// Issue a single self-asserted VC for the given observation. Returns
/// the new credential id. Pure function — no network, no keystore.
pub fn issue_for_observation(
    conn: &Connection,
    learner_key: &SigningKey,
    learner_did: &Did,
    obs: &CompletionObservation,
) -> Result<String, String> {
    // Idempotency: if this observation already has a credential id
    // recorded, return it — no-op. Prevents duplicate inserts if the
    // caller retries after a partial failure.
    if let Some(existing) = obs.credential_id.clone() {
        return Ok(existing);
    }

    let list_id = ensure_status_list(conn, learner_did)?;
    let index = allocate_status_index(conn, &list_id)?;

    let credential_id = format!("urn:uuid:{}", Uuid::new_v4());
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // Self-asserted course-completion claim. The properties land
    // directly on `credentialSubject` per W3C VC v2 — a `claimType`
    // discriminator is added so verifiers can route on it without
    // pattern-matching on individual property names.
    let claim = Claim::Custom(CustomClaim {
        properties: serde_json::Map::from_iter([
            ("claimType".into(), serde_json::json!("course_completion")),
            ("courseId".into(), serde_json::json!(obs.course_id)),
            (
                "completionRoot".into(),
                serde_json::json!(obs.completion_root),
            ),
            (
                "completionTime".into(),
                serde_json::json!(obs.completion_time),
            ),
        ]),
    });

    let witness = Witness {
        tx_hash: obs.tx_hash.clone(),
        validator_script_hash: obs.policy_id.clone(),
        validator_name: completion::WITNESS_VALIDATOR_NAME.to_string(),
    };

    let vc = VerifiableCredential {
        context: vec![W3C_VC_V1.into(), ALEXANDRIA_V1.into()],
        id: Some(credential_id.clone()),
        type_: vec!["VerifiableCredential".into(), "SelfAssertion".into()],
        issuer: learner_did.clone(),
        valid_from: now.clone(),
        valid_until: None,
        credential_subject: claim.into_subject(learner_did.clone()),
        credential_status: Some(CredentialStatus {
            id: format!("{list_id}#{index}"),
            type_: STATUS_LIST_TYPE.into(),
            status_purpose: "revocation".into(),
            status_list_index: index.to_string(),
            status_list_credential: list_id.clone(),
        }),
        terms_of_use: None,
        witness: Some(witness),
        proof: Proof {
            type_: "Ed25519Signature2020".into(),
            created: now.clone(),
            verification_method: VerificationMethodRef(format!("{}#key-1", learner_did.as_str())),
            proof_purpose: "assertionMethod".into(),
            jws: String::new(),
        },
    };
    let signed = sign_credential(
        UnsignedCredential { credential: vc },
        learner_key,
        learner_did,
    )
    .map_err(|e| format!("sign: {e}"))?;

    let signed_json = serde_json::to_string(&signed).map_err(|e| e.to_string())?;
    let integrity_hash = integrity_hash_of(&signed)?;

    conn.execute(
        "INSERT INTO credentials \
         (id, issuer_did, subject_did, credential_type, claim_kind, skill_id, \
          issuance_date, expiration_date, signed_vc_json, integrity_hash, \
          status_list_id, status_list_index, \
          witness_tx_hash, witness_validator_script_hash, \
          witness_validator_name, auto_issued) \
         VALUES (?1, ?2, ?3, 'SelfAssertion', 'custom', NULL, \
                 ?4, NULL, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1)",
        params![
            credential_id,
            learner_did.as_str(),
            learner_did.as_str(),
            now,
            signed_json,
            integrity_hash,
            list_id,
            index,
            obs.tx_hash,
            obs.policy_id,
            completion::WITNESS_VALIDATOR_NAME,
        ],
    )
    .map_err(|e| format!("insert credential: {e}"))?;

    // Queue for optional §12.3 integrity anchoring. Soft-fail: the
    // witness is already on-chain, so a missing anchor is tolerable.
    if let Err(e) = crate::cardano::anchor_queue::enqueue(conn, &credential_id) {
        log::debug!("auto-issuance: anchor enqueue failed for {credential_id}: {e}");
    }

    // Reputation feedback. Auto-earned VCs are self-asserted so
    // `on_credential_accepted` credits the learner only.
    if let Err(e) = crate::evidence::reputation::on_credential_accepted(conn, &credential_id) {
        log::debug!("auto-issuance: reputation update failed for {credential_id}: {e}");
    }

    Ok(credential_id)
}

// ----- helpers (mirror commands::credentials internals) -----

fn ensure_status_list(conn: &Connection, issuer_did: &Did) -> Result<String, String> {
    let list_id = format!("urn:alexandria:status-list:{}:1", issuer_did.as_str());
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

fn allocate_status_index(conn: &Connection, list_id: &str) -> Result<i64, String> {
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

fn integrity_hash_of(vc: &VerifiableCredential) -> Result<String, String> {
    let mut clone = vc.clone();
    clone.proof.jws.clear();
    let value = serde_json::to_value(&clone).map_err(|e| e.to_string())?;
    let bytes = serde_json_canonicalizer::to_vec(&value).map_err(|e| e.to_string())?;
    Ok(hex::encode(blake3::hash(&bytes).as_bytes()))
}

/// Find the credential id that was issued for a given observation
/// tx hash, if any. Handy for UI cross-references.
pub fn credential_for_witness_tx(
    conn: &Connection,
    tx_hash: &str,
) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT id FROM credentials WHERE witness_tx_hash = ?1 LIMIT 1",
        params![tx_hash],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        db
    }

    fn seed_observation(db: &Database, obs: &CompletionObservation) {
        db.conn()
            .execute(
                "INSERT INTO completion_observations ( \
                    policy_id, asset_name_hex, tx_hash, subject_pubkey, \
                    course_id, completion_root, completion_time, \
                    credential_id, observed_at, issued_at \
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, NULL)",
                params![
                    obs.policy_id,
                    obs.asset_name_hex,
                    obs.tx_hash,
                    obs.subject_pubkey,
                    obs.course_id,
                    obs.completion_root,
                    obs.completion_time,
                    obs.observed_at,
                ],
            )
            .unwrap();
    }

    #[test]
    fn tick_issues_vc_for_local_learner() {
        let db = test_db();
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let local_vk_hex = hex::encode(key.verifying_key().as_bytes());

        let obs = CompletionObservation {
            policy_id: "6380450179a6933acdf76213732f8626e1486b9ed5cc7fe7f46c98e0".into(),
            asset_name_hex: "aa".repeat(16),
            tx_hash: "cd".repeat(32),
            subject_pubkey: local_vk_hex.clone(),
            course_id: "22".repeat(16),
            completion_root: "33".repeat(32),
            completion_time: "2026-04-24T12:00:00Z".into(),
            credential_id: None,
            observed_at: "2026-04-24 12:00:00".into(),
            issued_at: None,
        };
        seed_observation(&db, &obs);

        let report = tick(db.conn(), &key).unwrap();
        assert_eq!(report.issued, 1, "expected one issuance");
        assert_eq!(report.skipped_foreign, 0);
        assert!(
            report.errors.is_empty(),
            "unexpected errors: {:?}",
            report.errors
        );

        // The credential landed with witness metadata populated.
        let (witness_tx, auto_issued, validator_name): (String, i64, String) = db
            .conn()
            .query_row(
                "SELECT witness_tx_hash, auto_issued, witness_validator_name \
                 FROM credentials WHERE witness_tx_hash IS NOT NULL",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(witness_tx, obs.tx_hash);
        assert_eq!(auto_issued, 1);
        assert_eq!(validator_name, completion::WITNESS_VALIDATOR_NAME);

        // The observation has been marked issued and is no longer pending.
        let pending = completion::pending_observations(db.conn()).unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn tick_skips_foreign_observations() {
        let db = test_db();
        let key = SigningKey::from_bytes(&[11u8; 32]);
        let foreign_vk_hex = "ff".repeat(32); // different pubkey

        let obs = CompletionObservation {
            policy_id: "aa".repeat(28),
            asset_name_hex: "bb".repeat(16),
            tx_hash: "cc".repeat(32),
            subject_pubkey: foreign_vk_hex,
            course_id: "dd".repeat(16),
            completion_root: "ee".repeat(32),
            completion_time: "2026-04-24T12:00:00Z".into(),
            credential_id: None,
            observed_at: "2026-04-24 12:00:00".into(),
            issued_at: None,
        };
        seed_observation(&db, &obs);

        let report = tick(db.conn(), &key).unwrap();
        assert_eq!(report.issued, 0);
        assert_eq!(report.skipped_foreign, 1);

        let creds: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert_eq!(creds, 0, "no VC for foreign observation");
    }

    #[test]
    fn tick_holds_back_when_attestation_requirement_unmet() {
        use super::super::attestation::{set_requirement_impl, submit_attestation_impl};
        use crate::domain::attestation::{
            SetCompletionRequirementParams, SubmitCompletionAttestationParams,
        };

        let db = test_db();
        let key = SigningKey::from_bytes(&[21u8; 32]);
        let local_vk_hex = hex::encode(key.verifying_key().as_bytes());
        let tx_hash_hex: String = (0..32).map(|i| format!("{:02x}", i + 1)).collect();
        let course_hex = "22".repeat(16);

        // Requirement: 1 attestor on this course.
        set_requirement_impl(
            db.conn(),
            &SetCompletionRequirementParams {
                course_id: course_hex.clone(),
                required_attestors: 1,
                dao_id: "dao_demo".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();

        let obs = CompletionObservation {
            policy_id: "77".repeat(28),
            asset_name_hex: "88".repeat(16),
            tx_hash: tx_hash_hex.clone(),
            subject_pubkey: local_vk_hex,
            course_id: course_hex.clone(),
            completion_root: "99".repeat(32),
            completion_time: "2026-04-24T12:00:00Z".into(),
            credential_id: None,
            observed_at: "2026-04-24 12:00:00".into(),
            issued_at: None,
        };
        seed_observation(&db, &obs);

        // First tick: no attestor → hold.
        let held = tick(db.conn(), &key).unwrap();
        assert_eq!(held.issued, 0);
        assert_eq!(held.waiting_on_attestations, 1);

        let count_before: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count_before, 0);

        // An assessor attests on the witness tx.
        let assessor = SigningKey::from_bytes(&[22u8; 32]);
        submit_attestation_impl(
            db.conn(),
            &assessor,
            &SubmitCompletionAttestationParams {
                witness_tx_hash: tx_hash_hex.clone(),
                note: None,
            },
        )
        .unwrap();

        // Second tick: requirement met → issue.
        let issued = tick(db.conn(), &key).unwrap();
        assert_eq!(issued.issued, 1);
        assert_eq!(issued.waiting_on_attestations, 0);
    }

    #[test]
    fn issue_is_idempotent_when_already_resolved() {
        let db = test_db();
        let key = SigningKey::from_bytes(&[3u8; 32]);
        let did = did_from_verifying_key(&key.verifying_key());

        let obs = CompletionObservation {
            policy_id: "01".repeat(28),
            asset_name_hex: "02".repeat(16),
            tx_hash: "03".repeat(32),
            subject_pubkey: hex::encode(key.verifying_key().as_bytes()),
            course_id: "04".repeat(16),
            completion_root: "05".repeat(32),
            completion_time: "2026-04-24T12:00:00Z".into(),
            credential_id: Some("urn:uuid:already-issued".into()),
            observed_at: "2026-04-24 12:00:00".into(),
            issued_at: Some("2026-04-24 12:01:00".into()),
        };

        let cid = issue_for_observation(db.conn(), &key, &did, &obs).unwrap();
        assert_eq!(cid, "urn:uuid:already-issued");

        let row_count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert_eq!(row_count, 0, "must not insert when already resolved");
    }
}
