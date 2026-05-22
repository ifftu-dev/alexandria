//! Device-to-device settings-sync end-to-end test.
//!
//! Models two of a user's devices — "A" (desktop) and "B" (phone) —
//! that share one identity and have completed an explicit pairing.
//! It drives the *production* sync path: A builds its outbound payload
//! from its real settings store, seals it under the pair's shared key,
//! and B ingests it through `device_sync::handle_sync_request` exactly
//! as it would a payload arriving over libp2p. The transport layer
//! (`/alexandria/sync/1.0`) is a transparent carrier for these sealed
//! bytes, so exercising the seal → handle → apply path here verifies
//! the cross-device behaviour without standing up two live swarms.
//!
//! Covers:
//!   - a `Scope::Sync` setting set on A reaches B,
//!   - a `Scope::Device` setting set on A never leaves A,
//!   - last-writer-wins: a stale inbound row does not clobber a newer
//!     local value on B.

use app_lib::db::Database;
use app_lib::p2p::device_sync::{handle_sync_request, SyncRequest, SyncResponse};
use app_lib::p2p::sync;
use app_lib::settings::SettingsStore;

use rusqlite::Connection;

const STAKE: &str = "stake_test1uself";
const PEER_A: &str = "12D3KooWDesktopA";
const SHARED_KEY: [u8; 32] = [7u8; 32];

// A registered `Scope::Sync` key and a registered `Scope::Device` key
// (see `settings/registry.rs`).
const SYNC_KEY: &str = "ui.theme";
const DEVICE_KEY: &str = "cardano.blockfrost_project_id";

fn device() -> Database {
    let db = Database::open_in_memory().expect("in-memory db");
    db.run_migrations().expect("migrations");
    db.conn()
        .execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, ?1, 'addr_test1q')",
            rusqlite::params![STAKE],
        )
        .expect("seed identity");
    db
}

/// Record device A as a paired peer of the local device, under the
/// shared key — the same row `pairing_accept_code` would write.
fn pair_with_a(conn: &Connection) {
    let code = app_lib::crypto::pairing::PairingCode {
        peer_id: PEER_A.into(),
        addresses: vec![],
        shared_key: SHARED_KEY,
        stake_address: STAKE.into(),
        device_id: "dev-desktop-a".into(),
        device_name: Some("Desktop".into()),
        platform: "macos".into(),
    };
    sync::complete_pairing(conn, &code).expect("complete pairing");
}

/// Deliver A's current sync state to B through the real request path,
/// returning how many rows B merged. Panics on any non-`Ok` response.
fn sync_a_to_b(a: &Connection, b: &Connection) -> i64 {
    let payload = sync::build_sync_payload(a).expect("A builds payload");
    let sealed = sync::seal_payload(&SHARED_KEY, &payload).expect("A seals payload");
    let req = SyncRequest {
        device_id: "dev-desktop-a".into(),
        stake_address: STAKE.into(),
        sealed,
        pairing: None,
    };
    match handle_sync_request(b, PEER_A, &req) {
        SyncResponse::Ok { merged, .. } => merged,
        other => panic!("expected Ok from B, got {other:?}"),
    }
}

fn value_of(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

#[test]
fn sync_scoped_setting_propagates_a_to_b() {
    let (a, b) = (device(), device());
    pair_with_a(b.conn());

    // A turns the theme dark.
    SettingsStore::set_raw(a.conn(), SYNC_KEY, "dark").expect("set on A");
    assert_eq!(value_of(a.conn(), SYNC_KEY).as_deref(), Some("dark"));
    assert_ne!(value_of(b.conn(), SYNC_KEY).as_deref(), Some("dark"));

    let merged = sync_a_to_b(a.conn(), b.conn());

    assert_eq!(
        merged, 1,
        "exactly the one theme setting should be merged on B"
    );
    assert_eq!(
        value_of(b.conn(), SYNC_KEY).as_deref(),
        Some("dark"),
        "sync-scoped setting must reach B"
    );
}

#[test]
fn device_scoped_setting_does_not_cross() {
    let (a, b) = (device(), device());
    pair_with_a(b.conn());

    // A sets a device-scoped secret (Blockfrost key).
    SettingsStore::set_raw(a.conn(), DEVICE_KEY, "preprodSECRET").expect("set on A");
    assert_eq!(
        value_of(a.conn(), DEVICE_KEY).as_deref(),
        Some("preprodSECRET")
    );

    // A device-scoped row must never appear in the outbound snapshot…
    let snapshot = sync::settings_outbound_snapshot(a.conn()).expect("snapshot");
    assert!(
        !snapshot.iter().any(|r| r.key == DEVICE_KEY),
        "device-scoped key leaked into outbound snapshot"
    );

    // …and a full sync must leave B without it.
    sync_a_to_b(a.conn(), b.conn());
    assert_eq!(
        value_of(b.conn(), DEVICE_KEY),
        None,
        "device-scoped setting must stay on A"
    );
}

#[test]
fn lww_stale_inbound_does_not_clobber_newer_local() {
    let (a, b) = (device(), device());
    pair_with_a(b.conn());

    // A wrote the theme long ago.
    SettingsStore::apply_sync_row(a.conn(), SYNC_KEY, "dark", "2000-01-01T00:00:00Z")
        .expect("seed old value on A");

    // B set it more recently to a different value.
    SettingsStore::apply_sync_row(b.conn(), SYNC_KEY, "light", "2099-01-01T00:00:00Z")
        .expect("seed newer value on B");

    let merged = sync_a_to_b(a.conn(), b.conn());

    assert_eq!(merged, 0, "stale inbound row must not be applied");
    assert_eq!(
        value_of(b.conn(), SYNC_KEY).as_deref(),
        Some("light"),
        "newer local value must win under LWW"
    );
}
