//! The activity log's size as the app reports it. Mirrored in
//! `src/types/index.ts`. The records themselves are
//! [`crate::learning_records::LearningRecord`].

use serde::{Deserialize, Serialize};

/// How big the activity log is and how big it may get before the oldest
/// interaction-level records are removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningRecordUsage {
    /// Total size of the stored records.
    pub bytes: u64,
    pub limit_bytes: u64,
    pub records: u64,
    /// Records of responses a package reported.
    pub interaction_records: u64,
}
