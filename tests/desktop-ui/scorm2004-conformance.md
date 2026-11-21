# SCORM 2004 conformance matrix

Status: self-tested. Claim wording: "SCORM 2004 4th Edition conformant, self-tested", never "certified"; packages built for the 2nd and 3rd Editions run on the same runtime.

Every row names the automated test that proves it, in «guillemets». Browser tests run at the player seam (the real player, wrapper and vendored runtime with its sequencing engine, and a fake host) in Chromium and WebKit, in the `desktop-ui` CI job; content aggregation runs at the host seam in Rust:

- `scorm2004-conformance.spec.ts`: the replay of the standards body's test suite, and the purpose-built checks. Purpose-built packages: `scorm2004-conformance/package/` (4th Edition probe), `scorm2004-edition2/`, `scorm2004-edition3/`.
- `package-player-2004.spec.ts`: the player's own SCORM 2004 lifecycle (ticket 10).
- `package-isolation.spec.ts` and `scorm12-conformance.spec.ts`: isolation and the wrapper behaviour both standards share.
- `crates/alexandria-studio/tests/scorm2004_import.rs`, `scorm2004_conformance_packages.rs`, `package_import.rs`: content aggregation.

A test titled `x: ${variable}` is named by its fixed part. «every row of the conformance matrix names a test that exists» fails if a named test is missing, if a replayed case named in a row does not exist, if a row's status hides a pinned failure, or if a case is in no row.

Runtime: `scorm-again` 3.5.0 (see `src-tauri/resources/scorm-again/README.md`). Rerun this matrix on every upgrade.

## The standards body's test suite

The SCORM 2004 4th Edition test suite (version 1.1.1) is published under the Apache License 2.0. Its LMS test is 188 test cases over 189 content packages; each case is a script of run-time calls with expected results, user interface questions, and the navigation the tester performs, in the order the LMS must deliver the activities. The lessons that carry the scripts out are Java applets, which no current browser runs, so the suite cannot be run as shipped. Its data can: the manifests and case scripts are vendored unchanged in `scorm2004-conformance/adl/` (see the README there), and `adlLmsTest.ts` reads each case as the suite's own Java does. The replay imports each package with Alexandria's importer, serves a probe lesson in place of every lesson page, carries out each visit's calls and checks them with the suite's evaluation rules, answers the user interface questions from what the player shows, and performs each navigation with the player's controls. The suite's SCO, manifest and content package tests are for content, not for a player, and are not used.

141 of the 188 cases pass in both engines. Each of the other 47 is pinned in `scorm2004-conformance/adl-known-failures.json` with its row, why, and every failing check, and its test passes only while it fails exactly that way; `scorm2004-conformance/known-failures.mjs` regenerates the file from a recorded run.

How the replay answers for the tester, where the suite leaves it to a person:

- A control the package hides counts as disabled: the suite's own answers treat it so (CM-01 and OB-04 hide Continue and expect "a disabled Continue").
- A lesson shows its instruction and terminates at once, so when its own navigation request already delivered an activity or ended the session, the tester has nothing to navigate; a relaunch still happens.
- "Log out and relaunch" is leaving the course and coming back, which suspends the package.
- The learner the suite expects is the player's pseudonymous learner.

## Content aggregation

| Requirement | LMS | Status | Test |
|---|---|---|---|
| Every LMS test package of the test suite imports: organizations, items, resources, `xml:base`, item parameters, sequencing, presentation, completion thresholds, data maps | M | passes | «every_lms_test_package_of_the_scorm2004_test_suite_imports_and_its_description_is_stored» |
| A package of each edition (2nd: CAM 1.3, 3rd, 4th) imports with its edition; a manifest naming no edition imports without one | M | passes | «a_scorm2004_package_from_each_edition_imports_with_its_edition» «a_scorm2004_manifest_that_names_no_edition_imports_without_one» |
| Sequencing definition of each activity: control modes, rules, limit conditions, rollup rules and considerations, objectives and maps, extended (adlseq) objectives, randomization and delivery controls, constrained choice | M | passes | «a_4th_edition_package_keeps_its_sequencing_on_each_activity» «a_sequenced_description_survives_being_stored_as_json» |
| Sequencing collections (`IDRef`) merged into each activity | M | passes | «a_4th_edition_package_keeps_its_sequencing_on_each_activity» |
| 3rd Edition completion threshold as text; 2nd Edition sequencing | M | passes | «a_3rd_edition_package_keeps_its_sequencing_and_its_text_completion_threshold» «a_2nd_edition_package_keeps_its_sequencing» |
| Objective identifiers are URIs: escapes decoded and white space collapsed before comparing (test suite OB-02a, OB-12a) | M | passes | «objective_ids_are_kept_decoded_and_with_white_space_collapsed» |
| Values in the wrong case are read; malformed sequencing is refused with a reason naming the activity | M | passes | «values_written_in_the_wrong_case_are_still_read» «malformed_sequencing_is_refused_as_unparseable» «a_refusal_for_broken_sequencing_names_the_activity» |
| A missing, remote or unsafe launch resource and the size limits are refused | M | passes | «the_package_refusals_apply_to_scorm2004_with_its_edition_attached» |
| An edition this build does not know is refused | M | passes | «an_edition_of_scorm_2004_this_build_does_not_know_is_refused_as_unsupported» |

