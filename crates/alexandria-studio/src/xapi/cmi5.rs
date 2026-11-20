//! cmi5 sessions, as the launching system sees them.
//!
//! An assignable unit writes xAPI statements about one session to the
//! endpoint the app opens for it. This module is the pure half of that
//! endpoint: it checks each statement against the cmi5 session and
//! registration rules ([`check`]), and writes the statements and documents
//! the launching system owes the unit ([`launched`], [`satisfied`],
//! [`abandoned`], [`launch_data`], [`learner_preferences`]).
//!
//! Statements here are raw JSON in the unit's own view: its actor is the
//! pseudonymous identifier the unit was given, its registration and session
//! identifiers are the ones in its launch. They never leave the device in
//! this form. What the learner's records keep is a [`super::LearningEvent`]
//! built from the facts a statement carries, rewritten for each recipient
//! when it is sent.
//!
//! Section numbers refer to the cmi5 specification (Quartz edition).

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};

pub use super::Satisfiable;
use crate::package::MoveOn;

/// The cmi5 category activity: marks a "cmi5 defined" statement (9.6.2.1).
pub const CATEGORY_CMI5: &str = "https://w3id.org/xapi/cmi5/context/categories/cmi5";
/// The moveOn category activity: on defined statements whose result carries
/// success or completion, and on no others (9.6.2.2).
pub const CATEGORY_MOVEON: &str = "https://w3id.org/xapi/cmi5/context/categories/moveon";
pub const EXTENSION_SESSION_ID: &str = "https://w3id.org/xapi/cmi5/context/extensions/sessionid";
pub const EXTENSION_MASTERY_SCORE: &str =
    "https://w3id.org/xapi/cmi5/context/extensions/masteryscore";
pub const EXTENSION_LAUNCH_MODE: &str = "https://w3id.org/xapi/cmi5/context/extensions/launchmode";
pub const EXTENSION_LAUNCH_URL: &str = "https://w3id.org/xapi/cmi5/context/extensions/launchurl";
pub const EXTENSION_MOVE_ON: &str = "https://w3id.org/xapi/cmi5/context/extensions/moveon";
pub const EXTENSION_LAUNCH_PARAMETERS: &str =
    "https://w3id.org/xapi/cmi5/context/extensions/launchparameters";
pub const EXTENSION_PROGRESS: &str = "https://w3id.org/xapi/cmi5/result/extensions/progress";
pub const ACTIVITY_TYPE_BLOCK: &str = "https://w3id.org/xapi/cmi5/activitytype/block";
pub const ACTIVITY_TYPE_COURSE: &str = "https://w3id.org/xapi/cmi5/activitytype/course";
/// The state document the launching system writes before every launch (10).
pub const LAUNCH_DATA_STATE_ID: &str = "LMS.LaunchData";
/// The agent profile document holding the learner's preferences (11).
pub const LEARNER_PREFERENCES_PROFILE_ID: &str = "cmi5LearnerPreferences";

const VERB_LAUNCHED: &str = "http://adlnet.gov/expapi/verbs/launched";
const VERB_INITIALIZED: &str = "http://adlnet.gov/expapi/verbs/initialized";
const VERB_COMPLETED: &str = "http://adlnet.gov/expapi/verbs/completed";
const VERB_PASSED: &str = "http://adlnet.gov/expapi/verbs/passed";
const VERB_FAILED: &str = "http://adlnet.gov/expapi/verbs/failed";
const VERB_TERMINATED: &str = "http://adlnet.gov/expapi/verbs/terminated";
const VERB_ABANDONED: &str = "https://w3id.org/xapi/adl/verbs/abandoned";
const VERB_SATISFIED: &str = "https://w3id.org/xapi/adl/verbs/satisfied";
const VERB_VOIDED: &str = "http://adlnet.gov/expapi/verbs/voided";

/// The cmi5 launch mode (10.2.2). The app launches every unit in `Normal`:
/// it has no browse or review offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Normal,
    Browse,
    Review,
}

impl LaunchMode {
    pub fn as_cmi5(self) -> &'static str {
        match self {
            LaunchMode::Normal => "Normal",
            LaunchMode::Browse => "Browse",
            LaunchMode::Review => "Review",
        }
    }
}

/// Context string for the registration key. Never change it: units have seen
/// registrations derived under it.
const REGISTRATION_CONTEXT: &str = "alexandria 2026-11-20 cmi5 registration v1";

