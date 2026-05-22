//! Device-to-device sync over libp2p request-response
//! (`/alexandria/sync/1.0`).
//!
//! A paired device sends a [`SyncRequest`] carrying its own sealed
//! [`SyncPayload`](super::sync::SyncPayload); the responder applies it,
//! then replies with its own sealed payload. Both directions merge via
//! last-writer-wins, so a single round trip reconciles the pair.
//!
//! Authentication chain:
//!   1. The requesting peer's libp2p `PeerId` is authenticated by the
//!      Noise transport handshake — it cannot be spoofed.
//!   2. We look up the 32-byte `shared_key` stored for that PeerId at
//!      pairing time. No pairing row → [`SyncResponse::Unauthorized`].
//!   3. The payload is sealed with that key (AES-256-GCM); a peer that
//!      isn't the paired device cannot produce ciphertext that opens,
//!      and cannot read our reply.
//!   4. As a secondary guard the request's `stake_address` must match
//!      the local identity's — sync never crosses users.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::sync;

/// One-time pairing-completion handshake the acceptor attaches to its
/// *first* sync request, so the initiator can finish the two-way pair.
/// The initiator generated the code; the acceptor proves possession by
/// echoing its `code_hash`, and supplies its own device metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingHandshake {
    /// Hash of the pairing code the initiator generated.
    pub code_hash: String,
    /// Acceptor's device label, for display on the initiator.
    pub device_name: Option<String>,
    /// Acceptor's platform.
    pub platform: String,
}

/// Inbound sync request: the sender's sealed payload plus the metadata
/// needed to authorise the exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    /// Sender's stable device id (`devices.id`).
    pub device_id: String,
    /// Sender's owning stake address — must match ours.
    pub stake_address: String,
    /// The sender's [`SyncPayload`](super::sync::SyncPayload) sealed
    /// under the pair's shared key.
    pub sealed: Vec<u8>,
    /// Present only on the acceptor's first contact; completes pairing.
    #[serde(default)]
    pub pairing: Option<PairingHandshake>,
}

/// Response to a [`SyncRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncResponse {
    /// Accepted. Carries the responder's own sealed payload for the
    /// requester to merge, plus how many rows the responder merged.
    Ok { sealed: Vec<u8>, merged: i64 },
    /// The requester is not a paired device of this user.
    Unauthorized,
    /// The request was authorised but could not be processed.
    Error(String),
}

/// Handle an inbound sync request from `peer_id` (the libp2p PeerId,
/// authenticated by the transport). Applies the sender's payload and
/// returns our own sealed payload on success.
pub fn handle_sync_request(conn: &Connection, peer_id: &str, req: &SyncRequest) -> SyncResponse {
    // (4) Same-user guard.
    match sync::local_stake_address(conn) {
        Ok(Some(local)) if local == req.stake_address => {}
        Ok(_) => return SyncResponse::Unauthorized,
        Err(e) => return SyncResponse::Error(e),
    }

    // (2) Must be a paired peer — only then do we hold a shared key.
    //     If we don't yet, this may be the acceptor's first contact:
    //     a valid pairing handshake (proving possession of a code we
    //     generated) completes the pair using that code's key.
    let key = match sync::get_pair_key(conn, peer_id) {
        Ok(Some(k)) => k,
        Ok(None) => match complete_inbound_pairing(conn, peer_id, req) {
            Ok(Some(k)) => k,
            Ok(None) => return SyncResponse::Unauthorized,
            Err(e) => return SyncResponse::Error(e),
        },
        Err(e) => return SyncResponse::Error(e),
    };

    // (3) Open the sender's payload; AEAD failure ⇒ wrong key / tamper.
    let incoming = match sync::open_payload(&key, &req.sealed) {
        Ok(p) => p,
        Err(e) => return SyncResponse::Error(format!("open: {e}")),
    };

    let merged = match sync::apply_sync_payload(conn, &incoming) {
        Ok((rows, _settings)) => rows,
        Err(e) => return SyncResponse::Error(format!("apply: {e}")),
    };

    // Reply with our own state so the exchange is bidirectional.
    let outbound = match sync::build_sync_payload(conn) {
        Ok(p) => p,
        Err(e) => return SyncResponse::Error(format!("build: {e}")),
    };
    match sync::seal_payload(&key, &outbound) {
        Ok(sealed) => SyncResponse::Ok { sealed, merged },
        Err(e) => SyncResponse::Error(format!("seal: {e}")),
    }
}

