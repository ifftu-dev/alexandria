# Desktop UI validation

Desktop changes need both automated regressions and a native package check. The
browser fixture uses production Vue components, onboarding, and theme CSS. It stubs
goal templates, pre-account platform/profile calls, and an empty offline dashboard.
It never opens or changes a real user profile.

## Automated checks

```sh
npm ci
npx playwright install chromium webkit
npm run test:desktop-ui
npm test
npm run build
```

On Linux CI, install browser system dependencies with
`npx playwright install --with-deps chromium webkit`.

The `Desktop UI (Chromium and WebKit)` CI job runs for frontend changes, including
the fixture and Playwright configuration. It retains HTML reports, failure
screenshots, and traces for 14 days. The suite checks:

- Light and dark select contrast even when the OS theme disagrees, using rendered
  background pixels rather than only computed CSS.
- Progress bars using the app's accent color in both browser engines and themes.
- Goal-picker layout in a 420-pixel-wide tiled window.
- Long dialogs in a short window: visible title/actions and scrolling content.
- Initial focus, Tab/Shift+Tab containment, Escape, and focus restoration.
- Nested dialogs retaining the outer scroll lock and handling one Escape at a time.
- Dialogs covering compact navigation and dismissing from the backdrop only.
- Input errors connected to their fields for assistive technology.
- Onboarding keyboard focus on buttons with decorative shadows, plus scrolling
  and navigation through the initial steps in a short window.
- Dashboard hero contrast across its gradient in both themes.
- Home's Add a goal action opening goal selection, with dismissal returning to Goals.

Chromium and Playwright WebKit are regression coverage, not a substitute for the
installed Windows WebView2 or Linux WebKitGTK package. Native decorations, GPU
drivers, scaling, file dialogs, and packaging must still be checked in the VMs.

## Native checklist

Use an isolated audit data directory and test profile. Do not reset the user's
profile or reuse their unfinished onboarding session. Record the package tag,
commit, OS/architecture, display scale, and evidence with each pass.

- Launch the packaged app and verify a rendered page, not just a live process.
- Confirm Windows/Linux have no Edit / Window / Help menu strip. macOS retains
  its application menu and native editing shortcuts.
- Inspect onboarding, profile selection, navigation, settings, course lists and
  player, empty/error states, search, and dialogs in both themes.
- Resize to a short/tiled window and check scrolling, clipped controls, overflow,
  and navigation access. Check keyboard use and OS display scaling.
- Verify native window controls, file dialogs, and close/relaunch behavior.

## Regressions addressed in October 2026

Linux dark-mode selects could paint a light native background beneath light text.
The app now declares its color scheme and gives single-select controls an explicit
appearance and chevron. Long shared dialogs previously extended beyond short
windows; their body now scrolls while the header and footer remain reachable.
Dialogs also manage focus and nested scroll locks. Compact navigation is below
modal overlays, and clicking the modal backdrop dismisses the dialog.
The shared keyboard-focus style also supplies an outline, so a component's
decorative box shadow cannot hide the focus indicator on onboarding buttons.
Native dark-mode inspection also exposed low-contrast white text on the dashboard
hero's light purple gradient. Its dark-theme background is now darker, and its
eyebrow uses the full text color. Native progress elements use the app's theme
colors instead of the OS's green/gray styling.
Desktop theme changes also update native window decorations. Selecting System
restores OS-controlled decorations; mobile and browser previews skip this API.
Home's Add a goal action now opens the goal picker on the Goals page instead of
sending the user to Skills & Credentials.
The goal picker's prompt remains in the closed control but is excluded from the
choice list. Windows WebView2 painted that disabled row almost black in its dark
native popup even after native window-theme synchronization; actual goal choices
were legible.

The Windows VM's username-availability failure was traced to unavailable internet
access on its Parallels shared adapter. After switching to the working bridged
Wi-Fi network, the native onboarding screen reported an available test name and
rejected the reserved name `admin`. Unlocking the existing test profile restored
Connected status and published its username to the primary relay registry. Network
mode changes are VM-specific diagnostics, not an application requirement.

Windows startup could fail before rendering because Git converted embedded,
hash-verified resources to CRLF. `.gitattributes` preserves exact resource and
built-in plugin bytes. `scripts/test_resource_checkout.py` exercises a Windows-style
checkout, while `scripts/ci/smoke-windows-startup.ps1` launches the packaged Windows
executable, checks startup diagnostics, requires a window, and rejects a native
menu bar. These checks do not assert that the webview painted successfully.

## Omarchy / Parallels AppImage troubleshooting

Use `uname -m` to select the package: `aarch64` needs ARM64; `x86_64` needs x86_64.
Parallels shared downloads may be under `/mnt/psf/Home/Downloads`, including the
artifact's `linux-arm64` or `linux-x64` subdirectory. Copy the actual downloaded
file to `~/Applications/Alexandria.AppImage`, then make it executable.

On the tested ARM64 Omarchy VM, the AppImage failed with `EGL_BAD_PARAMETER` until
the host Wayland client library was loaded. This local launcher worked:

```sh
#!/bin/sh
export LD_PRELOAD="/usr/lib/libwayland-client.so.0${LD_PRELOAD:+:$LD_PRELOAD}"
exec "$HOME/Applications/Alexandria.AppImage" "$@"
```

This is a workaround for that VM's AppImage/host graphics-library combination,
not a global Linux requirement. Verify the library exists before using it. The
installed launcher can be `~/Applications/Alexandria`; point its desktop entry
there. Software-rendering environment flags alone did not resolve this VM's error.
