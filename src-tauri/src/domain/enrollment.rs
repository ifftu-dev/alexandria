use serde::{Deserialize, Serialize};

/// An enrollment in a course.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Enrollment {
    pub id: String,
    pub course_id: String,
    pub enrolled_at: String,
    pub completed_at: Option<String>,
    pub status: String,
    pub updated_at: String,
}

/// Progress on a single course element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementProgress {
    pub id: String,
    pub enrollment_id: String,
    pub element_id: String,
    pub status: String,
    pub score: Option<f64>,
    pub time_spent: i64,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

/// Request to update progress on an element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProgressRequest {
    pub element_id: String,
    pub status: String,
    pub score: Option<f64>,
    pub time_spent: Option<i64>,
    /// Sentinel integrity session ID — linked to evidence records.
    pub integrity_session_id: Option<String>,
    /// Final integrity score from the Sentinel engine (0.0 to 1.0).
    pub integrity_score: Option<f64>,
}
