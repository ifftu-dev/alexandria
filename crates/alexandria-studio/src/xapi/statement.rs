//! The statement as it goes on the wire, and the checks every statement
//! passes before it is stored or sent.
//!
//! Only the subset valid in both xAPI 1.0.3 and 2.0 is representable: no
//! context agents or groups, no sub-statements, no attachments, no
//! signatures, durations at 0.01 s precision and timestamps in UTC.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, Serializer};

use super::{StatementError, MAX_ACCOUNT_NAME_LEN, MAX_IRI_LEN};

/// A language map: RFC 5646 tag → text.
pub type LanguageMap = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Statement {
    pub id: String,
    pub actor: Agent,
    pub verb: VerbObject,
    pub object: StatementObject,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<StatementResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Context>,
}

/// Always an identified Agent with an account: never `mbox` (personal data)
/// or `openid` (a store refused a DID there).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "objectType", deny_unknown_fields)]
pub enum Agent {
    Agent { account: Account },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Account {
    pub home_page: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerbObject {
    pub id: String,
    pub display: LanguageMap,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "objectType")]
pub enum StatementObject {
    Activity(Box<Activity>),
    StatementRef { id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Activity {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<Definition>,
}

/// An activity inside `context.contextActivities`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "objectType")]
pub enum ContextActivity {
    Activity(Activity),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Definition {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub activity_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<LanguageMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<LanguageMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correct_responses_pattern: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<Component>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Vec<Component>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Vec<Component>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Vec<Component>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<Component>>,
}

/// One choice, scale point, source, target or step of an interaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<LanguageMap>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatementResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<Score>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<BTreeMap<String, serde_json::Value>>,
}

/// Whole numbers serialise as JSON integers (`85`, not `85.0`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Score {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "number"
    )]
    pub scaled: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "number"
    )]
    pub raw: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "number"
    )]
    pub min: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "number"
    )]
    pub max: Option<f64>,
}

