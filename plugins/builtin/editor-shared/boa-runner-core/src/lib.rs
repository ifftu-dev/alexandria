//! Shared Boa engine core for the JavaScript and TypeScript editor plugins.
//!
//! A language grader cdylib wires the `alex_*` ABI to [`grade`]/[`run`] here and
//! passes an optional sucrase bundle: `None` runs the source as JavaScript,
//! `Some(sucrase_js)` strips TypeScript types (in-engine) before running. The
//! same build serves both the host Wasmtime grader (`alex_grade`) and in-browser
//! live eval (`alex_run`), so the two agree by construction.

use std::cell::RefCell;
use std::collections::VecDeque;

use boa_engine::{js_string, Context, JsValue, NativeFunction, Source};
use serde::{Deserialize, Serialize};

thread_local! {
    static STDOUT: RefCell<String> = const { RefCell::new(String::new()) };
    static STDERR: RefCell<String> = const { RefCell::new(String::new()) };
    static STDIN: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
}

// Deterministic getrandom backend: required so getrandom builds for
// wasm32-unknown-unknown without wasm-bindgen, and so any RNG is reproducible.
fn deterministic_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    for b in buf.iter_mut() {
        *b = 0;
    }
    Ok(())
}
getrandom::register_custom_getrandom!(deterministic_getrandom);

// ---------------------------------------------------------------------------
// Wire formats (shared with the mcq grader envelope)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GradeInput {
    version: String,
    content: Content,
    submission: Submission,
}

#[derive(Deserialize)]
struct Content {
    /// Visible tests — also sent to the iframe for live feedback.
    #[serde(default)]
    tests: Vec<TestCase>,
    /// Grader-only material. The host strips `grader_private` from the content
    /// it forwards to the sandboxed iframe, so hidden test expectations never
    /// reach the learner; only this deterministic grader sees them.
    #[serde(default)]
    grader_private: GraderPrivate,
}

#[derive(Default, Deserialize)]
struct GraderPrivate {
    #[serde(default)]
    tests: Vec<TestCase>,
}

#[derive(Deserialize)]
struct TestCase {
    #[serde(default)]
    name: String,
    #[serde(default)]
    stdin: String,
    #[serde(default)]
    expected_stdout: String,
    #[serde(default)]
    hidden: bool,
}

#[derive(Deserialize)]
struct Submission {
    #[serde(default)]
    source: String,
}

#[derive(Serialize)]
struct ScoreRecord {
    version: &'static str,
    score: f64,
    details: ScoreDetails,
}

#[derive(Serialize)]
struct ScoreDetails {
    passed: u32,
    total: u32,
    cases: Vec<CaseResult>,
}

#[derive(Serialize)]
struct CaseResult {
    name: String,
    passed: bool,
    hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    got: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Deserialize)]
struct RunInput {
    #[serde(default)]
    source: String,
    #[serde(default)]
    stdin: String,
}

#[derive(Serialize)]
struct RunResult {
    stdout: String,
    stderr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ---------------------------------------------------------------------------
// Public entry points (called by each language cdylib's ABI)
// ---------------------------------------------------------------------------

/// Grade a `{version, content, submission}` envelope, returning serialized
/// `ScoreRecord` bytes. `sucrase` is `Some` for TypeScript.
pub fn grade(bytes: &[u8], sucrase: Option<&str>) -> Vec<u8> {
    let record = grade_inner(bytes, sucrase);
    serde_json::to_vec(&record).expect("ScoreRecord serializes")
}

/// Run a `{source, stdin}` envelope for live eval, returning serialized
/// `RunResult` bytes. `sucrase` is `Some` for TypeScript.
pub fn run(bytes: &[u8], sucrase: Option<&str>) -> Vec<u8> {
    let result = run_inner(bytes, sucrase);
    serde_json::to_vec(&result).expect("RunResult serializes")
}

/// Copy `len` bytes at `ptr` out of linear memory. Returns empty for a
/// non-positive pointer/length.
///
/// # Safety
/// `ptr`/`len` must describe a readable region the host wrote (via the
/// allocator the grader exports); the host guarantees this.
pub unsafe fn read_input(ptr: i32, len: i32) -> Vec<u8> {
    if ptr <= 0 || len <= 0 {
        return Vec::new();
    }
    core::slice::from_raw_parts(ptr as *const u8, len as usize).to_vec()
}

/// Pack a to-be-returned output buffer into the ABI's `(ptr << 32) | len` i64,
/// leaking the buffer so the host can read it before it is dropped.
pub fn pack(bytes: Vec<u8>) -> i64 {
    let len = bytes.len() as i64;
    let ptr = bytes.as_ptr() as i64;
    core::mem::forget(bytes);
    ((ptr & 0xFFFF_FFFF) << 32) | (len & 0xFFFF_FFFF)
}

fn grade_inner(bytes: &[u8], sucrase: Option<&str>) -> ScoreRecord {
    let input: GradeInput = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(e) => return grade_error(format!("invalid grade input: {e}")),
    };
    if input.version != "1" {
        return grade_error(format!("unsupported input version '{}'", input.version));
    }

