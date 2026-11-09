# Remove public notification deep links

Triage: done
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

Remove the public `alexandria://notifications` and `alexandria://notification/<id>` links, and the inbox's open-on-query behaviour.

Do this in order:

1. Confirm that native push taps and held-tap replay route through the internal route only, so that removing the links breaks nothing. If any native path depends on them, move it to the internal route first.
2. List the debug-only notification emitter in the parent spec's decisions as debug-only. It stays refused outside debug builds.

Covers spec stories 14–16.

## Acceptance

- In the desktop UI suite, the removed links neither navigate nor mark anything read.
- Push-tap and held-tap tests still land on the right screen.
- The deep-link parser tests are updated.
- A test shows the debug emitter is refused in release configuration.

## Completion evidence — 2026-11-04

Native paths checked first:

- Banner taps (`notifications::install` `on_action` → `opens::local_tap`), push taps (`push::tapped`) and held-tap replay (`notifications_pending_open` → `useDeepLinks().openNotification`) all carry a row's in-app route on the `notifications:open` event, or in the sealed held open. None of them builds or parses an `alexandria://notification…` link.
- Every producer route is an in-app path: `/interview-room/<id>`, `/settings/organisations`, `/classrooms/…`, `/credentials/<id>` and `/settings/guardian`. Cloud's routes are checked against `cloud_copy`. No producer in the app, Cloud or the website emits the public links, so nothing had to move to the internal route.

What changed:

- `src/deeplink/parse.ts` no longer accepts `notifications` or `notification/<id>`, in either the custom-scheme or the https form.
- `src/pages/Notifications.vue` ignores `?open=` and only loads the list. Only the learner's own tap opens, marks read or follows a row.
- `notifications_emit_test` goes through `test_emitter_allowed(cfg!(debug_assertions))`, which refuses it outside debug builds. The parent spec lists it as debug-only.
- `docs/deeplink/README.md` already listed neither link, so it is unchanged.

Red → green:

- New desktop UI fixture `?screen=notification-links` (the real router and deep-link runtime over an unlocked profile with one unread row) and `tests/desktop-ui/notification-links.spec.ts`:
  - `the retired public notification links neither navigate nor mark anything read`: red, because `alexandria://notifications` navigated to `/notifications`. Now green in Chromium and WebKit.
  - `the inbox does not open a row named in its query`: red, because `/notifications?open=n-1` marked the row read and followed it to `/settings/organisations`. Now green.
- `the_test_emitter_is_refused_outside_debug_builds` (Rust): red at compile time, because the gate did not exist. Now green. It asserts the release refusal by value. A test binary is always a debug build, so the gate takes the build flag as an argument.
- Updated: the parser test now asserts that all four link forms parse to `null`. The `openLink` and `openRoute` cases that used the removed link now use a classroom link.

Push-tap and held-tap tests still pass: the Rust `notifications` tests (49), including `shared_token_receipt_and_held_tap_wait_for_matching_profile_across_restart` and `local_banner_tap_survives_lock_and_rejects_legacy_profileless_extras`, and the Vitest `recipient-bound notification taps` and `follows a tap the backend held` cases.

Checks: `cargo test -p alexandria-node notifications`; `cargo fmt --check`; `cargo clippy -p alexandria-node --all-targets -- -D warnings`; `npx vitest run` on `src/deeplink`, useNotifications and NotificationsPanel (67 passed); `npx playwright test tests/desktop-ui/notification-links.spec.ts`; `npx vue-tsc -b --noEmit`; `npm run check:tauri-commands`; the i18n checks; `scripts/check-docs.sh`.

Deviation: `alexandria://open?route=/notifications` still reaches the inbox through the generic route link, as it reaches any known route. It can no longer open, mark read or follow a row.

