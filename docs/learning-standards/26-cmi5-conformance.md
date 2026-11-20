# The cmi5 conformance matrix

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/25-cmi5-launch.md

Spec: What conformance means here → cmi5 row; Testing Decisions → cmi5 paragraph.

- A conformance matrix for the launch, session and completion rules that apply to packaged assignable units, each requirement mapped to an automated test through the host seam or the player seam.
- Uses the published cmi5 test packages.
- Requirements that apply only to remote units are listed as out of scope, not as gaps.

Done when: every row names a passing test, a recorded gap or an out-of-scope reason, and the matrix is committed beside the tests.

## Notes

2026-11-20: built.

- **Matrix** `src-tauri/src/packages/attempts/cmi5/conformance.md`, beside its host-seam suite `conformance.rs`. Rows name tests in guillemets; `every_row_of_the_cmi5_matrix_names_a_test_that_exists` fails if one is missing (it searches the cmi5 host tests, the studio session and import tests and `cmi5-player.spec.ts`).
- **Published packages.** The CATAPULT LMS Test Suite (Apache-2.0) is fetchable and may be committed. Its runtime packages 001 to 009 are committed as their unmodified `cmi5.xml` (with LICENSE and NOTICE) under `src-tauri/tests/fixtures/catapult-lts/`; each `lts_*` test imports one through the app's import path, launches it and replays over real HTTP the requests that package's unit makes (statements shaped as cmi5.js 3 shapes them), asserting the unit's own checks and the suite's post-run checks on the statements read back by registration. Import packages 101, 102, 206 and 207 were added to the import tests (201 to 205, 208 to 210 were already transcribed by ticket 24).
- **Decision: the units' JavaScript is not run, and CATAPULT's automated mode is not wired.** It needs a headless host serving the package to a browser, a test-only allowed origin on the endpoint and a remote read of statements by registration. All three are buildable, but driving them means a browser test starting a Rust host, which crosses the Rust and browser CI jobs, plus CATAPULT's webpack and Puppeteer toolchain. Replaying the same requests at the host seam covers the same checks against the real rules. Recorded under "Not covered here"; the manual procedure on a dev build stays the device-run route (ticket 28).
- **Fixes the suite found** (small, in this ticket): timestamps not in UTC refused (9.7.0.0-2); a scored passed or failed must carry the session's mastery score extension (9.6.3.2-2); a raw score needs min and max even beside a scaled one (9.5.1.0-3); the moveOn category is refused on allowed statements (9.6.2.2-2); every xAPI request is refused once the unit terminated (8.1.2.0-2); state requests for another registration are refused (8.1.4.0-3); an agent request without an agent is 400 (11.0.0.0-1); learner preferences writes must be a JSON object with a language tag list and `on`/`off` audio, checked before the profile preconditions (11.0.0.0-5, 11.1.0.0-1); `satisfied` for what moveOn already satisfies is recorded before `launched` (9.6.1.1-3, LTS 004-5). The player fixture unit and the host-seam harness now name the mastery score on scored judgements (the fixture's description is regenerated).

Known deviations (matrix rows, each pinned by a `known_deviation_*` test):

- Ticket 25's: a statement without id or timestamp gets one; rule breaks are refused (403), never voided; no grace period after terminated; launch mode always Normal (the Browse/Review rules pass in the studio crate); statement reads never page and `voidedStatementId` finds nothing (not a conformant record store); mobile own-window units play framed with focus mode forced.
- New: `initialized` is accepted before the learner preferences document is read (11.0.0.0-3 d2; refusing it could break units that never read it, owner decision); after a session ends the endpoint is gone, so late requests fail to connect instead of 401/403 (LTS 008-1); nothing is ever waived (LTS 009-1); course structure element order is not checked against the XSD (LTS 207-1 packaged); vendor extensions are ignored rather than validated; no endpoint account reads all statements (4.2.0.0-2).
- Out of scope: remote units (the published 101 and 20x structures as remote courses), course export, AU-side requirements.

Follow-ups:

- An author's preview of a cmi5 element is still not wired (load error). Not small: cmi5 sessions write statements, documents and the session row against a stored attempt, and previews store nothing; it needs an in-memory session store or a scratch attempt. Left for the owner to schedule.
- Owner decision: enforce learner-preferences-before-initialized (11.0.0.0-3) or keep the deviation.

Proposed doc edits, not made here:

- `docs/learning-standards-spec.md` → What conformance means here, cmi5 row: link the matrix (`src-tauri/src/packages/attempts/cmi5/conformance.md`) and say "self-tested with the CATAPULT LMS Test Suite, replayed at the host seam".
- `docs/learning-standards/27-*.md` (conformance statement): take the deviation list above.
- `docs/learning-standards/28-device-and-hosted-runs.md`: running CATAPULT's manual procedure (`lts/procedure.md`) on a dev build covers what the replay cannot (the units' JavaScript, the desktop own window).