/// The registration UUID a unit is given for a package attempt: a keyed hash
/// of the attempt's local id under a key derived from the learner's own key.
/// Stable for the attempt, unlinkable to the local id or to the learner's
/// records anywhere else.
pub fn registration(learner_key: &[u8; 32], attempt_id: &str) -> String {
    let key = blake3::derive_key(REGISTRATION_CONTEXT, learner_key);
    let digest = blake3::keyed_hash(&key, attempt_id.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    uuid::Builder::from_random_bytes(bytes)
        .into_uuid()
        .to_string()
}

/// The actor a unit is given: an agent with an account (9.2), named by the
/// pseudonymous learner identifier under the app's account home page.
pub fn actor(learner_id: &str) -> Value {
    json!({
        "objectType": "Agent",
        "account": { "homePage": super::ACCOUNT_HOME_PAGE, "name": learner_id },
    })
}

/// Whether an agent, as a statement or a document request names it, is the
/// session's actor: the same account. `objectType` may be left out.
pub fn same_agent(agent: &Value, actor: &Value) -> bool {
    let Some(agent) = agent.as_object() else {
        return false;
    };
    if agent
        .get("objectType")
        .is_some_and(|kind| kind.as_str() != Some("Agent"))
    {
        return false;
    }
    let account = &agent.get("account");
    let expected = &actor["account"];
    account.is_some_and(|account| {
        account["homePage"] == expected["homePage"] && account["name"] == expected["name"]
    })
}

/// What the launching system fixed for one session when it launched the
/// unit.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionFacts {
    /// [`actor`] of the learner.
    pub actor: Value,
    /// The registration UUID the unit was given.
    pub registration: String,
    pub session_id: String,
    /// The activity identifier the unit was given (8.1.5): never its
    /// publisher identifier.
    pub activity_id: String,
    /// The unit's identifier in the course structure.
    pub publisher_id: String,
    pub mastery_score: Option<f64>,
    pub launch_mode: LaunchMode,
}

impl SessionFacts {
    /// The context every statement the launching system writes in this
    /// session carries: registration, cmi5 category, the publisher grouping
    /// and the session identifier.
    fn lms_context(&self, publisher_id: &str) -> Map<String, Value> {
        let mut context = Map::new();
        context.insert("registration".into(), self.registration.clone().into());
        context.insert(
            "contextActivities".into(),
            json!({
                "category": [{ "objectType": "Activity", "id": CATEGORY_CMI5 }],
                "grouping": [{ "objectType": "Activity", "id": publisher_id }],
            }),
        );
        let mut extensions = Map::new();
        extensions.insert(EXTENSION_SESSION_ID.into(), self.session_id.clone().into());
        context.insert("extensions".into(), Value::Object(extensions));
        context
    }
}

/// The cmi5 defined verbs a unit may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnitVerb {
    Initialized,
    Completed,
    Passed,
    Failed,
    Terminated,
}

impl UnitVerb {
    fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            VERB_INITIALIZED => Self::Initialized,
            VERB_COMPLETED => Self::Completed,
            VERB_PASSED => Self::Passed,
            VERB_FAILED => Self::Failed,
            VERB_TERMINATED => Self::Terminated,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initialized => "initialized",
            Self::Completed => "completed",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Terminated => "terminated",
        }
    }

    /// The verb a stored [`as_str`](Self::as_str) names.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "initialized" => Self::Initialized,
            "completed" => Self::Completed,
            "passed" => Self::Passed,
            "failed" => Self::Failed,
            "terminated" => Self::Terminated,
            _ => return None,
        })
    }
}

/// What a session and its registration have seen so far, for [`check`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionRecord {
    /// cmi5 defined verbs the unit used in this session, in order.
    pub verbs: Vec<UnitVerb>,
    /// The launching system recorded `abandoned` for the session.
    pub abandoned: bool,
    /// Earlier sessions of this unit in the registration used `completed`.
    pub completed_before: bool,
    /// Earlier sessions of this unit in the registration used `passed`.
    pub passed_before: bool,
}

impl SessionRecord {
    pub fn initialized(&self) -> bool {
        self.verbs.contains(&UnitVerb::Initialized)
    }

    pub fn terminated(&self) -> bool {
        self.verbs.contains(&UnitVerb::Terminated)
    }

    /// No statement is accepted for the session any more.
    pub fn ended(&self) -> bool {
        self.terminated() || self.abandoned
    }

