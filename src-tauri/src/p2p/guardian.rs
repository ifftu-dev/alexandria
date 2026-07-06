//! Guardian link protocol (`/alexandria/guardian/1.0`).
//!
//! Cross-USER by design — unlike device sync there is no stake-address
//! match. The authentication chain:
//!
//!   1. The counterparty's libp2p `PeerId` is authenticated by the
//!      Noise transport handshake.
//!   2. `Link` is authorised by possession of a single-use invite code
//!      the child generated (consumed on first use, replay-proof).
//!   3. Everything else is authorised by the per-link 32-byte AEAD
//!      key: a payload that does not open under the stored key is
//!      rejected, and replies seal under the same key.
//!
//! The child pushes sealed activity snapshots to the guardian; the
//! guardian can also pull on demand. Guardianship is recorded as a
//! parent-issued `RoleCredential` (`role = "guardian"`, subject =
//! child DID) verified by the child before activation.
//!
//! Privacy invariant: guardian data NEVER rides gossip or device sync.
//! This request-response protocol, sealed under the link key, is the
//! only transport.

use rusqlite::{Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::domain::vc::{verify::verify_credential, VerifiableCredential, VerificationPolicy};

/// Tables mirrored to the guardian. Deliberately independent of the
/// device-sync allowlist: adding a table here means a parent sees it.
pub const GUARDIAN_SYNC_TABLES: &[&str] = &[
    "enrollments",
    "element_progress",
    "element_submissions",
    "plugin_irl_submissions",
    "classroom_members",
    // Course metadata so the guardian can render titles for the
    // child's enrollments (rows land in the mirror, not the parent's
    // own courses table).
    "courses",
];

/// One synced entity: a full-row JSON snapshot, LWW on `updated_at`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityRow {
    pub entity_id: String,
    pub data: serde_json::Value,
    pub updated_at: String,
}

