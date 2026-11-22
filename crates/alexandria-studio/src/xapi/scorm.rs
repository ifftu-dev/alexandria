//! SCORM run-time data to learning events, following the xAPI SCORM profile
//! (`https://w3id.org/xapi/scorm`).
//!
//! One terminated SCO session gives at most one completion-level event,
//! whose verb follows the profile's status mapping (passed / failed from the
//! success status, else completed, else scored), carrying the score and the
//! session time; and one interaction-level `responded` event per interaction
//! with a response. Every event carries the profile's category activity.
//!
//! The data comes from package script and is untrusted. A value that does
//! not parse (a score out of range, a malformed time, an unknown interaction
//! type) is left out rather than costing the learner the whole record; what
//! remains is still validated by the builder.
//!
//! Deliberate differences from the profile: no `initialized`, `terminated`,
//! `suspended` or `resumed` session statements (the app records element
//! launch itself, and the spec records outcomes, not sessions), and no
//! attempt activity in the grouping: the registration identifies the attempt,
//! and an attempt IRI would be one more identifier to derive per recipient.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use super::build::{Action, Interaction, InteractionType, LearningEvent, Outcome, Profile, Target};
use super::identifiers::ActivityRef;
use super::statement::{LanguageMap, Score};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScormEdition {
    /// SCORM 1.2 (`cmi.core.*`).
    Scorm12,
    /// SCORM 2004, any edition (`cmi.completion_status`, `cmi.success_status`).
    Scorm2004,
}

/// One SCO session as the host knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScormSession {
    pub edition: ScormEdition,
    /// The package activity (SCO) the data belongs to.
    pub activity: ActivityRef,
    pub name: Option<LanguageMap>,
    /// The local id of the session's result record. Interaction records get
    /// `<local id>|interaction|<n>`, `n` being the SCORM interaction index.
    pub local_id: String,
    /// The local package attempt id.
    pub registration: Option<String>,
    pub timestamp: DateTime<Utc>,
    /// Language of the interaction descriptions the package wrote.
    pub language: String,
}

/// The learning events for one session's run-time data, keyed by data model
/// element name (`cmi.core.lesson_status`, `cmi.interactions.0.id`, …; see
/// [`flatten_cmi`]).
pub fn events(session: &ScormSession, cmi: &BTreeMap<String, String>) -> Vec<LearningEvent> {
    let mut out = Vec::new();
    if let Some(action) = result_action(session, cmi) {
        out.push(event(session, session.local_id.clone(), action));
    }
    for n in interaction_indices(cmi) {
        if let Some(action) = interaction_action(session, cmi, n) {
            let local_id = format!("{}|interaction|{n}", session.local_id);
            out.push(event(session, local_id, action));
        }
    }
    out
}

fn event(session: &ScormSession, local_id: String, action: Action) -> LearningEvent {
    LearningEvent {
        local_id,
        timestamp: session.timestamp,
        registration: session.registration.clone(),
        action,
        profile: Some(Profile::Scorm),
    }
}

fn result_action(session: &ScormSession, cmi: &BTreeMap<String, String>) -> Option<Action> {
    let get = |key: &str| cmi.get(key).map(String::as_str);
    let (success, completion, score, duration) = match session.edition {
        ScormEdition::Scorm12 => {
            let (success, completion) = match get("cmi.core.lesson_status") {
                Some("passed") => (Some(true), Some(true)),
                Some("failed") => (Some(false), None),
                Some("completed") => (None, Some(true)),
                _ => (None, None),
            };
            let score = score_12(
                get("cmi.core.score.raw"),
                get("cmi.core.score.min"),
                get("cmi.core.score.max"),
            );
            let duration = get("cmi.core.session_time").and_then(timespan_12);
            (success, completion, score, duration)
        }
        ScormEdition::Scorm2004 => {
            let success = match get("cmi.success_status") {
                Some("passed") => Some(true),
                Some("failed") => Some(false),
                _ => None,
            };
            let completion = match get("cmi.completion_status") {
                Some("completed") => Some(true),
                Some("incomplete") => Some(false),
                _ => None,
            };
            let score = score_2004(
                get("cmi.score.scaled"),
                get("cmi.score.raw"),
                get("cmi.score.min"),
                get("cmi.score.max"),
            );
            let duration = get("cmi.session_time").and_then(interval_2004);
            (success, completion, score, duration)
        }
    };
    let target = Target {
        activity: session.activity.clone(),
        name: session.name.clone(),
    };
    let outcome = Outcome {
        score,
        duration_centiseconds: duration,
        ..Outcome::default()
    };
    Some(match (success, completion) {
        (Some(true), _) => Action::Passed {
            target,
            outcome: Outcome {
                completion,
                ..outcome
            },
        },
        (Some(false), _) => Action::Failed {
            target,
            outcome: Outcome {
                completion,
                ..outcome
            },
        },
        (None, Some(true)) => Action::Completed { target, outcome },
        _ if outcome.score.is_some() => Action::Scored { target, outcome },
        _ => return None,
    })
}

