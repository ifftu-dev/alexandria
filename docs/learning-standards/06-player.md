# The package player

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/01-runtime-library-check.md, docs/learning-standards/04-package-serving.md, docs/learning-standards/05-attempts-and-progress.md

Spec: Runtime; Serving and isolation (host-drawn label, focus mode, degrade message); Credentials → labelling; user stories 2 to 16.

- The package element renders in a player. Nodes that do not know the element type keep showing the existing unknown-element notice.
- A wrapper page served at the package origin holds the SCORM 1.2 API object from the vendored library; content in a nested frame finds it and calls it synchronously.
- Run-time state lives in the wrapper. Commits are coalesced and handed to the host at most once per short interval, and always on termination, frame unload and when the app is backgrounded. A persistence failure shows a player-level error and the attempt stays resumable.
- A permanent label outside the frame names the content as third-party with its publisher and says Alexandria never asks for a recovery phrase there.
- Focus mode fills the app window and keeps the label. A full-screen request from content becomes a focus-mode request.
- A table of contents for packages with several lessons; score and status shown and labelled self-reported; a control to start a fresh attempt; auto-advance suppressed.
- When content needs something blocked, the player says which thing.

Done when: in the browser harness a real SCORM 1.2 package launches, saves, resumes and completes through the player with a fake host bridge, and the label stays visible in and out of focus mode.

## Notes

2026-11-20: built.

- Player: `src/components/course/package/PackagePlayer.vue` (registered as element type `package` in `elementRegistry.ts`) and `packageSession.ts` (everything between the app and the wrapper). Wrapper: `src-tauri/src/package_serving/wrapper.html` and `wrapper.js`, served from the reserved path `<prefix>/_alexandria/` like the bootstrap; the message protocol is written at the top of `wrapper.js`.
- Launch order: open the attempt, open the launch, load the clearing page, load the wrapper, then per lesson `package_attempt_initialise` and a `launch` message; the wrapper builds a fresh API session, loads the stored state and only then opens the lesson frame. Closing: end the lesson, wait for its last commit, clear storage, close the launch, then terminate the attempt (so a completed package's files can be dropped).
- Content gets a facade with the eight SCORM 1.2 functions, bound to one lesson session. Calls after the session finished, or after the host ended it, are refused with error 301 (gap 2 of ticket 01); ticket 08 should confirm the code against the standard. A lesson that never sets its status finishes as completed (gap 1 workaround).
- Commits are coalesced in the wrapper (one per 2 s while a lesson runs) and sent at once on the content's finish, when the app is hidden and when the frame unloads; the host persists them one at a time, in order. A failed write shows a player-level error; the library is always answered `true`, so the lesson carries on and the attempt resumes from the last stored state.
- The host takes a commit only from the wrapper window and only for the lesson it launched, and fills in the attempt itself. Focus and blocked messages are taken from any frame inside the player's frame.
- The label is drawn above the frame in and out of focus mode, names the course author as publisher (or says the package came from a file opened on the device), and says Alexandria never asks for the recovery phrase there. Focus mode switches from the label, from a full-screen request by content, and off with Escape.
- Multi-lesson SCORM 1.2: the player owns the table of contents (headings included, invisible items left out), previous and next, and one API session per lesson. A launch resumes at the first lesson started but not complete, else the first not started.
- Blocked things the player names: a pop-up (opened in the frame instead), a dialog box, a plug-in, a resource from off the package origin (with its host). The bootstrap reports them; it now also replaces `alert`/`confirm`/`prompt` and defers a `window.open` navigation by one task so its report is not lost.
- Results: status and score are shown in the player and the course header and labelled self-reported. A course made only of package elements claims nothing on completion and shows a notice saying package results earn no credential; no auto-advance for package elements.
- Host additions: command `package_attempt_activities` (each lesson's reported result, for the table of contents) and `PackageLaunchInit.progress` (the progress a shown-only launch completed). Rust tests in `packages/attempts/tests.rs` and `package_serving/tests.rs`.
- Harness: `tests/desktop-ui/package-player.spec.ts` runs the real Player against a fake host bridge (`packageBackend.ts`, screen `package-player`) and the fixture package in `tests/desktop-ui/package-player/package/`, served with the bootstrap and the handler's content policy headers. `crates/alexandria-studio/tests/player_fixture_package.rs` keeps the fixture's description equal to what import makes of it. CI's change classifier now runs the browser checks for `src-tauri/src/package_serving/` and the backend for the fixture package.
- Not done here: a learner with no enrolment sees a notice instead of the package (local enrolment of a draft course, needed for a package file opened from disk, is ticket 07's); SCORM 2004 is refused as unsupported until ticket 10 adds its runtime entry.
- Unproven until a device run: messaging and frame identity at the real package scheme in each webview, focus mode inside the full app layout, content policy reports from the real handler, media in packages.

Proposed doc edits, not made here:

- `docs/learning-standards-spec.md` → Runtime: the commit interval, the 301 refusal after finish, and the resume rule above.
- `docs/plugins.md`, element dispatch note: the `package` element binding.
- `GLOSSARY.md`: **Focus mode** and **Package label**, if the glossary should carry player terms.

### Review (2026-11-21)

Frontend fix-ups from the integration review:

- Storage clearing: `clear.html` echoes a per-request nonce read from its fragment; the player checks the sender, the nonce and `ok`, and refuses to start a launch whose clearing failed or timed out (error shown). A failed clear on close only warns: the next launch clears again before anything runs. Unproven on devices: if a real webview reports `ok: false` for any step, every launch now stops; check on each platform (ticket 28).
- Messages for the wrapper target the launch origin. Gap: on app schemes with an opaque URL origin (`package://localhost`) the target stays `*`; sourceless messages after the frame is gone are taken only from `package://localhost` or `null` there.
- cmi5 unit picks use the lesson request counter and run one at a time; an overtaken session is ended at once. The lesson controls are disabled while a lesson opens.
- Focus mode keeps a compact bar with the SCORM 2004 navigation controls and the exit confirmation; the table of contents and the course chrome stay hidden.
- The wrapper hands over the SCORM 2004 engine snapshot on `visibilitychange` hidden (frame unload already did through `closePackage`).
- Known gaps (judgement, not done): the two table-of-contents blocks in `PackagePlayer.vue` were not merged into a row component (they read different data); `ElementEditorHost` and `OpenPackageFileButton` keep their two-line fallbacks around `packageErrorMessage`; `hideLMSUI` tokens and the session-ended reason stay strings.
