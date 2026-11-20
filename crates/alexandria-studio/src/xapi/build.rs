//! Learning events in, statements out.

use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::identifiers::{ActivityRef, Level, Recipient};
use super::statement::{
    language_tag, Activity, Agent, Component, Context, ContextActivities, ContextActivity,
    Definition, LanguageMap, Score, Statement, StatementObject, StatementResult, VerbObject,
    PROGRESS_EXTENSION, VOIDED,
};
use super::{StatementError, PLATFORM};

/// Something the learner did, as the app records it. Events are plain data
/// (serde) so they can be stored and rebuilt for any recipient later: the
/// same event and recipient always give the same statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearningEvent {
    /// The record's stable local id. Each recipient's statement id is a keyed
    /// hash of it.
    pub local_id: String,
    pub timestamp: DateTime<Utc>,
    /// The local enrolment or package attempt id, if the event belongs to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration: Option<String>,
    pub action: Action,
    /// The profile the statement follows, which adds its category activity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
}

/// What happened. Each action has one verb.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum Action {
    /// Enrolment in a course.
    Registered {
        target: Target,
    },
    Launched {
        target: Target,
    },
    /// Progress through an activity, a whole percentage (cmi5 extension).
    Progressed {
        target: Target,
        percent: u8,
    },
    Completed {
        target: Target,
        outcome: Outcome,
    },
    /// `outcome.success` is implied (`true`) and may be left unset.
    Passed {
        target: Target,
        outcome: Outcome,
    },
    /// `outcome.success` is implied (`false`) and may be left unset.
    Failed {
        target: Target,
        outcome: Outcome,
    },
    /// A score with no pass, fail or completion attached.
    Scored {
        target: Target,
        outcome: Outcome,
    },
    /// A response a package reported for one of its interactions.
    Responded {
        interaction: Interaction,
        outcome: Outcome,
    },
    /// Voids a statement this recipient was sent, by its local id.
    Voided {
        target_local_id: String,
    },
    /// A cmi5 session ended without the unit terminating it; the outcome
    /// carries the session's length.
    Abandoned {
        target: Target,
        outcome: Outcome,
    },
    /// Every unit of a cmi5 block or course met its moveOn criteria.
    Satisfied {
        target: Target,
        scope: Satisfiable,
    },
}

/// What a cmi5 `satisfied` record is about; it sets the object's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Satisfiable {
    Block,
    Course,
}

/// An activity and, optionally, its display name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub activity: ActivityRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<LanguageMap>,
}

impl Target {
    pub fn new(activity: ActivityRef) -> Self {
        Self {
            activity,
            name: None,
        }
    }

    pub fn named(activity: ActivityRef, language: &str, name: &str) -> Self {
        Self {
            activity,
            name: Some(BTreeMap::from([(language.to_string(), name.to_string())])),
        }
    }
}

/// An interaction activity, as the xAPI interaction model describes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Interaction {
    /// An interaction-level [`ActivityRef`].
    pub activity: ActivityRef,
    pub interaction_type: InteractionType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<LanguageMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correct_responses: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<InteractionComponent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scale: Vec<InteractionComponent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<InteractionComponent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target: Vec<InteractionComponent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<InteractionComponent>,
}

impl Interaction {
    pub fn new(activity: ActivityRef, interaction_type: InteractionType) -> Self {
        Self {
            activity,
            interaction_type,
            description: None,
            correct_responses: None,
            choices: Vec::new(),
            scale: Vec::new(),
            source: Vec::new(),
            target: Vec::new(),
            steps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionComponent {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<LanguageMap>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InteractionType {
    TrueFalse,
    Choice,
    FillIn,
    LongFillIn,
    Matching,
    Performance,
    Sequencing,
    Likert,
    Numeric,
    Other,
}

impl InteractionType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TrueFalse => "true-false",
            Self::Choice => "choice",
            Self::FillIn => "fill-in",
            Self::LongFillIn => "long-fill-in",
            Self::Matching => "matching",
            Self::Performance => "performance",
            Self::Sequencing => "sequencing",
            Self::Likert => "likert",
            Self::Numeric => "numeric",
            Self::Other => "other",
        }
    }

    /// The type named by a SCORM or xAPI interaction type string.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "true-false" => Self::TrueFalse,
            "choice" => Self::Choice,
            "fill-in" => Self::FillIn,
            "long-fill-in" => Self::LongFillIn,
            "matching" => Self::Matching,
            "performance" => Self::Performance,
            "sequencing" => Self::Sequencing,
            "likert" => Self::Likert,
            "numeric" => Self::Numeric,
            "other" => Self::Other,
            _ => return None,
        })
    }
}

