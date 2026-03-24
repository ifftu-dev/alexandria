//! Cross-device sync protocol.
//!
//! Implements private device-to-device synchronization over the P2P
//! network. Unlike gossip (which is public), sync messages are
//! encrypted with a key derived from the shared wallet mnemonic.
//!
//! Pairing model: both devices import the same BIP-39 mnemonic.
//! The sync key is derived via HKDF-SHA256 from the wallet's
//! Ed25519 signing key + salt "alexandria-cross-device-sync-v1".
//!
//! Merge strategies:
//!   - **LWW** (last-writer-wins by `updated_at`): enrollments,
//!     element_progress, course_notes
//!   - **Append-only union** (deduplicate by PK): evidence_records,
//!     skill_proof_evidence
//!   - **Derived** (not synced, recomputed): skill_proofs,
//!     reputation_assertions

use rusqlite::{params, Connection};

use crate::crypto::hash::{blake2b_256, entity_id};
use crate::domain::sync::{
    DeviceInfo, DeviceSyncSummary, SyncHistoryEntry, SyncQueueItem, SyncRow, SyncStatus,
    SyncTableState, SYNCABLE_TABLES,
};

/// HKDF salt for deriving the sync encryption key.
pub const SYNC_KEY_SALT: &str = "alexandria-cross-device-sync-v1";

/// Derive the 32-byte sync encryption key from a wallet signing key.
///
/// Uses HKDF-SHA256 (simplified as HMAC-SHA256 with a fixed salt)
/// to derive a deterministic key from the wallet's Ed25519 private
/// key bytes. Both devices with the same mnemonic will derive the
/// same key.
pub fn derive_sync_key(signing_key_bytes: &[u8]) -> [u8; 32] {
    // HKDF-extract: PRK = HMAC-SHA256(salt, IKM)
    // Simplified: we use blake2b with key material + salt concatenated
    let mut material = Vec::with_capacity(signing_key_bytes.len() + SYNC_KEY_SALT.len());
    material.extend_from_slice(signing_key_bytes);
    material.extend_from_slice(SYNC_KEY_SALT.as_bytes());
    blake2b_256(&material)
}

/// Register the local device in the database.
///
/// Called on first launch or when identity is created. Creates a
/// device record with `is_local = 1` and stores the device_id
/// in `local_identity`.
pub fn register_local_device(
    conn: &Connection,
    device_name: Option<&str>,
    platform: &str,
) -> Result<String, String> {
    let device_id = uuid::Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO devices (id, device_name, platform, is_local) \
         VALUES (?1, ?2, ?3, 1)",
        params![device_id, device_name, platform],
    )
    .map_err(|e| format!("failed to register device: {e}"))?;

    // Update local_identity with our device_id
    conn.execute(
        "UPDATE local_identity SET device_id = ?1 WHERE id = 1",
        params![device_id],
    )
    .map_err(|e| format!("failed to update local_identity device_id: {e}"))?;

    Ok(device_id)
}

/// Get the local device ID, registering if needed.
pub fn get_or_create_local_device(conn: &Connection, platform: &str) -> Result<String, String> {
    // Check if we already have a local device
    let existing: Option<String> = conn
        .query_row("SELECT id FROM devices WHERE is_local = 1", [], |row| {
            row.get(0)
        })
        .ok();

    if let Some(id) = existing {
        return Ok(id);
    }

    register_local_device(conn, None, platform)
}

/// Register a remote device discovered during sync.
pub fn register_remote_device(
    conn: &Connection,
    device_id: &str,
    device_name: Option<&str>,
    platform: Option<&str>,
    peer_id: Option<&str>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO devices (id, device_name, platform, is_local, peer_id) \
         VALUES (?1, ?2, ?3, 0, ?4) \
         ON CONFLICT(id) DO UPDATE SET \
         device_name = COALESCE(?2, devices.device_name), \
         platform = COALESCE(?3, devices.platform), \
         peer_id = COALESCE(?4, devices.peer_id), \
         last_synced = datetime('now')",
        params![device_id, device_name, platform, peer_id],
    )
    .map_err(|e| format!("failed to register remote device: {e}"))?;

    Ok(())
}

