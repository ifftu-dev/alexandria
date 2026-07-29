// SPDX-License-Identifier: LicenseRef-IFFTU-Enterprise
//! Mint `EntitlementCredential`s with the IFFTU issuer key.
//!
//! ## Why this is enterprise, not MIT
//!
//! The boundary doc's graph-integrity test asks "is it part of how a
//! credential is produced?" — and everything that produces a *skill* claim is
//! permanently MIT. An entitlement is deliberately outside that graph: it
//! carries no capability claim about a learner and has aggregation weight
//! `0.00`, so it can never affect a skill score. What it does carry is org,
//! plan, and seats. That is seat accounting and billing, which the boundary
//! doc places squarely in EE.
//!
//! Verification stays MIT (`domain/vc/entitlement.rs`), so a customer can
//! always audit a credential they were issued without enterprise sources.
//! Only minting one is restricted.
//!
//! ## Key custody
//!
//! Follows the `cardano::operator` precedent exactly: the signing key is
//! configured by environment variable on the issuing deployment and is absent
//! everywhere else. There is no "gate" to bypass — on a node without the key,
//! [`load_issuer_key`] returns `None` and issuance is simply not available.
//!
//!   * `ENTITLEMENT_ISSUER_SKEY_PATH` — path to a file holding the raw 32-byte
//!     Ed25519 seed as hex.
//!   * `ENTITLEMENT_ISSUER_SKEY_HEX` — the seed inline as hex (CI / tests).
//!
//! The DID derived from this key must appear in
//! [`TRUSTED_ENTITLEMENT_ISSUERS`] for the resulting credential to be honoured
//! by any client. Minting with an unlisted key produces a credential that
//! verifies and grants nothing — deliberately, so a leaked or rotated-out key
//! cannot unlock anything.
//!
//! [`TRUSTED_ENTITLEMENT_ISSUERS`]: crate::domain::vc::entitlement::TRUSTED_ENTITLEMENT_ISSUERS

use ed25519_dalek::SigningKey;

use crate::content_store::node::ContentNode;
use crate::content_store::{content, storage};
use crate::crypto::did::{derive_did_key, Did, VerificationMethodRef};
use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
use crate::domain::vc::{
    CredentialStatus, CredentialSubject, CredentialType, EntitlementClaim, Proof,
    VerifiableCredential,
};

/// What an entitlement is being issued for.
#[derive(Debug, Clone)]
pub struct IssueEntitlementRequest {
    /// DID of the organisation the entitlement is sold to. This is the
    /// credential subject, so the entitlement is bound to the org the same way
    /// a skill credential is bound to a learner.
    pub org_did: Did,
    /// Organisation identifier in the billing system.
    pub org_id: String,
    /// Plan identifier (e.g. `"team"`, `"enterprise"`).
    pub plan: String,
    /// Seats purchased. Advisory on the client; metered server-side.
    pub seats: u32,
    /// Feature keys this plan unlocks.
    pub features: Vec<String>,
    /// RFC 3339 instant the entitlement stops being valid.
    ///
    /// Required, not optional. An entitlement is a paid term, and an
    /// open-ended one would survive the end of the contract with revocation as
    /// the only remedy — which needs the client to have seen a status list.
    /// A fixed term expires correctly even on a device that has been offline
    /// since issuance.
    pub valid_until: String,
    /// Status-list entry backing revocation, so a term can be cut short.
    /// Reuses the ordinary VC status-list path rather than inventing a
    /// separate entitlement revocation mechanism.
    pub status: Option<CredentialStatus>,
}

/// Load the entitlement issuer key, or `None` when this node is not an
/// issuer — which is every node but IFFTU's.
pub fn load_issuer_key() -> Option<SigningKey> {
    let hex_str = if let Ok(path) = std::env::var("ENTITLEMENT_ISSUER_SKEY_PATH") {
        std::fs::read_to_string(&path).ok()?.trim().to_string()
    } else {
        std::env::var("ENTITLEMENT_ISSUER_SKEY_HEX").ok()?
    };

    let bytes = hex::decode(hex_str.trim()).ok()?;
    let seed: [u8; 32] = bytes.try_into().ok()?;
    Some(SigningKey::from_bytes(&seed))
}

/// True if this node holds an entitlement issuer key.
pub fn is_issuer() -> bool {
    std::env::var("ENTITLEMENT_ISSUER_SKEY_PATH").is_ok()
        || std::env::var("ENTITLEMENT_ISSUER_SKEY_HEX").is_ok()
}

