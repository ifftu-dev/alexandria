//! Previewing a package element: an author runs a package of their own
//! course as a learner would see it, without an enrolment.
//!
//! A preview follows the attempt lifecycle the player drives (open,
//! initialise per lesson, coalesced commits, terminate, start over) and
//! answers with the same types, but holds its run-time state in memory
//! only. Nothing is written: no attempt, no element progress, no learning
//! record, and so nothing syncs or reaches a guardian or a record recipient.
//! A preview ends when the player terminates it or the profile locks.
//!
//! The reading of run-time data and the rollup are the attempt rules from
//! [`alexandria_studio::package::progress`], so a preview completes when a
//! real attempt would.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use alexandria_studio::package::progress::{self, ActivityOutcome, HostValues};
use rusqlite::OptionalExtension;
use serde_json::Value;

use super::attempts::{self, AttemptError, MAX_SEQUENCING_BYTES};
use super::PackageDb;
use crate::domain::course_document::PACKAGE_ELEMENT_TYPE;
use crate::domain::enrollment::ElementProgress;
use crate::domain::package::Package;
use crate::domain::package_attempt::{
    PackageActivityResult, PackageAttempt, PackageAttemptProgress, PackageAttemptStatus,
    PackageCommit, PackageLaunchInit, PackageSequencingState, RootStatus,
};

/// Every preview attempt id starts with this.
pub const PREVIEW_ID_PREFIX: &str = "preview-";

/// The most previews held at once; opening another drops the oldest.
pub const MAX_OPEN_PREVIEWS: usize = 4;

#[derive(Debug, Clone)]
struct ActivityState {
    runtime: Value,
    complete: bool,
    score: Option<f64>,
    total_seconds: f64,
    session_seconds: f64,
}

#[derive(Debug, Clone)]
struct Preview {
    attempt: PackageAttempt,
    package: Package,
    activities: BTreeMap<String, ActivityState>,
    /// SCORM 2004: the engine's last snapshot and its rollup of the root.
    sequencing: Option<(String, RootStatus)>,
    opened_order: u64,
}

/// The open previews of the active profile, in memory.
#[derive(Debug, Default)]
pub struct Previews {
    open: Mutex<(u64, HashMap<String, Preview>)>,
}

impl Previews {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets every preview. Called when the profile locks.
    pub fn clear(&self) {
        if let Ok(mut open) = self.open.lock() {
            open.1.clear();
        }
    }

    fn with<T>(
        &self,
        attempt_id: &str,
        operation: impl FnOnce(&mut Preview) -> Result<T, AttemptError>,
    ) -> Result<T, AttemptError> {
        let mut open = self
            .open
            .lock()
            .map_err(|_| AttemptError::Database("preview lock poisoned".into()))?;
        let preview = open
            .1
            .get_mut(attempt_id)
            .ok_or_else(|| AttemptError::NotFound(format!("preview {attempt_id}")))?;
        operation(preview)
    }

    fn insert(&self, attempt: PackageAttempt, package: Package) -> Result<(), AttemptError> {
        let mut open = self
            .open
            .lock()
            .map_err(|_| AttemptError::Database("preview lock poisoned".into()))?;
        open.0 += 1;
        let order = open.0;
        let previews = &mut open.1;
        // One preview per element: starting over replaces the last one.
        previews.retain(|_, preview| preview.attempt.element_id != attempt.element_id);
        while previews.len() >= MAX_OPEN_PREVIEWS {
            let oldest = previews
                .iter()
                .min_by_key(|(_, preview)| preview.opened_order)
                .map(|(id, _)| id.clone());
            match oldest {
                Some(id) => previews.remove(&id),
                None => break,
            };
        }
        previews.insert(
            attempt.id.clone(),
            Preview {
                attempt,
                package,
                activities: BTreeMap::new(),
                sequencing: None,
                opened_order: order,
            },
        );
        Ok(())
    }

    fn remove(&self, attempt_id: &str) -> Result<Option<Preview>, AttemptError> {
        let mut open = self
            .open
            .lock()
            .map_err(|_| AttemptError::Database("preview lock poisoned".into()))?;
        Ok(open.1.remove(attempt_id))
    }

    fn number_for(&self, element_id: &str) -> i64 {
        self.open
            .lock()
            .ok()
            .and_then(|open| {
                open.1
                    .values()
                    .filter(|preview| preview.attempt.element_id == element_id)
                    .map(|preview| preview.attempt.attempt_number)
                    .max()
            })
            .unwrap_or(0)
    }
}

/// Opens a preview of a package element of a course the local identity
/// authored. Refused (`not_found`) for anyone else's course.
pub async fn open(
    db: &impl PackageDb,
    previews: &Previews,
    element_id: &str,
) -> Result<PackageAttempt, AttemptError> {
    start(db, previews, element_id, 1).await
}

/// Drops the element's current preview and opens a new one with no state.
pub async fn fresh(
    db: &impl PackageDb,
    previews: &Previews,
    element_id: &str,
) -> Result<PackageAttempt, AttemptError> {
    let number = previews.number_for(element_id) + 1;
    start(db, previews, element_id, number).await
}

