//! AI access: why an AI surface refuses, and which lesson text a model may
//! see.
//!
//! The age, guardian-approval and attempt rules are applied by the app's
//! `ai_access` module, which owns the profile's identity, the guardian link
//! and the integrity flags. This crate holds the typed reasons, so the two
//! kinds of caller map them once each: chat, whose errors are the string
//! codes the webview translates (the app's chat vocabulary renders them), and
//! the lesson tutor and Studio runs, whose errors are [`Error`]
//! (`From<Refused>`).

use std::fmt;

use rusqlite::Connection;

use crate::learning::{self, LessonTarget};
use crate::{store, Error};

/// Why a learner may not use an AI surface, a connection, or a lesson now.
#[derive(Debug)]
pub enum Refused {
    /// Under 13: models on the device or the learner's own network only.
    TooYoungForRemote,
    /// 13 to 17, and the vendor's terms require users to be 18 or older.
    AdultsOnly(&'static str),
    /// 13 to 17, and no guardian has been asked about this endpoint.
    GuardianRequired,
    GuardianPending,
    GuardianDeclined,
    /// Asking for an approval needs a linked guardian.
    NoGuardian,
    /// Only remote models need a guardian's approval.
    NotRemote,
    ConnectionDisabled,
    WrongCapability,
    /// An assessment attempt or interview check is in progress.
    AttemptInProgress,
    LessonNotFound,
    /// Not a text lesson: graded, interactive and video elements are never
    /// shown to a model.
    LessonWithheld,
    /// What could not be read.
    Storage(&'static str),
    Invalid(String),
    /// Any other Studio error, unchanged.
    Studio(Error),
}

impl Refused {
    /// The reason, written for people, without a code. Chat renders it with
    /// its code through the app's chat vocabulary (`ChatErrorCode::of_refused`).
    pub fn reason(&self) -> String {
        match self {
            Refused::TooYoungForRemote => {
                "learners under 13 can use models on this device or their own network only".into()
            }
            Refused::AdultsOnly(vendor) => format!("{vendor} requires users to be 18 or older"),
            Refused::GuardianRequired => "remote models need a guardian's approval; use a model on this device or your own network".into(),
            Refused::GuardianPending => "waiting for a guardian to approve this model".into(),
            Refused::GuardianDeclined => "a guardian did not approve this model".into(),
            Refused::NoGuardian => "link a guardian in Settings first".into(),
            Refused::NotRemote => "only remote models need a guardian's approval".into(),
            Refused::ConnectionDisabled => "the chosen model connection is disabled".into(),
            Refused::WrongCapability => "the chosen model connection does not answer text".into(),
            Refused::AttemptInProgress => {
                "AI assistance is paused while an assessment is in progress".into()
            }
            Refused::LessonNotFound => "no published course or lesson with that identifier".into(),
            Refused::LessonWithheld => "only text lessons can be attached".into(),
            Refused::Storage(what) => format!("{what} could not be read"),
            Refused::Invalid(message) => message.clone(),
            Refused::Studio(error) => error.to_string(),
        }
    }
}

/// The reason without a code; see [`Refused::reason`].
impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason())
    }
}

impl std::error::Error for Refused {}

/// The tutor and Studio runs: an attempt keeps its own error, a lesson the
/// tutor may not show is a permission error (whether it exists is not
/// said), and every other refusal is shown as its reason.
impl From<Refused> for Error {
    fn from(refused: Refused) -> Self {
        match refused {
            Refused::AttemptInProgress => Error::AssessmentActive,
            Refused::LessonNotFound | Refused::LessonWithheld => Error::Permission,
            Refused::ConnectionDisabled | Refused::WrongCapability => {
                Error::Unavailable("Choose an enabled text model".into())
            }
            Refused::Studio(error) => error,
            other => Error::Unavailable(other.reason()),
        }
    }
}

impl From<Error> for Refused {
    fn from(error: Error) -> Self {
        match error {
            Error::AssessmentActive => Refused::AttemptInProgress,
            other => Refused::Studio(other),
        }
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
) -> Result<LessonTarget, Refused> {
    let found = match reader {
        Reader::Chat => learning::lesson_target(conn, course_id, element_id),
        Reader::Tutor => store::can_use_tutor(conn, course_id)
            .and_then(|()| learning::course_lesson(conn, course_id, element_id)),
    };
    let target = match found {
        Ok(target) => target,
        Err(Error::NotFound) => return Err(Refused::LessonNotFound),
        Err(error) => return Err(error.into()),
    };
    if learning::content_kind(&target.element_type) != "text" {
        return Err(Refused::LessonWithheld);
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tutor_and_studio_runs_see_the_reason_without_its_code() {
        assert!(matches!(
            Error::from(Refused::AttemptInProgress),
            Error::AssessmentActive
        ));
        match Error::from(Refused::AdultsOnly("Groq")) {
            Error::Unavailable(message) => {
                assert_eq!(message, "Groq requires users to be 18 or older")
            }
            other => panic!("{other:?}"),
        }
        match Error::from(Refused::GuardianRequired) {
            Error::Unavailable(message) => assert!(message.starts_with("remote models need")),
            other => panic!("{other:?}"),
        }
    }
}
