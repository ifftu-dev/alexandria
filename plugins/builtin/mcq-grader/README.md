# MCQ Grader (Phase 2 canonical built-in)

A deterministic WASM grader for multiple-choice questions, run inside the
Alexandria host's [`plugins/wasm_runtime`](../../../src-tauri/src/plugins/wasm_runtime.rs)
sandbox. This is the reference implementation that proves the
credential-bearing path end-to-end.

## ABI v1 (frozen)

```
(export "alex_alloc"   (func (param i32) (result i32)))
(export "alex_dealloc" (func (param i32 i32)))
(export "alex_grade"   (func (param i32 i32) (result i64)))
(export "memory"       (memory))
```

`alex_grade` returns `(out_ptr << 32) | out_len` — the host reads
`out_len` bytes at `out_ptr` and parses as a JSON `ScoreRecord`.

The grader also exports the optional `alex_probe` (same signature). Given
the grade envelope plus a `seed`, it returns a follow-up probe asking for
every option that is not correct; the probe's expectation is graded by
`alex_grade` like any MCQ. The host's native MCQ probe in
`src-tauri/src/assessment/mcq.rs` must match it, and a test checks that.

## Build

```bash
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/mcq_grader.wasm dist/
```

The `dist/mcq_grader.wasm` artifact is checked in so the host's
reproducibility tests don't require a wasm toolchain at CI time. Rebuild
and re-commit `dist/` after any source change.

## Determinism contract

This grader is pure compute over JSON-serialized inputs:

- No clock, no RNG, no filesystem, no network — Wasmtime denies all by
  construction (no WASI imports linked).
- Float operations limited to `f64` divisions on small integer counts;
  Wasmtime's NaN canonicalization is on regardless.
- `serde_json` deserialization is deterministic on valid input.

A verifier — anywhere on the network, decades from now — can fetch the
`(content_cid, submission_cid, grader_cid)` triple and re-run this WASM
to confirm the recorded score reproduces.
