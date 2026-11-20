//! Package attempts: one learner's runs through a package element under an
//! enrolment, the run-time state the content stores, and the rollup of that
//! state into element progress.
//!
//! - **Device-local.** Attempts and activity state never sync. Element
//!   progress, which syncs and reaches the guardian mirror, is the only thing
//!   that leaves the device, as it does for every other element.
//! - **One write per commit.** The wrapper coalesces commits and hands the
//!   host the whole state of one activity; [`commit`] stores it, rolls it up
//!   and updates element progress in one transaction.
//! - **Self-reported.** A package result is written to element progress and
//!   nowhere else: never a graded submission, never credential evidence,
//!   never reputation. The database refuses the first (triggers in migration
//!   `package_attempts`).
//!
//! The standard-specific reading of run-time data and the rollup rule live
//! in [`alexandria_studio::package::progress`]; this module adds storage.

pub mod cmi5;
mod rows;

#[cfg(test)]
mod tests;

use alexandria_studio::package::progress::{self, HostValues};
use alexandria_studio::package::Activity;

use crate::db::Database;
use crate::domain::enrollment::{ElementProgress, UpdateProgressRequest};
use crate::domain::package::Package;
use crate::domain::package_attempt::{
    PackageActivityResult, PackageAttempt, PackageAttemptProgress, PackageAttemptStatus,
    PackageCommit, PackageLaunchInit,
};
use crate::learning_records;
use crate::package_serving::{drop_unless_open, Launches};
use crate::packages::{PackageDb, PackageDirs};

