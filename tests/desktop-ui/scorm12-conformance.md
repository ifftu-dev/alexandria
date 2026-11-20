# SCORM 1.2 conformance matrix

Status: self-tested. Claim wording: "SCORM 1.2 conformant, self-tested", never "certified".

Every row names the automated test that proves it, in «guillemets». The tests run at the player seam (the real player, wrapper and vendored runtime with a fake host) in Chromium and WebKit, in the `desktop-ui` CI job:

- `scorm12-conformance.spec.ts`: run-time API, data model, status rules, commits, ranged media. Probe package: `scorm12-conformance/package/`.
- `package-isolation.spec.ts`: isolation, with hostile fixtures in `package-isolation/package/`.
- `package-player.spec.ts`: the player's own lifecycle (ticket 06).

A test titled `x: ${variable}` is named by its fixed part. «every row of the conformance matrix names a test that exists» fails if a named test is missing.

Expected values come from the SCORM 1.2 run-time environment and the validators of the standards body's 1.2 test suite (error-code model and bounds as tabulated in the runtime library's `SCORM12_COVERAGE.md` at the pinned tag), not from the library. LMS column: M = the LMS must implement it, O = optional; every optional element is implemented.

Runtime: `scorm-again` 3.5.0 (see `src-tauri/resources/scorm-again/README.md`). Rerun this matrix on every upgrade.

## Run-time API

| Requirement | LMS | Status | Test |
|---|---|---|---|
| API discovery: `API` found from the lesson by the published algorithm | M | passes | «API: the lesson finds the API by the published discovery algorithm» |
| `LMSInitialize("")` starts a session; a second call fails (101); a non-empty argument fails (201) | M | passes | «API: LMSInitialize starts a session once; an argument is refused with 201 and a second call with 101» |
| `LMSGetValue`, `LMSSetValue`, `LMSCommit`, `LMSFinish` before `LMSInitialize` fail (301) | M | passes | «API: every call but the error functions fails with 301 before LMSInitialize» |
| `LMSCommit("")` and `LMSFinish("")` take only an empty argument (201) | M | passes | «API: LMSCommit and LMSFinish take only an empty argument (201)» |
| `LMSFinish("")` ends the session; later calls fail (301) and reach nothing | M | passes | «API: after LMSFinish every call but the error functions fails with 301 and nothing more reaches the host» |
| `LMSGetValue(element)` | M | passes | «core: the host-supplied elements read back and refuse writes with 403» |
| `LMSSetValue(element, value)` | M | passes | «core: lesson_location keeps up to 255 characters and refuses more (405)» |
| `LMSCommit("")` hands the whole state to the host | M | passes | «the host receives the whole run-time state on commit and on finish» |
| `LMSGetLastError()` reports the last call, 0 after a success | M | passes | «API: LMSGetLastError reports the last call and LMSGetErrorString and LMSGetDiagnostic describe every code» |
| `LMSGetErrorString(code)` describes every code | M | passes | «API: LMSGetLastError reports the last call and LMSGetErrorString and LMSGetDiagnostic describe every code» |
| `LMSGetDiagnostic(code)` returns detail | M | passes | «API: LMSGetLastError reports the last call and LMSGetErrorString and LMSGetDiagnostic describe every code» |
| `LMSGetDiagnostic("")` after a later success | M | known deviation: still returns the earlier error's text (cosmetic) | «known deviation: LMSGetDiagnostic still describes an earlier error after a later call succeeds» |

## Error codes

| Code | Meaning | Status | Test |
|---|---|---|---|
| 0 | No error | passes | «API: LMSGetLastError reports the last call and LMSGetErrorString and LMSGetDiagnostic describe every code» |
| 101 | General exception (second `LMSInitialize`) | passes | «API: LMSInitialize starts a session once; an argument is refused with 201 and a second call with 101» |
| 201 | Invalid argument (non-empty argument, index past the end of a list) | passes | «API: LMSCommit and LMSFinish take only an empty argument (201)» «objectives: every element round trips, is counted and validated» |
| 201 | Unknown element inside a known group | known deviation: 401 | «known deviation: an unknown element in a known group fails with 401 where the 1.2 test suite expects 201» |
| 202 | Element cannot have children | passes | «core: _count on an element that is not a list fails with 203 and _children on a leaf with 202» |
| 203 | Element not an array, cannot have count | passes | «core: _count on an element that is not a list fails with 203 and _children on a leaf with 202» |
| 301 | Not initialized (before `LMSInitialize`, after `LMSFinish`) | passes | «API: every call but the error functions fails with 301 before LMSInitialize» «API: after LMSFinish every call but the error functions fails with 301 and nothing more reaches the host» |
| 401 | Not implemented (unknown group) | passes | «data model: an unknown group fails with 401» |
| 402 | Invalid set value, element is a keyword | passes | «core: _children lists the standard elements and a keyword cannot be set (402)» |
| 403 | Element is read only | passes | «core: the host-supplied elements read back and refuse writes with 403» |
| 404 | Element is write only | passes | «core: exit and session_time are write-only (404) and refuse values outside their formats (405)» |
| 405 | Incorrect data type | passes | «core: score raw, min and max take a number from 0 to 100 or blank and refuse anything else (405)» |

