// SPDX-License-Identifier: MIT
//! Read the device's current enterprise entitlement.
//!
//! This command is MIT and registered unconditionally. In a community build
//! it exists and answers honestly: there is no trusted issuer compiled in, so
//! it returns the empty snapshot. That is the steady state, not a failure —
//! the community build is the whole product minus the commercial layer, not a
//! degraded enterprise build.
//!
//! It lives here rather than in `src/ee` so a user can read exactly what gates
//! the software they are running. Only the resolver that consumes the snapshot
//! is enterprise-licensed. See `docs/enterprise-boundary.md`.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::domain::vc::entitlement::{parse_entitlement_with, TRUSTED_ENTITLEMENT_ISSUERS};
use crate::domain::vc::verify::verify_credential;
use crate::domain::vc::{
    AcceptanceDecision, CredentialType, VerifiableCredential, VerificationPolicy,
};
use crate::AppState;

/// What the frontend's `EntitlementSnapshot` needs from the backend.
///
/// `enterpriseBuild` is deliberately absent: whether this is an enterprise
/// build is a frontend build-time fact (`@ee` resolving to `src/ee` rather
/// than `src/ee-stub`), not something the database can answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementSnapshot {
    /// Feature keys granted by every currently-valid entitlement, unioned.
    pub features: Vec<String>,
    /// Organisation named by the newest surviving entitlement.
    pub org_id: Option<String>,
    /// Plan named by the newest surviving entitlement.
    pub plan: Option<String>,
}

impl EntitlementSnapshot {
    /// Grants nothing. This is what the resolver returns whenever no
    /// entitlement survives, including the ordinary community case where none
    /// was ever installed — a test-only constructor because production builds
    /// it field-by-field from the loop's accumulators.
    #[cfg(test)]
    fn empty() -> Self {
        Self {
            features: Vec::new(),
            org_id: None,
            plan: None,
        }
    }
}

/// The policy an entitlement must satisfy.
///
/// Built explicitly rather than from [`VerificationPolicy::default`], which
/// omits `EntitlementCredential` on purpose — that default governs *capability*
/// credentials, and letting a commercial artifact through it would be wrong.
/// Here the inverse holds: only entitlements are acceptable.
fn entitlement_policy() -> VerificationPolicy {
    VerificationPolicy {
        reject_expired: true,
        require_integrity_anchor: false,
        allowed_types: vec![CredentialType::EntitlementCredential],
        reject_suspended: true,
        reject_superseded: true,
    }
}

/// Every stored credential typed as an entitlement, newest first.
///
/// Ordering matters: `org_id` and `plan` are single-valued in the snapshot,
/// so "newest wins" needs a defined newest.
fn stored_entitlements(conn: &rusqlite::Connection) -> Result<Vec<VerifiableCredential>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT signed_vc_json FROM credentials \
             WHERE credential_type = ?1 ORDER BY received_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params![CredentialType::EntitlementCredential.as_str()],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        let json = row.map_err(|e| e.to_string())?;
        // A row that will not deserialize is skipped rather than fatal: one
        // corrupt entitlement must not make the others unreadable.
        if let Ok(vc) = serde_json::from_str::<VerifiableCredential>(&json) {
            out.push(vc);
        }
    }
    Ok(out)
}

/// Resolve the current entitlement snapshot.
///
/// A credential counts only if it clears **both** gates:
///
/// 1. [`verify_credential`] accepts it — signature, issuer resolution, subject
///    binding, expiry, revocation, suspension, supersession.
/// 2. [`parse_entitlement`] accepts it — the issuer is one this build was
///    compiled to trust.
///
/// The second gate is not redundant. Verification proves a credential was
/// signed by whoever its issuer DID *names*, and a `did:key` is
/// self-describing, so anyone can mint one and self-issue a credential that
/// verifies perfectly. Without the issuer pin the first gate alone would grant
/// every feature to anybody who asked.
pub fn get_entitlement_snapshot_impl(
    conn: &rusqlite::Connection,
    now: &str,
) -> Result<EntitlementSnapshot, String> {
    snapshot_with_trusted(conn, now, TRUSTED_ENTITLEMENT_ISSUERS)
}

/// [`get_entitlement_snapshot_impl`] against an explicit issuer allowlist.
///
/// The shipped allowlist is empty, which makes every accept path below
/// unreachable in production today. Taking the list as a parameter keeps the
/// union and newest-wins logic testable now rather than the first time a real
/// issuer DID is compiled in.
fn snapshot_with_trusted(
    conn: &rusqlite::Connection,
    now: &str,
    trusted: &[&str],
) -> Result<EntitlementSnapshot, String> {
    let policy = entitlement_policy();
    let mut features: Vec<String> = Vec::new();
    let mut org_id = None;
    let mut plan = None;

    for vc in stored_entitlements(conn)? {
        let result = verify_credential(conn, &vc, now, &policy);
        if result.acceptance_decision != AcceptanceDecision::Accept {
            continue;
        }
        let Ok(claim) = parse_entitlement_with(&vc, trusted) else {
            continue;
        };

        // Newest-first iteration means the first survivor is the newest.
        if org_id.is_none() {
            org_id = Some(claim.org_id.clone());
            plan = Some(claim.entitlement_plan.clone());
        }

        // Union, preserving first-seen order and dropping duplicates. Feature
        // keys are not validated against a known set here — an unrecognised
        // key from a newer plan must survive the trip and be filtered at the
        // consuming edge, not silently dropped mid-pipeline.
        for f in claim.features {
            if !features.contains(&f) {
                features.push(f);
            }
        }
    }

    Ok(EntitlementSnapshot {
        features,
        org_id,
        plan,
    })
}

