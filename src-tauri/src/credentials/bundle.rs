//! Survivability: credential bundle export and offline verification
//! (§20.4).
//!
//! The export bundle is a single JSON document carrying everything a
//! third-party verifier needs to re-check the credentials without any
//! Alexandria infrastructure: the signed VCs themselves, the historical
//! key registry, and the revocation status lists.
//!
//! Determinism comes from JCS canonicalization — same inputs ⇒
//! byte-identical bundle, which is what the survivability tests assert
//! and what archival storage relies on for content-addressing.

use rusqlite::Connection;

use super::{store, Issuer};
use crate::clock::now_rfc3339;
use crate::domain::vc::{VerifiableCredential, VerificationPendingReason, VerificationResult};

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

fn bounded_credentials(entries: &[serde_json::Value]) -> Result<Vec<VerifiableCredential>, String> {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            store::credential_entry(entry).map_err(|error| format!("credential {index}: {error}"))
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
pub fn export(conn: &Connection) -> Result<String, String> {
    export_signed(conn, None)
}

/// As [`export`], and when `signer` is this node's issuer key,
/// also carry each list this node issues as a signed
/// `BitstringStatusListCredential`.
pub fn export_signed(conn: &Connection, signer: Option<&Issuer>) -> Result<String, String> {
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
    if let Some(issuer) = signer {
        let (key, did) = (issuer.key(), issuer.did());
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
pub fn verify_offline(json: &str, now: &str) -> Result<OfflineVerification, String> {
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
    let value = store::parse_credential_payload(json)?;

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
        let vc = store::credential_entry(&value).map_err(|error| format!("credential: {error}"))?;
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

pub fn verify_bundle_offline(
    bundle_json: &str,
    verification_time: &str,
) -> Result<(u32, u32), String> {
    use crate::domain::vc::{verify, AcceptanceDecision, VerificationPolicy};

    let value =
        store::parse_credential_payload(bundle_json).map_err(|e| format!("parse bundle: {e}"))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::fixtures::{sample_request, setup, NOW};
    use crate::credentials::{self, lifecycle};
    use crate::domain::vc::AcceptanceDecision;

    // ---- Shape-agnostic offline verification ---------------------------

    #[test]
    fn verify_offline_marks_a_status_bearing_bare_credential_pending() {
        // Reported as a bug: pasting a single credential into offline verify
        // failed with "missing field `format_version`", because only the
        // bundle shape was accepted. A credential signed by a `did:key` issuer
        // is verifiable on its own — the DID embeds the public key.
        let (db, issuer_key, _, subject) = setup();
        let vc = credentials::issue(db.conn(), &issuer_key, &sample_request(subject), NOW).unwrap();

        let json = serde_json::to_string(&vc).unwrap();
        let report = verify_offline(&json, NOW).unwrap();

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
        let (db, issuer_key, _, subject) = setup();
        credentials::issue(db.conn(), &issuer_key, &sample_request(subject), NOW).unwrap();
        let bundle = export(db.conn()).unwrap();

        let max = store::CREDENTIAL_PAYLOAD_JSON_LIMITS.max_bytes;
        let mut exact = bundle.clone();
        exact.push_str(&" ".repeat(max - bundle.len()));
        assert_eq!(verify_bundle_offline(&exact, NOW).unwrap(), (1, 1));
        exact.push(' ');
        let error = verify_bundle_offline(&exact, NOW).unwrap_err();
        assert!(error.contains("exceeds"), "{error}");

        let duplicated = format!("{{\"format_version\":\"forged\",{}", &bundle[1..]);
        let error = verify_offline(&duplicated, NOW).unwrap_err();
        assert!(error.contains("duplicate"), "{error}");

        // The bundle fits, but one credential in it exceeds what a direct
        // import of that credential would accept.
        let mut value: serde_json::Value = serde_json::from_str(&bundle).unwrap();
        value["credentials"][0]["credentialSubject"]["note"] = serde_json::Value::String(
            "a".repeat(alexandria_verify::vc::CREDENTIAL_JSON_LIMITS.max_string_bytes + 1),
        );
        let error = verify_offline(&value.to_string(), NOW).unwrap_err();
        assert!(error.contains("credential 0"), "{error}");
    }

    #[test]
    fn verify_offline_still_accepts_a_bundle_and_knows_the_difference() {
        let (db, issuer_key, _, subject) = setup();
        credentials::issue(db.conn(), &issuer_key, &sample_request(subject), NOW).unwrap();
        let bundle = export(db.conn()).unwrap();

        let report = verify_offline(&bundle, NOW).unwrap();
        assert_eq!(report.source, OfflineSource::Bundle);
        assert_eq!((report.accepted, report.total), (1, 1));
        // A bundle carries its own status lists, so revocation is conclusive.
        assert!(!report.revocation_unknown);
    }

    #[test]
    fn verify_offline_marks_status_bearing_credential_arrays_pending() {
        let (db, issuer_key, _, subject) = setup();
        let vc = credentials::issue(db.conn(), &issuer_key, &sample_request(subject), NOW).unwrap();

        let json = serde_json::to_string(&vec![vc.clone(), vc]).unwrap();
        let report = verify_offline(&json, NOW).unwrap();
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
        let (db, issuer_key, _, subject) = setup();
        let vc = credentials::issue(db.conn(), &issuer_key, &sample_request(subject), NOW).unwrap();

        let mut doc: serde_json::Value = serde_json::to_value(&vc).unwrap();
        doc["credentialSubject"]["level"] = serde_json::json!(99);
        let report = verify_offline(&doc.to_string(), NOW).unwrap();

        assert_eq!(report.accepted, 0, "a tampered credential must not verify");
        assert!(!report.results[0].valid_signature);
    }

    #[test]
    fn verify_offline_explains_unrecognised_json() {
        // The old message leaked the bundle parser's complaint about
        // `format_version`, which made no sense to someone holding a
        // credential.
        let err = verify_offline(r#"{"hello":"world"}"#, NOW).unwrap_err();
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

        let report = verify_offline(placeholder, NOW).unwrap();
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
        let (db, issuer_key, _, subject) = setup();
        let vc = credentials::issue(db.conn(), &issuer_key, &sample_request(subject), NOW).unwrap();
        let mut doc: serde_json::Value = serde_json::to_value(&vc).unwrap();
        doc["credentialSubject"]["level"] = serde_json::json!(99);

        let report = verify_offline(&doc.to_string(), NOW).unwrap();
        let reasons = rejection_reasons(&report.results[0]);
        assert!(report.results[0].issuer_resolved);
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("signature does not match")),
            "got: {reasons:?}"
        );
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
        let first =
            credentials::issue(db.conn(), &key, &sample_request(subject.clone()), NOW).unwrap();
        credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        lifecycle::revoke(
            db.conn(),
            &issuer,
            first.id.as_deref().unwrap(),
            "fixture",
            NOW,
        )
        .unwrap();
        let bundle = export_signed(db.conn(), Some(&key)).unwrap();
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
    fn a_signed_export_carries_the_status_list_as_a_bitstring_credential() {
        // The bundle a learner exports must verify with nothing from this
        // node: the list it names is inside it, as a credential signed by
        // the list's issuer, and a revocation set here reads as revoked
        // from that document alone.
        let (db, key, issuer, subject) = setup();
        let vc =
            credentials::issue(db.conn(), &key, &sample_request(subject.clone()), NOW).unwrap();
        let json = export_signed(db.conn(), Some(&key)).unwrap();
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
            &key.key().verifying_key(),
        )
        .unwrap();
        assert_eq!(bits.len() * 8, alexandria_verify::vc::status::MIN_BITS);
        let unsigned = export(db.conn()).unwrap();
        assert!(
            !unsigned.contains("status_list_credentials"),
            "no signer, no list credentials"
        );

        lifecycle::revoke(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();
        let json = export_signed(db.conn(), Some(&key)).unwrap();
        let mut bundle: CredentialBundle = serde_json::from_str(&json).unwrap();
        // Drop the raw rows: the signed list credential alone must carry the revocation.
        bundle.status_lists.clear();
        let json = serde_json_canonicalizer::to_string(&bundle).unwrap();
        let (accepted, total) = verify_bundle_offline(&json, NOW).unwrap();
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
        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();

        let accepted = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert_eq!(accepted.acceptance_decision, AcceptanceDecision::Accept);
        assert!(!accepted.revoked);

        lifecycle::revoke(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();

        let rejected = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(rejected.revoked, "revocation bit must propagate to verify");
        assert_eq!(rejected.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    fn export_bundle_is_deterministic_for_same_inputs() {
        // §20.4: same credential set + same fixed clock + same key
        // ⇒ byte-identical bundle. This is what lets the bundle
        // round-trip through content-addressed archival.
        let (db, key, _, subject) = setup();
        let _ = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        let a = export(db.conn()).unwrap();
        let b = export(db.conn()).unwrap();
        assert_eq!(a, b, "bundle MUST be byte-identical");
    }

    #[test]
    fn export_bundle_includes_credentials_and_status_lists() {
        let (db, key, issuer, subject) = setup();
        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        let json = export(db.conn()).unwrap();
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
        let (db, key, _, subject) = setup();
        let _ = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        let json = export(db.conn()).unwrap();
        let (accepted, total) = verify_bundle_offline(&json, NOW).unwrap();
        assert_eq!(total, 1);
        assert_eq!(accepted, 1, "round-tripped credential must verify");
    }

    #[test]
    fn offline_verifier_rejects_revoked_credential_in_bundle() {
        // The status list inside the bundle carries the revocation
        // bit, so the offline verifier sees the same Reject as the
        // local one.
        let (db, key, issuer, subject) = setup();
        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        lifecycle::revoke(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();
        let json = export(db.conn()).unwrap();
        let (accepted, total) = verify_bundle_offline(&json, NOW).unwrap();
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
            verify_bundle_offline(&bundle.to_string(), NOW).is_err(),
            "must reject unknown format_version"
        );
    }
}
