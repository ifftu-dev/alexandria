# Learning standards conformance statement

Status: active
Updated: 2026-11-21

What Alexandria claims about SCORM, cmi5 and xAPI, with the evidence for each claim and what it does not meet. Built by [learning-standards-spec.md](learning-standards-spec.md); this page is the conformance statement its "What conformance means here" section asks for. Hand it to a procurement reviewer as it stands: every number below comes from a test that runs in continuous integration, or is marked as not yet run.

How claims are worded:

- **SCORM is self-declared by design of the standard.** The wording is "conformant, self-tested", never "certified". Where the evidence does not support "conformant" for an area, the wording is "partially conformant, self-tested" and the reasons are listed.
- **Every claim names the edition, the evidence and the known gaps.** A known deviation is something the app does differently from the standard, pinned by a test so that it cannot change unnoticed. A gap is something not done at all.
- **The runtime is a pinned library.** The SCORM run-time API and the SCORM 2004 sequencing engine are `scorm-again` 3.5.0 (MIT), vendored unmodified in `src-tauri/resources/scorm-again/`. Upgrading it is a behaviour change: every matrix below is rerun.

## Summary

| Standard | Edition | Claim | Evidence | Result |
|---|---|---|---|---|
| SCORM 1.2 | 1.2 | **Conformant, self-tested**, with known deviations | Conformance matrix at the player seam | 74 of 82 rows pass (66 requirements, 8 isolation escapes that fail to escape); 6 known deviations; 1 known limit; 1 control |
| SCORM 2004 | 4th Edition; 2nd and 3rd Edition packages run on the same runtime | **Partially conformant, self-tested.** Content aggregation conformant; run-time environment conformant with known deviations; sequencing and navigation partially conformant | The standards body's LMS test suite replayed at the player seam, plus a conformance matrix | 141 of 188 suite cases pass; 139 of the 184 sequencing cases |
| cmi5 | Quartz edition; assignable units packaged in the archive only | **cmi5 packaged assignable units, self-tested.** Not a conformant learning management system for remote units | CATAPULT LMS Test Suite replayed at the host seam, plus a conformance matrix | 84 of 100 rows pass (4 of them with a pinned deviation in part); 14 known deviations; 2 out of scope; no gaps |
| xAPI statements written by the app | 2.0, kept within the subset also valid in 1.0.3 | **Well-formed for xAPI 1.0.3 and 2.0** | Canonical statement tests, the builder's output posted to the hosted record store, and four external stores | Every statement accepted by the record store (which passes the official suite) and by four external stores |
| Hosted record store (Alexandria Cloud) | xAPI 1.0.3 and 2.0 (IEEE 9274.1.1-2023) | **Conformant, self-tested with the official xAPI conformance suite.** No certificate yet | ADL `lrs-conformance-test-suite` in continuous integration, plus a load test | 1365 of 1365 (1.0.3), 1435 of 1435 (2.0); statement queries at most 20.5 ms at 1,000,998 statements |

