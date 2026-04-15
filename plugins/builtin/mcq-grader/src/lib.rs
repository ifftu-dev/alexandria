//! Canonical Alexandria MCQ grader.
//!
//! Phase 2 of the community plugin system. Compiles to
//! `wasm32-unknown-unknown` and is loaded by the host's
//! [`alexandria_node::plugins::wasm_runtime`] inside a deterministic
//! Wasmtime sandbox.
//!
//! ABI v1 (frozen):
//!
//! ```text
//! (export "alex_alloc"  (func (param i32) (result i32)))
//! (export "alex_dealloc" (func (param i32 i32)))
//! (export "alex_grade"  (func (param i32 i32) (result i64)))
//! ```
//!
//! Input envelope (JSON):
//!
//! ```json
//! {
//!   "version": "1",
//!   "content":    { "kind": "single" | "multi", "options": ["..."],
//!                   "correct_indices": [0, 2] },
//!   "submission": { "selected_indices": [0, 2] }
//! }
//! ```
//!
//! Output envelope (JSON):
//!
//! ```json
//! {
//!   "version": "1",
//!   "score": 1.0,
//!   "details": { "correct_count": 2, "total": 2, "kind": "single" | "multi" }
//! }
//! ```
//!
//! Determinism: pure function of inputs. No clock, RNG, allocator
//! noise that affects output bytes, or floating-point accumulation that
//! could differ across platforms (we only use `f64` divisions on small
//! integer counts). Wasmtime's NaN canonicalization is on either way.

#![no_std]

extern crate alloc;

use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
struct GradeInput {
    version: String,
    content: McqContent,
    submission: McqSubmission,
}

#[derive(Debug, Clone, Deserialize)]
struct McqContent {
    /// `"single"` or `"multi"`. Single = exactly one correct; the
    /// learner's `selected_indices` length must be 1. Multi = any
    /// non-empty subset of `correct_indices` counts proportionally.
    kind: String,
    #[serde(default)]
    options: Vec<String>,
    correct_indices: Vec<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct McqSubmission {
    #[serde(default)]
    selected_indices: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct ScoreRecord {
    version: &'static str,
    score: f64,
    details: ScoreDetails,
}

#[derive(Debug, Clone, Serialize)]
struct ScoreDetails {
    kind: String,
    correct_count: u32,
    incorrect_count: u32,
    total_correct: u32,
    selected_count: u32,
}

/// Host-callable allocator: returns a pointer into linear memory the
/// host is allowed to write `len` bytes into. We use `Vec::with_capacity`
/// + leak so the buffer lives until `alex_dealloc` is called.
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

/// Optional but useful: lets long-running hosts free buffers we no
/// longer need. The Phase-1 host doesn't call this (each Store is a
/// fresh instance), but plugin authors and future runtimes may.
#[no_mangle]
pub unsafe extern "C" fn alex_dealloc(ptr: i32, len: i32) {
    if ptr <= 0 || len <= 0 {
        return;
    }
    let _ = Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize);
}

/// Main entry point. Reads `len` bytes at `ptr`, parses the input
/// envelope, computes the score, and returns a packed `(out_ptr, out_len)`
/// where `out_ptr = packed >> 32` and `out_len = packed & 0xFFFF_FFFF`.
///
/// Errors are encoded as `ScoreRecord` with a `details.error` field rather
/// than via traps, so the host gets a structured response in every path
/// that reaches the return.
#[no_mangle]
pub extern "C" fn alex_grade(ptr: i32, len: i32) -> i64 {
    let input = unsafe { read_input(ptr, len) };
    let record = grade_inner(&input).unwrap_or_else(error_record);
    let bytes = serde_json::to_vec(&record).expect("ScoreRecord must serialize");
    pack_output(bytes)
}

unsafe fn read_input(ptr: i32, len: i32) -> Vec<u8> {
    if ptr <= 0 || len <= 0 {
        return Vec::new();
    }
    core::slice::from_raw_parts(ptr as *const u8, len as usize).to_vec()
}

fn grade_inner(bytes: &[u8]) -> Result<ScoreRecord, String> {
    let input: GradeInput = serde_json::from_slice(bytes)
        .map_err(|e| format!("invalid grade input: {e}"))?;
    if input.version != "1" {
        return Err(format!("unsupported input version '{}'", input.version));
    }
    grade_mcq(&input.content, &input.submission)
}

fn grade_mcq(content: &McqContent, submission: &McqSubmission) -> Result<ScoreRecord, String> {
    let kind = content.kind.as_str();
    if kind != "single" && kind != "multi" {
        return Err(format!("unknown mcq kind '{kind}'"));
    }
    if content.correct_indices.is_empty() {
        return Err("mcq content must have at least one correct option".to_string());
    }

    // Validate that all indices reference real options if `options` is
    // populated. `options` is informational for the renderer; correctness
    // is purely about index sets.
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
        // Single-answer: 1.0 iff exactly one index selected and it's correct.
        let score = if selected.len() == 1 && correct.contains(selected.iter().next().unwrap()) {
            1.0
        } else {
            0.0
        };
        let correct_count = if score > 0.0 { 1 } else { 0 };
        return Ok(ScoreRecord {
            version: "1",
            score,
            details: ScoreDetails {
                kind: "single".to_string(),
                correct_count,
                incorrect_count: selected_count.saturating_sub(correct_count),
                total_correct,
                selected_count,
            },
        });
    }

    // Multi: score is `(|selected ∩ correct| - |selected \ correct|) /
    // |correct|`, clamped to `[0, 1]`. This penalises wrong selections
    // (a learner who picks every option doesn't score full marks) while
    // crediting partial correctness — the same scoring the existing
    // built-in McqQuestion.vue uses.
    let intersect = selected.intersection(&correct).count() as i64;
    let extra = selected.difference(&correct).count() as i64;
    let raw = (intersect - extra).max(0) as f64 / total_correct as f64;
    let score = if raw > 1.0 { 1.0 } else { raw };
    Ok(ScoreRecord {
        version: "1",
        score,
        details: ScoreDetails {
            kind: "multi".to_string(),
            correct_count: intersect as u32,
            incorrect_count: extra as u32,
            total_correct,
            selected_count,
        },
    })
}

fn error_record(err: String) -> ScoreRecord {
    let mut details = ScoreDetails {
        kind: "error".to_string(),
        correct_count: 0,
        incorrect_count: 0,
        total_correct: 0,
        selected_count: 0,
    };
    // Stash the error message in `kind` since ScoreDetails is fixed-shape.
    // (A richer error envelope can land later without breaking ABI v1
    // because the host only reads `score` and treats `details` as opaque.)
    details.kind = format!("error: {err}");
    ScoreRecord {
        version: "1",
        score: 0.0,
        details,
    }
}

/// Pack `(ptr, len)` of an output buffer into a single i64 the way the
/// host expects: `(ptr_u32 as i64) << 32 | (len_u32 as i64)`. The buffer
/// itself is `mem::forget`-ed so the host can read it before it gets
/// dropped.
fn pack_output(bytes: Vec<u8>) -> i64 {
    let len = bytes.len() as i64;
    let ptr = bytes.as_ptr() as i64;
    core::mem::forget(bytes);
    ((ptr & 0xFFFF_FFFF) << 32) | (len & 0xFFFF_FFFF)
}