    /// The unit has used this verb in the registration, this session
    /// included.
    pub fn in_registration(&self, verb: UnitVerb) -> bool {
        self.verbs.contains(&verb)
            || match verb {
                UnitVerb::Completed => self.completed_before,
                UnitVerb::Passed => self.passed_before,
                _ => false,
            }
    }

    /// Takes an accepted statement into account.
    pub fn apply(&mut self, checked: &Checked) {
        if let Some(verb) = checked.verb {
            self.verbs.push(verb);
        }
    }
}

/// An accepted statement's facts.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    /// The cmi5 defined verb, or `None` for a "cmi5 allowed" statement.
    pub verb: Option<UnitVerb>,
    /// `result.score`, normalised to 0..1, on a passed or failed statement.
    pub score: Option<f64>,
    /// `result.duration`, in seconds, when it parses.
    pub duration_seconds: Option<f64>,
    /// The progress extension, 0..=100.
    pub progress: Option<u8>,
}

/// Why a statement was not accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// Not a statement this endpoint can read (HTTP 400).
    #[error("malformed statement: {0}")]
    Malformed(String),
    /// A statement that breaks a cmi5 rule for this session or registration
    /// (HTTP 403, cmi5 6.3: the launching system rejects it).
    #[error("refused by the cmi5 session rules: {0}")]
    Rule(String),
}

fn rule(message: impl Into<String>) -> Refusal {
    Refusal::Rule(message.into())
}

fn ids(list: &Value) -> impl Iterator<Item = &str> {
    let items: &[Value] = match list {
        Value::Array(items) => items,
        Value::Object(_) => std::slice::from_ref(list),
        _ => &[],
    };
    items.iter().filter_map(|item| item["id"].as_str())
}

