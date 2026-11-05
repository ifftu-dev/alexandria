# Localised notification text and push errors

Triage: done
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

The Android notification channel's name and description come from the app's locale resources, in the profile's language, when the channel is created. They are refreshed when the language changes.

Push-registration failures become a small, stable set of error codes, which the webview translates in every supported locale. Keep the raw failure text in a separate diagnostic field. Notification Settings shows the translated message and never the raw text.

Covers spec stories 11–13.

## Acceptance

- Each failure path maps to a documented code.
- Locale parity checks pass for the new strings.
- Rust tests assert codes by value.
- A Vitest shows the translated message for each code.
- The on-device check of the channel name in a non-English locale is recorded in ticket 17.

## Completion evidence — 2026-11-04

What changed:

- `notifications::push::PushErrorCode` holds the codes. Each failure path maps to one of them:
  - `permission_denied`: the learner refused the system notification prompt.
  - `token_unavailable`: the prompt could not run, or the push SDK returned no token.
  - `insecure_origin`: the Cloud origin is neither https nor loopback.
  - `cloud_unreachable`: the request got no answer.
  - `cloud_rejected`: Cloud answered with a non-2xx status.
  - `cloud_unacknowledged`: the answer had no matching versioned acknowledgement, or a removal was answered with something other than 204.
  - `invalid_response`: a registration answer had no valid recipient binding.
- `PushStatus.lastError` is replaced by `lastErrorCode` and `lastErrorDetail`, which hold the raw English text for diagnostics. Both stay camelCase until ticket 06. Migration 31 (`push_error_codes`) stores the code in `last_error` and the raw text in a new `last_error_detail` column, and drops free-text errors written before codes existed. `docs/database-schema.md` was regenerated.
- Notification Settings maps each code to `notifications.settings.pushErrors.<code>` through an exhaustive `Record<PushErrorCode, MessageKey>`, so it never renders the detail. All nine locales were translated by hand. The old `pushError` "Last attempt failed: {error}" string is gone.
- The Android channel's name ("Activity") and description come from `notifications.channel` in the locale catalogs, which `notifications/copy.rs` already embeds. `channel_text` resolves `system` to the device language. The channel is refreshed when a profile is created, restored or unlocked (`refresh_channel`), and when `user.language` is set or reset (`notifications::settings_changed`, called beside the push hook). At app start, before any profile is unlocked, the channel is created in the device language only if it does not exist yet, so it keeps the language the last profile wrote.

Red → green:

- `each_push_failure_is_reported_as_a_stable_code_with_its_raw_detail` and `native_and_transport_failures_have_their_own_codes`: red at compile time, because `PushErrorCode`, `PushFailure` and `permission` did not exist and the status had no code field. Now green. Through the fake Cloud they assert each wire code and its detail by value: 503, unversioned 2xx, 201, bad recipient, success, a DELETE answered 200, and the listener gone. They also cover a refused permission, a prompt failure, a token failure and an insecure origin.
- Vitest `NotificationsPanel push errors`: red, because there was no `push-error` element and every code failed. Now green for all seven codes in English (written out as literals), in French, and with no line once the last attempt succeeds. The raw detail never appears.
- `the_android_channel_reads_in_the_chosen_language_and_falls_back_to_english` and `changing_the_profile_language_refreshes_the_android_channel_text`: red at compile time, because the functions did not exist. Now green.

Checks: `cargo test -p alexandria-node notifications::` (42 passed), plus `db::` (69), `commands::` (315) and `profile::` (78); `cargo fmt --check`; `cargo clippy -p alexandria-node --all-targets -- -D warnings`; `npx vitest run` on the NotificationsPanel, useNotifications and `src/i18n` tests (64 passed); `npx vue-tsc -b --noEmit`; `npm run i18n:keys`, `i18n:parity` and `i18n:no-raw-text`; `npm run check:tauri-commands`; `python3 scripts/test_generate_schema_doc.py`; `scripts/check-docs.sh`.

Deviations:

- The Android-only channel calls (`create_channel`, `list_channels`) were not cross-compiled for Android. The tests cover the text and the refresh decision; the on-device check is ticket 17.
- The English channel name is now "Activity" rather than "Alexandria", so that it translates. Android already shows the app name above it.
- A language change that arrives by settings sync from another device refreshes the channel at the next unlock, not straight away.

