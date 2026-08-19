//! Attempt policy: how often a learner may take a credential-bearing
//! assessment.
//!
//! # The hole this closes
//!
//! A credential is issued only when an attempt passes, so a skill's
//! aggregated `raw_score` is a weighted mean of *successes only* — failures
//! leave no trace. Combined with unlimited attempts and a fresh random seed
//! per attempt, a learner can simply re-roll until a favourable draw arrives
//! and bank the one result that counts. That makes a passing score a
//! statement about persistence rather than capability, which is precisely
//! what an employer is being asked to trust.
//!
//! # Learning stays free
//!
//! This gates *credential-bearing attempts*, never study. The escalating
//! cooldown is deliberately not a lockout: waiting always eventually
//! restores an attempt, and the window below means a learner who comes back
//! months later starts fresh. Alexandria's promise is that learning is free
//! and unlimited; the credential is the scarce thing, and it should be.
//!
//! # Why a cooldown rather than a cap
//!
//! A hard cap punishes the learner who genuinely needs six tries, and it
//! cannot distinguish them from someone farming draws. A cooldown prices
//! re-rolling in the one currency a farmer actually minds — time — while
//! leaving the honest path open. Recording which attempt succeeded (see
//! `attempt_ordinal` on the issued claim) then makes the difference legible
//! to whoever reads the credential, rather than hiding it.

use serde::{Deserialize, Serialize};

/// One prior attempt, oldest-first ordering not required.
#[derive(Debug, Clone)]
pub struct AttemptRecord {
    /// When the attempt began, RFC 3339 or SQLite `datetime()` format.
    pub started_at: String,
    /// `None` while an attempt is still open.
    pub graded_at: Option<String>,
    /// `None` if never graded.
    pub passed: Option<bool>,
}

/// How an attempt history collapses into the score that reaches aggregation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScorePolicy {
    /// Highest graded score. The historical behaviour — every pass issues a
    /// credential and aggregation keeps the best evidence.
    #[default]
    Best,
    /// Most recent graded score, so a learner's current standing can fall.
    Latest,
}

impl ScorePolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScorePolicy::Best => "best",
            ScorePolicy::Latest => "latest",
        }
    }

    pub fn parse_lenient(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "latest" => ScorePolicy::Latest,
            _ => ScorePolicy::Best,
        }
    }
}

/// Per-bank attempt rules.
#[derive(Debug, Clone, PartialEq)]
pub struct AttemptPolicy {
    /// Hard ceiling within the window. `None` means unlimited, which is the
    /// default — cooldowns do the work, so nobody is permanently locked out
    /// of a skill.
    pub max_attempts: Option<u32>,
    /// Wait required *before* the nth attempt, indexed by how many attempts
    /// have already been used. The last entry repeats for every attempt
    /// beyond its length, so the escalation plateaus rather than growing
    /// without bound.
    pub cooldown_hours: Vec<u32>,
    /// Only attempts started within this many days count toward the cooldown
    /// escalation and `max_attempts`. `None` counts all history.
    pub attempt_window_days: Option<u32>,
    pub score_policy: ScorePolicy,
}

impl Default for AttemptPolicy {
    /// First attempt immediate, then 24h, 72h, then a week thereafter, over
    /// a 90-day window and with no hard cap.
    ///
    /// The first retry is same-day-ish on purpose: an honest learner who
    /// misread a question should not wait a week. The escalation bites the
    /// pattern that actually indicates farming — many attempts in quick
    /// succession.
    fn default() -> Self {
        Self {
            max_attempts: None,
            cooldown_hours: vec![0, 24, 72, 168],
            attempt_window_days: Some(90),
            score_policy: ScorePolicy::Best,
        }
    }
}