/// The result part of an event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<Score>,
    /// Time spent, in hundredths of a second (the precision both versions
    /// keep).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_centiseconds: Option<u64>,
    /// An interaction response in the xAPI response format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

/// A published profile a statement follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// The xAPI SCORM profile: every statement carries its category activity.
    Scorm,
    /// cmi5: every statement carries the cmi5 category activity, and one
    /// whose result says success or completion the moveOn category too.
    Cmi5,
}

impl Profile {
    fn category(self) -> Activity {
        match self {
            Self::Scorm => Activity {
                id: "https://w3id.org/xapi/scorm".into(),
                definition: Some(Definition {
                    activity_type: Some("http://adlnet.gov/expapi/activities/profile".into()),
                    ..Definition::default()
                }),
            },
            Self::Cmi5 => Activity {
                id: super::cmi5::CATEGORY_CMI5.into(),
                definition: None,
            },
        }
    }
}

/// The two kinds of learning record. Interaction-level records leave the
/// device only under a scope that names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    /// Launch, progress, completion, pass, fail, score (and voiding).
    Completion,
    /// Individual responses a package reported.
    Interaction,
}

impl LearningEvent {
    pub fn kind(&self) -> RecordKind {
        match self.action {
            Action::Responded { .. } => RecordKind::Interaction,
            _ => RecordKind::Completion,
        }
    }

    /// The voiding event for a statement already sent, by its local id. Its
    /// own local id is `void|<target local id>`, so a repeated void is the
    /// same statement.
    pub fn voiding(target_local_id: &str, timestamp: DateTime<Utc>) -> Self {
        Self {
            local_id: format!("void|{target_local_id}"),
            timestamp,
            registration: None,
            action: Action::Voided {
                target_local_id: target_local_id.to_string(),
            },
            profile: None,
        }
    }
}

/// Verbs from the ADL vocabulary (`http://adlnet.gov/expapi/verbs/`) and
/// the cmi5 verbs (`https://w3id.org/xapi/adl/verbs/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Registered,
    Launched,
    Initialized,
    Progressed,
    Completed,
    Passed,
    Failed,
    Scored,
    Responded,
    Answered,
    Terminated,
    Suspended,
    Resumed,
    Voided,
    Abandoned,
    Waived,
    Satisfied,
}

