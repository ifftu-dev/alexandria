# Code Editor: TypeScript (graded)

Same as the JavaScript editor plugin, but the source is TypeScript: types are
stripped in-engine by the bundled sucrase (`grader/src/sucrase.js`, run inside
the Boa engine) before execution. Everything else — the Boa runner
(`boa-runner-core`), the zero-import Wasmtime grader, in-browser live eval, the
`grader_private` hidden-test convention — is identical to `editor-javascript`.

See `../editor-javascript/README.md` for the architecture and content schema.
Build via `../editor-shared/build.sh typescript` (or `all`).
