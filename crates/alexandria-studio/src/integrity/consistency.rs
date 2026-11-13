//! Longitudinal consistency: does an attempt's outcome agree with what the
//! learner's own history on the skill predicts?
//!
//! The history is read from the profile database by the app; this module
//! only turns it into a verdict, so the app, the offline evaluator and tests
//! share one rule.

use serde::{Deserialize, Serialize};

/// Recency-weighted outcomes needed before a verdict other than
/// [`ConsistencyVerdict::InsufficientHistory`]: about four recent answers.
pub const MIN_EVIDENCE_WEIGHT: f64 = 4.0;

/// An observation's weight halves every fortnight, so recent practice
/// dominates and old history fades out.
pub const HALF_LIFE_HOURS: f64 = 14.0 * 24.0;

/// History older than this is not read at all; it matches the default
/// attempt window of a question bank.
pub const LOOKBACK_HOURS: f64 = 90.0 * 24.0;

/// How far the outcome may exceed the expected score before it diverges.
/// Only an outcome above expectation diverges: doing worse than practice
/// suggests is not evidence of help.
pub const DIVERGENCE_MARGIN: f64 = 0.4;

/// Expected-success weight one recent completed lesson adds.
pub const ENGAGEMENT_WEIGHT: f64 = 0.25;

/// Most expected-success weight lessons can add in total.
pub const ENGAGEMENT_CAP: f64 = 2.0;

/// The fusion signal a divergent attempt raises. Advisory until the
/// certified manifest gives it a ratio.
pub const DIVERGENCE_SIGNAL: &str = "longitudinal_divergence";

/// Where an observation came from.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    /// A practice question's first answer, given without a hint or reveal.
    Practice,
    /// A scored lesson element (a quiz) on the skill.
    Lesson,
    /// A graded item from an earlier attempt on the skill.
    PriorAttempt,
}

/// One outcome from before the attempt started.
#[derive(Clone, Debug, PartialEq)]
pub struct Observation {
    pub source: EvidenceSource,
    /// In `[0, 1]`.
    pub outcome: f64,
    /// Hours between the observation and the attempt's start.
    pub age_hours: f64,
}

/// A learner's history on one skill, all of it from before the attempt.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    pub observations: Vec<Observation>,
    /// Age in hours of each completed, unscored lesson on the skill.
    pub lessons_completed: Vec<f64>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsistencyVerdict {
    /// Enough recent history, and the outcome does not outrun it.
    Satisfied,
    /// Too little recent history to say. Never counts against the learner.
    InsufficientHistory,
    /// The outcome is far above what recent history predicts.
    Divergent,
}

/// The verdict and the numbers it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Consistency {
    pub verdict: ConsistencyVerdict,
    /// Score the history predicts, in `[0, 1]`.
    pub expected: f64,
    /// The attempt outcome compared, in `[0, 1]`.
    pub outcome: f64,
    /// Recency-weighted number of outcomes behind `expected`.
    pub evidence_weight: f64,
}

impl Consistency {
    /// Whether the `consistent` verification rung is satisfied.
    pub fn satisfied(&self) -> bool {
        self.verdict == ConsistencyVerdict::Satisfied
    }

    /// The fusion signals this result raises: [`DIVERGENCE_SIGNAL`] when
    /// divergent, otherwise none.
    pub fn signals(&self) -> Vec<String> {
        match self.verdict {
            ConsistencyVerdict::Divergent => vec![DIVERGENCE_SIGNAL.to_string()],
            _ => Vec::new(),
        }
    }
}

/// Compares an attempt outcome (in `[0, 1]`) with the learner's history.
///
/// The expected score is the recency-weighted mean of the history's
/// outcomes under a uniform prior (one success, one failure). Completed
/// lessons add a little expected success but no evidence weight: studying
/// earns the benefit of the doubt, it does not prove anything on its own.
pub fn assess(history: &History, outcome: f64) -> Consistency {
    let outcome = if outcome.is_finite() {
        outcome.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut evidence_weight = 0.0;
    let mut successes = 0.0;
    for o in &history.observations {
        let w = recency(o.age_hours);
        let value = if o.outcome.is_finite() {
            o.outcome.clamp(0.0, 1.0)
        } else {
            0.0
        };
        evidence_weight += w;
        successes += w * value;
    }
    let engagement = (history
        .lessons_completed
        .iter()
        .map(|age| recency(*age))
        .sum::<f64>()
        * ENGAGEMENT_WEIGHT)
        .min(ENGAGEMENT_CAP);
    let expected = (1.0 + successes + engagement) / (2.0 + evidence_weight + engagement);
    let verdict = if evidence_weight < MIN_EVIDENCE_WEIGHT {
        ConsistencyVerdict::InsufficientHistory
    } else if outcome - expected > DIVERGENCE_MARGIN {
        ConsistencyVerdict::Divergent
    } else {
        ConsistencyVerdict::Satisfied
    };
    Consistency {
        verdict,
        expected,
        outcome,
        evidence_weight,
    }
}

/// Weight an observation of this age carries: halves every
/// [`HALF_LIFE_HOURS`], zero outside the lookback or in the future.
fn recency(age_hours: f64) -> f64 {
    if !(age_hours > 0.0 && age_hours <= LOOKBACK_HOURS) {
        return 0.0;
    }
    0.5f64.powf(age_hours / HALF_LIFE_HOURS)
}
