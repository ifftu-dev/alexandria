//! Deterministic Python grader/runner for Alexandria's plugin system.
//!
//! Embeds the RustPython VM (pure Rust) compiled to a zero-import
//! `wasm32-unknown-unknown` module. One artifact serves both the host-side
//! Wasmtime grader (`alex_grade`) and in-browser live eval (`alex_run`), so the
//! live-eval result equals the graded result by construction.
//!
//! No stdlib: `print`/`input` are injected as native functions backed by Rust
//! buffers, which covers teaching-level Python (variables, control flow,
//! functions, lists/dicts/comprehensions). `import math` etc. are not available.
//!
//! ABI v1 (frozen, matches `mcq-grader`):
//! ```text
//! (export "alex_alloc"   (func (param i32) (result i32)))
//! (export "alex_dealloc" (func (param i32 i32)))
//! (export "alex_grade"   (func (param i32 i32) (result i64)))
//! (export "alex_run"     (func (param i32 i32) (result i64)))
//! (export "memory"       (memory))
//! ```
//!
//! Determinism: getrandom is a fixed zero stream (see below), so string hashing
//! and any RNG are reproducible; no clock/OS access.

use std::cell::RefCell;
use std::collections::VecDeque;

use rustpython_vm as vm;
use serde::{Deserialize, Serialize};
use vm::function::FuncArgs;
use vm::{PyResult, VirtualMachine};

thread_local! {
    static STDOUT: RefCell<String> = const { RefCell::new(String::new()) };
    static STDIN: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
}

/// Deterministic getrandom 0.3 custom backend — required so getrandom builds for
/// wasm32-unknown-unknown without JS, and so RustPython's string hashing / RNG
/// are a pure function of the inputs.
///
/// # Safety
/// `dest`/`len` describe a writable region getrandom hands us.
#[no_mangle]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    core::ptr::write_bytes(dest, 0, len);
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire formats (shared with the mcq / boa graders)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GradeInput {
    version: String,
    content: Content,
    submission: Submission,
}

#[derive(Deserialize)]
struct Content {
    #[serde(default)]
    tests: Vec<TestCase>,
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
// ABI
// ---------------------------------------------------------------------------

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

/// # Safety
/// `ptr`/`len` must be a region previously returned by [`alex_alloc`] with the
/// same `len`, not yet freed. The host only calls this with such pairs.
#[no_mangle]
pub unsafe extern "C" fn alex_dealloc(ptr: i32, len: i32) {
    if ptr <= 0 || len <= 0 {
        return;
    }
    let _ = Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize);
}

#[no_mangle]
pub extern "C" fn alex_grade(ptr: i32, len: i32) -> i64 {
    let bytes = read_input(ptr, len);
    pack(serde_json::to_vec(&grade_inner(&bytes)).expect("serialize"))
}

#[no_mangle]
pub extern "C" fn alex_run(ptr: i32, len: i32) -> i64 {
    let bytes = read_input(ptr, len);
    pack(serde_json::to_vec(&run_inner(&bytes)).expect("serialize"))
}

fn read_input(ptr: i32, len: i32) -> Vec<u8> {
    if ptr <= 0 || len <= 0 {
        return Vec::new();
    }
    unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize).to_vec() }
}

fn pack(bytes: Vec<u8>) -> i64 {
    let len = bytes.len() as i64;
    let ptr = bytes.as_ptr() as i64;
    core::mem::forget(bytes);
    ((ptr & 0xFFFF_FFFF) << 32) | (len & 0xFFFF_FFFF)
}

// ---------------------------------------------------------------------------
// Grading + running
// ---------------------------------------------------------------------------

fn grade_inner(bytes: &[u8]) -> ScoreRecord {
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

    let mut passed = 0u32;
    let mut cases = Vec::with_capacity(total as usize);
    for (i, (t, is_hidden)) in all.iter().enumerate() {
        let outcome = run_python(&input.submission.source, &t.stdin);
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

fn run_inner(bytes: &[u8]) -> RunResult {
    match serde_json::from_slice::<RunInput>(bytes) {
        Ok(input) => run_python(&input.source, &input.stdin),
        Err(e) => RunResult {
            stdout: String::new(),
            stderr: String::new(),
            error: Some(format!("invalid run input: {e}")),
        },
    }
}

/// Execute `source` in a fresh RustPython interpreter with injected
/// `print`/`input` backed by Rust buffers. Captures stdout; a Python exception
/// (or syntax error) is returned in `error`.
fn run_python(source: &str, stdin: &str) -> RunResult {
    STDOUT.with(|o| o.borrow_mut().clear());
    STDIN.with(|q| {
        let mut q = q.borrow_mut();
        q.clear();
        if !stdin.is_empty() {
            for line in stdin.split('\n') {
                q.push_back(line.to_string());
            }
        }
    });

    let interp = vm::Interpreter::without_stdlib(Default::default());
    let error = interp.enter(|vm| {
        let scope = vm.new_scope_with_builtins();

        let print_fn = vm.new_function(
            "print",
            |args: FuncArgs, vm: &VirtualMachine| -> PyResult<()> {
                let mut line = String::new();
                for (i, a) in args.args.iter().enumerate() {
                    if i > 0 {
                        line.push(' ');
                    }
                    line.push_str(&a.str(vm)?.to_string_lossy());
                }
                STDOUT.with(|o| {
                    o.borrow_mut().push_str(&line);
                    o.borrow_mut().push('\n');
                });
                Ok(())
            },
        );
        let input_fn = vm.new_function("input", |_args: FuncArgs, vm: &VirtualMachine| {
            let s = STDIN.with(|q| q.borrow_mut().pop_front()).unwrap_or_default();
            vm.ctx.new_str(s)
        });
        if scope
            .globals
            .set_item("print", print_fn.into(), vm)
            .and_then(|_| scope.globals.set_item("input", input_fn.into(), vm))
            .is_err()
        {
            return Some("failed to install host functions".to_string());
        }

        let code = match vm.compile(source, vm::compiler::Mode::Exec, "<submission>".to_owned()) {
            Ok(c) => c,
            Err(e) => return Some(format!("{e}")),
        };
        match vm.run_code_obj(code, scope) {
            Ok(_) => None,
            Err(exc) => {
                let mut s = String::new();
                let _ = vm.write_exception(&mut s, &exc);
                Some(s.trim().to_string())
            }
        }
    });

    RunResult {
        stdout: STDOUT.with(|o| o.borrow().clone()),
        stderr: String::new(),
        error,
    }
}
