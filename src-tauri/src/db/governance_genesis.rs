use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::governance_certificate::{
    decode_and_verify_genesis, FoundingGenesisEnvelope, VerifiedFoundingGenesis,
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum PinGenesisError {
    #[error("invalid governance genesis: {0}")]
    Verification(String),
    #[error("a different genesis is already pinned under this DAO id")]
    ConflictingAnchor,
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
}

#[derive(Debug, Clone)]
pub(crate) struct PinnedGenesis {
    pub envelope: FoundingGenesisEnvelope,
    pub verified: VerifiedFoundingGenesis,
    pub newly_pinned: bool,
}

/// Persist an already-confirmed local trust decision.
///
/// Callers must present the exact canonical envelope bytes. This function has
/// no discovery or replacement path: the same bytes are idempotent, while a
/// different envelope under an existing derived id fails closed.
pub(crate) fn pin_genesis(
    conn: &Connection,
    canonical_bytes: &[u8],
) -> Result<PinnedGenesis, PinGenesisError> {
    let (envelope, verified) = decode_and_verify_genesis(canonical_bytes, None)
        .map_err(|error| PinGenesisError::Verification(error.to_string()))?;
    let existing = load_pinned_genesis(conn, &verified.dao_id)?;
    if let Some(existing) = existing {
        if existing != canonical_bytes {
            return Err(PinGenesisError::ConflictingAnchor);
        }
        return Ok(PinnedGenesis {
            envelope,
            verified,
            newly_pinned: false,
        });
    }

    conn.execute(
        "INSERT INTO governance_genesis_trust_anchors \
         (dao_id, genesis_hash, genesis_json, name, scope_type, scope_id, rules_hash) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            verified.dao_id,
            verified.genesis_hash,
            canonical_bytes,
            envelope.core.name,
            envelope.core.scope.scope_type,
            envelope.core.scope.scope_id,
            verified.rules_hash,
        ],
    )?;
    Ok(PinnedGenesis {
        envelope,
        verified,
        newly_pinned: true,
    })
}

