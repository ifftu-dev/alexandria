# Code Editor: Python (graded)

Same architecture as `editor-javascript`, but the source is Python: it runs on
the **RustPython** VM (pure Rust) compiled to a zero-import `wasm32-unknown-unknown`
module — both for in-browser live eval and the host-side deterministic grader.

No stdlib: `print`/`input` are injected as native functions, which covers
teaching-level Python (variables, control flow, functions, lists/dicts/
comprehensions, string ops). `import math` and other modules are not available.

Note: RustPython (git main) requires rustc >= 1.95, so this grader crate pins
1.97 via its own `rust-toolchain.toml` — isolated from the rest of the repo,
which builds on the workspace toolchain.

See `../editor-javascript/README.md` for the shared architecture and content
schema. Build via `../editor-shared/build.sh python` (or `all`).