/// The answer to "may this learner start an attempt right now".
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyDecision {
    Allow {
        /// 1-based index of the attempt about to start. Recorded on the
        /// issued claim so a reader can see "passed on attempt 7".
        ordinal: u32,
    },
    Cooldown {
        /// RFC 3339 instant at which an attempt becomes available.
        until: String,
        attempts_used: u32,
    },
    Exhausted {
        attempts_used: u32,
        max: u32,
    },
}

impl PolicyDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, PolicyDecision::Allow { .. })
    }

    /// Human-readable refusal, matching the `Result<_, String>` idiom used
    /// across `commands/`.
    pub fn refusal(&self) -> Option<String> {
        match self {
            PolicyDecision::Allow { .. } => None,
            PolicyDecision::Cooldown {
                until,
                attempts_used,
            } => Some(format!(
                "assessment cooldown active after {attempts_used} attempt(s) — \
                 next attempt available at {until}"
            )),
            PolicyDecision::Exhausted { attempts_used, max } => Some(format!(
                "attempt limit reached: {attempts_used} of {max} used for this assessment"
            )),
        }
    }
}

/// Decide whether an attempt may start.
///
/// Pure and total: unparseable timestamps are treated as outside the window
/// rather than erroring, because a malformed row must not be able to lock a
/// learner out of a skill permanently.
pub fn evaluate_attempt_policy(
    history: &[AttemptRecord],
    policy: &AttemptPolicy,
    now: &str,
) -> PolicyDecision {
    let Some(now_ts) = parse_time(now) else {
        // Without a usable clock reading we cannot compute a cooldown.
        // Allowing is the safe direction: the failure mode is one extra
        // attempt, not a learner locked out by a clock bug.
        return PolicyDecision::Allow {
            ordinal: history.len() as u32 + 1,
        };
    };

    let window_start = policy
        .attempt_window_days
        .map(|d| now_ts - chrono::Duration::days(d as i64));

    // Attempts inside the window, most recent first.
    let mut in_window: Vec<(chrono::DateTime<chrono::Utc>, &AttemptRecord)> = history
        .iter()
        .filter_map(|a| parse_time(&a.started_at).map(|t| (t, a)))
        .filter(|(t, _)| window_start.is_none_or(|w| *t >= w))
        .collect();
    in_window.sort_by_key(|(started, _)| std::cmp::Reverse(*started));

    let attempts_used = in_window.len() as u32;

    if let Some(max) = policy.max_attempts {
        if attempts_used >= max {
            return PolicyDecision::Exhausted { attempts_used, max };
        }
    }

    // An attempt that was started but never graded is still open; the
    // learner should finish it rather than be handed another.
    if let Some((_, open)) = in_window.iter().find(|(_, a)| a.graded_at.is_none()) {
        let _ = open;
        return PolicyDecision::Allow {
            ordinal: attempts_used,
        };
    }

    let Some((last_started, _)) = in_window.first() else {
        return PolicyDecision::Allow { ordinal: 1 };
    };

    let wait_hours = cooldown_for(attempts_used, &policy.cooldown_hours);
    if wait_hours == 0 {
        return PolicyDecision::Allow {
            ordinal: attempts_used + 1,
        };
    }

    let available_at = *last_started + chrono::Duration::hours(wait_hours as i64);
    if now_ts >= available_at {
        PolicyDecision::Allow {
            ordinal: attempts_used + 1,
        }
    } else {
        PolicyDecision::Cooldown {
            until: available_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            attempts_used,
        }
    }
}

/// Cooldown required before the attempt following `attempts_used` prior
/// ones. Beyond the table's length the last entry repeats.
fn cooldown_for(attempts_used: u32, table: &[u32]) -> u32 {
    if table.is_empty() {
        return 0;
    }
    let idx = (attempts_used as usize).min(table.len() - 1);
    table[idx]
}

