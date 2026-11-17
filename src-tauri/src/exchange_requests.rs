//! Credential requests stored as verified.
//!
//! An organisation's credential request ([`CredentialRequest`]) may carry
//! the issuance policy it will judge the answer against. Before an attempt
//! answering one starts, the app fetches the request again, checks the
//! organisation's proof against its domain, and stores it here. The trust
//! profile chosen at session start takes the request's floor from this
//! table, never from the client, as a sponsor role's floor comes from its
//! stored policy ([`crate::sentinel::trust_profile`]). The attempt then
//! keeps its own copy of the policy (`assessment_attempts.request_policy_json`).

use alexandria_verify::exchange::CredentialRequest;
use rusqlite::{params, Connection, OptionalExtension};

use crate::sentinel::scoring::TrustProfile;

/// Store `request`, verified at `now_iso` and pulled from `directory_url`,
/// replacing an earlier copy, and drop requests that expired before
/// `now_unix`. Refuses a request [`CredentialRequest::check`] refuses.
pub(crate) fn store(
    conn: &Connection,
    directory_url: &str,
    request: &CredentialRequest,
    now_unix: i64,
    now_iso: &str,
) -> Result<(), String> {
    request.check()?;
    let json = serde_json::to_string(request).map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM exchange_requests WHERE expires_at <= ?1",
        params![now_unix],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO exchange_requests (id, directory_url, request_json, expires_at, verified_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET directory_url = excluded.directory_url,
             request_json = excluded.request_json, expires_at = excluded.expires_at,
             verified_at = excluded.verified_at",
        params![request.id, directory_url, json, request.expires_at, now_iso],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// The stored request with `id`, if this device verified one.
pub(crate) fn stored(conn: &Connection, id: &str) -> Result<Option<CredentialRequest>, String> {
    let json: Option<String> = conn
        .query_row(
            "SELECT request_json FROM exchange_requests WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    json.map(|j| serde_json::from_str(&j).map_err(|e| format!("stored request unreadable: {e}")))
        .transpose()
}

/// The minimum trust profile the stored request's policy asks for. A
/// request this device has not verified is refused rather than read as "no
/// floor".
pub(crate) fn floor(conn: &Connection, id: &str) -> Result<Option<TrustProfile>, String> {
    let request = stored(conn, id)?
        .ok_or("this credential request has not been verified on this device; open it again from your requests")?;
    Ok(request
        .issuance_policy
        .and_then(|policy| policy.min_trust_profile))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::credentials::IssuancePolicy;
    use crate::db::Database;

    /// A request for `skill_id` from a verified organisation, with `policy`.
    pub(crate) fn request(
        id: &str,
        skill_id: &str,
        policy: Option<IssuancePolicy>,
    ) -> CredentialRequest {
        CredentialRequest {
            id: id.into(),
            audience: "urn:organization:example".into(),
            nonce: format!("{id}-nonce"),
            organization: "Example".into(),
            subject_did: "did:key:z6MkExample".into(),
            skill_id: skill_id.into(),
            network_id: "preprod".into(),
            taxonomy_digest: "digest".into(),
            purpose: "Hiring".into(),
            role_label: "Engineer".into(),
            require_new_assessment: true,
            created_at: 100,
            expires_at: 10_000,
            issuance_policy: policy,
        }
    }

    fn camera_floor() -> IssuancePolicy {
        IssuancePolicy {
            min_trust_profile: Some(TrustProfile::Camera),
            ..IssuancePolicy::default()
        }
    }

    #[test]
    fn a_stored_request_gives_its_floor_and_an_unknown_one_is_refused() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let req = request("r1", "skill_rust", Some(camera_floor()));
        store(db.conn(), "https://cloud.example", &req, 200, "now").unwrap();
        assert_eq!(stored(db.conn(), "r1").unwrap(), Some(req.clone()));
        assert_eq!(floor(db.conn(), "r1").unwrap(), Some(TrustProfile::Camera));
        assert!(floor(db.conn(), "unknown").is_err());

        // A request without a policy has no floor.
        store(
            db.conn(),
            "https://cloud.example",
            &request("r2", "skill_rust", None),
            200,
            "now",
        )
        .unwrap();
        assert_eq!(floor(db.conn(), "r2").unwrap(), None);
    }

    #[test]
    fn storing_replaces_the_copy_and_drops_expired_requests() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        store(
            db.conn(),
            "https://cloud.example",
            &request("r1", "skill_rust", None),
            200,
            "now",
        )
        .unwrap();
        store(
            db.conn(),
            "https://cloud.example",
            &request("r1", "skill_rust", Some(camera_floor())),
            200,
            "later",
        )
        .unwrap();
        assert_eq!(floor(db.conn(), "r1").unwrap(), Some(TrustProfile::Camera));
        let mut fresh = request("r2", "skill_rust", None);
        fresh.expires_at = 50_000;
        store(
            db.conn(),
            "https://cloud.example",
            &fresh,
            20_000,
            "much later",
        )
        .unwrap();
        assert!(stored(db.conn(), "r1").unwrap().is_none(), "expired");
    }

    #[test]
    fn a_request_naming_a_model_connection_is_not_stored() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let policy = IssuancePolicy {
            probe_connection_id: Some("conn".into()),
            ..IssuancePolicy::default()
        };
        assert!(store(
            db.conn(),
            "https://cloud.example",
            &request("r1", "skill_rust", Some(policy)),
            200,
            "now",
        )
        .is_err());
        assert!(stored(db.conn(), "r1").unwrap().is_none());
    }
}
