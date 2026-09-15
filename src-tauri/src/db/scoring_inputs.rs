//! Verified scoring inputs.
//!
//! Derived skill states, reputation rows and talent-index projections may
//! score only credentials whose signed bytes verify now and whose signed
//! subject, skill and identifier match the indexed row. The scoring view, a
//! stored `revoked = 0`, or a readable JSON shape is not evidence on its own.
//!
//! Cached projections also record a fingerprint of the local state their
//! inputs depend on: the rows, their revocation and suspension flags, the
//! referenced status lists, issuer key history, supersession, completion
//! endorsements and the calculation versions. A read compares fingerprints so
//! a change, including removal of the last input, forces a recomputation
//! instead of serving a stale score.

use rusqlite::types::ValueRef;
use rusqlite::{params, Connection};

use alexandria_verify::trust::{classify_credential, CredentialTrust, TRUST_CALCULATION_VERSION};

use crate::commands::attestation::{stored_completion_evidence, StoredCompletionEvidence};
use crate::domain::vc::{
    verify_credential_db, SkillClaim, VerifiableCredential, VerificationPolicy,
};

const FINGERPRINT_DOMAIN: &[u8] = b"alexandria/scoring-input-fingerprint/v1";

/// A scoring input that verified at the requested time.
pub(crate) struct VerifiedSkillInput {
    pub(crate) credential_id: String,
    pub(crate) credential: VerifiableCredential,
    pub(crate) claim: SkillClaim,
}

impl VerifiedSkillInput {
    /// Whether the subject signed this claim about themselves. A self-issued
    /// input, endorsed or not, adds no issuer independence.
    pub(crate) fn self_issued(&self) -> bool {
        self.credential.issuer == self.credential.credential_subject.id
    }
}