Manual checks the spec requires are not yet run: a physical iOS and Android device per phase, the identifier check repeated with real records, and the hosted xAPI run that issues the certificate. They are listed under [Not yet run](#not-yet-run) and will be recorded here.

## SCORM 1.2

**Claim:** SCORM 1.2 conformant, self-tested. Run-time environment with every mandatory and optional data model element, and content packaging import.

**Evidence:**

- Matrix: [tests/desktop-ui/scorm12-conformance.md](../tests/desktop-ui/scorm12-conformance.md). Every API call, every error code (0, 101, 201, 202, 203, 301, 401, 402, 403, 404, 405) and every data model element names a passing test, in Chromium and WebKit, in the `desktop-ui` CI job. A check fails the build if a row names a test that does not exist.
- Isolation: [tests/desktop-ui/package-isolation.spec.ts](../tests/desktop-ui/package-isolation.spec.ts) with hostile fixture packages, one escape each: a request off the package origin, a peer connection, a frame navigation, a pop-up, reading another launch's address, reading storage an earlier launch left, the command bridge, and covering the label in and out of focus mode. All fail to escape; a control proves the checks see an escape when the bootstrap is removed. A launch whose storage clearing fails or is spoofed does not start.
- Content packaging: `crates/alexandria-studio/tests/package_import.rs` (every refusal code, bounded and path-safe extraction).

**Known deviations** (all lenient validation in the runtime library; none stops valid content):

- `cmi.suspend_data` accepted up to 64,000 characters where the standard caps it at 4,096.
- Identifiers with a space accepted where the 1.2 test suite refuses them (405).
- Timespans with more than four digits of hours accepted.
- `student_response` and `pattern` not checked against the interaction type.
- An unknown element inside a known group fails with 401 where the 1.2 test suite expects 201.
- `LMSGetDiagnostic("")` still describes an earlier error after a later call succeeds (cosmetic).

**Known limit:** the wrapper is nested inside the app window. A lesson whose API search does not stop at the wrapper (a 2004-style search from a 1.2 lesson) climbs to the app window and gets a security error where an ordinary system would find nothing. Conforming discovery is unaffected.

**Not covered:** the standards body's SCORM 1.2 test suite needs a Java applet and is not driven; the test packages are purpose-built. No published sample package is committed.

## SCORM 2004 4th Edition

**Claim:** SCORM 2004 4th Edition partially conformant, self-tested. Packages built for the 2nd and 3rd Editions run on the same runtime.

| Area | Wording | Why |
|---|---|---|
| Content aggregation | Conformant, self-tested | Every one of the suite's 189 LMS test manifests imports; each edition's package imports with its edition |
| Run-time environment | Conformant, self-tested, with known deviations | API and error-state cases pass (`API`, `DMB`); the data model accepts some values it should refuse, and the suite's data model case (`DMI`) fails on exactly those |
| Sequencing and navigation | Partially conformant, self-tested | 139 of the 184 sequencing cases pass; the other 45 fail in the vendored sequencing engine |

**Evidence:**

- The SCORM 2004 4th Edition test suite 1.1.1 (Apache-2.0). Its LMS test lessons are Java applets no browser runs, so its data is replayed instead: the 189 manifests and 188 case scripts are vendored unchanged in `tests/desktop-ui/scorm2004-conformance/adl/`, each package is imported by Alexandria's importer, a probe lesson carries out each visit's calls under the suite's own evaluation rules, and the player's controls perform the navigation. 141 of 188 cases pass in Chromium and WebKit. The other 47 are pinned in `tests/desktop-ui/scorm2004-conformance/adl-known-failures.json` with every failing check, identical in both engines and across runs; each test passes only while its case fails exactly that way, so a fix or a regression both show.
- Matrix: [tests/desktop-ui/scorm2004-conformance.md](../tests/desktop-ui/scorm2004-conformance.md), 52 rows: 41 pass, 9 known deviations, 1 deviation by design, 1 known limit. Content aggregation rows name host-seam tests in `crates/alexandria-studio/tests/scorm2004_import.rs` and `scorm2004_conformance_packages.rs`.
- 2nd and 3rd Editions: purpose-built packages (`tests/desktop-ui/scorm2004-edition2/`, `scorm2004-edition3/`) run delivery, a rule on a global objective, launch data, passing score, a text completion threshold and course completion. No official suite exists for those editions in this form.

**The 47 failing suite cases:**

| Cases | What goes wrong | Area |
|---|---|---|
| 24 | The engine delivers a different activity from the one the suite expects after flow, retry, rollup or objective-map rules | Sequencing |
| 10 | Choice validity: `constrainChoice` applied to the wrong activity, `stopForwardTraversal` not stopping a choice, `choiceExit` and `forwardOnly` edge cases; no table-of-contents entry for the root | Navigation |
| 5 | The engine delivers nothing after attempt limits, forward-only flow or skipped activities | Sequencing |
| 4 | The sequencing session does not end when an exit or exit-parent rule ends the root's attempt; the player offers a choice instead | Sequencing |
| 2 | Objective completion, progress and scores read through global objective maps do not reach `cmi.objectives` | Sequencing |
| 1 (`DMI`) | Lenient validation: timestamps with a zone after minutes, more than two fraction digits or years past 2038, durations without `T`, identifiers such as `urn:`, unknown language tags and malformed response patterns are accepted; 403 or 0 for a read past the end of a list and 408 where the suite expects 351 | Run-time |
| 1 (`DDM`) | By design: the case expects data shared across packages; each package keeps its own, so one package never reads another's data | Run-time |

The 45 sequencing failures are in the vendored library. They are to be fixed upstream or by a vendored patch, never by a rewrite; until then the wording for sequencing and navigation stays "partially conformant". Upstream issues are not filed yet.

**Other known deviations:**

- `adl.nav.request_valid.continue` and `.previous` answer whether a request may be issued, not whether it would succeed. The player's own controls use the engine's look-ahead instead.
- Global objectives and shared data are scoped to the package (`objectivesGlobalToSystem` and `sharedDataGlobalToSystem` are not read), by design.
- `cmi.credit` is always `credit` and `cmi.mode` always `normal`: there is no browse or review launch.
- Library faults worked around in the wrapper, with the vendored files left unmodified: a stale error code after reading `completion_status` or `success_status`; an attempt duration limit read as the attempt's duration (no such activity was ever delivered); `SetValue` on a group name replacing part of the data model; launch values and comments kept from the previous lesson.
- Same nesting limit as SCORM 1.2.

**Not covered:** selection and randomisation, and `jump` requests (no suite case, no purpose-built package yet; the library's own tests cover them). Time limit rules measure only within one launch of the player: the host gives the engine no clock.

## cmi5

**Claim:** cmi5 packaged assignable units, self-tested. The app performs the launching system's duties for units packaged in the archive. It is not claimed as a conformant learning management system, and a course structure that launches a remote unit is refused at import.

**Evidence:**

- Matrix: [src-tauri/src/packages/attempts/cmi5/conformance.md](../src-tauri/src/packages/attempts/cmi5/conformance.md), 100 rows: 80 pass, 4 pass with a pinned deviation in part, 14 known deviations, 2 out of scope, 0 gaps. A test fails if a row names a test that does not exist.
- The CATAPULT LMS Test Suite (Apache-2.0, commit `806c0baa`). Runtime packages 001 to 009 are committed as their unmodified course structures in `src-tauri/tests/fixtures/catapult-lts/`. Each `lts_*` test imports the package through the app's import path, launches it, replays over real HTTP to the session's loopback endpoint the requests that package's unit makes, and asserts the unit's own checks and the suite's post-run checks. Import packages 101, 102 and 201 to 210 are in `crates/alexandria-studio/tests/cmi5_import.rs`.
- Session rules: `crates/alexandria-studio/tests/cmi5_session.rs`. Player: `tests/desktop-ui/cmi5-player.spec.ts` runs a built-in cmi5 package to completion in Chromium and WebKit against a fake endpoint.

**Known deviations:**

- The on-device endpoint is not a conformant record store: statement reads filter but never page, `voidedStatementId` finds nothing, reads are scoped to the attempt, and no account can read every statement across sessions.
- A statement without an id or timestamp gets one from the endpoint instead of being refused.
- Statements that break the session rules are refused with 403, which the specification permits for the unit's credentials, rather than voided. Nothing is ever voided.
- No grace period after `terminated`: the session takes nothing more at once. After a session ends the endpoint is gone, so a late request fails to connect rather than receiving 401 or 403.
- Launch mode is always Normal: no Browse or Review offer (the rules for those modes pass in the session tests). Nothing is ever waived.
- `initialized` is accepted before the learner preferences document is read (requirement 11.0.0.0-3, derived test). Refusing it could break units that never read it; owner decision pending.
- The unit may write the learner preferences document; only `LMS.LaunchData` is refused to it.
- Course structure element order is not checked against the schema; vendor extensions are ignored rather than validated.
- On mobile, a unit that asks for its own window plays in the package frame with focus mode forced on. Tauri mobile has one webview, and redirecting it would run the unit in the window that holds the app's capabilities. A unit that refuses to run in a frame will not work on mobile. On desktop it gets its own window, which has no commands and cannot navigate outside its launch.

**Out of scope:** remote assignable units (including the published examples in the cmi5 specification, which launch only remote units, and so are refused as published), course export, and requirements on the unit side.

**Not covered:** the units' own JavaScript is not run; CATAPULT's automated mode needs a browser driving a Rust host and is not wired. The desktop own window itself (its chrome, closing it) is a manual check. An author's preview of a cmi5 element is not wired and shows a load error; this is not a cmi5 requirement.

## xAPI statements written by the app

**Claim:** learning records are well-formed xAPI statements, valid under both 1.0.3 and 2.0. The output types can express only the subset valid in both (no context agents or groups, sub-statements, attachments or per-statement signatures; UTC timestamps; durations to the hundredth of a second), so one statement serves both versions. Verbs come from the ADL and cmi5 vocabularies; SCORM run-time data maps to statements following the SCORM profile for xAPI.

**Evidence:**

- Identifier check (ticket 12, before the builder was written): 60 hand-built statements per version carrying the identifier scheme, sent to four open-source stores from four vendors (SQL LRS 0.9.14, the ADL reference LRS, the Learning Locker xAPI service, Ralph 5.1.0). Every store accepted every DID URL activity identifier form and returned it byte for byte; filtering by activity, agent and registration worked. The official suite, with its templates rewritten to use the scheme, passed 1365 of 1365 (1.0.3) and 1435 of 1435 (2.0) on a fresh SQL LRS. The canonical statements from that check are committed in `crates/alexandria-studio/fixtures/xapi-canonical/`; the raw runs are kept with the research notes, outside this repository.
- Builder tests: `crates/alexandria-studio/tests/xapi_statements.rs` and `xapi_scorm.rs` reproduce all nine canonical statements exactly for both versions, and refuse invalid input before storage with stable codes.
- Agreement with the record store: `crates/alexandria-studio/fixtures/xapi-store/builder-output.json` (21 statements: every action, the SCORM and cmi5 profiles, a SCORM 2004 session, every actor choice, a private course, and the identifier edges up to exactly 2083 characters) is kept equal to the builder's output by `xapi_store_fixture.rs`. The hosted service holds a byte-for-byte copy and posts it in both versions (`alexandria-cloud`: `tests/xapi_app_statements.rs`); every statement is accepted, reads back intact, and is found by actor, activity, related course and registration.

**Known deviations and limits:**

- The account home page of the actor is a fixed `urn:uuid`, not a dereferenceable web address. xAPI calls it an IRL; neither the stores tested nor the suite check that it resolves, and it keeps identifiers free of any web domain.
- From the SCORM profile: interactions use the verb `responded`; no session statements (`initialized`, `terminated`, `suspended`, `resumed`); no attempt activity in the grouping, since the registration identifies the attempt.
- No per-statement signature: xAPI allows only RS256, RS384 and RS512, which the learner's Ed25519 key cannot produce. An export of the activity log carries a detached Ed25519 signature over the whole file instead (format `alexandria-record-export/1`).
- Activity identifiers are capped at 2083 characters and account names at 50, the lowest limits seen in the stores tested; longer package or interaction identifiers fall back to a digest form.
- A SCORM value that does not parse (an out-of-range score, a malformed time) is left out of the record rather than refusing it.

**Records leaving the device.** A record destination is spoken to directly: the version is negotiated from the store's `about` resource (2.0.0 preferred, else 1.0.3) and only HTTP Basic credentials are supported. Known gaps: statements a cmi5 unit writes beyond the defined verbs are in the activity log, its export, its size limit and delete-all, but are not sent to record destinations or record shares. A record share carries the learner's DID beside the per-recipient identifier and is signed with the learner's key, so the organisation can link the two; the per-recipient identifier only stops two recipients matching records (owner decision pending). Record destinations are refused for supervised profiles until a guardian approval step exists. Records from private courses always carry names until private courses exist.

## Hosted record store

**Claim:** Alexandria Cloud's record store is conformant to xAPI 1.0.3 and 2.0, self-tested with the official xAPI conformance suite in continuous integration. No certificate has been issued: the hosted run against a deployed store is still to do.

**Evidence** (in the `alexandria-cloud` repository):

- `scripts/xapi-conformance.sh` fetches ADL's `lrs-conformance-test-suite` at a pinned commit (`5bc232d3`) with its packages from a lock file, serves the record store on a loopback port with a fresh organisation and interface credential per version, and fails on any failed test or a run that stops early. The CI job "xAPI conformance and load" runs it on every change. Result: xAPI 1.0.3 1365 of 1365, xAPI 2.0 1435 of 1435, under Node 22 (the CI version) and Node 26. A wrong credential turns it red.
- Integration tests against Postgres for each resource in both versions, tenant isolation, read and write scope, share-scope enforcement at ingest, deletion on revoke, the end-of-enrolment rule, access logging, forwarding and retry.
- Load test (`scripts/xapi-load.sh`, `tests/xapi_load.rs`, run in release in CI too): 1,000,998 statements across 20 organisations, the measured one holding 500,500 from 5,000 learners in 40 courses; Postgres 16 with default settings on a laptop, release build. Slowest of five runs per query through the HTTP interface: 20.5 ms (activities resource); every statement query 15.0 ms or less, paging through `more` 11.7 ms per page. The test fails if any query takes over 500 ms or any statement query's plan scans the statements table; CI repeats it at 200,000 statements. Writes: 516 statements per second in batches of 100, recorded, not bounded. With shared records in the mix at 200,000 statements, the slowest query took 13.6 ms.

**Resources:** statements (PUT, POST, GET with every query filter, paging, voiding, attachments, signed statements, the 1.0.3 alternate request syntax), activity state, activity profile and agent profile documents with ETag concurrency control, agents, activities and about. One endpoint base per organisation, HTTP Basic interface credentials with read or write scope, tenant isolation by row-level security in the data layer.

**Known deviations and gaps:**

- Any absolute IRI is taken as an account home page, not only an IRL, so the app's `urn:uuid` home page is accepted. The reference store and the three others tested do the same; a home page with no scheme is still refused, as the suite checks.
- The 1.0.3 alternate request syntax is served for statements only, not for the document resources (the suite does not exercise it there).
- A DELETE of all states with `If-Match` or `If-None-Match` is refused with 400, since a precondition names one document.
- A signed statement with no `x5c` header is accepted on structure; certificate chains are not checked against any trust root.
- No CORS on `/xapi/`, so a browser-based client on another origin cannot call the store. `/xapi/` is rate limited per credential (or per address when none is presented), and failed sign-ins per address; a deployment outside the organisation's residency region refuses the store with 403.
- Statements an organisation's own tools write are not forwarded to its external store; only shared records are.
- Narrowing a record share deletes the records already received from courses or record kinds narrowed away, keeps only their identifiers so they are never stored again, and voids any copies forwarded to the external store; dropping records from a share entirely works like a revoke. Revoking deletes them all. The owner may still choose to keep narrowed-away records instead.
- Limits: request bodies 16 MiB, documents 4 MiB, 500 statements per page. Retention is the service's fixed 180 days; there is no per-organisation policy yet.
- Load test limits: shared records (which add a share and enrolment check per row) are not in the load mix; the document resources are not load-tested.

## Isolation and platform notes

These are not conformance requirements of any standard, but a reviewer will ask.

- Package content runs under its own `package` scheme, in a frame, with a launch-scoped unguessable path prefix, a content policy that confines requests to the package's origin, an injected bootstrap that starts the document and removes the command bridge and network-capable interfaces, storage cleared before and after every launch, and an app-drawn label. Every package shares one origin on every platform; the prefix and storage clearing are what separate them.
- Residual escapes, not fixed: a closed declarative shadow root holding a `srcdoc` frame can still reach a peer connection (disabling WebRTC at the webview would close it); in WebKit, a fetch preload inside an XML document parsed through `responseXML` and adopted into a page.
- A package result is a self-report from untrusted script. It never becomes a graded submission, credential evidence or a reputation input; database triggers refuse it, and a course made only of package elements issues no credential.
- A launch waits until the whole archive is extracted; the spec's "the first lesson can start once its own files are extracted" is not met.
- A package in a private course cannot be played yet (its archive is encrypted); private courses do not exist yet.
- Package attempts are device-local: an attempt does not resume on another device.

## Not yet run

| Check | Required by | State |
|---|---|---|
| A physical iOS and Android device per phase: a package with large video, focus mode, ranged media at the custom scheme, video from the custom scheme in WKWebView, storage clearing reporting success, the cmi5 loopback endpoint (cleartext to `127.0.0.1` only on Android, no Local Network prompt, survival of backgrounding and process death) | Spec, Testing Decisions | Not run |
| Windows WebView2 and Linux WebKitGTK runs of packages and of the cmi5 endpoint | Ticket 23 | Not run |
| CATAPULT's manual procedure on a dev build (the units' own JavaScript, the desktop own window) | Ticket 26 | Not run |
| The identifier check repeated with real records against external stores | Spec, Testing Decisions | Not run |
| The hosted xAPI conformance run against a deployed record store, which issues the certificate | Spec, What conformance means here | Not run |

Each result is recorded here when it is run ([ticket 28](learning-standards/28-device-and-hosted-runs.md)).

## Not claimed

Exporting Alexandria courses as SCORM or cmi5 packages; LTI, AICC and Common Cartridge; remote cmi5 units and any package content loaded from the network; resuming a package attempt on another device; the xAPI per-statement signature format; independent certification of SCORM or cmi5; the on-device cmi5 endpoint as a record store.
