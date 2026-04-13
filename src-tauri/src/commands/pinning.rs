//! IPC commands for PinBoard management + quota introspection. Stubs — PR 10.

use tauri::State;

use crate::p2p::pinboard::PinboardCommitment;
use crate::AppState;

#[tauri::command]
pub async fn declare_pinboard_commitment(
    _state: State<'_, AppState>,
    _subject_did: String,
    _scope: Vec<String>,
) -> Result<PinboardCommitment, String> {
    Err("PR 10 — declare_pinboard_commitment not yet implemented".into())
}

#[tauri::command]
pub async fn revoke_pinboard_commitment(
    _state: State<'_, AppState>,
    _commitment_id: String,
) -> Result<(), String> {
    Err("PR 10 — revoke_pinboard_commitment not yet implemented".into())
}

#[tauri::command]
pub async fn list_my_commitments(
    _state: State<'_, AppState>,
) -> Result<Vec<PinboardCommitment>, String> {
    Err("PR 10 — list_my_commitments not yet implemented".into())
}

#[tauri::command]
pub async fn list_incoming_commitments(
    _state: State<'_, AppState>,
) -> Result<Vec<PinboardCommitment>, String> {
    Err("PR 10 — list_incoming_commitments not yet implemented".into())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QuotaBreakdown {
    pub subject_authored_bytes: u64,
    pub pinboard_bytes: u64,
    pub cache_bytes: u64,
    pub enrollment_bytes: u64,
    pub total_quota_bytes: u64,
}

#[tauri::command]
pub async fn get_quota_breakdown(_state: State<'_, AppState>) -> Result<QuotaBreakdown, String> {
    Err("PR 10 — get_quota_breakdown not yet implemented".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_breakdown_round_trips() {
        // The frontend dashboard reads this to render the tiered
        // storage breakdown; a silent field rename would break the UI.
        let q = QuotaBreakdown {
            subject_authored_bytes: 1_000,
            pinboard_bytes: 2_000,
            cache_bytes: 3_000,
            enrollment_bytes: 4_000,
            total_quota_bytes: 10_000,
        };
        let s = serde_json::to_string(&q).unwrap();
        let back: QuotaBreakdown = serde_json::from_str(&s).unwrap();
        assert_eq!(back.subject_authored_bytes, 1_000);
        assert_eq!(back.pinboard_bytes, 2_000);
        assert_eq!(back.cache_bytes, 3_000);
        assert_eq!(back.enrollment_bytes, 4_000);
        assert_eq!(back.total_quota_bytes, 10_000);
    }

    #[test]
    fn quota_breakdown_sum_equals_total_when_tiers_fill_quota() {
        // Shape-level invariant the eviction code relies on: the per-
        // tier bytes never over-count the total quota. Locking it here
        // gives PR 10 a harness to plug its real accounting into.
        let q = QuotaBreakdown {
            subject_authored_bytes: 1_000,
            pinboard_bytes: 2_000,
            cache_bytes: 3_000,
            enrollment_bytes: 4_000,
            total_quota_bytes: 10_000,
        };
        let sum = q.subject_authored_bytes + q.pinboard_bytes + q.cache_bytes + q.enrollment_bytes;
        assert!(sum <= q.total_quota_bytes);
    }
}