/// Mint and sign an entitlement credential.
///
/// `now` is the RFC 3339 issuance instant, passed in rather than read from the
/// clock so issuance is reproducible in tests.
///
/// The signature is produced by the MIT `sign_credential` primitive — this
/// module contributes the *shape* and the *key*, not any new cryptography.
pub fn issue_entitlement(
    signing_key: &SigningKey,
    req: &IssueEntitlementRequest,
    credential_id: &str,
    now: &str,
) -> Result<VerifiableCredential, String> {
    if req.valid_until.as_str() <= now {
        return Err("entitlement would be expired at issuance".into());
    }

    let issuer = derive_did_key(signing_key);
    let claim = EntitlementClaim {
        org_id: req.org_id.clone(),
        entitlement_plan: req.plan.clone(),
        seats: req.seats,
        features: req.features.clone(),
    };

    let credential = VerifiableCredential {
        context: vec!["https://www.w3.org/ns/credentials/v2".into()],
        id: Some(credential_id.to_string()),
        type_: vec![
            "VerifiableCredential".into(),
            CredentialType::EntitlementCredential.as_str().to_string(),
        ],
        issuer: issuer.clone(),
        valid_from: now.to_string(),
        valid_until: Some(req.valid_until.clone()),
        credential_subject: CredentialSubject {
            id: req.org_did.clone(),
            properties: claim.into_properties(),
        },
        credential_status: req.status.clone(),
        terms_of_use: None,
        witness: None,
        integrity: None,
        proof: Proof {
            type_: "Ed25519Signature2020".into(),
            created: now.to_string(),
            verification_method: VerificationMethodRef(format!("{}#key-1", issuer.as_str())),
            proof_purpose: "assertionMethod".into(),
            jws: String::new(),
        },
    };

    sign_credential(UnsignedCredential { credential }, signing_key, &issuer)
        .map_err(|e| format!("sign entitlement: {e}"))
}

/// The staging issuer key, derived from the published seed in
/// [`crate::domain::vc::entitlement::STAGING_ISSUER_SEED_HEX`].
///
/// Available only under `ee-staging`, which is the same feature that puts the
/// matching DID in the trusted allowlist — so the key and the trust in it
/// appear and disappear together, and neither can be enabled without the other.
///
/// Not a secret and not a shortcut around [`load_issuer_key`]: production
/// issuance still reads a real key from the environment. This exists so the
/// delivery and activation flow can be exercised without minting the
/// production key first.
#[cfg(feature = "ee-staging")]
pub fn staging_issuer_key() -> SigningKey {
    let bytes = hex::decode(crate::domain::vc::entitlement::STAGING_ISSUER_SEED_HEX)
        .expect("published staging seed is valid hex");
    let seed: [u8; 32] = bytes
        .try_into()
        .expect("published staging seed is 32 bytes");
    SigningKey::from_bytes(&seed)
}

/// Where a published credential can be fetched from.
///
/// Mirrors `commands::import::CredentialTicket` — the issuer produces this,
/// the customer's device consumes it. Kept as a separate type rather than
/// shared because the MIT side must not depend on enterprise code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedCredential {
    /// BLAKE3 hash of the credential JSON, hex. Half the ticket.
    pub hash: String,
    /// Size of the published bytes.
    pub size: u64,
}

/// The `pins` row type used for published credentials.
///
/// Not one of the eviction tiers (`cache`, `course`, `evidence`, `profile`,
/// `taxonomy`) because it is not any of those things: it is issuer-authored
/// content whose whole purpose is to stay servable.
const PIN_TYPE: &str = "entitlement";

/// Publish a signed credential so customer devices can fetch it by hash.
///
/// Stored **unencrypted**. The credential is already signed, so confidentiality
/// is the only thing at stake, and a 32-byte BLAKE3 hash is unguessable — the
/// hash behaves as a capability, so nobody enumerates their way to an
/// organisation's plan and seat count. Encrypting instead would mean shipping a
/// content key alongside every ticket, which is strictly worse.
///
/// Pinned with `auto_unpin = false`, so storage pressure can never reclaim it.
/// A provider that pins evictably will silently stop serving under load, and
/// the failure looks identical to the customer as "your entitlement does not
/// exist".
///
/// Availability past this point is replication, not protocol: iroh-blobs has no
/// DHT-backed persistence, and content exists only while some node holding it
/// is online. Run more than one provider. Note that a customer only needs the
/// fetch to succeed **once** — after import the credential is local and
/// verifies offline forever.
pub async fn publish_entitlement(
    node: &ContentNode,
    conn: &rusqlite::Connection,
    credential: &VerifiableCredential,
) -> Result<PublishedCredential, String> {
    let bytes = serde_json::to_vec(credential).map_err(|e| format!("serialize credential: {e}"))?;

    let added = content::add_bytes_unencrypted(node, &bytes)
        .await
        .map_err(|e| format!("publish credential: {e}"))?;

    storage::upsert_pin(conn, &added.hash, PIN_TYPE, added.size, false);

    Ok(PublishedCredential {
        hash: added.hash,
        size: added.size,
    })
}

