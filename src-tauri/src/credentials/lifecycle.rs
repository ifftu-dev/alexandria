//! Changing a credential's standing after it was issued: revoking it
//! (permanent, through its status list), suspending it and reinstating it.
//! Only the credential's issuer may do any of them.

use rusqlite::{params, Connection, OptionalExtension};

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
) -> Result<(), String> {
    let transaction = conn.unchecked_transaction().map_err(|e| e.to_string())?;
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
        .optional()
        .map_err(|e| e.to_string())?
        .and_then(
            |(credential_issuer, list_id, index, list_issuer)| match (list_id, index) {
                (Some(list_id), Some(index)) => {
                    Some((credential_issuer, list_id, index, list_issuer))
                }
                _ => None,
            },
        );

    let (credential_issuer, list_id, index, list_issuer) =
        row.ok_or_else(|| format!("credential {credential_id} not found or has no status list"))?;
    if credential_issuer != issuer_did.as_str() || list_issuer != issuer_did.as_str() {
        return Err("only the credential issuer may revoke it".to_string());
    }

    // Read current bits, set the revocation bit, write back + bump version.
    let mut bits: Vec<u8> = transaction
        .query_row(
            "SELECT bits FROM credential_status_lists WHERE list_id = ?1",
            params![list_id],
            |r| r.get(0),
        )
        .map_err(|e| format!("load status list: {e}"))?;
    // Bitstring Status List: index 0 is the most significant bit of byte 0.
    alexandria_verify::vc::status::set_bit(&mut bits, index as usize, true)
        .map_err(|e| e.to_string())?;

    transaction
        .execute(
            "UPDATE credential_status_lists \
         SET bits = ?2, version = version + 1, updated_at = ?3 \
         WHERE list_id = ?1 AND issuer_did = ?4",
            params![list_id, bits, now, issuer_did.as_str()],
        )
        .map_err(|e| format!("update status list: {e}"))?;

    transaction
        .execute(
            "UPDATE credentials \
         SET revoked = 1, revoked_at = ?2, revocation_reason = ?3 \
         WHERE id = ?1 AND issuer_did = ?4",
            params![credential_id, now, reason, issuer_did.as_str()],
        )
        .map_err(|e| format!("update credential: {e}"))?;

    transaction.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// §11.3 suspension. Set `suspended = 1` plus optional
/// `suspended_until` for automatic reinstatement at verify time.
/// Idempotent — re-suspending updates the until window.
pub fn suspend(
    conn: &Connection,
    issuer_did: &Did,
    credential_id: &str,
    until: Option<&str>,
    reason: Option<&str>,
    now: &str,
) -> Result<(), String> {
    let updated = conn
        .execute(
            "UPDATE credentials \
             SET suspended = 1, suspended_at = ?2, \
                 suspended_until = ?3, suspended_reason = ?4 \
             WHERE id = ?1 AND issuer_did = ?5",
            params![credential_id, now, until, reason, issuer_did.as_str()],
        )
        .map_err(|e| format!("suspend credential: {e}"))?;
    if updated == 0 {
        return Err(format!(
            "credential {credential_id} not found or caller is not its issuer"
        ));
    }
    Ok(())
}

/// §11.3 reinstatement — clear the suspension flag. Idempotent.
pub fn reinstate(conn: &Connection, issuer_did: &Did, credential_id: &str) -> Result<(), String> {
    let updated = conn
        .execute(
            "UPDATE credentials \
         SET suspended = 0, suspended_at = NULL, \
             suspended_until = NULL, suspended_reason = NULL \
         WHERE id = ?1 AND issuer_did = ?2",
            params![credential_id, issuer_did.as_str()],
        )
        .map_err(|e| format!("reinstate credential: {e}"))?;
    if updated == 0 {
        return Err(format!(
            "credential {credential_id} not found or caller is not its issuer"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::fixtures::{sample_request, setup, test_key, NOW};
    use crate::credentials::{self, Issuer};
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
        assert!(error.contains("only the credential issuer"));

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
        assert!(suspend_error.contains("caller is not its issuer"));

        suspend(
            db.conn(),
            &other_issuer,
            credential_id,
            None,
            Some("issuer review"),
            NOW,
        )
        .unwrap();
        let reinstate_error = reinstate(db.conn(), &local_issuer, credential_id).unwrap_err();
        assert!(reinstate_error.contains("caller is not its issuer"));

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
        reinstate(db.conn(), &issuer, vc.id.as_deref().unwrap()).unwrap();
        let after = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(!after.suspended);
        assert_eq!(after.acceptance_decision, AcceptanceDecision::Accept);
    }

    #[test]
    fn suspension_with_until_in_past_is_no_longer_active() {
        use crate::domain::vc::verify_credential_db;
        use crate::domain::vc::VerificationPolicy;

        let (db, key, issuer, subject) = setup();
        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();

        // Suspended until a time before NOW — verifier sees the
        // suspension as auto-expired and treats the credential as
        // active again.
        suspend(
            db.conn(),
            &issuer,
            vc.id.as_deref().unwrap(),
            Some("2026-01-01T00:00:00Z"),
            None,
            NOW,
        )
        .unwrap();
        let result = verify_credential_db(db.conn(), &vc, NOW, &VerificationPolicy::default());
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