impl Verb {
    pub fn id(self) -> &'static str {
        match self {
            Self::Registered => "http://adlnet.gov/expapi/verbs/registered",
            Self::Launched => "http://adlnet.gov/expapi/verbs/launched",
            Self::Initialized => "http://adlnet.gov/expapi/verbs/initialized",
            Self::Progressed => "http://adlnet.gov/expapi/verbs/progressed",
            Self::Completed => "http://adlnet.gov/expapi/verbs/completed",
            Self::Passed => "http://adlnet.gov/expapi/verbs/passed",
            Self::Failed => "http://adlnet.gov/expapi/verbs/failed",
            Self::Scored => "http://adlnet.gov/expapi/verbs/scored",
            Self::Responded => "http://adlnet.gov/expapi/verbs/responded",
            Self::Answered => "http://adlnet.gov/expapi/verbs/answered",
            Self::Terminated => "http://adlnet.gov/expapi/verbs/terminated",
            Self::Suspended => "http://adlnet.gov/expapi/verbs/suspended",
            Self::Resumed => "http://adlnet.gov/expapi/verbs/resumed",
            Self::Voided => VOIDED,
            Self::Abandoned => "https://w3id.org/xapi/adl/verbs/abandoned",
            Self::Waived => "https://w3id.org/xapi/adl/verbs/waived",
            Self::Satisfied => "https://w3id.org/xapi/adl/verbs/satisfied",
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Launched => "launched",
            Self::Initialized => "initialized",
            Self::Progressed => "progressed",
            Self::Completed => "completed",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Scored => "scored",
            Self::Responded => "responded",
            Self::Answered => "answered",
            Self::Terminated => "terminated",
            Self::Suspended => "suspended",
            Self::Resumed => "resumed",
            Self::Voided => "voided",
            Self::Abandoned => "abandoned",
            Self::Waived => "waived",
            Self::Satisfied => "satisfied",
        }
    }

    fn object(self) -> VerbObject {
        VerbObject {
            id: self.id().into(),
            display: BTreeMap::from([("en-US".to_string(), self.display().to_string())]),
        }
    }
}

impl Action {
    pub fn verb(&self) -> Verb {
        match self {
            Self::Registered { .. } => Verb::Registered,
            Self::Launched { .. } => Verb::Launched,
            Self::Progressed { .. } => Verb::Progressed,
            Self::Completed { .. } => Verb::Completed,
            Self::Passed { .. } => Verb::Passed,
            Self::Failed { .. } => Verb::Failed,
            Self::Scored { .. } => Verb::Scored,
            Self::Responded { .. } => Verb::Responded,
            Self::Voided { .. } => Verb::Voided,
            Self::Abandoned { .. } => Verb::Abandoned,
            Self::Satisfied { .. } => Verb::Satisfied,
        }
    }
}

/// Whether activity names go into the statement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Names {
    #[default]
    Disclose,
    /// For a private course whose owner has not allowed its title to be
    /// disclosed: definitions keep their type and interaction structure but
    /// lose every name and description.
    Omit,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildOptions {
    /// `context.language`: the learner's language, an RFC 5646 tag.
    pub language: Option<String>,
    pub names: Names,
}

/// Builds one validated statement for `recipient`.
pub fn build_statement(
    event: &LearningEvent,
    recipient: &Recipient,
    options: &BuildOptions,
) -> Result<Statement, StatementError> {
    if event.local_id.is_empty() {
        return Err(StatementError::InvalidIdentifier(
            "the local statement id is empty".into(),
        ));
    }
    if let Some(language) = &options.language {
        language_tag(language)?;
    }
    let verb = event.action.verb();
    let actor = Agent::Agent {
        account: recipient.account().clone(),
    };
    let id = recipient.statement_id(&event.local_id);
    let timestamp = event.timestamp.to_rfc3339_opts(SecondsFormat::Millis, true);

    let statement = match &event.action {
        Action::Voided { target_local_id } => {
            if target_local_id.is_empty() {
                return Err(StatementError::InvalidIdentifier(
                    "the voided statement's local id is empty".into(),
                ));
            }
            Statement {
                id,
                actor,
                verb: verb.object(),
                object: StatementObject::StatementRef {
                    id: recipient.statement_id(target_local_id),
                },
                timestamp,
                result: None,
                context: None,
            }
        }
        action => {
            let (activity, definition) = object_of(action, options.names)?;
            let result = result_of(action)?;
            let mut context = context_of(&activity, event.profile)?;
            if event.profile == Some(Profile::Cmi5)
                && result
                    .as_ref()
                    .is_some_and(|r| r.success.is_some() || r.completion.is_some())
            {
                context
                    .context_activities
                    .get_or_insert_with(ContextActivities::default)
                    .category
                    .push(ContextActivity::Activity(Activity {
                        id: super::cmi5::CATEGORY_MOVEON.into(),
                        definition: None,
                    }));
            }
            context.registration = match event.registration.as_deref() {
                Some("") => {
                    return Err(StatementError::InvalidIdentifier(
                        "the local registration id is empty".into(),
                    ))
                }
                Some(local) => Some(recipient.registration(local)),
                None => None,
            };
            context.platform = Some(PLATFORM.into());
            context.language = options.language.clone();
            Statement {
                id,
                actor,
                verb: verb.object(),
                object: StatementObject::Activity(Box::new(Activity {
                    id: activity.iri()?,
                    definition: Some(definition),
                })),
                timestamp,
                result,
                context: Some(context),
            }
        }
    };
    statement.validate()?;
    Ok(statement)
}

