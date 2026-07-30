// SPDX-License-Identifier: MIT
//! Accept a credential somebody handed you.
//!
//! Every other way a credential enters the local store either mints it here
//! (`commands::credentials::issue_credential`, signed with *this* identity) or
//! pulls it from a peer over P2P. Neither covers the ordinary case of being
//! given a credential out of band — a file, a download, a blob fetched by
//! hash — which is how an issuer that is not a peer delivers one.
//!
//! This is MIT and unconditional. Receiving and verifying a credential is the
//! community product; nothing about accepting one is commercial. It is also
//! deliberately type-agnostic: it stores any valid VC, and leaves policy about
//! *what a credential means* to the readers. `get_entitlement_snapshot` decides
//! whether an entitlement counts; this only decides whether it is real.

use std::str::FromStr;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::content_store::{content, fetch};
use crate::domain::vc::verify::verify_credential;
use crate::domain::vc::{
    AcceptanceDecision, CredentialSubject, EntitlementClaim, RoleClaim, SkillClaim,
    VerifiableCredential, VerificationPolicy,
};
use crate::AppState;

/// Outcome of an import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    /// Envelope id of the credential now in the store.
    pub credential_id: String,
    /// False when a credential with this id was already present. Import is
    /// idempotent, so re-importing is a success, not an error — but the caller
    /// usually wants to say "already installed" rather than "installed".
    pub stored: bool,
}

/// The policy an imported credential must satisfy.
///
/// `allowed_types` is empty on purpose, which means "no type constraint". This
/// is a general inbox: an entitlement, a membership role, a skill credential
/// from an institution are all legitimate things to be handed, and deciding
/// which ones *matter* belongs to whoever reads them later. Everything else is
/// strict — an expired, revoked, suspended or superseded credential is not
/// worth storing.
fn import_policy() -> VerificationPolicy {
    VerificationPolicy {
        reject_expired: true,
        require_integrity_anchor: false,
        allowed_types: vec![],
        reject_suspended: true,
        reject_superseded: true,
    }
}

/// The `claim_kind` column value for a subject, mirroring `Claim::kind_str`.
///
/// An imported credential arrives in its on-disk W3C shape with the claim
/// inlined in the subject, so the discriminator has to be recovered from the
/// marker properties rather than read off a `Claim` enum. Order matters: the
/// markers are distinct (`skillId`, `role`, `entitlementPlan`), and anything
/// carrying none of them is a custom claim, which is legitimate.
fn claim_kind_of(subject: &CredentialSubject) -> &'static str {
    if SkillClaim::extract(subject).is_some() {
        "skill"
    } else if RoleClaim::extract(subject).is_some() {
        "role"
    } else if EntitlementClaim::extract(subject).is_some() {
        "entitlement"
    } else {
        "custom"
    }
}