/// The child's sealed activity snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GuardianActivityPayload {
    pub child_did: String,
    pub display_name: Option<String>,
    /// The child's self-reported birthdate — carried only inside the
    /// sealed link payload, never published.
    pub birthdate: Option<String>,
    pub tables: Vec<(String, Vec<ActivityRow>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GuardianRequest {
    /// Parent → child: accept an invite and establish the link.
    Link {
        /// Hash of the invite code (proves possession; single use).
        code_hash: String,
        /// The link id the parent generated; both sides store it.
        link_id: String,
        guardian_did: String,
        guardian_stake_address: String,
        guardian_display_name: Option<String>,
        /// Parent-issued `RoleCredential` (role = "guardian",
        /// subject = child DID), JSON-serialised.
        guardian_vc_json: String,
    },
    /// Child → guardian: sealed [`GuardianActivityPayload`].
    ActivityPush { link_id: String, sealed: Vec<u8> },
    /// Guardian → child: request a fresh sealed snapshot.
    ActivityPull { link_id: String },
    /// Either side: revoke the link. `sealed_marker` must open to
    /// `b"revoke:<link_id>"` under the link key (proves key possession).
    Revoke {
        link_id: String,
        sealed_marker: Vec<u8>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GuardianResponse {
    /// Link established; carries the child's initial sealed snapshot.
    Linked {
        sealed_snapshot: Vec<u8>,
    },
    /// Push applied; how many entity rows merged.
    Merged {
        rows: i64,
    },
    /// Pull result: a sealed [`GuardianActivityPayload`].
    Sealed {
        sealed: Vec<u8>,
    },
    Unauthorized,
    Error(String),
}

// ── sealing helpers (generic over the payload type) ─────────────────

pub fn seal<T: Serialize>(key: &[u8; 32], value: &T) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(value).map_err(|e| format!("serialize: {e}"))?;
    crate::crypto::content_crypto::encrypt(key, &json).map_err(|e| format!("seal: {e}"))
}

pub fn open<T: DeserializeOwned>(key: &[u8; 32], sealed: &[u8]) -> Result<T, String> {
    let plaintext = crate::crypto::content_crypto::decrypt(key, sealed)
        .map_err(|e| format!("open: {e}"))?
        .ok_or_else(|| "payload not encrypted / wrong key".to_string())?;
    serde_json::from_slice(&plaintext).map_err(|e| format!("parse: {e}"))
}

// ── link persistence ────────────────────────────────────────────────

pub fn record_pending_invite(
    conn: &Connection,
    code_hash: &str,
    shared_key: &[u8; 32],
    ttl_secs: i64,
) -> Result<(), String> {
    conn.execute(
        "DELETE FROM guardian_pending_invites WHERE expires_at < datetime('now')",
        [],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO guardian_pending_invites (code_hash, shared_key, expires_at) \
         VALUES (?1, ?2, datetime('now', ?3))",
        rusqlite::params![code_hash, &shared_key[..], format!("{ttl_secs} seconds")],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Consume a still-valid pending invite (single use).
pub fn take_pending_invite(conn: &Connection, code_hash: &str) -> Result<Option<[u8; 32]>, String> {
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT shared_key FROM guardian_pending_invites \
             WHERE code_hash = ?1 AND expires_at >= datetime('now')",
            rusqlite::params![code_hash],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM guardian_pending_invites WHERE code_hash = ?1",
        rusqlite::params![code_hash],
    )
    .map_err(|e| e.to_string())?;
    match blob {
        Some(b) if b.len() == 32 => {
            let mut key = [0u8; 32];
            key.copy_from_slice(&b);
            Ok(Some(key))
        }
        _ => Ok(None),
    }
}

/// Fetch a link's shared key, constrained to the expected side and a
/// non-revoked status.
pub fn get_link_key(
    conn: &Connection,
    link_id: &str,
    side: &str,
) -> Result<Option<[u8; 32]>, String> {
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT shared_key FROM guardian_links \
             WHERE id = ?1 AND side = ?2 AND status != 'revoked'",
            rusqlite::params![link_id, side],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match blob {
        Some(b) if b.len() == 32 => {
            let mut key = [0u8; 32];
            key.copy_from_slice(&b);
            Ok(Some(key))
        }
        Some(_) => Err("stored guardian key is not 32 bytes".into()),
        None => Ok(None),
    }
}

// ── snapshot build / apply ──────────────────────────────────────────

/// Read one local-DID-adjacent identity fields + serialise every
/// guardian-synced table into activity rows.
pub fn build_activity_snapshot(conn: &Connection) -> Result<GuardianActivityPayload, String> {
    let (display_name, birthdate): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT display_name, birthdate FROM local_identity WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or((None, None));
    let child_did: String = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'identity.local_did'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();

    let mut tables = Vec::with_capacity(GUARDIAN_SYNC_TABLES.len());
    for table in GUARDIAN_SYNC_TABLES {
        tables.push((table.to_string(), table_activity_rows(conn, table)?));
    }
    Ok(GuardianActivityPayload {
        child_did,
        display_name,
        birthdate,
        tables,
    })
}

/// Serialise every row of one guardian-synced table. The table name is
/// checked against [`GUARDIAN_SYNC_TABLES`] (never interpolate caller
/// input).
fn table_activity_rows(conn: &Connection, table: &str) -> Result<Vec<ActivityRow>, String> {
    use rusqlite::types::ValueRef;
    if !GUARDIAN_SYNC_TABLES.contains(&table) {
        return Err(format!("table '{table}' is not guardian-synced"));
    }
    let mut stmt = conn
        .prepare(&format!("SELECT * FROM {table}"))
        .map_err(|e| e.to_string())?;
    let col_names: Vec<String> = stmt
        .column_names()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let rows = stmt
        .query_map([], |row| {
            let mut obj = serde_json::Map::new();
            for (i, name) in col_names.iter().enumerate() {
                let v = match row.get_ref(i)? {
                    ValueRef::Null => serde_json::Value::Null,
                    ValueRef::Integer(n) => serde_json::Value::from(n),
                    ValueRef::Real(f) => serde_json::Value::from(f),
                    ValueRef::Text(t) => {
                        serde_json::Value::from(String::from_utf8_lossy(t).to_string())
                    }
                    ValueRef::Blob(b) => {
                        use base64::Engine;
                        serde_json::Value::from(base64::engine::general_purpose::STANDARD.encode(b))
                    }
                };
                obj.insert(name.clone(), v);
            }
            let entity_id = match obj.get("id").and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                // classroom_members has a composite key.
                None => {
                    let a = obj
                        .get("classroom_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let b = obj
                        .get("stake_address")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    format!("{a}:{b}")
                }
            };
            let updated_at = obj
                .get("updated_at")
                .or_else(|| obj.get("created_at"))
                .or_else(|| obj.get("enrolled_at"))
                .or_else(|| obj.get("joined_at"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Ok(ActivityRow {
                entity_id,
                data: serde_json::Value::Object(obj),
                updated_at,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Merge an inbound snapshot into `guardian_activity_rows` (LWW) and
/// refresh the link's cached child metadata. Returns rows merged.
pub fn apply_activity_snapshot(
    conn: &Connection,
    link_id: &str,
    payload: &GuardianActivityPayload,
) -> Result<i64, String> {
    let mut merged = 0i64;
    for (table, rows) in &payload.tables {
        if !GUARDIAN_SYNC_TABLES.contains(&table.as_str()) {
            log::warn!("guardian: ignoring non-synced table '{table}' from ward");
            continue;
        }
        for row in rows {
            let data = serde_json::to_string(&row.data).map_err(|e| e.to_string())?;
            let changed = conn
                .execute(
                    "INSERT INTO guardian_activity_rows (link_id, table_name, entity_id, payload_json, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5) \
                     ON CONFLICT(link_id, table_name, entity_id) DO UPDATE SET \
                         payload_json = excluded.payload_json, \
                         updated_at = excluded.updated_at \
                     WHERE excluded.updated_at >= guardian_activity_rows.updated_at",
                    rusqlite::params![link_id, table, row.entity_id, data, row.updated_at],
                )
                .map_err(|e| e.to_string())?;
            merged += changed as i64;
        }
    }
    conn.execute(
        "UPDATE guardian_links SET \
             child_birthdate = COALESCE(?2, child_birthdate), \
             peer_display_name = COALESCE(?3, peer_display_name), \
             peer_did = CASE WHEN ?4 != '' THEN ?4 ELSE peer_did END, \
             last_sync_at = datetime('now'), updated_at = datetime('now') \
         WHERE id = ?1",
        rusqlite::params![
            link_id,
            payload.birthdate,
            payload.display_name,
            payload.child_did
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(merged)
}

// ── the protocol handler (pure function; unit-testable) ─────────────

/// Handle an inbound guardian request from `peer_id` (authenticated by
/// the transport).
pub fn handle_guardian_request(
    conn: &Connection,
    peer_id: &str,
    req: &GuardianRequest,
) -> GuardianResponse {
    match req {
        GuardianRequest::Link {
            code_hash,
            link_id,
            guardian_did,
            guardian_stake_address,
            guardian_display_name,
            guardian_vc_json,
        } => handle_link(
            conn,
            peer_id,
            code_hash,
            link_id,
            guardian_did,
            guardian_stake_address,
            guardian_display_name.as_deref(),
            guardian_vc_json,
        ),
        GuardianRequest::ActivityPush { link_id, sealed } => {
            let key = match get_link_key(conn, link_id, "guardian") {
                Ok(Some(k)) => k,
                Ok(None) => return GuardianResponse::Unauthorized,
                Err(e) => return GuardianResponse::Error(e),
            };
            let payload: GuardianActivityPayload = match open(&key, sealed) {
                Ok(p) => p,
                Err(e) => return GuardianResponse::Error(format!("open push: {e}")),
            };
            match apply_activity_snapshot(conn, link_id, &payload) {
                Ok(rows) => GuardianResponse::Merged { rows },
                Err(e) => GuardianResponse::Error(format!("apply push: {e}")),
            }
        }
        GuardianRequest::ActivityPull { link_id } => {
            let key = match get_link_key(conn, link_id, "ward") {
                Ok(Some(k)) => k,
                Ok(None) => return GuardianResponse::Unauthorized,
                Err(e) => return GuardianResponse::Error(e),
            };
            let snapshot = match build_activity_snapshot(conn) {
                Ok(s) => s,
                Err(e) => return GuardianResponse::Error(format!("snapshot: {e}")),
            };
            match seal(&key, &snapshot) {
                Ok(sealed) => {
                    let _ = conn.execute(
                        "UPDATE guardian_links SET last_sync_at = datetime('now') WHERE id = ?1",
                        rusqlite::params![link_id],
                    );
                    GuardianResponse::Sealed { sealed }
                }
                Err(e) => GuardianResponse::Error(format!("seal: {e}")),
            }
        }
        GuardianRequest::Revoke {
            link_id,
            sealed_marker,
        } => handle_revoke(conn, link_id, sealed_marker),
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_link(
    conn: &Connection,
    peer_id: &str,
    code_hash: &str,
    link_id: &str,
    guardian_did: &str,
    guardian_stake_address: &str,
    guardian_display_name: Option<&str>,
    guardian_vc_json: &str,
) -> GuardianResponse {
    // (2) Single-use invite: only honour a code this profile generated.
    let key = match take_pending_invite(conn, code_hash) {
        Ok(Some(k)) => k,
        Ok(None) => return GuardianResponse::Unauthorized,
        Err(e) => return GuardianResponse::Error(e),
    };

    // Verify the guardianship credential: signature must check out,
    // issuer must be the claimed guardian, subject must be us.
    let vc: VerifiableCredential = match serde_json::from_str(guardian_vc_json) {
        Ok(v) => v,
        Err(e) => return GuardianResponse::Error(format!("bad guardian VC: {e}")),
    };
    if vc.issuer.as_str() != guardian_did {
        return GuardianResponse::Error("guardian VC issuer mismatch".into());
    }
    let local_did: String = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'identity.local_did'",
            [],
            |r| r.get(0),
        )
        .optional()
        .unwrap_or_default()
        .unwrap_or_default();
    if !local_did.is_empty() && vc.credential_subject.id.as_str() != local_did {
        return GuardianResponse::Error("guardian VC subject is not this profile".into());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let policy = VerificationPolicy {
        reject_expired: true,
        require_integrity_anchor: false,
        allowed_types: vec![],
        reject_suspended: true,
        reject_superseded: true,
    };
    let result = verify_credential(conn, &vc, &now, &policy);
    if !result.valid_signature || !result.issuer_resolved {
        return GuardianResponse::Error("guardian VC signature verification failed".into());
    }

    // Store the credential locally (private by default — the VC-fetch
    // layer only serves it to the subject/allowlisted peers).
    let vc_id = vc
        .id
        .clone()
        .unwrap_or_else(|| format!("urn:guardian:{link_id}"));
    let integrity_hash = hex::encode(crate::crypto::hash::blake2b_256(
        guardian_vc_json.as_bytes(),
    ));
    if let Err(e) = conn.execute(
        "INSERT OR REPLACE INTO credentials \
         (id, issuer_did, subject_did, credential_type, claim_kind, \
          issuance_date, signed_vc_json, integrity_hash) \
         VALUES (?1, ?2, ?3, 'RoleCredential', 'role', ?4, ?5, ?6)",
        rusqlite::params![
            vc_id,
            guardian_did,
            vc.credential_subject.id.as_str(),
            vc.valid_from,
            guardian_vc_json,
            integrity_hash
        ],
    ) {
        return GuardianResponse::Error(format!("store guardian VC: {e}"));
    }

    // Record the ward-side link and unlock the profile.
    if let Err(e) = conn.execute(
        "INSERT OR REPLACE INTO guardian_links \
         (id, side, peer_did, peer_stake_address, peer_peer_id, peer_display_name, \
          shared_key, status, guardian_vc_id) \
         VALUES (?1, 'ward', ?2, ?3, ?4, ?5, ?6, 'active', ?7)",
        rusqlite::params![
            link_id,
            guardian_did,
            guardian_stake_address,
            peer_id,
            guardian_display_name,
            &key[..],
            vc_id
        ],
    ) {
        return GuardianResponse::Error(format!("store link: {e}"));
    }
    if let Err(e) = conn.execute(
        "UPDATE local_identity SET activation_state = 'active', updated_at = datetime('now') \
         WHERE id = 1 AND activation_state = 'pending_guardian'",
        [],
    ) {
        return GuardianResponse::Error(format!("activate profile: {e}"));
    }

    // Reply with the initial sealed snapshot.
    let snapshot = match build_activity_snapshot(conn) {
        Ok(s) => s,
        Err(e) => return GuardianResponse::Error(format!("snapshot: {e}")),
    };
    match seal(&key, &snapshot) {
        Ok(sealed_snapshot) => GuardianResponse::Linked { sealed_snapshot },
        Err(e) => GuardianResponse::Error(format!("seal: {e}")),
    }
}

fn handle_revoke(conn: &Connection, link_id: &str, sealed_marker: &[u8]) -> GuardianResponse {
    // Either side may receive a revoke; find the link whatever side we hold.
    let key = match get_link_key(conn, link_id, "ward").and_then(|k| match k {
        Some(k) => Ok(Some(k)),
        None => get_link_key(conn, link_id, "guardian"),
    }) {
        Ok(Some(k)) => k,
        Ok(None) => return GuardianResponse::Unauthorized,
        Err(e) => return GuardianResponse::Error(e),
    };
    let marker: String = match open(&key, sealed_marker) {
        Ok(m) => m,
        Err(e) => return GuardianResponse::Error(format!("open revoke: {e}")),
    };
    if marker != format!("revoke:{link_id}") {
        return GuardianResponse::Unauthorized;
    }

    let side: Option<String> = conn
        .query_row(
            "SELECT side FROM guardian_links WHERE id = ?1",
            rusqlite::params![link_id],
            |r| r.get(0),
        )
        .optional()
        .unwrap_or(None);
    if let Err(e) = conn.execute(
        "UPDATE guardian_links SET status = 'revoked', updated_at = datetime('now') WHERE id = ?1",
        rusqlite::params![link_id],
    ) {
        return GuardianResponse::Error(format!("revoke link: {e}"));
    }

    // Guardian revoked a still-minor ward → the gate comes back.
    if side.as_deref() == Some("ward") {
        let birthdate: Option<String> = conn
            .query_row(
                "SELECT birthdate FROM local_identity WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .unwrap_or(None)
            .flatten();
        let still_minor = birthdate
            .as_deref()
            .map(|b| crate::domain::identity::is_minor(b, chrono::Utc::now().date_naive()))
            .unwrap_or(false);
        let has_other_active: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM guardian_links \
                 WHERE side = 'ward' AND status = 'active' AND id != ?1",
                rusqlite::params![link_id],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if still_minor && !has_other_active {
            let _ = conn.execute(
                "UPDATE local_identity SET activation_state = 'pending_guardian', \
                 updated_at = datetime('now') WHERE id = 1",
                [],
            );
        }
    }
    GuardianResponse::Merged { rows: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    fn seed_identity(conn: &Connection, birthdate: Option<&str>, activation: &str) {
        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address, account_role, birthdate, activation_state) \
             VALUES (1, 'stake_child', 'addr_child', 'learner', ?1, ?2)",
            rusqlite::params![birthdate, activation],
        )
        .unwrap();
    }

    fn seed_guardian_link(conn: &Connection, link_id: &str, side: &str, key: [u8; 32]) {
        conn.execute(
            "INSERT INTO guardian_links (id, side, peer_did, shared_key, status) \
             VALUES (?1, ?2, 'did:key:zPeer', ?3, 'active')",
            rusqlite::params![link_id, side, &key[..]],
        )
        .unwrap();
    }

    /// Privacy invariant: guardian data must never ride device sync.
    /// If someone adds these tables to SYNCABLE_TABLES, this fails and
    /// forces a deliberate decision.
    #[test]
    fn guardian_tables_never_device_synced() {
        for t in [
            "guardian_links",
            "guardian_pending_invites",
            "guardian_activity_rows",
        ] {
            assert!(
                !crate::domain::sync::SYNCABLE_TABLES.contains(&t),
                "guardian table `{t}` must not be device-synced"
            );
        }
    }

    #[test]
    fn link_without_pending_invite_is_unauthorized() {
        let db = test_db();
        seed_identity(db.conn(), Some("2012-01-01"), "pending_guardian");
        let req = GuardianRequest::Link {
            code_hash: "nope".into(),
            link_id: "l1".into(),
            guardian_did: "did:key:zParent".into(),
            guardian_stake_address: "stake_parent".into(),
            guardian_display_name: None,
            guardian_vc_json: "{}".into(),
        };
        assert!(matches!(
            handle_guardian_request(db.conn(), "12D3KooWParent", &req),
            GuardianResponse::Unauthorized
        ));
    }

    #[test]
    fn invite_is_single_use() {
        let db = test_db();
        seed_identity(db.conn(), Some("2012-01-01"), "pending_guardian");
        record_pending_invite(db.conn(), "hash1", &[7u8; 32], 300).unwrap();

        assert_eq!(
            take_pending_invite(db.conn(), "hash1").unwrap(),
            Some([7u8; 32])
        );
        // Consumed — second take fails.
        assert_eq!(take_pending_invite(db.conn(), "hash1").unwrap(), None);
    }

    #[test]
    fn expired_invite_is_rejected() {
        let db = test_db();
        seed_identity(db.conn(), Some("2012-01-01"), "pending_guardian");
        record_pending_invite(db.conn(), "hash2", &[7u8; 32], -10).unwrap();
        assert_eq!(take_pending_invite(db.conn(), "hash2").unwrap(), None);
    }

    #[test]
    fn push_with_wrong_key_errors_and_unknown_link_unauthorized() {
        let db = test_db();
        seed_identity(db.conn(), None, "active");
        let key = [3u8; 32];
        seed_guardian_link(db.conn(), "l1", "guardian", key);

        // Unknown link.
        let req = GuardianRequest::ActivityPush {
            link_id: "does-not-exist".into(),
            sealed: vec![1, 2, 3],
        };
        assert!(matches!(
            handle_guardian_request(db.conn(), "12D3KooWChild", &req),
            GuardianResponse::Unauthorized
        ));

        // Wrong key / tampered ciphertext.
        let wrong = seal(&[9u8; 32], &GuardianActivityPayload::default()).unwrap();
        let req = GuardianRequest::ActivityPush {
            link_id: "l1".into(),
            sealed: wrong,
        };
        assert!(matches!(
            handle_guardian_request(db.conn(), "12D3KooWChild", &req),
            GuardianResponse::Error(_)
        ));
    }

    #[test]
    fn push_merges_lww_rows() {
        let db = test_db();
        seed_identity(db.conn(), None, "active");
        let key = [4u8; 32];
        seed_guardian_link(db.conn(), "l1", "guardian", key);

        let payload = GuardianActivityPayload {
            child_did: "did:key:zChild".into(),
            display_name: Some("Ada".into()),
            birthdate: Some("2012-05-01".into()),
            tables: vec![(
                "enrollments".into(),
                vec![ActivityRow {
                    entity_id: "en1".into(),
                    data: serde_json::json!({"id": "en1", "course_id": "c1", "status": "active"}),
                    updated_at: "2026-07-01T00:00:00Z".into(),
                }],
            )],
        };
        let sealed = seal(&key, &payload).unwrap();
        let resp = handle_guardian_request(
            db.conn(),
            "12D3KooWChild",
            &GuardianRequest::ActivityPush {
                link_id: "l1".into(),
                sealed,
            },
        );
        assert!(matches!(resp, GuardianResponse::Merged { rows: 1 }));

        // Older row does not clobber newer.
        let older = GuardianActivityPayload {
            tables: vec![(
                "enrollments".into(),
                vec![ActivityRow {
                    entity_id: "en1".into(),
                    data: serde_json::json!({"id": "en1", "status": "stale"}),
                    updated_at: "2020-01-01T00:00:00Z".into(),
                }],
            )],
            ..payload.clone()
        };
        let sealed = seal(&key, &older).unwrap();
        let _ = handle_guardian_request(
            db.conn(),
            "12D3KooWChild",
            &GuardianRequest::ActivityPush {
                link_id: "l1".into(),
                sealed,
            },
        );
        let stored: String = db
            .conn()
            .query_row(
                "SELECT payload_json FROM guardian_activity_rows WHERE entity_id = 'en1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(stored.contains("\"active\""), "older write must not win");

        // Cached child metadata refreshed.
        let bd: Option<String> = db
            .conn()
            .query_row(
                "SELECT child_birthdate FROM guardian_links WHERE id = 'l1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bd.as_deref(), Some("2012-05-01"));
    }

    #[test]
    fn pull_returns_sealed_snapshot_openable_with_link_key() {
        let db = test_db();
        seed_identity(db.conn(), Some("2012-01-01"), "active");
        let key = [5u8; 32];
        seed_guardian_link(db.conn(), "l1", "ward", key);
        // FK order: the course row must exist before the enrollment.
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) VALUES ('c1', 'C', 'stake_x')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO enrollments (id, course_id, status) VALUES ('en1', 'c1', 'active')",
                [],
            )
            .unwrap();

        let resp = handle_guardian_request(
            db.conn(),
            "12D3KooWParent",
            &GuardianRequest::ActivityPull {
                link_id: "l1".into(),
            },
        );
        let GuardianResponse::Sealed { sealed } = resp else {
            panic!("expected Sealed, got {resp:?}");
        };
        let snapshot: GuardianActivityPayload = open(&key, &sealed).unwrap();
        assert_eq!(snapshot.birthdate.as_deref(), Some("2012-01-01"));
        let enrollments = snapshot
            .tables
            .iter()
            .find(|(t, _)| t == "enrollments")
            .unwrap();
        assert_eq!(enrollments.1.len(), 1);
        assert_eq!(enrollments.1[0].entity_id, "en1");
    }

    #[test]
    fn revoke_requires_key_possession_and_regates_minor_ward() {
        let db = test_db();
        seed_identity(db.conn(), Some("2012-01-01"), "active");
        let key = [6u8; 32];
        seed_guardian_link(db.conn(), "l1", "ward", key);

        // Bad marker (sealed under the wrong key) → rejected, link intact.
        let bad = seal(&[1u8; 32], &"revoke:l1".to_string()).unwrap();
        assert!(matches!(
            handle_guardian_request(
                db.conn(),
                "12D3KooWParent",
                &GuardianRequest::Revoke {
                    link_id: "l1".into(),
                    sealed_marker: bad,
                }
            ),
            GuardianResponse::Error(_)
        ));

        // Correct marker → revoked, and the still-minor ward re-gates.
        let good = seal(&key, &"revoke:l1".to_string()).unwrap();
        assert!(matches!(
            handle_guardian_request(
                db.conn(),
                "12D3KooWParent",
                &GuardianRequest::Revoke {
                    link_id: "l1".into(),
                    sealed_marker: good,
                }
            ),
            GuardianResponse::Merged { .. }
        ));
        let (status, activation): (String, String) = db
            .conn()
            .query_row(
                "SELECT gl.status, li.activation_state \
                 FROM guardian_links gl, local_identity li WHERE gl.id = 'l1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "revoked");
        assert_eq!(activation, "pending_guardian");
    }

    #[test]
    fn revoke_leaves_adult_ward_active() {
        let db = test_db();
        // Ward is 18+ now.
        seed_identity(db.conn(), Some("2000-01-01"), "active");
        let key = [8u8; 32];
        seed_guardian_link(db.conn(), "l2", "ward", key);

        let good = seal(&key, &"revoke:l2".to_string()).unwrap();
        let _ = handle_guardian_request(
            db.conn(),
            "12D3KooWParent",
            &GuardianRequest::Revoke {
                link_id: "l2".into(),
                sealed_marker: good,
            },
        );
        let activation: String = db
            .conn()
            .query_row(
                "SELECT activation_state FROM local_identity WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(activation, "active");
    }
}
