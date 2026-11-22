//! Alexandria Language Lab grader.
//!
//! Compiles to `wasm32-unknown-unknown` and runs inside the host's
//! deterministic Wasmtime sandbox, exactly like the MCQ grader.
//!
//! ABI v1 (frozen):
//!
//! ```text
//! (export "alex_alloc"   (func (param i32) (result i32)))
//! (export "alex_dealloc" (func (param i32 i32)))
//! (export "alex_grade"   (func (param i32 i32) (result i64)))
//! ```
//!
//! Input: `{"version":"1","content":{...},"submission":{...}}`. The content
//! is the element's full `content_inline`, including the top-level
//! `grader_private` block the host strips before the plugin UI sees it.
//! `content.activity` picks the scorer: `dictation`, `reading`, `writing`
//! or `builder` (see `grade.rs` and the README for each shape).
//! `read_aloud` goes to an instructor and is never graded here.
//!
//! Output: `{"version":"1","score":<0..=1>,"details":{...}}`. Errors are a
//! score of 0 with `details.error`, never a trap.
//!
//! Determinism: item scores are integers in basis points; the only float
//! operation is the final division of two integers.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod grade;
mod text;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct GradeInput {
    version: String,
    content: Value,
    #[serde(default)]
    submission: Value,
}

/// Grade one JSON input envelope and return the JSON output envelope.
pub fn grade_json(bytes: &[u8]) -> Vec<u8> {
    let record = match grade_inner(bytes) {
        Ok(outcome) => json!({
            "version": "1",
            "score": outcome.score(),
            "details": outcome.details,
        }),
        Err(err) => json!({
            "version": "1",
            "score": 0.0,
            "details": { "error": err },
        }),
    };
    serde_json::to_vec(&record).unwrap_or_else(|_| {
        br#"{"version":"1","score":0.0,"details":{"error":"serialize"}}"#.to_vec()
    })
}

fn grade_inner(bytes: &[u8]) -> Result<grade::Outcome, String> {
    let input: GradeInput =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid grade input: {e}"))?;
    if input.version != "1" {
        return Err(format!("unsupported input version '{}'", input.version));
    }
    grade::grade(&input.content, &input.submission)
}

/// Host-callable allocator: a buffer the host may write `len` bytes into.
#[no_mangle]
pub extern "C" fn alex_alloc(len: i32) -> i32 {
    if len <= 0 {
        return 0;
    }
    let mut buf: Vec<u8> = Vec::with_capacity(len as usize);
    let ptr = buf.as_mut_ptr();
    core::mem::forget(buf);
    ptr as i32
}

/// Frees a buffer from `alex_alloc` (each grade runs in a fresh instance,
/// so the host does not need to call this).
///
/// # Safety
/// `ptr`/`len` must come from a prior `alex_alloc(len)` call.
#[no_mangle]
pub unsafe extern "C" fn alex_dealloc(ptr: i32, len: i32) {
    if ptr <= 0 || len <= 0 {
        return;
    }
    let _ = Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize);
}