## Run-time API

| Requirement | LMS | Status | Test |
|---|---|---|---|
| API discovery: `API_1484_11` found by the published algorithm (parents, then the opener) | M | passes | «host values: a lesson reads the learner, credit, mode, entry and every value its manifest gives it» |
| The eight functions, state errors before and after initialisation, `GetErrorString` and `GetDiagnostic` for known and unknown codes, as the test suite checks them: `API` | M | passes | «ADL LMS test package » |
| Before `Initialize`: `Terminate` 112, `GetValue` 122, `SetValue` 132, `Commit` 142; `Initialize("x")` 201; a second `Initialize` 103 | M | passes | «API: before Initialize every call but the error functions fails with its own code; Initialize takes only an empty argument, once» |
| After `Terminate`: `GetValue` 123, `SetValue` 133, `Commit` 143, `Terminate` 113, `Initialize` 104; nothing more reaches the host | M | passes | «API: after Terminate every call but the error functions fails with the after-termination codes and nothing reaches the host» |
| A successful read sets error 0, `completion_status` and `success_status` included | M | passes (the wrapper works around the runtime) | «API: a successful read of completion_status or success_status reports error 0» |
| `Commit` and `Terminate` hand the whole run-time state to the host | M | passes | «the host receives the whole run-time state on commit and on terminate» |
| A lesson closed without `Terminate` still hands over its state, and the engine's state goes to the host | M | passes | «a lesson left without terminating still hands over its state when the player closes» |
| Commits are coalesced: one host write per interval, the latest state, at once on termination (shared wrapper code) | M | passes | «commits are coalesced: one host write per interval with the latest state, and at once on finish» |
| A search for a name the wrapper does not define (a 1.2-style search from a 2004 lesson) climbs to the app window and throws, where a same-origin system would find nothing | | known limit of nesting the wrapper in the app | «known limit: a search for the SCORM 1.2 API from a 2004 lesson climbs past the wrapper and is refused by the app window» |

## Error codes

| Code | Meaning | Status | Test |
|---|---|---|---|
| 0 | No error | passes | «ADL LMS test package » (`API`) «API: a successful read of completion_status or success_status reports error 0» |
| 103, 104 | Already initialized; content instance terminated | passes | «API: before Initialize every call but the error functions fails with its own code; Initialize takes only an empty argument, once» «API: after Terminate every call but the error functions fails with the after-termination codes and nothing reaches the host» |
| 112, 113, 122, 123, 132, 133, 142, 143 | Termination, retrieve, store, commit before initialization or after termination | passes | «API: before Initialize every call but the error functions fails with its own code; Initialize takes only an empty argument, once» «API: after Terminate every call but the error functions fails with the after-termination codes and nothing reaches the host» |
| 201 | General argument error | passes | «ADL LMS test package » (`API`) |
| 301, 351 | General get and set failure (empty element) | passes | «ADL LMS test package » (`API`) |
| 401 to 408 | Undefined element, unimplemented, not initialized, read only, write only, type mismatch, out of range, dependency not established; with 301 for a read past the end of a list | known deviation: 403 or 0 for a read past the end of a list, 408 where the suite expects 351 (pinned) | «ADL LMS test package » (`DMI`) |
| 401 | A group name (`cmi`, `cmi.objectives`, `adl.data`) is no element | passes (the wrapper refuses it; the runtime would replace part of its data model) | «data model: a group name such as cmi or cmi.objectives is no element (401) and setting it changes nothing» |

## Data model

The test suite's data model implementation case (`DMI`, 8,000 checks) sets and reads every element with valid and invalid values, and its behaviour case (`DMB`) checks status rules, resume and time across sessions. `DMI` is pinned: its rows below pass except for the validation relaxations named.

