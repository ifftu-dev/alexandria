//! Attestation evidence at issuance: what device attestation a credential
//! carries in its `evidence`, and what it is bound to.
//!
//! A credential bound to an integrity session whose stored attestation is
//! acceptable (it verifies, or no trust input on this build can check it)
//! carries that attestation as one `DeviceAttestationEvidence` entry
//! ([`alexandria_verify::attestation::AttestationEvidence`]). Before signing,
//! issuance asks the platform, through a [`BindingAttester`], for evidence
//! over the credential's binding digest: then the entry is *credential-bound*.
//! When that cannot be obtained, or does not check out, the entry is
//! *session-bound* and the result says why. Evidence is checked with the
//! verifier library before it is embedded, so issuance never signs evidence
//! that a verifier would reject, and a failure to obtain it never fails
//! issuance. The binding is fixed when the credential is signed; it is never
//! upgraded later.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use alexandria_verify::attestation::{
    binding_digest, verify_evidence, AttestationEvidence, AttestationOutcome, DeviceAttestation,
};

use super::CredentialError;
use crate::domain::vc::VerifiableCredential;
use crate::sentinel::attestation::{BindingAttester, NativeAttester, TrustConfig};

/// Where issuance gets attestation evidence: the trust inputs it checks
/// evidence against and the attester that produces evidence over a binding
/// digest.
pub struct EvidenceSource<'a> {
    pub trust: TrustConfig,
    pub attester: &'a dyn BindingAttester,
}

impl EvidenceSource<'static> {
    /// This build's trust inputs and this device's own platform. A paired
    /// phone's session issues session-bound evidence through this source;
    /// grading asks the phone instead.
    pub fn native() -> Self {
        Self {
            trust: TrustConfig::from_profile(),
            attester: &NativeAttester,
        }
    }
}

/// What attestation evidence an issued credential carries, for the issuance
/// result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "binding", rename_all = "snake_case")]
pub enum EvidenceBinding {
    /// No attestation evidence: no session, an unattested session, or a
    /// session attestation that does not verify.
    None,
    /// Evidence over this credential's subject, claim and integrity
    /// assertion.
    Credential,
    /// Only the session's own evidence, and why the credential could not be
    /// bound.
    Session { reason: String },
}

/// Put the attestation evidence of `vc`'s integrity session into its
/// `evidence`, credential-bound when the attester supplies a proof that
/// verifies, else session-bound. Call after the integrity assertion is in
/// place and before signing. Only database errors fail.
pub(crate) fn attach(
    conn: &Connection,
    vc: &mut VerifiableCredential,
    source: &EvidenceSource<'_>,
) -> Result<EvidenceBinding, CredentialError> {
    let Some(session_id) = vc.integrity.as_ref().map(|a| a.session_id.clone()) else {
        return Ok(EvidenceBinding::None);
    };
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT started_at, attestation_json FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((started_at, Some(json))) = row else {
        return Ok(EvidenceBinding::None);
    };
    let record: DeviceAttestation = match serde_json::from_str(&json) {
        Ok(record) => record,
        Err(e) => {
            log::warn!(target: "sentinel", "session {session_id}: stored attestation unreadable: {e}");
            return Ok(EvidenceBinding::None);
        }
    };
    let trust = source.trust.inputs();
    let check = |vc: &mut VerifiableCredential, evidence: AttestationEvidence| {
        vc.evidence = vec![evidence.into_entry()];
        let outcome = verify_evidence(vc, &trust);
        if outcome.rejects() {
            vc.evidence.clear();
        }
        outcome
    };

    let session = AttestationEvidence::session_bound(&record, &started_at);
    if let AttestationOutcome::Invalid { reason } = check(vc, session.clone()) {
        log::warn!(
            target: "sentinel",
            "session {session_id}: attestation not carried: {}",
            reason.as_str()
        );
        return Ok(EvidenceBinding::None);
    }
    let reason = match binding_digest(vc) {
        Err(e) => format!("the binding digest could not be computed: {e}"),
        Ok(digest) => match source.attester.bind(&session_id, &record, &digest) {
            Err(why) => why,
            Ok(proof) => {
                let bound = AttestationEvidence::credential_bound(&record, &started_at, proof);
                match check(vc, bound) {
                    AttestationOutcome::Invalid { reason } => format!(
                        "the evidence over this credential did not verify ({})",
                        reason.as_str()
                    ),
                    _ => return Ok(EvidenceBinding::Credential),
                }
            }
        },
    };
    log::info!(target: "sentinel", "session {session_id}: evidence is session-bound: {reason}");
    check(vc, session);
    Ok(EvidenceBinding::Session { reason })
}

