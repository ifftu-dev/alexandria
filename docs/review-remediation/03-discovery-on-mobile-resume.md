# Discovery on mobile resume

Triage: done
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

Add a command that runs one Cloud discovery pass for the current profile. On mobile, the webview's existing resume handler calls it alongside push re-registration.

Discovery passes are serialised. A resume pass that overlaps the background loop's pass joins that pass rather than starting a second one. Results go through the existing de-duplication, so push and discovery can't double-record. Register the command in the handler list and the command check.

Covers spec stories 9–10.

## Acceptance

- After resume, a missed interview invitation is recorded without waiting for the 60-second loop.
- A resume during an active pass doesn't send duplicate requests and doesn't create duplicate rows.
- Rust tests against the fake Cloud server, the notifications composable Vitest, and `check:tauri-commands` pass.

## Completion evidence — 2026-11-04

What changed:

- `notifications::cloud_pull::discover(app, lease)` runs one discovery pass for the profile the lease admits. It reads the origin, fetches every list, records unreported items through the existing dedupe keys, and announces them under the lease's session (ticket 02).
- Passes are serialised through a slot in `AppState` (`cloud_discovery`). A caller for the same session that arrives while a pass is running joins it and shares its result. The pass runs as its own task, wrapped in `run_until_closed`, so it ends when the profile locks and the slot never keeps a lease alive.
- The 60-second background loop now calls `discover` instead of its inline pull, so the loop and resume share one pass. `notifications::deliver` lost its last caller and was removed.
- New command `notifications_discover`, profile-scoped through `ProfileLease`. It is registered in `generate_handler![]` and `src/generated/tauri-commands.ts`.
- `useNotifications`: on mobile, the resume handler (visible, same generation and session) calls `push_register` and `notifications_discover` once each. The 60-second webview timer still retries push only.

Red → green:

- `a_resume_pass_records_a_missed_invitation_without_waiting_for_the_loop`: red against a stub (`Ok(0)`, no row). Now green: one interview invitation is recorded and announced under the session.
- `a_resume_during_a_running_pass_joins_it_without_repeating_requests_or_rows`: red with unserialised passes, because the fake Cloud received 10 requests instead of 5. Now green: 5 requests, both callers get `Ok(1)`, and a later pass re-asks but records 0, leaving one row.
- Vitest `looks for missed activity and re-registers push once each when the app returns`: red, because `notifications_discover` was called 0 times. Now green. `does nothing on resume once the profile session has changed` is green.

Checks: `cargo test -p alexandria-node notifications::` (38 passed), plus `commands::notifications`, `commands::p2p` and `profile::`; the join test passed 5 runs out of 5; `cargo fmt --check`; `cargo clippy -p alexandria-node --all-targets -- -D warnings`; `npx vitest run src/composables/__tests__/useNotifications.test.ts` (22 passed); `npx vue-tsc -b --noEmit`; `npm run check:tauri-commands`; `scripts/check-docs.sh`.

Deviations:

- `check:tauri-commands` had been failing since ticket 01, which grouped the `ProfileState as State` import in `commands/settings.rs`. It was fixed in a separate commit.
- One existing Vitest case now tears down after itself, because its leaked resume listener counted against the new tests.
- The tests use the on-disk profile helper rather than an in-memory database, as tickets 01 and 02 did.
- Mobile targets were not cross-compiled; no platform-gated code changed.
