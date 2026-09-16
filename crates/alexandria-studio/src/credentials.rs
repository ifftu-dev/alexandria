//! Credential summaries and presentation verification for assistants
//! holding `credentials:read`.
//!
//! Summaries carry provenance — issuer, skill, dates, revocation and the
//! integrity hash — but never `signed_vc_json`. The signed document is the
//! credential: handing it to an assistant would send the whole claim to that
//! assistant's model provider, and anyone holding it can show it onward. Only
//! the learner's own credentials are listed.
//!
//! `verify_presentation` keeps the order the app uses: audience, then the
//! replay probe, then the signature. Probing replay before checking the
//! signature means a caller cannot learn whether a nonce was used by timing
//! the response, and the nonce is recorded only once a signature verifies.

use alexandria_verify::did::{parse_did_key, resolve_did_key};
use alexandria_verify::vc::sign::b64url_decode;
use ed25519_dalek::Signature;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::skills::local_did;
use crate::{Error, Result};

pub const MAX_PAGE: u32 = 50;
pub const DEFAULT_PAGE: u32 = 20;
const MAX_OFFSET: u32 = 10_000;
/// Largest presentation this will parse, so a caller cannot hand over an
/// unbounded document.
pub const MAX_PRESENTATION_BYTES: usize = 256_000;

/// A presentation as the app builds it: a canonical payload naming the
/// audience and nonce, and a detached Ed25519 JWS over it by the subject.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationEnvelope {
    pub id: String,
    pub payload_json: String,
    pub proof: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationVerification {
    Accepted,
    BadSignature,
    AudienceMismatch,
    Replayed,
    Malformed,
}

impl PresentationVerification {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::BadSignature => "bad_signature",
            Self::AudienceMismatch => "audience_mismatch",
            Self::Replayed => "replayed",
            Self::Malformed => "malformed",
        }
    }
}

const SUMMARY_COLS: &str = "c.id, c.issuer_did, c.subject_did, c.credential_type, c.claim_kind,
     c.skill_id, s.name, c.issuance_date, c.expiration_date, c.revoked, c.revoked_at,
     c.revocation_reason, c.status_list_id IS NOT NULL, c.supersedes, c.received_at,
     c.integrity_hash";

fn summary(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "credential_id": row.get::<_, String>(0)?,
        "issuer_did": row.get::<_, String>(1)?,
        "subject_did": row.get::<_, String>(2)?,
        "credential_type": row.get::<_, String>(3)?,
        "claim_kind": row.get::<_, String>(4)?,
        "skill_id": row.get::<_, Option<String>>(5)?,
        "skill_name": row.get::<_, Option<String>>(6)?,
        "issuance_date": row.get::<_, String>(7)?,
        "expiration_date": row.get::<_, Option<String>>(8)?,
        "revoked": row.get::<_, bool>(9)?,
        "revoked_at": row.get::<_, Option<String>>(10)?,
        "revocation_reason": row.get::<_, Option<String>>(11)?,
        "has_status_list": row.get::<_, bool>(12)?,
        "supersedes": row.get::<_, Option<String>>(13)?,
        "received_at": row.get::<_, String>(14)?,
        "integrity_hash": row.get::<_, String>(15)?,
    }))
}

/// The learner's own credentials, newest first.
pub fn list_credentials(
    conn: &Connection,
    skill_id: &str,
    include_revoked: bool,
    limit: u32,
    cursor: &str,
) -> Result<Value> {
    if skill_id.len() > 200 {
        return Err(Error::Invalid("skill identifier is too long".into()));
    }
    let limit = if limit == 0 {
        DEFAULT_PAGE
    } else {
        limit.min(MAX_PAGE)
    };
    let offset: u32 = if cursor.is_empty() {
        0
    } else {
        cursor
            .parse()
            .ok()
            .filter(|offset| *offset <= MAX_OFFSET)
            .ok_or_else(|| Error::Invalid("unknown cursor".into()))?
    };
    let did = local_did(conn);
    if did.is_empty() {
        return Ok(json!({"items": [], "next_cursor": null, "subject_did": ""}));
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT {SUMMARY_COLS}
         FROM credentials c LEFT JOIN skills s ON s.id = c.skill_id
         WHERE c.subject_did = ?1 AND (?2 = '' OR c.skill_id = ?2) AND (?3 OR c.revoked = 0)
         ORDER BY c.received_at DESC, c.id LIMIT ?4 OFFSET ?5"
    ))?;
    let rows = stmt.query_map(
        params![did, skill_id, include_revoked, limit + 1, offset],
        summary,
    )?;
    let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    let next_cursor = (items.len() > limit as usize && offset + limit <= MAX_OFFSET)
        .then(|| (offset + limit).to_string());
    items.truncate(limit as usize);
    Ok(json!({"items": items, "next_cursor": next_cursor, "subject_did": did}))
}

