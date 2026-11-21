//! The record writers the host calls, and the one insert they share.

use alexandria_studio::package::Standard;
use alexandria_studio::xapi::scorm::{self, ScormEdition, ScormSession};
use alexandria_studio::xapi::{
    Action, ActivityRef, LanguageMap, LearningEvent, Outcome, Profile, Score, Target,
};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::{validate, RecordError, LIMIT_BYTES};
use crate::domain::course_document::{
    ASSESSED_ELEMENT_TYPES, ASSESSMENT_PASS_SCORE, PACKAGE_ELEMENT_TYPE,
};
use crate::domain::enrollment::{ElementProgress, Enrollment};
use crate::domain::package::Package;
use crate::domain::package_attempt::PackageAttempt;
use crate::settings::{registry::keys, SettingsStore};

/// Language tag of the names records carry: course and element titles are
/// in whatever language their author wrote, which is not recorded.
const NAME_LANGUAGE: &str = "und";

/// The profile's DID, which every local record names its learner by.
pub(super) fn learner_did(conn: &Connection) -> Result<String, RecordError> {
    let did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
    if did.is_empty() {
        return Err(RecordError::Unnamed("the profile has no DID yet".into()));
    }
    Ok(did)
}

/// What a record is about, beyond its event.
struct Placement<'a> {
    course_id: &'a str,
    element_id: Option<&'a str>,
    registration: Option<&'a str>,
}

/// Validates and stores one event. A record whose local id is already in
/// the log is left as it is, so every writer can be repeated.
fn insert(
    conn: &Connection,
    learner_did: &str,
    placement: &Placement<'_>,
    event: &LearningEvent,
) -> Result<(), RecordError> {
    validate(event, learner_did)?;
    let activity_id = event_activity(event)?.iri()?;
    let kind = match event.kind() {
        alexandria_studio::xapi::RecordKind::Completion => "completion",
        alexandria_studio::xapi::RecordKind::Interaction => "interaction",
    };
    let event_json = serde_json::to_string(event)?;
    conn.execute(
        "INSERT INTO learning_records \
         (id, learner_did, kind, verb, course_id, element_id, registration, activity_id, \
          event_json, bytes, recorded_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
         ON CONFLICT(id) DO NOTHING",
        params![
            event.local_id,
            learner_did,
            kind,
            event.action.verb().display(),
            placement.course_id,
            placement.element_id,
            placement.registration,
            activity_id,
            event_json,
            event_json.len() as i64,
            timestamp(event.timestamp),
        ],
    )?;
    super::enforce_limit(conn, LIMIT_BYTES)?;
    Ok(())
}

fn event_activity(event: &LearningEvent) -> Result<&ActivityRef, RecordError> {
    Ok(match &event.action {
        Action::Registered { target }
        | Action::Launched { target }
        | Action::Progressed { target, .. }
        | Action::Completed { target, .. }
        | Action::Passed { target, .. }
        | Action::Failed { target, .. }
        | Action::Scored { target, .. }
        | Action::Abandoned { target, .. }
        | Action::Satisfied { target, .. } => &target.activity,
        Action::Responded { interaction, .. } => &interaction.activity,
        Action::Voided { .. } => {
            return Err(RecordError::Database(
                "a voiding statement is built when sending, never stored".into(),
            ))
        }
    })
}

