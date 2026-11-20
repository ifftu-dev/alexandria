//! The run-time state of a package attempt, read and written by rule: the
//! identifier a package's content receives, what a lesson is handed at
//! launch, what the host reads from the data a lesson stores, and when a
//! package counts as complete.
//!
//! Pure: no storage and no clock. The app stores attempts and activity state
//! and calls these functions on them.
//!
//! Only SCORM 1.2 is read so far. The SCORM 2004 run-time and the cmi5 launch
//! plug in at [`read_report`], [`initial_runtime`] and [`rollup`], one match
//! arm each.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::{Launch, LaunchKind, PackageDescription, Standard};

/// Context string for the learner identifier key. Never change it: every
/// package's content has seen identifiers derived under it.
const LEARNER_ID_CONTEXT: &str = "alexandria 2026-11-20 package learner identifier v1";

/// The learner identifier a package's content receives: a keyed hash of the
/// package hash under a key derived from the learner's own key.
///
/// The same learner gets the same identifier at every attempt at one
/// package, a different one at every other package, and nobody without the
/// learner's key can link two of them or recompute one. 32 lowercase hex
/// characters, which every standard's identifier type admits.
pub fn learner_id(learner_key: &[u8; 32], package_hash: &str) -> String {
    let key = blake3::derive_key(LEARNER_ID_CONTEXT, learner_key);
    let digest = blake3::keyed_hash(&key, package_hash.trim().to_ascii_lowercase().as_bytes());
    hex(&digest.as_bytes()[..16])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What the host reads from one activity's stored run-time data.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ActivityReport {
    /// The activity counts as complete under its standard's rule. SCORM 1.2:
    /// a lesson status of `completed` or `passed`.
    pub complete: bool,
    /// The reported score normalised to 0 to 1, when one is reported.
    pub score: Option<f64>,
    /// The reported time of the session the data belongs to, in seconds.
    pub session_seconds: f64,
}

/// Reads the facts the host keeps from one activity's run-time data: the
/// library's commit payload, `{ "cmi": { … } }`. Anything missing or
/// malformed reads as not reported.
pub fn read_report(standard: Standard, runtime: &Value) -> ActivityReport {
    match standard {
        Standard::Scorm12 => scorm12::report(&runtime["cmi"]),
        // Ticket 10 (SCORM 2004 run-time) and ticket 25 (cmi5 launch).
        Standard::Scorm2004 | Standard::Cmi5 => ActivityReport::default(),
    }
}

/// How a lesson is entered, as the standards name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// The activity's first launch in this attempt.
    AbInitio,
    /// The content last exited with `suspend`.
    Resume,
    /// Neither: the standards give an empty value.
    Other,
}

impl Entry {
    pub fn as_str(self) -> &'static str {
        match self {
            Entry::AbInitio => "ab-initio",
            Entry::Resume => "resume",
            Entry::Other => "",
        }
    }
}

/// How the next launch of an activity is entered, given what it last stored
/// in this attempt (`None` when it has never been launched in it).
pub fn entry(standard: Standard, stored: Option<&Value>) -> Entry {
    let Some(stored) = stored else {
        return Entry::AbInitio;
    };
    let exit = match standard {
        Standard::Scorm12 => &stored["cmi"]["core"]["exit"],
        Standard::Scorm2004 | Standard::Cmi5 => &stored["cmi"]["exit"],
    };
    if exit.as_str() == Some("suspend") {
        Entry::Resume
    } else {
        Entry::Other
    }
}

/// The values only the host knows, for [`initial_runtime`].
#[derive(Debug, Clone, Copy)]
pub struct HostValues<'a> {
    pub learner_id: &'a str,
    pub learner_name: &'a str,
    pub entry: Entry,
    /// The activity's accumulated time across finished sessions.
    pub total_seconds: f64,
    pub launch: &'a Launch,
}

/// The run-time data to load into the library before a lesson starts: the
/// activity's stored data with the last session's own values removed (a
/// commit payload loaded unchanged would add the old session time to the
/// total again and leave `entry` empty), and every value only the host
/// knows filled in.
pub fn initial_runtime(standard: Standard, stored: Option<&Value>, host: HostValues<'_>) -> Value {
    let cmi = stored
        .and_then(|stored| stored.get("cmi"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let cmi = match standard {
        Standard::Scorm12 => scorm12::initial(cmi, host),
        // Ticket 10 (SCORM 2004 run-time) and ticket 25 (cmi5 launch).
        Standard::Scorm2004 | Standard::Cmi5 => cmi,
    };
    let mut runtime = Map::new();
    runtime.insert("cmi".into(), Value::Object(cmi));
    Value::Object(runtime)
}

/// What one activity of the current attempt has reached.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ActivityOutcome {
    pub complete: bool,
    pub score: Option<f64>,
}

/// What a package as a whole has reached in one attempt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rollup {
    /// The package reports completion: the element is complete.
    pub complete: bool,
    /// The element's score, 0 to 1, self-reported.
    pub score: Option<f64>,
}

