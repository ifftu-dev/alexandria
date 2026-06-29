# codejudge: JavaScript

An interactive Alexandria plugin for solving coding challenges in **JavaScript**.
The learner writes a solution in an in-app editor (with syntax highlighting); the
plugin runs it **locally**, in a sandboxed QuickJS WebAssembly engine, against the
problem's test cases and reports pass/fail. No network, no server, no host
process — everything happens inside the plugin iframe.

## How it runs code locally

- **Runtime:** [quickjs-emscripten](https://github.com/justjake/quickjs-emscripten)
  — the QuickJS JavaScript engine compiled to WebAssembly. User code is
  interpreted *inside* the wasm sandbox; the plugin never calls the host's
  `eval`/`Function`. The wasm is embedded as base64 in the bundle, so it
  instantiates via `WebAssembly` (allowed by `wasm-unsafe-eval`) without any
  network fetch — exactly what the plugin iframe CSP (`connect-src 'none'`, no
  `unsafe-eval`) requires.
- **I/O contract:** the solution reads the test input from **stdin** and writes
  its answer to **stdout**. In the sandbox: `stdin` is a global string,
  `readLine()` returns the next line, and `print(...)` / `console.log(...)` write
  to stdout.
- **Safety:** a wall-clock interrupt handler aborts runaway programs (time
  limit), so an infinite loop can't hang the iframe.

## Problems

Problems use the shared, language-agnostic
[`codejudge` content schema](../codejudge-shared/CONTENT_SCHEMA.md): a statement
plus **visible** sample tests and **hidden** tests. Visible tests run on *Run
sample tests*; *Submit* runs all of them and reports the pass fraction to the
host via `alex.complete()`. Hidden tests never reveal their data.

## Build

Third-party runtimes are **not committed**. The fetch step downloads CodeMirror,
bakes the problem bank into `ui/problems.js`, and **builds** the offline QuickJS
bundle (`esbuild` over the singlefile-browser variant) into `ui/vendor/`:

```bash
plugins/builtin/codejudge-shared/fetch-runtimes.sh javascript
```

The host then embeds the bundle via `include_bytes!` and installs it as a
built-in at startup. End users never fetch anything.

## Related

Part of the **codejudge** family. Installing the `codejudge-multilang` umbrella
auto-installs this and the other language plugins via the plugin dependency
mechanism.
