//! Reading stored credentials, and decoding untrusted credential payloads
//! under structural limits before any typed decoding.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::vc::VerifiableCredential;

pub fn get(conn: &Connection, credential_id: &str) -> Result<Option<VerifiableCredential>, String> {
    let json: Option<String> = conn
        .query_row(
            "SELECT signed_vc_json FROM credentials WHERE id = ?1",
            params![credential_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match json {
        Some(s) => Ok(Some(serde_json::from_str(&s).map_err(|e| e.to_string())?)),
        None => Ok(None),
    }
}

pub fn list(
    conn: &Connection,
    subject_did: Option<&str>,
    skill_id: Option<&str>,
) -> Result<Vec<VerifiableCredential>, String> {
    list_filtered(conn, subject_did, skill_id, false)
}

/// [`list`], optionally leaving out revoked credentials.
///
/// Counts of what a learner has earned must skip revoked rows; the
/// credential list itself still shows them, marked as revoked.
pub fn list_filtered(
    conn: &Connection,
    subject_did: Option<&str>,
    skill_id: Option<&str>,
    active_only: bool,
) -> Result<Vec<VerifiableCredential>, String> {
    let mut sql = String::from("SELECT signed_vc_json FROM credentials WHERE 1=1");
    if active_only {
        sql.push_str(" AND revoked = 0");
    }
    let mut args: Vec<String> = Vec::new();
    if let Some(s) = subject_did {
        sql.push_str(" AND subject_did = ?");
        args.push(s.to_string());
    }
    if let Some(k) = skill_id {
        sql.push_str(" AND skill_id = ?");
        args.push(k.to_string());
    }
    sql.push_str(" ORDER BY received_at DESC");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args.iter()), |r| {
            r.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        let s = r.map_err(|e| e.to_string())?;
        out.push(serde_json::from_str(&s).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Structural limits for an untrusted credential payload: a bundle, a list of
/// credentials, or one credential. A bundle's status lists carry base64
/// bitmaps up to the status-list bitmap cap, so its strings may be longer than
/// a credential's; every credential entry is still held to
/// [`alexandria_verify::vc::CREDENTIAL_JSON_LIMITS`].
pub const CREDENTIAL_PAYLOAD_JSON_LIMITS: alexandria_verify::json::JsonLimits =
    alexandria_verify::json::JsonLimits {
        max_bytes: 16 * 1024 * 1024,
        max_depth: 32,
        max_array_len: 4096,
        max_object_entries: 256,
        max_string_bytes: 4 * crate::p2p::vc_status::MAX_BITS_BYTES.div_ceil(3),
    };

/// Parse an untrusted credential payload under
/// [`CREDENTIAL_PAYLOAD_JSON_LIMITS`] before any typed decoding.
pub(crate) fn parse_credential_payload(payload: &str) -> Result<serde_json::Value, String> {
    alexandria_verify::json::parse_untrusted(payload.as_bytes(), &CREDENTIAL_PAYLOAD_JSON_LIMITS)
        .map_err(|error| format!("not a valid credential payload: {error}"))
}

/// Decode one credential entry of a parsed payload under the single-credential
/// limits, so a bundle or list cannot carry a credential a direct import would
/// refuse.
pub(crate) fn credential_entry(
    entry: &serde_json::Value,
) -> Result<VerifiableCredential, alexandria_verify::json::UntrustedJsonError> {
    let bytes = serde_json::to_vec(entry)
        .map_err(|error| alexandria_verify::json::UntrustedJsonError::Invalid(error.to_string()))?;
    alexandria_verify::vc::decode_credential(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::fixtures::{sample_request, setup, NOW};
    use crate::credentials::{self, lifecycle};
    use crate::domain::vc::Claim;

    #[test]
    fn get_credential_returns_none_for_unknown_id() {
        let (db, _, _, _) = setup();
        let got = get(db.conn(), "urn:uuid:missing").unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn list_credentials_filters_by_subject_and_skill() {
        let (db, key, _, subject) = setup();
        credentials::issue(db.conn(), &key, &sample_request(subject.clone()), NOW).unwrap();
        // Different skill
        let mut req2 = sample_request(subject.clone());
        if let Claim::Skill(ref mut s) = req2.claim {
            s.skill_id = "other_skill".into();
        }
        credentials::issue(db.conn(), &key, &req2, NOW).unwrap();

        let all = list(db.conn(), Some(subject.as_str()), None).unwrap();
        assert_eq!(all.len(), 2);
        let one = list(db.conn(), Some(subject.as_str()), Some("other_skill")).unwrap();
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn active_only_listing_leaves_out_revoked_credentials() {
        let (db, key, issuer, subject) = setup();
        let vc =
            credentials::issue(db.conn(), &key, &sample_request(subject.clone()), NOW).unwrap();
        lifecycle::revoke(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();

        let all = list_filtered(db.conn(), Some(subject.as_str()), None, false).unwrap();
        assert_eq!(
            all.len(),
            1,
            "the full list still shows a revoked credential"
        );
        let active = list_filtered(db.conn(), Some(subject.as_str()), None, true).unwrap();
        assert!(active.is_empty());
    }
}
