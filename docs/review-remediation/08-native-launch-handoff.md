# Native launch handoff

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

Make each native launch surface use the same light and dark background as the web first paint, with the same mark the opening uses.

- **iOS:** a named light/dark colour asset in the launch screen. Hand-edit the Xcode project; never regenerate it.
- **Android 12+:** the splash theme's background and icon. The web opening starts with the mark already in place, continuing the system splash rather than replaying a reveal.
- **Android below 12:** the window background colour.
- **Desktop:** create the window hidden, with the theme background, and show it on the first web paint. A short fallback timeout shows it anyway.

Covers spec stories 23–26.

## Acceptance

- The iOS compile check passes, including with `--features tutoring-video-ios`.
- The Android and desktop builds pass.
- The opening's Vitest covers starting with the mark already in place on Android.
- The desktop fallback timeout is tested at the frontend seam.
- On-device verification is recorded in ticket 17.