/// One of the learner's own credentials. Another subject's credential is
/// reported missing rather than refused, so the tool cannot be used to probe
/// what else this device holds.
pub fn get_credential(conn: &Connection, credential_id: &str) -> Result<Value> {
    if credential_id.is_empty() || credential_id.len() > 300 {
        return Err(Error::Invalid("invalid credential identifier".into()));
    }
    let did = local_did(conn);
    conn.query_row(
        &format!(
            "SELECT {SUMMARY_COLS}
             FROM credentials c LEFT JOIN skills s ON s.id = c.skill_id
             WHERE c.id = ?1 AND c.subject_did = ?2"
        ),
        params![credential_id, did],
        summary,
    )
    .optional()?
    .ok_or(Error::NotFound)
}

/// Check a presentation against the audience it was meant for.
pub fn verify_presentation(
    conn: &Connection,
    envelope: &PresentationEnvelope,
    expected_audience: &str,
) -> Result<PresentationVerification> {
    if envelope.payload_json.len() > MAX_PRESENTATION_BYTES
        || envelope.proof.len() > 4_000
        || envelope.subject.len() > 300
    {
        return Err(Error::Invalid("the presentation is too large".into()));
    }
    let Ok(payload) = serde_json::from_str::<Value>(&envelope.payload_json) else {
        return Ok(PresentationVerification::Malformed);
    };
    let (Some(audience), Some(nonce)) = (
        payload.get("audience").and_then(Value::as_str),
        payload.get("nonce").and_then(Value::as_str),
    ) else {
        return Ok(PresentationVerification::Malformed);
    };
    if audience != expected_audience {
        return Ok(PresentationVerification::AudienceMismatch);
    }

    let seen: i64 = conn.query_row(
        "SELECT COUNT(*) FROM presentations_seen WHERE audience = ?1 AND nonce = ?2",
        params![audience, nonce],
        |row| row.get(0),
    )?;
    if seen > 0 {
        return Ok(PresentationVerification::Replayed);
    }

    let Ok(subject) = parse_did_key(&envelope.subject) else {
        return Ok(PresentationVerification::Malformed);
    };
    let Ok(key) = resolve_did_key(&subject) else {
        return Ok(PresentationVerification::Malformed);
    };

    let parts: Vec<&str> = envelope.proof.split('.').collect();
    if parts.len() != 3 || !parts[1].is_empty() {
        return Ok(PresentationVerification::BadSignature);
    }
    let Some(signature) = b64url_decode(parts[2]).filter(|bytes| bytes.len() == 64) else {
        return Ok(PresentationVerification::BadSignature);
    };
    let mut bytes = [0u8; 64];
    bytes.copy_from_slice(&signature);
    let mut signed = Vec::with_capacity(parts[0].len() + 1 + envelope.payload_json.len());
    signed.extend_from_slice(parts[0].as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(envelope.payload_json.as_bytes());
    if key
        .verify_strict(&signed, &Signature::from_bytes(&bytes))
        .is_err()
    {
        return Ok(PresentationVerification::BadSignature);
    }

    // First writer wins a race; the loser sees the row and reports Replayed.
    conn.execute(
        "INSERT OR IGNORE INTO presentations_seen (audience, nonce) VALUES (?1, ?2)",
        params![audience, nonce],
    )?;
    Ok(PresentationVerification::Accepted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alexandria_verify::did::did_from_verifying_key;
    use alexandria_verify::vc::sign::b64url;
    use ed25519_dalek::{Signer, SigningKey};

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE skills (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE credentials (id TEXT PRIMARY KEY, issuer_did TEXT NOT NULL,
                 subject_did TEXT NOT NULL, credential_type TEXT NOT NULL, claim_kind TEXT NOT NULL,
                 skill_id TEXT, issuance_date TEXT NOT NULL, expiration_date TEXT,
                 signed_vc_json TEXT NOT NULL, integrity_hash TEXT NOT NULL, status_list_id TEXT,
                 status_list_index INTEGER, revoked INTEGER NOT NULL DEFAULT 0, revoked_at TEXT,
                 revocation_reason TEXT, supersedes TEXT, received_at TEXT NOT NULL);
             CREATE TABLE presentations_seen (audience TEXT NOT NULL, nonce TEXT NOT NULL,
                 seen_at TEXT NOT NULL DEFAULT '2026-01-01', PRIMARY KEY (audience, nonce));
             INSERT INTO app_settings(key, value) VALUES ('identity.local_did', 'did:key:me');
             INSERT INTO skills(id, name) VALUES ('a', 'Alpha');
             INSERT INTO credentials(id, issuer_did, subject_did, credential_type, claim_kind,
                 skill_id, issuance_date, signed_vc_json, integrity_hash, revoked, received_at) VALUES
                 ('mine-1', 'did:key:issuer', 'did:key:me', 'FormalCredential', 'skill', 'a',
                  '2026-01-01', '{\"SECRET\":\"signed document\"}', 'hash-1', 0, '2026-02-01'),
                 ('mine-2', 'did:key:issuer', 'did:key:me', 'FormalCredential', 'skill', 'a',
                  '2026-01-02', '{\"SECRET\":\"signed document\"}', 'hash-2', 1, '2026-02-02'),
                 ('theirs', 'did:key:issuer', 'did:key:other', 'FormalCredential', 'skill', 'a',
                  '2026-01-03', '{\"SECRET\":\"signed document\"}', 'hash-3', 0, '2026-02-03');",
        )
        .unwrap();
        conn
    }

    fn present(key: &SigningKey, audience: &str, nonce: &str) -> PresentationEnvelope {
        let payload = json!({"audience": audience, "nonce": nonce, "bundle": []}).to_string();
        let header = b64url(br#"{"alg":"EdDSA"}"#);
        let mut signed = header.clone().into_bytes();
        signed.push(b'.');
        signed.extend_from_slice(payload.as_bytes());
        let signature = b64url(&key.sign(&signed).to_bytes());
        PresentationEnvelope {
            id: "urn:presentation:1".into(),
            payload_json: payload,
            proof: format!("{header}..{signature}"),
            subject: did_from_verifying_key(&key.verifying_key())
                .as_str()
                .to_string(),
        }
    }

    #[test]
    fn summaries_carry_provenance_but_never_the_signed_document() {
        let conn = db();
        let live = list_credentials(&conn, "", false, 0, "").unwrap();
        let items = live["items"].as_array().unwrap();
        assert_eq!(items.len(), 1, "revoked and other subjects are left out");
        assert_eq!(items[0]["credential_id"], "mine-1");
        assert_eq!(items[0]["skill_name"], "Alpha");
        assert_eq!(items[0]["integrity_hash"], "hash-1");
        assert_eq!(items[0]["has_status_list"], false);

        let all = list_credentials(&conn, "", true, 0, "").unwrap();
        assert_eq!(all["items"].as_array().unwrap().len(), 2);
        assert_eq!(all["items"][0]["credential_id"], "mine-2", "newest first");
        assert_eq!(all["items"][0]["revoked"], true);

        let one = get_credential(&conn, "mine-1").unwrap();
        assert_eq!(one["issuer_did"], "did:key:issuer");
        assert!(matches!(
            get_credential(&conn, "theirs"),
            Err(Error::NotFound)
        ));

        for text in [live.to_string(), all.to_string(), one.to_string()] {
            assert!(!text.contains("SECRET"), "signed document leaked: {text}");
            assert!(!text.contains("signed_vc_json"));
        }
    }

    #[test]
    fn a_presentation_is_accepted_once_and_then_replayed() {
        let conn = db();
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let envelope = present(&key, "did:key:verifier", "nonce-1");

        assert_eq!(
            verify_presentation(&conn, &envelope, "did:key:verifier").unwrap(),
            PresentationVerification::Accepted
        );
        assert_eq!(
            verify_presentation(&conn, &envelope, "did:key:verifier").unwrap(),
            PresentationVerification::Replayed,
            "the nonce was recorded on acceptance"
        );
        assert_eq!(
            verify_presentation(&conn, &envelope, "did:key:someone-else").unwrap(),
            PresentationVerification::AudienceMismatch
        );
    }

    #[test]
    fn tampering_and_junk_are_refused_without_recording_a_nonce() {
        let conn = db();
        let key = SigningKey::from_bytes(&[9u8; 32]);
        let mut tampered = present(&key, "did:key:verifier", "nonce-2");
        tampered.payload_json = tampered
            .payload_json
            .replace("\"bundle\":[]", "\"bundle\":[1]");
        assert_eq!(
            verify_presentation(&conn, &tampered, "did:key:verifier").unwrap(),
            PresentationVerification::BadSignature
        );

        let mut malformed = present(&key, "did:key:verifier", "nonce-3");
        malformed.payload_json = "not json".into();
        assert_eq!(
            verify_presentation(&conn, &malformed, "did:key:verifier").unwrap(),
            PresentationVerification::Malformed
        );

        let mut unknown_subject = present(&key, "did:key:verifier", "nonce-4");
        unknown_subject.subject = "did:web:example.com".into();
        assert_eq!(
            verify_presentation(&conn, &unknown_subject, "did:key:verifier").unwrap(),
            PresentationVerification::Malformed
        );

        let seen: i64 = conn
            .query_row("SELECT COUNT(*) FROM presentations_seen", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(seen, 0, "a refused presentation records nothing");
    }
}
