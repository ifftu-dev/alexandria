# Localised notification text and push errors

Triage: ready-for-agent
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
