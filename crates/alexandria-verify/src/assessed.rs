//! The evidence entry naming the credential a role credential was issued on.
//!
//! An organisation (through its hosted service, or a sponsor on their own
//! device) issues a role credential on a candidate's assessment credential.
//! The candidate's device attestation evidence cannot travel with it: its
//! binding digest covers the assessment credential's own subject and claim,
//! so on the role credential it would verify as `binding_mismatch`. The role
//! credential instead carries one [`AssessedCredentialEvidence`] entry naming
//! the assessed credential by id and integrity hash, with the attestation
//! outcome and verified assurance level the issuer established when it
//! verified that credential. The issuer's signature is what vouches for it.
//!
//! The issuance policy rule judges a role credential's assurance from this
//! entry ([`crate::policy::evaluate_assessed`]).

use serde::{Deserialize, Serialize};

use crate::attestation::{AttestationOutcome, VerifiedAssuranceLevel};
use crate::vc::{EvidenceEntry, VcError, VerifiableCredential};

/// The `type` of an [`AssessedCredentialEvidence`] entry.
pub const ASSESSED_CREDENTIAL_EVIDENCE_TYPE: &str = "AssessedCredentialEvidence";

/// What an issuer established about the credential a role credential was
/// issued on. On the wire, one member of the credential's `evidence`:
///
/// ```json
/// {
///   "type": "AssessedCredentialEvidence",
///   "credentialId": "urn:uuid:…",
///   "integrityHash": "<hex BLAKE3 of the JCS-canonical credential without proofValue>",
///   "attestation": { "outcome": "verified_credential_bound", "platform": "ios" },
///   "verifiedAssuranceLevel": "device_attested"
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessedCredentialEvidence {
    /// Always [`ASSESSED_CREDENTIAL_EVIDENCE_TYPE`].
    #[serde(rename = "type")]
    pub type_: String,
    /// The assessed credential's envelope `id`; `null` when it has none.
    pub credential_id: Option<String>,
    /// The assessed credential's [`crate::vc::anchor::integrity_hash`].
    pub integrity_hash: String,
    /// What the issuer's verification established about the assessed
    /// credential's attestation evidence.
    pub attestation: AttestationOutcome,
    /// The verified assurance level that outcome establishes.
    pub verified_assurance_level: VerifiedAssuranceLevel,
}

/// Why a credential's [`AssessedCredentialEvidence`] cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssessedEvidenceError {
    #[error("more than one AssessedCredentialEvidence entry")]
    Duplicate,
    #[error("the AssessedCredentialEvidence entry does not parse: {0}")]
    Malformed(String),
}

impl AssessedCredentialEvidence {
    /// The entry naming `assessed`, with the `attestation` outcome the issuer
    /// established for it and the level that outcome verifies.
    pub fn new(
        assessed: &VerifiableCredential,
        attestation: AttestationOutcome,
    ) -> Result<Self, VcError> {
        Ok(Self {
            type_: ASSESSED_CREDENTIAL_EVIDENCE_TYPE.into(),
            credential_id: assessed.id.clone(),
            integrity_hash: crate::vc::anchor::integrity_hash(assessed)?,
            verified_assurance_level: attestation.verified_assurance_level(),
            attestation,
        })
    }

    /// The level a policy judges: the entry's stated level when its own
    /// outcome verifies it, else `local`. An entry stating `device_attested`
    /// beside an outcome that is not credential-bound states more than its
    /// own outcome shows.
    pub fn assurance_level(&self) -> VerifiedAssuranceLevel {
        if self.verified_assurance_level == self.attestation.verified_assurance_level() {
            self.verified_assurance_level
        } else {
            VerifiedAssuranceLevel::Local
        }
    }

    /// This entry as a member of a credential's `evidence`.
    pub fn into_entry(self) -> EvidenceEntry {
        match serde_json::to_value(self).expect("assessed credential evidence serializes") {
            serde_json::Value::Object(map) => EvidenceEntry(map),
            _ => unreachable!("assessed credential evidence serializes to an object"),
        }
    }

