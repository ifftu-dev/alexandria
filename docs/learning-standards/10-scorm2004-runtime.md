# SCORM 2004 runtime, sequencing and navigation

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/06-player.md, docs/learning-standards/09-scorm2004-import.md

Spec: Runtime; Attempts and progress → sequencing state and SCORM 2004 rollup; user stories 5 and 6.

- The wrapper exposes the SCORM 2004 API object from the vendored library beside the 1.2 one, chosen by the package's standard.
- The sequencing engine comes from the library. Its global objective and tracking state is stored per attempt in a sequencing state table and restored on resume.
- The player provides previous, continue, suspend and exit. Each is enabled, disabled or hidden as the engine and the package's presentation settings say.
- A package element is complete when the root activity's completion status is completed; score and time roll up as for 1.2.

Done when: a multi-activity SCORM 2004 package with sequencing rules runs in the harness, a locked activity cannot be reached, suspend and resume restore the engine's state, and host-seam tests cover the rollup.

## Notes

2026-11-20: built.

- Host: two new commands, `package_attempt_sequencing` (read the stored engine snapshot) and `package_attempt_sequencing_save` (store the snapshot and the root's completion status, then roll up). `package_attempt_initialise` takes an optional `resumed`. The wrapper sends the engine state after every navigation and on close, in the same write queue as commits, not on every commit.
- Rollup: the element completes when the stored root completion status is `completed`. Score is the mean of the lessons' scaled scores, clamped to 0..1. Time is the sum of the ISO 8601 session times. Both are worked out as for 1.2.
- Decision: a SCORM 2004 lesson that did not exit with `suspend` starts a new attempt with fresh run-time data and `entry = ab-initio`. The exception is when the engine resumes it (`deliveryWasResumed`, resume all after a suspend all), which keeps the data so that closing the player mid-lesson resumes the lesson where it was. This rests on my reading of cmi.entry and cmi.exit; ticket 11 should check it against the book.
- Controls: Previous, Continue, Suspend (`suspendAll`) and Exit (`exitAll`, with an inline confirmation). Continue and Previous come from `previewNavigationRequest` while a lesson runs; otherwise they fall back to the runtime's validity flags. A control is hidden when the delivered activity's `hideLMSUI` names it (`previous`, `continue`, `suspendAll`, `exitAll`). A table-of-contents entry is enabled only when the engine says a choice of it is valid.
- Player navigation travels with the lesson's termination: the wrapper sets `adl.nav.request` before removing the lesson frame. This avoids the trap where navigating with the lesson still open never terminated it. Closing the player suspends all.
- Library gaps worked around in the wrapper:
  - The stale error code after `GetValue` of `cmi.completion_status` / `cmi.success_status` is reset to 0 by the facade.
  - With no current activity (start delivered nothing), the runtime reports no valid choices: its look-ahead from the root is an empty stub. The wrapper asks the engine's own choice validation for each activity instead.
  - `stopForwardTraversal`, `constrainChoice` and the `request_valid` look-ahead are untouched and stay known gaps for ticket 11.
- Found: WebKit delivers what a frame posts while it is being removed with `event.source === null`. The player session now accepts such a message as the wrapper's only after the frame has left the page and when its origin matches the launch origin. This also affects the SCORM 1.2 unmount path.
- Not done: the host passes no clock for time-limit rules (`getAttemptElapsedSeconds`). A primary objective without an identifier is not passed to the runtime. Both are for ticket 11's matrix.
- Proposed doc edits (not made): the spec's Runtime section could name the two new commands. The player's harness notes could mention the second fixture `tests/desktop-ui/package-player-2004/` and its golden test `player_fixture_package_2004.rs`.
