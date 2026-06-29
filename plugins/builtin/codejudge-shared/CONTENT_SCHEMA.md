# codejudge content schema (v1)

Canonical, **language-agnostic** problem format shared by every codejudge
language plugin (`codejudge-javascript`, `codejudge-python`, `codejudge-lua`)
and the `codejudge-multilang` umbrella. The build/vendor step copies these
files into each plugin bundle's `ui/problems/`.

A problem describes *what* to compute and the test cases — never how, and never
in a specific language. The **I/O contract is uniform**: the learner's solution
reads the test input from **stdin** and writes its answer to **stdout**. Each
language plugin runs the submitted source against these cases inside its bundled
in-browser interpreter (no network, no host) and compares stdout.

## Problem object

```json
{
  "version": "1",
  "id": "two-sum",
  "title": "Two Sum",
  "difficulty": "easy | medium | hard",
  "statement_md": "Markdown problem statement (rendered sanitized in the UI).",
  "io": "stdin-stdout",
  "limits": { "time_ms": 2000 },
  "tests": {
    "visible": [ { "input": "2 7 11 15\n9\n", "output": "0 1\n", "explain": "optional" } ],
    "hidden":  [ { "input": "3 2 4\n6\n",     "output": "1 2\n" } ]
  }
}
```

- **visible** tests are shown to the learner (samples) and run on demand for
  instant feedback.
- **hidden** tests are withheld; they only run when the learner submits, and the
  pass-fraction drives `alex.complete(progress)`. Authors put edge cases here.
- Output comparison normalizes trailing whitespace per line and trailing blank
  lines (same rule across all language plugins).

## Element content passed to a plugin

The host passes an element's content (`content_inline`) to the plugin iframe in
the `init` message. For a codejudge element that is either a full problem object
(above) or a reference + per-element overrides:

```json
{
  "version": "1",
  "problem_id": "two-sum",          // resolve from the bundle's ui/problems/
  "starter_code": "…optional override of the language plugin's default stub…"
}
```

A plugin resolves `problem_id` against its bundled problems, or accepts an inline
`problem` object. `language` is implied by which plugin renders the element — a
`codejudge-python` element runs Python; the field is not part of the problem.

## Why no `language` in the problem

Problems are reusable across languages: the statement and the stdin/stdout test
cases are identical whether solved in JS, Python, or Lua. Only the **starter
stub** differs, and that is owned by each language plugin (`stubs/<lang>.txt`),
not the problem. This is what lets the umbrella plugin offer the same problem
set in every installed language.