/// Rolls an attempt's activity outcomes, keyed by activity identifier, up to
/// the package.
///
/// SCORM 1.2 has no rollup of its own and a package can hold several
/// lessons, each its own session. The rule here: the package is complete
/// when every launchable activity is complete, a lesson by its status
/// (`completed` or `passed`) and a shown-only asset by being launched. The
/// score is the mean of the scores the lessons reported.
///
/// SCORM 2004 completes on the root activity's completion status from the
/// sequencing engine (ticket 10) and cmi5 on the course's completion criteria
/// (ticket 25); until those land, neither completes.
pub fn rollup(
    description: &PackageDescription,
    outcomes: &BTreeMap<String, ActivityOutcome>,
) -> Rollup {
    let scores: Vec<f64> = outcomes.values().filter_map(|o| o.score).collect();
    let score = (!scores.is_empty()).then(|| scores.iter().sum::<f64>() / scores.len() as f64);
    let complete = match description.standard {
        Standard::Scorm12 => {
            let launchable = description.launchable_activities();
            !launchable.is_empty()
                && launchable
                    .iter()
                    .all(|activity| outcomes.get(&activity.id).is_some_and(|o| o.complete))
        }
        Standard::Scorm2004 | Standard::Cmi5 => false,
    };
    Rollup { complete, score }
}

/// Whether launching the activity is all it takes to complete it: content
/// that is only shown and reports nothing.
pub fn completes_on_launch(launch: &Launch) -> bool {
    launch.kind == LaunchKind::Asset
}

mod scorm12 {
    //! SCORM 1.2: `cmi.core.*` names, `CMITimespan` times, scores on the
    //! lesson's own `min`..`max` scale (0 to 100 when unset).

    use serde_json::{Map, Value};

    use super::{ActivityReport, HostValues};

    pub(super) fn report(cmi: &Value) -> ActivityReport {
        let core = &cmi["core"];
        let complete = matches!(core["lesson_status"].as_str(), Some("completed" | "passed"));
        ActivityReport {
            complete,
            score: score(&core["score"]),
            session_seconds: core["session_time"]
                .as_str()
                .and_then(parse_timespan)
                .unwrap_or(0.0),
        }
    }

    fn number(value: &Value) -> Option<f64> {
        match value {
            Value::Number(number) => number.as_f64(),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        }
        .filter(|n: &f64| n.is_finite())
    }

    fn score(score: &Value) -> Option<f64> {
        let raw = number(&score["raw"])?;
        let min = number(&score["min"]).unwrap_or(0.0);
        let max = number(&score["max"]).unwrap_or(100.0);
        let (min, max) = if max > min { (min, max) } else { (0.0, 100.0) };
        Some(((raw - min) / (max - min)).clamp(0.0, 1.0))
    }

    /// `HHHH:MM:SS.SS`: two to four hour digits, optional hundredths.
    pub(super) fn parse_timespan(text: &str) -> Option<f64> {
        let mut parts = text.trim().split(':');
        let (hours, minutes, seconds) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
        if !digits(hours) || !digits(minutes) {
            return None;
        }
        let whole_seconds = seconds.split('.').next().unwrap_or_default();
        if !digits(whole_seconds) {
            return None;
        }
        let hours: f64 = hours.parse().ok()?;
        let minutes: f64 = minutes.parse().ok()?;
        let seconds: f64 = seconds.parse().ok()?;
        Some(hours * 3600.0 + minutes * 60.0 + seconds)
    }

    /// The longest time a `CMITimespan` can say.
    const MAX_CENTISECONDS: u64 = (9999 * 3600 + 59 * 60 + 59) * 100 + 99;

    pub(super) fn format_timespan(seconds: f64) -> String {
        let centis = ((seconds.max(0.0) * 100.0).round() as u64).min(MAX_CENTISECONDS);
        let (whole, fraction) = (centis / 100, centis % 100);
        let (hours, minutes, secs) = (whole / 3600, whole / 60 % 60, whole % 60);
        if fraction == 0 {
            format!("{hours:04}:{minutes:02}:{secs:02}")
        } else {
            format!("{hours:04}:{minutes:02}:{secs:02}.{fraction:02}")
        }
    }

    fn decimal(value: f64) -> String {
        if value.fract() == 0.0 {
            format!("{value:.0}")
        } else {
            value.to_string()
        }
    }

    /// Takes the named member out as an object, or an empty one when it is
    /// missing or not an object.
    fn take_object(map: &mut Map<String, Value>, key: &str) -> Map<String, Value> {
        match map.remove(key) {
            Some(Value::Object(object)) => object,
            _ => Map::new(),
        }
    }

    pub(super) fn initial(mut cmi: Map<String, Value>, host: HostValues<'_>) -> Map<String, Value> {
        let mut core = take_object(&mut cmi, "core");
        core.remove("session_time");
        core.remove("exit");
        core.insert("student_id".into(), host.learner_id.into());
        core.insert("student_name".into(), host.learner_name.into());
        core.insert("entry".into(), host.entry.as_str().into());
        core.insert("credit".into(), "credit".into());
        core.insert("lesson_mode".into(), "normal".into());
        core.insert(
            "total_time".into(),
            format_timespan(host.total_seconds).into(),
        );
        cmi.insert("core".into(), Value::Object(core));

        let launch = host.launch;
        match &launch.launch_data {
            Some(data) => cmi.insert("launch_data".into(), data.clone().into()),
            None => cmi.remove("launch_data"),
        };
        let mut student_data = take_object(&mut cmi, "student_data");
        let manifest_values = [
            ("mastery_score", launch.mastery_score.map(decimal)),
            ("max_time_allowed", launch.max_time_allowed.clone()),
            ("time_limit_action", launch.time_limit_action.clone()),
        ];
        for (name, value) in manifest_values {
            match value {
                Some(value) => student_data.insert(name.into(), value.into()),
                None => student_data.remove(name),
            };
        }
        cmi.insert("student_data".into(), Value::Object(student_data));
        cmi
    }
}
