# Native launch handoff

Triage: done
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

## Completion evidence — 2026-11-04

What changed:

- **Shared colours.** Every native launch surface uses the web first-paint background (`#f7f6f3` light, `#111827` dark) and the opening mark at rest (outline and branches, in the app primary colour). `launch_window.rs` holds the colours, and its test checks `index.html`, the Android resources, the iOS colour set and the storyboard against them.
- **iOS.** `LaunchBackground.colorset` and `LaunchMark.imageset` (SVG, light and dark) sit in the existing `Assets.xcassets`. That catalog is a folder reference, so `project.pbxproj` is unchanged. The launch storyboard uses the named colour and centres the 96 pt mark.
- **Android.** `launch_background` and `launch_mark` colours are in `values` and `values-night`. The theme sets the window background (below 12). On 12+, `values-v31` and `values-night-v31` also set the splash background and the `launch_mark` vector icon, drawn 96 dp wide at the centre. The night qualifier outranks the API level, so all four theme files carry the items.
- **Opening.** On Android and iOS, `AppOpening` starts centred, where the native mark was, and still. It then docks to the logo with no reveal replay. Desktop has no native mark, so it keeps the reveal.
- **Desktop.** The main window is created hidden. In setup, `launch_window::install` gives it the background for the OS theme. `main.ts` shows it through `showOnFirstPaint`: on the first frame after mount, or after 150 ms, since a hidden webview may not schedule frames. A Rust timer shows the window after 2 s if the webview never runs that code. A second launch also shows the window. The late `show()` after boot in `App.vue` is gone.

Red → green:

- `AppOpening.android.test.ts`: red (the mark started above centre and replayed the reveal), then green.
- `launchWindow.test.ts`: the first-frame test was red (no module), then green. The fallback test passed at once, because the timer landed with the first slice. A mutation check (fallback disabled) turned it red.
- `launch_window::tests::native_launch_surfaces_match_the_web_first_paint`: red (no native resources), then green.

Checks:

- Targeted Vitest files (`src/components/auth`, `launchWindow.test.ts`) and `npx vue-tsc -b --noEmit`.
- `cargo test -p alexandria-node --lib launch_window`, `cargo fmt --check` and `cargo clippy -p alexandria-node --all-targets -- -D warnings`.
- `cargo check -p alexandria-node --target aarch64-apple-ios --features tutoring-video-ios`.
- `xmllint` on the changed Android XML and the storyboard; `python3 -m json.tool` on the asset JSON.
- `xcrun ibtool --compile` on the storyboard and `xcrun actool` on the catalog: both compile, with light and dark variants.
- `aapt2 compile` and gradle `:app:processArm64DebugResources`.
- `scripts/check-docs.sh`.

Not run: full `xcodebuild`, gradle assemble and `tauri build` bundles. On-device verification (cold and warm launch, light and dark, no flash and no second splash) is in ticket 17.

Deviations:

- iOS also continues from the native mark. Its launch screen now shows the mark, and replaying the reveal after it would be a second splash.
- The native surfaces follow the OS appearance. An in-app theme that differs from the OS takes over at the web first paint.
- The Android 12+ splash ends at the activity's first frame. It is not held until the web paints. If ticket 17 sees a blank frame between the splash and the opening, holding it needs `core-splashscreen` and a signal from the webview.