/// Verify a credential and store it.
///
/// Verification happens **before** the insert and a failure is an error, not a
/// stored-but-flagged row. A credential that does not verify is not a
/// credential, and letting one land in the table would put the burden of
/// re-checking on every future reader — one of which would eventually forget.
///
/// Idempotent on the envelope `id`: re-importing the same credential reports
/// success with `stored: false` rather than raising a constraint violation.
/// Delivery channels retry, and a customer who clicks the same link twice has
/// not done anything wrong.
pub fn import_credential_impl(
    conn: &rusqlite::Connection,
    vc: &VerifiableCredential,
    now: &str,
) -> Result<ImportOutcome, String> {
    let credential_id = vc
        .id
        .clone()
        .ok_or_else(|| "credential has no envelope id".to_string())?;

    let result = verify_credential(conn, vc, now, &import_policy());
    if result.acceptance_decision != AcceptanceDecision::Accept {
        // Report the failed checks rather than a bare "invalid" — an import
        // that fails for an expired credential and one that fails for a bad
        // signature call for very different responses from the user.
        let mut why = Vec::new();
        if !result.valid_signature {
            why.push("signature");
        }
        if !result.issuer_resolved {
            why.push("issuer not resolvable");
        }
        if !result.subject_bound {
            why.push("subject not a DID");
        }
        if result.revoked {
            why.push("revoked");
        }
        if result.expired {
            why.push("expired");
        }
        if result.suspended {
            why.push("suspended");
        }
        if result.superseded {
            why.push("superseded");
        }
        if why.is_empty() {
            why.push("rejected by policy");
        }
        return Err(format!("credential did not verify: {}", why.join(", ")));
    }

    let already: Option<String> = conn
        .query_row(
            "SELECT id FROM credentials WHERE id = ?1",
            params![credential_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if already.is_some() {
        return Ok(ImportOutcome {
            credential_id,
            stored: false,
        });
    }

    // Derive the denormalized columns from the credential body. Nothing here
    // is trusted input in its own right — every one of these is recomputed
    // from the payload whose signature was just verified.
    let claim_kind = claim_kind_of(&vc.credential_subject);
    let skill_id = SkillClaim::extract(&vc.credential_subject).map(|s| s.skill_id);
    let provenance = SkillClaim::extract(&vc.credential_subject)
        .and_then(|s| s.provenance)
        .map(|p| p.as_str().to_string());
    let credential_type = vc
        .type_
        .iter()
        .find(|t| t.as_str() != "VerifiableCredential")
        .cloned()
        .ok_or_else(|| "credential names no class beyond VerifiableCredential".to_string())?;
    let (list_id, list_index) = match &vc.credential_status {
        Some(s) => (
            Some(s.status_list_credential.clone()),
            Some(s.status_list_index.clone()),
        ),
        None => (None, None),
    };
    // Same derivation `issue_credential` uses, reused rather than reimplemented
    // so an imported credential and a locally issued one hash identically.
    let integrity_hash = super::credentials::integrity_hash_of(vc)?;
    let signed_json = serde_json::to_string(vc).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO credentials \
         (id, issuer_did, subject_did, credential_type, claim_kind, skill_id, \
          issuance_date, expiration_date, signed_vc_json, integrity_hash, \
          status_list_id, status_list_index, provenance) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            credential_id,
            vc.issuer.as_str(),
            vc.credential_subject.id.as_str(),
            credential_type,
            claim_kind,
            skill_id,
            vc.valid_from,
            vc.valid_until,
            signed_json,
            integrity_hash,
            list_id,
            list_index,
            provenance,
        ],
    )
    .map_err(|e| format!("insert imported credential: {e}"))?;

    Ok(ImportOutcome {
        credential_id,
        stored: true,
    })
}

#[tauri::command]
pub async fn import_credential(
    state: State<'_, AppState>,
    credential: VerifiableCredential,
) -> Result<ImportOutcome, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let now = super::credentials::now_rfc3339();
    import_credential_impl(db.conn(), &credential, &now)
}

/// Where a credential can be fetched from: a provider endpoint and the BLAKE3
/// hash of the credential's bytes.
///
/// This pair is the whole "ticket". It is small enough to sit in an email line
/// or an `alexandria://` deep link, which a whole credential is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialTicket {
    /// The provider's iroh endpoint id, base32-hex.
    pub provider: String,
    /// BLAKE3 hash of the credential JSON, hex.
    pub hash: String,
}

/// Parse credential JSON that arrived as raw bytes.
///
/// Split out from the fetch so the decode half is testable without a live iroh
/// node. Anything that is not a VC is rejected here rather than reaching the
/// verifier with a half-populated envelope.
fn credential_from_bytes(bytes: &[u8]) -> Result<VerifiableCredential, String> {
    serde_json::from_slice(bytes)
        .map_err(|e| format!("fetched bytes are not a verifiable credential: {e}"))
}

/// Resolve a ticket's provider field to an iroh address.
///
/// A bare endpoint id carries no transport addresses, so the connection relies
/// on iroh's address lookup to find a route — which is the point: the customer
/// copies one short string, not a set of IPs that go stale.
fn provider_addr(provider: &str) -> Result<iroh::EndpointAddr, String> {
    let id = iroh::EndpointId::from_str(provider.trim())
        .map_err(|e| format!("bad provider endpoint id: {e}"))?;
    Ok(iroh::EndpointAddr::from(id))
}

