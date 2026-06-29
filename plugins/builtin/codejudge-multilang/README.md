# codejudge

The umbrella plugin for the **codejudge** coding-challenge suite. It runs no code
itself — it declares the per-language judge plugins as **dependencies**, so
installing `codejudge` automatically installs:

- [`codejudge-javascript`](../codejudge-javascript/README.md) — runs JS locally
  in a QuickJS WebAssembly sandbox.
- [`codejudge-lua`](../codejudge-lua/README.md) — runs Lua locally in the
  fengari VM.
- `codejudge-python` (Pyodide) — coming.

Each language plugin is a self-contained interactive element that runs the
learner's solution **locally** (no network, no server) against the shared,
language-agnostic
[problem bank](../codejudge-shared/CONTENT_SCHEMA.md).

This umbrella's own UI is a landing page: it lists the bundled languages and the
problem bank. The actual solving happens in the language elements.

## Dependency behaviour

`codejudge` exercises the host plugin-dependency mechanism: its manifest lists
the language plugins under `dependencies`, the host resolves and installs them
before recording the umbrella, and refuses to uninstall a language plugin while
the umbrella still depends on it. The Settings → Plugins UI shows the
relationship on each card.

## Build

```bash
plugins/builtin/codejudge-shared/fetch-runtimes.sh all
```

bakes the problem bank for every codejudge plugin (and builds the JS runtime).