/// Reads `len` bytes at `ptr`, grades, and returns `(out_ptr << 32) | out_len`.
#[no_mangle]
pub extern "C" fn alex_grade(ptr: i32, len: i32) -> i64 {
    let input = if ptr <= 0 || len <= 0 {
        Vec::new()
    } else {
        // SAFETY: the host wrote `len` bytes at `ptr` via `alex_alloc`.
        unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize).to_vec() }
    };
    let bytes = grade_json(&input);
    let out_len = bytes.len() as i64;
    let out_ptr = bytes.as_ptr() as i64;
    core::mem::forget(bytes);
    ((out_ptr & 0xFFFF_FFFF) << 32) | (out_len & 0xFFFF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(content: Value, submission: Value) -> Value {
        let input = serde_json::to_vec(&json!({
            "version": "1",
            "content": content,
            "submission": submission,
        }))
        .unwrap();
        serde_json::from_slice(&grade_json(&input)).unwrap()
    }

    fn score(v: &Value) -> f64 {
        v["score"].as_f64().unwrap()
    }

    #[test]
    fn dictation_scores_word_lcs_per_item_and_averages() {
        let content = json!({
            "activity": "dictation",
            "items": [
                { "text": "Good morning.", "hint_hi": "सुप्रभात" },
                { "text": "I drink tea every day." },
            ]
        });
        let perfect = run(
            content.clone(),
            json!({ "answers": ["good morning", "I drink tea every day"] }),
        );
        assert_eq!(score(&perfect), 1.0);

        // Item 1 perfect, item 2: 4 of 5 words match ("drinks") → 0.8. Mean 0.9.
        let partial = run(
            content.clone(),
            json!({ "answers": ["Good morning!", "I drinks tea every day."] }),
        );
        assert_eq!(score(&partial), 0.9);
        let item = &partial["details"]["items"][1];
        assert_eq!(item["matched"], 4);
        assert_eq!(item["expected"], 5);
        assert_eq!(
            item["diff"][1],
            json!({ "word": "drink", "status": "missing" })
        );
        assert_eq!(
            item["diff"][2],
            json!({ "word": "drinks", "status": "extra" })
        );

        // Extra words are penalised: max(expected, typed) is the denominator.
        let padded = run(
            content.clone(),
            json!({ "answers": ["good good morning morning", ""] }),
        );
        assert_eq!(score(&padded), 0.25);

        let missing = run(content, json!({}));
        assert_eq!(score(&missing), 0.0);
    }

    #[test]
    fn reading_counts_correct_choices() {
        let content = json!({
            "activity": "reading",
            "passage": "My name is Priya.",
            "glossary": { "name": "नाम" },
            "questions": [
                { "prompt": "Q1", "options": ["a", "b"] },
                { "prompt": "Q2", "options": ["a", "b", "c"] },
            ],
            "grader_private": { "answers": [1, 2] }
        });
        assert_eq!(
            score(&run(content.clone(), json!({ "selected": [1, 2] }))),
            1.0
        );
        assert_eq!(
            score(&run(content.clone(), json!({ "selected": [1, null] }))),
            0.5
        );
        assert_eq!(
            score(&run(content.clone(), json!({ "selected": [0] }))),
            0.0
        );
        let r = run(content, json!({ "selected": [0, 2] }));
        assert_eq!(r["details"]["correct"], json!([false, true]));
    }

    #[test]
    fn reading_rejects_bad_keys() {
        let content = json!({
            "activity": "reading",
            "questions": [{ "prompt": "Q1", "options": ["a", "b"] }],
            "grader_private": { "answers": [5] }
        });
        let r = run(content, json!({ "selected": [5] }));
        assert_eq!(score(&r), 0.0);
        assert!(r["details"]["error"]
            .as_str()
            .unwrap()
            .contains("out of range"));
    }

    fn writing_content() -> Value {
        json!({
            "activity": "writing",
            "items": [
                { "prompt_hi": "मेरे पास एक छाता है।" },
                { "prompt_hi": "अपने शहर के बारे में लिखिए।" },
            ],
            "grader_private": { "items": [
                { "accept": ["I have an umbrella."], "require": ["umbrella"] },
                { "require": ["city"], "min_words": 5 },
            ]}
        })
    }

    #[test]
    fn writing_accepts_exact_answers_and_scores_checks() {
        let r = run(
            writing_content(),
            json!({ "answers": ["i have an umbrella", "My city is very big."] }),
        );
        assert_eq!(score(&r), 1.0);
        assert_eq!(r["details"]["items"][0]["accepted"], true);

        // Item 1: "a umbrella" → capital ok, full stop ok, articles fail,
        // umbrella ok → 3/4. Item 2: no capital, no full stop, articles ok,
        // city ok, only 4 words → 2/5. Mean (7500 + 4000) / 2.
        let r = run(
            writing_content(),
            json!({ "answers": ["I have a umbrella.", "my city is big"] }),
        );
        assert_eq!(score(&r), 0.575);
        assert_eq!(r["details"]["items"][0]["failed"], json!(["articles"]));
        assert_eq!(
            r["details"]["items"][0]["article_issues"][0],
            json!({ "article": "a", "next": "umbrella", "expected": "an" })
        );
        assert_eq!(
            r["details"]["items"][1]["failed"],
            json!(["capital", "end_punctuation", "min_words"])
        );
    }

    #[test]
    fn writing_empty_and_custom_checks() {
        let content = json!({
            "activity": "writing",
            "items": [{ "prompt_hi": "x" }],
            "grader_private": { "items": [{ "checks": ["capital"], "require": ["my name is"] }] }
        });
        let r = run(content.clone(), json!({ "answers": [""] }));
        assert_eq!(score(&r), 0.0);
        assert_eq!(r["details"]["items"][0]["failed"], json!(["empty"]));
        assert_eq!(
            score(&run(
                content.clone(),
                json!({ "answers": ["My name is Asha"] })
            )),
            1.0
        );
        assert_eq!(
            score(&run(content, json!({ "answers": ["I am Asha"] }))),
            0.5
        );

        let bad = json!({
            "activity": "writing",
            "items": [{ "prompt_hi": "x" }],
            "grader_private": { "items": [{ "checks": ["rhymes"] }] }
        });
        let r = run(bad, json!({ "answers": ["Hello."] }));
        assert!(r["details"]["error"].as_str().unwrap().contains("rhymes"));
    }

    #[test]
    fn builder_matches_answer_or_alternatives() {
        let content = json!({
            "activity": "builder",
            "items": [
                { "tiles": ["rice.", "I", "eat"] },
                { "tiles": ["on", "cricket", "We", "Sunday", "play"] },
            ],
            "grader_private": { "items": [
                { "answer": "I eat rice." },
                { "answer": "We play cricket on Sunday.", "also": ["On Sunday we play cricket."] },
            ]}
        });
        assert_eq!(
            score(&run(
                content.clone(),
                json!({ "orders": [[1, 2, 0], [2, 4, 1, 0, 3]] })
            )),
            1.0
        );
        assert_eq!(
            score(&run(
                content.clone(),
                json!({ "orders": [[1, 2, 0], [0, 3, 2, 4, 1]] })
            )),
            1.0
        );
        let wrong = run(
            content.clone(),
            json!({ "orders": [[1, 0, 2], [2, 4, 1, 0, 3]] }),
        );
        assert_eq!(score(&wrong), 0.5);
        assert_eq!(wrong["details"]["correct"], json!([false, true]));
        // Repeated, out-of-range and missing orders are wrong, not errors.
        assert_eq!(
            score(&run(content.clone(), json!({ "orders": [[1, 1, 0], [9]] }))),
            0.0
        );
        assert_eq!(score(&run(content, json!({}))), 0.0);
    }

    #[test]
    fn read_aloud_and_bad_input_are_errors() {
        let r = run(json!({ "activity": "read_aloud", "items": [] }), json!({}));
        assert_eq!(score(&r), 0.0);
        assert!(r["details"]["error"].is_string());
        let r = run(json!({ "activity": "poetry" }), json!({}));
        assert!(r["details"]["error"].as_str().unwrap().contains("poetry"));
        let r: Value = serde_json::from_slice(&grade_json(b"not json")).unwrap();
        assert_eq!(score(&r), 0.0);
        let r: Value =
            serde_json::from_slice(&grade_json(br#"{"version":"2","content":{}}"#)).unwrap();
        assert!(r["details"]["error"].as_str().unwrap().contains("version"));
    }

    #[test]
    fn output_is_byte_identical_across_runs() {
        let input = serde_json::to_vec(&json!({
            "version": "1",
            "content": writing_content(),
            "submission": { "answers": ["I have a umbrella.", "my city is big"] },
        }))
        .unwrap();
        assert_eq!(grade_json(&input), grade_json(&input));
    }
}
