# Code Editor: JavaScript (graded)

A gradable code element: the learner writes JavaScript with CodeMirror 6 syntax
highlighting, sees live output as they type, runs the visible test cases locally,
and submits for a credential-bearing score.

## How it runs

One wasm artifact serves both sides, so the live-eval result equals the graded
result by construction:

- **In-browser** (`ui/worker.js`) — the Boa JS engine (`grader/`, compiled to
  `wasm32-unknown-unknown`) runs in a Web Worker for live eval and visible-test
  feedback. The wasm is base64-inlined (`ui/vendor/runner-wasm.js`); no `fetch`,
  so CSP `connect-src 'none'` holds. Runaway code is killed via
  `worker.terminate()`.
- **Host grader** (`grader/dist/editor_javascript_grader.wasm`) — the same Boa
  build, import-stubbed to zero imports, run in the host's deterministic
  Wasmtime sandbox on `alex.submit()` for the actual score. Desktop only.

## Build

```bash
# 1. compile the runner
cd grader && cargo build --target wasm32-unknown-unknown --release && cd ..

# 2. import-stub -> dist, and print the grader CID
../editor-shared/tools/wasmstub/target/release/wasmstub \
  grader/target/wasm32-unknown-unknown/release/editor_javascript_grader.wasm \
  grader/dist/editor_javascript_grader.wasm

# 3. base64-inline the stubbed wasm for the worker, and build the CM6 bundle
#    (see editor-shared/build.sh which does 1-3 and updates manifest.grader.cid)
```

Put the CID printed by `wasmstub` into `manifest.json` (`grader.cid` and
`grader.blake3`); the host re-verifies it before grading.

## Content schema (`content_inline`)

```json
{
  "title": "Double it",
  "prompt": "Read n, print n*2.",
  "starter_code": "const n = Number(readLine());\n",
  "tests": [{ "name": "example", "stdin": "4", "expected_stdout": "8" }],
  "grader_private": {
    "tests": [{ "name": "big", "stdin": "1000000", "expected_stdout": "2000000" }]
  }
}
```

`tests` are visible (shown in the iframe). `grader_private.tests` are hidden —
the host strips `grader_private` before the iframe `init`, so they never reach
the learner, but the grader scores over all of them.