/// Checks one statement a unit sent against the session's rules (9.3 to
/// 9.6, 7.1.3). `record` is what the session and registration have seen
/// before it; the caller applies the result to it before checking the next
/// statement of a batch.
///
/// Every statement must name the session's actor, registration, session
/// identifier and publisher grouping (the context template). A "cmi5
/// defined" statement (one carrying the cmi5 category) must be about the
/// unit and follow the verb, result and ordering rules; a "cmi5 allowed"
/// statement may use any verb between `initialized` and `terminated`.
/// Voiding is never accepted (6.3).
pub fn check(
    statement: &Value,
    facts: &SessionFacts,
    record: &SessionRecord,
) -> Result<Checked, Refusal> {
    let Some(object) = statement.as_object() else {
        return Err(Refusal::Malformed("a statement is a JSON object".into()));
    };
    let verb_id = statement["verb"]["id"]
        .as_str()
        .ok_or_else(|| Refusal::Malformed("the statement has no verb id".into()))?;
    for required in ["actor", "object"] {
        if !object.contains_key(required) {
            return Err(Refusal::Malformed(format!(
                "the statement has no {required}"
            )));
        }
    }
    if verb_id == VERB_VOIDED {
        return Err(rule("a unit is not permitted to void statements"));
    }
    if record.abandoned {
        return Err(rule("the session was abandoned"));
    }
    if record.terminated() {
        return Err(rule("the session was terminated"));
    }
    if !same_agent(&statement["actor"], &facts.actor) {
        return Err(rule(
            "the actor is not the learner the unit was launched for",
        ));
    }
    let context = &statement["context"];
    if context["registration"].as_str() != Some(facts.registration.as_str()) {
        return Err(rule("the registration is not the session's"));
    }
    if context["extensions"][EXTENSION_SESSION_ID].as_str() != Some(facts.session_id.as_str()) {
        return Err(rule("the session identifier is not the session's"));
    }
    let activities = &context["contextActivities"];
    if !ids(&activities["grouping"]).any(|id| id == facts.publisher_id) {
        return Err(rule(
            "the context template's publisher grouping activity is missing",
        ));
    }
    let defined = ids(&activities["category"]).any(|id| id == CATEGORY_CMI5);
    let moveon_category = ids(&activities["category"]).any(|id| id == CATEGORY_MOVEON);
    let result = &statement["result"];
    let progress = match &result["extensions"][EXTENSION_PROGRESS] {
        Value::Null => None,
        value => Some(
            value
                .as_u64()
                .filter(|p| *p <= 100)
                .map(|p| p as u8)
                .ok_or_else(|| rule("progress is a whole number from 0 to 100"))?,
        ),
    };
    let duration_seconds = result["duration"].as_str().and_then(parse_duration);
    if result["duration"].is_string() && duration_seconds.is_none() {
        return Err(Refusal::Malformed(
            "the duration is not an ISO 8601 duration".into(),
        ));
    }

    if !defined {
        if !record.initialized() {
            return Err(rule("the first statement of a session must be initialized"));
        }
        return Ok(Checked {
            verb: None,
            score: None,
            duration_seconds,
            progress,
        });
    }

    let verb = UnitVerb::from_id(verb_id).ok_or_else(|| {
        rule(format!(
            "{verb_id} is not a cmi5 defined verb a unit may use"
        ))
    })?;
    if !record.initialized() && verb != UnitVerb::Initialized {
        return Err(rule("the first statement of a session must be initialized"));
    }
    let object = &statement["object"];
    if object
        .get("objectType")
        .is_some_and(|kind| kind.as_str() != Some("Activity"))
        || object["id"].as_str() != Some(facts.activity_id.as_str())
    {
        return Err(rule(
            "a cmi5 defined statement is about the unit's activity identifier",
        ));
    }
    if record.verbs.contains(&verb) {
        return Err(rule(format!(
            "{} was already used in this session",
            verb.as_str()
        )));
    }
    if facts.launch_mode != LaunchMode::Normal
        && !matches!(verb, UnitVerb::Initialized | UnitVerb::Terminated)
    {
        return Err(rule(format!(
            "{} is not recorded in {} mode",
            verb.as_str(),
            facts.launch_mode.as_cmi5()
        )));
    }
    match verb {
        UnitVerb::Completed if record.in_registration(UnitVerb::Completed) => {
            return Err(rule("completed was already used in this registration"))
        }
        UnitVerb::Passed if record.in_registration(UnitVerb::Passed) => {
            return Err(rule("passed was already used in this registration"))
        }
        UnitVerb::Failed if record.in_registration(UnitVerb::Passed) => {
            return Err(rule("failed cannot follow passed in a registration"))
        }
        UnitVerb::Passed | UnitVerb::Failed
            if record
                .verbs
                .iter()
                .any(|v| matches!(v, UnitVerb::Passed | UnitVerb::Failed)) =>
        {
            return Err(rule("a session uses at most one of passed and failed"))
        }
        _ => {}
    }

    // Result rules (9.5).
    let success = &result["success"];
    let completion = &result["completion"];
    let score_value = &result["score"];
    let judged = matches!(verb, UnitVerb::Passed | UnitVerb::Failed);
    if !score_value.is_null() && !judged {
        return Err(rule(format!("{} carries no score", verb.as_str())));
    }
    match verb {
        UnitVerb::Passed if success.as_bool() != Some(true) => {
            return Err(rule("passed sets result.success to true"))
        }
        UnitVerb::Failed if success.as_bool() != Some(false) => {
            return Err(rule("failed sets result.success to false"))
        }
        UnitVerb::Initialized | UnitVerb::Completed | UnitVerb::Terminated
            if !success.is_null() =>
        {
            return Err(rule(format!("{} carries no success", verb.as_str())))
        }
        _ => {}
    }
    match verb {
        UnitVerb::Completed if completion.as_bool() != Some(true) => {
            return Err(rule("completed sets result.completion to true"))
        }
        UnitVerb::Completed => {}
        _ if !completion.is_null() => {
            return Err(rule(format!("{} carries no completion", verb.as_str())))
        }
        _ => {}
    }
    let needs_duration = !matches!(verb, UnitVerb::Initialized);
    if needs_duration && duration_seconds.is_none() {
        return Err(rule(format!("{} carries a duration", verb.as_str())));
    }
    let has_outcome = !success.is_null() || !completion.is_null();
    if has_outcome != moveon_category {
        return Err(rule(if has_outcome {
            "a statement with success or completion carries the moveOn category"
        } else {
            "only a statement with success or completion carries the moveOn category"
        }));
    }
    let score = if judged {
        normalised_score(score_value)?
    } else {
        None
    };
    if let (Some(mastery), Some(scaled)) = (facts.mastery_score, score_value["scaled"].as_f64()) {
        if verb == UnitVerb::Passed && scaled < mastery {
            return Err(rule("a passed score is at least the mastery score"));
        }
        if verb == UnitVerb::Failed && scaled >= mastery {
            return Err(rule("a failed score is below the mastery score"));
        }
    }
    Ok(Checked {
        verb: Some(verb),
        score,
        duration_seconds,
        progress,
    })
}

