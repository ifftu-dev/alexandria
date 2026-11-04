# Motion: progress, slow start, tokens and preference storage

Triage: done
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

## Completion evidence — 2026-11-04

What changed:

- **Progress.** `data-progress` is the one shared marker. The global reduced-motion rules in `main.css` skip it. In Reduced, marked indicators turn at `--duration-progress-reduced` (3 s); in Off they stand still. AppSpinner always carries text (its caption, or an accessible "Loading…"). A loading AppButton keeps its name, and in Off shows its label beside a still spinner. Text-less border spinners now use AppSpinner; the others carry the marker beside their existing text.
- **Slow start.** `useSlowNotice` adds a line after 2 s: "Taking longer than expected" under the startup step (new `AppStartupStatus`), and "Still unlocking" beside the decryption stage. Translated by hand into all nine locales.
- **Tokens.** `main.css` gains `--duration-fast/base/slow/emphasis/reveal`. `animateMotion` takes a duration token name and reads `--ease-settle`. `motionDuration()` gives timers the same token: the atlas highlight and its animation both use `--duration-reveal`, and the completion confetti timer uses `--duration-emphasis`. No easing curve is hardcoded in the composable. BrandMark, the completion modal and the atlas use duration tokens. CourseCard, AppSidebar and Player now use `var(--ease-settle)` instead of a copied curve.
- **Storage.** `ui.motion` is registered as a device-scoped setting (default `system`). After unlock it drives the preference and refreshes the pre-unlock cache. Changes write back while a profile is unlocked.
- **Colours.** The top-bar badge uses `--app-error` / `--app-error-foreground`. The completion modal's 38 literal fallbacks are removed (its tokens exist in both themes). The atlas field palette moved into `main.css` with light and dark values, and `skill-atlas.css` is deleted.

Red → green:

- `AppSpinner.test.ts` and `AppButton.test.ts` (marker and text in Off): red, then green.
- `motion.spec.ts` (desktop UI, Chromium and WebKit): two new tests read the real cascade. They show a 3 s infinite rotation in Reduced (setting and OS preference), and in Off `animation-name: none` with visible text. Red (no marked indicator), then green.
- `AppStartupStatus.test.ts` and `ProfileSelect.motion.test.ts`: red, then green.
- `useMotion.test.ts` (reads tokens, mirrors the setting) and `SkillAtlasExplorer.test.ts` (highlight lasts the reveal token): red, then green.
- `colorTokens.test.ts` (atlas palette in both themes; badge and modal use tokens only): red, then green.
- `settings::registry::motion_preference_is_a_device_setting`: red, then green.

Checks: targeted Vitest files, `npx playwright test` on the motion, skill-atlas and personal-atlas specs (43 passed), `npx vue-tsc -b --noEmit`, `npm run check:csp`, `npm run i18n:keys`, `npm run i18n:parity`, `npm run i18n:no-raw-text`, `npm run check:tauri-commands`, `cargo test -p alexandria-node --lib settings::` (19 passed), `cargo fmt --check`, `cargo clippy -p alexandria-node --all-targets -- -D warnings`, `scripts/check-docs.sh`.

Deviations:

- jsdom does not apply `main.css`'s cascade, so the Vitests check the marker and the text. The rotation itself (slow in Reduced, still in Off) is proven in the desktop UI test.
- Mapping to the nearest token changed some durations slightly: the opening mark 680 → 650 ms, the onboarding step 240 → 200 ms, completion card-pop 500 → 650 ms, shine 500 → 320 ms, progress width 450 → 320 ms.
- Skeleton `animate-pulse` placeholders and the onboarding hero rings are not progress indicators, so they stay frozen under Reduced.

## Notes

- 2026-11-04: The spinner's Reduced and Off rule leaked onto the whole document. In scoped CSS, `:global(:root[data-motion='reduced']) .app-spinner__arc` compiles to a bare `:root[data-motion='reduced']`, dropping the arc, so `stroke-dasharray: 30 60` was inherited and every stroked SVG icon turned dashed in Reduced and Off. The selector is now `:root[data-motion='reduced'] .app-spinner__arc`, which compiles to `.app-spinner__arc[data-v-…]` under `:root[...]`. The new AppSpinner Vitest compiles the style and was red (bare `:root[...]`), now green. The desktop UI progress test now checks that the spinner's track keeps a solid stroke and the arc keeps its fixed gap. It was red in Chromium (track `30px, 60px`), now green. No other component mixes `:global(...)` with a scoped descendant.