/// Verified skill inputs for one subject and skill, oldest first.
///
/// Rows whose signed content does not match the index, that fail
/// verification, or whose verification is pending are excluded. Database
/// failures and corrupt stored completion evidence are errors.
pub(crate) fn verified_skill_inputs(
    conn: &Connection,
    subject_did: &str,
    skill_id: &str,
    verification_time: &str,
    network_id: &str,
) -> Result<Vec<VerifiedSkillInput>, String> {
    let rows = {
        let mut statement = conn
            .prepare(
                "SELECT id, signed_vc_json FROM scoring_credentials \
                 WHERE subject_did = ?1 AND skill_id = ?2 AND claim_kind = 'skill' \
                   AND revoked = 0 \
                 ORDER BY issuance_date, id",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![subject_did, skill_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        rows
    };

    let policy = VerificationPolicy::default();
    let mut inputs = Vec::with_capacity(rows.len());
    for (credential_id, signed_json) in rows {
        let Ok(credential) = serde_json::from_str::<VerifiableCredential>(&signed_json) else {
            continue;
        };
        if credential
            .id
            .as_deref()
            .is_some_and(|id| id != credential_id)
            || credential.credential_subject.id.as_str() != subject_did
        {
            continue;
        }
        let Some(claim) = SkillClaim::extract(&credential.credential_subject) else {
            continue;
        };
        if claim.skill_id != skill_id {
            continue;
        }
        let verification = verify_credential_db(conn, &credential, verification_time, &policy);
        let evidence = stored_completion_evidence(conn, &credential_id)?;
        let trust = classify_credential(
            &credential,
            &verification,
            &policy,
            network_id,
            evidence.as_ref().map(StoredCompletionEvidence::as_evidence),
        );
        if matches!(
            trust,
            CredentialTrust::Invalid { .. } | CredentialTrust::Pending { .. }
        ) {
            continue;
        }
        inputs.push(VerifiedSkillInput {
            credential_id,
            credential,
            claim,
        });
    }
    Ok(inputs)
}

/// Fingerprint of the local state that can change the verified inputs for
/// one subject and skill under `calculation_version`.
pub(crate) fn scoring_input_fingerprint(
    conn: &Connection,
    subject_did: &str,
    skill_id: &str,
    calculation_version: &str,
) -> Result<String, String> {
    let mut statement = conn
        .prepare(
            "SELECT c.id, c.signed_vc_json, c.integrity_hash, c.revoked, c.suspended, \
                    c.suspended_until, c.status_list_id, c.status_list_index, \
                    EXISTS(SELECT 1 FROM scoring_credentials s WHERE s.id = c.id), \
                    EXISTS(SELECT 1 FROM credentials n WHERE n.supersedes = c.id), \
                    (SELECT group_concat(entry, ';') FROM ( \
                        SELECT l.list_id || '|' || l.version || '|' || hex(l.bits) AS entry \
                        FROM credential_status_lists l \
                        WHERE l.list_id IN ( \
                            c.status_list_id, \
                            json_extract(c.signed_vc_json, '$.credentialStatus.statusListCredential'), \
                            json_extract(c.signed_vc_json, '$.credentialStatus.id') \
                        ) \
                        ORDER BY l.list_id)), \
                    (SELECT group_concat(entry, ';') FROM ( \
                        SELECT k.key_id || '|' || k.public_key_hex || '|' || k.valid_from \
                               || '|' || COALESCE(k.valid_until, '') AS entry \
                        FROM key_registry k WHERE k.did = c.issuer_did \
                        ORDER BY k.key_id)), \
                    (SELECT group_concat(entry, ';') FROM ( \
                        SELECT cc.id || '|' || e.id AS entry \
                        FROM completion_claims cc \
                        LEFT JOIN course_completion_endorsements e ON e.claim_id = cc.id \
                        WHERE EXISTS ( \
                            SELECT 1 FROM json_each(cc.credential_ids_json) \
                            WHERE json_each.value = c.id \
                        ) \
                        ORDER BY cc.id, e.id)) \
             FROM credentials c \
             WHERE c.subject_did = ?1 AND c.skill_id = ?2 \
             ORDER BY c.id",
        )
        .map_err(|error| error.to_string())?;
    let columns = statement.column_count();

    let mut hasher = blake3::Hasher::new();
    hasher.update(FINGERPRINT_DOMAIN);
    update_field(&mut hasher, calculation_version.as_bytes());
    update_field(&mut hasher, &TRUST_CALCULATION_VERSION.to_le_bytes());
    update_field(&mut hasher, subject_did.as_bytes());
    update_field(&mut hasher, skill_id.as_bytes());

    let mut rows = statement
        .query(params![subject_did, skill_id])
        .map_err(|error| error.to_string())?;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        for index in 0..columns {
            let value = row.get_ref(index).map_err(|error| error.to_string())?;
            update_value(&mut hasher, value);
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn update_field(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn update_value(hasher: &mut blake3::Hasher, value: ValueRef<'_>) {
    match value {
        ValueRef::Null => update_field(hasher, b"n"),
        ValueRef::Integer(integer) => {
            update_field(hasher, b"i");
            update_field(hasher, &integer.to_le_bytes());
        }
        ValueRef::Real(real) => {
            update_field(hasher, b"r");
            update_field(hasher, &real.to_le_bytes());
        }
        ValueRef::Text(text) => {
            update_field(hasher, b"t");
            update_field(hasher, text);
        }
        ValueRef::Blob(blob) => {
            update_field(hasher, b"b");
            update_field(hasher, blob);
        }
    }
}

#[cfg(test)]
mod tests {
    use alexandria_verify::did::derive_did_key;
    use alexandria_verify::vc::CredentialStatus;
    use ed25519_dalek::SigningKey;

    use super::*;
    use crate::db::opinion_eligibility::test_support::{
        store_credential, store_skill_credential, NOW,
    };
    use crate::db::Database;

    const VERSION: &str = "test-version";

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open database");
        db.run_migrations().expect("run migrations");
        db
    }

    fn input_ids(db: &Database, subject: &str) -> Vec<String> {
        verified_skill_inputs(db.conn(), subject, "skill", NOW, "preprod")
            .unwrap()
            .into_iter()
            .map(|input| input.credential_id)
            .collect()
    }

    #[test]
    fn only_verified_inputs_matching_signed_content_are_scored() {
        let db = test_db();
        let instructor = key(3);
        let learner_key = key(1);
        let learner = derive_did_key(&learner_key);
        store_skill_credential(&db, "issued", &instructor, &learner, "skill", 3);
        store_skill_credential(&db, "self", &learner_key, &learner, "skill", 4);
        store_skill_credential(&db, "tampered", &instructor, &learner, "skill", 2);
        db.conn()
            .execute(
                "UPDATE credentials SET signed_vc_json = \
                 json_set(signed_vc_json, '$.credentialSubject.level', 5) WHERE id = 'tampered'",
                [],
            )
            .unwrap();
        store_skill_credential(&db, "other-skill", &instructor, &learner, "other", 5);
        db.conn()
            .execute(
                "UPDATE credentials SET skill_id = 'skill' WHERE id = 'other-skill'",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO credentials (id, issuer_did, subject_did, credential_type, \
                 claim_kind, skill_id, issuance_date, signed_vc_json, integrity_hash, revoked) \
                 VALUES ('unsigned', 'did:key:zForged', ?1, 'FormalCredential', 'skill', \
                         'skill', '2026-01-01T00:00:00Z', \
                         '{\"credentialSubject\":{\"skillId\":\"skill\",\"level\":5}}', 'h', 0)",
                params![learner.as_str()],
            )
            .unwrap();
        store_credential(
            &db,
            "pending",
            &instructor,
            &learner,
            "skill",
            5,
            Some(CredentialStatus {
                id: "urn:status:1#0".into(),
                type_: "BitstringStatusListEntry".into(),
                status_purpose: "revocation".into(),
                status_list_index: "0".into(),
                status_list_credential: "urn:status:1".into(),
            }),
        );

        let inputs =
            verified_skill_inputs(db.conn(), learner.as_str(), "skill", NOW, "preprod").unwrap();
        let ids = inputs
            .iter()
            .map(|input| input.credential_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["issued", "self"]);
        assert!(!inputs[0].self_issued());
        assert!(inputs[1].self_issued());

        // Indexed under the learner but signed for someone else: excluded.
        let someone_else = derive_did_key(&key(9));
        store_skill_credential(&db, "misindexed", &instructor, &someone_else, "skill", 5);
        db.conn()
            .execute(
                "UPDATE credentials SET subject_did = ?1 WHERE id = 'misindexed'",
                params![learner.as_str()],
            )
            .unwrap();
        assert_eq!(input_ids(&db, learner.as_str()), vec!["issued", "self"]);
    }

    #[test]
    fn fingerprint_changes_with_inputs_status_and_version() {
        let db = test_db();
        let instructor = key(3);
        let learner = derive_did_key(&key(1));
        let fingerprint = |version: &str| {
            scoring_input_fingerprint(db.conn(), learner.as_str(), "skill", version).unwrap()
        };

        let empty = fingerprint(VERSION);
        store_skill_credential(&db, "issued", &instructor, &learner, "skill", 3);
        let one = fingerprint(VERSION);
        assert_ne!(empty, one);
        assert_eq!(one, fingerprint(VERSION), "unchanged state is stable");
        assert_ne!(one, fingerprint("another-version"));

        db.conn()
            .execute("UPDATE credentials SET revoked = 1 WHERE id = 'issued'", [])
            .unwrap();
        let revoked = fingerprint(VERSION);
        assert_ne!(one, revoked);

        db.conn()
            .execute(
                "UPDATE credentials SET revoked = 0, suspended = 1 WHERE id = 'issued'",
                [],
            )
            .unwrap();
        let suspended = fingerprint(VERSION);
        assert_ne!(one, suspended);
        assert_ne!(revoked, suspended);

        db.conn()
            .execute(
                "UPDATE credentials SET suspended = 0 WHERE id = 'issued'",
                [],
            )
            .unwrap();
        assert_eq!(one, fingerprint(VERSION));

        db.conn()
            .execute("DELETE FROM credentials WHERE id = 'issued'", [])
            .unwrap();
        assert_eq!(
            empty,
            fingerprint(VERSION),
            "removing the last input is visible"
        );
    }

    #[test]
    fn fingerprint_tracks_referenced_status_list_versions() {
        let db = test_db();
        let instructor = key(3);
        let learner = derive_did_key(&key(1));
        store_credential(
            &db,
            "listed",
            &instructor,
            &learner,
            "skill",
            3,
            Some(CredentialStatus {
                id: "urn:status:1#0".into(),
                type_: "BitstringStatusListEntry".into(),
                status_purpose: "revocation".into(),
                status_list_index: "0".into(),
                status_list_credential: "urn:status:1".into(),
            }),
        );
        let fingerprint =
            || scoring_input_fingerprint(db.conn(), learner.as_str(), "skill", VERSION).unwrap();
        let without_list = fingerprint();

        db.conn()
            .execute(
                "INSERT INTO credential_status_lists (list_id, issuer_did, version, bits, bit_length) \
                 VALUES ('urn:status:1', ?1, 1, X'00', 8)",
                params![derive_did_key(&instructor).as_str()],
            )
            .unwrap();
        let first_version = fingerprint();
        assert_ne!(without_list, first_version);

        db.conn()
            .execute(
                "UPDATE credential_status_lists SET version = 2, bits = X'01' \
                 WHERE list_id = 'urn:status:1'",
                [],
            )
            .unwrap();
        assert_ne!(first_version, fingerprint());
    }
}
