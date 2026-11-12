//! Response-process features: how a learner responded to each item, measured
//! the same way for a choice click, a typed answer or a plugin event.
//!
//! The client records the timeline of each item visit (presentation, inputs,
//! answer changes) and submits only per-visit aggregates. The backend judges
//! them against a reading floor from the item's length and difficulty, and
//! against the learner's own response baseline. Every threshold here is
//! unmeasured: the signals are advisory (severity `Info`) until the
//! measurement corpus certifies a likelihood ratio for them.

use serde::{Deserialize, Serialize};

/// Fixed part of the reading floor: orienting to a new item.
const READING_FLOOR_BASE_MS: f64 = 800.0;
/// Fast skimming, about 50 characters a second. A floor, not a typical pace.
const READING_FLOOR_MS_PER_CHAR: f64 = 20.0;
/// Each difficulty level above the easiest adds this share of reading time.
const READING_FLOOR_DIFFICULTY_STEP: f64 = 0.15;
/// Baseline items needed before relay lag can be judged.
const BASELINE_MIN_ITEMS: usize = 3;
/// Items in one window needed before their lag can be called uniform.
const RELAY_MIN_ITEMS: usize = 2;
/// Lag every item must carry over the learner's baseline.
const RELAY_MIN_ADDED_MS: f64 = 3_000.0;
/// Spread of the added lag, as a share of its mean, still called uniform.
const RELAY_UNIFORM_SPREAD: f64 = 0.5;
/// Bounds that no honest collector exceeds in one window.
const MAX_ITEMS: usize = 64;
const MAX_BASELINE_ITEMS: usize = 32;
const MAX_ITEM_ID_LEN: usize = 128;

/// The flags this module raises, in the order they are emitted.
pub const RESPONSE_PROCESS_SIGNALS: &[&str] = &[
    "response_below_reading_floor",
    "response_relay_lag",
    "response_idle_burst",
    "answer_change_after_idle",
    "gaze_down_then_response",
    "voice_then_response",
];

/// How the learner answers an item. The features are the same for every
/// kind; the kind lets the corpus measure them separately.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    #[default]
    Choice,
    Text,
    Plugin,
    /// A spoken answer to a question heard in an interview held in
    /// Alexandria, timed from the end of the question to the first speech.
    /// A heard question has no reading floor: answering at once is turn-taking.
    Voice,
}

/// Response timing for the items the learner worked on in one window.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResponseFeatures {
    /// Item visits that ended in this window. A visit ends when the learner
    /// turns to another item or the attempt ends, so each appears once.
    pub items: Vec<ItemResponse>,
    /// The learner's response baseline; `None` until there is one.
    pub baseline: Option<ResponseBaseline>,
    /// Response events that followed a gaze-down within the cross-modal
    /// window (a few seconds).
    pub gaze_down_then_response: u32,
    /// Response events that followed a second-voice event within the
    /// cross-modal window.
    pub voice_then_response: u32,
}

/// One visit to an item, measured from when it became available to read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ItemResponse {
    pub item_id: String,
    pub input: InputKind,
    /// Characters the learner had to read: the prompt plus any options.
    pub chars: u32,
    /// Bank difficulty, 1 (easiest) to 5; `None` when the item has none.
    pub difficulty: Option<u8>,
    /// A return to an item already visited. Its latency says nothing about
    /// reading, so it is never judged against the floor or the baseline.
    pub revisit: bool,
    /// From presentation to the first input.
    pub first_input_ms: f64,
    /// From presentation to the last input of the visit.
    pub commit_ms: f64,
    /// Inputs that changed an answer already given.
    pub revisions: u32,
    /// Times the learner sat idle and then produced the answer in a burst.
    pub idle_bursts: u32,
    /// Answer changes made straight after an idle stretch.
    pub changes_after_idle: u32,
}