/// Builds a batch, refusing the whole batch if any event is invalid.
pub fn build_batch(
    events: &[LearningEvent],
    recipient: &Recipient,
    options: &BuildOptions,
) -> Result<Vec<Statement>, StatementError> {
    events
        .iter()
        .map(|event| build_statement(event, recipient, options))
        .collect()
}

const COURSE_TYPE: &str = "http://adlnet.gov/expapi/activities/course";
const LESSON_TYPE: &str = "http://adlnet.gov/expapi/activities/lesson";
const INTERACTION_TYPE: &str = "http://adlnet.gov/expapi/activities/cmi.interaction";

fn object_of(action: &Action, names: Names) -> Result<(ActivityRef, Definition), StatementError> {
    let keep = |map: &Option<LanguageMap>| match names {
        Names::Disclose => map.clone(),
        Names::Omit => None,
    };
    let target = match action {
        Action::Registered { target }
        | Action::Launched { target }
        | Action::Progressed { target, .. }
        | Action::Completed { target, .. }
        | Action::Passed { target, .. }
        | Action::Failed { target, .. }
        | Action::Scored { target, .. }
        | Action::Abandoned { target, .. } => target,
        Action::Satisfied { target, scope } => {
            let activity_type = match scope {
                Satisfiable::Block => super::cmi5::ACTIVITY_TYPE_BLOCK,
                Satisfiable::Course => super::cmi5::ACTIVITY_TYPE_COURSE,
            };
            let definition = Definition {
                activity_type: Some(activity_type.into()),
                name: keep(&target.name),
                ..Definition::default()
            };
            return Ok((target.activity.clone(), definition));
        }
        Action::Responded { interaction, .. } => {
            if interaction.activity.level()? != Level::Interaction {
                return Err(StatementError::InvalidActivity(
                    "a response must be about an interaction".into(),
                ));
            }
            let components = |list: &[InteractionComponent]| {
                (!list.is_empty()).then(|| {
                    list.iter()
                        .map(|c| Component {
                            id: c.id.clone(),
                            description: keep(&c.description),
                        })
                        .collect()
                })
            };
            let definition = Definition {
                activity_type: Some(INTERACTION_TYPE.into()),
                description: keep(&interaction.description),
                interaction_type: Some(interaction.interaction_type.as_str().into()),
                correct_responses_pattern: interaction.correct_responses.clone(),
                choices: components(&interaction.choices),
                scale: components(&interaction.scale),
                source: components(&interaction.source),
                target: components(&interaction.target),
                steps: components(&interaction.steps),
                ..Definition::default()
            };
            return Ok((interaction.activity.clone(), definition));
        }
        Action::Voided { .. } => unreachable!("a voiding statement has no activity"),
    };
    let activity_type = match target.activity.level()? {
        Level::Course => COURSE_TYPE,
        Level::Element | Level::PackageActivity => LESSON_TYPE,
        Level::Interaction => {
            return Err(StatementError::InvalidActivity(
                "an interaction is reported with a response".into(),
            ))
        }
    };
    if matches!(action, Action::Registered { .. }) && target.activity.level()? != Level::Course {
        return Err(StatementError::InvalidActivity(
            "enrolment is in a course".into(),
        ));
    }
    let definition = Definition {
        activity_type: Some(activity_type.into()),
        name: keep(&target.name),
        ..Definition::default()
    };
    Ok((target.activity.clone(), definition))
}