/// What an issuance pass asked its attester for: evidence over `digest`
/// from `session_id`'s attested key.
#[derive(Debug, Clone)]
pub struct BindRequest {
    pub session_id: String,
    pub record: DeviceAttestation,
    pub digest: [u8; 32],
}

/// An attester that answers nothing and remembers what it was asked, for a
/// rehearsal pass rolled back before any platform call is made.
#[derive(Default)]
pub struct RecordingAttester(std::sync::Mutex<Option<BindRequest>>);

impl RecordingAttester {
    /// The request the rehearsal made, if it reached the attester.
    pub fn take(self) -> Option<BindRequest> {
        self.0.into_inner().ok().flatten()
    }
}

impl BindingAttester for RecordingAttester {
    fn bind(
        &self,
        session_id: &str,
        record: &DeviceAttestation,
        digest: &[u8; 32],
    ) -> Result<String, String> {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(BindRequest {
                session_id: session_id.to_string(),
                record: record.clone(),
                digest: *digest,
            });
        }
        Err("evidence is obtained after the rehearsal".into())
    }
}

/// Evidence obtained before the issuing transaction opened, answered to the
/// one digest it was obtained for.
///
/// Issuance that waits on a platform (a paired phone answers in seconds)
/// runs in two passes: a rehearsal in a transaction that is rolled back,
/// with a [`RecordingAttester`], learns the binding digest; the platform is
/// asked with no transaction open; the committed pass then answers from this.
/// A committed pass whose digest differs (the credential changed in between)
/// falls back to session-bound evidence with that reason.
#[derive(Debug, Clone)]
pub struct PreparedEvidence {
    obtained: Option<([u8; 32], Result<String, String>)>,
}

impl PreparedEvidence {
    /// Ask `attester` for what `request` names, if anything. Call with no
    /// transaction open: this is the wait.
    pub fn obtain(request: Option<BindRequest>, attester: &dyn BindingAttester) -> Self {
        Self {
            obtained: request.map(|request| {
                let answer = attester.bind(&request.session_id, &request.record, &request.digest);
                (request.digest, answer)
            }),
        }
    }
}

