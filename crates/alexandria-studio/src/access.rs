//! AI access: why an AI surface denies access, and which lesson text a
//! model may see.
//!
//! The age, guardian-approval and attempt rules are applied by the app's
//! `ai_access` module, which owns the profile's identity, the guardian link
//! and the integrity flags. This crate holds the typed reasons, so the two
//! kinds of caller map them once each: chat, whose errors are the string
//! codes the webview translates (the app's chat vocabulary renders them), and
//! the lesson tutor and Studio runs, whose errors are [`Error`] ([`run_error`]).
//! A denial is never a Studio error, and a Studio error is never a denial.

use rusqlite::Connection;

use crate::learning::{self, LessonTarget};
use crate::{store, Error};

/// Why a learner may not use an AI surface, a connection, or a lesson now.
/// Its display is the reason written for people, without a code.
#[derive(Debug, thiserror::Error)]
pub enum AccessDenied {
    /// Under 13: models on the device or the learner's own network only.
    #[error("learners under 13 can use models on this device or their own network only")]
    TooYoungForRemote,
    /// 13 to 17, and the vendor's terms require users to be 18 or older.
    #[error("{0} requires users to be 18 or older")]
    AdultsOnly(&'static str),
    /// 13 to 17, and no guardian has been asked about this endpoint.
    #[error(
        "remote models need a guardian's approval; use a model on this device or your own network"
    )]
    GuardianRequired,
    #[error("waiting for a guardian to approve this model")]
    GuardianPending,
    #[error("a guardian did not approve this model")]
    GuardianDeclined,
    /// Asking for an approval needs a linked guardian.
    #[error("link a guardian in Settings first")]
    NoGuardian,
    /// Only remote models need a guardian's approval.
    #[error("only remote models need a guardian's approval")]
    NotRemote,
    #[error("the chosen model connection is disabled")]
    ConnectionDisabled,
    #[error("the chosen model connection does not answer text")]
    WrongCapability,
    /// An assessment attempt or interview check is in progress.
    #[error("AI assistance is paused while an assessment is in progress")]
    AttemptInProgress,
    /// No such lesson, or one in a course the reader may not read: whether
    /// it exists is not said.
    #[error("no published course or lesson with that identifier")]
    LessonNotFound,
    /// Not a text lesson: graded, interactive and video elements are never
    /// shown to a model.
    #[error("only text lessons can be attached")]
    LessonWithheld,
    /// What could not be read.
    #[error("{0} could not be read")]
    Storage(&'static str),
    /// The Studio data the rule reads (lessons, the attempt gate) could not
    /// be accessed.
    #[error("studio data could not be accessed")]
    Database(#[source] rusqlite::Error),
    #[error("{0}")]
    Invalid(String),
}

/// How the tutor and Studio runs show a denial: an attempt keeps its own
/// error, a lesson the tutor may not show is a permission error (whether it
/// exists is not said), and every other denial is shown as its reason.
pub fn run_error(denied: AccessDenied) -> Error {
    match denied {
        AccessDenied::AttemptInProgress => Error::AssessmentActive,
        AccessDenied::LessonNotFound | AccessDenied::LessonWithheld => Error::Permission,
        AccessDenied::ConnectionDisabled | AccessDenied::WrongCapability => {
            Error::Unavailable("Choose an enabled text model".into())
        }
        AccessDenied::Database(error) => Error::Storage(error),
        AccessDenied::Invalid(message) => Error::Invalid(message),
        other => Error::Unavailable(other.to_string()),
    }
}

/// Which surface wants to show a lesson to a model. They differ only in which
/// courses they may read; the lesson rule is the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reader {
    /// Global chat: published courses written by someone else, as the
    /// assistant broker reads them.
    Chat,
    /// The lesson tutor: any course the learner takes or wrote.
    Tutor,
}

/// The lesson whose text a model may see: an element of `course_id` the
/// reader may read, and a text lesson. Quizzes, essays, multiple-choice and
/// other graded elements, assessments, interactive elements, plugins and
/// video are withheld from every model.
pub fn visible_lesson(
    conn: &Connection,
    reader: Reader,
    course_id: &str,
    element_id: &str,
) -> Result<LessonTarget, AccessDenied> {
    let found = match reader {
        Reader::Chat => learning::lesson_target(conn, course_id, element_id),
        Reader::Tutor => store::can_use_tutor(conn, course_id)
            .and_then(|()| learning::course_lesson(conn, course_id, element_id)),
    };
    let target = found.map_err(|error| match error {
        Error::NotFound | Error::Permission => AccessDenied::LessonNotFound,
        Error::Storage(error) => AccessDenied::Database(error),
        Error::Invalid(message) => AccessDenied::Invalid(message),
        // The lookups produce no other error.
        other => AccessDenied::Invalid(other.to_string()),
    })?;
    if learning::content_kind(&target.element_type) != "text" {
        return Err(AccessDenied::LessonWithheld);
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tutor_and_studio_runs_see_the_reason_without_its_code() {
        assert!(matches!(
            run_error(AccessDenied::AttemptInProgress),
            Error::AssessmentActive
        ));
        match run_error(AccessDenied::AdultsOnly("Groq")) {
            Error::Unavailable(message) => {
                assert_eq!(message, "Groq requires users to be 18 or older")
            }
            other => panic!("{other:?}"),
        }
        match run_error(AccessDenied::GuardianRequired) {
            Error::Unavailable(message) => assert!(message.starts_with("remote models need")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            run_error(AccessDenied::Database(rusqlite::Error::InvalidQuery)),
            Error::Storage(_)
        ));
    }
}
