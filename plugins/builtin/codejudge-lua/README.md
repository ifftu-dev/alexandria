# codejudge: Lua

An interactive Alexandria plugin for solving coding challenges in **Lua**. The
learner writes a solution in an in-app editor (with syntax highlighting); the
plugin runs it **locally**, in a sandboxed in-browser Lua VM, against the
problem's test cases and reports pass/fail. No network, no server, no host
process — everything happens inside the plugin iframe.

## How it runs code locally

- **Runtime:** [wasmoon](https://github.com/ceifa/wasmoon) — Lua 5.4 compiled to
  WebAssembly. Lua runs *inside* the wasm sandbox, so it never calls JS `eval`,
  which keeps it within the plugin iframe's strict CSP (`wasm-unsafe-eval`
  allowed, but `unsafe-eval` and network are not). The wasm is embedded in the
  bundle and instantiated with no fetch. (We started on fengari, a pure-JS Lua
  VM, but it needs `unsafe-eval` internally, which the CSP forbids.)
- **I/O contract:** the solution reads the test input from **stdin**
  (`io.read("l")`, `io.read("n")`, `io.read("a")`, `io.lines()`) and writes its
  answer to **stdout** (`print`, `io.write`). `runner.js` rebinds those to a
  host-backed stdin string + an output buffer.
- **Safety:** a VM instruction-count hook aborts runaway programs (the Lua
  equivalent of a time limit), so an infinite loop can't hang the iframe.

## Problems

Problems use the shared, language-agnostic
[`codejudge` content schema](../codejudge-shared/CONTENT_SCHEMA.md): a statement
plus **visible** sample tests and **hidden** tests. Visible tests run on
*Run sample tests*; *Submit* runs all of them and reports the pass fraction to
the host via `alex.complete()`. Hidden tests never reveal their data.

## Build

The third-party runtime (wasmoon — Lua 5.4 → WebAssembly) and CodeMirror are
vendored into `ui/vendor/`, and the problem bank is baked into `ui/problems.js`.
These generated files **are committed** (so the app and CI build with no extra
step). Regenerate them after changing the runtime versions or the problem bank:

```bash
plugins/builtin/codejudge-shared/fetch-runtimes.sh lua
```

The host embeds the bundle via `include_bytes!` and installs it as a built-in at
startup. End users never fetch anything.

## Related

Part of the **codejudge** family. Installing the `codejudge-multilang` umbrella
auto-installs this and the other language plugins via the plugin dependency
mechanism.
