//! Changing a credential's standing after it was issued: revoking it
//! (permanent, through its revocation list), suspending it and reinstating
//! it (reversible, through its suspension list). Only the credential's
//! issuer may do any of them.

use rusqlite::{params, Connection, OptionalExtension};

use super::{CredentialError, StatusChange};
use crate::crypto::did::Did;

/// Flip the revocation bit in the issuer's status list and mark the
/// local `credentials` row as revoked. Idempotent — calling it twice
/// leaves the bit set and the row flagged.
pub fn revoke(
    conn: &Connection,
    issuer_did: &Did,
    credential_id: &str,
    reason: &str,
    now: &str,
) -> Result<(), CredentialError> {
    let transaction = conn.unchecked_transaction()?;
    let row: Option<(String, String, i64, String)> = transaction
        .query_row(
            "SELECT c.issuer_did, c.status_list_id, c.status_list_index, s.issuer_did \
             FROM credentials c \
             JOIN credential_status_lists s ON s.list_id = c.status_list_id \
             WHERE c.id = ?1",
            params![credential_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get(3)?,
                ))
            },
        )
        .optional()?
        .and_then(
            |(credential_issuer, list_id, index, list_issuer)| match (list_id, index) {
                (Some(list_id), Some(index)) => {
                    Some((credential_issuer, list_id, index, list_issuer))
                }
                _ => None,
            },
        );

    let (credential_issuer, list_id, index, list_issuer) =
        row.ok_or_else(|| CredentialError::NotRevocable(credential_id.to_string()))?;
    if credential_issuer != issuer_did.as_str() || list_issuer != issuer_did.as_str() {
        return Err(CredentialError::NotIssuer(StatusChange::Revoke));
    }

    // Read current bits, set the revocation bit, write back + bump version.
    let mut bits: Vec<u8> = transaction
        .query_row(
            "SELECT bits FROM credential_status_lists WHERE list_id = ?1",
            params![list_id],
            |r| r.get(0),
        )
        .map_err(CredentialError::step("load status list"))?;
    // Bitstring Status List: index 0 is the most significant bit of byte 0.
    alexandria_verify::vc::status::set_bit(&mut bits, index as usize, true)
        .map_err(CredentialError::StatusBit)?;

    transaction
        .execute(
            "UPDATE credential_status_lists \
         SET bits = ?2, version = version + 1, updated_at = ?3 \
         WHERE list_id = ?1 AND issuer_did = ?4",
            params![list_id, bits, now, issuer_did.as_str()],
        )
        .map_err(CredentialError::step("update status list"))?;

    transaction
        .execute(
            "UPDATE credentials \
         SET revoked = 1, revoked_at = ?2, revocation_reason = ?3 \
         WHERE id = ?1 AND issuer_did = ?4",
            params![credential_id, now, reason, issuer_did.as_str()],
        )
        .map_err(CredentialError::step("update credential"))?;

    transaction.commit()?;
    Ok(())
}

/// §11.3 suspension: set the credential's bit in its suspension list, so
/// every verifier that reads the list sees it, and record the issuer's own
/// bookkeeping (`suspended_until`, reason) beside it. Idempotent —
/// re-suspending updates the until window; a timed suspension is lifted by
/// [`lift_expired_suspensions`] when its time comes.
pub fn suspend(
    conn: &Connection,
    issuer_did: &Did,
    credential_id: &str,
    until: Option<&str>,
    reason: Option<&str>,
    now: &str,
) -> Result<(), CredentialError> {
    let transaction = conn.unchecked_transaction()?;
    let (list_id, index) = suspension_slot(
        &transaction,
        issuer_did,
        credential_id,
        StatusChange::Suspend,
    )?
    .ok_or_else(|| CredentialError::NotSuspendable(credential_id.to_string()))?;
    write_bit(
        &transaction,
        issuer_did,
        &list_id,
        index,
        true,
        StatusChange::Suspend,
        now,
    )?;
    transaction
        .execute(
            "UPDATE credentials \
             SET suspended = 1, suspended_at = ?2, \
                 suspended_until = ?3, suspended_reason = ?4 \
             WHERE id = ?1 AND issuer_did = ?5",
            params![credential_id, now, until, reason, issuer_did.as_str()],
        )
        .map_err(CredentialError::step("suspend credential"))?;
    transaction.commit()?;
    Ok(())
}