/// Get device info by ID.
pub fn get_device(conn: &Connection, device_id: &str) -> Result<DeviceInfo, String> {
    conn.query_row(
        "SELECT id, device_name, platform, first_seen, last_synced, is_local, peer_id \
         FROM devices WHERE id = ?1",
        params![device_id],
        |row| {
            Ok(DeviceInfo {
                id: row.get(0)?,
                device_name: row.get(1)?,
                platform: row.get(2)?,
                first_seen: row.get(3)?,
                last_synced: row.get(4)?,
                is_local: row.get::<_, i64>(5)? == 1,
                peer_id: row.get(6)?,
            })
        },
    )
    .map_err(|e| format!("device not found: {e}"))
}

/// List all known devices.
pub fn list_devices(conn: &Connection) -> Result<Vec<DeviceInfo>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, device_name, platform, first_seen, last_synced, is_local, peer_id \
             FROM devices ORDER BY is_local DESC, first_seen ASC",
        )
        .map_err(|e| e.to_string())?;

    let devices = stmt
        .query_map([], |row| {
            Ok(DeviceInfo {
                id: row.get(0)?,
                device_name: row.get(1)?,
                platform: row.get(2)?,
                first_seen: row.get(3)?,
                last_synced: row.get(4)?,
                is_local: row.get::<_, i64>(5)? == 1,
                peer_id: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(devices)
}

/// Remove a remote device and its sync state.
pub fn remove_device(conn: &Connection, device_id: &str) -> Result<(), String> {
    // Don't allow removing the local device
    let is_local: bool = conn
        .query_row(
            "SELECT is_local FROM devices WHERE id = ?1",
            params![device_id],
            |row| Ok(row.get::<_, i64>(0)? == 1),
        )
        .map_err(|e| format!("device not found: {e}"))?;

    if is_local {
        return Err("cannot remove local device".into());
    }

    conn.execute(
        "DELETE FROM sync_state WHERE device_id = ?1",
        params![device_id],
    )
    .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM devices WHERE id = ?1", params![device_id])
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Get the sync vector (per-table timestamps) for a remote device.
pub fn get_sync_vector(conn: &Connection, device_id: &str) -> Result<Vec<SyncTableState>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT device_id, table_name, last_synced_at, row_count \
             FROM sync_state WHERE device_id = ?1",
        )
        .map_err(|e| e.to_string())?;

    let states = stmt
        .query_map(params![device_id], |row| {
            Ok(SyncTableState {
                device_id: row.get(0)?,
                table_name: row.get(1)?,
                last_synced_at: row.get(2)?,
                row_count: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(states)
}

/// Update the sync state for a specific table+device.
pub fn update_sync_state(
    conn: &Connection,
    device_id: &str,
    table_name: &str,
    last_synced_at: &str,
    row_count: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO sync_state (device_id, table_name, last_synced_at, row_count) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(device_id, table_name) DO UPDATE SET \
         last_synced_at = ?3, row_count = sync_state.row_count + ?4",
        params![device_id, table_name, last_synced_at, row_count],
    )
    .map_err(|e| e.to_string())?;

    // Update device's last_synced
    conn.execute(
        "UPDATE devices SET last_synced = datetime('now') WHERE id = ?1",
        params![device_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Enqueue a change for outbound sync.
pub fn enqueue_change(
    conn: &Connection,
    table_name: &str,
    row_id: &str,
    operation: &str,
    row_data: Option<&str>,
    updated_at: &str,
) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO sync_queue (table_name, row_id, operation, row_data, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![table_name, row_id, operation, row_data, updated_at],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    Ok(id)
}

/// Get pending sync queue items for a specific device.
///
/// Returns items that have not yet been delivered to the given device.
pub fn get_pending_queue_items(
    conn: &Connection,
    device_id: &str,
    limit: i64,
) -> Result<Vec<SyncQueueItem>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, table_name, row_id, operation, row_data, updated_at, \
             queued_at, delivered_to \
             FROM sync_queue \
             WHERE delivered_to NOT LIKE '%' || ?1 || '%' \
             ORDER BY queued_at ASC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;

    let items = stmt
        .query_map(params![device_id, limit], |row| {
            let delivered_json: String = row.get(7)?;
            let delivered: Vec<String> = serde_json::from_str(&delivered_json).unwrap_or_default();
            Ok(SyncQueueItem {
                id: row.get(0)?,
                table_name: row.get(1)?,
                row_id: row.get(2)?,
                operation: row.get(3)?,
                row_data: row.get(4)?,
                updated_at: row.get(5)?,
                queued_at: row.get(6)?,
                delivered_to: delivered,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(items)
}

/// Mark queue items as delivered to a specific device.
pub fn mark_delivered(conn: &Connection, queue_ids: &[i64], device_id: &str) -> Result<(), String> {
    for id in queue_ids {
        // Load current delivered_to, append device_id
        let current_json: String = conn
            .query_row(
                "SELECT delivered_to FROM sync_queue WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        let mut delivered: Vec<String> = serde_json::from_str(&current_json).unwrap_or_default();

        if !delivered.contains(&device_id.to_string()) {
            delivered.push(device_id.to_string());
        }

        let new_json = serde_json::to_string(&delivered).unwrap_or_default();
        conn.execute(
            "UPDATE sync_queue SET delivered_to = ?1 WHERE id = ?2",
            params![new_json, id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Prune sync queue items that have been delivered to all known devices.
pub fn prune_delivered_queue(conn: &Connection) -> Result<i64, String> {
    // Get all remote device IDs
    let mut stmt = conn
        .prepare("SELECT id FROM devices WHERE is_local = 0")
        .map_err(|e| e.to_string())?;

    let device_ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    if device_ids.is_empty() {
        return Ok(0);
    }

    // Delete items delivered to ALL devices.
    // Collect all queue IDs and their delivered_to in one pass, then
    // delete the fully-delivered ones in a second pass.
    let mut deleted = 0i64;

    let queue_entries: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, delivered_to FROM sync_queue")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    for (queue_id, delivered_json) in &queue_entries {
        let delivered: Vec<String> = serde_json::from_str(delivered_json).unwrap_or_default();

        let all_delivered = device_ids.iter().all(|did| delivered.contains(did));

        if all_delivered {
            conn.execute("DELETE FROM sync_queue WHERE id = ?1", params![queue_id])
                .map_err(|e| e.to_string())?;
            deleted += 1;
        }
    }

    Ok(deleted)
}

/// Merge incoming sync rows using LWW (last-writer-wins) strategy.
///
/// For each row:
///   1. Check if the row exists locally
///   2. If not, insert it (new row from remote)
///   3. If yes, compare `updated_at` timestamps
///   4. Remote wins if its `updated_at` is strictly newer
///
/// Returns the number of rows actually merged (inserted or updated).
pub fn merge_lww_rows(
    conn: &Connection,
    table_name: &str,
    rows: &[SyncRow],
) -> Result<i64, String> {
    // Validate table name is in the syncable list
    if !SYNCABLE_TABLES.contains(&table_name) {
        return Err(format!("table '{table_name}' is not syncable"));
    }

    let mut merged = 0i64;

    for row in rows {
        match row.operation.as_str() {
            "insert" | "update" => {
                let data = row
                    .data
                    .as_ref()
                    .ok_or_else(|| format!("missing data for {} on {}", row.row_id, table_name))?;

                // Check if row exists and get its updated_at
                let local_updated: Option<String> = conn
                    .query_row(
                        &format!(
                            "SELECT updated_at FROM {} WHERE id = ?1",
                            sanitize_table_name(table_name)?
                        ),
                        params![row.row_id],
                        |r| r.get(0),
                    )
                    .ok();

                match local_updated {
                    Some(local_ts) => {
                        // LWW: remote wins if strictly newer
                        if row.updated_at > local_ts {
                            apply_row_update(conn, table_name, &row.row_id, data)?;
                            merged += 1;
                        }
                    }
                    None => {
                        // Row doesn't exist locally — insert
                        apply_row_insert(conn, table_name, &row.row_id, data)?;
                        merged += 1;
                    }
                }
            }
            "delete" => {
                // For append-only tables, ignore deletes
                if table_name == "evidence_records" || table_name == "skill_proof_evidence" {
                    continue;
                }
                let deleted = conn
                    .execute(
                        &format!(
                            "DELETE FROM {} WHERE id = ?1",
                            sanitize_table_name(table_name)?
                        ),
                        params![row.row_id],
                    )
                    .map_err(|e| e.to_string())?;
                if deleted > 0 {
                    merged += 1;
                }
            }
            op => {
                log::warn!("unknown sync operation '{}' for {}", op, row.row_id);
            }
        }
    }

    Ok(merged)
}

/// Merge incoming rows using append-only union strategy.
///
/// For evidence_records and skill_proof_evidence: insert if not
/// exists (deduplicate by primary key), never update or delete.
pub fn merge_append_only(
    conn: &Connection,
    table_name: &str,
    rows: &[SyncRow],
) -> Result<i64, String> {
    let safe_table = sanitize_table_name(table_name)?;
    let mut merged = 0i64;

    for row in rows {
        if row.operation == "delete" {
            continue; // Never delete in append-only tables
        }

        let data = match &row.data {
            Some(d) => d,
            None => continue,
        };

        // Check if already exists
        let exists: bool = conn
            .query_row(
                &format!("SELECT COUNT(*) > 0 FROM {} WHERE id = ?1", safe_table),
                params![row.row_id],
                |r| r.get(0),
            )
            .unwrap_or(false);

        if !exists {
            apply_row_insert(conn, table_name, &row.row_id, data)?;
            merged += 1;
        }
    }

    Ok(merged)
}

/// Apply a row insert from sync data.
///
/// Parses the JSON data object and builds an INSERT statement
/// dynamically from the key-value pairs.
fn apply_row_insert(
    conn: &Connection,
    table_name: &str,
    row_id: &str,
    data: &serde_json::Value,
) -> Result<(), String> {
    let safe_table = sanitize_table_name(table_name)?;

    let obj = data
        .as_object()
        .ok_or_else(|| "sync data must be a JSON object".to_string())?;

    if obj.is_empty() {
        return Err("sync data object is empty".to_string());
    }

    let mut columns = Vec::new();
    let mut placeholders = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    // Always include the id
    columns.push("id".to_string());
    placeholders.push(format!("?{idx}"));
    values.push(Box::new(row_id.to_string()));
    idx += 1;

    for (key, val) in obj {
        if key == "id" {
            continue; // Already handled
        }
        sanitize_column_name(key)?;
        columns.push(key.clone());
        placeholders.push(format!("?{idx}"));
        match val {
            serde_json::Value::String(s) => values.push(Box::new(s.clone())),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    values.push(Box::new(i));
                } else if let Some(f) = n.as_f64() {
                    values.push(Box::new(f));
                } else {
                    values.push(Box::new(n.to_string()));
                }
            }
            serde_json::Value::Bool(b) => values.push(Box::new(*b as i64)),
            serde_json::Value::Null => values.push(Box::new(rusqlite::types::Null)),
            _ => values.push(Box::new(val.to_string())),
        }
        idx += 1;
    }

    let sql = format!(
        "INSERT OR IGNORE INTO {} ({}) VALUES ({})",
        safe_table,
        columns.join(", "),
        placeholders.join(", ")
    );

    let params_ref: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();

    conn.execute(&sql, params_ref.as_slice())
        .map_err(|e| format!("sync insert into {table_name} failed: {e}"))?;

    Ok(())
}

/// Apply a row update from sync data.
///
/// Builds a dynamic UPDATE statement from the JSON key-value pairs.
fn apply_row_update(
    conn: &Connection,
    table_name: &str,
    row_id: &str,
    data: &serde_json::Value,
) -> Result<(), String> {
    let safe_table = sanitize_table_name(table_name)?;

    let obj = data
        .as_object()
        .ok_or_else(|| "sync data must be a JSON object".to_string())?;

    let mut set_clauses = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    for (key, val) in obj {
        if key == "id" {
            continue;
        }
        let safe_col = sanitize_column_name(key)?;
        set_clauses.push(format!("{safe_col} = ?{idx}"));
        match val {
            serde_json::Value::String(s) => values.push(Box::new(s.clone())),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    values.push(Box::new(i));
                } else if let Some(f) = n.as_f64() {
                    values.push(Box::new(f));
                } else {
                    values.push(Box::new(n.to_string()));
                }
            }
            serde_json::Value::Bool(b) => values.push(Box::new(*b as i64)),
            serde_json::Value::Null => values.push(Box::new(rusqlite::types::Null)),
            _ => values.push(Box::new(val.to_string())),
        }
        idx += 1;
    }

    if set_clauses.is_empty() {
        return Ok(());
    }

    // Add row_id as the last parameter for WHERE
    values.push(Box::new(row_id.to_string()));

    let sql = format!(
        "UPDATE {} SET {} WHERE id = ?{idx}",
        safe_table,
        set_clauses.join(", ")
    );

    let params_ref: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();

    conn.execute(&sql, params_ref.as_slice())
        .map_err(|e| format!("sync update on {table_name} failed: {e}"))?;

    Ok(())
}

/// Sanitize a column name to prevent SQL injection via crafted JSON keys.
///
/// Only allows alphanumeric characters and underscores, must start with
/// a letter or underscore, and must be at most 64 characters.
fn sanitize_column_name(name: &str) -> Result<&str, String> {
    if !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name
            .bytes()
            .next()
            .map_or(false, |b| b.is_ascii_alphabetic() || b == b'_')
    {
        Ok(name)
    } else {
        Err(format!("invalid column name for sync: '{name}'"))
    }
}

/// Sanitize a table name to prevent SQL injection.
///
/// Only allows known syncable table names.
fn sanitize_table_name(name: &str) -> Result<&str, String> {
    // Extended list: syncable tables plus related tables that might
    // be referenced during sync operations
    const ALLOWED: &[&str] = &[
        "enrollments",
        "element_progress",
        "course_notes",
        "evidence_records",
        "skill_proof_evidence",
        "course_enrollments",
    ];

    if ALLOWED.contains(&name) {
        Ok(name)
    } else {
        Err(format!("table name '{name}' is not allowed for sync"))
    }
}

/// Get overall sync status.
pub fn get_sync_status(conn: &Connection) -> Result<SyncStatus, String> {
    let device_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM devices WHERE is_local = 0",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let queue_length: i64 = conn
        .query_row("SELECT COUNT(*) FROM sync_queue", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    let last_sync: Option<String> = conn
        .query_row(
            "SELECT MAX(last_synced) FROM devices WHERE is_local = 0",
            [],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    // Per-device summaries
    let mut stmt = conn
        .prepare(
            "SELECT d.id, d.device_name, d.last_synced, \
             (SELECT COUNT(*) FROM sync_state WHERE device_id = d.id) \
             FROM devices d WHERE d.is_local = 0 ORDER BY d.last_synced DESC",
        )
        .map_err(|e| e.to_string())?;

    let devices: Vec<DeviceSyncSummary> = stmt
        .query_map([], |row| {
            Ok(DeviceSyncSummary {
                device_id: row.get(0)?,
                device_name: row.get(1)?,
                last_synced: row.get(2)?,
                tables_synced: row.get(3)?,
                is_online: false, // Would be set from P2P peer tracking
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(SyncStatus {
        device_count,
        queue_length,
        auto_sync: false, // Would be loaded from a settings table
        last_sync,
        devices,
    })
}

/// Record a sync event in the sync_log table.
pub fn record_sync_event(
    conn: &Connection,
    device_id: &str,
    direction: &str,
    rows_count: i64,
) -> Result<(), String> {
    let log_id = entity_id(&[device_id, direction, &chrono::Utc::now().to_rfc3339()]);

    conn.execute(
        "INSERT INTO sync_log (entity_type, entity_id, direction, peer_id, signature) \
         VALUES ('sync', ?1, ?2, ?3, ?4)",
        params![log_id, direction, device_id, rows_count.to_string()],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Get sync history entries from the sync_log.
pub fn get_sync_history(conn: &Connection, limit: i64) -> Result<Vec<SyncHistoryEntry>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT sl.peer_id, d.device_name, sl.synced_at, \
             CAST(sl.signature AS INTEGER), sl.direction \
             FROM sync_log sl \
             LEFT JOIN devices d ON d.id = sl.peer_id \
             WHERE sl.entity_type = 'sync' \
             ORDER BY sl.synced_at DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;

    let history = stmt
        .query_map(params![limit], |row| {
            Ok(SyncHistoryEntry {
                device_id: row.get(0)?,
                device_name: row.get(1)?,
                synced_at: row.get::<_, String>(2).unwrap_or_default(),
                rows_sent: 0,
                rows_received: row.get(3).unwrap_or(0),
                direction: row.get(4).unwrap_or_else(|_| "unknown".into()),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn setup_identity(db: &Database) {
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test1ulearner', 'addr_test1q123')",
                [],
            )
            .unwrap();
    }

    #[test]
    fn register_local_device() {
        let db = test_db();
        setup_identity(&db);

        let device_id = super::register_local_device(db.conn(), Some("Test Mac"), "macos")
            .expect("register device");

        assert!(!device_id.is_empty());

        // Verify it's marked as local
        let is_local: bool = db
            .conn()
            .query_row(
                "SELECT is_local = 1 FROM devices WHERE id = ?1",
                params![device_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(is_local);

        // Verify device_id is on local_identity
        let stored_id: Option<String> = db
            .conn()
            .query_row(
                "SELECT device_id FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_id, Some(device_id));
    }

    #[test]
    fn register_remote_device_and_list() {
        let db = test_db();

        super::register_remote_device(
            db.conn(),
            "remote-uuid-1",
            Some("Linux Desktop"),
            Some("linux"),
            Some("12D3KooW..."),
        )
        .unwrap();

        let devices = super::list_devices(db.conn()).unwrap();
        assert_eq!(devices.len(), 1);
        assert!(!devices[0].is_local);
        assert_eq!(devices[0].device_name.as_deref(), Some("Linux Desktop"));
    }

    #[test]
    fn remove_remote_device() {
        let db = test_db();

        super::register_remote_device(db.conn(), "remote-1", Some("Old"), None, None).unwrap();
        assert_eq!(super::list_devices(db.conn()).unwrap().len(), 1);

        super::remove_device(db.conn(), "remote-1").unwrap();
        assert_eq!(super::list_devices(db.conn()).unwrap().len(), 0);
    }

    #[test]
    fn cannot_remove_local_device() {
        let db = test_db();
        setup_identity(&db);

        let device_id = super::register_local_device(db.conn(), Some("My Mac"), "macos").unwrap();

        let result = super::remove_device(db.conn(), &device_id);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot remove local"));
    }

    #[test]
    fn sync_key_deterministic() {
        let key1 = derive_sync_key(b"test_signing_key_bytes");
        let key2 = derive_sync_key(b"test_signing_key_bytes");
        assert_eq!(key1, key2);
    }

    #[test]
    fn sync_key_differs_for_different_keys() {
        let key1 = derive_sync_key(b"key_one");
        let key2 = derive_sync_key(b"key_two");
        assert_ne!(key1, key2);
    }

    #[test]
    fn enqueue_and_retrieve_changes() {
        let db = test_db();

        // Need a device for pending items
        super::register_remote_device(db.conn(), "remote-1", None, None, None).unwrap();

        let id = super::enqueue_change(
            db.conn(),
            "enrollments",
            "enr_123",
            "insert",
            Some(r#"{"id":"enr_123","course_id":"c1","status":"active"}"#),
            "2025-01-01T00:00:00Z",
        )
        .unwrap();
        assert!(id > 0);

        let pending = super::get_pending_queue_items(db.conn(), "remote-1", 100).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].row_id, "enr_123");
        assert_eq!(pending[0].operation, "insert");
    }

    #[test]
    fn mark_delivered_removes_from_pending() {
        let db = test_db();

        super::register_remote_device(db.conn(), "remote-1", None, None, None).unwrap();

        let id = super::enqueue_change(
            db.conn(),
            "enrollments",
            "enr_1",
            "insert",
            Some("{}"),
            "2025-01-01T00:00:00Z",
        )
        .unwrap();

        super::mark_delivered(db.conn(), &[id], "remote-1").unwrap();

        let pending = super::get_pending_queue_items(db.conn(), "remote-1", 100).unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn lww_merge_insert_new_row() {
        let db = test_db();

        // Create a course (enrollment FK target)
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) VALUES ('c1', 'Test', 'addr1')",
                [],
            )
            .unwrap();

        let rows = vec![SyncRow {
            row_id: "enr_new".into(),
            operation: "insert".into(),
            data: Some(serde_json::json!({
                "course_id": "c1",
                "status": "active",
                "updated_at": "2025-01-01T00:00:00Z"
            })),
            updated_at: "2025-01-01T00:00:00Z".into(),
        }];

        let merged = merge_lww_rows(db.conn(), "enrollments", &rows).unwrap();
        assert_eq!(merged, 1);

        // Verify it was inserted
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM enrollments WHERE id = 'enr_new'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn lww_merge_remote_wins_newer_timestamp() {
        let db = test_db();

        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) VALUES ('c1', 'Test', 'addr1')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO enrollments (id, course_id, status, updated_at) \
                 VALUES ('enr_1', 'c1', 'active', '2025-01-01T00:00:00Z')",
                [],
            )
            .unwrap();

        // Remote has a newer timestamp with updated status
        let rows = vec![SyncRow {
            row_id: "enr_1".into(),
            operation: "update".into(),
            data: Some(serde_json::json!({
                "status": "completed",
                "updated_at": "2025-06-01T00:00:00Z"
            })),
            updated_at: "2025-06-01T00:00:00Z".into(),
        }];

        let merged = merge_lww_rows(db.conn(), "enrollments", &rows).unwrap();
        assert_eq!(merged, 1);

        // Verify status was updated
        let status: String = db
            .conn()
            .query_row(
                "SELECT status FROM enrollments WHERE id = 'enr_1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "completed");
    }

    #[test]
    fn lww_merge_local_wins_older_timestamp() {
        let db = test_db();

        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) VALUES ('c1', 'Test', 'addr1')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO enrollments (id, course_id, status, updated_at) \
                 VALUES ('enr_1', 'c1', 'completed', '2025-06-01T00:00:00Z')",
                [],
            )
            .unwrap();

        // Remote has an OLDER timestamp — local should win
        let rows = vec![SyncRow {
            row_id: "enr_1".into(),
            operation: "update".into(),
            data: Some(serde_json::json!({
                "status": "active",
                "updated_at": "2025-01-01T00:00:00Z"
            })),
            updated_at: "2025-01-01T00:00:00Z".into(),
        }];

        let merged = merge_lww_rows(db.conn(), "enrollments", &rows).unwrap();
        assert_eq!(merged, 0); // Nothing merged — local wins

        let status: String = db
            .conn()
            .query_row(
                "SELECT status FROM enrollments WHERE id = 'enr_1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "completed"); // Unchanged
    }

    #[test]
    fn append_only_deduplicates() {
        let db = test_db();

        // Set up required foreign keys
        db.conn()
            .execute(
                "INSERT INTO subject_fields (id, name) VALUES ('sf1', 'CS')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub1', 'Algo', 'sf1')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO skills (id, name, subject_id) VALUES ('sk1', 'Sort', 'sub1')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) VALUES ('c1', 'Test', 'addr1')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO skill_assessments (id, skill_id, assessment_type, proficiency_level, difficulty, weight) \
                 VALUES ('sa1', 'sk1', 'quiz', 'apply', 0.5, 1.0)",
                [],
            )
            .unwrap();

        // Insert a row first
        db.conn()
            .execute(
                "INSERT INTO evidence_records \
                 (id, skill_id, skill_assessment_id, proficiency_level, course_id, score, difficulty, trust_factor) \
                 VALUES ('ev_1', 'sk1', 'sa1', 'apply', 'c1', 0.80, 0.50, 1.0)",
                [],
            )
            .unwrap();

        // Try to merge the same row — should be ignored (dedup)
        let rows = vec![SyncRow {
            row_id: "ev_1".into(),
            operation: "insert".into(),
            data: Some(serde_json::json!({
                "skill_id": "sk1",
                "score": 0.90
            })),
            updated_at: "2025-06-01T00:00:00Z".into(),
        }];

        let merged = merge_append_only(db.conn(), "evidence_records", &rows).unwrap();
        assert_eq!(merged, 0); // Already exists

        // Original score unchanged
        let score: f64 = db
            .conn()
            .query_row(
                "SELECT score FROM evidence_records WHERE id = 'ev_1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!((score - 0.80).abs() < 0.001);
    }

    #[test]
    fn sanitize_rejects_invalid_tables() {
        assert!(sanitize_table_name("enrollments").is_ok());
        assert!(sanitize_table_name("evidence_records").is_ok());
        assert!(sanitize_table_name("users; DROP TABLE").is_err());
        assert!(sanitize_table_name("local_identity").is_err());
    }

    #[test]
    fn sync_status_empty() {
        let db = test_db();
        let status = get_sync_status(db.conn()).unwrap();
        assert_eq!(status.device_count, 0);
        assert_eq!(status.queue_length, 0);
        assert!(status.devices.is_empty());
    }

    #[test]
    fn sync_state_tracking() {
        let db = test_db();

        super::register_remote_device(db.conn(), "remote-1", None, None, None).unwrap();

        update_sync_state(
            db.conn(),
            "remote-1",
            "enrollments",
            "2025-06-01T00:00:00Z",
            5,
        )
        .unwrap();

        let vector = get_sync_vector(db.conn(), "remote-1").unwrap();
        assert_eq!(vector.len(), 1);
        assert_eq!(vector[0].table_name, "enrollments");
        assert_eq!(vector[0].row_count, 5);
    }

    #[test]
    fn prune_delivered_queue_items() {
        let db = test_db();

        super::register_remote_device(db.conn(), "remote-1", None, None, None).unwrap();

        let id = super::enqueue_change(
            db.conn(),
            "enrollments",
            "enr_1",
            "insert",
            Some("{}"),
            "2025-01-01T00:00:00Z",
        )
        .unwrap();

        // Mark as delivered
        super::mark_delivered(db.conn(), &[id], "remote-1").unwrap();

        // Prune
        let pruned = prune_delivered_queue(db.conn()).unwrap();
        assert_eq!(pruned, 1);

        // Queue should be empty
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM sync_queue", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
