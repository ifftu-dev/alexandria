//! The run-time state of a package attempt, read and written by rule: the
//! identifier a package's content receives, what a lesson is handed at
//! launch, what the host reads from the data a lesson stores, and when a
//! package counts as complete.
//!
//! Pure: no storage and no clock. The app stores attempts and activity state
//! and calls these functions on them.
//!
//! SCORM 1.2 and SCORM 2004 are read. The cmi5 launch plugs in at
//! [`read_report`], [`initial_runtime`] and [`rollup`], one match arm each.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
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
        Standard::Scorm2004 => scorm2004::report(&runtime["cmi"]),
        // Ticket 25 (cmi5 launch).
        Standard::Cmi5 => ActivityReport::default(),
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
    } else if standard == Standard::Scorm2004 {
        // The lesson's attempt ended; the next launch is a new one.
        Entry::AbInitio
    } else {
        Entry::Other
    }
}

/// How an activity the SCORM 2004 sequencing engine delivered is entered.
/// `resumed` is the engine's word that the delivery resumes a suspended
/// activity (resume all after a suspend all), which keeps the lesson's data
/// even when the lesson itself did not exit with `suspend`. Other standards
/// have no engine and follow [`entry`].
pub fn delivery_entry(standard: Standard, stored: Option<&Value>, resumed: bool) -> Entry {
    if resumed && standard == Standard::Scorm2004 && stored.is_some() {
        return Entry::Resume;
    }
    entry(standard, stored)
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
        Standard::Scorm2004 => scorm2004::initial(cmi, host),
        // Ticket 25 (cmi5 launch).
        Standard::Cmi5 => cmi,
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

/// What the SCORM 2004 sequencing engine reports for a package's root
/// activity, the course as a whole, after its own rollup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootStatus {
    /// `completed`, `incomplete`, `not attempted` or `unknown`.
    pub completion_status: String,
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
/// SCORM 2004 completes when the sequencing engine reports the root
/// activity's completion status as `completed` (`root`, absent until the
/// engine has reported); its score is worked out as for SCORM 1.2. cmi5
/// completes on the course's completion criteria (ticket 25); until that
/// lands, it does not complete.
pub fn rollup(
    description: &PackageDescription,
    outcomes: &BTreeMap<String, ActivityOutcome>,
    root: Option<&RootStatus>,
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
        Standard::Scorm2004 => root.is_some_and(|root| root.completion_status == "completed"),
        Standard::Cmi5 => false,
    };
    Rollup { complete, score }
}

/// Whether launching the activity is all it takes to complete it: content
/// that is only shown and reports nothing.
pub fn completes_on_launch(launch: &Launch) -> bool {
    launch.kind == LaunchKind::Asset
}

/// A number the content wrote as a JSON number or as text.
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
    .filter(|n: &f64| n.is_finite())
}