| Requirement | LMS | Status | Test |
|---|---|---|---|
| Every element's access, vocabulary, range and format checks, `_children`, `_count`, `_version` (`DMI`) | M | known deviation: accepts timestamps with a zone after minutes, more than two fraction digits or years past 2038, durations without T, identifiers such as `urn:`, unknown language tags and malformed response patterns (pinned) | «ADL LMS test package » |
| Behaviour: success status from the passing score and score, completion from the threshold and progress, entry and total time across sessions, an earlier lesson's values never reaching a later one (`DMB`) | M | passes (the wrapper clears what the runtime keeps between lessons; the host counts a resumed lesson's own attempt time) | «ADL LMS test package » |
| `cmi.learner_id` (pseudonymous), `cmi.learner_name`, `cmi.credit` (always `credit`), `cmi.mode` (always `normal`), `cmi.entry`, `cmi.launch_data` (manifest `adlcp:dataFromLMS`), `cmi.scaled_passing_score` (primary objective's minimum measure when satisfied by measure), `cmi.max_time_allowed` (attempt absolute duration limit), `cmi.time_limit_action`, `cmi.completion_threshold`, `cmi.total_time`, the manifest's objective ids; all read-only (404) | M | passes | «host values: a lesson reads the learner, credit, mode, entry and every value its manifest gives it» |
| A lesson its manifest gives nothing reads nothing (403 or the default), not the lesson before it's values or comments | M | passes (the wrapper works around the runtime) | «host values: a lesson its manifest gives nothing reads nothing, not the values of the lesson before it» |
| Entry and data on relaunch: `resume` with the saved data and the attempt's time after `suspend`; any other exit ends the learner attempt and the next launch is `ab-initio` with new data | M | passes | «a suspended lesson resumes with its data; a lesson that ends without suspend starts a new attempt with none» |
| A primary objective without an identifier still gives the passing score and is not among `cmi.objectives` | M | passes | «a primary objective without an identifier still sets the passing score and is not among the lesson objectives» |
| `adl.nav.request` and lesson-driven navigation: every case that navigates itself (`SX-03` and the other automatic cases) | M | passes | «ADL LMS test package » |
| `adl.nav.request_valid.continue` and `.previous` | M | known deviation: answer whether the request may be issued, not whether it would succeed (true where the next activity is disabled); the player's own controls use the engine's look-ahead | «known deviation: adl.nav.request_valid.continue answers true where the next activity is disabled» |
| `adl.data` stores, read and write through the manifest's data maps, within one package | M | known deviation: the case continues in a second package and expects data shared across packages; each package keeps its own (pinned) | «ADL LMS test package » (`DDM`) |

## Sequencing and navigation

Rows that pass list the test suite's passing cases; the cases that do not pass are in the rows after them, grouped by what goes wrong.

| Requirement | LMS | Status | Test |
|---|---|---|---|
| Control modes, flow and choice navigation, pre-condition rules and the run-time values a delivery starts with: `CM-01` `CM-02a` `CM-02b` `CM-03a` `CM-04a` `CM-04d` `CM-05` `CM-08` `CM-09ab` `CM-09bb` `CM-09cb` `CM-10` `CM-11` `CM-13` `CM-14` `CM-15` | M | passes | «ADL LMS test package » |
| Completion and objective data through extended (adlseq) objective maps: `CO-01` `CO-02b` `CO-06` `CO-08a` `CO-08b` `CO-09` `CO-11` `CO-12a` `CO-12b` `CO-12c` `CO-13b` | M | passes | «ADL LMS test package » |
| Completion thresholds and completion by progress measure: `CT-01` `CT-02` `CT-03` `CT-04` `CT-05` `CT-06` `CT-07` | M | passes | «ADL LMS test package » |
| Measures: satisfaction by measure, measure rules, weighted measure rollup: `MS-01` `MS-02` `MS-03` `MS-04` `MS-05b` `MS-07` | M | passes | «ADL LMS test package » |
| Objectives: primary and local objectives, global objective maps, reading and writing satisfaction and measure: `OB-01a` `OB-01b` `OB-01c` `OB-02a` `OB-02b` `OB-03a` `OB-03b` `OB-04` `OB-05a` `OB-05b` `OB-05c` `OB-06` `OB-07a` `OB-07b` `OB-10a` `OB-10b` `OB-10c` `OB-10d` `OB-11a` `OB-11b` `OB-12a` `OB-12b` `OB-12c` `OB-13a` `OB-13b` `OB-13c` `OB-14a` `OB-14b` `OB-15` `OB-16a` `OB-16b` `OB-16c` `OB-16d` `OB-17a` `OB-17b` | M | passes | «ADL LMS test package » |
| Rollup: rollup rules, child activity sets, rollup considerations, tracked and set-by-content controls: `RU-01aa` `RU-01ab` `RU-01ba` `RU-01bb` `RU-02a` `RU-02b` `RU-03a` `RU-03b` `RU-04aa` `RU-04ab` `RU-04ba` `RU-04bb` `RU-04bc` `RU-04bd` `RU-05a` `RU-05b` `RU-06a` `RU-06b` `RU-07a` `RU-07b` `RU-08a` `RU-08b` `RU-09` `RU-12a` `RU-12b` `RU-13a` `RU-13b` `RU-13c` `RU-13e` `RU-14a` `RU-14b` `RU-14c` `RU-14d` `RU-15a` `RU-15c` `RU-15d` `RU-16` `RU-17a` `RU-17b` `RU-18a` `RU-18b` `RU-19a` | M | passes | «ADL LMS test package » |
| Sequencing rules: exit, post-condition (retry, continue, previous, exit parent, exit all) and their order: `SX-02` `SX-03` `SX-04a` `SX-05` `SX-06` `SX-07a` `SX-07b` `SX-07c` `SX-07d` `SX-07e` `SX-08a` `SX-08b` `SX-09` `SX-10a` `SX-10b` `SX-10c` `SX-10d` `SX-11a` `SX-11b` `SX-12a` `SX-12b` `SX-12c` | M | passes | «ADL LMS test package » |
| Choice: what the table of contents and controls offer under choice, choiceExit, forwardOnly, constrainChoice, preventActivation, stopForwardTraversal, disabled and hidden-from-choice rules, and choosing the root: `CM-07a` `CM-07b` `CM-07c` `CM-07d` `CM-07e` `CM-07f` `CM-16` `CM-17a` `CM-17b` `SX-04b` | M | known deviation: the runtime's choice validity (constrainChoice applied to the wrong activity, stopForwardTraversal not stopping a choice, choiceExit and forwardOnly edge cases); no entry for the root of the activity tree (pinned) | «ADL LMS test package » |
| Delivery: the activity delivered after flow, retry, rollup and objective-map rules: `CM-04b` `CM-06` `CM-09aa` `CM-09ba` `CM-09ca` `CO-02a` `CO-03` `CO-04a` `CO-04b` `CO-04c` `CO-05a` `CO-05b` `CO-07b` `CO-12d` `CO-13a` `MS-05a` `MS-06` `OB-03c` `OB-09b` `RU-10` `RU-11` `RU-13d` `RU-15b` `RU-19b` | M | known deviation: the engine delivers a different activity from the suite's (pinned) | «ADL LMS test package » |
| Delivery after attempt limits, forward-only flow and skipped activities: `CM-03b` `CM-04c` `RU-07c` `T-01a` `T-01b` | M | known deviation: the engine delivers nothing where the suite expects an activity (pinned) | «ADL LMS test package » |
| Ending the sequencing session when an exit rule or exit-parent rule ends the attempt on the root: `CO-07a` `OB-08a` `OB-08b` `OB-09a` | M | known deviation: the session does not end; the player offers a choice (pinned) | «ADL LMS test package » |
| Objective completion, progress and scores read through global objective maps into `cmi.objectives`: `CO-10` `SX-11c` | M | known deviation: the values do not reach `cmi.objectives` (pinned) | «ADL LMS test package » |
| Start, continue, previous, choice, suspend all and resume all from the player's controls, a locked lesson, a delivery that offers nothing, completion from the root's rollup | M | passes | «a lesson the package locks cannot be reached until the lesson before it is passed» «suspend and resume restore the sequencing engine and the lesson where it was left» «when starting delivers nothing the learner chooses a lesson» «the package completes when the sequencing engine reports the course completed» |
| Attempt absolute duration limit: an activity with one is delivered and the content reads it as `cmi.max_time_allowed` (test suite `CM-01`) | M | passes (the wrapper works around the runtime) | «ADL LMS test package » |
| Global objectives are scoped to the package (`objectivesGlobalToSystem` is not read) | M | known deviation by design: one package never reads another's data | «ADL LMS test package » (`DDM`) |

## Editions and preview

| Requirement | LMS | Status | Test |
|---|---|---|---|
| A 2nd Edition (CAM 1.3) package: delivery, a pre-condition rule on a global objective, launch data, passing score, course completion | M | passes | «a SCORM 2004 2nd Edition package runs on the same runtime: delivery, a rule on a global objective, completion» |
| A 3rd Edition package: the same, and a text completion threshold completing a lesson by progress measure | M | passes | «a SCORM 2004 3rd Edition package runs on the same runtime: its completion threshold completes a lesson by progress» |
| An author's preview runs a 2004 package under its sequencing rules, holding the engine's state through the preview commands and recording no attempt | M | passes | «preview runs a SCORM 2004 package under its sequencing rules through the preview commands only» |

## Not covered here

- Selection and randomisation, and `jump` requests: the test suite has no case for them and no purpose-built package covers them yet. The runtime library's own unit tests cover both.
- Time limit rules (`timeLimitExceeded`) take the runtime's own clock and the content's reported times; the host gives the engine no clock (`getAttemptElapsedSeconds` is not wired), so a limit is measured within one launch of the player only.
- Auxiliary resources: SCORM 2004 tells players to ignore them.
- Device runs (iOS, Android) are manual and recorded in the conformance statement.
