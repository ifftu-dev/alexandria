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

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::domain::vc::entitlement::{parse_entitlement_with, TRUSTED_ENTITLEMENT_ISSUERS};
use crate::domain::vc::verify::verify_credential;
use crate::domain::vc::{
    AcceptanceDecision, CredentialType, RoleClaim, VerifiableCredential, VerificationPolicy,
};
use crate::AppState;

/// The `role` token on a credential that makes its subject a member of the
/// issuing organisation.
///
/// Deliberately an ordinary [`CredentialType::RoleCredential`] rather than a
/// dedicated class: it is the same shape as the guardianship credential
/// (`commands/guardian.rs` issues `role: "guardian"`, `scope: <issuer DID>`),
/// and reusing it means no new credential class, no aggregation weight to
/// decide, and no migration.
pub const ORG_MEMBER_ROLE: &str = "member";

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

/// The policy a membership credential must satisfy.
///
/// A membership is an ordinary role credential, so it gets the ordinary
/// treatment: expired, revoked, suspended or superseded memberships do not
/// carry an entitlement. That is what lets an organisation take a seat back by
/// revoking one credential, using the status-list path that already exists.
fn membership_policy() -> VerificationPolicy {
    VerificationPolicy {
        reject_expired: true,
        require_integrity_anchor: false,
        allowed_types: vec![CredentialType::RoleCredential],
        reject_suspended: true,
        reject_superseded: true,
    }
}

/// This device's DID, or `None` on a profile that has not established one.
///
/// `None` is not an error and must not be treated as "skip the check" — a
/// device with no identity is entitled to nothing.
fn local_did(conn: &rusqlite::Connection) -> Option<String> {
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = 'identity.local_did'",
        [],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .filter(|did| !did.is_empty())
}

/// Role credentials naming `subject_did` as their subject.
///
/// Filtered again by the caller against the credential body — see
/// [`binds_to_holder`] for why the denormalized column alone is not enough.
fn stored_memberships(
    conn: &rusqlite::Connection,
    subject_did: &str,
) -> Result<Vec<VerifiableCredential>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT signed_vc_json FROM credentials \
             WHERE credential_type = ?1 AND subject_did = ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params![CredentialType::RoleCredential.as_str(), subject_did],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        let json = row.map_err(|e| e.to_string())?;
        if let Ok(vc) = serde_json::from_str::<VerifiableCredential>(&json) {
            out.push(vc);
        }
    }
    Ok(out)
}