/// Fetch a credential from an iroh provider and import it.
///
/// Content addressing gives an integrity check that is independent of the
/// signature: the bytes are verified against the BLAKE3 hash during the fetch,
/// so a substituted payload fails before it ever reaches [`verify_credential`].
/// The two checks answer different questions — the hash says "these are the
/// bytes you asked for", the signature says "the issuer stands behind them" —
/// and passing one does not imply the other.
///
/// Availability is a fetch-time concern only. Once imported, the credential
/// lives in the local store and every later read verifies offline, so a device
/// that activated once keeps working with every provider down.
#[tauri::command]
pub async fn import_credential_from_peer(
    state: State<'_, AppState>,
    ticket: CredentialTicket,
) -> Result<ImportOutcome, String> {
    let addr = provider_addr(&ticket.provider)?;
    let node = state.content_node_required().await?;

    fetch::fetch_hex_from_peer(&node, addr, &ticket.hash)
        .await
        .map_err(|e| format!("fetch credential {}: {e}", ticket.hash))?;

    let bytes = content::get_bytes(&node, &ticket.hash)
        .await
        .map_err(|e| format!("read fetched credential {}: {e}", ticket.hash))?;
    let vc = credential_from_bytes(&bytes)?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let now = super::credentials::now_rfc3339();
    import_credential_impl(db.conn(), &vc, &now)
}

/// Result of importing a payload that may hold many credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    /// Newly stored.
    pub imported: u32,
    /// Verified fine but already present.
    pub already_present: u32,
    /// Rejected, with the reason for each. Never silently dropped: a user who
    /// hands over ten credentials and gets nine needs to know which one failed
    /// and why.
    pub failed: Vec<ImportFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    /// Envelope id when the credential had one, else a positional label.
    pub credential_id: String,
    pub reason: String,
}

/// Pull the credentials out of a payload that is either a single credential or
/// an exported bundle.
///
/// Both shapes are accepted because `export_credentials_bundle` produces the
/// bundle form, and an export you cannot re-import is not a backup. A single
/// credential is what an issuer hands over directly.
///
/// A bundle's `keyRegistry` and `statusLists` are deliberately **ignored**. They
/// are assertions about which keys belong to which issuer and which credentials
/// are revoked — accepting those from whoever handed you a file would let them
/// rewrite your trust and revocation state. Credentials themselves are safe to
/// take from anyone precisely because each one carries a signature that is
/// checked on the way in.
fn credentials_in(payload: &str) -> Result<Vec<VerifiableCredential>, String> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|e| format!("not valid JSON: {e}"))?;

    if let Some(list) = value.get("credentials").and_then(|c| c.as_array()) {
        let mut out = Vec::with_capacity(list.len());
        for (i, item) in list.iter().enumerate() {
            let vc: VerifiableCredential = serde_json::from_value(item.clone())
                .map_err(|e| format!("bundle entry {i} is not a credential: {e}"))?;
            out.push(vc);
        }
        return Ok(out);
    }

    // Bare array, for a payload that is just a list of credentials.
    if let Some(list) = value.as_array() {
        let mut out = Vec::with_capacity(list.len());
        for (i, item) in list.iter().enumerate() {
            let vc: VerifiableCredential = serde_json::from_value(item.clone())
                .map_err(|e| format!("entry {i} is not a credential: {e}"))?;
            out.push(vc);
        }
        return Ok(out);
    }

    let vc: VerifiableCredential =
        serde_json::from_value(value).map_err(|e| format!("not a credential: {e}"))?;
    Ok(vec![vc])
}

/// Import every credential in `payload`, continuing past individual failures.
///
/// One bad credential in a bundle must not cost the user the other nine, so
/// each is imported independently and failures are collected rather than
/// aborting. The payload failing to parse at all is still a hard error — that
/// is a wrong file, not a partial one.
pub fn import_credentials_impl(
    conn: &rusqlite::Connection,
    payload: &str,
    now: &str,
) -> Result<ImportSummary, String> {
    let credentials = credentials_in(payload)?;
    if credentials.is_empty() {
        return Err("no credentials in this file".into());
    }

    let mut summary = ImportSummary {
        imported: 0,
        already_present: 0,
        failed: Vec::new(),
    };

    for (i, vc) in credentials.iter().enumerate() {
        let label = vc
            .id
            .clone()
            .unwrap_or_else(|| format!("credential {}", i + 1));
        match import_credential_impl(conn, vc, now) {
            Ok(outcome) if outcome.stored => summary.imported += 1,
            Ok(_) => summary.already_present += 1,
            Err(reason) => summary.failed.push(ImportFailure {
                credential_id: label,
                reason,
            }),
        }
    }

    Ok(summary)
}

