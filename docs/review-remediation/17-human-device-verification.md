# Human device verification

Triage: ready-for-human
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md
Blocked by: alexandria/docs/review-remediation/04-localised-notification-text-and-push-errors.md, alexandria/docs/review-remediation/08-native-launch-handoff.md

## Task

These checks need physical devices and a person watching. Record each result as a dated note below; failures become new findings in the parent spec.

1. **Native launch handoff.** Cold and warm launch, in light and dark, on:
   - iPhone
   - Android 12+
   - Android below 12, or an emulator if no device is available
   - macOS

   Check for no background colour flash, no second splash, and no replay after resume.
2. **Android channel name.** Check that it shows in a non-English locale in system notification settings.
3. **Call-manager retest.** Deepening ticket 02 promised this before merge, and it was never recorded. Run a two-device call: camera, screen share, chat, leave and rejoin, and background/foreground camera return. Add a dated note to deepening ticket 02 as well.

Not in scope: native motion performance acceptance and notifications ticket 06's provider checks. Those stay with their own specs.

## Notes