fn timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// The course's creator DID and title. A course this profile authored and
/// never received from elsewhere has no stored creator: its creator is the
/// profile itself.
fn course(conn: &Connection, course_id: &str, learner_did: &str) -> Result<Course, RecordError> {
    let row: Option<(Option<String>, String)> = conn
        .query_row(
            "SELECT COALESCE(c.creator_did, \
                    CASE WHEN c.author_address = li.stake_address THEN ?2 END), c.title \
             FROM courses c LEFT JOIN local_identity li ON li.id = 1 WHERE c.id = ?1",
            params![course_id, learner_did],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match row {
        Some((Some(creator_did), title)) => Ok(Course {
            activity: ActivityRef::course(creator_did, course_id),
            title,
        }),
        Some((None, _)) => Err(RecordError::Unnamed(format!(
            "course {course_id} has no known creator"
        ))),
        None => Err(RecordError::Unnamed(format!(
            "course {course_id} is not stored"
        ))),
    }
}

struct Course {
    activity: ActivityRef,
    title: String,
}

/// An element of an enrolment's course, as records name it.
struct Element {
    course_id: String,
    course: Course,
    activity: ActivityRef,
    title: String,
    element_type: String,
}

fn element(
    conn: &Connection,
    enrollment_id: &str,
    element_id: &str,
    learner_did: &str,
) -> Result<Element, RecordError> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT e.course_id, ce.title, ce.element_type FROM enrollments e \
             JOIN course_chapters ch ON ch.course_id = e.course_id \
             JOIN course_elements ce ON ce.chapter_id = ch.id \
             WHERE e.id = ?1 AND ce.id = ?2",
            params![enrollment_id, element_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((course_id, title, element_type)) = row else {
        return Err(RecordError::Unnamed(format!(
            "element {element_id} is not in the course of enrolment {enrollment_id}"
        )));
    };
    let course = course(conn, &course_id, learner_did)?;
    Ok(Element {
        activity: course.activity.element(element_id),
        course_id,
        course,
        title,
        element_type,
    })
}

/// An element's progress as it stood before an update.
#[derive(Debug, Clone, PartialEq)]
pub struct PriorProgress {
    pub status: String,
    pub score: Option<f64>,
}

/// Records what an element progress update means:
///
/// - the first move to `in_progress` is the element's launch;
/// - a move to `completed` is a completion, or for an assessed element with
///   a score a pass or fail at [`ASSESSMENT_PASS_SCORE`] (result and score
///   only, never the answers). A later `completed` with a different score
///   (a retake) is recorded again, except for a package element, whose
///   results the package records itself;
/// - each element newly completed records the course's progress, as the
///   whole percentage of its elements completed.
///
/// [`ASSESSMENT_PASS_SCORE`]: crate::domain::course_document::ASSESSMENT_PASS_SCORE
pub fn record_progress(
    conn: &Connection,
    prior: Option<&PriorProgress>,
    progress: &ElementProgress,
) -> Result<(), RecordError> {
    let prior_status = prior.map(|p| p.status.as_str());
    let newly_completed = progress.status == "completed" && prior_status != Some("completed");
    let launch = progress.status == "in_progress"
        && !matches!(prior_status, Some("in_progress") | Some("completed"));
    if !launch && progress.status != "completed" {
        return Ok(());
    }
    let learner_did = learner_did(conn)?;
    let element = element(
        conn,
        &progress.enrollment_id,
        &progress.element_id,
        &learner_did,
    )?;
    if !newly_completed
        && !launch
        && (element.element_type == PACKAGE_ELEMENT_TYPE
            || prior.and_then(|p| p.score) == progress.score)
    {
        return Ok(());
    }
    let placement = Placement {
        course_id: &element.course_id,
        element_id: Some(&progress.element_id),
        registration: Some(&progress.enrollment_id),
    };
    let target = Target::named(element.activity.clone(), NAME_LANGUAGE, &element.title);
    let now = Utc::now();
    let event = |local_id: String, action: Action| LearningEvent {
        local_id,
        timestamp: now,
        registration: Some(progress.enrollment_id.clone()),
        action,
        profile: None,
    };
    if launch {
        let local_id = format!(
            "launched|{}|{}",
            progress.enrollment_id, progress.element_id
        );
        return insert(
            conn,
            &learner_did,
            &placement,
            &event(local_id, Action::Launched { target }),
        );
    }

    let outcome = Outcome {
        score: progress.score.map(|scaled| Score {
            scaled: Some(scaled),
            ..Score::default()
        }),
        duration_centiseconds: u64::try_from(progress.time_spent)
            .ok()
            .filter(|seconds| *seconds > 0)
            .map(|seconds| seconds * 100),
        ..Outcome::default()
    };
    let assessed = ASSESSED_ELEMENT_TYPES.contains(&element.element_type.as_str());
    let action = match progress.score {
        Some(score) if assessed && score >= ASSESSMENT_PASS_SCORE => Action::Passed {
            target,
            outcome: Outcome {
                completion: Some(true),
                ..outcome
            },
        },
        Some(_) if assessed => Action::Failed {
            target,
            outcome: Outcome {
                completion: Some(true),
                ..outcome
            },
        },
        _ => Action::Completed { target, outcome },
    };
    let local_id = format!("result|{}", uuid::Uuid::new_v4());
    insert(conn, &learner_did, &placement, &event(local_id, action))?;

    if newly_completed {
        let percent = course_percent(conn, &progress.enrollment_id, &element.course_id)?;
        let course_event = event(
            format!("progressed|{}|{percent}", progress.enrollment_id),
            Action::Progressed {
                target: Target::named(
                    element.course.activity.clone(),
                    NAME_LANGUAGE,
                    &element.course.title,
                ),
                percent,
            },
        );
        insert(
            conn,
            &learner_did,
            &Placement {
                element_id: None,
                ..placement
            },
            &course_event,
        )?;
    }
    Ok(())
}

/// The whole percentage of the course's elements the enrolment completed.
fn course_percent(
    conn: &Connection,
    enrollment_id: &str,
    course_id: &str,
) -> Result<u8, RecordError> {
    let (completed, total): (i64, i64) = conn.query_row(
        "SELECT \
           (SELECT COUNT(*) FROM element_progress ep \
              JOIN course_elements ce ON ce.id = ep.element_id \
              JOIN course_chapters ch ON ch.id = ce.chapter_id \
            WHERE ep.enrollment_id = ?1 AND ch.course_id = ?2 AND ep.status = 'completed'), \
           (SELECT COUNT(*) FROM course_elements ce \
              JOIN course_chapters ch ON ch.id = ce.chapter_id WHERE ch.course_id = ?2)",
        params![enrollment_id, course_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if total == 0 {
        return Ok(0);
    }
    Ok(u8::try_from((completed.min(total) * 100) / total).unwrap_or(100))
}

/// A package activity of an attempt, as records name it.
fn package_activity(
    conn: &Connection,
    attempt: &PackageAttempt,
    package: &Package,
    activity_id: &str,
    learner_did: &str,
) -> Result<(Element, ActivityRef, Option<LanguageMap>), RecordError> {
    let element = element(
        conn,
        &attempt.enrollment_id,
        &attempt.element_id,
        learner_did,
    )?;
    let activity = element
        .activity
        .package_activity(&attempt.package_hash, activity_id);
    let name = package
        .description
        .launchable_activities()
        .into_iter()
        .find(|a| a.id == activity_id)
        .map(|a| LanguageMap::from([(NAME_LANGUAGE.to_string(), a.title.clone())]));
    Ok((element, activity, name))
}

fn profile_of(standard: Standard) -> Option<Profile> {
    match standard {
        Standard::Scorm12 | Standard::Scorm2004 => Some(Profile::Scorm),
        Standard::Cmi5 => Some(Profile::Cmi5),
    }
}

/// The identifier of a package activity of an attempt (a unit, block or
/// course node for cmi5): the element's DID URL extended with the package
/// hash and the activity's own identifier. It is what a cmi5 unit is given
/// as its `activityId` and what its records are about.
pub fn package_activity_ref(
    conn: &Connection,
    attempt: &PackageAttempt,
    activity_id: &str,
) -> Result<ActivityRef, RecordError> {
    let learner_did = learner_did(conn)?;
    let element = element(
        conn,
        &attempt.enrollment_id,
        &attempt.element_id,
        &learner_did,
    )?;
    let activity = element
        .activity
        .package_activity(&attempt.package_hash, activity_id);
    activity.iri()?;
    Ok(activity)
}

/// What the launching system records for a cmi5 session.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmi5Record {
    /// The unit reported `completed`, `passed` or `failed` (its verb), with
    /// the score and duration it gave.
    Result {
        verb: alexandria_studio::xapi::cmi5::UnitVerb,
        score: Option<f64>,
        duration_seconds: Option<f64>,
    },
    /// The session ended without `terminated`, after this long.
    Abandoned { seconds: f64 },
    /// A block or the course was satisfied.
    Satisfied(alexandria_studio::xapi::Satisfiable),
}

/// Records one cmi5 event about `activity_id` (a unit, or for
/// [`Cmi5Record::Satisfied`] a block or the course, by course structure
/// identifier) in an attempt. `local_id` keys the record: the same id is
/// written once.
pub fn record_cmi5(
    conn: &Connection,
    attempt: &PackageAttempt,
    package: &Package,
    activity_id: &str,
    local_id: String,
    record: &Cmi5Record,
) -> Result<(), RecordError> {
    use alexandria_studio::xapi::cmi5::UnitVerb;
    let learner_did = learner_did(conn)?;
    let element = element(
        conn,
        &attempt.enrollment_id,
        &attempt.element_id,
        &learner_did,
    )?;
    let activity = element
        .activity
        .package_activity(&attempt.package_hash, activity_id);
    let name = find_activity(&package.description.tree, activity_id)
        .map(|a| LanguageMap::from([(NAME_LANGUAGE.to_string(), a.title.clone())]));
    let target = Target { activity, name };
    let centiseconds = |seconds: Option<f64>| seconds.map(|s| (s.max(0.0) * 100.0).round() as u64);
    let action = match record {
        Cmi5Record::Result {
            verb,
            score,
            duration_seconds,
        } => {
            let outcome = Outcome {
                score: score.map(|scaled| Score {
                    scaled: Some(scaled),
                    ..Score::default()
                }),
                duration_centiseconds: centiseconds(*duration_seconds),
                ..Outcome::default()
            };
            match verb {
                UnitVerb::Passed => Action::Passed { target, outcome },
                UnitVerb::Failed => Action::Failed { target, outcome },
                UnitVerb::Completed => Action::Completed {
                    target,
                    outcome: Outcome {
                        score: None,
                        ..outcome
                    },
                },
                UnitVerb::Initialized | UnitVerb::Terminated => return Ok(()),
            }
        }
        Cmi5Record::Abandoned { seconds } => Action::Abandoned {
            target,
            outcome: Outcome {
                duration_centiseconds: centiseconds(Some(*seconds)),
                ..Outcome::default()
            },
        },
        Cmi5Record::Satisfied(scope) => Action::Satisfied {
            target,
            scope: *scope,
        },
    };
    let event = LearningEvent {
        local_id,
        timestamp: Utc::now(),
        registration: Some(attempt.id.clone()),
        action,
        profile: Some(Profile::Cmi5),
    };
    insert(
        conn,
        &learner_did,
        &Placement {
            course_id: &element.course_id,
            element_id: Some(&attempt.element_id),
            registration: Some(&attempt.id),
        },
        &event,
    )
}

fn find_activity<'a>(
    node: &'a alexandria_studio::package::Activity,
    id: &str,
) -> Option<&'a alexandria_studio::package::Activity> {
    if node.id == id {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_activity(child, id))
}

