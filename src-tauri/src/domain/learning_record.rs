//! The activity log as the app reports it: its size and the rows of its
//! view. Mirrored in `src/types/index.ts`. The records themselves are
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

/// One row of the activity log view: a learning record and what it is about.
/// Read a page at a time, newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearningRecordEntry {
    /// The record's stable local id.
    pub id: String,
    /// `completion` or `interaction`.
    pub kind: String,
    /// The xAPI verb's short name: `registered`, `launched`, `progressed`,
    /// `completed`, `passed`, `failed`, `scored`, `responded`, `voided`.
    pub verb: String,
    pub course_id: String,
    /// The course's title on this device, if it is still here.
    pub course_title: Option<String>,
    pub element_id: Option<String>,
    pub element_title: Option<String>,
    /// The activity's identifier, a DID URL.
    pub activity_id: String,
    /// Kept as the one summary of its attempt when the log was over its limit.
    pub summary: bool,
    pub bytes: u64,
    pub recorded_at: String,
    /// Course progress, for `progressed`.
    pub percent: Option<u8>,
    /// Scaled score, -1 to 1, when the record carries one.
    pub score: Option<f64>,
}

/// A signed export of the activity log, as two files to write unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningRecordExport {
    /// A JSON array of xAPI statements.
    pub statements: String,
    /// The detached signature over `statements`' exact bytes
    /// (`alexandria_verify::record_export`).
    pub signature: String,
}