    let visible = input.content.tests.iter().map(|t| (t, t.hidden));
    let hidden = input.content.grader_private.tests.iter().map(|t| (t, true));
    let all: Vec<(&TestCase, bool)> = visible.chain(hidden).collect();

    let total = all.len() as u32;
    if total == 0 {
        return grade_error("content has no test cases".to_string());
    }

    // TypeScript: strip types ONCE per grade (sucrase is expensive in-engine),
    // then run the resulting JavaScript against every test case.
    let source = match transpiled(sucrase, &input.submission.source) {
        Ok(s) => s,
        Err(e) => return grade_error(format!("TypeScript error: {e}")),
    };

    let mut passed = 0u32;
    let mut cases = Vec::with_capacity(total as usize);
    for (i, (t, is_hidden)) in all.iter().enumerate() {
        // No Boa loop cap for grading — the host's Wasmtime fuel budget bounds
        // runtime deterministically.
        let outcome = run_js(&source, &t.stdin, None);
        let ok = outcome.error.is_none() && outcome.stdout.trim() == t.expected_stdout.trim();
        if ok {
            passed += 1;
        }
        let name = if t.name.is_empty() {
            format!("case {}", i + 1)
        } else {
            t.name.clone()
        };
        cases.push(CaseResult {
            name,
            passed: ok,
            hidden: *is_hidden,
            got: if *is_hidden {
                None
            } else {
                Some(outcome.stdout.trim().to_string())
            },
            error: if *is_hidden { None } else { outcome.error },
        });
    }

    ScoreRecord {
        version: "1",
        score: passed as f64 / total as f64,
        details: ScoreDetails {
            passed,
            total,
            cases,
        },
    }
}

fn grade_error(msg: String) -> ScoreRecord {
    ScoreRecord {
        version: "1",
        score: 0.0,
        details: ScoreDetails {
            passed: 0,
            total: 0,
            cases: vec![CaseResult {
                name: "error".to_string(),
                passed: false,
                hidden: false,
                got: None,
                error: Some(msg),
            }],
        },
    }
}

fn run_inner(bytes: &[u8], sucrase: Option<&str>) -> RunResult {
    let input: RunInput = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(e) => {
            return RunResult {
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("invalid run input: {e}")),
            }
        }
    };
    let source = match transpiled(sucrase, &input.source) {
        Ok(s) => s,
        Err(e) => {
            return RunResult {
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("TypeScript error: {e}")),
            }
        }
    };
    // Live eval runs on the iframe main thread (no worker), so cap loop
    // iterations to keep a runaway program from freezing the UI. Generous enough
    // for real solutions; the credential grader (host, fuel-bounded) uses no cap.
    run_js(&source, &input.stdin, Some(LIVE_LOOP_LIMIT))
}

/// Boa loop-iteration cap for in-browser live eval. Real solutions stay well
/// under this; an infinite loop errors out instead of hanging the iframe.
const LIVE_LOOP_LIMIT: u64 = 20_000_000;

/// Return `source` unchanged for JavaScript, or the type-stripped JavaScript for
/// TypeScript (`sucrase` is `Some`). Runs sucrase once in a throwaway context.
fn transpiled(sucrase: Option<&str>, source: &str) -> Result<String, String> {
    match sucrase {
        None => Ok(source.to_string()),
        Some(suc) => {
            let mut ctx = Context::default();
            strip_typescript(&mut ctx, suc, source)
        }
    }
}