async fn start(
    db: &impl PackageDb,
    previews: &Previews,
    element_id: &str,
    attempt_number: i64,
) -> Result<PackageAttempt, AttemptError> {
    let element = element_id.to_string();
    let package = db
        .run("packages.preview-open", move |db| {
            Ok(authored_package(db.conn(), &element))
        })
        .await
        .map_err(AttemptError::Database)??;
    let now = crate::clock::now_rfc3339();
    let attempt = PackageAttempt {
        id: format!("{PREVIEW_ID_PREFIX}{}", uuid::Uuid::new_v4()),
        enrollment_id: String::new(),
        element_id: element_id.to_string(),
        package_hash: package.hash.clone(),
        attempt_number,
        status: PackageAttemptStatus::InProgress,
        started_at: now.clone(),
        ended_at: None,
        updated_at: now,
    };
    previews.insert(attempt.clone(), package)?;
    Ok(attempt)
}

/// The package of a package element whose course the local identity wrote.
fn authored_package(
    conn: &rusqlite::Connection,
    element_id: &str,
) -> Result<Package, AttemptError> {
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT e.element_type, e.content_cid FROM course_elements e \
             JOIN course_chapters ch ON ch.id = e.chapter_id \
             JOIN courses c ON c.id = ch.course_id \
             JOIN local_identity li ON li.id = 1 AND li.stake_address = c.author_address \
             WHERE e.id = ?1",
            [element_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((element_type, content)) = row else {
        return Err(AttemptError::NotFound(format!(
            "element {element_id} in a course you wrote"
        )));
    };
    let hash = content
        .filter(|_| element_type == PACKAGE_ELEMENT_TYPE)
        .ok_or_else(|| AttemptError::NotAPackage(format!("element {element_id} has no package")))?;
    super::rows::get(conn, &hash)?
        .ok_or_else(|| AttemptError::NotAPackage(format!("package {hash} is not stored")))
}

/// The initialisation data for one lesson of a preview, built as for an
/// attempt: the same pseudonymous identifier (from `learner_key`), the
/// display name, the manifest's values and the preview's state so far.
pub async fn initialise(
    db: &impl PackageDb,
    previews: &Previews,
    learner_key: &[u8; 32],
    attempt_id: &str,
    activity_id: &str,
) -> Result<PackageLaunchInit, AttemptError> {
    initialise_delivery(db, previews, learner_key, attempt_id, activity_id, false).await
}

/// [`initialise`] for an activity the SCORM 2004 sequencing engine delivered,
/// `resumed` as for [`super::attempts::initialise_delivery`].
pub async fn initialise_delivery(
    db: &impl PackageDb,
    previews: &Previews,
    learner_key: &[u8; 32],
    attempt_id: &str,
    activity_id: &str,
    resumed: bool,
) -> Result<PackageLaunchInit, AttemptError> {
    let learner_name = db
        .run("packages.preview-initialise", |db| {
            let name: Option<Option<String>> = db
                .conn()
                .query_row(
                    "SELECT display_name FROM local_identity WHERE id = 1",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            Ok(name.flatten().unwrap_or_default())
        })
        .await
        .map_err(AttemptError::Database)?;
    previews.with(attempt_id, |preview| {
        let package = preview.package.clone();
        let activity = package
            .description
            .launchable_activities()
            .into_iter()
            .find(|activity| activity.id == activity_id)
            .ok_or_else(|| AttemptError::NotFound(format!("activity {activity_id}")))?;
        let launch = activity
            .launch
            .as_ref()
            .ok_or_else(|| AttemptError::NotFound(format!("activity {activity_id}")))?;

        // A session the content left open ends here, as for an attempt.
        if let Some(state) = preview.activities.get_mut(activity_id) {
            state.total_seconds += state.session_seconds;
            state.session_seconds = 0.0;
        }
        let stored = preview.activities.get(activity_id).cloned();
        let mut launched = None;
        if progress::completes_on_launch(launch) {
            preview.activities.insert(
                activity_id.to_string(),
                ActivityState {
                    runtime: serde_json::json!({}),
                    complete: true,
                    score: None,
                    total_seconds: 0.0,
                    session_seconds: 0.0,
                },
            );
            launched = Some(roll_up(preview));
        }
        let standard = package.standard;
        let entry =
            progress::delivery_entry(standard, stored.as_ref().map(|s| &s.runtime), resumed);
        let learner_id = progress::learner_id(learner_key, &package.hash);
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
            attempt_id: preview.attempt.id.clone(),
            activity_id: activity_id.to_string(),
            standard,
            learner_id,
            learner_name: learner_name.clone(),
            entry: entry.as_str().to_string(),
            launch_data: launch.launch_data.clone(),
            mastery_score: launch.mastery_score,
            max_time_allowed: launch.max_time_allowed.clone(),
            time_limit_action: launch.time_limit_action.clone(),
            runtime,
            progress: launched,
        })
    })
}