fn number<S: Serializer>(value: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(v) if v.fract() == 0.0 && v.abs() < 9.0e15 => s.serialize_i64(*v as i64),
        Some(v) => s.serialize_f64(*v),
        None => s.serialize_none(),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_activities: Option<ContextActivities>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextActivities {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parent: Vec<ContextActivity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grouping: Vec<ContextActivity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub category: Vec<ContextActivity>,
}

pub(super) const VOIDED: &str = "http://adlnet.gov/expapi/verbs/voided";
pub(super) const PROGRESS_EXTENSION: &str = "https://w3id.org/xapi/cmi5/result/extensions/progress";

const INTERACTION_TYPES: [&str; 10] = [
    "true-false",
    "choice",
    "fill-in",
    "long-fill-in",
    "matching",
    "performance",
    "sequencing",
    "likert",
    "numeric",
    "other",
];

impl Account {
    pub fn validate(&self) -> Result<(), StatementError> {
        iri("account home page", &self.home_page)
            .map_err(|e| StatementError::InvalidAccount(e.to_string()))?;
        let len = self.name.chars().count();
        if len == 0 || len > MAX_ACCOUNT_NAME_LEN {
            return Err(StatementError::InvalidAccount(format!(
                "an account name must be 1 to {MAX_ACCOUNT_NAME_LEN} characters"
            )));
        }
        Ok(())
    }
}

impl Statement {
    /// Every rule a statement must meet before it is stored or sent. The
    /// builder runs it on what it builds; a statement read back from storage
    /// can be checked again the same way.
    pub fn validate(&self) -> Result<(), StatementError> {
        uuid_field("statement id", &self.id)?;
        let Agent::Agent { account } = &self.actor;
        account.validate()?;
        iri("verb id", &self.verb.id)?;
        language_map("verb display", &self.verb.display)?;
        timestamp(&self.timestamp)?;
        match &self.object {
            StatementObject::Activity(activity) => {
                if self.verb.id == VOIDED {
                    return Err(StatementError::InvalidStatement(
                        "a voiding statement must reference a statement".into(),
                    ));
                }
                activity_check(activity)?;
            }
            StatementObject::StatementRef { id } => {
                if self.verb.id != VOIDED {
                    return Err(StatementError::InvalidStatement(
                        "only a voiding statement references a statement".into(),
                    ));
                }
                uuid_field("referenced statement id", id)?;
                if self.result.is_some() || self.context.is_some() {
                    return Err(StatementError::InvalidStatement(
                        "a voiding statement carries no result or context".into(),
                    ));
                }
            }
        }
        if let Some(result) = &self.result {
            result_check(result)?;
        }
        if let Some(context) = &self.context {
            context_check(context)?;
        }
        Ok(())
    }
}

fn result_check(result: &StatementResult) -> Result<(), StatementError> {
    if let Some(score) = &result.score {
        score_check(score)?;
    }
    if let Some(duration) = &result.duration {
        duration_check(duration)?;
    }
    for (key, value) in result.extensions.iter().flatten() {
        iri("result extension", key)?;
        if key == PROGRESS_EXTENSION && value.as_u64().is_none_or(|p| p > 100) {
            return Err(StatementError::InvalidResult(
                "progress must be a whole number from 0 to 100".into(),
            ));
        }
    }
    Ok(())
}

fn score_check(score: &Score) -> Result<(), StatementError> {
    let bad = |m: &str| Err(StatementError::InvalidResult(m.into()));
    let values = [score.scaled, score.raw, score.min, score.max];
    if values.iter().flatten().any(|v| !v.is_finite()) {
        return bad("a score must be a finite number");
    }
    if score.scaled.is_some_and(|s| !(-1.0..=1.0).contains(&s)) {
        return bad("a scaled score must be between -1 and 1");
    }
    if let (Some(min), Some(max)) = (score.min, score.max) {
        if min >= max {
            return bad("a score's minimum must be below its maximum");
        }
    }
    if let Some(raw) = score.raw {
        if score.min.is_some_and(|min| raw < min) || score.max.is_some_and(|max| raw > max) {
            return bad("a raw score must lie between the minimum and maximum");
        }
    }
    if values.iter().all(Option::is_none) {
        return bad("a score must carry a value");
    }
    Ok(())
}

/// `PT[nH][nM][n[.nn]S]`, the only duration form the builder writes: no
/// date part and at most two decimals, inside both versions' rules.
fn duration_check(duration: &str) -> Result<(), StatementError> {
    let bad = || {
        Err(StatementError::InvalidResult(format!(
            "duration {duration:?} is not of the form PT#H#M#.##S"
        )))
    };
    let Some(mut rest) = duration.strip_prefix("PT") else {
        return bad();
    };
    if rest.is_empty() {
        return bad();
    }
    for unit in ['H', 'M', 'S'] {
        if let Some(at) = rest.find(unit) {
            let number = &rest[..at];
            let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
            let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
            let fraction_ok = unit == 'S' && fraction.len() <= 2 && digits(fraction);
            if whole.is_empty()
                || !digits(whole)
                || (number.contains('.') && (!fraction_ok || fraction.is_empty()))
            {
                return bad();
            }
            rest = &rest[at + 1..];
        }
    }
    if rest.is_empty() {
        Ok(())
    } else {
        bad()
    }
}

fn context_check(context: &Context) -> Result<(), StatementError> {
    if let Some(registration) = &context.registration {
        uuid_field("registration", registration)?;
    }
    if let Some(language) = &context.language {
        language_tag(language)?;
    }
    if let Some(activities) = &context.context_activities {
        for ContextActivity::Activity(activity) in activities
            .parent
            .iter()
            .chain(&activities.grouping)
            .chain(&activities.category)
        {
            activity_check(activity)?;
        }
    }
    Ok(())
}

fn activity_check(activity: &Activity) -> Result<(), StatementError> {
    iri("activity id", &activity.id)?;
    let Some(definition) = &activity.definition else {
        return Ok(());
    };
    if let Some(t) = &definition.activity_type {
        iri("activity type", t)?;
    }
    for map in [&definition.name, &definition.description]
        .into_iter()
        .flatten()
    {
        language_map("activity definition", map)?;
    }
    let components = [
        &definition.choices,
        &definition.scale,
        &definition.source,
        &definition.target,
        &definition.steps,
    ];
    match &definition.interaction_type {
        Some(t) if !INTERACTION_TYPES.contains(&t.as_str()) => {
            return Err(StatementError::InvalidActivity(format!(
                "unknown interaction type {t:?}"
            )));
        }
        None if definition.correct_responses_pattern.is_some()
            || components.iter().any(|c| c.is_some()) =>
        {
            return Err(StatementError::InvalidActivity(
                "interaction details need an interaction type".into(),
            ));
        }
        _ => {}
    }
    for list in components.into_iter().flatten() {
        let mut seen = std::collections::BTreeSet::new();
        for component in list {
            if component.id.is_empty() || !seen.insert(component.id.as_str()) {
                return Err(StatementError::InvalidActivity(
                    "interaction component ids must be present and unique".into(),
                ));
            }
            if let Some(description) = &component.description {
                language_map("interaction component", description)?;
            }
        }
    }
    Ok(())
}

/// An absolute IRI: a scheme, no whitespace or control characters, within
/// [`MAX_IRI_LEN`].
pub(super) fn iri(what: &str, value: &str) -> Result<(), StatementError> {
    let bad = |m: String| Err(StatementError::InvalidStatement(m));
    if value.len() > MAX_IRI_LEN {
        return bad(format!("{what} is longer than {MAX_IRI_LEN} characters"));
    }
    let Some((scheme, rest)) = value.split_once(':') else {
        return bad(format!("{what} {value:?} has no scheme"));
    };
    let scheme_ok = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !scheme_ok || rest.is_empty() {
        return bad(format!("{what} {value:?} is not an absolute IRI"));
    }
    if value
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"'))
    {
        return bad(format!("{what} contains characters an IRI cannot hold"));
    }
    Ok(())
}

fn uuid_field(what: &str, value: &str) -> Result<(), StatementError> {
    let parsed = uuid::Uuid::try_parse(value).ok();
    // Hyphenated lower-case only: the form every store round-trips.
    if parsed.is_some_and(|u| u.hyphenated().to_string() == value) {
        Ok(())
    } else {
        Err(StatementError::InvalidIdentifier(format!(
            "{what} {value:?} is not a lower-case hyphenated UUID"
        )))
    }
}

fn timestamp(value: &str) -> Result<(), StatementError> {
    match chrono::DateTime::parse_from_rfc3339(value) {
        Ok(_) if value.ends_with('Z') => Ok(()),
        _ => Err(StatementError::InvalidStatement(format!(
            "timestamp {value:?} is not an RFC 3339 UTC time"
        ))),
    }
}

fn language_map(what: &str, map: &LanguageMap) -> Result<(), StatementError> {
    if map.is_empty() {
        return Err(StatementError::InvalidLanguage(format!(
            "{what} has an empty language map"
        )));
    }
    map.keys().try_for_each(|tag| language_tag(tag))
}

/// A well-formed RFC 5646 tag in its common shape: a 2–8 letter primary
/// subtag and 1–8 character alphanumeric subtags.
pub(super) fn language_tag(tag: &str) -> Result<(), StatementError> {
    let mut parts = tag.split('-');
    let primary = parts.next().unwrap_or_default();
    let primary_ok =
        (2..=8).contains(&primary.len()) && primary.chars().all(|c| c.is_ascii_alphabetic());
    let rest_ok =
        parts.all(|p| (1..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()));
    if primary_ok && rest_ok {
        Ok(())
    } else {
        Err(StatementError::InvalidLanguage(format!(
            "{tag:?} is not a language tag"
        )))
    }
}