/// Parse the two timestamp shapes this database holds: RFC 3339 written by
/// `now_rfc3339`, and SQLite's `datetime('now')` default on older rows.
fn parse_time(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(t.with_timezone(&chrono::Utc));
    }
    chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|n| n.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-07-22T12:00:00Z";

    fn attempt(started_at: &str, passed: Option<bool>) -> AttemptRecord {
        AttemptRecord {
            started_at: started_at.to_string(),
            graded_at: Some(started_at.to_string()),
            passed,
        }
    }

    fn hours_before(h: i64) -> String {
        (chrono::DateTime::parse_from_rfc3339(NOW).unwrap() - chrono::Duration::hours(h))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }

    #[test]
    fn a_first_attempt_is_always_allowed() {
        let d = evaluate_attempt_policy(&[], &AttemptPolicy::default(), NOW);
        assert_eq!(d, PolicyDecision::Allow { ordinal: 1 });
    }

    #[test]
    fn a_second_attempt_waits_out_the_first_cooldown() {
        // Default table is [0, 24, ...]: one attempt used means 24h.
        let history = [attempt(&hours_before(1), Some(false))];
        let d = evaluate_attempt_policy(&history, &AttemptPolicy::default(), NOW);
        assert!(matches!(d, PolicyDecision::Cooldown { .. }), "got {d:?}");
        assert!(d.refusal().unwrap().contains("cooldown"));
    }

    #[test]
    fn the_cooldown_expires() {
        let history = [attempt(&hours_before(25), Some(false))];
        let d = evaluate_attempt_policy(&history, &AttemptPolicy::default(), NOW);
        assert_eq!(d, PolicyDecision::Allow { ordinal: 2 });
    }

    #[test]
    fn cooldowns_escalate_with_each_attempt() {
        let policy = AttemptPolicy::default();

        // Two used → 72h required. At 30h still cooling, at 80h allowed.
        let two = [
            attempt(&hours_before(80), Some(false)),
            attempt(&hours_before(30), Some(false)),
        ];
        assert!(matches!(
            evaluate_attempt_policy(&two, &policy, NOW),
            PolicyDecision::Cooldown { .. }
        ));

        let two_old = [
            attempt(&hours_before(200), Some(false)),
            attempt(&hours_before(80), Some(false)),
        ];
        assert!(evaluate_attempt_policy(&two_old, &policy, NOW).is_allowed());
    }

    #[test]
    fn escalation_plateaus_rather_than_growing_forever() {
        // Ten prior attempts must not demand a decade; the table's last
        // entry (168h) repeats.
        let policy = AttemptPolicy::default();
        let history: Vec<AttemptRecord> = (0..10)
            .map(|i| attempt(&hours_before(169 + i * 200), Some(false)))
            .collect();
        assert!(
            evaluate_attempt_policy(&history, &policy, NOW).is_allowed(),
            "a week past the last attempt should always be enough"
        );
    }

    #[test]
    fn attempts_outside_the_window_stop_counting() {
        // A learner returning after months starts fresh — the point of the
        // window is that this is never a permanent lockout.
        let policy = AttemptPolicy::default();
        let history: Vec<AttemptRecord> = (0..5)
            .map(|i| attempt(&hours_before(24 * 200 + i), Some(false)))
            .collect();
        assert_eq!(
            evaluate_attempt_policy(&history, &policy, NOW),
            PolicyDecision::Allow { ordinal: 1 },
            "stale attempts should not count toward escalation"
        );
    }

    #[test]
    fn a_hard_cap_is_reported_as_exhausted() {
        let policy = AttemptPolicy {
            max_attempts: Some(2),
            ..AttemptPolicy::default()
        };
        let history = [
            attempt(&hours_before(500), Some(false)),
            attempt(&hours_before(400), Some(false)),
        ];
        let d = evaluate_attempt_policy(&history, &policy, NOW);
        assert_eq!(
            d,
            PolicyDecision::Exhausted {
                attempts_used: 2,
                max: 2
            }
        );
        assert!(d.refusal().unwrap().contains("limit reached"));
    }

    #[test]
    fn unlimited_is_the_default_so_nobody_is_locked_out_of_a_skill() {
        let policy = AttemptPolicy::default();
        assert!(policy.max_attempts.is_none());
        let history: Vec<AttemptRecord> = (0..50)
            .map(|i| attempt(&hours_before(200 + i), Some(false)))
            .collect();
        assert!(evaluate_attempt_policy(&history, &policy, NOW).is_allowed());
    }

    #[test]
    fn an_ungraded_attempt_is_resumed_rather_than_replaced() {
        // Handing out a second attempt while one is open would be its own
        // re-roll: start two, submit the better one.
        let policy = AttemptPolicy::default();
        let history = [AttemptRecord {
            started_at: hours_before(1),
            graded_at: None,
            passed: None,
        }];
        let d = evaluate_attempt_policy(&history, &policy, NOW);
        assert!(
            d.is_allowed(),
            "an open attempt should not be blocked: {d:?}"
        );
    }

    #[test]
    fn passing_does_not_bar_a_retake() {
        // Improving a score is legitimate; the cooldown still applies.
        let policy = AttemptPolicy::default();
        let history = [attempt(&hours_before(500), Some(true))];
        assert!(evaluate_attempt_policy(&history, &policy, NOW).is_allowed());
    }

    #[test]
    fn ordinal_counts_the_attempt_about_to_start() {
        let policy = AttemptPolicy::default();
        let history = [
            attempt(&hours_before(900), Some(false)),
            attempt(&hours_before(800), Some(false)),
        ];
        assert_eq!(
            evaluate_attempt_policy(&history, &policy, NOW),
            PolicyDecision::Allow { ordinal: 3 }
        );
    }

    #[test]
    fn sqlite_datetime_rows_parse() {
        // Rows written by the column default rather than `now_rfc3339`.
        let history = [attempt("2026-07-22 11:00:00", Some(false))];
        let d = evaluate_attempt_policy(&history, &AttemptPolicy::default(), NOW);
        assert!(
            matches!(d, PolicyDecision::Cooldown { .. }),
            "legacy timestamp should still enforce a cooldown, got {d:?}"
        );
    }

    #[test]
    fn unparseable_history_does_not_lock_a_learner_out() {
        // A malformed row must fail open. The cost is one extra attempt;
        // the cost of failing closed is a learner permanently unable to
        // evidence a skill.
        let history = [attempt("not a timestamp", Some(false))];
        assert!(evaluate_attempt_policy(&history, &AttemptPolicy::default(), NOW).is_allowed());
    }

    #[test]
    fn an_unparseable_clock_fails_open() {
        assert!(evaluate_attempt_policy(
            &[attempt(&hours_before(1), Some(false))],
            &AttemptPolicy::default(),
            "nonsense"
        )
        .is_allowed());
    }

    #[test]
    fn an_empty_cooldown_table_means_no_waiting() {
        let policy = AttemptPolicy {
            cooldown_hours: vec![],
            ..AttemptPolicy::default()
        };
        let history = [attempt(&hours_before(1), Some(false))];
        assert!(evaluate_attempt_policy(&history, &policy, NOW).is_allowed());
    }

    #[test]
    fn score_policy_round_trips() {
        for p in [ScorePolicy::Best, ScorePolicy::Latest] {
            assert_eq!(ScorePolicy::parse_lenient(p.as_str()), p);
        }
        assert_eq!(ScorePolicy::parse_lenient("nonsense"), ScorePolicy::Best);
        assert_eq!(ScorePolicy::default(), ScorePolicy::Best);
    }

    #[test]
    fn cooldown_table_indexing_is_bounded() {
        assert_eq!(cooldown_for(0, &[0, 24, 72]), 0);
        assert_eq!(cooldown_for(1, &[0, 24, 72]), 24);
        assert_eq!(cooldown_for(2, &[0, 24, 72]), 72);
        assert_eq!(cooldown_for(99, &[0, 24, 72]), 72, "last entry repeats");
        assert_eq!(cooldown_for(3, &[]), 0);
    }
}
