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
- **HTTPS app-links** are shareable anywhere and open the app when installed;
  when it isn't, the browser stays on `alexandria.ifftu.dev`, which should show
  a "download / open in app" page. HTTPS requires the association files below.

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
   - Serve [`well-known/assetlinks.json`](well-known/assetlinks.json).
   - Replace `REPLACE_WITH_RELEASE_KEYSTORE_SHA256` with the SHA-256 fingerprint
     of the Android **release** signing certificate:
     `keytool -list -v -keystore <release.jks> -alias <alias>` → copy the
     `SHA256:` line. Android App-Link auto-verification is blocked until this is
     a real release-keystore fingerprint; the custom scheme is unaffected.

The marketing site should also render a fallback page at each path
(`/guardian/accept`, `/course/<id>`, `/classroom/<id>`, `/open`) for visitors
without the app installed.

## Verifying
- **macOS**: `open 'alexandria://course/abc'` / `open 'alexandria://guardian/accept?code=TEST'`.
- **iOS sim**: `xcrun simctl openurl booted 'alexandria://guardian/accept?code=TEST'`.
- **Android**: `adb shell am start -W -a android.intent.action.VIEW -d 'alexandria://classroom/xyz' org.alexandria.node`.
- Fire a link while on the profile picker → it should replay right after unlock.