/// Holds one coalesced commit in the preview and rolls it up as an attempt
/// would. Refuses what an attempt refuses (`invalid_commit`).
pub fn commit(
    previews: &Previews,
    commit: PackageCommit,
) -> Result<PackageAttemptProgress, AttemptError> {
    attempts::checked_runtime(&commit)?;
    previews.with(&commit.attempt_id, |preview| {
        attempts::launchable(&preview.package, &commit.activity_id)?;
        let report = progress::read_report(preview.package.standard, &commit.runtime);
        let prior_total = preview
            .activities
            .get(&commit.activity_id)
            .map_or(0.0, |state| state.total_seconds);
        let (total_seconds, session_seconds) =
            attempts::activity_time(prior_total, report.session_seconds, commit.session_ended);
        preview.activities.insert(
            commit.activity_id.clone(),
            ActivityState {
                runtime: commit.runtime.clone(),
                complete: report.complete,
                score: report.score,
                total_seconds,
                session_seconds,
            },
        );
        Ok(roll_up(preview))
    })
}

/// Holds the SCORM 2004 engine's state in the preview and rolls it up as an
/// attempt would: complete when the engine reports the root completed.
pub fn save_sequencing(
    previews: &Previews,
    state: PackageSequencingState,
) -> Result<PackageAttemptProgress, AttemptError> {
    if state.snapshot.len() > MAX_SEQUENCING_BYTES {
        return Err(AttemptError::InvalidCommit(format!(
            "sequencing state is {} bytes; the limit is {MAX_SEQUENCING_BYTES}",
            state.snapshot.len()
        )));
    }
    previews.with(&state.attempt_id, |preview| {
        preview.sequencing = Some((state.snapshot, state.root));
        Ok(roll_up(preview))
    })
}

/// The SCORM 2004 engine snapshot the preview holds, if any.
pub fn sequencing(previews: &Previews, attempt_id: &str) -> Result<Option<String>, AttemptError> {
    previews.with(attempt_id, |preview| {
        Ok(preview
            .sequencing
            .as_ref()
            .map(|(snapshot, _)| snapshot.clone()))
    })
}

/// What every lesson of a preview has reported, in learner order.
pub fn activities(
    previews: &Previews,
    attempt_id: &str,
) -> Result<Vec<PackageActivityResult>, AttemptError> {
    previews.with(attempt_id, |preview| {
        Ok(preview
            .package
            .description
            .launchable_activities()
            .into_iter()
            .map(|activity| {
                let state = preview.activities.get(&activity.id);
                PackageActivityResult {
                    activity_id: activity.id.clone(),
                    started: state.is_some(),
                    complete: state.is_some_and(|state| state.complete),
                    score: state.and_then(|state| state.score),
                }
            })
            .collect())
    })
}

/// Ends a preview and forgets it.
pub fn terminate(
    previews: &Previews,
    attempt_id: &str,
) -> Result<PackageAttemptProgress, AttemptError> {
    let ended = previews.with(attempt_id, |preview| {
        for state in preview.activities.values_mut() {
            state.total_seconds += state.session_seconds;
            state.session_seconds = 0.0;
        }
        Ok(roll_up(preview))
    })?;
    previews.remove(attempt_id)?;
    Ok(ended)
}

/// The preview's rollup, shaped like an attempt's: element progress here is
/// what a learner's would be, but it is never written anywhere.
fn roll_up(preview: &mut Preview) -> PackageAttemptProgress {
    let outcomes: BTreeMap<String, ActivityOutcome> = preview
        .activities
        .iter()
        .map(|(id, state)| {
            (
                id.clone(),
                ActivityOutcome {
                    complete: state.complete,
                    score: state.score,
                },
            )
        })
        .collect();
    let rollup = progress::rollup(
        &preview.package.description,
        &outcomes,
        preview.sequencing.as_ref().map(|(_, root)| root),
    );
    let now = crate::clock::now_rfc3339();
    if rollup.complete && preview.attempt.status == PackageAttemptStatus::InProgress {
        preview.attempt.status = PackageAttemptStatus::Completed;
        preview.attempt.ended_at = Some(now.clone());
    }
    preview.attempt.updated_at = now.clone();
    let seconds: f64 = preview
        .activities
        .values()
        .map(|state| state.total_seconds + state.session_seconds)
        .sum();
    let complete = preview.attempt.status == PackageAttemptStatus::Completed;
    PackageAttemptProgress {
        attempt: preview.attempt.clone(),
        element_progress: ElementProgress {
            id: preview.attempt.id.clone(),
            enrollment_id: String::new(),
            element_id: preview.attempt.element_id.clone(),
            status: if complete { "completed" } else { "in_progress" }.to_string(),
            score: rollup.score,
            time_spent: seconds.round() as i64,
            completed_at: preview.attempt.ended_at.clone(),
            updated_at: now,
        },
        package_completed: rollup.complete,
        cache_dropped: false,
    }
}
