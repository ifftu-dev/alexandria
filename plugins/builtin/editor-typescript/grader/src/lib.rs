//! TypeScript grader/runner ABI. Identical to the JavaScript grader except the
//! source is TypeScript: `boa-runner-core` strips types with the bundled sucrase
//! (`sucrase.js`, run inside the Boa engine) before executing. The engine,
//! scoring, and wire formats all live in `boa-runner-core`.
//!
//! ABI v1 (frozen, matches `mcq-grader`):
//! ```text
//! (export "alex_alloc"   (func (param i32) (result i32)))
//! (export "alex_dealloc" (func (param i32 i32)))
//! (export "alex_grade"   (func (param i32 i32) (result i64)))
//! (export "alex_run"     (func (param i32 i32) (result i64)))
//! (export "memory"       (memory))
//! ```

use boa_runner_core::{grade, pack, read_input, run};

/// Bundled sucrase (IIFE exposing `AlexSucrase.transform`). Produced by
/// `editor-shared/build.sh` from `editor-shared/cm6-build`.
const SUCRASE: &str = include_str!("sucrase.js");

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
    pack(grade(&bytes, Some(SUCRASE)))
}

#[no_mangle]
pub extern "C" fn alex_run(ptr: i32, len: i32) -> i64 {
    let bytes = unsafe { read_input(ptr, len) };
    pack(run(&bytes, Some(SUCRASE)))
}
