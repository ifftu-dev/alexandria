# Confirm and vendor the SCORM runtime library

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md

Spec: Runtime; Further Notes → Runtime library.

- Confirm the candidate runtime library against the wrapper design: one page at the package origin holds the run-time API objects, content in a nested frame finds them by each standard's discovery algorithm and calls them synchronously, and the host receives state asynchronously.
- Record its licence, the exact version pinned, and what it covers for SCORM 1.2, SCORM 2004 (2nd to 4th Edition) and sequencing and navigation. List every gap found.
- Vendor the pinned build into the repository with its licence file. No package-manager fetch at build or run time.
- If it falls short, say where and whether an upstream fix or a vendored patch closes the gap. Do not start a rewrite.

Done when: the pinned library and its licence are vendored; a minimal page loads it and a one-file SCORM 1.2 fixture initialises, sets a value, commits and terminates against it in the browser harness; the findings are appended here under Notes.