## Data model

| Element | LMS | Access | Status | Test |
|---|---|---|---|---|
| `cmi.core._children` | M | RO | passes | «core: _children lists the standard elements and a keyword cannot be set (402)» |
| `cmi.core.student_id` (pseudonymous, from the host) | M | RO | passes | «core: the host-supplied elements read back and refuse writes with 403» |
| `cmi.core.student_name` | M | RO | passes | «core: the host-supplied elements read back and refuse writes with 403» |
| `cmi.core.lesson_location` (CMIString255) | M | RW | passes | «core: lesson_location keeps up to 255 characters and refuses more (405)» |
| `cmi.core.credit` (always `credit`) | M | RO | passes | «core: the host-supplied elements read back and refuse writes with 403» |
| `cmi.core.lesson_status` (vocabulary; `not attempted` refused) | M | RW | passes | «core: lesson_status takes its vocabulary and refuses anything else, not attempted included (405)» |
| `cmi.core.entry` (`ab-initio`, `resume`, empty) | M | RO | passes | «core: the host-supplied elements read back and refuse writes with 403» «a suspended lesson resumes with entry resume, its saved data and the time of finished sessions» «a lesson left without suspending comes back with an empty entry» |
| `cmi.core.score._children` | M | RO | passes | «core: _children lists the standard elements and a keyword cannot be set (402)» |
| `cmi.core.score.raw` (0 to 100 or blank) | M | RW | passes | «core: score raw, min and max take a number from 0 to 100 or blank and refuse anything else (405)» |
| `cmi.core.score.max` | O | RW | passes | «core: score raw, min and max take a number from 0 to 100 or blank and refuse anything else (405)» |
| `cmi.core.score.min` | O | RW | passes | «core: score raw, min and max take a number from 0 to 100 or blank and refuse anything else (405)» |
| `cmi.core.total_time` (sum of finished sessions, from the host) | M | RO | passes | «core: the host-supplied elements read back and refuse writes with 403» «a suspended lesson resumes with entry resume, its saved data and the time of finished sessions» |
| `cmi.core.lesson_mode` (always `normal`: no browse or review launch is offered) | O | RO | passes | «core: the host-supplied elements read back and refuse writes with 403» |
| `cmi.core.exit` (vocabulary) | M | WO | passes | «core: exit and session_time are write-only (404) and refuse values outside their formats (405)» |
| `cmi.core.session_time` (CMITimespan) | M | WO | passes | «core: exit and session_time are write-only (404) and refuse values outside their formats (405)» |
| CMITimespan hours above 9999 | | | known deviation: accepted | «known deviation: a timespan may have more than four digits of hours where the standard allows at most 9999» |
| `cmi.suspend_data` (CMIString4096) | M | RW | passes | «suspend_data keeps 4096 characters and reads them back» |
| `cmi.suspend_data` above 4096 characters | M | RW | known deviation: accepted up to 64000 | «known deviation: suspend_data accepts up to 64000 characters where the standard caps it at 4096» |
| `cmi.launch_data` (manifest `adlcp:datafromlms`) | M | RO | passes | «launch_data is the manifest data from the LMS, read-only, and empty without one» |
| `cmi.comments` (CMIString4096) | O | RW | passes | «comments take up to 4096 characters and comments_from_lms is read-only» |
| `cmi.comments_from_lms` (empty: no LMS comments) | O | RO | passes | «comments take up to 4096 characters and comments_from_lms is read-only» |
| `cmi.objectives._children`, `._count` | O | RO | passes | «objectives: every element round trips, is counted and validated» |
| `cmi.objectives.n.id` (CMIIdentifier) | O | RW | passes | «objectives: every element round trips, is counted and validated» |
| `cmi.objectives.n.id` with a space | O | RW | known deviation: accepted | «known deviation: an identifier with a space is accepted where the 1.2 test suite refuses it (405)» |
| `cmi.objectives.n.score._children`, `.raw`, `.min`, `.max` | O | RW | passes | «objectives: every element round trips, is counted and validated» |
| `cmi.objectives.n.status` | O | RW | passes | «objectives: every element round trips, is counted and validated» |
| `cmi.student_data._children` | O | RO | passes | «student_data comes from the manifest and is read-only» |
| `cmi.student_data.mastery_score` (manifest `adlcp:masteryscore`) | O | RO | passes | «student_data comes from the manifest and is read-only» |
| `cmi.student_data.max_time_allowed` (manifest `adlcp:maxtimeallowed`) | O | RO | passes | «student_data comes from the manifest and is read-only» |
| `cmi.student_data.time_limit_action` (manifest `adlcp:timelimitaction`) | O | RO | passes | «student_data comes from the manifest and is read-only» |
| `cmi.student_preference._children` | O | RO | passes | «student_preference: audio, language, speed and text round trip within their ranges» |
| `cmi.student_preference.audio` (-1 to 100) | O | RW | passes | «student_preference: audio, language, speed and text round trip within their ranges» |
| `cmi.student_preference.language` | O | RW | passes | «student_preference: audio, language, speed and text round trip within their ranges» |
| `cmi.student_preference.speed` (-100 to 100) | O | RW | passes | «student_preference: audio, language, speed and text round trip within their ranges» |
| `cmi.student_preference.text` (-1 to 1) | O | RW | passes | «student_preference: audio, language, speed and text round trip within their ranges» |
| `cmi.interactions._children`, `._count` | O | RO | passes | «interactions: every element is write-only, counted and validated» |
| `cmi.interactions.n.id`, `.time`, `.type`, `.weighting`, `.student_response`, `.result`, `.latency` | O | WO | passes | «interactions: every element is write-only, counted and validated» |
| `cmi.interactions.n.objectives._count`, `.objectives.m.id` | O | RO / WO | passes | «interactions: every element is write-only, counted and validated» |
| `cmi.interactions.n.correct_responses._count`, `.correct_responses.m.pattern` | O | RO / WO | passes | «interactions: every element is write-only, counted and validated» |
| `student_response` and `pattern` checked against the interaction type | O | WO | known deviation: not checked | «known deviation: interaction responses and patterns are not checked against the interaction type» |