impl BindingAttester for PreparedEvidence {
    fn bind(
        &self,
        _session_id: &str,
        _record: &DeviceAttestation,
        digest: &[u8; 32],
    ) -> Result<String, String> {
        match &self.obtained {
            Some((asked, answer)) if asked == digest => answer.clone(),
            Some(_) => Err("the credential changed while its evidence was obtained".into()),
            None => Err("no evidence was obtained before issuance".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::fixtures::{sample_request, setup, NOW};
    use crate::credentials::issue::issue_with;
    use crate::credentials::{Issued, Issuer};
    use crate::crypto::did::Did;
    use crate::sentinel::attestation::{
        apple_app_id, session_nonce, AttestationPayload, BindingAttester, DeviceAttestation,
        Platform, TrustConfig,
    };
    use alexandria_verify::attestation::test_support::{
        b64, build_assertion, build_world, World, AAGUID_PROD,
    };
    use alexandria_verify::attestation::{verify_evidence, AttestationOutcome};
    use rusqlite::params;

    const SESSION: &str = "isess_attested";
    const STARTED: &str = "2026-04-12T23:00:00Z";
    const ROOT: &str = "c0ffee";

    /// A finished session on an iPhone whose App Attest key attested the
    /// session nonce and signed the terminal root (counter 1).
    fn attested_session(conn: &rusqlite::Connection) -> World {
        let w = build_world(
            &apple_app_id(),
            session_nonce(SESSION, STARTED),
            AAGUID_PROD,
            0,
        );
        let record = DeviceAttestation {
            platform: Platform::Ios,
            nonce_hex: hex::encode(session_nonce(SESSION, STARTED)),
            app_id: apple_app_id(),
            payload: AttestationPayload::AppAttest {
                key_id_b64: w.key_id_b64(),
                attestation_b64: w.attestation_b64(),
                assertion_b64: Some(b64(&build_assertion(&w, ROOT.as_bytes(), 1))),
                asserted_root: Some(ROOT.into()),
            },
            captured_at: STARTED.into(),
            companion: None,
        };
        conn.execute(
            "INSERT INTO integrity_sessions
                 (id, status, assurance_level, attestation_json, commitment_root,
                  started_at, ended_at)
             VALUES (?1, 'completed', 'device_attested', ?2, ?3, ?4, ?5)",
            params![
                SESSION,
                serde_json::to_string(&record).unwrap(),
                ROOT,
                STARTED,
                NOW
            ],
        )
        .unwrap();
        w
    }

    fn trust(w: &World) -> TrustConfig {
        TrustConfig {
            apple_root_pem: w.root_pem.clone(),
            ..TrustConfig::default()
        }
    }

    /// The session's attested key, signing whatever digest issuance asks
    /// for the way `generateAssertion` would (the next counter value).
    struct FakePhone<'a>(&'a World);

    impl BindingAttester for FakePhone<'_> {
        fn bind(
            &self,
            session_id: &str,
            _record: &DeviceAttestation,
            digest: &[u8; 32],
        ) -> Result<String, String> {
            assert_eq!(session_id, SESSION);
            Ok(b64(&build_assertion(self.0, digest, 2)))
        }
    }

    fn issue_on_session(
        (db, key, subject): (&crate::db::Database, &Issuer, &Did),
        attester: &dyn BindingAttester,
        trust: TrustConfig,
    ) -> Issued {
        let mut req = sample_request(subject.clone());
        req.integrity_session_id = Some(SESSION.into());
        issue_with(
            db.conn(),
            key,
            &req,
            NOW,
            &EvidenceSource { trust, attester },
        )
        .unwrap()
    }

    #[test]
    fn an_attested_session_issues_a_credential_bound_to_its_evidence() {
        let (db, key, _, subject) = setup();
        let w = attested_session(db.conn());
        let issued = issue_on_session((&db, &key, &subject), &FakePhone(&w), trust(&w));
        assert_eq!(issued.evidence, EvidenceBinding::Credential);
        assert_eq!(
            verify_evidence(&issued.credential, &trust(&w).inputs()),
            AttestationOutcome::VerifiedCredentialBound {
                platform: Platform::Ios
            }
        );
    }

    /// The platform call fails: the phone has gone, or the platform refused.
    struct Unavailable;

    impl BindingAttester for Unavailable {
        fn bind(&self, _: &str, _: &DeviceAttestation, _: &[u8; 32]) -> Result<String, String> {
            Err("the paired phone is no longer linked to this session".into())
        }
    }

    #[test]
    fn without_evidence_over_the_credential_the_session_evidence_is_carried_session_bound() {
        let (db, key, _, subject) = setup();
        let w = attested_session(db.conn());
        let issued = issue_on_session((&db, &key, &subject), &Unavailable, trust(&w));
        assert_eq!(
            issued.evidence,
            EvidenceBinding::Session {
                reason: "the paired phone is no longer linked to this session".into()
            }
        );
        assert_eq!(
            verify_evidence(&issued.credential, &trust(&w).inputs()),
            AttestationOutcome::VerifiedSessionBound {
                platform: Platform::Ios
            }
        );
        // The signed credential, as stored, still verifies.
        let verified = crate::domain::vc::verify_credential_db(
            db.conn(),
            &issued.credential,
            NOW,
            &crate::domain::vc::VerificationPolicy::default(),
        );
        assert!(verified.valid_signature);
    }

    /// A phone whose proof covers something other than this credential.
    struct WrongDigest<'a>(&'a World);

