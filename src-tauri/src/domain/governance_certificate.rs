//! App-facing re-exports for the I/O-free governance certificate verifier.

pub use alexandria_verify::governance::*;

/// Deterministic, fully signed founding genesis fixtures for app tests.
#[cfg(test)]
pub(crate) mod test_support {
    use alexandria_verify::did::did_from_verifying_key;
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    pub(crate) struct FounderKeys {
        pub identity: SigningKey,
        pub consensus: SigningKey,
        pub governance: SigningKey,
    }

    pub(crate) fn founder_keys() -> Vec<FounderKeys> {
        let mut keys: Vec<_> = (0..GOVERNANCE_COMMITTEE_SIZE)
            .map(|index| FounderKeys {
                identity: SigningKey::from_bytes(&[20 + index as u8; 32]),
                consensus: SigningKey::from_bytes(&[40 + index as u8; 32]),
                governance: SigningKey::from_bytes(&[1 + index as u8; 32]),
            })
            .collect();
        keys.sort_by_key(member_id);
        keys
    }

    pub(crate) fn member_id(keys: &FounderKeys) -> String {
        did_from_verifying_key(&keys.identity.verifying_key()).0
    }

    pub(crate) fn core(name: &str, scope_id: &str) -> FoundingGenesisCore {
        FoundingGenesisCore {
            genesis_version: GOVERNANCE_GENESIS_VERSION,
            protocol_version: GOVERNANCE_CERTIFICATE_VERSION,
            name: name.into(),
            scope: GenesisScope {
                scope_type: "subject".into(),
                scope_id: scope_id.into(),
            },
            members: founder_keys()
                .iter()
                .map(|keys| GenesisMember {
                    member_id: member_id(keys),
                    identity_public_key_hex: hex::encode(keys.identity.verifying_key().to_bytes()),
                    consensus_public_key_hex: hex::encode(
                        keys.consensus.verifying_key().to_bytes(),
                    ),
                    governance_public_key_hex: hex::encode(
                        keys.governance.verifying_key().to_bytes(),
                    ),
                })
                .collect(),
            rules: GenesisRules {
                rules_version: "1".into(),
                committee_size: GOVERNANCE_COMMITTEE_SIZE as u8,
                receipt_threshold: GOVERNANCE_QUORUM as u8,
                outcome_threshold: GOVERNANCE_QUORUM as u8,
                proposal_approval_numerator: 2,
                proposal_approval_denominator: 3,
                minimum_turnout_count: 5,
            },
            qualification_policy: GenesisQualificationPolicy {
                policy_version: "1".into(),
                accepted_issuers: vec!["did:key:issuer".into()],
                accepted_assessment_evidence: vec!["assessment-credential".into()],
            },
            activation: GenesisActivation {
                cometbft_chain_id: format!("alexandria-{scope_id}"),
                initial_epoch: 0,
                initial_height: 1,
                activation_time_unix: 1_800_000_000,
            },
        }
    }

    pub(crate) fn sign(core: FoundingGenesisCore) -> FoundingGenesisEnvelope {
        let signing_bytes = core.acceptance_signing_bytes().unwrap();
        let acceptances = founder_keys()
            .iter()
            .map(|keys| FoundingAcceptance {
                member_id: member_id(keys),
                identity_signature_hex: hex::encode(keys.identity.sign(&signing_bytes).to_bytes()),
                consensus_signature_hex: hex::encode(
                    keys.consensus.sign(&signing_bytes).to_bytes(),
                ),
                governance_signature_hex: hex::encode(
                    keys.governance.sign(&signing_bytes).to_bytes(),
                ),
            })
            .collect();
        FoundingGenesisEnvelope { core, acceptances }
    }

    /// Canonical bytes of a fully signed genesis for `scope_id`.
    pub(crate) fn signed_genesis(scope_id: &str) -> Vec<u8> {
        sign(core("Shared display name", scope_id))
            .canonical_bytes()
            .unwrap()
    }
}
