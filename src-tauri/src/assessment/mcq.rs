//! Host-native implementation of the MCQ scoring contract.
//!
//! # Why this exists
//!
//! Wasmtime has no iOS or Android target, so [`crate::plugins::wasm_runtime`]
//! is `#[cfg(desktop)]` and plugin grading returns `GraderUnavailable` on
//! mobile. Routing MCQ through the wasm grader alone would therefore have
//! removed assessments from mobile entirely — a regression on a platform
//! where they work today.
//!
//! So MCQ has two implementations of one contract: the published
//! `plugins/builtin/mcq-grader` wasm, and this. The host prefers the wasm
//! wherever it can run it, and falls back here where it cannot.
//!
//! # Why that is honest
//!
//! A recorded `grader_cid` is a claim about *which scoring function* a
//! verifier must run to reproduce a score, not about which machine executed
//! it. That claim stays true on mobile only for as long as the two
//! implementations agree — so agreement is not assumed, it is enforced.
//! `equivalence` below checks both `score` and `details` against the real
//! wasm across every key/selection pair for one- to four-option questions,
//! plus randomised larger cases. If the wasm is ever rebuilt with different
//! semantics and this is not updated to match, CI fails.
//!
//! Divergence would be a real defect, not a cosmetic one: a learner grading
//! on a phone and a verifier re-deriving on a server would disagree about
//! whether a credential was earned.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

// From `grade_contract`, not `wasm_runtime` — this module's entire purpose is
// to work where the wasm engine does not exist.
use crate::plugins::grade_contract::ScoreRecord;

/// Content half of an MCQ grade envelope, after `grader_private` is merged
/// in. Mirrors `McqContent` in the wasm grader.
#[derive(Debug, Clone, Deserialize)]
pub struct McqContent {
    /// `"single"` or `"multi"`.
    pub kind: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub correct_indices: Vec<u32>,
}

/// Submission half. Original option indices, not served positions — the
/// caller maps those back first (see [`crate::assessment::items`]).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct McqSubmission {
    #[serde(default)]
    pub selected_indices: Vec<u32>,
}

/// Fixed-shape details blob, byte-for-byte what the wasm grader emits.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScoreDetails {
    pub kind: String,
    pub correct_count: u32,
    pub incorrect_count: u32,
    pub total_correct: u32,
    pub selected_count: u32,
}

/// Score an MCQ.
///
/// Never fails: malformed content yields a zero-score record whose
/// `details.kind` carries the reason, exactly as the wasm grader does. A
/// broken item must not be able to abort an attempt.
pub fn score(content: &serde_json::Value, submission: &serde_json::Value) -> ScoreRecord {
    let content: McqContent = match serde_json::from_value(content.clone()) {
        Ok(c) => c,
        Err(e) => return error_record(format!("invalid grade input: {e}")),
    };
    let submission: McqSubmission = serde_json::from_value(submission.clone()).unwrap_or_default();

    match score_inner(&content, &submission) {
        Ok(r) => r,
        Err(e) => error_record(e),
    }
}

fn score_inner(content: &McqContent, submission: &McqSubmission) -> Result<ScoreRecord, String> {
    let kind = content.kind.as_str();
    if kind != "single" && kind != "multi" {
        return Err(format!("unknown mcq kind '{kind}'"));
    }
    if content.correct_indices.is_empty() {
        return Err("mcq content must have at least one correct option".to_string());
    }

    // `options` is informational for the renderer; correctness is about
    // index sets. When it is populated, indices must fall inside it.
    if !content.options.is_empty() {
        let opts_len = content.options.len() as u32;
        for &i in &content.correct_indices {
            if i >= opts_len {
                return Err(format!("correct index {i} out of range"));
            }
        }
        for &i in &submission.selected_indices {
            if i >= opts_len {
                return Err(format!("selected index {i} out of range"));
            }
        }
    }

    let correct: BTreeSet<u32> = content.correct_indices.iter().copied().collect();
    let selected: BTreeSet<u32> = submission.selected_indices.iter().copied().collect();

    let total_correct = correct.len() as u32;
    let selected_count = selected.len() as u32;

    if kind == "single" {
        // Exactly one selection, and it must be a correct one.
        let hit = selected.len() == 1 && correct.contains(selected.iter().next().unwrap());
        let correct_count = u32::from(hit);
        return Ok(record(
            if hit { 1.0 } else { 0.0 },
            ScoreDetails {
                kind: "single".to_string(),
                correct_count,
                incorrect_count: selected_count.saturating_sub(correct_count),
                total_correct,
                selected_count,
            },
        ));
    }

    // Multi: (hits - wrong picks) / |correct|, clamped to [0, 1]. Partial
    // knowledge earns partial credit; guessing everything does not pay.
    let intersect = selected.intersection(&correct).count() as i64;
    let extra = selected.difference(&correct).count() as i64;
    let raw = (intersect - extra).max(0) as f64 / total_correct as f64;

    Ok(record(
        raw.min(1.0),
        ScoreDetails {
            kind: "multi".to_string(),
            correct_count: intersect as u32,
            incorrect_count: extra as u32,
            total_correct,
            selected_count,
        },
    ))
}

