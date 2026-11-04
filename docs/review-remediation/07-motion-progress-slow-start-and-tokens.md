# Motion: progress, slow start, tokens and preference storage

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

- **Progress indicators.**
  - Exempt spinners, loading buttons and progress bars from the global reduced-motion rule, through one shared marker.
  - In Reduced, spinners keep their slow rotation.
  - In Off, they are static and paired with visible or accessible text.
  - A loading button always keeps an accessible name.
- **Slow start.** After about two seconds, the startup screen shows a translated "Taking longer than expected" line, without replacing the current step. Profile unlock shows a translated "still unlocking" line after the same delay.
- **Tokens.** The motion composable reads easing and durations from the CSS tokens. Component animations use duration tokens. Timers tied to an animation derive from that animation's token, including the atlas evidence highlight, which is currently clipped (650 ms timer, 800 ms animation).
- **Preference storage.** Register the motion preference as a device-scoped setting. The pre-unlock local cache stays and mirrors it, the same way theme works.
- **Colours.**
  - The top-bar badge foreground uses the error-foreground token.
  - The completion modal's fallbacks use light/dark tokens.
  - The atlas field palette moves into the design tokens, with light and dark values.

Covers spec stories 27–31, 45–47 and 57.

## Acceptance

- The spinner and button Vitests show slow rotation in Reduced and a static, labelled indicator in Off.
- The opening/startup test shows the slow-start line after the delay.
- The motion composable test shows it reads tokens and mirrors the setting.
- No hardcoded easing curves remain in the motion composable.
- Strict checks and locale parity pass.