/// Whether `entitlement` was issued to *this* device's holder.
///
/// Without this an entitlement is a bearer token: `verify_credential`'s subject
/// check only asserts the subject looks like a DID (`starts_with("did:")`), so
/// a credential copied to a second machine would unlock there too. One
/// purchased seat would unlock unlimited devices.
///
/// Two routes bind, matching the two ways an entitlement is sold.
///
/// **Per-seat.** The entitlement names this device's holder directly.
///
/// **Per-org.** The entitlement names an *organisation* DID, which can never
/// equal a user's DID, so the device must additionally hold a membership
/// credential issued by that organisation. Every one of these must hold:
///
/// * the membership's subject is this device's holder — it is about *me*;
/// * the membership's **issuer** is the entitlement's **subject** — signed by
///   the very organisation the entitlement names;
/// * its `scope` names that same organisation, so a membership cannot be
///   replayed against a different one;
/// * it independently verifies — unexpired, unrevoked, unsuspended.
///
/// The issuer condition is what makes this cryptographic rather than a string
/// comparison. An organisation DID is a `did:key`, which is self-describing, so
/// verifying the membership's signature checks it against the exact key the
/// entitlement names as its subject. Forging membership needs the
/// organisation's private key.
///
/// Seats are deliberately not counted here. A device cannot see the other
/// members, so it cannot know the organisation is over its limit; that stays
/// server-side, as `useEntitlements.ts` already says.
fn binds_to_holder(
    conn: &rusqlite::Connection,
    entitlement: &VerifiableCredential,
    local_did: &str,
    now: &str,
) -> bool {
    let org_or_user = entitlement.credential_subject.id.as_str();

    if org_or_user == local_did {
        return true;
    }

    let policy = membership_policy();
    for vc in stored_memberships(conn, local_did).unwrap_or_default() {
        // Re-check the subject against the credential body. `subject_did` is a
        // denormalized column, and a row whose column disagrees with the signed
        // payload must not be able to bind an entitlement to the wrong holder.
        if vc.credential_subject.id.as_str() != local_did {
            continue;
        }
        if vc.issuer.as_str() != org_or_user {
            continue;
        }
        let Some(role) = RoleClaim::extract(&vc.credential_subject) else {
            continue;
        };
        if role.role != ORG_MEMBER_ROLE || role.scope.as_deref() != Some(org_or_user) {
            continue;
        }
        if verify_credential(conn, &vc, now, &policy).acceptance_decision
            == AcceptanceDecision::Accept
        {
            return true;
        }
    }

    false
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
/// A credential counts only if it clears **all three** gates:
///
/// 1. [`verify_credential`] accepts it — signature, issuer resolution, expiry,
///    revocation, suspension, supersession.
/// 2. [`parse_entitlement_with`] accepts it — the issuer is one this build was
///    compiled to trust.
/// 3. [`binds_to_holder`] accepts it — it was issued to *this* device's holder,
///    directly or through organisation membership.
///
/// None of the three is redundant.
///
/// Gate 2 exists because verification proves a credential was signed by whoever
/// its issuer DID *names*, and a `did:key` is self-describing, so anyone can
/// mint one and self-issue a credential that verifies perfectly. Without the
/// issuer pin, gate 1 alone would grant every feature to anybody who asked.
///
/// Gate 3 exists because verification's subject check only asserts the subject
/// looks like a DID at all. Without it, an entitlement is a bearer token: copy
/// the file to another machine and it unlocks there too.
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
    // No identity, nothing to bind an entitlement to. Returning early rather
    // than falling through keeps a fresh profile from being a hole in gate 3.
    let Some(holder) = local_did(conn) else {
        return Ok(EntitlementSnapshot {
            features: Vec::new(),
            org_id: None,
            plan: None,
        });
    };

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
        if !binds_to_holder(conn, &vc, &holder, now) {
            continue;
        }

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
    use crate::domain::vc::entitlement::is_trusted_issuer_in;
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
        subject: &Did,
        features: &[&str],
        valid_until: Option<&str>,
    ) -> VerifiableCredential {
        let signed = signed_entitlement(issuer_key, subject, features, valid_until);
        insert_credential(
            db,
            &signed,
            CredentialType::EntitlementCredential,
            "entitlement",
        );
        signed
    }

    /// Build and sign an entitlement without storing it — the shape a billing
    /// service produces before delivery.
    fn signed_entitlement(
        issuer_key: &SigningKey,
        subject: &Did,
        features: &[&str],
        valid_until: Option<&str>,
    ) -> VerifiableCredential {
        let issuer = derive_did_key(issuer_key);
        let subject = subject.clone();
        let claim = EntitlementClaim {
            org_id: "org-1".into(),
            entitlement_plan: "enterprise".into(),
            seats: 10,
            features: features.iter().map(|f| f.to_string()).collect(),
        };
        let vc = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some(format!(
                "urn:test:entitlement:{}:{}",
                subject.as_str(),
                features.join("-")
            )),
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

        signed
    }

    /// Store an already-signed credential the way the issuance path would.
    fn insert_credential(
        db: &Database,
        signed: &VerifiableCredential,
        class: CredentialType,
        claim_kind: &str,
    ) {
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
                    class.as_str(),
                    claim_kind,
                    signed.valid_from,
                    "test-integrity-hash",
                    serde_json::to_string(signed).unwrap(),
                ],
            )
            .unwrap();
    }

    /// The DID this device's holder uses in these tests.
    const LOCAL_DID: &str = "did:key:z6MkThisDeviceHolder";
    fn holder() -> Did {
        Did(LOCAL_DID.into())
    }

    /// The organisation's signing key. Route B verifies a real signature
    /// against the DID the entitlement names as its subject, so the
    /// organisation DID has to be a genuine `did:key` — a placeholder string
    /// would make the membership unverifiable and the test vacuous.
    fn org_key() -> SigningKey {
        key("acme-org")
    }

    /// An organisation DID. Never equal to `LOCAL_DID`, which is the whole
    /// reason route B exists.
    fn org() -> Did {
        derive_did_key(&org_key())
    }

    fn open_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        set_local_did(&db, LOCAL_DID);
        db
    }

    /// A database with no established identity, to prove that is not a hole.
    fn open_db_without_identity() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    fn set_local_did(db: &Database, did: &str) {
        db.conn()
            .execute(
                "INSERT INTO app_settings (key, value, scope, updated_at) \
                 VALUES ('identity.local_did', ?1, 'device', datetime('now')) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![did],
            )
            .unwrap();
    }

    /// Sign and store a membership role credential: `org_key` (the
    /// organisation) attesting that `subject` is one of its members.
    ///
    /// `scope` is a parameter rather than derived so a test can build the
    /// mismatched-scope replay case.
    fn store_membership(
        db: &Database,
        org_key: &SigningKey,
        subject: &Did,
        role: &str,
        scope: &str,
        valid_until: Option<&str>,
    ) -> VerifiableCredential {
        let signed = signed_membership(org_key, subject, role, scope, valid_until);
        insert_credential(db, &signed, CredentialType::RoleCredential, "role");
        signed
    }

    /// Build and sign a membership without storing it.
    fn signed_membership(
        org_key: &SigningKey,
        subject: &Did,
        role: &str,
        scope: &str,
        valid_until: Option<&str>,
    ) -> VerifiableCredential {
        let issuer = derive_did_key(org_key);
        let claim = RoleClaim {
            role: role.to_string(),
            scope: Some(scope.to_string()),
        };
        let vc = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some(format!(
                "urn:test:membership:{}:{}:{}",
                issuer.as_str(),
                subject.as_str(),
                scope
            )),
            type_: vec![
                "VerifiableCredential".into(),
                CredentialType::RoleCredential.as_str().to_string(),
            ],
            issuer: issuer.clone(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_until: valid_until.map(str::to_string),
            credential_subject: crate::domain::vc::CredentialSubject {
                id: subject.clone(),
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
        let signed =
            sign_credential(UnsignedCredential { credential: vc }, org_key, &issuer).unwrap();

        signed
    }

    /// The shipped configuration. No trusted issuer is compiled in, so a
    /// perfectly valid, correctly signed, unexpired entitlement still grants
    /// nothing — this is the test that would fail first if the issuer pin were
    /// ever dropped.
    #[test]
    fn a_valid_but_self_issued_entitlement_grants_nothing() {
        let db = open_db();
        let signed = store_entitlement(&db, &key("attacker"), &holder(), &["talent_index"], None);

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
        let signed = store_entitlement(&db, &k, &holder(), &["talent_index"], None);
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
        let a = store_entitlement(&db, &k, &holder(), &["talent_index"], None);
        store_entitlement(
            &db,
            &k,
            &holder(),
            &["employer_console", "talent_index"],
            None,
        );
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
        let signed = store_entitlement(
            &db,
            &k,
            &holder(),
            &["talent_index"],
            Some("2026-02-01T00:00:00Z"),
        );
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
        let signed = store_entitlement(&db, &k, &holder(), &["talent_index"], None);
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

    // ---- gate 3: binding to this device's holder -------------------------

    /// The regression test for the bearer-token defect. `verify_credential`'s
    /// subject check only asserts the subject looks like a DID, so before this
    /// gate existed a credential copied to a second machine unlocked there
    /// too — one purchased seat unlocking unlimited devices.
    ///
    /// Do not delete this test.
    #[test]
    fn an_entitlement_issued_to_someone_else_does_not_unlock_this_device() {
        let db = open_db();
        let k = key("ifftu");
        let someone_else = Did("did:key:z6MkADifferentPersonEntirely".into());
        let signed = store_entitlement(&db, &k, &someone_else, &["talent_index"], None);
        let trusted = [signed.issuer.as_str()];

        // The credential itself is beyond reproach: signed by a trusted
        // issuer, unexpired, unrevoked. It simply is not ours.
        let verified = verify_credential(db.conn(), &signed, NOW, &entitlement_policy());
        assert_eq!(verified.acceptance_decision, AcceptanceDecision::Accept);
        assert!(is_trusted_issuer_in(&signed.issuer, &trusted));

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(snapshot, EntitlementSnapshot::empty());
    }

    /// Route B, the accept path: IFFTU signs to the organisation, the
    /// organisation signs membership to this holder.
    #[test]
    fn an_org_entitlement_unlocks_via_a_membership_credential() {
        let db = open_db();
        let k = key("ifftu");
        let signed = store_entitlement(&db, &k, &org(), &["talent_index"], None);
        store_membership(
            &db,
            &org_key(),
            &holder(),
            ORG_MEMBER_ROLE,
            org().as_str(),
            None,
        );
        let trusted = [signed.issuer.as_str()];

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(snapshot.features, vec!["talent_index".to_string()]);
    }

    /// The same entitlement without the membership grants nothing. An org DID
    /// can never equal a user DID, so route A cannot rescue it.
    #[test]
    fn an_org_entitlement_alone_grants_nothing() {
        let db = open_db();
        let k = key("ifftu");
        let signed = store_entitlement(&db, &k, &org(), &["talent_index"], None);
        let trusted = [signed.issuer.as_str()];

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(snapshot, EntitlementSnapshot::empty());
    }

    /// Being a member of some *other* organisation does not entitle you to
    /// this one's plan. This is the condition that makes the check
    /// cryptographic: the membership must be signed by the very key the
    /// entitlement names as its subject.
    #[test]
    fn membership_of_a_different_org_does_not_bind() {
        let db = open_db();
        let k = key("ifftu");
        let signed = store_entitlement(&db, &k, &org(), &["talent_index"], None);

        let other_org_key = key("other-org");
        let other_org = derive_did_key(&other_org_key);
        assert_ne!(other_org.as_str(), org().as_str());
        store_membership(
            &db,
            &other_org_key,
            &holder(),
            ORG_MEMBER_ROLE,
            other_org.as_str(),
            None,
        );
        let trusted = [signed.issuer.as_str()];

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(snapshot, EntitlementSnapshot::empty());
    }

    /// The replay guard. A membership genuinely signed by some other
    /// organisation, but whose `scope` names the *target* organisation, must
    /// not bind — otherwise anyone able to issue themselves a role credential
    /// could point it at a paying customer.
    #[test]
    fn a_membership_whose_scope_disagrees_with_its_issuer_does_not_bind() {
        let db = open_db();
        let k = key("ifftu");
        let signed = store_entitlement(&db, &k, &org(), &["talent_index"], None);

        let attacker_key = key("attacker-org");
        store_membership(
            &db,
            &attacker_key,
            &holder(),
            ORG_MEMBER_ROLE,
            // scope claims the target org, but the signature is the attacker's
            org().as_str(),
            None,
        );
        let trusted = [signed.issuer.as_str()];

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(snapshot, EntitlementSnapshot::empty());
    }

    /// Revoking a seat is revoking the membership. Here the expiry path
    /// stands in for the status list, which the same policy also honours.
    #[test]
    fn an_expired_membership_takes_the_seat_back() {
        let db = open_db();
        let k = key("ifftu");
        let signed = store_entitlement(&db, &k, &org(), &["talent_index"], None);
        store_membership(
            &db,
            &org_key(),
            &holder(),
            ORG_MEMBER_ROLE,
            org().as_str(),
            Some("2026-02-01T00:00:00Z"),
        );
        let trusted = [signed.issuer.as_str()];

        // Inside the membership term the entitlement resolves...
        let before = snapshot_with_trusted(db.conn(), "2026-01-15T00:00:00Z", &trusted).unwrap();
        assert_eq!(before.features, vec!["talent_index".to_string()]);

        // ...and once the membership lapses it does not, even though the
        // entitlement itself never expires.
        let after = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(after, EntitlementSnapshot::empty());
    }

    /// A role credential from the right organisation but naming a different
    /// role does not confer membership. Guards against any role the org
    /// happens to issue doubling as an entitlement key.
    #[test]
    fn a_role_other_than_member_does_not_bind() {
        let db = open_db();
        let k = key("ifftu");
        let signed = store_entitlement(&db, &k, &org(), &["talent_index"], None);
        store_membership(&db, &org_key(), &holder(), "guardian", org().as_str(), None);
        let trusted = [signed.issuer.as_str()];

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(snapshot, EntitlementSnapshot::empty());
    }

    /// A membership issued to somebody else, sitting in this device's store,
    /// binds nothing. Storing a credential is not holding it.
    #[test]
    fn a_membership_issued_to_another_person_does_not_bind() {
        let db = open_db();
        let k = key("ifftu");
        let signed = store_entitlement(&db, &k, &org(), &["talent_index"], None);
        let colleague = Did("did:key:z6MkSomeoneElseAtTheOrg".into());
        store_membership(
            &db,
            &org_key(),
            &colleague,
            ORG_MEMBER_ROLE,
            org().as_str(),
            None,
        );
        let trusted = [signed.issuer.as_str()];

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(snapshot, EntitlementSnapshot::empty());
    }

    /// A profile with no identity is entitled to nothing. Absent
    /// `identity.local_did` the binding gate has nothing to compare against,
    /// and that must fail closed rather than be skipped.
    #[test]
    fn a_device_without_an_identity_is_entitled_to_nothing() {
        let db = open_db_without_identity();
        let k = key("ifftu");
        // Issued directly to a holder that would otherwise match route A.
        let signed = store_entitlement(&db, &k, &holder(), &["talent_index"], None);
        let trusted = [signed.issuer.as_str()];

        assert!(local_did(db.conn()).is_none());
        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(snapshot, EntitlementSnapshot::empty());

        // Establishing the identity is what turns it on.
        set_local_did(&db, LOCAL_DID);
        let after = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(after.features, vec!["talent_index".to_string()]);
    }

    // ---- the delivery seam ------------------------------------------------

    /// The whole point of Unit 2: a credential that arrived from outside,
    /// through `import_credential`, must resolve here.
    ///
    /// This is the seam most likely to break silently. `stored_entitlements`
    /// filters on the denormalized `credential_type` column, which the import
    /// path derives independently from the credential body — if the two ever
    /// disagree an imported entitlement becomes invisible rather than
    /// rejected, which is far harder to notice.
    #[test]
    fn an_imported_entitlement_resolves_through_the_snapshot() {
        let db = open_db();
        let k = key("ifftu");

        // Build and sign it without touching the database, exactly as a
        // billing service would, then hand it over as an opaque credential.
        let unstored = signed_entitlement(&k, &holder(), &["talent_index"], None);
        let trusted = [unstored.issuer.as_str()];

        // Nothing yet.
        assert_eq!(
            snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap(),
            EntitlementSnapshot::empty()
        );

        let outcome =
            crate::commands::import::import_credential_impl(db.conn(), &unstored, NOW).unwrap();
        assert!(outcome.stored);

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(snapshot.features, vec!["talent_index".to_string()]);
        assert_eq!(snapshot.org_id.as_deref(), Some("org-1"));
    }

    /// The per-org half of the same seam: both credentials arrive by import,
    /// and the chain still binds.
    #[test]
    fn an_imported_org_entitlement_and_membership_bind() {
        let db = open_db();
        let k = key("ifftu");

        let ent = signed_entitlement(&k, &org(), &["talent_index"], None);
        let trusted = [ent.issuer.as_str()];
        crate::commands::import::import_credential_impl(db.conn(), &ent, NOW).unwrap();

        // Entitlement alone binds nothing.
        assert_eq!(
            snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap(),
            EntitlementSnapshot::empty()
        );

        let membership =
            signed_membership(&org_key(), &holder(), ORG_MEMBER_ROLE, org().as_str(), None);
        crate::commands::import::import_credential_impl(db.conn(), &membership, NOW).unwrap();

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(snapshot.features, vec!["talent_index".to_string()]);
    }

    /// Proves the per-org model needs no new issuance code.
    ///
    /// The organisation's admin mints membership with the ordinary
    /// `issue_credential` command — which already signs with the local
    /// identity, exactly what an org admin wants — using
    /// `RoleCredential` + `role: "member"`. Nothing enterprise-specific is
    /// involved, and nothing had to be added for it.
    ///
    /// This asserts against the real production issuance path rather than the
    /// hand-rolled `store_membership` helper the other tests use, so a change
    /// to `issue_credential_impl` that broke org membership would surface here.
    #[test]
    fn org_membership_can_be_minted_with_the_ordinary_issue_command() {
        use crate::commands::credentials::{issue_credential_impl, IssueCredentialRequest};
        use crate::domain::vc::Claim;

        let db = open_db();
        let org_signing_key = org_key();
        let org_did = derive_did_key(&org_signing_key);

        // The organisation admin issues membership to this device's holder.
        let minted = issue_credential_impl(
            db.conn(),
            &org_signing_key,
            &org_did,
            &IssueCredentialRequest {
                credential_type: CredentialType::RoleCredential,
                subject: holder(),
                claim: Claim::Role(RoleClaim {
                    role: ORG_MEMBER_ROLE.to_string(),
                    scope: Some(org_did.as_str().to_string()),
                }),
                evidence_refs: vec![],
                expiration_date: None,
                supersedes: None,
                integrity_session_id: None,
                integrity_policy: None,
            },
            "2026-01-01T00:00:00Z",
        )
        .expect("an org can mint membership with the existing command");

        assert_eq!(minted.issuer.as_str(), org_did.as_str());
        assert_eq!(minted.credential_subject.id.as_str(), LOCAL_DID);

        // IFFTU's entitlement, addressed to the organisation.
        let ent = signed_entitlement(&key("ifftu"), &org_did, &["talent_index"], None);
        crate::commands::import::import_credential_impl(db.conn(), &ent, NOW).unwrap();
        let trusted = [ent.issuer.as_str()];

        let snapshot = snapshot_with_trusted(db.conn(), NOW, &trusted).unwrap();
        assert_eq!(
            snapshot.features,
            vec!["talent_index".to_string()],
            "membership minted by the ordinary path must bind the entitlement"
        );
    }
}