pub(crate) fn load_pinned_genesis(
    conn: &Connection,
    dao_id: &str,
) -> Result<Option<Vec<u8>>, PinGenesisError> {
    let stored = conn
        .query_row(
            "SELECT genesis_json, genesis_hash, name, scope_type, scope_id, rules_hash \
         FROM governance_genesis_trust_anchors WHERE dao_id = ?1",
            [dao_id],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((bytes, genesis_hash, name, scope_type, scope_id, rules_hash)) = stored else {
        return Ok(None);
    };
    let (envelope, verified) = decode_and_verify_genesis(&bytes, Some(dao_id))
        .map_err(|error| PinGenesisError::Verification(error.to_string()))?;
    if verified.genesis_hash != genesis_hash
        || verified.rules_hash != rules_hash
        || envelope.core.name != name
        || envelope.core.scope.scope_type != scope_type
        || envelope.core.scope.scope_id != scope_id
    {
        return Err(PinGenesisError::Verification(
            "stored governance genesis metadata does not match its canonical envelope".into(),
        ));
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;
    use crate::db::Database;
    use crate::domain::governance_certificate::{
        FoundingAcceptance, FoundingGenesisCore, GenesisActivation, GenesisMember,
        GenesisQualificationPolicy, GenesisRules, GenesisScope, GOVERNANCE_CERTIFICATE_VERSION,
        GOVERNANCE_COMMITTEE_SIZE, GOVERNANCE_GENESIS_VERSION, GOVERNANCE_QUORUM,
    };

    fn signed_genesis(scope_id: &str) -> Vec<u8> {
        let governance_keys: Vec<_> = (1..=GOVERNANCE_COMMITTEE_SIZE)
            .map(|seed| SigningKey::from_bytes(&[seed as u8; 32]))
            .collect();
        let identity_keys: Vec<_> = (0..GOVERNANCE_COMMITTEE_SIZE)
            .map(|index| SigningKey::from_bytes(&[20 + index as u8; 32]))
            .collect();
        let consensus_keys: Vec<_> = (0..GOVERNANCE_COMMITTEE_SIZE)
            .map(|index| SigningKey::from_bytes(&[40 + index as u8; 32]))
            .collect();
        let core = FoundingGenesisCore {
            genesis_version: GOVERNANCE_GENESIS_VERSION,
            protocol_version: GOVERNANCE_CERTIFICATE_VERSION,
            name: "Shared display name".into(),
            scope: GenesisScope {
                scope_type: "subject".into(),
                scope_id: scope_id.into(),
            },
            members: governance_keys
                .iter()
                .enumerate()
                .map(|(index, key)| GenesisMember {
                    member_id: format!("member-{index}"),
                    identity_public_key_hex: hex::encode(
                        identity_keys[index].verifying_key().to_bytes(),
                    ),
                    consensus_public_key_hex: hex::encode(
                        consensus_keys[index].verifying_key().to_bytes(),
                    ),
                    governance_public_key_hex: hex::encode(key.verifying_key().to_bytes()),
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
        };
        let signing_bytes = core.acceptance_signing_bytes().unwrap();
        FoundingGenesisEnvelope {
            core,
            acceptances: governance_keys
                .iter()
                .enumerate()
                .map(|(index, key)| FoundingAcceptance {
                    member_id: format!("member-{index}"),
                    identity_signature_hex: hex::encode(
                        identity_keys[index].sign(&signing_bytes).to_bytes(),
                    ),
                    consensus_signature_hex: hex::encode(
                        consensus_keys[index].sign(&signing_bytes).to_bytes(),
                    ),
                    governance_signature_hex: hex::encode(key.sign(&signing_bytes).to_bytes()),
                })
                .collect(),
        }
        .canonical_bytes()
        .unwrap()
    }

    #[test]
    fn pinning_is_exact_and_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let bytes = signed_genesis("computer-science");
        let first = pin_genesis(db.conn(), &bytes).unwrap();
        assert!(first.newly_pinned);
        let second = pin_genesis(db.conn(), &bytes).unwrap();
        assert!(!second.newly_pinned);
        assert_eq!(first.verified.dao_id, second.verified.dao_id);
        assert_eq!(
            load_pinned_genesis(db.conn(), &first.verified.dao_id).unwrap(),
            Some(bytes)
        );
    }

    #[test]
    fn matching_names_do_not_substitute_for_genesis_identity() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let first = pin_genesis(db.conn(), &signed_genesis("computer-science")).unwrap();
        let second = pin_genesis(db.conn(), &signed_genesis("data-science")).unwrap();
        assert_ne!(first.verified.dao_id, second.verified.dao_id);
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM governance_genesis_trust_anchors \
                 WHERE name = 'Shared display name'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn noncanonical_bytes_are_never_pinned() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let canonical = signed_genesis("computer-science");
        let envelope: FoundingGenesisEnvelope = serde_json::from_slice(&canonical).unwrap();
        let pretty = serde_json::to_vec_pretty(&envelope).unwrap();
        assert!(matches!(
            pin_genesis(db.conn(), &pretty),
            Err(PinGenesisError::Verification(_))
        ));
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM governance_genesis_trust_anchors",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn corrupted_pinned_bytes_or_metadata_fail_closed_on_read() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let pinned = pin_genesis(db.conn(), &signed_genesis("computer-science")).unwrap();
        db.conn()
            .execute(
                "UPDATE governance_genesis_trust_anchors SET name = 'Substituted' \
                 WHERE dao_id = ?1",
                [&pinned.verified.dao_id],
            )
            .unwrap();
        assert!(matches!(
            load_pinned_genesis(db.conn(), &pinned.verified.dao_id),
            Err(PinGenesisError::Verification(_))
        ));

        db.conn()
            .execute(
                "UPDATE governance_genesis_trust_anchors SET name = 'Shared display name', \
                 genesis_json = x'7b7d' WHERE dao_id = ?1",
                [&pinned.verified.dao_id],
            )
            .unwrap();
        assert!(matches!(
            load_pinned_genesis(db.conn(), &pinned.verified.dao_id),
            Err(PinGenesisError::Verification(_))
        ));
    }
}
