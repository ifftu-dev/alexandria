//! Package attempts as the player and the host exchange them. Mirrored in
//! `src/types/index.ts`.

use alexandria_studio::package::Standard;
use serde::{Deserialize, Serialize};

use crate::domain::enrollment::ElementProgress;

/// Where an attempt stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageAttemptStatus {
    /// Open: the learner can launch and resume it.
    InProgress,
    /// The package reported completion. Still the attempt a launch resumes.
    Completed,
    /// Replaced by a fresh attempt. Refuses further commits.
    Closed,
}

impl PackageAttemptStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PackageAttemptStatus::InProgress => "in_progress",
            PackageAttemptStatus::Completed => "completed",
            PackageAttemptStatus::Closed => "closed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "in_progress" => Some(PackageAttemptStatus::InProgress),
            "completed" => Some(PackageAttemptStatus::Completed),
            "closed" => Some(PackageAttemptStatus::Closed),
            _ => None,
        }
    }
}

/// One learner's run through a package element under an enrolment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageAttempt {
    pub id: String,
    pub enrollment_id: String,
    pub element_id: String,
    pub package_hash: String,
    /// 1 for the first attempt at the element under this enrolment.
    pub attempt_number: i64,
    pub status: PackageAttemptStatus,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub updated_at: String,
}

/// What the wrapper needs before one lesson starts: the values only the host
/// knows, and the run-time data to load before the lesson initialises.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageLaunchInit {
    pub attempt_id: String,
    pub activity_id: String,
    pub standard: Standard,
    /// Pseudonymous: derived from the learner's key and the package hash,
    /// stable across attempts at one package, unlinkable across packages.
    pub learner_id: String,
    /// The profile's display name.
    pub learner_name: String,
    /// `ab-initio` on the activity's first launch in this attempt, `resume`
    /// when the content last exited with `suspend`, otherwise empty.
    pub entry: String,
    pub launch_data: Option<String>,
    /// On the package's own standard's scale (see the package description).
    pub mastery_score: Option<f64>,
    pub max_time_allowed: Option<String>,
    pub time_limit_action: Option<String>,
    /// The run-time data to hand the library's `loadFromJSON` before the
    /// lesson starts: the stored state with the session values removed and
    /// every host-supplied value above filled in, in the standard's shape.
    pub runtime: serde_json::Value,
}

/// One coalesced commit from the wrapper: the whole run-time state of one
/// activity, as the library renders it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageCommit {
    pub attempt_id: String,
    pub activity_id: String,
    /// True for the commit the content's own finish call produced: the
    /// session ends and its time is added to the activity's total.
    #[serde(default)]
    pub session_ended: bool,
    /// The library's commit payload, `{ "cmi": { … } }`.
    pub runtime: serde_json::Value,
}

/// The attempt and the element progress after a commit or a termination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageAttemptProgress {
    pub attempt: PackageAttempt,
    pub element_progress: ElementProgress,
    /// The package reports completion under this attempt.
    pub package_completed: bool,
    /// The package's extracted files were dropped (termination of a
    /// completed attempt only); the next launch rebuilds them.
    pub cache_dropped: bool,
}
