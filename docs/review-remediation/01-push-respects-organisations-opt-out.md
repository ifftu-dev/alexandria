# Push respects the organisations opt-out

Triage: done
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md
Release: next patch release, ahead of every other ticket in this spec

## Task

Make push registration eligible only while the push preference is on, organisations are on, and a Cloud origin is present. When organisations are off, push reconciliation behaves as it does with push disabled:

1. Retire routing first.
2. Mark registrations for deletion.
3. Send the existing signed DELETE, and keep it pending across restarts until Cloud acknowledges it.

The stored push preference never changes.

Changing the organisations setting triggers a push reconcile, just as changing the push setting does. Push status gains `paused_by_organisations`. Settings shows a translated explanation (all supported locales) that links to the organisations switch.

Covers spec stories 1–5.

## Acceptance

- With organisations off, no PUT reaches the fake Cloud server. An existing registration is deleted, and the deletion survives restart while offline until it is acknowledged.
- Turning organisations back on re-registers, provided push is still on.
- The push preference is untouched throughout.
- Rust module tests at the notifications interface (in-memory DB, fake Cloud server), a Vitest for the Settings status, strict frontend checks, and Rust fmt and clippy all pass.
- No Cloud change is needed. The existing DELETE endpoint is used.

## Completion evidence — 2026-11-04

What changed:

- `notifications::push` treats push as eligible only while the push preference and `organisations.enabled` are both on (and a Cloud origin is set). With organisations off, reconcile takes the push-disabled path: it retires routing, marks rows for deletion and sends the signed DELETE until Cloud acknowledges it. The commit step re-checks both settings, so a PUT that races the switch cannot publish a recipient. The stored push preference is never written.
- `PushStatus` gains `paused_by_organisations`. It serialises as `pausedByOrganisations`, keeping the struct's camelCase until ticket 06.
- `set_setting` and `reset_setting` call `push::settings_changed` in the background. For `organisations.enabled` it runs the same reconcile-then-register path that `push_set_enabled` uses.
- Settings → Notifications shows a translated "paused" explanation that links to `/settings/organisations` (all nine locales, written by hand) and hides the register button while paused.

Red → green:

- `organisations_off_retires_registration_until_acknowledged_and_on_re_registers`: red, because a PUT reached the fake Cloud while organisations were off. Now green: DELETE 503, restart, DELETE 204, re-register on switch-on, preference untouched.
- `switching_organisations_off_reconciles_push_and_other_settings_do_not`: red against a no-op hook. Now green.
- `NotificationsPanel.test.ts` (Vitest): red, because the explanation was missing. Now green.

Checks: `cargo test -p alexandria-node notifications::` (34 passed), `cargo fmt --check`, `cargo clippy -p alexandria-node --all-targets -- -D warnings`, `npx vue-tsc -b --noEmit`, `npm run i18n:keys`, `npm run i18n:parity`, `npm run i18n:no-raw-text`, `npm run check:tauri-commands`, `scripts/check-docs.sh`.

Deviation: the Rust tests use the existing encrypted on-disk profile helper (`boundary_tests::active`) rather than `Database::open_in_memory()`, because surviving a restart needs a profile that can be reopened. An inbound settings sync that turns organisations off is reconciled by the mobile retry and resume paths, not straight away.
