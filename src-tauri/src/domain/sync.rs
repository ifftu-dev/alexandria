//! Cross-device sync domain types.
//!
//! Types for multi-device synchronization via the P2P network.
//! Pairing = importing the same mnemonic on both devices. Data
//! is encrypted with XChaCha20-Poly1305 (key derived via HKDF
//! from the wallet signing key).
//!
//! Merge strategy:
//!   - LWW (last-writer-wins): enrollments, element_progress, course_notes,
//!     integrity_sessions, local_identity
//!   - Derived (not synced): reputation_assertions — recomputed locally
//!     from credentials after sync
//!
//! Post-migration 040: `evidence_records` and `skill_proof_evidence`
//! were retired together with the SkillProof pipeline. Auto-earned VCs
//! are the new canonical artifact; `credentials` sync is tracked as
//! follow-up work (see memory: project_vc_first_architecture.md).

use serde::{Deserialize, Serialize};

/// Tables eligible for cross-device sync.
pub const SYNCABLE_TABLES: &[&str] = &["enrollments", "element_progress", "course_notes"];

/// Information about a known device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Unique device identifier (UUID).
    pub id: String,
    /// User-assigned device name (e.g., "MacBook Pro").
    pub device_name: Option<String>,
    /// Platform: macos, windows, linux.
    pub platform: Option<String>,
    /// When this device was first seen.
    pub first_seen: String,
    /// Last successful sync timestamp.
    pub last_synced: Option<String>,
    /// Whether this is the local device.
    pub is_local: bool,
    /// The device's libp2p PeerId (if known).
    pub peer_id: Option<String>,
}

/// Per-table sync state with a remote device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTableState {
    /// Remote device ID.
    pub device_id: String,
    /// Table name.
    pub table_name: String,
    /// Timestamp of the last row synced from this device.
    pub last_synced_at: String,
    /// Number of rows synced from this device for this table.
    pub row_count: i64,
}

/// An item in the outbound sync queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncQueueItem {
    /// Auto-increment queue ID.
    pub id: i64,
    /// Which table the change belongs to.
    pub table_name: String,
    /// Primary key of the changed row.
    pub row_id: String,
    /// Type of change: insert, update, or delete.
    pub operation: String,
    /// JSON snapshot of the row data (null for deletes).
    pub row_data: Option<String>,
    /// Timestamp of the change (used as LWW tiebreaker).
    pub updated_at: String,
    /// When the item was queued.
    pub queued_at: String,
    /// Device IDs that have received this item.
    pub delivered_to: Vec<String>,
}

/// A sync message exchanged between devices over the P2P network.
///
/// Encrypted with XChaCha20-Poly1305 before transmission. The
/// key is derived via HKDF-SHA256 from the shared wallet signing
/// key + a fixed salt "alexandria-cross-device-sync-v1".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMessage {
    /// Message type.
    pub msg_type: SyncMessageType,
    /// Sender device ID.
    pub device_id: String,
    /// Sender device name (for display).
    pub device_name: Option<String>,
    /// Unix timestamp of the message.
    pub timestamp: u64,
}

/// Types of sync messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SyncMessageType {
    /// Handshake: announce device presence and capabilities.
    Hello {
        platform: String,
        /// Per-table last-synced timestamps (LWW vector).
        sync_vector: Vec<SyncTableState>,
    },
    /// Request rows newer than the given timestamps.
    RequestSync {
        /// Per-table: (table_name, since_timestamp).
        requests: Vec<(String, String)>,
    },
    /// Response with rows to merge.
    SyncData {
        /// Table name → list of row JSON snapshots.
        table_name: String,
        rows: Vec<SyncRow>,
    },
    /// Acknowledgement that sync data was received and merged.
    SyncAck {
        /// Per-table count of rows merged.
        merged: Vec<(String, i64)>,
    },
}

/// A single row to sync (serialized as JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRow {
    /// Primary key of the row.
    pub row_id: String,
    /// Operation that produced this row.
    pub operation: String,
    /// Full row data as JSON (null for deletes).
    pub data: Option<serde_json::Value>,
    /// Timestamp for LWW conflict resolution.
    pub updated_at: String,
}