/// `scaled`, else `raw` between `min` and `max`.
fn normalised_score(score: &Value) -> Result<Option<f64>, Refusal> {
    if score.is_null() {
        return Ok(None);
    }
    if let Some(scaled) = score["scaled"].as_f64() {
        if !(-1.0..=1.0).contains(&scaled) {
            return Err(Refusal::Malformed(
                "score.scaled is between -1 and 1".into(),
            ));
        }
        return Ok(Some(scaled.clamp(0.0, 1.0)));
    }
    let raw = score["raw"].as_f64();
    let min = score["min"].as_f64();
    let max = score["max"].as_f64();
    match (raw, min, max) {
        (Some(raw), Some(min), Some(max)) if max > min && (min..=max).contains(&raw) => {
            Ok(Some((raw - min) / (max - min)))
        }
        (None, _, _) => Ok(None),
        _ => Err(rule("a raw score comes with min and max around it")),
    }
}

/// Seconds in an ISO 8601 duration (`P[nY][nM][nW][nD][T[nH][nM][nS]]`,
/// years as 365 days and months as 30). `None` when it does not parse.
pub fn parse_duration(text: &str) -> Option<f64> {
    let rest = text.trim().strip_prefix('P')?;
    if rest.is_empty() {
        return None;
    }
    let (date, time) = match rest.split_once('T') {
        Some((date, time)) if !time.is_empty() => (date, Some(time)),
        Some(_) => return None,
        None => (rest, None),
    };
    fn units(part: &str, scale: &[(char, f64)]) -> Option<f64> {
        let mut total = 0.0;
        let mut number = String::new();
        let mut next = 0;
        for c in part.chars() {
            if c.is_ascii_digit() || c == '.' || c == ',' {
                number.push(if c == ',' { '.' } else { c });
                continue;
            }
            let at = scale[next..].iter().position(|(unit, _)| *unit == c)? + next;
            let value: f64 = number.parse().ok()?;
            total += value * scale[at].1;
            number.clear();
            next = at + 1;
        }
        number.is_empty().then_some(total)
    }
    let day = 86_400.0;
    let mut seconds = units(
        date,
        &[
            ('Y', 365.0 * day),
            ('M', 30.0 * day),
            ('W', 7.0 * day),
            ('D', day),
        ],
    )?;
    if let Some(time) = time {
        seconds += units(time, &[('H', 3600.0), ('M', 60.0), ('S', 1.0)])?;
    }
    seconds.is_finite().then_some(seconds)
}

/// `PT[nH][nM][n[.nn]S]` for a number of seconds, to the hundredth.
pub fn format_duration(seconds: f64) -> String {
    let centis = (seconds.max(0.0) * 100.0).round() as u64;
    let (hours, minutes, rest) = (centis / 360_000, centis / 6_000 % 60, centis % 6_000);
    let mut out = String::from("PT");
    if hours > 0 {
        out.push_str(&format!("{hours}H"));
    }
    if minutes > 0 {
        out.push_str(&format!("{minutes}M"));
    }
    if rest > 0 || out.len() == 2 {
        let (whole, fraction) = (rest / 100, rest % 100);
        if fraction == 0 {
            out.push_str(&format!("{whole}S"));
        } else {
            out.push_str(&format!("{whole}.{fraction:02}S"));
        }
    }
    out
}

fn timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn statement(
    id: &str,
    facts: &SessionFacts,
    verb: (&str, &str),
    object: Value,
    context: Map<String, Value>,
    at: DateTime<Utc>,
) -> Map<String, Value> {
    let mut statement = Map::new();
    statement.insert("id".into(), id.into());
    statement.insert("actor".into(), facts.actor.clone());
    statement.insert(
        "verb".into(),
        json!({ "id": verb.0, "display": { "en-US": verb.1 } }),
    );
    statement.insert("object".into(), object);
    statement.insert("context".into(), Value::Object(context));
    statement.insert("timestamp".into(), timestamp(at).into());
    statement
}

/// The values of one launch beyond its [`SessionFacts`].
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchValues<'a> {
    pub move_on: MoveOn,
    /// The unit's `launchParameters` from the course structure.
    pub launch_parameters: Option<&'a str>,
    /// The unit's `entitlementKey` from the course structure.
    pub entitlement_key: Option<&'a str>,
    /// Where the unit sends the learner when it exits.
    pub return_url: Option<&'a str>,
    /// The launch URL without the cmi5 launch parameters.
    pub launch_url: &'a str,
}