/// §11.3 reinstatement: clear the credential's bit in its suspension list
/// and the issuer's bookkeeping. Idempotent. A credential that names no
/// suspension list has no published suspension to clear, so only the
/// bookkeeping changes.
pub fn reinstate(
    conn: &Connection,
    issuer_did: &Did,
    credential_id: &str,
    now: &str,
) -> Result<(), CredentialError> {
    let transaction = conn.unchecked_transaction()?;
    if let Some((list_id, index)) = suspension_slot(
        &transaction,
        issuer_did,
        credential_id,
        StatusChange::Reinstate,
    )? {
        write_bit(
            &transaction,
            issuer_did,
            &list_id,
            index,
            false,
            StatusChange::Reinstate,
            now,
        )?;
    }
    transaction
        .execute(
            "UPDATE credentials \
         SET suspended = 0, suspended_at = NULL, \
             suspended_until = NULL, suspended_reason = NULL \
         WHERE id = ?1 AND issuer_did = ?2",
            params![credential_id, issuer_did.as_str()],
        )
        .map_err(CredentialError::step("reinstate credential"))?;
    transaction.commit()?;
    Ok(())
}

/// Reinstate every credential `issuer_did` suspended until a time at or
/// before `now`. Run on the status-list publication path, so the next
/// publication carries the cleared bits. Returns how many were lifted.
pub fn lift_expired_suspensions(
    conn: &Connection,
    issuer_did: &Did,
    now: &str,
) -> Result<usize, CredentialError> {
    let due: Vec<String> = {
        let mut statement = conn.prepare(
            "SELECT id FROM credentials \
             WHERE issuer_did = ?1 AND suspended = 1 \
               AND suspended_until IS NOT NULL AND suspended_until <= ?2 \
             ORDER BY id",
        )?;
        let rows = statement.query_map(params![issuer_did.as_str(), now], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for credential_id in &due {
        reinstate(conn, issuer_did, credential_id, now)?;
    }
    Ok(due.len())
}

/// The suspension list and index a credential `issuer_did` issued names in
/// its signed `credentialStatus`, or `None` when it names none.
fn suspension_slot(
    conn: &Connection,
    issuer_did: &Did,
    credential_id: &str,
    change: StatusChange,
) -> Result<Option<(String, usize)>, CredentialError> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT issuer_did, signed_vc_json FROM credentials WHERE id = ?1",
            params![credential_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (credential_issuer, signed_json) =
        row.ok_or_else(|| CredentialError::NotFound(credential_id.to_string()))?;
    if credential_issuer != issuer_did.as_str() {
        return Err(CredentialError::NotIssuer(change));
    }
    let credential: crate::domain::vc::VerifiableCredential = serde_json::from_str(&signed_json)?;
    let Some(entry) = credential.status_entry(crate::domain::vc::STATUS_PURPOSE_SUSPENSION) else {
        return Ok(None);
    };
    let index = entry
        .status_list_index
        .parse::<usize>()
        .map_err(|_| CredentialError::NotSuspendable(credential_id.to_string()))?;
    Ok(Some((entry.status_list_credential.clone(), index)))
}

/// Set or clear one bit in a list `issuer_did` owns, bumping its version
/// when the bit changes so the host learns of it.
fn write_bit(
    conn: &Connection,
    issuer_did: &Did,
    list_id: &str,
    index: usize,
    value: bool,
    change: StatusChange,
    now: &str,
) -> Result<(), CredentialError> {
    let mut bits: Vec<u8> = conn
        .query_row(
            "SELECT bits FROM credential_status_lists WHERE list_id = ?1 AND issuer_did = ?2",
            params![list_id, issuer_did.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(CredentialError::step("load status list"))?
        .ok_or(CredentialError::NotIssuer(change))?;
    if alexandria_verify::vc::status::get_bit(&bits, index) == Some(value) {
        return Ok(());
    }
    alexandria_verify::vc::status::set_bit(&mut bits, index, value)
        .map_err(CredentialError::StatusBit)?;
    conn.execute(
        "UPDATE credential_status_lists \
         SET bits = ?2, version = version + 1, updated_at = ?3 \
         WHERE list_id = ?1 AND issuer_did = ?4",
        params![list_id, bits, now, issuer_did.as_str()],
    )
    .map_err(CredentialError::step("update status list"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::fixtures::{sample_request, setup, test_key, NOW};
    use crate::credentials::{self, CredentialError, Issuer, StatusChange};
    use crate::crypto::did::derive_did_key;

    #[test]
    fn revoke_sets_bit_and_marks_row_revoked() {
        let (db, key, issuer, subject) = setup();
        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        revoke(
            db.conn(),
            &issuer,
            vc.id.as_deref().unwrap(),
            "superseded",
            NOW,
        )
        .unwrap();

        let revoked: i64 = db
            .conn()
            .query_row(
                "SELECT revoked FROM credentials WHERE id = ?1",
                params![vc.id.as_deref().unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(revoked, 1);

        // Bit 0 must be flipped in the status list.
        let bits: Vec<u8> = db
            .conn()
            .query_row(
                "SELECT bits FROM credential_status_lists WHERE issuer_did = ?1",
                params![issuer.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bits[0] & 0x80, 0x80, "index 0 is the most significant bit");
    }

    #[test]
    fn revoke_is_idempotent() {
        let (db, key, issuer, subject) = setup();
        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        revoke(db.conn(), &issuer, vc.id.as_deref().unwrap(), "r1", NOW).unwrap();
        revoke(db.conn(), &issuer, vc.id.as_deref().unwrap(), "r2", NOW).unwrap();
        // One bit set; not doubled up.
        let bits: Vec<u8> = db
            .conn()
            .query_row(
                "SELECT bits FROM credential_status_lists WHERE issuer_did = ?1",
                params![issuer.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bits[0], 0x80, "index 0 is the most significant bit");
    }

    #[test]
    fn one_local_issuer_cannot_revoke_another_issuers_credential() {
        let (db, _, local_issuer, subject) = setup();
        let other_key = test_key("other-issuer");
        let credential = credentials::issue(
            db.conn(),
            &Issuer::from_key(other_key.clone()),
            &sample_request(subject),
            NOW,
        )
        .unwrap();

        let error = revoke(
            db.conn(),
            &local_issuer,
            credential.id.as_deref().unwrap(),
            "not mine",
            NOW,
        )
        .unwrap_err();
        assert!(
            matches!(error, CredentialError::NotIssuer(StatusChange::Revoke)),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            "only the credential issuer may revoke it"
        );

        let revoked: i64 = db
            .conn()
            .query_row(
                "SELECT revoked FROM credentials WHERE id = ?1",
                params![credential.id.as_deref().unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revoked, 0);
    }

    #[test]
    fn one_local_issuer_cannot_suspend_or_reinstate_another_issuers_credential() {
        let (db, _, local_issuer, subject) = setup();
        let other_key = test_key("other-suspension-issuer");
        let other_issuer = derive_did_key(&other_key);
        let credential = credentials::issue(
            db.conn(),
            &Issuer::from_key(other_key.clone()),
            &sample_request(subject),
            NOW,
        )
        .unwrap();
        let credential_id = credential.id.as_deref().unwrap();

        let suspend_error = suspend(
            db.conn(),
            &local_issuer,
            credential_id,
            None,
            Some("not mine"),
            NOW,
        )
        .unwrap_err();
        assert!(
            matches!(
                suspend_error,
                CredentialError::NotIssuer(StatusChange::Suspend)
            ),
            "{suspend_error:?}"
        );

        suspend(
            db.conn(),
            &other_issuer,
            credential_id,
            None,
            Some("issuer review"),
            NOW,
        )
        .unwrap();
        let reinstate_error = reinstate(db.conn(), &local_issuer, credential_id, NOW).unwrap_err();
        assert!(
            matches!(
                reinstate_error,
                CredentialError::NotIssuer(StatusChange::Reinstate)
            ),
            "{reinstate_error:?}"
        );

        let suspended: i64 = db
            .conn()
            .query_row(
                "SELECT suspended FROM credentials WHERE id = ?1",
                params![credential_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(suspended, 1);
    }

    #[test]
    fn a_status_change_to_an_unknown_credential_names_it() {
        let (db, _, issuer, _) = setup();
        let missing = "urn:uuid:missing";
        assert!(matches!(
            revoke(db.conn(), &issuer, missing, "gone", NOW).unwrap_err(),
            CredentialError::NotRevocable(id) if id == missing
        ));
        assert!(matches!(
            suspend(db.conn(), &issuer, missing, None, None, NOW).unwrap_err(),
            CredentialError::NotFound(id) if id == missing
        ));
        assert!(matches!(
            reinstate(db.conn(), &issuer, missing, NOW).unwrap_err(),
            CredentialError::NotFound(id) if id == missing
        ));
    }

    #[test]
    fn suspension_round_trip_flips_verify_decision() {
        use crate::domain::vc::verify_credential_db;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, issuer, subject) = setup();
        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();

        // Pre-suspension: accepted.
        let pre = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert_eq!(pre.acceptance_decision, AcceptanceDecision::Accept);
        assert!(!pre.suspended);

        // Suspend with no upper bound — indefinite suspension.
        suspend(
            db.conn(),
            &issuer,
            vc.id.as_deref().unwrap(),
            None,
            Some("under review"),
            NOW,
        )
        .unwrap();
        let mid = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(mid.suspended);
        assert_eq!(mid.acceptance_decision, AcceptanceDecision::Reject);

        // Reinstate.
        reinstate(db.conn(), &issuer, vc.id.as_deref().unwrap(), NOW).unwrap();
        let after = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(!after.suspended);
        assert_eq!(after.acceptance_decision, AcceptanceDecision::Accept);
    }

    /// Only what a status host would serve: each pending list signed by the
    /// issuer and checked as a fetched document, keyed by its URL.
    struct Served(Vec<(String, Vec<u8>)>);

    impl crate::domain::vc::VerificationStore for Served {
        fn registered_key(
            &self,
            _did: &Did,
        ) -> crate::domain::vc::StoreLookup<alexandria_verify::did::KeyRegistryEntry> {
            crate::domain::vc::StoreLookup::Missing
        }
        fn status_list_bits(&self, list_id: &str) -> crate::domain::vc::StoreLookup<Vec<u8>> {
            self.0
                .iter()
                .find(|(id, _)| id == list_id)
                .map(|(_, bits)| crate::domain::vc::StoreLookup::Found(bits.clone()))
                .unwrap_or(crate::domain::vc::StoreLookup::Missing)
        }
        fn is_superseded(&self, _credential_id: &str) -> crate::domain::vc::StoreLookup<bool> {
            crate::domain::vc::StoreLookup::Missing
        }
    }

    /// Issue against a configured status host, as an issuer that publishes.
    fn hosted() -> (
        crate::db::Database,
        credentials::Issuer,
        Did,
        crate::domain::vc::VerifiableCredential,
    ) {
        use crate::settings::{registry::keys, SettingsStore};
        let (db, key, issuer, subject) = setup();
        SettingsStore::set(
            db.conn(),
            keys::CREDENTIAL_STATUS_HOST,
            "https://cloud.example".to_string(),
        )
        .unwrap();
        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        (db, key, issuer, vc)
    }

    /// Publish what is pending and return what the host now serves: every
    /// list this issuer has published so far, newest version winning.
    fn publish(
        db: &crate::db::Database,
        key: &credentials::Issuer,
        issuer: &Did,
        served: &mut Served,
    ) {
        use crate::credentials::status_list;
        for list in status_list::pending(db.conn(), issuer).unwrap() {
            let document = status_list::signed(&list, key, NOW).unwrap();
            let bits = alexandria_verify::vc::status::verify_fetched_list(
                &document,
                &list.list_id,
                issuer,
                &list.status_purpose,
            )
            .unwrap();
            served.0.retain(|(id, _)| id != &list.list_id);
            served.0.push((list.list_id.clone(), bits));
            status_list::mark_published(db.conn(), &[(list.list_id, list.version)]).unwrap();
        }
    }

    #[test]
    fn a_suspended_credential_verifies_as_suspended_from_its_published_list_alone() {
        use crate::domain::vc::{verify, AcceptanceDecision, VerificationPolicy};
        let (db, key, issuer, vc) = hosted();
        let mut served = Served(Vec::new());
        publish(&db, &key, &issuer, &mut served);
        let policy = VerificationPolicy::default();
        let before = verify::verify_credential(&served, &vc, NOW, &policy);
        assert_eq!(before.acceptance_decision, AcceptanceDecision::Accept);

        suspend(
            db.conn(),
            &issuer,
            vc.id.as_deref().unwrap(),
            None,
            Some("under review"),
            NOW,
        )
        .unwrap();
        publish(&db, &key, &issuer, &mut served);
        // No issuer database: only the lists as the host serves them.
        let after = verify::verify_credential(&served, &vc, NOW, &policy);
        assert!(after.suspended);
        assert!(
            !after.revoked,
            "suspension never touches the revocation list"
        );
        assert_eq!(after.acceptance_decision, AcceptanceDecision::Reject);

        reinstate(db.conn(), &issuer, vc.id.as_deref().unwrap(), NOW).unwrap();
        publish(&db, &key, &issuer, &mut served);
        let reinstated = verify::verify_credential(&served, &vc, NOW, &policy);
        assert!(!reinstated.suspended);
        assert_eq!(reinstated.acceptance_decision, AcceptanceDecision::Accept);
    }

    #[test]
    fn a_timed_suspension_is_lifted_in_the_published_list_when_its_time_comes() {
        use crate::domain::vc::{verify, VerificationPolicy};
        let (db, key, issuer, vc) = hosted();
        let id = vc.id.as_deref().unwrap();
        let mut served = Served(Vec::new());
        suspend(
            db.conn(),
            &issuer,
            id,
            Some("2026-05-01T00:00:00Z"),
            None,
            NOW,
        )
        .unwrap();

        // Not yet due: nothing lifts, and the published bit stays set.
        assert_eq!(
            lift_expired_suspensions(db.conn(), &issuer, NOW).unwrap(),
            0
        );
        publish(&db, &key, &issuer, &mut served);
        let policy = VerificationPolicy::default();
        assert!(verify::verify_credential(&served, &vc, NOW, &policy).suspended);

        // Due: the issuer's node clears the bit, and the next publication
        // carries the cleared list.
        let later = "2026-05-02T00:00:00Z";
        assert_eq!(
            lift_expired_suspensions(db.conn(), &issuer, later).unwrap(),
            1
        );
        publish(&db, &key, &issuer, &mut served);
        assert!(!verify::verify_credential(&served, &vc, later, &policy).suspended);
        let suspended: i64 = db
            .conn()
            .query_row(
                "SELECT suspended FROM credentials WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(suspended, 0, "the issuer's bookkeeping follows the list");
    }

    #[test]
    fn an_indefinite_suspension_is_never_lifted_by_time() {
        let (db, _key, issuer, vc) = hosted();
        suspend(
            db.conn(),
            &issuer,
            vc.id.as_deref().unwrap(),
            None,
            None,
            NOW,
        )
        .unwrap();
        assert_eq!(
            lift_expired_suspensions(db.conn(), &issuer, "2099-01-01T00:00:00Z").unwrap(),
            0
        );
    }

    #[test]
    fn suspension_leaves_revocation_permanent_and_separate() {
        use crate::domain::vc::verify_credential_db;
        use crate::domain::vc::VerificationPolicy;
        let (db, key, issuer, subject) = setup();
        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        let id = vc.id.as_deref().unwrap();
        revoke(db.conn(), &issuer, id, "withdrawn", NOW).unwrap();
        suspend(db.conn(), &issuer, id, None, None, NOW).unwrap();
        reinstate(db.conn(), &issuer, id, NOW).unwrap();
        let result = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(result.revoked, "reinstating never clears a revocation");
        assert!(!result.suspended);
    }

    #[test]
    fn permissive_policy_can_accept_suspended() {
        use crate::domain::vc::verify_credential_db;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, issuer, subject) = setup();
        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        suspend(
            db.conn(),
            &issuer,
            vc.id.as_deref().unwrap(),
            None,
            None,
            NOW,
        )
        .unwrap();

        let permissive = VerificationPolicy {
            reject_suspended: false,
            ..Default::default()
        };
        let result = verify_credential_db(db.conn(), &vc, NOW, &permissive);
        assert!(result.suspended);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
    }
}