/// Overall sync status reported to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    /// Number of known remote devices.
    pub device_count: i64,
    /// Number of items in the outbound sync queue.
    pub queue_length: i64,
    /// Whether auto-sync is enabled.
    pub auto_sync: bool,
    /// Last sync timestamp (across all devices).
    pub last_sync: Option<String>,
    /// Per-device sync summaries.
    pub devices: Vec<DeviceSyncSummary>,
}

/// Per-device sync summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncSummary {
    pub device_id: String,
    pub device_name: Option<String>,
    pub last_synced: Option<String>,
    pub tables_synced: i64,
    pub is_online: bool,
}

/// Result of a sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// Number of rows sent to remote.
    pub rows_sent: i64,
    /// Number of rows received from remote.
    pub rows_received: i64,
    /// Number of rows merged (with LWW conflict resolution).
    pub rows_merged: i64,
    /// Per-table breakdown.
    pub table_stats: Vec<(String, i64, i64)>,
    /// Duration in milliseconds.
    pub duration_ms: i64,
}

/// Sync history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncHistoryEntry {
    /// Remote device ID.
    pub device_id: String,
    /// Remote device name.
    pub device_name: Option<String>,
    /// When the sync occurred.
    pub synced_at: String,
    /// Rows sent.
    pub rows_sent: i64,
    /// Rows received.
    pub rows_received: i64,
    /// Direction: push, pull, or bidirectional.
    pub direction: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syncable_tables_count() {
        assert_eq!(SYNCABLE_TABLES.len(), 3);
    }

    #[test]
    fn syncable_tables_contents() {
        assert!(SYNCABLE_TABLES.contains(&"enrollments"));
        assert!(SYNCABLE_TABLES.contains(&"element_progress"));
        assert!(SYNCABLE_TABLES.contains(&"course_notes"));
    }

    #[test]
    fn sync_message_type_hello_serde() {
        let msg_type = SyncMessageType::Hello {
            platform: "macos".into(),
            sync_vector: vec![SyncTableState {
                device_id: "dev1".into(),
                table_name: "enrollments".into(),
                last_synced_at: "2025-01-01".into(),
                row_count: 10,
            }],
        };
        let json = serde_json::to_string(&msg_type).unwrap();
        assert!(json.contains("\"type\":\"Hello\""));
        let parsed: SyncMessageType = serde_json::from_str(&json).unwrap();
        if let SyncMessageType::Hello {
            platform,
            sync_vector,
        } = parsed
        {
            assert_eq!(platform, "macos");
            assert_eq!(sync_vector.len(), 1);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn sync_message_type_request_sync_serde() {
        let msg_type = SyncMessageType::RequestSync {
            requests: vec![("enrollments".into(), "2025-01-01".into())],
        };
        let json = serde_json::to_string(&msg_type).unwrap();
        let parsed: SyncMessageType = serde_json::from_str(&json).unwrap();
        if let SyncMessageType::RequestSync { requests } = parsed {
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].0, "enrollments");
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn sync_message_type_sync_data_serde() {
        let msg_type = SyncMessageType::SyncData {
            table_name: "enrollments".into(),
            rows: vec![SyncRow {
                row_id: "row1".into(),
                operation: "insert".into(),
                data: Some(serde_json::json!({"id": "row1"})),
                updated_at: "2025-01-01".into(),
            }],
        };
        let json = serde_json::to_string(&msg_type).unwrap();
        let parsed: SyncMessageType = serde_json::from_str(&json).unwrap();
        if let SyncMessageType::SyncData { table_name, rows } = parsed {
            assert_eq!(table_name, "enrollments");
            assert_eq!(rows.len(), 1);
            assert!(rows[0].data.is_some());
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn sync_message_type_ack_serde() {
        let msg_type = SyncMessageType::SyncAck {
            merged: vec![("enrollments".into(), 5)],
        };
        let json = serde_json::to_string(&msg_type).unwrap();
        let parsed: SyncMessageType = serde_json::from_str(&json).unwrap();
        if let SyncMessageType::SyncAck { merged } = parsed {
            assert_eq!(merged.len(), 1);
            assert_eq!(merged[0].1, 5);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn sync_message_envelope_serde() {
        let msg = SyncMessage {
            msg_type: SyncMessageType::SyncAck { merged: vec![] },
            device_id: "dev1".into(),
            device_name: Some("MacBook".into()),
            timestamp: 1700000000,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: SyncMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.device_id, "dev1");
        assert_eq!(parsed.timestamp, 1700000000);
    }
}