/// Execute `source` (already plain JavaScript) in a fresh Boa context.
/// `console.log` → stdout, `console.error`/`warn` → stderr,
/// `readLine()`/`input()` read `stdin`. `loop_limit` caps loop iterations when
/// `Some` (used for main-thread live eval); grading passes `None`.
fn run_js(source: &str, stdin: &str, loop_limit: Option<u64>) -> RunResult {
    STDOUT.with(|o| o.borrow_mut().clear());
    STDERR.with(|o| o.borrow_mut().clear());
    STDIN.with(|q| {
        let mut q = q.borrow_mut();
        q.clear();
        if !stdin.is_empty() {
            for line in stdin.split('\n') {
                q.push_back(line.to_string());
            }
        }
    });

    let mut ctx = Context::default();
    if let Some(limit) = loop_limit {
        ctx.runtime_limits_mut().set_loop_iteration_limit(limit);
    }
    if let Err(e) = install_host(&mut ctx) {
        return RunResult {
            stdout: String::new(),
            stderr: String::new(),
            error: Some(format!("host init failed: {e}")),
        };
    }

    let error = match ctx.eval(Source::from_bytes(source.as_bytes())) {
        Ok(_) => None,
        Err(e) => {
            let msg = e.to_string();
            STDERR.with(|o| {
                o.borrow_mut().push_str(&msg);
                o.borrow_mut().push('\n');
            });
            Some(msg)
        }
    };

    RunResult {
        stdout: STDOUT.with(|o| o.borrow().clone()),
        stderr: STDERR.with(|o| o.borrow().clone()),
        error,
    }
}

/// Load the bundled sucrase into the context and strip TypeScript types,
/// returning plain JavaScript. Sucrase is pure JS and runs inside Boa.
fn strip_typescript(ctx: &mut Context, sucrase_src: &str, source: &str) -> Result<String, String> {
    // sucrase's IIFE assigns to a global var and expects `self`/`window`.
    ctx.eval(Source::from_bytes(
        "var self = globalThis; var window = globalThis;",
    ))
    .map_err(|e| e.to_string())?;
    ctx.eval(Source::from_bytes(sucrase_src.as_bytes()))
        .map_err(|e| format!("failed to load transpiler: {e}"))?;
    let src_literal = serde_json::to_string(source).map_err(|e| e.to_string())?;
    let call = format!(
        "globalThis.__ALEX_TSJS = AlexSucrase.transform({src_literal}, {{ transforms: ['typescript'] }}).code;"
    );
    ctx.eval(Source::from_bytes(call.as_bytes()))
        .map_err(|e| e.to_string())?;
    let v = ctx
        .global_object()
        .get(js_string!("__ALEX_TSJS"), ctx)
        .map_err(|e| e.to_string())?;
    Ok(v.to_string(ctx)
        .map_err(|e| e.to_string())?
        .to_std_string_escaped())
}

fn install_host(ctx: &mut Context) -> Result<(), String> {
    fn push(target: &RefCell<String>, args: &[JsValue], ctx: &mut Context) {
        let mut line = String::new();
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            match a.to_string(ctx) {
                Ok(s) => line.push_str(&s.to_std_string_escaped()),
                Err(_) => line.push_str("<unprintable>"),
            }
        }
        let mut b = target.borrow_mut();
        b.push_str(&line);
        b.push('\n');
    }

    ctx.register_global_callable(
        js_string!("__alex_out"),
        1,
        NativeFunction::from_fn_ptr(|_, args, ctx| {
            STDOUT.with(|o| push(o, args, ctx));
            Ok(JsValue::undefined())
        }),
    )
    .map_err(|e| e.to_string())?;

    ctx.register_global_callable(
        js_string!("__alex_err"),
        1,
        NativeFunction::from_fn_ptr(|_, args, ctx| {
            STDERR.with(|o| push(o, args, ctx));
            Ok(JsValue::undefined())
        }),
    )
    .map_err(|e| e.to_string())?;

    ctx.register_global_callable(
        js_string!("__alex_readline"),
        0,
        NativeFunction::from_fn_ptr(|_, _, _| {
            let next = STDIN.with(|q| q.borrow_mut().pop_front());
            Ok(match next {
                Some(s) => JsValue::from(js_string!(s.as_str())),
                None => JsValue::null(),
            })
        }),
    )
    .map_err(|e| e.to_string())?;

    let prelude = r#"
        globalThis.console = {
            log: (...a) => __alex_out(...a),
            info: (...a) => __alex_out(...a),
            debug: (...a) => __alex_out(...a),
            error: (...a) => __alex_err(...a),
            warn: (...a) => __alex_err(...a),
        };
        globalThis.print = (...a) => __alex_out(...a);
        globalThis.readLine = () => __alex_readline();
        globalThis.readline = () => __alex_readline();
        globalThis.input = (prompt) => { if (prompt !== undefined) __alex_out(prompt); return __alex_readline(); };
    "#;
    ctx.eval(Source::from_bytes(prelude))
        .map_err(|e| format!("prelude: {e}"))?;
    Ok(())
}
