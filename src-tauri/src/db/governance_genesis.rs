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
    /// The envelope stored for this DAO id. When `stored_envelope_differs` is
    /// true this is the earlier pin, not the envelope just presented.
    pub envelope: FoundingGenesisEnvelope,
    pub verified: VerifiedFoundingGenesis,
    pub newly_pinned: bool,
    /// A differently signed but valid envelope over the same core was already
    /// pinned. Its bytes were kept; the presented bytes were not stored.
    pub stored_envelope_differs: bool,
}

/// Persist an already-confirmed local trust decision.
///
/// Callers must present the exact canonical envelope bytes. The DAO id is the
/// hash of the genesis core, so two valid envelopes can share one id when a
/// founder re-signed the same core. This function never replaces a stored
/// anchor:
///
/// - identical bytes are idempotent;
/// - a different valid envelope over the same core keeps the stored bytes and
///   reports `stored_envelope_differs`, because both commit to the identical
///   DAO, keys and rules;
/// - anything else under an existing id fails closed.
///
/// The insert is a single `ON CONFLICT DO NOTHING` statement followed by a
/// re-read, so concurrent pins of one genesis all succeed and exactly one of
/// them reports `newly_pinned`.
pub(crate) fn pin_genesis(
    conn: &Connection,
    canonical_bytes: &[u8],
) -> Result<PinnedGenesis, PinGenesisError> {
    let (envelope, verified) = decode_and_verify_genesis(canonical_bytes, None)
        .map_err(|error| PinGenesisError::Verification(error.to_string()))?;

    let inserted = conn.execute(
        "INSERT INTO governance_genesis_trust_anchors \
         (dao_id, genesis_hash, genesis_json, name, scope_type, scope_id, rules_hash) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(dao_id) DO NOTHING",
        params![
            verified.dao_id(),
            verified.genesis_hash(),
            canonical_bytes,
            envelope.core.name,
            envelope.core.scope.scope_type,
            envelope.core.scope.scope_id,
            verified.rules_hash(),
        ],
    )?;
    if inserted == 1 {
        return Ok(PinnedGenesis {
            envelope,
            verified,
            newly_pinned: true,
            stored_envelope_differs: false,
        });
    }

    let existing =
        load_pinned_genesis(conn, verified.dao_id())?.ok_or(PinGenesisError::ConflictingAnchor)?;
    if existing == canonical_bytes {
        return Ok(PinnedGenesis {
            envelope,
            verified,
            newly_pinned: false,
            stored_envelope_differs: false,
        });
    }
    let (stored_envelope, stored_verified) =
        decode_and_verify_genesis(&existing, Some(verified.dao_id()))
            .map_err(|error| PinGenesisError::Verification(error.to_string()))?;
    if stored_envelope.core != envelope.core || stored_verified != verified {
        return Err(PinGenesisError::ConflictingAnchor);
    }
    Ok(PinnedGenesis {
        envelope: stored_envelope,
        verified: stored_verified,
        newly_pinned: false,
        stored_envelope_differs: true,
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
    if verified.genesis_hash() != genesis_hash
        || verified.rules_hash() != rules_hash
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
    use std::sync::{Arc, Barrier};

    use super::*;
    use crate::db::Database;
    use crate::domain::governance_certificate::test_support::{
        self, core, founder_keys, sign, signed_genesis,
    };

    /// A second valid Ed25519 signature by founder 2's governance key over
    /// the `computer-science` fixture core, made with a non-default nonce
    /// (`hash_prefix = [0x55; 32]`). The app crate does not enable
    /// `ed25519-dalek/hazmat`, so it is a fixed vector. It stays valid only
    /// while `test_support::core` is unchanged.
    const RESIGNED_GOVERNANCE_SIGNATURE: &str = "8f32007f700c1b5a648f27d3a658cc048357f061397a6ca28158cf96caebd2cb31a20f52bb7b80df680d87f363ce59c9030d316921f2b7a04cf70793c4f72202";

    fn migrated() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    fn anchor_count(db: &Database) -> i64 {
        db.conn()
            .query_row(
                "SELECT COUNT(*) FROM governance_genesis_trust_anchors",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn pinning_is_exact_and_idempotent() {
        let db = migrated();
        let bytes = signed_genesis("computer-science");
        let first = pin_genesis(db.conn(), &bytes).unwrap();
        assert!(first.newly_pinned);
        let second = pin_genesis(db.conn(), &bytes).unwrap();
        assert!(!second.newly_pinned);
        assert!(!second.stored_envelope_differs);
        assert_eq!(first.verified.dao_id(), second.verified.dao_id());
        assert_eq!(
            load_pinned_genesis(db.conn(), first.verified.dao_id()).unwrap(),
            Some(bytes)
        );
    }

    #[test]
    fn a_resigned_envelope_for_a_pinned_core_keeps_the_stored_bytes() {
        let db = migrated();
        let original = sign(core("Shared display name", "computer-science"));
        let original_bytes = original.canonical_bytes().unwrap();
        let first = pin_genesis(db.conn(), &original_bytes).unwrap();

        let keys = founder_keys();
        let mut resigned = original.clone();
        assert_eq!(
            resigned.acceptances[2].member_id,
            test_support::member_id(&keys[2])
        );
        resigned.acceptances[2].governance_signature_hex = RESIGNED_GOVERNANCE_SIGNATURE.into();
        let resigned_bytes = resigned.canonical_bytes().unwrap();
        assert_ne!(resigned_bytes, original_bytes);

        let second = pin_genesis(db.conn(), &resigned_bytes).unwrap();
        assert!(!second.newly_pinned);
        assert!(second.stored_envelope_differs);
        assert_eq!(second.verified.dao_id(), first.verified.dao_id());
        assert_eq!(second.envelope, original);
        assert_eq!(anchor_count(&db), 1);
        assert_eq!(
            load_pinned_genesis(db.conn(), first.verified.dao_id()).unwrap(),
            Some(original_bytes)
        );
    }

    #[test]
    fn concurrent_pins_of_one_genesis_are_idempotent() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("pins.sqlite");
        {
            let db = Database::open(&path).unwrap();
            db.run_migrations().unwrap();
        }
        let bytes = Arc::new(signed_genesis("computer-science"));
        let pinners = 6;
        let barrier = Arc::new(Barrier::new(pinners));
        let handles: Vec<_> = (0..pinners)
            .map(|_| {
                let path = path.clone();
                let bytes = bytes.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let db = Database::open(&path).unwrap();
                    db.conn()
                        .busy_timeout(std::time::Duration::from_secs(10))
                        .unwrap();
                    barrier.wait();
                    pin_genesis(db.conn(), &bytes).map(|pinned| pinned.newly_pinned)
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert!(
            results.iter().all(Result::is_ok),
            "concurrent pin failed: {results:?}"
        );
        let newly_pinned = results
            .iter()
            .filter(|result| matches!(result, Ok(true)))
            .count();
        assert_eq!(newly_pinned, 1);
        let db = Database::open(&path).unwrap();
        assert_eq!(anchor_count(&db), 1);
    }

    #[test]
    fn matching_names_do_not_substitute_for_genesis_identity() {
        let db = migrated();
        let first = pin_genesis(db.conn(), &signed_genesis("computer-science")).unwrap();
        let second = pin_genesis(db.conn(), &signed_genesis("data-science")).unwrap();
        assert_ne!(first.verified.dao_id(), second.verified.dao_id());
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
        let db = migrated();
        let canonical = signed_genesis("computer-science");
        let envelope: FoundingGenesisEnvelope = serde_json::from_slice(&canonical).unwrap();
        let pretty = serde_json::to_vec_pretty(&envelope).unwrap();
        assert!(matches!(
            pin_genesis(db.conn(), &pretty),
            Err(PinGenesisError::Verification(_))
        ));
        assert_eq!(anchor_count(&db), 0);
    }

    #[test]
    fn corrupted_pinned_bytes_or_metadata_fail_closed_on_read() {
        let db = migrated();
        let pinned = pin_genesis(db.conn(), &signed_genesis("computer-science")).unwrap();
        let dao_id = pinned.verified.dao_id().to_owned();
        db.conn()
            .execute(
                "UPDATE governance_genesis_trust_anchors SET name = 'Substituted' \
                 WHERE dao_id = ?1",
                [&dao_id],
            )
            .unwrap();
        assert!(matches!(
            load_pinned_genesis(db.conn(), &dao_id),
            Err(PinGenesisError::Verification(_))
        ));
        assert!(pin_genesis(db.conn(), &signed_genesis("computer-science")).is_err());

        db.conn()
            .execute(
                "UPDATE governance_genesis_trust_anchors SET name = 'Shared display name', \
                 genesis_json = x'7b7d' WHERE dao_id = ?1",
                [&dao_id],
            )
            .unwrap();
        assert!(matches!(
            load_pinned_genesis(db.conn(), &dao_id),
            Err(PinGenesisError::Verification(_))
        ));
    }
}
