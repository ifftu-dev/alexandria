//! Build-time post-processor: rewrite a grader wasm so every *function import*
//! becomes a local stub returning zero values, yielding a zero-import module
//! that instantiates in the host's empty-linker Wasmtime sandbox.
//!
//! Only the grader build is stubbed. The in-browser (wasm-bindgen) build keeps
//! its real JS-provided imports (Date etc.). In the grader these imports are
//! only reachable via JS `Date`/timezone, meaningless in a deterministic
//! grader, so stubbing them to zero is sound for grading.

use walrus::{ir::Value, Module, ValType};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (input, output) = (&args[1], &args[2]);
    let mut module = Module::from_file(input).expect("parse wasm");

    let mut func_imports = Vec::new();
    let mut non_func = 0;
    for imp in module.imports.iter() {
        match imp.kind {
            walrus::ImportKind::Function(fid) => {
                func_imports.push((fid, imp.module.clone(), imp.name.clone()))
            }
            _ => {
                non_func += 1;
                eprintln!("WARN non-function import {}::{}", imp.module, imp.name);
            }
        }
    }

    let count = func_imports.len();
    for (fid, m, n) in func_imports {
        let ty_id = module.funcs.get(fid).ty();
        let results: Vec<ValType> = module.types.get(ty_id).results().to_vec();
        module
            .replace_imported_func(fid, |(body, _args)| {
                for r in &results {
                    match r {
                        ValType::I32 => {
                            body.const_(Value::I32(0));
                        }
                        ValType::I64 => {
                            body.const_(Value::I64(0));
                        }
                        ValType::F32 => {
                            body.const_(Value::F32(0.0));
                        }
                        ValType::F64 => {
                            body.const_(Value::F64(0.0));
                        }
                        other => panic!("unsupported stub result {other:?}"),
                    }
                }
            })
            .expect("replace_imported_func");
        eprintln!("stubbed {m}::{n}");
    }

    module.emit_wasm_file(output).expect("write wasm");

    // BLAKE3 of the emitted wasm — this is the grader CID the manifest pins and
    // the host re-verifies before running it.
    let bytes = std::fs::read(output).expect("read emitted wasm");
    let hash = blake3::hash(&bytes);
    println!("STUBBED_FUNC_IMPORTS={count} NON_FUNC_IMPORTS={non_func}");
    println!("OUTPUT_BLAKE3={}", hash.to_hex());
}