/// Records one launch of one activity of a package attempt.
pub fn record_package_launch(
    conn: &Connection,
    attempt: &PackageAttempt,
    package: &Package,
    activity_id: &str,
) -> Result<(), RecordError> {
    let learner_did = learner_did(conn)?;
    let (element, activity, name) =
        package_activity(conn, attempt, package, activity_id, &learner_did)?;
    let event = LearningEvent {
        local_id: format!(
            "launched|{}|{activity_id}|{}",
            attempt.id,
            uuid::Uuid::new_v4()
        ),
        timestamp: Utc::now(),
        registration: Some(attempt.id.clone()),
        action: Action::Launched {
            target: Target { activity, name },
        },
        profile: profile_of(package.standard),
    };
    insert(
        conn,
        &learner_did,
        &Placement {
            course_id: &element.course_id,
            element_id: Some(&attempt.element_id),
            registration: Some(&attempt.id),
        },
        &event,
    )
}

/// Records what a package reported for one activity of an attempt, from the
/// run-time data it stored: the result (pass, fail, completion or score,
/// following the xAPI SCORM profile) and one interaction-level record per
/// interaction with a response. Called when a session ends and when the
/// attempt's launch ends.
///
/// Each attempt, activity and verb has one result record, and each
/// interaction index one response record; a repeat is ignored, so calling
/// this again for the same state writes nothing. cmi5 records are the cmi5
/// session's own and are not read here.
pub fn record_package_session(
    conn: &Connection,
    attempt: &PackageAttempt,
    package: &Package,
    activity_id: &str,
    runtime: &serde_json::Value,
) -> Result<(), RecordError> {
    let edition = match package.standard {
        Standard::Scorm12 => ScormEdition::Scorm12,
        Standard::Scorm2004 => ScormEdition::Scorm2004,
        Standard::Cmi5 => return Ok(()),
    };
    let cmi = scorm::flatten_cmi(runtime);
    if cmi.is_empty() {
        return Ok(());
    }
    let learner_did = learner_did(conn)?;
    let (element, activity, name) =
        package_activity(conn, attempt, package, activity_id, &learner_did)?;
    let session = ScormSession {
        edition,
        activity,
        name,
        local_id: format!("package|{}|{activity_id}", attempt.id),
        registration: Some(attempt.id.clone()),
        timestamp: Utc::now(),
        language: NAME_LANGUAGE.to_string(),
    };
    let placement = Placement {
        course_id: &element.course_id,
        element_id: Some(&attempt.element_id),
        registration: Some(&attempt.id),
    };
    for mut event in scorm::events(&session, &cmi) {
        if event.local_id == session.local_id {
            event.local_id = format!("{}|{}", session.local_id, event.action.verb().display());
        }
        insert(conn, &learner_did, &placement, &event)?;
    }
    Ok(())
}

/// Records the learner's enrolment in a course.
pub fn record_enrolment(conn: &Connection, enrollment: &Enrollment) -> Result<(), RecordError> {
    let learner_did = learner_did(conn)?;
    let course = course(conn, &enrollment.course_id, &learner_did)?;
    let event = LearningEvent {
        local_id: format!("registered|{}", enrollment.id),
        timestamp: Utc::now(),
        registration: Some(enrollment.id.clone()),
        action: Action::Registered {
            target: Target::named(course.activity, NAME_LANGUAGE, &course.title),
        },
        profile: None,
    };
    insert(
        conn,
        &learner_did,
        &Placement {
            course_id: &enrollment.course_id,
            element_id: None,
            registration: Some(&enrollment.id),
        },
        &event,
    )
}