/// Why an attempt operation failed. Displays as `code: detail`.
#[derive(Debug, thiserror::Error)]
pub enum AttemptError {
    /// The enrolment, element, attempt or activity does not exist, or does
    /// not belong where the request says.
    #[error("not_found: {0}")]
    NotFound(String),
    /// The element is not a package element, or its package is not stored.
    #[error("not_a_package: {0}")]
    NotAPackage(String),
    /// The attempt was replaced by a fresh one and takes no more state.
    #[error("attempt_closed: {0}")]
    Closed(String),
    /// The commit is not run-time data this host accepts.
    #[error("invalid_commit: {0}")]
    InvalidCommit(String),
    #[error("package files: {0}")]
    Package(#[from] crate::packages::PackageError),
    #[error("database: {0}")]
    Database(String),
}

impl From<rusqlite::Error> for AttemptError {
    fn from(error: rusqlite::Error) -> Self {
        AttemptError::Database(error.to_string())
    }
}

impl From<crate::enrollment::EnrollmentError> for AttemptError {
    fn from(error: crate::enrollment::EnrollmentError) -> Self {
        AttemptError::Database(error.to_string())
    }
}

/// Runs `operation` in one transaction on whatever `db` is. The typed error
/// crosses the executor inside the `Ok` side, so callers can match on it.
async fn in_transaction<T, F>(
    db: &impl PackageDb,
    label: &'static str,
    operation: F,
) -> Result<T, AttemptError>
where
    T: Send + 'static,
    F: FnOnce(&Database) -> Result<T, AttemptError> + Send + 'static,
{
    db.run(label, move |db| {
        Ok(crate::db::with_typed_transaction(db.conn(), || {
            operation(db)
        }))
    })
    .await
    .map_err(AttemptError::Database)?
}

/// Opens the learner's current attempt at a package element, starting the
/// first one when there is none. The current attempt is the latest one not
/// closed by [`fresh`]; a completed attempt is still resumed, for review.
/// Marks the element in progress unless it is already complete.
pub async fn open(
    db: &impl PackageDb,
    enrollment_id: &str,
    element_id: &str,
) -> Result<PackageAttempt, AttemptError> {
    let enrollment_id = enrollment_id.to_string();
    let element_id = element_id.to_string();
    in_transaction(db, "packages.attempt-open", move |db| {
        let conn = db.conn();
        let package_hash = rows::package_element(conn, &enrollment_id, &element_id)?;
        let attempt = match rows::current(conn, &enrollment_id, &element_id)? {
            Some(attempt) => attempt,
            None => rows::start(conn, &enrollment_id, &element_id, &package_hash)?,
        };
        mark_started(db, &attempt)?;
        Ok(attempt)
    })
    .await
}

/// Closes the learner's current attempt at a package element and starts the
/// next one, with no run-time state. The closed attempt keeps its state and
/// refuses further commits; the element's completion, if any, stands.
pub async fn fresh(
    db: &impl PackageDb,
    enrollment_id: &str,
    element_id: &str,
) -> Result<PackageAttempt, AttemptError> {
    let enrollment_id = enrollment_id.to_string();
    let element_id = element_id.to_string();
    in_transaction(db, "packages.attempt-fresh", move |db| {
        let conn = db.conn();
        let package_hash = rows::package_element(conn, &enrollment_id, &element_id)?;
        if let Some(current) = rows::current(conn, &enrollment_id, &element_id)? {
            rows::close(conn, &current.id)?;
        }
        let attempt = rows::start(conn, &enrollment_id, &element_id, &package_hash)?;
        mark_started(db, &attempt)?;
        Ok(attempt)
    })
    .await
}

/// Records that the learner is working on the element, without undoing a
/// completion.
fn mark_started(db: &Database, attempt: &PackageAttempt) -> Result<ElementProgress, AttemptError> {
    let status = match rows::element_status(db.conn(), &attempt.enrollment_id, &attempt.element_id)?
    {
        Some(status) if status == "completed" => status,
        _ => "in_progress".to_string(),
    };
    Ok(crate::enrollment::update_progress(
        db,
        attempt.enrollment_id.clone(),
        UpdateProgressRequest {
            element_id: attempt.element_id.clone(),
            status,
            score: None,
            time_spent: None,
            integrity_session_id: None,
            integrity_score: None,
        },
    )?)
}

/// The initialisation data for one launch of one activity of an attempt:
/// the pseudonymous learner identifier (from `learner_key`, the learner's
/// signing key, and the package hash), the display name, the manifest's
/// launch values and the run-time data to load before the lesson starts.
///
/// A session the content never finished is closed here: its reported time
/// joins the activity's total before the new session begins.
pub async fn initialise(
    db: &impl PackageDb,
    learner_key: &[u8; 32],
    attempt_id: &str,
    activity_id: &str,
) -> Result<PackageLaunchInit, AttemptError> {
    let attempt_id = attempt_id.to_string();
    let activity_id = activity_id.to_string();
    let learner_key = *learner_key;
    in_transaction(db, "packages.attempt-initialise", move |db| {
        let conn = db.conn();
        let attempt = rows::get(conn, &attempt_id)?;
        let package = stored_package(conn, &attempt.package_hash)?;
        let activity = launchable(&package, &activity_id)?;
        let launch = activity
            .launch
            .as_ref()
            .ok_or_else(|| AttemptError::NotFound(format!("activity {activity_id}")))?;

        // A session the content left open ends here: record what it reported
        // before its time is folded in.
        if let Some(open) = rows::activity(conn, &attempt.id, &activity_id)? {
            learning_records::log_unwritten(learning_records::record_package_session(
                conn,
                &attempt,
                &package,
                &activity_id,
                &open.runtime,
            ));
        }
        let stored = rows::finish_open_session(conn, &attempt.id, &activity_id)?;
        if attempt.status != PackageAttemptStatus::Closed {
            learning_records::log_unwritten(learning_records::record_package_launch(
                conn,
                &attempt,
                &package,
                &activity_id,
            ));
        }
        let mut rolled_up = None;
        if progress::completes_on_launch(launch) && attempt.status != PackageAttemptStatus::Closed {
            // Shown-only content reports nothing: launching it completes it.
            rows::put_activity(
                conn,
                &attempt.id,
                &activity_id,
                &rows::ActivityWrite {
                    runtime_json: "{}",
                    complete: true,
                    score: None,
                    total_seconds: 0.0,
                    session_seconds: 0.0,
                },
            )?;
            rolled_up = Some(roll_up(db, &attempt, &package)?);
        }
        let standard = package.standard;
        let entry = progress::entry(standard, stored.as_ref().map(|s| &s.runtime));
        let learner_id = progress::learner_id(&learner_key, &package.hash);
        let learner_name = rows::display_name(conn)?;
        let runtime = progress::initial_runtime(
            standard,
            stored.as_ref().map(|s| &s.runtime),
            HostValues {
                learner_id: &learner_id,
                learner_name: &learner_name,
                entry,
                total_seconds: stored.as_ref().map_or(0.0, |s| s.total_seconds),
                launch,
            },
        );
        Ok(PackageLaunchInit {
            attempt_id: attempt.id,
            activity_id,
            standard,
            learner_id,
            learner_name,
            entry: entry.as_str().to_string(),
            launch_data: launch.launch_data.clone(),
            mastery_score: launch.mastery_score,
            max_time_allowed: launch.max_time_allowed.clone(),
            time_limit_action: launch.time_limit_action.clone(),
            runtime,
            progress: rolled_up,
        })
    })
    .await
}

/// The largest run-time state one commit may carry, serialised. SCORM 2004
/// allows 64,000 characters of suspend data and the interaction records add
/// to that; a megabyte is far beyond what a lesson legitimately stores.
pub const MAX_RUNTIME_BYTES: usize = 1 << 20;

/// Persists one coalesced commit: the whole run-time state of one activity,
/// stored as the content left it, rolled up into the attempt and the
/// element's progress in the same transaction. A commit marked
/// `session_ended` adds the session's reported time to the activity's total.
pub async fn commit(
    db: &impl PackageDb,
    commit: PackageCommit,
) -> Result<PackageAttemptProgress, AttemptError> {
    if !commit.runtime.get("cmi").is_some_and(|cmi| cmi.is_object()) {
        return Err(AttemptError::InvalidCommit(
            "run-time data must be an object with a `cmi` object".into(),
        ));
    }
    let runtime_json = serde_json::to_string(&commit.runtime)
        .map_err(|e| AttemptError::InvalidCommit(e.to_string()))?;
    if runtime_json.len() > MAX_RUNTIME_BYTES {
        return Err(AttemptError::InvalidCommit(format!(
            "run-time data is {} bytes; the limit is {MAX_RUNTIME_BYTES}",
            runtime_json.len()
        )));
    }
    in_transaction(db, "packages.attempt-commit", move |db| {
        let conn = db.conn();
        let attempt = open_attempt(conn, &commit.attempt_id)?;
        let package = stored_package(conn, &attempt.package_hash)?;
        launchable(&package, &commit.activity_id)?;

        let report = progress::read_report(package.standard, &commit.runtime);
        let prior_total = rows::activity(conn, &attempt.id, &commit.activity_id)?
            .map_or(0.0, |stored| stored.total_seconds);
        let (total_seconds, session_seconds) = if commit.session_ended {
            (prior_total + report.session_seconds, 0.0)
        } else {
            (prior_total, report.session_seconds)
        };
        rows::put_activity(
            conn,
            &attempt.id,
            &commit.activity_id,
            &rows::ActivityWrite {
                runtime_json: &runtime_json,
                complete: report.complete,
                score: report.score,
                total_seconds,
                session_seconds,
            },
        )?;
        if commit.session_ended {
            learning_records::log_unwritten(learning_records::record_package_session(
                conn,
                &attempt,
                &package,
                &commit.activity_id,
                &commit.runtime,
            ));
        }
        roll_up(db, &attempt, &package)
    })
    .await
}

/// Ends the launch of an attempt: the player calls this when it closes the
/// package frame. Every session the content left open ends and its time
/// joins the total; the rollup and element progress are brought up to date.
/// When the package reports completion its extracted files are dropped, unless
/// a launch of the package is still open (the player closes its launch first);
/// the next launch rebuilds them from the archive.
pub async fn terminate(
    db: &impl PackageDb,
    dirs: &PackageDirs,
    launches: &Launches,
    attempt_id: &str,
) -> Result<PackageAttemptProgress, AttemptError> {
    let attempt_id = attempt_id.to_string();
    let mut ended = in_transaction(db, "packages.attempt-terminate", move |db| {
        let conn = db.conn();
        let attempt = open_attempt(conn, &attempt_id)?;
        let package = stored_package(conn, &attempt.package_hash)?;
        // Record what each activity reported; results already recorded when
        // their session ended are not written twice.
        for activity_id in rows::outcomes(conn, &attempt.id)?.into_keys() {
            if let Some(stored) = rows::activity(conn, &attempt.id, &activity_id)? {
                learning_records::log_unwritten(learning_records::record_package_session(
                    conn,
                    &attempt,
                    &package,
                    &activity_id,
                    &stored.runtime,
                ));
            }
        }
        rows::finish_open_sessions(conn, &attempt.id)?;
        roll_up(db, &attempt, &package)
    })
    .await?;
    if ended.package_completed {
        ended.cache_dropped = drop_unless_open(launches, dirs, &ended.attempt.package_hash).await?;
    }
    Ok(ended)
}

/// What every lesson of an attempt has reported, in the order a learner
/// meets the lessons: the player's table of contents. Reads only.
pub async fn activities(
    db: &impl PackageDb,
    attempt_id: &str,
) -> Result<Vec<PackageActivityResult>, AttemptError> {
    let attempt_id = attempt_id.to_string();
    in_transaction(db, "packages.attempt-activities", move |db| {
        let conn = db.conn();
        let attempt = rows::get(conn, &attempt_id)?;
        let package = stored_package(conn, &attempt.package_hash)?;
        let outcomes = rows::outcomes(conn, &attempt.id)?;
        Ok(package
            .description
            .launchable_activities()
            .into_iter()
            .map(|activity| {
                let outcome = outcomes.get(&activity.id);
                PackageActivityResult {
                    activity_id: activity.id.clone(),
                    started: outcome.is_some(),
                    complete: outcome.is_some_and(|outcome| outcome.complete),
                    score: outcome.and_then(|outcome| outcome.score),
                }
            })
            .collect())
    })
    .await
}

/// The attempt, refused when a fresh attempt has closed it.
fn open_attempt(
    conn: &rusqlite::Connection,
    attempt_id: &str,
) -> Result<PackageAttempt, AttemptError> {
    let attempt = rows::get(conn, attempt_id)?;
    if attempt.status == PackageAttemptStatus::Closed {
        return Err(AttemptError::Closed(format!(
            "attempt {attempt_id} was replaced by a fresh attempt"
        )));
    }
    Ok(attempt)
}

/// Rolls the attempt's activity state up to the package and writes the
/// element's progress: completed when the package reports completion (and
/// never undone by a later attempt), the package's score, and the time of
/// every session of every attempt at the element.
fn roll_up(
    db: &Database,
    attempt: &PackageAttempt,
    package: &Package,
) -> Result<PackageAttemptProgress, AttemptError> {
    let conn = db.conn();
    let outcomes = rows::outcomes(conn, &attempt.id)?;
    let rollup = progress::rollup(&package.description, &outcomes);
    if rollup.complete && attempt.status == PackageAttemptStatus::InProgress {
        rows::mark_completed(conn, &attempt.id)?;
    }
    let seconds = rows::element_seconds(conn, &attempt.enrollment_id, &attempt.element_id)?;
    let already_complete = rows::element_status(conn, &attempt.enrollment_id, &attempt.element_id)?
        .is_some_and(|status| status == "completed");
    let status = if rollup.complete || already_complete {
        "completed"
    } else {
        "in_progress"
    };
    let element_progress = crate::enrollment::update_progress(
        db,
        attempt.enrollment_id.clone(),
        UpdateProgressRequest {
            element_id: attempt.element_id.clone(),
            status: status.to_string(),
            score: rollup.score,
            time_spent: Some(seconds.round() as i64),
            integrity_session_id: None,
            integrity_score: None,
        },
    )?;
    Ok(PackageAttemptProgress {
        attempt: rows::get(conn, &attempt.id)?,
        element_progress,
        package_completed: rollup.complete,
        cache_dropped: false,
    })
}

fn stored_package(conn: &rusqlite::Connection, hash: &str) -> Result<Package, AttemptError> {
    super::rows::get(conn, hash)
        .map_err(AttemptError::Database)?
        .ok_or_else(|| AttemptError::NotAPackage(format!("package {hash} is not stored")))
}

/// The package's launchable activity with this identifier.
fn launchable<'a>(package: &'a Package, activity_id: &str) -> Result<&'a Activity, AttemptError> {
    package
        .description
        .launchable_activities()
        .into_iter()
        .find(|activity| activity.id == activity_id)
        .ok_or_else(|| {
            AttemptError::NotFound(format!(
                "activity {activity_id} is not a lesson of package {}",
                package.hash
            ))
        })
}