    /// The credential's entry: `Ok(None)` when it has none.
    pub fn find(credential: &VerifiableCredential) -> Result<Option<Self>, AssessedEvidenceError> {
        let mut entries = credential
            .evidence
            .iter()
            .filter(|e| e.has_type(ASSESSED_CREDENTIAL_EVIDENCE_TYPE));
        let Some(entry) = entries.next() else {
            return Ok(None);
        };
        if entries.next().is_some() {
            return Err(AssessedEvidenceError::Duplicate);
        }
        serde_json::from_value(serde_json::Value::Object(entry.0.clone()))
            .map(Some)
            .map_err(|e| AssessedEvidenceError::Malformed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::Platform;
    use crate::vc::{CredentialSubject, Proof};

    fn assessed() -> VerifiableCredential {
        VerifiableCredential {
            context: vec![crate::vc::context::W3C_VC_V2.into()],
            id: Some("urn:uuid:11111111-1111-4111-8111-111111111111".into()),
            type_: vec!["VerifiableCredential".into(), "AssessmentCredential".into()],
            issuer: crate::did::Did(
                "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK".into(),
            )
            .into(),
            valid_from: "2026-11-16T10:00:00Z".into(),
            valid_until: None,
            credential_subject: CredentialSubject {
                id: crate::did::Did(
                    "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK".into(),
                ),
                properties: Default::default(),
            },
            credential_status: None,
            terms_of_use: None,
            witness: None,
            integrity: None,
            evidence: Vec::new(),
            related_resource: Vec::new(),
            proof: Proof::unsigned("2026-11-16T10:00:00Z"),
        }
    }

    /// The wire shape is the one Alexandria Cloud has always emitted, so a
    /// role credential it signed reads back unchanged.
    #[test]
    fn the_entry_has_the_shape_cloud_emits() {
        let credential = assessed();
        let entry = AssessedCredentialEvidence::new(
            &credential,
            AttestationOutcome::VerifiedCredentialBound {
                platform: Platform::Ios,
            },
        )
        .unwrap()
        .into_entry();
        assert_eq!(
            serde_json::Value::Object(entry.0),
            serde_json::json!({
                "type": "AssessedCredentialEvidence",
                "credentialId": "urn:uuid:11111111-1111-4111-8111-111111111111",
                "integrityHash": crate::vc::anchor::integrity_hash(&credential).unwrap(),
                "attestation": { "outcome": "verified_credential_bound", "platform": "ios" },
                "verifiedAssuranceLevel": "device_attested",
            })
        );
    }

    #[test]
    fn a_credential_without_an_id_is_named_by_null() {
        let mut credential = assessed();
        credential.id = None;
        let entry = AssessedCredentialEvidence::new(&credential, AttestationOutcome::NotSupplied)
            .unwrap()
            .into_entry();
        assert_eq!(entry.0["credentialId"], serde_json::Value::Null);
        assert_eq!(entry.0["verifiedAssuranceLevel"], "local");
    }

    #[test]
    fn find_reads_one_entry_and_refuses_two() {
        let mut role = assessed();
        assert_eq!(AssessedCredentialEvidence::find(&role), Ok(None));
        let entry =
            AssessedCredentialEvidence::new(&assessed(), AttestationOutcome::NotSupplied).unwrap();
        role.evidence.push(entry.clone().into_entry());
        assert_eq!(
            AssessedCredentialEvidence::find(&role),
            Ok(Some(entry.clone()))
        );
        role.evidence.push(entry.into_entry());
        assert_eq!(
            AssessedCredentialEvidence::find(&role),
            Err(AssessedEvidenceError::Duplicate)
        );
    }

    /// A stated level the entry's own outcome does not verify is judged
    /// `local`.
    #[test]
    fn a_level_its_outcome_does_not_verify_is_local() {
        let mut entry = AssessedCredentialEvidence::new(
            &assessed(),
            AttestationOutcome::VerifiedSessionBound {
                platform: Platform::Ios,
            },
        )
        .unwrap();
        assert_eq!(entry.assurance_level(), VerifiedAssuranceLevel::Local);
        entry.verified_assurance_level = VerifiedAssuranceLevel::DeviceAttested;
        assert_eq!(entry.assurance_level(), VerifiedAssuranceLevel::Local);
    }
}
