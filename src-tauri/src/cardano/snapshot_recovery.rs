//! Reputation snapshot records.
//!
//! A snapshot is a signed `DerivedCredential` whose canonical hash goes
//! through the ordinary credential-anchor queue; this module reads the
//! stored rows back.
//!
//! The earlier CIP-68 path — minting a soulbound reference/user token pair,
//! freezing its inputs and projecting a chain receipt onto the row — is
//! deleted. Rows it wrote are preserved and still list and read; they cannot
//! be rebuilt, resubmitted or recovered.

use rusqlite::Connection;

use crate::domain::reputation::SnapshotRecord;

pub fn record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotRecord> {
    Ok(SnapshotRecord {
        id: row.get(0)?,
        actor_address: row.get(1)?,
        subject_id: row.get(2)?,
        role: row.get(3)?,
        skill_count: row.get(4)?,
        tx_status: row.get(5)?,
        tx_hash: row.get(6)?,
        error_message: row.get(7)?,
        snapshot_at: row.get(8)?,
        confirmed_at: row.get(9)?,
        computation_spec: row.get(10)?,
        credential_id: row.get(11)?,
    })
}

pub fn record(conn: &Connection, id: &str) -> Result<SnapshotRecord, String> {
    conn.query_row(
        "SELECT rs.id, rs.actor_address, rs.subject_id, rs.role, rs.skill_count,
            CASE WHEN rs.credential_id IS NULL THEN rs.tx_status ELSE ca.anchor_status END,
            CASE WHEN rs.credential_id IS NULL THEN rs.tx_hash ELSE ca.anchor_tx_hash END,
            CASE WHEN rs.credential_id IS NULL THEN rs.error_message ELSE ca.last_error END,
            rs.snapshot_at,
            CASE WHEN rs.credential_id IS NULL THEN rs.confirmed_at ELSE ca.confirmed_at END,
            rs.computation_spec, rs.credential_id
         FROM reputation_snapshots rs
         LEFT JOIN credential_anchors ca ON ca.credential_id = rs.credential_id
         WHERE rs.id = ?1",
        [id],
        record_from_row,
    )
    .map_err(|e| format!("snapshot not found: {e}"))
}