#[tauri::command]
pub async fn get_entitlement_snapshot(
    state: State<'_, AppState>,
) -> Result<EntitlementSnapshot, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let now = super::credentials::now_rfc3339();
    get_entitlement_snapshot_impl(db.conn(), &now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::derive_did_key;
    use crate::crypto::did::{Did, VerificationMethodRef};
    use crate::db::Database;
    use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
    use crate::domain::vc::{EntitlementClaim, Proof};
    use ed25519_dalek::SigningKey;

    const NOW: &str = "2026-04-13T00:00:00Z";

    fn key(seed: &str) -> SigningKey {
        let mut bytes = [0u8; 32];
        let s = seed.as_bytes();
        bytes[..s.len().min(32)].copy_from_slice(&s[..s.len().min(32)]);
        SigningKey::from_bytes(&bytes)
    }

    /// Sign a well-formed entitlement and store it exactly the way the
    /// issuance path would, so the query in `stored_entitlements` is exercised
    /// against a real row rather than a hand-built one.
    fn store_entitlement(
        db: &Database,
        issuer_key: &SigningKey,
        features: &[&str],
        valid_until: Option<&str>,
    ) -> VerifiableCredential {
        let issuer = derive_did_key(issuer_key);
        let subject = Did("did:key:z6MkOrgSubject".into());
        let claim = EntitlementClaim {
            org_id: "org-1".into(),
            entitlement_plan: "enterprise".into(),
            seats: 10,
            features: features.iter().map(|f| f.to_string()).collect(),
        };
        let vc = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some(format!("urn:test:entitlement:{}", features.join("-"))),
            type_: vec![
                "VerifiableCredential".into(),
                CredentialType::EntitlementCredential.as_str().to_string(),
            ],
            issuer: issuer.clone(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_until: valid_until.map(str::to_string),
            credential_subject: crate::domain::vc::CredentialSubject {
                id: subject,
                properties: claim.into_properties(),
            },
            credential_status: None,
            terms_of_use: None,
            witness: None,
            integrity: None,
            proof: Proof {
                type_: "Ed25519Signature2020".into(),
                created: "2026-01-01T00:00:00Z".into(),
                verification_method: VerificationMethodRef(format!("{}#key-1", issuer.as_str())),
                proof_purpose: "assertionMethod".into(),
                jws: String::new(),
            },
        };
        let signed = sign_credential(
            UnsignedCredential {
                credential: vc.clone(),
            },
            issuer_key,
            &issuer,
        )
        .unwrap();

        db.conn()
            .execute(
                "INSERT INTO credentials \
                 (id, issuer_did, subject_did, credential_type, claim_kind, \
                  issuance_date, integrity_hash, signed_vc_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    signed.id.clone().unwrap(),
                    signed.issuer.as_str(),
                    signed.credential_subject.id.as_str(),
                    CredentialType::EntitlementCredential.as_str(),
                    "entitlement",
                    signed.valid_from,
                    "test-integrity-hash",
                    serde_json::to_string(&signed).unwrap(),
                ],
            )
            .unwrap();
        signed
    }

    fn open_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    /// The shipped configuration. No trusted issuer is compiled in, so a
    /// perfectly valid, correctly signed, unexpired entitlement still grants
    /// nothing — this is the test that would fail first if the issuer pin were
    /// ever dropped.
    #[test]
    fn a_valid_but_self_issued_entitlement_grants_nothing() {
        let db = open_db();
        let signed = store_entitlement(&db, &key("attacker"), &["talent_index"], None);

        // The credential really is valid — the refusal is about trust, not
        // about the credential being broken.
        let verified = verify_credential(db.conn(), &signed, NOW, &entitlement_policy());
        assert_eq!(verified.acceptance_decision, AcceptanceDecision::Accept);

        let snapshot = get_entitlement_snapshot_impl(db.conn(), NOW).unwrap();
        assert_eq!(snapshot, EntitlementSnapshot::empty());
    }

    /// An empty store is not an error condition.
    #[test]
    fn no_entitlements_yields_the_empty_snapshot() {
        let db = open_db();
        let snapshot = get_entitlement_snapshot_impl(db.conn(), NOW).unwrap();
        assert_eq!(snapshot, EntitlementSnapshot::empty());
    }

    /// A row that will not deserialize must not take the whole query down.
    #[test]
    fn a_corrupt_row_is_skipped_not_fatal() {
        let db = open_db();
        db.conn()
            .execute(
                "INSERT INTO credentials \
                 (id, issuer_did, subject_did, credential_type, claim_kind, \
                  issuance_date, integrity_hash, signed_vc_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "urn:test:corrupt",
                    "did:key:z6MkWhoever",
                    "did:key:z6MkSubject",
                    CredentialType::EntitlementCredential.as_str(),
                    "entitlement",
                    "2026-01-01T00:00:00Z",
                    "test-integrity-hash",
                    "{not valid json",
                ],
            )
            .unwrap();
        let snapshot = get_entitlement_snapshot_impl(db.conn(), NOW).unwrap();
        assert_eq!(snapshot, EntitlementSnapshot::empty());
    }

    /// Non-entitlement credentials must never reach the entitlement path,
    /// however they were typed in the database.
    #[test]
    fn credentials_of_other_types_are_not_considered() {
        let db = open_db();
        db.conn()
            .execute(
                "INSERT INTO credentials \
                 (id, issuer_did, subject_did, credential_type, claim_kind, \
                  issuance_date, integrity_hash, signed_vc_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "urn:test:formal",
                    "did:key:z6MkWhoever",
                    "did:key:z6MkSubject",
                    CredentialType::FormalCredential.as_str(),
                    "skill",
                    "2026-01-01T00:00:00Z",
                    "test-integrity-hash",
                    "{}",
                ],
            )
            .unwrap();
        assert!(stored_entitlements(db.conn()).unwrap().is_empty());
    }

    /// Guards the intent behind `entitlement_policy`: it must be the mirror
    /// image of the default, not a copy of it.
    #[test]
    fn entitlement_policy_accepts_only_entitlements() {
        assert_eq!(
            entitlement_policy().allowed_types,
            vec![CredentialType::EntitlementCredential]
        );
        assert!(!VerificationPolicy::default()
            .allowed_types
            .contains(&CredentialType::EntitlementCredential));
    }

    /// The accept path, reachable only via an injected allowlist because the
    /// shipped one is empty. Without this the union logic below would ship
    /// having never executed.
    #[test]
    fn a_trusted_entitlement_grants_its_features() {
        let db = open_db();
        let k = key("ifftu");
        let signed = store_entitlement(&db, &k, &["talent_index"], None);
        let trusted = [signed.issuer.as_str()];

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(snapshot.features, vec!["talent_index".to_string()]);
        assert_eq!(snapshot.org_id.as_deref(), Some("org-1"));
        assert_eq!(snapshot.plan.as_deref(), Some("enterprise"));
    }

    /// Q1, decided: features from several valid entitlements are unioned, not
    /// overwritten by whichever happens to be newest.
    #[test]
    fn features_from_several_entitlements_are_unioned() {
        let db = open_db();
        let k = key("ifftu");
        let a = store_entitlement(&db, &k, &["talent_index"], None);
        store_entitlement(&db, &k, &["employer_console", "talent_index"], None);
        let trusted = [a.issuer.as_str()];

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        let mut got = snapshot.features.clone();
        got.sort();
        assert_eq!(
            got,
            vec!["employer_console".to_string(), "talent_index".to_string()],
            "union, and no duplicate for the key both credentials carry"
        );
    }

    /// An expired entitlement contributes nothing, without any status list
    /// being involved — the check is offline.
    #[test]
    fn an_expired_entitlement_is_ignored_even_when_trusted() {
        let db = open_db();
        let k = key("ifftu");
        let signed = store_entitlement(&db, &k, &["talent_index"], Some("2026-02-01T00:00:00Z"));
        let trusted = [signed.issuer.as_str()];

        // Valid before the term ends...
        let before = snapshot_with_trusted(db.conn(), "2026-01-15T00:00:00Z", &trusted).unwrap();
        assert_eq!(before.features, vec!["talent_index".to_string()]);

        // ...and grants nothing after.
        let after = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(after, EntitlementSnapshot::empty());
    }

    /// An entitlement from a trusted issuer whose signature does not check out
    /// must be refused. Guards against the issuer pin being mistaken for a
    /// substitute for verification rather than an addition to it.
    #[test]
    fn a_tampered_entitlement_from_a_trusted_issuer_is_refused() {
        let db = open_db();
        let k = key("ifftu");
        let signed = store_entitlement(&db, &k, &["talent_index"], None);
        let trusted = [signed.issuer.as_str()];

        // Rewrite the stored VC to claim an extra feature, leaving the
        // signature untouched.
        let mut tampered = signed.clone();
        let mut claim = EntitlementClaim::extract(&tampered.credential_subject).unwrap();
        claim.features.push("employer_console".into());
        tampered.credential_subject.properties = claim.into_properties();
        db.conn()
            .execute(
                "UPDATE credentials SET signed_vc_json = ?1 WHERE id = ?2",
                rusqlite::params![
                    serde_json::to_string(&tampered).unwrap(),
                    signed.id.clone().unwrap()
                ],
            )
            .unwrap();

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(
            snapshot,
            EntitlementSnapshot::empty(),
            "a broken signature must not be rescued by a trusted issuer"
        );
    }
}
