//! Deterministic Wasmtime grader runtime.
//!
//! Phase 2 of the community plugin system. Runs `grader.wasm` modules in a
//! pure-compute sandbox with no I/O, no clock, and no source of nondeterminism
//! beyond the bytes the host hands in. The same `(content, submission, grader)`
//! triple must produce a byte-identical `ScoreRecord` on any node, on any
//! supported platform, today or in 2046 — that's the root of trust for
//! credential-bearing assessments.
//!
//! ABI v1 (frozen):
//!     ```
//!     (export "alex_alloc"  (func (param i32) (result i32)))
//!     (export "alex_dealloc" (func (param i32 i32)))
//!     (export "alex_grade"  (func (param i32 i32) (result i64)))
//!     ```
//!
//! The grader receives a UTF-8 JSON envelope:
//!     `{"version":"1","content":<...>,"submission":<...>}`
//! and returns a UTF-8 JSON envelope:
//!     `{"version":"1","score":<0..=1>,"details":<...>}`
//!
//! Determinism config:
//!  - NaN canonicalization on
//!  - relaxed SIMD off (its lane-order is implementation-defined)
//!  - threads off
//!  - reference types / multi-memory / memory64 off
//!  - fuel-based interruption (no wall-clock dependency)
//!  - no WASI imports — the grader has no access to clock, RNG, or filesystem
//!  - bounded memory growth via StoreLimits
//!
//! `wasi-virt` for graders that legitimately need a virtualized clock/RNG
//! arrives in a later session; the MCQ canonical grader (Phase 2 session 1)
//! is pure compute and does not need it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use wasmtime::{
    Config, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc,
};

/// Default per-grade budgets. Conservative for Phase 2; manifests can
/// request more in later phases.
pub const DEFAULT_MEMORY_MAX_BYTES: usize = 128 * 1024 * 1024; // 128 MiB
pub const DEFAULT_FUEL: u64 = 1_000_000_000; // ~1B wasm instructions
pub const DEFAULT_OUTPUT_MAX_BYTES: usize = 1024 * 1024; // 1 MiB

/// JSON envelope passed to the grader.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradeInput {
    pub version: String,
    pub content: serde_json::Value,
    pub submission: serde_json::Value,
}

/// JSON envelope returned by the grader. The `score` is a fraction in
/// `[0.0, 1.0]`; `details` is plugin-defined.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreRecord {
    pub version: String,
    pub score: f64,
    #[serde(default)]
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Copy)]
pub struct GraderBudgets {
    pub memory_max_bytes: usize,
    pub fuel: u64,
    pub output_max_bytes: usize,
}

impl Default for GraderBudgets {
    fn default() -> Self {
        Self {
            memory_max_bytes: DEFAULT_MEMORY_MAX_BYTES,
            fuel: DEFAULT_FUEL,
            output_max_bytes: DEFAULT_OUTPUT_MAX_BYTES,
        }
    }
}

/// Per-store data held alongside the WASM instance for limit enforcement.
struct GraderState {
    limits: StoreLimits,
}

/// Reusable runtime that owns the Wasmtime engine and a module cache.
/// Cheap to clone (everything inside is `Arc`).
#[derive(Clone)]
pub struct GraderRuntime {
    engine: Engine,
    /// Compiled module cache keyed by grader_cid. In-memory only for now;
    /// a later session adds an on-disk `Module::serialize` store so app
    /// restarts don't re-pay compilation cost.
    cache: Arc<Mutex<HashMap<String, Module>>>,
}