/// The `LMS.LaunchData` state document (10.2).
pub fn launch_data(facts: &SessionFacts, values: &LaunchValues<'_>) -> Value {
    let mut extensions = Map::new();
    extensions.insert(EXTENSION_SESSION_ID.into(), facts.session_id.clone().into());
    let mut document = Map::new();
    document.insert(
        "contextTemplate".into(),
        json!({
            "contextActivities": {
                "grouping": [{ "objectType": "Activity", "id": facts.publisher_id }],
            },
            "extensions": extensions,
        }),
    );
    document.insert("launchMode".into(), facts.launch_mode.as_cmi5().into());
    document.insert("moveOn".into(), values.move_on.as_cmi5().into());
    if let Some(parameters) = values.launch_parameters {
        document.insert("launchParameters".into(), parameters.into());
    }
    if let Some(mastery) = facts.mastery_score {
        document.insert("masteryScore".into(), mastery.into());
    }
    if let Some(url) = values.return_url {
        document.insert("returnURL".into(), url.into());
    }
    if let Some(key) = values.entitlement_key {
        document.insert("entitlementKey".into(), json!({ "courseStructure": key }));
    }
    Value::Object(document)
}

/// The `cmi5LearnerPreferences` agent profile document (11). `languages`
/// is a comma-separated list of language tags, most preferred first.
pub fn learner_preferences(languages: &str) -> Value {
    json!({ "languagePreference": languages, "audioPreference": "on" })
}

/// The `launched` statement the launching system records before the unit
/// starts (9.3.1, with the context extensions of 9.6.3).
pub fn launched(
    id: &str,
    facts: &SessionFacts,
    values: &LaunchValues<'_>,
    at: DateTime<Utc>,
) -> Value {
    let mut context = facts.lms_context(&facts.publisher_id);
    let extensions = context
        .get_mut("extensions")
        .and_then(Value::as_object_mut)
        .expect("lms_context has extensions");
    extensions.insert(
        EXTENSION_LAUNCH_MODE.into(),
        facts.launch_mode.as_cmi5().into(),
    );
    extensions.insert(EXTENSION_LAUNCH_URL.into(), values.launch_url.into());
    extensions.insert(EXTENSION_MOVE_ON.into(), values.move_on.as_cmi5().into());
    if let Some(parameters) = values.launch_parameters {
        extensions.insert(EXTENSION_LAUNCH_PARAMETERS.into(), parameters.into());
    }
    if let Some(mastery) = facts.mastery_score {
        extensions.insert(EXTENSION_MASTERY_SCORE.into(), mastery.into());
    }
    let object = json!({ "objectType": "Activity", "id": facts.activity_id });
    Value::Object(statement(
        id,
        facts,
        (VERB_LAUNCHED, "launched"),
        object,
        context,
        at,
    ))
}

/// The `abandoned` statement for a session that ended without
/// `terminated` (9.3.6), with the session's length (9.5.4.2).
pub fn abandoned(id: &str, facts: &SessionFacts, seconds: f64, at: DateTime<Utc>) -> Value {
    let context = facts.lms_context(&facts.publisher_id);
    let object = json!({ "objectType": "Activity", "id": facts.activity_id });
    let mut statement = statement(
        id,
        facts,
        (VERB_ABANDONED, "abandoned"),
        object,
        context,
        at,
    );
    statement.insert(
        "result".into(),
        json!({ "duration": format_duration(seconds) }),
    );
    Value::Object(statement)
}

/// The `satisfied` statement for a block or the course whose units have
/// all met their moveOn criteria (9.3.9). `object_id` is the identifier the
/// app generated for it (never its publisher identifier, 9.4);
/// `publisher_id` is its identifier in the course structure, used as the
/// grouping activity. The session is the one whose statement satisfied it.
pub fn satisfied(
    id: &str,
    facts: &SessionFacts,
    kind: Satisfiable,
    object_id: &str,
    publisher_id: &str,
    at: DateTime<Utc>,
) -> Value {
    let context = facts.lms_context(publisher_id);
    let activity_type = match kind {
        Satisfiable::Block => ACTIVITY_TYPE_BLOCK,
        Satisfiable::Course => ACTIVITY_TYPE_COURSE,
    };
    let object = json!({
        "objectType": "Activity",
        "id": object_id,
        "definition": { "type": activity_type },
    });
    Value::Object(statement(
        id,
        facts,
        (VERB_SATISFIED, "satisfied"),
        object,
        context,
        at,
    ))
}