## Behaviour

| Requirement | Status | Test |
|---|---|---|
| A lesson that never sets `lesson_status` finishes `completed` | passes (wrapper works around the library) | «a lesson that never sets its status finishes completed, and the mastery score turns a score into passed or failed» |
| With credit and a mastery score, a score decides `passed` or `failed` at finish | passes | «the mastery score overrides the status a lesson reports when it has a score» |
| Suspend and resume: `entry` is `resume`, location and suspend data come back | passes | «a suspended lesson resumes with entry resume, its saved data and the time of finished sessions» |
| Commits are coalesced: one host write per interval, the latest state, and at once on finish | passes | «commits are coalesced: one host write per interval with the latest state, and at once on finish» |
| A lesson closed without `LMSFinish` still hands over its state | passes | «a lesson left without finishing still hands over its state when the player closes» |
| Pending state goes to the host when the app is backgrounded | passes | «pending state is handed over at once when the app goes to the background» |
| A persistence failure shows a player-level error; the attempt stays resumable | passes | «a failed save shows a player-level error and the next save clears it» |
| Long media plays by range; nothing above the whole-file cap is sent whole | passes | «long media in a package plays by range: every request is ranged, none is sent whole, and seeking asks for a later range» |
| A search for a name the wrapper does not define (a 2004-style search from a 1.2 lesson) climbs to the app window and throws, where a same-origin system would find nothing | known limit of nesting the wrapper in the app | «known limit: a search for the SCORM 2004 API from a 1.2 lesson climbs past the wrapper and is refused by the app window» |

## Isolation

Each row is one hostile fixture; the test asserts the escape failed by what the browser did. The harness serves the package origin with the content policy and permissions policy read from `handler.rs`, the bootstrap linked into every page, and the app's own `frame-src` from `tauri.conf.json`.

| Escape tried | Status | Test |
|---|---|---|
| A request off the package origin (every governed interface, a worker, preloads, every reachable window) | fails to escape | «no request leaves the package origin, from the lesson, a worker or any window the lesson can reach» |
| A peer connection, from the lesson and every window it can reach or make | fails to escape | «no peer connection can be made from any window the lesson can reach» |
| A frame navigation out of the package origin (app window, package frame, lesson frame, links, forms, refresh, data address) | fails to escape | «no frame leaves the package origin: » |
| A pop-up (`window.open`, links and forms to new windows, from new frames and the wrapper) | fails to escape | «no window opens: » |
| Reading another launch's address | fails to escape | «a launch cannot read or reach an earlier launch» |
| Reading storage left by an earlier launch | fails to escape | «nothing a launch stored reaches the next launch of the package (» |
| Reaching the command bridge, including through frames the content makes | fails to escape | «the command bridge is gone from every window the lesson can reach, frames it made itself included» |
| The checks can see an escape (bootstrap removed) | control | «control: without the bootstrap the same lessons do reach the bridge and a peer connection» |
| Covering the label, in and out of focus mode | fails to escape | «hostile content cannot cover the label, in or out of focus mode» «focus mode fills the window, a full-screen request becomes focus mode, and the label stays in view» |

## Not covered here

- Published sample packages: none is committed. The test packages are purpose-built; the standards body's 1.2 test suite needs a Java applet and is not driven.
- Device runs (iOS, Android) are manual and recorded in the conformance statement.
- Content packaging import is covered at the host seam (`crates/alexandria-studio/tests/package_import.rs`).