/// Mint a staging entitlement to this device's holder and install it.
///
/// A one-click smoke test for the whole chain, available only under
/// `ee-staging` — the same feature that puts the staging DID in the trusted
/// allowlist, so the key and the trust in it appear together.
///
/// It skips *delivery* deliberately: the credential is minted and imported
/// locally, so a failure here is unambiguously the entitlement logic rather
/// than a network problem. Exercising the real delivery path is what
/// `import_credential_from_peer` is for.
///
/// The import goes through the ordinary MIT `import_credential_impl`, so this
/// is not a back door around verification — the minted credential has to
/// verify and bind exactly like one that arrived from outside.
#[cfg(feature = "ee-staging")]
#[tauri::command]
pub async fn mint_staging_entitlement(
    state: tauri::State<'_, crate::AppState>,
    features: Vec<String>,
) -> Result<crate::commands::import::ImportOutcome, String> {
    let now = crate::commands::credentials::now_rfc3339();
    // A year out. Long enough that a test session never trips the expiry, short
    // enough that a forgotten staging credential does not live forever.
    let valid_until = "2027-12-31T00:00:00Z".to_string();

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let holder = crate::commands::entitlements::local_did(db.conn())
        .ok_or("this profile has no identity yet — create or unlock one first")?;

    let req = IssueEntitlementRequest {
        org_did: Did(holder),
        org_id: "staging-org".into(),
        plan: "staging".into(),
        seats: 1,
        features,
        valid_until,
        status: None,
    };

    // Unique per mint so repeated clicks do not collide on the envelope id,
    // which import treats as "already installed".
    let credential_id = format!("urn:alexandria:staging-entitlement:{now}");
    let vc = issue_entitlement(&staging_issuer_key(), &req, &credential_id, &now)?;

    crate::commands::import::import_credential_impl(db.conn(), &vc, &now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::vc::entitlement::{is_trusted_issuer, parse_entitlement, EntitlementError};
    use crate::domain::vc::verify::verify_credential;
    use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

    const NOW: &str = "2026-04-13T00:00:00Z";
    const LATER: &str = "2027-04-13T00:00:00Z";

    fn issuer_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn request() -> IssueEntitlementRequest {
        IssueEntitlementRequest {
            org_did: Did("did:key:z6MkOrgSubject".into()),
            org_id: "org-1".into(),
            plan: "enterprise".into(),
            seats: 25,
            features: vec!["talent_index".into(), "employer_console".into()],
            valid_until: LATER.into(),
            status: None,
        }
    }

    fn policy() -> VerificationPolicy {
        VerificationPolicy {
            allowed_types: vec![CredentialType::EntitlementCredential],
            ..VerificationPolicy::default()
        }
    }

    /// A minted entitlement must satisfy the ordinary MIT verification path —
    /// issuance introduces no private envelope shape.
    #[test]
    fn a_minted_entitlement_verifies_through_the_mit_path() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();

        let vc = issue_entitlement(&issuer_key(), &request(), "urn:test:ent:1", NOW).unwrap();
        let result = verify_credential(db.conn(), &vc, NOW, &policy());
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
        assert!(result.valid_signature);
    }

    /// Holding the signing key is not the same as being trusted. Until the
    /// issuer DID is added to the compiled-in allowlist, a credential this
    /// module mints grants nothing — which is exactly what a leaked or
    /// rotated-out key must do.
    #[test]
    fn minting_with_an_unlisted_key_grants_nothing() {
        let vc = issue_entitlement(&issuer_key(), &request(), "urn:test:ent:1", NOW).unwrap();
        assert!(!is_trusted_issuer(&vc.issuer));
        assert_eq!(
            parse_entitlement(&vc),
            Err(EntitlementError::UntrustedIssuer)
        );
    }

    /// The subject is the organisation, and the claim round-trips intact.
    #[test]
    fn the_subject_is_the_org_and_the_claim_survives_signing() {
        let req = request();
        let vc = issue_entitlement(&issuer_key(), &req, "urn:test:ent:1", NOW).unwrap();

        assert_eq!(vc.credential_subject.id, req.org_did);
        let claim = EntitlementClaim::extract(&vc.credential_subject).unwrap();
        assert_eq!(claim.org_id, "org-1");
        assert_eq!(claim.entitlement_plan, "enterprise");
        assert_eq!(claim.seats, 25);
        assert!(claim.grants("talent_index"));
        assert!(claim.grants("employer_console"));
    }

    /// A term is mandatory and must be in the future — issuing something
    /// already expired is a billing bug, not a credential to store.
    #[test]
    fn an_already_expired_term_is_refused() {
        let mut req = request();
        req.valid_until = "2020-01-01T00:00:00Z".into();
        assert!(issue_entitlement(&issuer_key(), &req, "urn:test:ent:1", NOW).is_err());
    }

    /// The expiry actually bites at verification time, offline, with no
    /// status list involved.
    #[test]
    fn the_term_expires_without_needing_revocation() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();

        let vc = issue_entitlement(&issuer_key(), &request(), "urn:test:ent:1", NOW).unwrap();
        let after_term = "2028-01-01T00:00:00Z";
        let result = verify_credential(db.conn(), &vc, after_term, &policy());
        assert!(result.expired);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    /// A published credential must be pinned unevictably. A provider that
    /// pins evictably stops serving under storage pressure, and to the
    /// customer that is indistinguishable from the entitlement not existing.
    #[test]
    fn publishing_pins_unevictably() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();

        // Exercise the pin bookkeeping directly; adding bytes needs a running
        // iroh node, which a unit test has no business starting.
        storage::upsert_pin(db.conn(), "abc123", PIN_TYPE, 512, false);

        let (pin_type, auto_unpin): (String, i64) = db
            .conn()
            .query_row(
                "SELECT pin_type, auto_unpin FROM pins WHERE cid = ?1",
                rusqlite::params!["abc123"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(pin_type, PIN_TYPE);
        assert_eq!(auto_unpin, 0, "published credentials must never be evicted");

        // And it must not count as reclaimable space. Asserted against the
        // table directly rather than through the eviction engine's private
        // helper, which would mean widening its visibility for a test.
        let evictable: i64 = db
            .conn()
            .query_row(
                "SELECT COALESCE(SUM(size_bytes), 0) FROM pins WHERE auto_unpin = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(evictable, 0);
    }

    /// The bytes a provider serves are exactly what the importer parses. This
    /// is the wire contract between the enterprise issuer and the MIT import
    /// path, and nothing else pins it.
    #[test]
    fn published_bytes_are_what_the_importer_reads() {
        let vc = issue_entitlement(&issuer_key(), &request(), "urn:test:ent:1", NOW).unwrap();
        let served = serde_json::to_vec(&vc).unwrap();

        let round_tripped: VerifiableCredential = serde_json::from_slice(&served).unwrap();
        assert_eq!(
            serde_json::to_value(&round_tripped).unwrap(),
            serde_json::to_value(&vc).unwrap()
        );
    }

    /// The full path in its real configuration: mint with the staging key,
    /// import as a device would, resolve through the snapshot — using
    /// `TRUSTED_ENTITLEMENT_ISSUERS` itself rather than an injected list.
    ///
    /// Every other accept-path test injects the allowlist, which proves the
    /// logic but not the wiring. This is the one that would catch the staging
    /// DID being present but wrong, or the feature enabling the key without
    /// enabling the trust.
    #[cfg(feature = "ee-staging")]
    #[test]
    fn the_staging_issuer_unlocks_end_to_end() {
        use crate::commands::entitlements::get_entitlement_snapshot_impl;
        use crate::commands::import::import_credential_impl;

        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();

        // The device's holder, and the entitlement addressed to them.
        let holder = Did("did:key:z6MkStagingTestHolder".into());
        db.conn()
            .execute(
                "INSERT INTO app_settings (key, value, scope, updated_at) \
                 VALUES ('identity.local_did', ?1, 'device', datetime('now'))",
                rusqlite::params![holder.as_str()],
            )
            .unwrap();

        let key = staging_issuer_key();
        assert!(
            crate::domain::vc::entitlement::is_trusted_issuer(&derive_did_key(&key)),
            "the staging key must derive the DID the staging allowlist trusts"
        );

        let mut req = request();
        req.org_did = holder;
        let vc = issue_entitlement(&key, &req, "urn:test:staging:1", NOW).unwrap();

        import_credential_impl(db.conn(), &vc, NOW).unwrap();

        let snapshot = get_entitlement_snapshot_impl(db.conn(), NOW).unwrap();
        assert_eq!(snapshot.features, vec!["talent_index", "employer_console"]);
        assert_eq!(snapshot.org_id.as_deref(), Some("org-1"));
    }

    /// Absent configuration, a node is not an issuer. Asserted without
    /// touching the environment so the test cannot race a sibling.
    #[test]
    fn is_issuer_requires_explicit_configuration() {
        if std::env::var("ENTITLEMENT_ISSUER_SKEY_PATH").is_err()
            && std::env::var("ENTITLEMENT_ISSUER_SKEY_HEX").is_err()
        {
            assert!(!is_issuer());
            assert!(load_issuer_key().is_none());
        }
    }
}