fn result_of(action: &Action) -> Result<Option<StatementResult>, StatementError> {
    let conflict = |m: &str| Err(StatementError::InvalidResult(m.into()));
    let (outcome, success, completion) = match action {
        Action::Registered { .. }
        | Action::Launched { .. }
        | Action::Voided { .. }
        | Action::Satisfied { .. } => return Ok(None),
        Action::Abandoned { outcome, .. } => {
            if outcome.success.is_some()
                || outcome.completion.is_some()
                || outcome.score.is_some()
                || outcome.response.is_some()
            {
                return conflict("an abandoned record carries only the session's length");
            }
            (outcome, None, None)
        }
        Action::Progressed { percent, .. } => {
            if *percent > 100 {
                return conflict("progress must be a whole number from 0 to 100");
            }
            return Ok(Some(StatementResult {
                extensions: Some(BTreeMap::from([(
                    PROGRESS_EXTENSION.to_string(),
                    serde_json::Value::from(*percent),
                )])),
                ..StatementResult::default()
            }));
        }
        Action::Completed { outcome, .. } => (outcome, outcome.success, Some(true)),
        Action::Passed { outcome, .. } => (outcome, Some(true), outcome.completion),
        Action::Failed { outcome, .. } => (outcome, Some(false), outcome.completion),
        Action::Scored { outcome, .. } => {
            if outcome.score.is_none() {
                return conflict("a scored record needs a score");
            }
            (outcome, outcome.success, outcome.completion)
        }
        Action::Responded { outcome, .. } => (outcome, outcome.success, outcome.completion),
    };
    if outcome.success.is_some() && outcome.success != success {
        return conflict("the outcome's success contradicts the verb");
    }
    if outcome.completion.is_some() && outcome.completion != completion {
        return conflict("the outcome's completion contradicts the verb");
    }
    if outcome.response.is_some() && !matches!(action, Action::Responded { .. }) {
        return conflict("only a response carries a response");
    }
    Ok(Some(StatementResult {
        score: outcome.score.clone(),
        success,
        completion,
        response: outcome.response.clone(),
        duration: outcome.duration_centiseconds.map(iso_duration),
        extensions: None,
    }))
}

/// Parent and grouping follow the activity's nesting: an element's parent is
/// its course; a package activity's parent is its element and an
/// interaction's is its package activity, both grouped under the course.
fn context_of(activity: &ActivityRef, profile: Option<Profile>) -> Result<Context, StatementError> {
    let reference = |a: &ActivityRef| -> Result<ContextActivity, StatementError> {
        Ok(ContextActivity::Activity(Activity {
            id: a.iri()?,
            definition: None,
        }))
    };
    let mut activities = ContextActivities::default();
    if let Some(parent) = activity.parent() {
        activities.parent.push(reference(&parent)?);
        if parent.level()? != Level::Course {
            activities.grouping.push(reference(&activity.course_ref())?);
        }
    }
    if let Some(profile) = profile {
        activities
            .category
            .push(ContextActivity::Activity(profile.category()));
    }
    let empty = activities == ContextActivities::default();
    Ok(Context {
        context_activities: (!empty).then_some(activities),
        ..Context::default()
    })
}

/// `PT[nH][nM][n[.nn]S]` from hundredths of a second.
fn iso_duration(centiseconds: u64) -> String {
    let hours = centiseconds / 360_000;
    let minutes = centiseconds / 6_000 % 60;
    let rest = centiseconds % 6_000;
    let mut out = String::from("PT");
    if hours > 0 {
        out.push_str(&format!("{hours}H"));
    }
    if minutes > 0 {
        out.push_str(&format!("{minutes}M"));
    }
    if rest > 0 || out.len() == 2 {
        let (seconds, fraction) = (rest / 100, rest % 100);
        match fraction {
            0 => out.push_str(&format!("{seconds}S")),
            f if f % 10 == 0 => out.push_str(&format!("{seconds}.{}S", f / 10)),
            f => out.push_str(&format!("{seconds}.{f:02}S")),
        }
    }
    out
}
