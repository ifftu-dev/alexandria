# codejudge: Lua

An interactive Alexandria plugin for solving coding challenges in **Lua**. The
learner writes a solution in an in-app editor (with syntax highlighting); the
plugin runs it **locally**, in a sandboxed in-browser Lua VM, against the
problem's test cases and reports pass/fail. No network, no server, no host
process — everything happens inside the plugin iframe.

## How it runs code locally

- **Runtime:** [fengari](https://fengari.io) — a complete Lua 5.3 VM written in
  pure JavaScript. It interprets Lua directly, so it needs neither WebAssembly
  `eval` nor JS `eval`, which keeps it within the plugin iframe's strict CSP
  (`connect-src 'none'`, no `unsafe-eval`).
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

Third-party runtimes (fengari, CodeMirror) are **not committed**. Fetch them
into `ui/vendor/` and bake the problem bank into `ui/problems.js` before
building the app:

```bash
plugins/builtin/codejudge-shared/fetch-runtimes.sh lua
```

The host then embeds the bundle via `include_bytes!` and installs it as a
built-in at startup. End users never fetch anything.

## Related

Part of the **codejudge** family. Installing the `codejudge-multilang` umbrella
auto-installs this and the other language plugins via the plugin dependency
mechanism.