/// Complete the initiator side of a pairing on the acceptor's first
/// contact. Returns the now-shared key on success, or `None` if there
/// is no valid pairing handshake to honour. The stake-address match
/// has already been verified by the caller.
fn complete_inbound_pairing(
    conn: &Connection,
    peer_id: &str,
    req: &SyncRequest,
) -> Result<Option<[u8; 32]>, String> {
    let Some(handshake) = req.pairing.as_ref() else {
        return Ok(None);
    };
    // Single-use: only honour a code this device actually generated.
    let Some(key) = sync::take_pending_pairing(conn, &handshake.code_hash)? else {
        return Ok(None);
    };
    // Record the acceptor as a paired device under the same key. Its
    // dial addresses aren't in the handshake — the live connection and
    // peer-exchange will supply them.
    let code = crate::crypto::pairing::PairingCode {
        peer_id: peer_id.to_string(),
        addresses: vec![],
        shared_key: key,
        stake_address: req.stake_address.clone(),
        device_id: req.device_id.clone(),
        device_name: handshake.device_name.clone(),
        platform: handshake.platform.clone(),
    };
    sync::complete_pairing(conn, &code)?;
    Ok(Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::pairing::PairingCode;
    use crate::db::Database;

    fn seed_identity(conn: &Connection, stake: &str) {
        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, ?1, 'addr_test1q')",
            rusqlite::params![stake],
        )
        .unwrap();
    }

    fn pair_a_device(conn: &Connection, peer_id: &str, stake: &str, key: [u8; 32]) {
        let code = PairingCode {
            peer_id: peer_id.into(),
            addresses: vec![],
            shared_key: key,
            stake_address: stake.into(),
            device_id: "dev-remote".into(),
            device_name: Some("Phone".into()),
            platform: "android".into(),
        };
        sync::complete_pairing(conn, &code).unwrap();
    }

    #[test]
    fn unpaired_peer_is_unauthorized() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_identity(db.conn(), "stake_test1uself");
        let req = SyncRequest {
            device_id: "dev-x".into(),
            stake_address: "stake_test1uself".into(),
            sealed: vec![1, 2, 3],
            pairing: None,
        };
        assert!(matches!(
            handle_sync_request(db.conn(), "12D3KooWStranger", &req),
            SyncResponse::Unauthorized
        ));
    }

    #[test]
    fn wrong_stake_address_is_unauthorized() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_identity(db.conn(), "stake_test1uself");
        let key = [9u8; 32];
        pair_a_device(db.conn(), "12D3KooWPeer", "stake_test1uself", key);
        let req = SyncRequest {
            device_id: "dev-remote".into(),
            stake_address: "stake_test1uOTHER".into(), // different user
            sealed: vec![1, 2, 3],
            pairing: None,
        };
        assert!(matches!(
            handle_sync_request(db.conn(), "12D3KooWPeer", &req),
            SyncResponse::Unauthorized
        ));
    }

    #[test]
    fn paired_peer_round_trips_and_applies() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_identity(db.conn(), "stake_test1uself");
        let key = [3u8; 32];
        pair_a_device(db.conn(), "12D3KooWPeer", "stake_test1uself", key);

        // The peer offers one settings row.
        let payload = sync::SyncPayload {
            settings: vec![sync::SettingsSyncRow {
                key: "theme".into(),
                value: "dark".into(),
                updated_at: "2099-01-01T00:00:00Z".into(),
            }],
            tables: vec![],
        };
        let sealed = sync::seal_payload(&key, &payload).unwrap();
        let req = SyncRequest {
            device_id: "dev-remote".into(),
            stake_address: "stake_test1uself".into(),
            sealed,
            pairing: None,
        };
        let resp = handle_sync_request(db.conn(), "12D3KooWPeer", &req);
        match resp {
            SyncResponse::Ok { sealed, .. } => {
                // The reply must open under the same key.
                let back = sync::open_payload(&key, &sealed).unwrap();
                // Our reply is a valid (possibly empty) snapshot.
                let _ = sync::payload_row_count(&back);
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn tampered_payload_under_valid_pair_errors() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_identity(db.conn(), "stake_test1uself");
        pair_a_device(db.conn(), "12D3KooWPeer", "stake_test1uself", [4u8; 32]);
        let req = SyncRequest {
            device_id: "dev-remote".into(),
            stake_address: "stake_test1uself".into(),
            sealed: vec![0x01, 0xde, 0xad, 0xbe, 0xef], // not openable
            pairing: None,
        };
        assert!(matches!(
            handle_sync_request(db.conn(), "12D3KooWPeer", &req),
            SyncResponse::Error(_)
        ));
    }

    #[test]
    fn acceptor_first_contact_completes_pairing() {
        // Initiator generated a code (key recorded as pending). The
        // acceptor's first request carries the matching handshake and a
        // payload sealed with that key — the initiator must complete the
        // pair and apply the payload.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_identity(db.conn(), "stake_test1uself");
        let key = [5u8; 32];
        let code_hash = "deadbeefcafe";
        sync::record_pending_pairing(db.conn(), code_hash, &key, 300).unwrap();

        let payload = sync::SyncPayload {
            settings: vec![sync::SettingsSyncRow {
                key: "theme".into(),
                value: "light".into(),
                updated_at: "2099-01-01T00:00:00Z".into(),
            }],
            tables: vec![],
        };
        let req = SyncRequest {
            device_id: "dev-acceptor".into(),
            stake_address: "stake_test1uself".into(),
            sealed: sync::seal_payload(&key, &payload).unwrap(),
            pairing: Some(PairingHandshake {
                code_hash: code_hash.into(),
                device_name: Some("Acceptor".into()),
                platform: "ios".into(),
            }),
        };
        let resp = handle_sync_request(db.conn(), "12D3KooWAcceptor", &req);
        assert!(matches!(resp, SyncResponse::Ok { .. }), "got {resp:?}");

        // The acceptor is now a stored paired device with the shared key.
        let stored = sync::get_pair_key(db.conn(), "12D3KooWAcceptor").unwrap();
        assert_eq!(stored, Some(key));
        // The pending code was consumed (single use).
        let req2 = SyncRequest {
            pairing: Some(PairingHandshake {
                code_hash: code_hash.into(),
                device_name: None,
                platform: "ios".into(),
            }),
            ..req.clone()
        };
        // A *different* unpaired peer reusing the code now fails.
        assert!(matches!(
            handle_sync_request(db.conn(), "12D3KooWReplay", &req2),
            SyncResponse::Unauthorized
        ));
    }
}