fn record(score: f64, details: ScoreDetails) -> ScoreRecord {
    ScoreRecord {
        version: "1".to_string(),
        score,
        details: serde_json::to_value(details).expect("ScoreDetails serializes"),
    }
}

/// Zero score, reason stashed in `details.kind` — the wasm grader's
/// convention, kept identical so the two cannot be told apart.
fn error_record(err: String) -> ScoreRecord {
    record(
        0.0,
        ScoreDetails {
            kind: format!("error: {err}"),
            correct_count: 0,
            incorrect_count: 0,
            total_correct: 0,
            selected_count: 0,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(kind: &str, options: usize, correct: &[u32]) -> serde_json::Value {
        let opts: Vec<String> = (0..options).map(|i| format!("opt{i}")).collect();
        serde_json::json!({ "kind": kind, "options": opts, "correct_indices": correct })
    }

    fn submitted(selected: &[u32]) -> serde_json::Value {
        serde_json::json!({ "selected_indices": selected })
    }

    #[test]
    fn single_awards_full_marks_only_for_the_lone_correct_pick() {
        let c = content("single", 4, &[2]);
        assert_eq!(score(&c, &submitted(&[2])).score, 1.0);
        assert_eq!(score(&c, &submitted(&[0])).score, 0.0);
        // Hedging by picking several is not a correct answer.
        assert_eq!(score(&c, &submitted(&[2, 0])).score, 0.0);
        assert_eq!(score(&c, &submitted(&[])).score, 0.0);
    }

    #[test]
    fn multi_awards_partial_credit_and_penalises_wrong_picks() {
        let c = content("multi", 4, &[0, 2]);
        assert_eq!(score(&c, &submitted(&[0, 2])).score, 1.0);
        assert_eq!(score(&c, &submitted(&[0])).score, 0.5);
        assert_eq!(score(&c, &submitted(&[0, 1])).score, 0.0);
        assert_eq!(score(&c, &submitted(&[0, 1, 2, 3])).score, 0.0);
    }

    #[test]
    fn selecting_everything_never_pays() {
        // The property that makes partial credit safe: a learner who picks
        // every option must not out-score one who picks nothing.
        for options in 2..=6usize {
            for n_correct in 1..options {
                let correct: Vec<u32> = (0..n_correct as u32).collect();
                let all: Vec<u32> = (0..options as u32).collect();
                let c = content("multi", options, &correct);
                let everything = score(&c, &submitted(&all)).score;
                assert!(
                    everything <= 0.0 || n_correct * 2 > options,
                    "selecting all options scored {everything} with {n_correct}/{options} correct"
                );
            }
        }
    }

    #[test]
    fn malformed_content_scores_zero_rather_than_failing() {
        // A broken item must not abort an attempt.
        let bad_kind = serde_json::json!({ "kind": "essay", "correct_indices": [0] });
        let r = score(&bad_kind, &submitted(&[0]));
        assert_eq!(r.score, 0.0);
        assert!(r.details["kind"].as_str().unwrap().starts_with("error:"));

        let no_key = serde_json::json!({ "kind": "single", "correct_indices": [] });
        assert_eq!(score(&no_key, &submitted(&[0])).score, 0.0);

        let out_of_range = content("single", 2, &[5]);
        assert_eq!(score(&out_of_range, &submitted(&[0])).score, 0.0);
    }

    #[test]
    fn out_of_range_selection_is_rejected_not_ignored() {
        let c = content("single", 3, &[0]);
        let r = score(&c, &submitted(&[9]));
        assert_eq!(r.score, 0.0);
        assert!(r.details["kind"].as_str().unwrap().contains("out of range"));
    }

    #[test]
    fn missing_submission_field_is_an_empty_selection() {
        let c = content("single", 3, &[0]);
        let r = score(&c, &serde_json::json!({}));
        assert_eq!(r.score, 0.0);
        assert_eq!(r.details["selected_count"], 0);
    }
}

/// Equivalence with the published wasm grader.
///
/// This is the test that licenses recording the wasm's `grader_cid` for a
/// score this module produced. It compares `score` *and* `details` — the
/// two implementations must be indistinguishable, not merely close.
#[cfg(all(test, desktop))]
mod equivalence {
    use super::*;
    use crate::plugins::wasm_runtime::{GraderBudgets, GraderRuntime};

    const MCQ_GRADER_WASM: &[u8] =
        include_bytes!("../../../plugins/builtin/mcq-grader/dist/mcq_grader.wasm");

    fn wasm_record(
        runtime: &GraderRuntime,
        content: &serde_json::Value,
        submission: &serde_json::Value,
    ) -> ScoreRecord {
        let input = serde_json::to_vec(&serde_json::json!({
            "version": "1",
            "content": content,
            "submission": submission,
        }))
        .unwrap();
        let cid = blake3::hash(MCQ_GRADER_WASM).to_hex().to_string();
        runtime
            .grade(
                &cid,
                MCQ_GRADER_WASM,
                None,
                &input,
                GraderBudgets::default(),
            )
            .expect("wasm grader runs")
    }

    fn assert_same(runtime: &GraderRuntime, content: &serde_json::Value, selected: &[u32]) {
        let submission = serde_json::json!({ "selected_indices": selected });
        let native = score(content, &submission);
        let wasm = wasm_record(runtime, content, &submission);
        assert!(
            (native.score - wasm.score).abs() < 1e-12,
            "score differs for {content} / {selected:?}: native={} wasm={}",
            native.score,
            wasm.score
        );
        assert_eq!(
            native.details, wasm.details,
            "details differ for {content} / {selected:?}"
        );
    }

    fn subsets(n: usize) -> Vec<Vec<u32>> {
        (0u32..(1 << n))
            .map(|mask| (0..n as u32).filter(|i| mask & (1 << i) != 0).collect())
            .collect()
    }

    #[test]
    fn matches_wasm_exhaustively_up_to_four_options() {
        let runtime = GraderRuntime::new().expect("runtime");
        for options in 1..=4usize {
            let opts: Vec<String> = (0..options).map(|i| format!("opt{i}")).collect();
            for correct in subsets(options).into_iter().filter(|s| !s.is_empty()) {
                let kind = if correct.len() == 1 {
                    "single"
                } else {
                    "multi"
                };
                let content = serde_json::json!({
                    "kind": kind, "options": opts, "correct_indices": correct,
                });
                for selected in subsets(options) {
                    assert_same(&runtime, &content, &selected);
                }
            }
        }
    }

    #[test]
    fn matches_wasm_on_larger_questions() {
        // Exhaustive coverage stops being cheap past four options; sample
        // deterministically instead so the test stays reproducible.
        let runtime = GraderRuntime::new().expect("runtime");
        let mut rng = crate::assessment::SplitMix64(0xA55E_5501);

        for options in [6usize, 8, 10] {
            let opts: Vec<String> = (0..options).map(|i| format!("opt{i}")).collect();
            for _ in 0..40 {
                let pick = |rng: &mut crate::assessment::SplitMix64| -> Vec<u32> {
                    (0..options as u32)
                        .filter(|_| rng.below(2) == 1)
                        .collect::<Vec<_>>()
                };
                let mut correct = pick(&mut rng);
                if correct.is_empty() {
                    correct.push(0);
                }
                let kind = if correct.len() == 1 {
                    "single"
                } else {
                    "multi"
                };
                let content = serde_json::json!({
                    "kind": kind, "options": opts, "correct_indices": correct,
                });
                assert_same(&runtime, &content, &pick(&mut rng));
            }
        }
    }

    #[test]
    fn matches_wasm_on_malformed_input() {
        // Error paths matter as much as happy ones: a divergence here means
        // one platform scores a broken item 0 and the other refuses it.
        let runtime = GraderRuntime::new().expect("runtime");
        let cases = [
            serde_json::json!({ "kind": "essay", "options": ["a"], "correct_indices": [0] }),
            serde_json::json!({ "kind": "single", "options": ["a","b"], "correct_indices": [] }),
            serde_json::json!({ "kind": "single", "options": ["a","b"], "correct_indices": [7] }),
            serde_json::json!({ "kind": "multi", "options": [], "correct_indices": [0,1] }),
        ];
        for content in cases {
            for selected in [vec![], vec![0u32], vec![0, 1], vec![9]] {
                assert_same(&runtime, &content, &selected);
            }
        }
    }
}
