//! JavaScript grader/runner ABI. Thin wrapper: the engine, scoring, and wire
//! formats live in `boa-runner-core`; this crate just exports the frozen
//! `alex_*` ABI and runs source as plain JavaScript (no transpile step).
//!
//! One artifact serves both callers:
//!  - **Grader** (`alex_grade`) — run host-side in the empty-linker Wasmtime
//!    sandbox for credential scoring. The wasm is import-stubbed to zero imports
//!    by `editor-shared/tools/wasmstub` before it reaches the host.
//!  - **In-browser live eval** (`alex_run`) — the same wasm, base64-inlined into
//!    the iframe bundle and instantiated with an empty import object in a Worker.
//!
//! ABI v1 (frozen, matches `mcq-grader`):
//! ```text
//! (export "alex_alloc"   (func (param i32) (result i32)))
//! (export "alex_dealloc" (func (param i32 i32)))
//! (export "alex_grade"   (func (param i32 i32) (result i64)))
//! (export "alex_run"     (func (param i32 i32) (result i64)))
//! (export "memory"       (memory))
//! ```

use boa_runner_core::{grade, pack, read_input, run, Lang};

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
    let bytes = unsafe { read_input(ptr, len) };
    pack(grade(&bytes, Lang::Js))
}

#[no_mangle]
pub extern "C" fn alex_run(ptr: i32, len: i32) -> i64 {
    let bytes = unsafe { read_input(ptr, len) };
    pack(run(&bytes, Lang::Js))
}
