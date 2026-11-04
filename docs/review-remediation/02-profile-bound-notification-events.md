# Profile-bound notification events

Triage: done
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

Recording and announcing a notification take the profile session from the lease of the job that produced it. Nothing reads the session at send time. The new-event payload, the unread-count payload and the OS banner's profile tag all use the captured session.

If the lease is no longer current when the announcement would go out, the announcement and banner are dropped, and the row stays in its own profile's store. Mark-read, mark-all-read and clear emit their count update under the lease they ran with.

Covers spec stories 6–8.

## Acceptance

- A test switches profile between recording and announcing. No event or banner reaches the new profile, and the row exists only in the original profile.
- Count updates after mark-read or clear carry the acting profile's session, even if a switch follows straight after.
- Rust module tests and the notifications composable Vitest pass.

## Completion evidence — 2026-11-04

What changed:

- `notifications::announce`, `deliver` and `emit_changed` take the session of the producing job's lease. Nothing reads `profile_operations.session()` or `active_id()` at send time any more.
- `notifications::announcement(state, session)` decides what goes out. It returns the session and the profile the OS banner is tagged with, reading the profile while that session is held current. Once the session is stale it returns `None`, so the new-event payload, the unread count and the banner are all dropped and the row stays in its own profile's store.
- `emit_changed` drops a count update whose session is no longer current. Mark-read, mark-all-read and clear emit under the lease they ran with.
- Producers pass their lease's session. The background pass passes it for auto-issuance and Cloud discovery, push receive and the debug emitter pass theirs, and inbound gossip and guardian links take it from the job they ran in (`InboundDatabase::run_in_session`, with `P2pEvent::GuardianLinked` gaining a `session` field).
- `os_banner` skips when no notification plugin is installed, so mock apps can run the whole announce path.

Red → green:

- `a_profile_switch_before_announcing_drops_the_event_and_banner_and_keeps_the_row_home`: red with the session read at send time, because the banner was tagged with profile B after the switch. Now green: before the switch the webview hears both events with A's session, after it nothing is heard and no banner is built, and the row is in A only.
- `count_updates_carry_the_acting_session_and_never_the_next_profiles`: red, because the count update after clear-then-switch carried B's session. Now green.

Checks: `cargo test -p alexandria-node notifications::` (36 passed), plus `p2p::inbound`, `p2p::guardian`, `profile::` and `commands::notifications`; `cargo fmt --check`; `cargo clippy -p alexandria-node --all-targets -- -D warnings`; `npx vitest run src/composables/__tests__/useNotifications.test.ts` (20 passed); `scripts/check-docs.sh`.

Deviations: the tests use the encrypted on-disk profile helper (`push::boundary_tests::active`, plus a new `switch_to`) rather than `Database::open_in_memory()`, because a profile switch needs two real profiles in one app state. The banner is observed through `announcement`, the value the banner is built from, rather than through the OS. The composable needed no change, since it already ignores payloads from another session.