#[tauri::command]
pub async fn import_credentials(
    state: State<'_, AppState>,
    payload: String,
) -> Result<ImportSummary, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let now = super::credentials::now_rfc3339();
    import_credentials_impl(db.conn(), &payload, &now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::{derive_did_key, Did, VerificationMethodRef};
    use crate::db::Database;
    use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
    use crate::domain::vc::{CredentialType, Proof};
    use ed25519_dalek::SigningKey;

    const NOW: &str = "2026-04-13T00:00:00Z";

    fn key(seed: &str) -> SigningKey {
        let mut bytes = [0u8; 32];
        let s = seed.as_bytes();
        bytes[..s.len().min(32)].copy_from_slice(&s[..s.len().min(32)]);
        SigningKey::from_bytes(&bytes)
    }

    fn open_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    /// A signed credential of `class` carrying `properties`, never stored.
    fn signed_vc(
        issuer_key: &SigningKey,
        class: CredentialType,
        id: &str,
        properties: serde_json::Value,
        valid_until: Option<&str>,
    ) -> VerifiableCredential {
        let issuer = derive_did_key(issuer_key);
        let vc = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some(id.to_string()),
            type_: vec!["VerifiableCredential".into(), class.as_str().to_string()],
            issuer: issuer.clone(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_until: valid_until.map(str::to_string),
            credential_subject: CredentialSubject {
                id: Did("did:key:z6MkTheSubject".into()),
                properties: properties.as_object().unwrap().clone(),
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
        sign_credential(UnsignedCredential { credential: vc }, issuer_key, &issuer).unwrap()
    }

    fn entitlement_props() -> serde_json::Value {
        serde_json::json!({
            "orgId": "org-1",
            "entitlementPlan": "enterprise",
            "seats": 10,
            "features": ["talent_index"],
        })
    }

    #[test]
    fn a_valid_credential_is_stored() {
        let db = open_db();
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:1",
            entitlement_props(),
            None,
        );

        let outcome = import_credential_impl(db.conn(), &vc, NOW).unwrap();
        assert!(outcome.stored);
        assert_eq!(outcome.credential_id, "urn:test:import:1");

        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credentials WHERE id = ?1",
                params!["urn:test:import:1"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    /// A tampered credential must not reach the table at all. Storing it and
    /// flagging it would push the re-check onto every future reader.
    #[test]
    fn a_tampered_credential_is_refused_and_not_stored() {
        let db = open_db();
        let mut vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:2",
            entitlement_props(),
            None,
        );
        // Grant an extra feature without re-signing.
        let mut claim = EntitlementClaim::extract(&vc.credential_subject).unwrap();
        claim.features.push("employer_console".into());
        vc.credential_subject.properties = claim.into_properties();

        let err = import_credential_impl(db.conn(), &vc, NOW).unwrap_err();
        assert!(err.contains("signature"), "unexpected reason: {err}");

        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn an_expired_credential_is_refused() {
        let db = open_db();
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:3",
            entitlement_props(),
            Some("2026-02-01T00:00:00Z"),
        );

        let err = import_credential_impl(db.conn(), &vc, NOW).unwrap_err();
        assert!(err.contains("expired"), "unexpected reason: {err}");
    }

    /// Delivery channels retry and customers click links twice.
    #[test]
    fn importing_the_same_credential_twice_is_idempotent() {
        let db = open_db();
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:4",
            entitlement_props(),
            None,
        );

        assert!(import_credential_impl(db.conn(), &vc, NOW).unwrap().stored);
        let second = import_credential_impl(db.conn(), &vc, NOW).unwrap();
        assert!(!second.stored, "second import must not duplicate");

        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    /// The denormalized columns must be recomputed from the signed body, since
    /// readers filter on them. `get_entitlement_snapshot` queries by
    /// `credential_type`, so getting this wrong would make an imported
    /// entitlement invisible.
    #[test]
    fn denormalized_columns_are_derived_from_the_credential_body() {
        let db = open_db();
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:5",
            entitlement_props(),
            None,
        );
        import_credential_impl(db.conn(), &vc, NOW).unwrap();

        let (ctype, kind, issuer, subject): (String, String, String, String) = db
            .conn()
            .query_row(
                "SELECT credential_type, claim_kind, issuer_did, subject_did \
                 FROM credentials WHERE id = ?1",
                params!["urn:test:import:5"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(ctype, CredentialType::EntitlementCredential.as_str());
        assert_eq!(kind, "entitlement");
        assert_eq!(issuer, vc.issuer.as_str());
        assert_eq!(subject, "did:key:z6MkTheSubject");
    }

    /// Role credentials import too — Unit 2's per-org route needs membership
    /// credentials to arrive by the same door as entitlements.
    #[test]
    fn a_role_credential_imports_with_the_role_claim_kind() {
        let db = open_db();
        let vc = signed_vc(
            &key("org"),
            CredentialType::RoleCredential,
            "urn:test:import:6",
            serde_json::json!({ "role": "member", "scope": "did:key:z6MkOrg" }),
            None,
        );
        import_credential_impl(db.conn(), &vc, NOW).unwrap();

        let kind: String = db
            .conn()
            .query_row(
                "SELECT claim_kind FROM credentials WHERE id = ?1",
                params!["urn:test:import:6"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "role");
    }

    /// The import policy imposes no type constraint on purpose — this is a
    /// general inbox, not an entitlement-specific one.
    #[test]
    fn import_policy_accepts_any_credential_class() {
        assert!(
            import_policy().allowed_types.is_empty(),
            "an empty allowed_types is what makes this a general inbox"
        );
    }

    #[test]
    fn a_credential_without_an_envelope_id_is_refused() {
        let db = open_db();
        let mut vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:import:7",
            entitlement_props(),
            None,
        );
        vc.id = None;

        let err = import_credential_impl(db.conn(), &vc, NOW).unwrap_err();
        assert!(err.contains("envelope id"), "unexpected reason: {err}");
    }

    // ---- iroh delivery ----------------------------------------------------

    /// The decode half of the fetch path, exercised without a live node.
    /// Round-trips through the exact bytes a provider would serve.
    #[test]
    fn credential_bytes_round_trip_through_the_fetch_decoder() {
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:ticket:1",
            entitlement_props(),
            None,
        );
        let served = serde_json::to_vec(&vc).unwrap();

        let decoded = credential_from_bytes(&served).unwrap();
        // Compared as JSON rather than by PartialEq, which the domain type does
        // not derive — and which is the stronger assertion anyway, since it
        // catches a field silently dropped in the round trip.
        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            serde_json::to_value(&vc).unwrap()
        );

        // And it still imports, so the fetch path and the file path converge
        // on the same credential rather than two nearly-identical ones.
        let db = open_db();
        assert!(
            import_credential_impl(db.conn(), &decoded, NOW)
                .unwrap()
                .stored
        );
    }

    /// A provider serving something that is not a credential must fail at the
    /// decode step, not reach the verifier with a half-populated envelope.
    #[test]
    fn non_credential_bytes_are_rejected_before_verification() {
        let err = credential_from_bytes(b"{\"not\": \"a credential\"}").unwrap_err();
        assert!(err.contains("not a verifiable credential"), "got: {err}");

        let err = credential_from_bytes(b"absolute nonsense").unwrap_err();
        assert!(err.contains("not a verifiable credential"), "got: {err}");
    }

    /// A malformed provider id is caught locally rather than surfacing as an
    /// opaque connection failure after a timeout.
    #[test]
    fn a_malformed_provider_id_is_rejected_up_front() {
        assert!(provider_addr("not-an-endpoint-id").is_err());
        assert!(provider_addr("").is_err());
    }

    /// A well-formed endpoint id parses to an address carrying that id.
    #[test]
    fn a_well_formed_provider_id_parses() {
        let secret = iroh::SecretKey::generate();
        let id = secret.public();
        let addr = provider_addr(&id.to_string()).expect("a real endpoint id parses");
        assert_eq!(addr.id, id);
    }

    /// Surrounding whitespace is survivable — this string is copied out of
    /// emails and terminals.
    #[test]
    fn a_provider_id_with_surrounding_whitespace_still_parses() {
        let secret = iroh::SecretKey::generate();
        let id = secret.public();
        let addr = provider_addr(&format!("  {id}\n")).expect("whitespace is trimmed");
        assert_eq!(addr.id, id);
    }

    // ---- bundle-aware import ---------------------------------------------

    /// The export/import round trip. `export_credentials_bundle` produces the
    /// bundle envelope, so an import that only understood bare credentials
    /// would make the export unusable as a backup.
    #[test]
    fn a_bundle_envelope_imports_its_credentials() {
        let db = open_db();
        let a = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:bundle:a",
            entitlement_props(),
            None,
        );
        let b = signed_vc(
            &key("issuer"),
            CredentialType::RoleCredential,
            "urn:test:bundle:b",
            serde_json::json!({ "role": "member", "scope": "did:key:z6MkOrg" }),
            None,
        );
        let bundle = serde_json::json!({
            "formatVersion": "1",
            "credentials": [a, b],
            "keyRegistry": [],
            "statusLists": [],
        })
        .to_string();

        let summary = import_credentials_impl(db.conn(), &bundle, NOW).unwrap();
        assert_eq!(summary.imported, 2);
        assert!(summary.failed.is_empty());
    }

    /// A bundle's key registry and status lists are assertions about issuer
    /// keys and revocation. Taking those from whoever handed you a file would
    /// let them rewrite your trust state, so they are ignored — credentials
    /// are safe to accept from anyone only because each carries a signature.
    #[test]
    fn a_bundle_cannot_smuggle_in_key_registry_or_status_lists() {
        let db = open_db();
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:bundle:c",
            entitlement_props(),
            None,
        );
        let bundle = serde_json::json!({
            "formatVersion": "1",
            "credentials": [vc],
            "keyRegistry": [{
                "did": "did:key:z6MkAttacker",
                "keyId": "k1",
                "publicKeyHex": "00",
                "validFrom": "2020-01-01T00:00:00Z",
                "validUntil": null,
                "rotatedBy": null,
            }],
            "statusLists": [{ "listId": "urn:evil", "bits": "AAAA" }],
        })
        .to_string();

        import_credentials_impl(db.conn(), &bundle, NOW).unwrap();

        let keys: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM key_registry", [], |r| r.get(0))
            .unwrap();
        let lists: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM credential_status_lists", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            keys, 0,
            "an imported bundle must not write the key registry"
        );
        assert_eq!(lists, 0, "an imported bundle must not write status lists");
    }

    /// One bad credential must not cost the user the good ones.
    #[test]
    fn a_failure_does_not_abort_the_rest_of_the_bundle() {
        let db = open_db();
        let good = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:bundle:good",
            entitlement_props(),
            None,
        );
        let mut tampered = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:bundle:bad",
            entitlement_props(),
            None,
        );
        let mut claim = EntitlementClaim::extract(&tampered.credential_subject).unwrap();
        claim.features.push("employer_console".into());
        tampered.credential_subject.properties = claim.into_properties();

        let bundle = serde_json::json!({
            "formatVersion": "1",
            "credentials": [tampered, good],
            "keyRegistry": [],
            "statusLists": [],
        })
        .to_string();

        let summary = import_credentials_impl(db.conn(), &bundle, NOW).unwrap();
        assert_eq!(summary.imported, 1, "the good credential still lands");
        assert_eq!(summary.failed.len(), 1);
        assert_eq!(summary.failed[0].credential_id, "urn:test:bundle:bad");
        assert!(summary.failed[0].reason.contains("signature"));
    }

    #[test]
    fn a_bare_credential_still_imports() {
        let db = open_db();
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:bare",
            entitlement_props(),
            None,
        );
        let summary =
            import_credentials_impl(db.conn(), &serde_json::to_string(&vc).unwrap(), NOW).unwrap();
        assert_eq!(summary.imported, 1);
    }

    #[test]
    fn re_importing_reports_already_present_rather_than_failing() {
        let db = open_db();
        let vc = signed_vc(
            &key("issuer"),
            CredentialType::EntitlementCredential,
            "urn:test:dupe",
            entitlement_props(),
            None,
        );
        let payload = serde_json::to_string(&vc).unwrap();
        import_credentials_impl(db.conn(), &payload, NOW).unwrap();

        let again = import_credentials_impl(db.conn(), &payload, NOW).unwrap();
        assert_eq!(again.imported, 0);
        assert_eq!(again.already_present, 1);
        assert!(again.failed.is_empty());
    }

    /// The wrong file is a hard error, not a partial import.
    #[test]
    fn a_payload_that_is_not_credentials_is_rejected_outright() {
        let db = open_db();
        assert!(import_credentials_impl(db.conn(), "not json", NOW).is_err());
        assert!(import_credentials_impl(db.conn(), "{\"unrelated\": true}", NOW).is_err());
        let empty = serde_json::json!({ "credentials": [] }).to_string();
        assert!(import_credentials_impl(db.conn(), &empty, NOW).is_err());
    }
}
