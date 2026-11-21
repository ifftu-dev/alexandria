# Prototype the cmi5 on-device endpoint's transport

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/06-player.md

Spec: cmi5 → transport bullet and own-window bullet.

- A throwaway prototype that answers one question: how does unmodified cmi5 content, running in the package frame, send xAPI requests with bodies to an endpoint on the device, on every supported platform, and does a session survive the app being backgrounded and resumed.
- A loopback listener is the expected answer. A custom scheme is acceptable only where request bodies reach the handler.
- Settle how the package content policy admits that one endpoint and nothing else.
- Settle what a unit that requires its own window does on desktop and on mobile.

Done when: the answer for desktop is demonstrated and recorded here under Notes, with what was tried and what failed; anything that can only be confirmed on a physical phone is listed for ticket 28. The prototype code is not merged.

## Notes

2026-11-20: answered. The prototype stays outside the repository and is not merged.

### Answer

- **Transport: a loopback listener on every platform.** One listener per open cmi5 session, bound to `127.0.0.1` on a port the OS picks, plain HTTP/1.1. The unit stays in the ticket 06 package frame on the package scheme (`package://localhost` on macOS, iOS and Linux; `http://package.localhost` on Windows and Android) and calls the listener cross-origin, with CORS. The launch parameters carry `endpoint=http://127.0.0.1:<port>/xapi/` and `fetch=http://127.0.0.1:<port>/fetch/<one-time code>`.
- **The custom scheme is rejected as the endpoint.** Android never passes request bodies to the handler: wry 0.55 builds every Android scheme request with an empty body, and Tauri's own IPC falls back to `postMessage` on Android for that reason. On WKWebView a string body arrives but a `Blob` body does not: neither `httpBody` nor `httpBodyStream` is set, and wry reads only those two. Windows and Linux would pass bodies (Linux needs WebKitGTK 2.40 or later through wry's `linux-body` feature). One transport everywhere costs less than a scheme endpoint on four platforms plus a listener on Android.
- **The session survives backgrounding.** Session state (token, one-time fetch flag, records) lives in the app, not in the socket. On desktop nothing happens to a loopback listener when the window is hidden. On iOS a suspended app's listening socket can be reclaimed by the system, so the listener rebinds the same port on resume, and the unit carries on with the same endpoint URL and token. The same-port rebind was demonstrated; the reclaim itself needs a phone.
- **Own window.** Desktop: a separate window. Mobile: the package frame in forced focus mode. Details below.

### Evidence (demonstrated on macOS, headless, no app window)

- A Rust listener (hyper 1 + tokio, both already in `Cargo.lock`) implementing the fetch URL, `statements` (POST, PUT, GET), `activities/state` (GET, PUT, POST, DELETE, plus a computed `LMS.LaunchData`), `agents/profile` and `about`. It checks Host, Origin and token. A stdin control drops and rebinds the socket.
- Pages served at the package origin with the real `CONTENT_POLICY`, `PERMISSIONS_POLICY` and `bootstrap.js` from `package_serving/` (read from the source files). They are framed by an app page in the ticket 06 sandbox. Two units:
  - a hand-written one: fetch and XHR, JSON bodies, batch POST, a 1 MiB PUT, a keepalive POST, a wrong token, and policy probes;
  - an unmodified off-the-shelf cmi5 client library (`@xapi/cmi5` 1.4.0, axios over XHR) running `initialize`, `progress`, `complete`, `pass`, `terminate`.
- **Real WKWebView** (macOS 27.3, a Swift harness with no window). App page at `tauri://localhost`, unit at `package://localhost` through a `WKURLSchemeHandler`. Both units passed every step. The listener saw `Origin: package://localhost` and real preflights. The frame is a secure context, and `http://127.0.0.1` is not blocked as mixed content. The same harness posted bodies to the scheme handler itself: JSON 17 B and a 1 MiB string arrived in `httpBody`; a `Blob` arrived empty.
- **Playwright WebKit 27.2** (`http://package.localhost` shape): every step passed, with real preflights.
- **Playwright Chromium 156** (`http://package.localhost` shape, the Windows and Android form): every step passed with Local Network Access off and with the permission granted. With default flags every request was refused before reaching the listener, with the console error "Permission was denied for this request to access the loopback address space". See the Local Network Access section. One variant served the fixture pages from a real server instead of Playwright interception, because Playwright answers CORS preflights itself when it intercepts. Real preflights reached the listener there too.
- **Policy**, all engines: `connect-src 'self' http://127.0.0.1:<port>` admitted the endpoint. It refused `http://localhost:<port>`, `http://127.0.0.1:<port+1>` (fetch and XHR), `http://[::1]:<port>` and a remote https URL, each reported as a `connect-src` violation.
- **Hostile callers**, all engines:
  - another page origin holding the token is refused at the preflight (403, no CORS headers);
  - its no-cors form POST is sent but refused 403 by the Origin check;
  - a forged Host (DNS rebinding) gets 421;
  - a request with no Origin, even with the token, gets 403;
  - a wrong token gets a readable 401;
  - a second use of the fetch URL returns `error-code` 1.
- **Survival**, all engines:
  - a request sent while the listener process was stopped for 3 s (SIGSTOP) completed after SIGCONT;
  - a request sent while the socket was dropped failed with a network error;
  - after rebinding the same port, the next request succeeded with the same token;
  - dropping and rebinding with no request in between also succeeded: the browsers retried their dead pooled connection without the page seeing anything.

Tried and failed:

