//! AI access: why an AI surface refuses, and which lesson text a model may
//! see.
//!
//! The age, guardian-approval and attempt rules are applied by the app's
//! `ai_access` module, which owns the profile's identity, the guardian link
//! and the integrity flags. This crate holds the typed reasons, so the two
//! kinds of caller map them once each: chat, whose errors are the string
//! codes the webview translates (`Display`), and the lesson tutor and Studio
//! runs, whose errors are [`Error`] (`From<Refused>`).

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
    fn code(&self) -> &'static str {
        match self {
            Refused::TooYoungForRemote => "under_13",
            Refused::AdultsOnly(_) => "adults_only",
            Refused::GuardianRequired => "guardian_required",
            Refused::GuardianPending => "guardian_pending",
            Refused::GuardianDeclined => "guardian_declined",
            Refused::NoGuardian => "no_guardian",
            Refused::NotRemote | Refused::LessonWithheld | Refused::Invalid(_) => "invalid_input",
            Refused::ConnectionDisabled | Refused::WrongCapability => "no_connection",
            Refused::AttemptInProgress => "assessment_active",
            Refused::LessonNotFound => "not_found",
            Refused::Storage(_) => "storage_error",
            Refused::Studio(_) => "studio",
        }
    }

    /// The reason without its code.
    fn message(&self) -> String {
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

/// The code the webview translates, then the reason.
impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::Studio(error) => error.fmt(f),
            other => write!(f, "{}: {}", other.code(), other.message()),
        }
    }
}

impl std::error::Error for Refused {}

/// Chat's errors are the string codes.
impl From<Refused> for String {
    fn from(refused: Refused) -> Self {
        refused.to_string()
    }
}

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
            other => Error::Unavailable(other.message()),
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
    fn every_refusal_reads_as_the_code_the_webview_translates() {
        let cases: [(Refused, &str); 13] = [
            (Refused::TooYoungForRemote, "under_13: learners under 13 can use models on this device or their own network only"),
            (Refused::AdultsOnly("OpenRouter"), "adults_only: OpenRouter requires users to be 18 or older"),
            (Refused::GuardianRequired, "guardian_required: remote models need a guardian's approval; use a model on this device or your own network"),
            (Refused::GuardianPending, "guardian_pending: waiting for a guardian to approve this model"),
            (Refused::GuardianDeclined, "guardian_declined: a guardian did not approve this model"),
            (Refused::NoGuardian, "no_guardian: link a guardian in Settings first"),
            (Refused::NotRemote, "invalid_input: only remote models need a guardian's approval"),
            (Refused::ConnectionDisabled, "no_connection: the chosen model connection is disabled"),
            (Refused::AttemptInProgress, &Error::AssessmentActive.to_string()),
            (Refused::LessonNotFound, &Error::NotFound.to_string()),
            (Refused::LessonWithheld, "invalid_input: only text lessons can be attached"),
            (Refused::Storage("profile"), "storage_error: profile could not be read"),
            (Refused::Studio(Error::Conflict), &Error::Conflict.to_string()),
        ];
        for (refused, expected) in cases {
            assert_eq!(refused.to_string(), expected);
        }
    }

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
