# The SCORM 2004 conformance matrix

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/08-scorm12-conformance.md, docs/learning-standards/10-scorm2004-runtime.md

Spec: Testing Decisions → player seam; Further Notes → Official SCORM test suites.

- First, find out whether any part of the official SCORM 2004 test suites can be driven through the player seam, and record the answer here. The matrix does not depend on it.
- A SCORM 2004 4th Edition conformance matrix: content aggregation, run-time environment, and sequencing and navigation, each mandatory requirement mapped to an automated test. Uses the standards body's published test and sample packages plus small purpose-built ones.
- Packages built for the 2nd and 3rd Editions are shown to run on the same runtime.
- Any requirement the vendored library does not meet is fixed upstream or in a vendored patch, and listed as a known gap until it is.

Done when: every row names a passing test or a recorded gap, in Chromium and WebKit, and the matrix is committed beside the tests.

## Notes

2026-11-21, closed.

- Official test suites: yes, in part. The SCORM 2004 4th Edition test suite (1.1.1) is published by the standards body under Apache-2.0. Its LMS test lessons are Java applets that no browser runs, but its data drives the player seam: the 189 LMS test manifests and 188 case scripts are vendored unchanged in `tests/desktop-ui/scorm2004-conformance/adl/` (with the licence and provenance), every manifest imports through Alexandria's importer, and `tests/desktop-ui/adlLmsTest.ts` reads each case as the suite's Java does. The replay serves a probe lesson for every lesson page, carries out each visit's calls with the suite's evaluation rules, answers the user interface questions from the player, and performs the navigation with the player's controls. The suite's SCO, manifest and content package tests are for content and are not used. No sample courses are committed; the purpose-built packages cover what the suite does not.
- Result: 141 of 188 cases pass in Chromium and WebKit. The other 47 are pinned in `adl-known-failures.json` (row, why, every failing check), identical across both engines and repeated runs; each test passes only while its case fails exactly that way. `known-failures.mjs` regenerates the file from an `ADL_RECORD` run.
- Matrix: `tests/desktop-ui/scorm2004-conformance.md`, beside `scorm2004-conformance.spec.ts`. Content aggregation rows name Rust tests at the host seam; the matrix check accepts those, checks every replayed case named in a row exists, that a row with a pinned case says "known", and that every case is in some row.
- Fixed here: objective references compared and stored as decoded URIs (OB-02a refused, OB-12a); activities with an attempt duration limit were never delivered, because the runtime reads the limit as the attempt's duration (wrapper workaround, CM-01); `SetValue` on a group name (`cmi`, `cmi.objectives`) replaced the runtime's data model (wrapper answers 401, DMI); `reset()` kept the previous lesson's launch values and learner comments, so a lesson without a passing score read the one before's (wrapper clears them, DMB); a resumed lesson's `total_time` counted every session of earlier ended attempts (host now uses the total the runtime rendered on the last termination, DMB); the table of contents now hides activities a hidden-from-choice rule hides and offers choosable clusters; the harness serves `.htm` pages as HTML, as the handler does; the fake host supplies the real host's 2004 values.
- Ticket 10's questions. A lesson that did not exit with `suspend` gets a new attempt with fresh data: confirmed by the suite (DMB: after `time-out` the next launch is `ab-initio` with zero time; after a relaunch from a suspended package the engine resumes it). Time limits: the engine measures `timeLimitExceeded` from its own clock and the content's reported times; no host clock is wired, recorded under Not covered. A primary objective without an id: no observable loss; the host still gives the passing score, it is correctly absent from `cmi.objectives`, and the suite cases with id-less primary objectives (CM-01, MS-07, OB-*) pass. Preview: new test runs a 2004 package in the course editor preview through the preview sequencing commands only.
- Known gaps left (rows marked known deviation, all upstream): choice validity (constrainChoice, stopForwardTraversal, choiceExit and forwardOnly edge cases) and no table-of-contents entry for the root; 24 cases where the engine delivers a different activity and 5 where it delivers nothing; the session does not end when an exit or exit-parent rule ends the root's attempt; objective completion, progress and scores from global maps not reaching `cmi.objectives`; `request_valid` look-ahead; value validation relaxations and some error codes (DMI). By design: data shared across packages (DDM, `objectivesGlobalToSystem`). Not covered: selection and randomisation and `jump` (no suite case, no fixture yet).
- Deviations from the ticket: library gaps are worked around in the wrapper rather than in a vendored patch (the vendored files stay unmodified); the remaining gaps are pinned, not fixed. Answers the suite leaves to a person are made explicit in the matrix (a hidden control counts as disabled, as the suite's own answers assume).
- CI cost: the replay adds 376 browser tests, about 3.5 minutes with eight workers and roughly 14 with the CI's two. The change classifier now runs the backend description tests for the 2004 fixture directories (it also lacked `package-player-2004/`).
- Wrapper look-ahead: with the duration accessor on every activity the engine's continue preview stopped answering, so the workaround applies only to activities that have a limit; a lesson with a limit may show the same symptom (falls back to the runtime's validity flags).

Proposed doc edits, not made here:

- `docs/learning-standards-spec.md`, What conformance means here: link the 2004 matrix; Further Notes, Official SCORM test suites: the 2004 LMS test's data is replayed at the player seam, the 1.2 suite is not.
- `docs/desktop-ui-validation.md`: the replay, `ADL_RECORD`, `known-failures.mjs`, and the `?adl=` screen parameter.
- The conformance statement, when written: the 141 of 188 result and the pinned gaps.
- Upstream issues to file with the runtime library: the duration-limit alias, group-name `SetValue`, values kept by `reset()`, and the sequencing gaps above.