- Serving the unit itself from the listener, with the document and endpoint on one loopback origin and no policy change. It works in WebKit, but in Chromium with default flags the iframe navigation from the app page to loopback is blocked by Local Network Access as well, so it gains nothing. It also needs `frame-src http://127.0.0.1:*` in the main window and a second serving path. Kept only as a fallback if a device shows the package scheme failing for media.
- Granting via the iframe's `allow` attribute could not be isolated: Playwright grants per origin, so the frame worked with or without `allow`.

### Local Network Access (Windows and Android)

- Chromium treats a document served by an embedder or interceptor as public. Fetches from it to loopback need the Local Network Access permission (`loopback-network`, alias `local-network-access`).
- Android WebView: Chromium's intent to ship says the permission is granted unconditionally in WebView, because apps cannot grant new permission types yet. Android 17's `ACCESS_LOCAL_NETWORK` (target SDK 37) is about LAN addresses; loopback is not named. The app targets SDK 36 today.
- Windows WebView2: Local Network Access is disabled today. Microsoft's rollout notice says it stays off until new values in `CoreWebView2PermissionKind` ship for `PermissionRequested`, and that cross-origin iframes will need `allow="local-network-access"`. When that lands, the host grants the permission for the package origin only, and the cmi5 frame (only that frame) gets the `allow` token. Ticket 25 needs no code for this now.

### Content policy rule

- Responses under a cmi5 launch prefix use `CONTENT_POLICY` with `connect-src 'self' http://127.0.0.1:<port>`, where port is that session's listener port. SCORM launches keep `connect-src 'self'`. The literal IPv4 address, never `localhost` or `[::1]`.
- The CSP cannot narrow by path through redirects, so the rest is enforced by the listener. Checks run in this order:
  - Host must equal `127.0.0.1:<port>`, else 421;
  - Origin must equal the platform's package origin (`package_serving::origin()`), else 403 with no CORS headers, including when Origin is missing;
  - xAPI resources need `Authorization: Basic <token>` (401, readable) and `X-Experience-API-Version` (400);
  - the fetch URL is POST, one use, and returns `{"auth-token": …}`;
  - the token is 32 random bytes and dies with the session;
  - the listener exists only while the session is open.

### Own window

- **Desktop:** a new webview window with its own label, not listed in any capability, so it has no commands. It loads the unit's launch URL on the package scheme under the same launch prefix, endpoint and policy. The player shows that the unit is open in its own window, with "bring to front" and "end". Closing the window ends the unit; without a `terminated` statement the session rules write `abandoned`. `returnURL` points at a new reserved page `_alexandria/returned.html`. In the own window the app closes the window when that page loads; in the frame it posts an exit message to the player.
- **Mobile:** Tauri 2 mobile has one webview, so the unit plays in the package frame with focus mode forced on and no exit. cmi5 8.1 allows only a new window or redirecting the current one. Redirecting the main webview was rejected: Tauri classifies any URL on a registered custom scheme as a local origin for its IPC permission check, so the unit would run top-level in the window that holds the app's capabilities, guarded only by the bootstrap's deletion. A second native webview needs a native plugin and is out of scope. The full-screen frame is recorded as a deviation for the conformance statement (ticket 27). A unit that refuses to run framed will not work on mobile.

### For ticket 28 (needs a device)

iOS, release build, physical phone:

1. Fetch and XHR with bodies from `package://localhost` to `http://127.0.0.1:<port>` succeed (no ATS block), and no Local Network prompt appears.
2. The Origin header is `package://localhost`.
3. Background mid-session (home, app switcher, lock, more than 30 s), then resume. Is the listening socket reclaimed? Does the rebind on `WindowEvent::Resumed` keep the port, and does the next statement succeed with the same token?
4. What happens to a request in flight at the moment of suspension?
5. The app is killed while suspended: the next open writes `abandoned`.

Android, release build, physical phone:

6. Cleartext to `127.0.0.1` with a network security config that permits only that host. Release sets `usesCleartextTraffic=false`. Confirm that without the config the WebView refuses.
7. No Local Network Access refusal or prompt in the installed WebView version.
8. The Origin header is `http://package.localhost`, and preflights reach the listener.
9. Background (home, recents, screen off for minutes, Doze), then resume: the listener is still alive and the next statement succeeds.
10. Process death in the background leads to `abandoned` on the next open.
11. When target SDK moves to 37: loopback is unaffected by `ACCESS_LOCAL_NETWORK`.
12. OwnWindow units fill the screen in forced focus mode.

Desktop platforms not run here (not phones, but no machine was available):

13. Windows WebView2: the runtime version; the unit works as is; a preview with `--enable-features=LocalNetworkAccessChecks,msWebViewAllowLocalNetworkAccessChecks` fails until the permission is granted; binding `127.0.0.1` raises no firewall prompt.
14. Linux WebKitGTK: the Origin header is `package://localhost`, and CORS from the custom scheme to http works.
15. macOS: the real wry window instead of the standalone WKWebView harness, and the own window has no commands.

Proposed doc edits, not made here:

- `docs/learning-standards-spec.md` → cmi5: transport is a per-session loopback listener with the policy rule above. Mobile own-window behaviour is the forced-focus frame, a declared deviation.
- `docs/learning-standards/28-device-and-hosted-runs.md`: add the checklist above.
- `docs/learning-standards/08-scorm12-conformance.md`: Tauri classifies custom-scheme URLs as local origins for IPC, which raises the stakes of the bridge-escape fixture.