fn number(value: Option<&str>) -> Option<f64> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

/// Raw, minimum and maximum, kept only if they are consistent.
fn range(
    raw: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
) -> Option<(f64, Option<f64>, Option<f64>)> {
    let raw = raw?;
    let consistent = min.is_none_or(|m| m <= raw)
        && max.is_none_or(|m| raw <= m)
        && !matches!((min, max), (Some(lo), Some(hi)) if lo >= hi);
    consistent.then_some((raw, min, max))
}

fn scaled(value: f64) -> Option<f64> {
    (-1.0..=1.0).contains(&value).then_some(value)
}

/// SCORM 1.2 has no scaled score. The profile divides the raw score by 100;
/// when the package states a range, the raw score is placed within it.
fn score_12(raw: Option<&str>, min: Option<&str>, max: Option<&str>) -> Option<Score> {
    let (raw, min, max) = range(number(raw), number(min), number(max))?;
    let scaled_value = match (min, max) {
        (Some(lo), Some(hi)) => (raw - lo) / (hi - lo),
        _ => raw / 100.0,
    };
    Some(Score {
        scaled: scaled(scaled_value),
        raw: Some(raw),
        min,
        max,
    })
}

fn score_2004(
    scaled_value: Option<&str>,
    raw: Option<&str>,
    min: Option<&str>,
    max: Option<&str>,
) -> Option<Score> {
    let scaled_value = number(scaled_value).and_then(scaled);
    let (raw, min, max) = match range(number(raw), number(min), number(max)) {
        Some((raw, min, max)) => (Some(raw), min, max),
        None => (None, None, None),
    };
    (scaled_value.is_some() || raw.is_some()).then_some(Score {
        scaled: scaled_value,
        raw,
        min,
        max,
    })
}

/// SCORM 1.2 CMITimespan `HHHH:MM:SS[.S[S]]` in hundredths of a second.
fn timespan_12(value: &str) -> Option<u64> {
    let mut parts = value.trim().split(':');
    let (h, m, s) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !(2..=4).contains(&h.len()) || m.len() != 2 {
        return None;
    }
    let (hours, minutes) = (digits(h)?, digits(m)?);
    let (whole, fraction) = s.split_once('.').unwrap_or((s, ""));
    if whole.len() != 2 || minutes > 59 || fraction.len() > 2 {
        return None;
    }
    let seconds = digits(whole)?;
    if seconds > 59 {
        return None;
    }
    Some((hours * 3600 + minutes * 60 + seconds) * 100 + centis(fraction)?)
}

/// SCORM 2004 timeinterval (ISO 8601 duration) in hundredths of a second.
/// Years and months have no fixed length and are refused.
fn interval_2004(value: &str) -> Option<u64> {
    let rest = value.trim().strip_prefix('P')?;
    let (date, time) = rest.split_once('T').unwrap_or((rest, ""));
    if rest.is_empty() || rest.ends_with('T') {
        return None;
    }
    let mut total = 0u64;
    let mut take = |part: &str, units: &[(char, u64)], fraction_allowed: bool| -> Option<()> {
        let mut rest = part;
        for &(unit, centis_per) in units {
            if let Some(at) = rest.find(unit) {
                let number = &rest[..at];
                let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
                if number.contains('.') && (!fraction_allowed || unit != 'S' || fraction.is_empty())
                {
                    return None;
                }
                let fraction = &fraction[..fraction.len().min(2)];
                total = total
                    .checked_add(digits(whole)?.checked_mul(centis_per)?)?
                    .checked_add(if unit == 'S' { centis(fraction)? } else { 0 })?;
                rest = &rest[at + 1..];
            }
        }
        rest.is_empty().then_some(())
    };
    take(date, &[('D', 8_640_000)], false)?;
    take(time, &[('H', 360_000), ('M', 6_000), ('S', 100)], true)?;
    Some(total)
}