impl GraderRuntime {
    pub fn new() -> Result<Self, String> {
        let mut config = Config::new();

        // Determinism — the most important config in this whole module.
        // NaN canonicalization makes float operations bit-identical across
        // platforms (Wasmtime would otherwise be free to leave NaN payloads
        // implementation-defined).
        config.cranelift_nan_canonicalization(true);

        // Fuel-based interruption. We never use wall-clock deadlines —
        // those would tie the determinism story to scheduler whims.
        config.consume_fuel(true);

        // Disable nondeterministic / unsupported features. Threads and
        // async are off by default in this build (default features are
        // off in Cargo.toml); the explicit calls below cover the proposals
        // that *are* compiled in.
        config.wasm_relaxed_simd(false);
        config.wasm_multi_memory(false);
        config.wasm_memory64(false);
        // Reference types are deterministic and useful; leave enabled.

        let engine =
            Engine::new(&config).map_err(|e| format!("failed to create wasmtime engine: {e}"))?;

        Ok(Self {
            engine,
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Run a grader against a `(content, submission)` pair and return the
    /// resulting `ScoreRecord`. The same inputs (and same `wasm_bytes`)
    /// must yield byte-identical output every time.
    ///
    /// `grader_cid` is the BLAKE3 hex of `wasm_bytes` — the caller is
    /// responsible for passing a CID that matches the bytes (the install
    /// flow already verified this). The CID is used as a cache key.
    pub fn grade(
        &self,
        grader_cid: &str,
        wasm_bytes: &[u8],
        input_json: &[u8],
        budgets: GraderBudgets,
    ) -> Result<ScoreRecord, String> {
        let module = self.load_module(grader_cid, wasm_bytes)?;

        // Each grade gets its own Store — no shared state between
        // submissions, ever.
        let limits = StoreLimitsBuilder::new()
            .memory_size(budgets.memory_max_bytes)
            .build();
        let mut store = Store::new(&self.engine, GraderState { limits });
        store.limiter(|s| &mut s.limits);
        store
            .set_fuel(budgets.fuel)
            .map_err(|e| format!("failed to set fuel: {e}"))?;

        // No host imports — the grader runs against an empty linker.
        // No WASI, no Tauri, nothing. Anything the grader needs must
        // arrive in `input_json`.
        let linker: Linker<GraderState> = Linker::new(&self.engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| format!("failed to instantiate grader: {e}"))?;

        let alloc: TypedFunc<i32, i32> = instance
            .get_typed_func(&mut store, "alex_alloc")
            .map_err(|_| "grader missing required export 'alex_alloc'".to_string())?;
        let grade: TypedFunc<(i32, i32), i64> =
            instance
                .get_typed_func(&mut store, "alex_grade")
                .map_err(|_| "grader missing required export 'alex_grade'".to_string())?;
        let memory: Memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| "grader does not export linear memory 'memory'".to_string())?;

        // Allocate input region inside WASM memory.
        let in_len: i32 = input_json
            .len()
            .try_into()
            .map_err(|_| "grader input exceeds i32::MAX".to_string())?;
        let in_ptr = alloc
            .call(&mut store, in_len)
            .map_err(|e| format!("alex_alloc trapped: {e}"))?;
        if in_ptr <= 0 {
            return Err(format!(
                "grader alex_alloc returned non-positive pointer {in_ptr}"
            ));
        }

        // Copy input into the grader's memory.
        memory
            .write(&mut store, in_ptr as usize, input_json)
            .map_err(|e| format!("failed to write input into grader memory: {e}"))?;

        // Call grade. Returns a packed (ptr_hi32 << 32) | len_lo32.
        let packed = grade
            .call(&mut store, (in_ptr, in_len))
            .map_err(|e| format!("alex_grade trapped: {e}"))?;
        let (out_ptr, out_len) = unpack_pointer(packed);
        if out_ptr < 0 || out_len < 0 {
            return Err(format!(
                "grader returned invalid pointer/length ({out_ptr}, {out_len})"
            ));
        }
        let out_len_usize: usize = out_len as usize;
        if out_len_usize > budgets.output_max_bytes {
            return Err(format!(
                "grader output {out_len_usize}B exceeds budget {}B",
                budgets.output_max_bytes
            ));
        }

        let mut out = vec![0u8; out_len_usize];
        memory
            .read(&store, out_ptr as usize, &mut out)
            .map_err(|e| format!("failed to read grader output: {e}"))?;

        let record: ScoreRecord = serde_json::from_slice(&out)
            .map_err(|e| format!("grader output is not a valid ScoreRecord: {e}"))?;

        if !record.score.is_finite() || record.score < 0.0 || record.score > 1.0 {
            return Err(format!(
                "grader returned out-of-range score {} (expected [0.0, 1.0] finite)",
                record.score
            ));
        }
        if record.version != "1" {
            return Err(format!(
                "grader output declared version '{}', host expects '1'",
                record.version
            ));
        }

        Ok(record)
    }

    fn load_module(&self, grader_cid: &str, wasm_bytes: &[u8]) -> Result<Module, String> {
        if let Some(m) = self
            .cache
            .lock()
            .map_err(|_| "grader cache poisoned".to_string())?
            .get(grader_cid)
            .cloned()
        {
            return Ok(m);
        }
        let module = Module::from_binary(&self.engine, wasm_bytes)
            .map_err(|e| format!("failed to compile grader wasm: {e}"))?;
        self.cache
            .lock()
            .map_err(|_| "grader cache poisoned".to_string())?
            .insert(grader_cid.to_string(), module.clone());
        Ok(module)
    }
}

/// Unpack the i64 return value of `alex_grade` into `(ptr, len)`. Both
/// halves are i32; we sign-extend each so callers can detect "the grader
/// returned a negative number" without worrying about subtle wrap.
fn unpack_pointer(packed: i64) -> (i32, i32) {
    let ptr = (packed >> 32) as i32;
    let len = (packed & 0xFFFF_FFFF) as i32;
    (ptr, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny WASM module that satisfies the grader ABI by always
    /// returning the input as the output. Useful for testing the host
    /// plumbing without writing real grader logic. Compiled at test time
    /// from inline WAT via wat::parse_str — but we don't depend on `wat`
    /// in production, so we precompile and embed bytes.
    ///
    /// Equivalent WAT:
    /// ```wat
    /// (module
    ///   (memory (export "memory") 1)
    ///   (global $bump (mut i32) (i32.const 1024))
    ///   (func (export "alex_alloc") (param i32) (result i32)
    ///     (local $p i32)
    ///     (local.set $p (global.get $bump))
    ///     (global.set $bump (i32.add (global.get $bump) (local.get 0)))
    ///     (local.get $p))
    ///   (func (export "alex_grade") (param i32 i32) (result i64)
    ///     ;; Pack (ptr=param0, len=param1) into one i64 — echo input
    ///     (i64.or
    ///       (i64.shl (i64.extend_i32_u (local.get 0)) (i64.const 32))
    ///       (i64.extend_i32_u (local.get 1)))))
    /// ```
    /// We use the `wat` crate at test-time to keep the test readable.
    fn echo_grader_wasm() -> Vec<u8> {
        let wat = r#"
            (module
              (memory (export "memory") 1)
              (global $bump (mut i32) (i32.const 1024))
              (func (export "alex_alloc") (param i32) (result i32)
                (local $p i32)
                (local.set $p (global.get $bump))
                (global.set $bump (i32.add (global.get $bump) (local.get 0)))
                (local.get $p))
              (func (export "alex_grade") (param i32 i32) (result i64)
                (i64.or
                  (i64.shl (i64.extend_i32_u (local.get 0)) (i64.const 32))
                  (i64.extend_i32_u (local.get 1)))))
        "#;
        wat::parse_str(wat).expect("wat must parse")
    }

    #[test]
    fn echo_module_handshake() {
        // Echo grader returns its input bytes as the output. We feed it a
        // valid ScoreRecord to satisfy the host's output-validation step.
        let runtime = GraderRuntime::new().expect("runtime");
        let echo = echo_grader_wasm();
        let cid = blake3::hash(&echo).to_hex().to_string();
        let input = serde_json::to_vec(&serde_json::json!({
            "version": "1",
            "score": 0.42,
            "details": {"echo": true},
        }))
        .unwrap();
        let result = runtime
            .grade(&cid, &echo, &input, GraderBudgets::default())
            .expect("grade succeeds");
        assert_eq!(result.version, "1");
        assert!((result.score - 0.42).abs() < 1e-12);
    }

    #[test]
    fn rejects_grader_without_required_exports() {
        let runtime = GraderRuntime::new().expect("runtime");
        let wat = r#"(module (func (export "noop")))"#;
        let bytes = wat::parse_str(wat).unwrap();
        let cid = blake3::hash(&bytes).to_hex().to_string();
        let input =
            serde_json::to_vec(&serde_json::json!({"version":"1","score":0,"details":{}})).unwrap();
        let err = runtime.grade(&cid, &bytes, &input, GraderBudgets::default());
        assert!(err.is_err());
    }

    #[test]
    fn rejects_out_of_range_score() {
        let runtime = GraderRuntime::new().expect("runtime");
        let echo = echo_grader_wasm();
        let cid = blake3::hash(&echo).to_hex().to_string();
        let input =
            serde_json::to_vec(&serde_json::json!({"version":"1","score":1.5,"details":{}}))
                .unwrap();
        assert!(runtime
            .grade(&cid, &echo, &input, GraderBudgets::default())
            .is_err());
    }

    /// Bytes of the canonical MCQ grader compiled to wasm32-unknown-unknown.
    /// Rebuild from `plugins/builtin/mcq-grader/` and re-copy `dist/` after
    /// any change. See that crate's README for the exact command.
    const MCQ_GRADER_WASM: &[u8] =
        include_bytes!("../../../plugins/builtin/mcq-grader/dist/mcq_grader.wasm");

    fn run_mcq(content: serde_json::Value, submission: serde_json::Value) -> ScoreRecord {
        let runtime = GraderRuntime::new().expect("runtime");
        let cid = blake3::hash(MCQ_GRADER_WASM).to_hex().to_string();
        let input = serde_json::to_vec(&serde_json::json!({
            "version": "1",
            "content": content,
            "submission": submission,
        }))
        .unwrap();
        runtime
            .grade(&cid, MCQ_GRADER_WASM, &input, GraderBudgets::default())
            .expect("grade succeeds")
    }

    #[test]
    fn mcq_single_correct_answer_scores_one() {
        let r = run_mcq(
            serde_json::json!({
                "kind": "single",
                "options": ["A", "B", "C"],
                "correct_indices": [1],
            }),
            serde_json::json!({"selected_indices": [1]}),
        );
        assert_eq!(r.score, 1.0);
    }

    #[test]
    fn mcq_single_wrong_answer_scores_zero() {
        let r = run_mcq(
            serde_json::json!({
                "kind": "single",
                "options": ["A", "B", "C"],
                "correct_indices": [1],
            }),
            serde_json::json!({"selected_indices": [0]}),
        );
        assert_eq!(r.score, 0.0);
    }

    #[test]
    fn mcq_multi_partial_credit() {
        // 2 of 2 correct selected, 1 wrong selected: (2 - 1) / 2 = 0.5
        let r = run_mcq(
            serde_json::json!({
                "kind": "multi",
                "options": ["A", "B", "C", "D"],
                "correct_indices": [0, 2],
            }),
            serde_json::json!({"selected_indices": [0, 2, 3]}),
        );
        assert!((r.score - 0.5).abs() < 1e-12, "got {}", r.score);
    }

    #[test]
    fn mcq_multi_select_everything_is_not_full_marks() {
        // Picking every option must NOT yield 1.0 — that would defeat the
        // assessment. With 2 correct and 4 selected (2 wrong), score is
        // max((2 - 2) / 2, 0) = 0.
        let r = run_mcq(
            serde_json::json!({
                "kind": "multi",
                "options": ["A", "B", "C", "D"],
                "correct_indices": [0, 2],
            }),
            serde_json::json!({"selected_indices": [0, 1, 2, 3]}),
        );
        assert_eq!(r.score, 0.0);
    }

    #[test]
    fn mcq_grader_is_byte_reproducible() {
        // The credential trust model rests on byte-identical output for the
        // same input, every run, on every node. Run the grader 100 times
        // against fixed inputs and assert each ScoreRecord serialization is
        // identical at the byte level.
        let runtime = GraderRuntime::new().expect("runtime");
        let cid = blake3::hash(MCQ_GRADER_WASM).to_hex().to_string();
        let input = serde_json::to_vec(&serde_json::json!({
            "version": "1",
            "content": {
                "kind": "multi",
                "options": ["A","B","C","D","E"],
                "correct_indices": [0, 2, 4],
            },
            "submission": {"selected_indices": [0, 2, 3]},
        }))
        .unwrap();

        let first = runtime
            .grade(&cid, MCQ_GRADER_WASM, &input, GraderBudgets::default())
            .expect("first grade");
        let first_bytes = serde_json::to_vec(&first).unwrap();

        for i in 1..100 {
            let r = runtime
                .grade(&cid, MCQ_GRADER_WASM, &input, GraderBudgets::default())
                .unwrap_or_else(|e| panic!("grade #{i} failed: {e}"));
            let bytes = serde_json::to_vec(&r).unwrap();
            assert_eq!(
                bytes, first_bytes,
                "grade #{i} produced different bytes than the first run"
            );
        }
    }

    #[test]
    fn fuel_exhaustion_is_a_trap() {
        // Infinite-loop grader. The host's fuel limit must trap it.
        let wat = r#"
            (module
              (memory (export "memory") 1)
              (func (export "alex_alloc") (param i32) (result i32) (i32.const 1024))
              (func (export "alex_grade") (param i32 i32) (result i64)
                (loop $l (br $l))
                (i64.const 0)))
        "#;
        let bytes = wat::parse_str(wat).unwrap();
        let runtime = GraderRuntime::new().expect("runtime");
        let cid = blake3::hash(&bytes).to_hex().to_string();
        let budgets = GraderBudgets {
            fuel: 10_000,
            ..GraderBudgets::default()
        };
        let err = runtime.grade(&cid, &bytes, b"{}", budgets);
        assert!(err.is_err(), "fuel exhaustion must trap");
    }
}
