# Code Editors

The collection plugin for the graded code-editor suite. It runs no code itself —
it declares the per-language editor plugins as **dependencies**, so installing
`editors` automatically installs:

- [`editor-javascript`](../editor-javascript/README.md) — Boa (WebAssembly).
- [`editor-typescript`](../editor-typescript/README.md) — Boa + sucrase type-strip.
- [`editor-cpp`](../editor-cpp/README.md) — JSCPP C/C++ interpreter.
- [`editor-python`](../editor-python/README.md) — RustPython (WebAssembly).

Each language plugin is a self-contained `graded` element: a CodeMirror 6 editor
with syntax highlighting and live evaluation that runs the learner's code
**locally** (no network, no server) against visible tests, then submits for a
deterministic, credential-bearing score computed by the host's Wasmtime grader.
The same zero-import wasm engine powers both the in-browser live eval and the
host grader, so a passing live run equals the graded result by construction.

This collection's own UI is a landing page listing the bundled languages; the
actual editing and grading happen in the language elements.

## Scope

`editors` and its language plugins are **course-scoped** (`manifest.scope =
"course"`): they are not installed at startup for everyone. They install the
first time a learner enrolls in a course that requires them, and precompile
their graders as part of that install so the first submission is fast. Once
installed they are machine-wide (shared by any course needing the same plugin).

## Dependency behaviour

`editors` exercises the host plugin-dependency mechanism: its manifest lists the
language plugins under `dependencies`, the host resolves and installs them before
recording the collection, and refuses to uninstall a language plugin while the
collection still depends on it. The Settings → Plugins UI shows the relationship
on each card.
