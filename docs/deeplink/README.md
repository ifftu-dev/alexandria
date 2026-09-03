# Deep Linking

Alexandria opens shared links straight into the app on all five platforms
(macOS, Windows, Linux, iOS, Android). Two link forms resolve to the same
in-app destinations:

| Intent | Custom scheme | HTTPS app-link |
|---|---|---|
| Guardian invite accept | `alexandria://guardian/accept?code=<code>` | `https://alexandria.ifftu.dev/guardian/accept?code=<code>` |
| Course / tutorial | `alexandria://course/<id>` | `https://alexandria.ifftu.dev/course/<id>` |
| Classroom | `alexandria://classroom/<id>` | `https://alexandria.ifftu.dev/classroom/<id>` |
| Generic route (fallback) | `alexandria://open?route=/any/path` | `https://alexandria.ifftu.dev/open?route=/any/path` |

- The **custom scheme** always works once the app is installed — no server, no
  verification. It is the reliable baseline (QR codes, cross-device hand-off,
  in-app "share" buttons).
- **HTTPS app-links** are shareable anywhere and open the app when installed.
  When the OS does not hand the link to the app — not installed, or not yet
  verified — the browser stays on `alexandria.ifftu.dev` and the site's
  `app-open.html` interstitial rebuilds the `alexandria://` URL from the path
  and offers it alongside a download link. HTTPS additionally requires the
  association files below **and** a signed release build (see Android signing).

Received links are **queued until a profile is unlocked** and replayed the
moment one is: a parent tapping a guardian link on a fresh install lands on the
profile picker, and the link fires as soon as they unlock.

## Implementation map
- Parser: `src/deeplink/parse.ts` (+ `parse.test.ts`) — pure, both forms → a
  canonical target. The generic `open?route=` target is validated against the
  router's registered routes before navigating (open-redirect guard).
- Runtime: `src/deeplink/useDeepLinks.ts` — subscribes via
  `@tauri-apps/plugin-deep-link` (`getCurrent` for cold start, `onOpenUrl` for
  warm), queues on the locked screens, replays on `onProfileReady`.
- Backend: `tauri-plugin-deep-link` + desktop `tauri-plugin-single-instance`
  registered in `src-tauri/src/lib.rs`; scheme declared in
  `src-tauri/tauri.conf.json` (`plugins.deep-link`).
- Platform manifests: `gen/android/app/src/main/AndroidManifest.xml`
  (intent-filters), `gen/apple/alexandria-node_iOS/{Info.plist,*.entitlements}`.

## HTTPS app-links — hosting contract (marketing site)
The custom scheme needs nothing external. HTTPS app-links additionally require
these two files served on **alexandria.ifftu.dev**:

1. `https://alexandria.ifftu.dev/.well-known/apple-app-site-association`
   - Serve the contents of [`well-known/apple-app-site-association`](well-known/apple-app-site-association).
   - `Content-Type: application/json`, **no extension**, **no redirect**, over
     valid TLS. appID is `VLMNL3V44U.org.alexandria.node`.
2. `https://alexandria.ifftu.dev/.well-known/assetlinks.json`
   - Serve [`well-known/assetlinks.json`](well-known/assetlinks.json), which
     carries the SHA-256 of the release signing certificate. See below for where
     that value comes from.

Both are live, served from `public/.well-known/` in the `alexandria-website`
repo. The path rewrites that put `/guardian/*`, `/course/*`, `/classroom/*` and
`/open` on the interstitial live in that repo's `public/_redirects`, not in
`netlify.toml` — Netlify evaluates `_redirects` first, and Nitro's
`netlify-static` preset appends a `/* /404.html 404` catch-all to it.

## Android signing — App Links depend on it
Auto-verification matches the installed APK's signing certificate against the
fingerprint in `assetlinks.json`. Both halves have to be right:

- **The published fingerprint.** Read it from the release keystore, which lives
  only as the `ANDROID_KEYSTORE` repository secret: run the **Android Signing
  Fingerprint** workflow (`.github/workflows/android-signing-fingerprint.yml`,
  manual dispatch) and copy the `SHA256:` line into the website's
  `assetlinks.json`. Locally, `keytool -list -v -keystore <release.jks> -alias
  <alias>` gives the same value.
- **The artifact.** Release builds sign from `keystore.properties`, which CI
  writes from that secret (`.github/workflows/mobile-shared.yml`) and
  `gen/android/app/build.gradle.kts` reads. Releases up to and including
  `v0.4.5-alpha` predate that wiring and are **unsigned** — they cannot install
  on a device and no https link can verify against them. Links verify only
  against builds cut after it.

If Alexandria is ever distributed through Google Play with Play App Signing, the
value to publish becomes the **app signing certificate** from Play Console, not
this upload key; publishing the upload key would break links for Play installs.

The custom scheme is unaffected by any of this — it needs no server, no
association file and no verification.

## Verifying
Custom scheme — works on any installed build:
- **macOS**: `open 'alexandria://course/abc'` / `open 'alexandria://guardian/accept?code=TEST'`.
- **Windows**: `start "" "alexandria://open?route=/settings"`. **Linux**: `xdg-open '…'`.
- **iOS sim**: `xcrun simctl openurl booted 'alexandria://guardian/accept?code=TEST'`.
- **Android**: `adb shell am start -W -a android.intent.action.VIEW -d 'alexandria://classroom/xyz' org.alexandria.node`.
- Fire a link while on the profile picker → it should replay right after unlock.

HTTPS app-links — needs a signed release build installed:
- Same commands with the `https://alexandria.ifftu.dev/…` form.
- **Android verification state**: `adb shell pm get-app-links org.alexandria.node`.
  Android caches failures, so after fixing the association or the signing, reset
  with `adb shell pm set-app-links --package org.alexandria.node 0 all` and
  reinstall.
- **The hosted statement**, independent of any device:
  `curl 'https://digitalassetlinks.googleapis.com/v1/statements:list?source.web.site=https://alexandria.ifftu.dev&relation=delegate_permission/common.handle_all_urls'`
  — an empty `errorCode` means Google's verifier accepts it.
- **Guardian links need a real invite code**: the parser returns null (link
  ignored) without `?code=`, so smoke-test with `/open` or `/course` instead.