/// A decimal as the data models spell it: no trailing `.0`.
fn decimal(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

mod scorm12 {
    //! SCORM 1.2: `cmi.core.*` names, `CMITimespan` times, scores on the
    //! lesson's own `min`..`max` scale (0 to 100 when unset).

    use serde_json::{Map, Value};

    use super::{decimal, number, ActivityReport, HostValues};

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

mod scorm2004 {
    //! SCORM 2004: `cmi.*` names, ISO 8601 durations, scaled scores from -1
    //! to 1. One behaviour serves the 2nd, 3rd and 4th Editions.
    //!
    //! A learner attempt on a lesson ends unless it is suspended, and the
    //! next launch is then a new attempt with new run-time data. Only a
    //! resumed lesson is handed its stored data again.

    use serde_json::{Map, Value};

    use super::{decimal, number, ActivityReport, Entry, HostValues};

    pub(super) fn report(cmi: &Value) -> ActivityReport {
        ActivityReport {
            complete: cmi["completion_status"].as_str() == Some("completed"),
            score: score(&cmi["score"]),
            session_seconds: cmi["session_time"]
                .as_str()
                .and_then(parse_duration)
                .unwrap_or(0.0),
        }
    }

    /// The scaled score, a negative one counted as zero; else the raw score
    /// against the lesson's own range when it states one.
    fn score(score: &Value) -> Option<f64> {
        if let Some(scaled) = number(&score["scaled"]) {
            return Some(scaled.clamp(0.0, 1.0));
        }
        let raw = number(&score["raw"])?;
        let (min, max) = (number(&score["min"])?, number(&score["max"])?);
        (max > min).then(|| ((raw - min) / (max - min)).clamp(0.0, 1.0))
    }

    /// Seconds in one designator's unit. Years and months have no fixed
    /// length; they count as 365 and 30 days.
    const DATE_UNITS: [(char, f64); 3] = [
        ('Y', 365.0 * 86400.0),
        ('M', 30.0 * 86400.0),
        ('D', 86400.0),
    ];
    const TIME_UNITS: [(char, f64); 3] = [('H', 3600.0), ('M', 60.0), ('S', 1.0)];

    /// Adds one part of a duration (`1Y2M3D` or `4H5M6.7S`) to `seconds`.
    /// Designators come in order, each at most once; only seconds may have a
    /// fraction.
    fn add_part(part: &str, units: &[(char, f64)], seconds: &mut f64) -> Option<usize> {
        let mut digits = String::new();
        let mut remaining = units.iter();
        let mut found = 0;
        for c in part.chars() {
            if c.is_ascii_digit() || c == '.' {
                digits.push(c);
                continue;
            }
            let (unit, scale) = remaining.find(|(unit, _)| *unit == c)?;
            if digits.is_empty() || (digits.contains('.') && *unit != 'S') {
                return None;
            }
            *seconds += digits.parse::<f64>().ok()? * scale;
            digits.clear();
            found += 1;
        }
        digits.is_empty().then_some(found)
    }

    /// An XML Schema duration, `P[nY][nM][nD][T[nH][nM][n[.n]S]]`.
    pub(super) fn parse_duration(text: &str) -> Option<f64> {
        let rest = text.trim().strip_prefix('P')?;
        let (date, time) = match rest.split_once('T') {
            Some((_, "")) => return None,
            Some((date, time)) => (date, time),
            None => (rest, ""),
        };
        let mut seconds = 0.0;
        let found =
            add_part(date, &DATE_UNITS, &mut seconds)? + add_part(time, &TIME_UNITS, &mut seconds)?;
        (found > 0).then_some(seconds)
    }

    /// `PT#H#M#S`, to the hundredth of a second.
    pub(super) fn format_duration(seconds: f64) -> String {
        let centis = (seconds.max(0.0) * 100.0).round() as u64;
        let (whole, fraction) = (centis / 100, centis % 100);
        let (hours, minutes, secs) = (whole / 3600, whole / 60 % 60, whole % 60);
        let mut out = String::from("PT");
        if hours > 0 {
            out.push_str(&format!("{hours}H"));
        }
        if minutes > 0 {
            out.push_str(&format!("{minutes}M"));
        }
        if fraction > 0 {
            out.push_str(&format!("{secs}.{fraction:02}S"));
        } else if secs > 0 || (hours == 0 && minutes == 0) {
            out.push_str(&format!("{secs}S"));
        }
        out
    }

    pub(super) fn initial(cmi: Map<String, Value>, host: HostValues<'_>) -> Map<String, Value> {
        let resumed = host.entry == Entry::Resume;
        let mut cmi = if resumed { cmi } else { Map::new() };
        cmi.remove("session_time");
        cmi.remove("exit");
        cmi.insert("learner_id".into(), host.learner_id.into());
        cmi.insert("learner_name".into(), host.learner_name.into());
        cmi.insert("entry".into(), host.entry.as_str().into());
        cmi.insert("credit".into(), "credit".into());
        cmi.insert("mode".into(), "normal".into());
        let total = if resumed { host.total_seconds } else { 0.0 };
        cmi.insert("total_time".into(), format_duration(total).into());

        let launch = host.launch;
        let manifest_values = [
            ("launch_data", launch.launch_data.clone()),
            ("scaled_passing_score", launch.mastery_score.map(decimal)),
            ("max_time_allowed", launch.max_time_allowed.clone()),
            ("time_limit_action", launch.time_limit_action.clone()),
        ];
        for (name, value) in manifest_values {
            match value {
                Some(value) => cmi.insert(name.into(), value.into()),
                None => cmi.remove(name),
            };
        }
        cmi
    }
}