fn digits(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// One or two decimal digits of a second as hundredths.
fn centis(fraction: &str) -> Option<u64> {
    match fraction.len() {
        0 => Some(0),
        1 => Some(digits(fraction)? * 10),
        _ => digits(fraction),
    }
}

fn interaction_indices(cmi: &BTreeMap<String, String>) -> Vec<u64> {
    let mut indices: Vec<u64> = cmi
        .keys()
        .filter_map(|k| k.strip_prefix("cmi.interactions."))
        .filter_map(|rest| rest.split('.').next()?.parse().ok())
        .collect();
    indices.sort_unstable();
    indices.dedup();
    indices
}

fn interaction_action(
    session: &ScormSession,
    cmi: &BTreeMap<String, String>,
    n: u64,
) -> Option<Action> {
    let field = |name: &str| {
        cmi.get(&format!("cmi.interactions.{n}.{name}"))
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    };
    let id = field("id")?;
    let interaction_type = InteractionType::parse(field("type")?)?;
    let scorm12 = session.edition == ScormEdition::Scorm12;
    let convert = |value: &str| {
        if scorm12 {
            response_12(interaction_type, value)
        } else {
            Some(value.to_string())
        }
    };
    let response_key = if scorm12 {
        "student_response"
    } else {
        "learner_response"
    };
    let response = convert(field(response_key)?)?;
    let mut patterns: Vec<(u64, String)> = cmi
        .iter()
        .filter_map(|(k, v)| {
            let rest = k.strip_prefix(&format!("cmi.interactions.{n}.correct_responses."))?;
            let m = rest.strip_suffix(".pattern")?.parse().ok()?;
            Some((m, convert(v)?))
        })
        .collect();
    patterns.sort_by_key(|(m, _)| *m);

    let mut interaction = Interaction::new(session.activity.interaction(id), interaction_type);
    interaction.correct_responses =
        (!patterns.is_empty()).then(|| patterns.into_iter().map(|(_, p)| p).collect());
    interaction.description = field("description")
        .map(|text| BTreeMap::from([(session.language.clone(), text.to_string())]));
    let success = match field("result") {
        Some("correct") => Some(true),
        Some("wrong" | "incorrect") => Some(false),
        _ => None,
    };
    Some(Action::Responded {
        interaction,
        outcome: Outcome {
            success,
            response: Some(response),
            ..Outcome::default()
        },
    })
}

/// SCORM 1.2 response and pattern syntax to the xAPI (SCORM 2004) syntax.
fn response_12(kind: InteractionType, value: &str) -> Option<String> {
    let list = |v: &str| -> Vec<String> {
        v.trim()
            .trim_start_matches('{')
            .trim_end_matches('}')
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    };
    Some(match kind {
        InteractionType::TrueFalse => match value.trim() {
            "t" | "true" | "1" => "true".into(),
            "f" | "false" | "0" => "false".into(),
            _ => return None,
        },
        InteractionType::Choice | InteractionType::Sequencing => list(value).join("[,]"),
        InteractionType::Matching => list(value)
            .iter()
            .map(|pair| pair.replacen('.', "[.]", 1))
            .collect::<Vec<_>>()
            .join("[,]"),
        _ => value.to_string(),
    })
}

/// Flattens stored run-time JSON (`{ "cmi": { … } }` or the bare `cmi`
/// object, arrays or index-keyed objects for collections) to data model
/// element names. Nulls are dropped.
pub fn flatten_cmi(value: &serde_json::Value) -> BTreeMap<String, String> {
    let root = value.get("cmi").unwrap_or(value);
    let mut out = BTreeMap::new();
    flatten_into("cmi", root, &mut out);
    out
}

fn flatten_into(prefix: &str, value: &serde_json::Value, out: &mut BTreeMap<String, String>) {
    use serde_json::Value;
    match value {
        Value::Null => {}
        Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        Value::Bool(_) | Value::Number(_) => {
            out.insert(prefix.to_string(), value.to_string());
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                flatten_into(&format!("{prefix}.{i}"), item, out);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                flatten_into(&format!("{prefix}.{key}"), item, out);
            }
        }
    }
}