    impl BindingAttester for WrongDigest<'_> {
        fn bind(&self, _: &str, _: &DeviceAttestation, _: &[u8; 32]) -> Result<String, String> {
            Ok(b64(&build_assertion(self.0, &[0u8; 32], 2)))
        }
    }

    #[test]
    fn a_proof_that_does_not_verify_is_never_signed_and_falls_back_to_the_session() {
        let (db, key, _, subject) = setup();
        let w = attested_session(db.conn());
        let issued = issue_on_session((&db, &key, &subject), &WrongDigest(&w), trust(&w));
        assert_eq!(
            issued.evidence,
            EvidenceBinding::Session {
                reason: "the evidence over this credential did not verify (binding_mismatch)"
                    .into()
            }
        );
        assert_eq!(
            verify_evidence(&issued.credential, &trust(&w).inputs()),
            AttestationOutcome::VerifiedSessionBound {
                platform: Platform::Ios
            }
        );
    }

    /// A role policy requiring `device_attested` is judged on the evidence
    /// issuance attaches: credential-bound evidence from the phone meets it,
    /// session-bound evidence does not, and the refusal mints nothing.
    #[test]
    fn a_device_attested_policy_is_met_only_by_credential_bound_evidence() {
        use crate::credentials::{CredentialError, IssuancePolicy, PolicyRefusal};
        let issue_under_policy = |phone_present: bool| {
            let (db, key, _, subject) = setup();
            let w = attested_session(db.conn());
            let mut req = sample_request(subject);
            req.integrity_session_id = Some(SESSION.into());
            req.integrity_policy = Some(IssuancePolicy {
                required_assurance_level: Some("device_attested".into()),
                ..Default::default()
            });
            let phone = FakePhone(&w);
            let attester: &dyn BindingAttester = if phone_present { &phone } else { &Unavailable };
            let source = EvidenceSource {
                trust: trust(&w),
                attester,
            };
            let result = issue_with(db.conn(), &key, &req, NOW, &source);
            let stored: i64 = db
                .conn()
                .query_row("SELECT COUNT(*) FROM credentials", [], |r| r.get(0))
                .unwrap();
            (result, stored)
        };

        let (bound, stored) = issue_under_policy(true);
        let bound = bound.expect("credential-bound evidence meets device_attested");
        assert_eq!(bound.evidence, EvidenceBinding::Credential);
        assert_eq!(stored, 1);

        let (refused, stored) = issue_under_policy(false);
        assert!(
            matches!(
                &refused,
                Err(CredentialError::Policy(PolicyRefusal::AssuranceBelowRequired { level, required }))
                    if level == "local" && required == "device_attested"
            ),
            "{refused:?}"
        );
        assert_eq!(stored, 0, "a refused credential is never signed or stored");
    }

    #[test]
    fn an_unattested_session_issues_a_credential_with_no_attestation_evidence() {
        let (db, key, _, subject) = setup();
        db.conn()
            .execute(
                "INSERT INTO integrity_sessions (id, status, started_at, ended_at)
                 VALUES (?1, 'completed', ?2, ?3)",
                params![SESSION, STARTED, NOW],
            )
            .unwrap();
        let w = build_world(&apple_app_id(), [0; 32], AAGUID_PROD, 0);
        let issued = issue_on_session((&db, &key, &subject), &FakePhone(&w), trust(&w));
        assert_eq!(issued.evidence, EvidenceBinding::None);
        assert!(issued.credential.evidence.is_empty());
        assert_eq!(
            verify_evidence(&issued.credential, &trust(&w).inputs()),
            AttestationOutcome::NotSupplied
        );
    }

    #[test]
    fn a_session_attestation_that_does_not_verify_is_not_carried() {
        let (db, key, _, subject) = setup();
        let w = attested_session(db.conn());
        // Against the real Apple root the synthetic chain is untrusted: the
        // credential carries nothing a verifier would reject.
        let issued = issue_on_session(
            (&db, &key, &subject),
            &FakePhone(&w),
            TrustConfig::default(),
        );
        assert_eq!(issued.evidence, EvidenceBinding::None);
        assert!(issued.credential.evidence.is_empty());
    }

    #[test]
    fn an_exported_bundle_carries_the_evidence_as_part_of_the_credential() {
        let (db, key, _, subject) = setup();
        let w = attested_session(db.conn());
        let issued = issue_on_session((&db, &key, &subject), &FakePhone(&w), trust(&w));
        let bundle: serde_json::Value =
            serde_json::from_str(&crate::credentials::bundle::export(db.conn()).unwrap()).unwrap();
        let bundled: crate::domain::vc::VerifiableCredential =
            serde_json::from_value(bundle["credentials"][0].clone()).unwrap();
        assert_eq!(bundled.evidence, issued.credential.evidence);
        assert_eq!(
            verify_evidence(&bundled, &trust(&w).inputs()),
            AttestationOutcome::VerifiedCredentialBound {
                platform: Platform::Ios
            }
        );
    }

    /// Real platform material is larger than the synthetic chains. With
    /// generous upper bounds (an App Attest attestation object with its
    /// receipt well under 12 KiB, Play Integrity tokens under 8 KiB each) a
    /// credential still decodes under the untrusted-credential limits, so
    /// the limits stay as they are.
    #[test]
    fn evidence_at_platform_sizes_fits_the_credential_limits() {
        use alexandria_verify::vc::{decode_credential, CREDENTIAL_JSON_LIMITS};
        let (db, key, _, subject) = setup();
        let w = attested_session(db.conn());
        let issued = issue_on_session((&db, &key, &subject), &FakePhone(&w), trust(&w));
        let base64_of = |raw_bytes: usize| "A".repeat(raw_bytes.div_ceil(3) * 4);
        let mut json = serde_json::to_value(&issued.credential).unwrap();
        let ios = json["evidence"][0]["material"].as_object_mut().unwrap();
        ios.insert("attestation".into(), base64_of(12 * 1024).into());
        ios.insert("sessionAssertion".into(), base64_of(1024).into());
        ios.insert("bindingAssertion".into(), base64_of(1024).into());
        let mut android = json.clone();
        android["evidence"][0]["material"] = serde_json::json!({
            "kind": "play_integrity",
            "sessionToken": "A".repeat(8 * 1024),
            "bindingToken": "A".repeat(8 * 1024),
        });
        for credential in [json, android] {
            let bytes = serde_json::to_vec(&credential).unwrap();
            assert!(bytes.len() < CREDENTIAL_JSON_LIMITS.max_bytes / 4);
            assert!(decode_credential(&bytes).is_ok());
        }
    }
}
