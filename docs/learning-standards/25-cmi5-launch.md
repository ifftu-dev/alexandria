# cmi5 launch, sessions and completion

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/14-learning-records.md, docs/learning-standards/23-cmi5-transport-prototype.md, docs/learning-standards/24-cmi5-import.md

Spec: cmi5 (launching system's duties, actor, on-device endpoint); Attempts and progress → cmi5 completion; Learning records → document stores.

- The app performs the launching system's duties: launch parameters, the one-time authorisation token, launch data and learner preference documents, the launched statement, validation of a unit's statements against the session rules, satisfied statements for blocks and the course, the abandoned statement on abnormal end, and completion criteria evaluation.
- Content talks xAPI to an on-device endpoint that exists only while a session is open, accepts only that session's token and implements only the resources cmi5 requires, backed by the local record tables and the document stores cmi5 needs. It is not claimed as a conformant record store.
- The transport is the one ticket 23 settled.
- The actor given to a unit is the pseudonymous identifier other package content receives. Records a unit writes are rewritten to the recipient identifier when sent anywhere.
- A package element is complete when the course's completion criteria are satisfied.

Done when: a published cmi5 package launches, runs a session and completes in the harness; host-seam tests cover token single use, a statement breaking the session rules being refused, satisfied and abandoned statements, and the endpoint refusing requests outside an open session.

## Notes

2026-11-20: built.

- **Pure half** in the studio crate: `xapi::cmi5` checks each unit statement against the session and registration rules (initialized first and once, no defined verb twice in a session, one of passed or failed per session, completed and passed once per registration, failed never after passed, nothing after terminated or abandoned, the context template's actor, registration, session id and publisher grouping, result rules of 9.5, the moveOn category, the mastery score, no voiding) and writes the launched, satisfied and abandoned statements and the LMS.LaunchData and learner preferences documents. `package::progress` gains `unit_satisfied`, `satisfied_nodes` and the cmi5 rollup arm. Learning events gain `Abandoned` and `Satisfied` actions and `Profile::Cmi5`.
- **Host half** in `packages::attempts::cmi5`: launch, end, the endpoint, its listener and the tables. Migration 48 `cmi5_sessions`: sessions, every statement the endpoint accepted or the app wrote (as the unit sees them), and the xAPI documents (state, agent profile, activity profile). `docs/database-schema.md` is regenerated.
- **Transport** as ticket 23 settled: one hyper listener on `127.0.0.1:0` per open session, checks in the order Host, Origin, session open, token, version header; a launch with an open session is served with `connect-src 'self' http://127.0.0.1:<port>`. Android release builds get a network security config allowing cleartext to `127.0.0.1` only; debug builds keep cleartext everywhere through a debug-only copy. On mobile, `WindowEvent::Resumed` binds a reclaimed endpoint again on its port; a session that cannot come back ends abandoned and the player hears `package-cmi5-ended`.
- **Identity.** The actor is the pseudonymous learner identifier under the app's account home page. The registration is a UUID keyed from the learner's key and the attempt id (new context string in `xapi::cmi5::registration`). The unit's activity id, and the object of `satisfied` for a block or the course, is the package activity DID URL the records use; when the course cannot be named that way, a URN from the package hash and the course structure id. Never the publisher's id.
- **Records.** completed, passed, failed, launched, satisfied and abandoned become learning records (completion-level, cmi5 profile), rebuilt for each recipient when sent. "cmi5 allowed" statements stay in the session's statement table only.
- **Completion.** A unit's stored outcome is its moveOn satisfaction; NotApplicable units count without a launch. `satisfied` is written once per block and course per attempt, in the session that satisfied it; the moveOn criteria are evaluated at each launch, so NotApplicable-only blocks are satisfied at the first launch.
- **Abandoned.** Ending a session the unit did not terminate (leaving the player, a new launch in the same attempt, closing the own window, terminating the attempt) writes abandoned once, with the time from launch to the unit's last statement. A session left open when the app stopped is abandoned at the attempt's next launch, its facts read back from its launched statement.
- **Own window.** Desktop: a webview window with label `cmi5-unit-<session>`, listed in no capability; closing it ends the session, and the window closes when the unit loads `_alexandria/returned.html` (a new reserved file that posts `au-exited` to the player). Mobile: the unit plays framed with focus mode forced on.
- **Harness.** `tests/desktop-ui/package-player/cmi5/package/` is a built-in cmi5 package (a block of two units, a third unit asking for its own window) with its description checked by `player_fixture_package.rs`. Host-seam tests (`packages::attempts::cmi5::tests`, 11) drive its units over real HTTP; the browser check `cmi5-player.spec.ts` (4 tests, Chromium and WebKit) runs it through the player to completion against a fake endpoint.
- **Shared files** for the merge with tickets 07, 08, 10, 15 and 17: `wrapper.js` gains a separate `launchFramed` branch at the top of `launch()` and an `apiName` guard; `packageSession.ts` gains cmi5 functions and two optional events; `PackagePlayer.vue` gains a placeholder, a notice and a listener; the harness is a new screen (`cmi5-player`), backend and spec, leaving the SCORM ones untouched. `src/types/index.ts` mirrors `PackageActivity.cmi5`, which ticket 24 had not mirrored.
- **Merge** with the integration branch after tickets 07, 08, 10 and 15: migration 48 was still free; the activity log labels the new satisfied and abandoned records. An author's preview of a cmi5 package element (ticket 07's preview commands) is not wired: `package_cmi5_launch` needs a real attempt, so a preview shows the load error. Follow-up.

Deviations, for the conformance statement (ticket 27):

- Not a conformant record store: statements GET supports `statementId` and the verb, activity, registration, agent, since, until, limit and ascending filters only, never pages, and scopes to the attempt. `voidedStatementId` always answers 404.
- A statement with no id or timestamp gets one from the endpoint instead of being refused.
- The launching system rejects rule-breaking statements (403) rather than voiding them; it never voids.
- After `terminated` the session takes no more statements at once (no grace period).
- Launch mode is always Normal; there is no browse or review offer and no `waived`.
- The unit may write the learner preferences document; only LMS.LaunchData is refused to it.
- Mobile own-window units play framed (ticket 23's deviation).

Proposed doc edits, not made here:

- `docs/learning-standards-spec.md` → cmi5: the transport bullet becomes "a per-session listener on the loopback address, admitted only in that launch's content policy"; add that the unit's activity id is the package activity DID URL and its registration is derived from the learner's key.
- `GLOSSARY.md`: "cmi5 session" (one launch of an assignable unit, with its endpoint) if the glossary takes standards terms.
- `docs/learning-standards/28-device-and-hosted-runs.md`: ticket 23's device checklist applies to this build as is; add "the Android network security config admits only 127.0.0.1 in release".
- `src-tauri/src/AGENTS.md`: reserved package files now include `returned.html`; a cmi5 launch's policy depends on `Launches::set_endpoint_port`.