/// Where a baseline comes from: earlier monitored sessions on this device, or
/// the first items of this attempt when there is no history yet.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineSource {
    #[default]
    Attempt,
    History,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResponseBaseline {
    pub source: BaselineSource,
    /// The first-visit timings the baseline is made of.
    pub items: Vec<BaselineItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BaselineItem {
    pub chars: u32,
    pub difficulty: Option<u8>,
    pub first_input_ms: f64,
}

/// The shortest time in which an item of `chars` characters at `difficulty`
/// can have been read. An item without a difficulty is treated as the
/// easiest, so a missing value never raises the floor.
pub fn reading_floor_ms(chars: u32, difficulty: Option<u8>) -> f64 {
    let level = f64::from(difficulty.unwrap_or(1).clamp(1, 5) - 1);
    let factor = 1.0 + READING_FLOOR_DIFFICULTY_STEP * level;
    (READING_FLOOR_BASE_MS + f64::from(chars) * READING_FLOOR_MS_PER_CHAR) * factor
}

/// Time to first input beyond what reading required.
fn excess_ms(chars: u32, difficulty: Option<u8>, first_input_ms: f64) -> f64 {
    first_input_ms - reading_floor_ms(chars, difficulty)
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

impl ResponseFeatures {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let baseline = self.baseline.as_ref().map_or(&[][..], |b| &b.items[..]);
        if self.items.len() > MAX_ITEMS || baseline.len() > MAX_BASELINE_ITEMS {
            return Err("response features carry too many items".into());
        }
        let difficulties = self
            .items
            .iter()
            .map(|i| i.difficulty)
            .chain(baseline.iter().map(|b| b.difficulty));
        if difficulties.flatten().any(|d| !(1..=5).contains(&d)) {
            return Err("item difficulty must be between 1 and 5".into());
        }
        let timings = self
            .items
            .iter()
            .flat_map(|i| [i.first_input_ms, i.commit_ms])
            .chain(baseline.iter().map(|b| b.first_input_ms));
        for value in timings {
            if !value.is_finite() || value < 0.0 {
                return Err("response timings must be finite, non-negative durations".into());
            }
        }
        for item in &self.items {
            if item.item_id.is_empty() || item.item_id.len() > MAX_ITEM_ID_LEN {
                return Err("response items need a short item id".into());
            }
            if item.commit_ms < item.first_input_ms || item.changes_after_idle > item.revisions {
                return Err("response timings are inconsistent".into());
            }
        }
        Ok(())
    }

    /// First visits: the only ones that say anything about reading.
    fn first_visits(&self) -> impl Iterator<Item = &ItemResponse> {
        self.items.iter().filter(|i| !i.revisit)
    }

    /// Median excess over the reading floor across the baseline items.
    fn baseline_excess_ms(&self) -> Option<f64> {
        let items = &self.baseline.as_ref()?.items;
        if items.len() < BASELINE_MIN_ITEMS {
            return None;
        }
        median(
            items
                .iter()
                .map(|b| excess_ms(b.chars, b.difficulty, b.first_input_ms))
                .collect(),
        )
    }

    /// Relay lag: every item in the window waited about the same extra time
    /// over the learner's baseline, as when someone else reads the question
    /// and passes the answer back.
    fn relay_lag(&self) -> bool {
        let Some(baseline) = self.baseline_excess_ms() else {
            return false;
        };
        let added: Vec<f64> = self
            .first_visits()
            .map(|i| excess_ms(i.chars, i.difficulty, i.first_input_ms) - baseline)
            .collect();
        if added.len() < RELAY_MIN_ITEMS || added.iter().any(|a| *a < RELAY_MIN_ADDED_MS) {
            return false;
        }
        let mean = added.iter().sum::<f64>() / added.len() as f64;
        let spread = added.iter().copied().fold(f64::MIN, f64::max)
            - added.iter().copied().fold(f64::MAX, f64::min);
        spread <= RELAY_UNIFORM_SPREAD * mean
    }

    /// The window's response-process flags, in [`RESPONSE_PROCESS_SIGNALS`]
    /// order.
    pub(crate) fn flags(&self) -> Vec<&'static str> {
        let mut flags = Vec::new();
        if self.first_visits().any(|i| {
            i.input != InputKind::Voice
                && i.first_input_ms < reading_floor_ms(i.chars, i.difficulty)
        }) {
            flags.push("response_below_reading_floor");
        }
        if self.relay_lag() {
            flags.push("response_relay_lag");
        }
        if self.items.iter().any(|i| i.idle_bursts > 0) {
            flags.push("response_idle_burst");
        }
        if self.items.iter().any(|i| i.changes_after_idle > 0) {
            flags.push("answer_change_after_idle");
        }
        if self.gaze_down_then_response > 0 {
            flags.push("gaze_down_then_response");
        }
        if self.voice_then_response > 0 {
            flags.push("voice_then_response");
        }
        flags
    }

    /// First visits observed in this window: the session's response-process
    /// coverage.
    pub fn observed_items(&self) -> u32 {
        self.first_visits().count() as u32
    }
}
