# Serving package content in isolation

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/03-package-storage.md

Spec: Serving and isolation (own scheme, launch-scoped addresses, origin storage, content policy, removed interfaces, device capabilities); Serving performance.

- A dedicated scheme for package content, separate from the plugin scheme, registered for desktop and mobile. The app's frame policy allows it.
- Opening a launch returns an unguessable path prefix bound to one package and valid only while that launch is open. Nothing else is served.
- The handler is asynchronous, honours byte-range requests, reads only the requested range and caps whole-file responses.
- Every response carries the package content policy. Every HTML response gets the injected bootstrap that removes the command bridge and the network-capable interfaces the policy does not govern.
- Storage at the package origin is cleared before and after each launch and service worker registration is blocked.
- No camera, microphone, location, clipboard read or full screen. A new-window call is redirected into the player frame.

Done when: handler tests show a ranged read returns only the range, an expired or foreign launch prefix is refused, headers carry the policy, and HTML carries the bootstrap. Browser-level escape attempts are proved in ticket 08.

## Notes

2026-11-20: built.

- `crate::package_serving` (`src-tauri/src/package_serving/`) holds the scheme handler (`serve`, a plain request-to-response function), the launch registry (`Launches`) and `open_launch`. `commands/package_launches.rs` adds `package_launch_open` (hash and a progress channel, returns `OpenedPackageLaunch`) and `package_launch_close` (prefix, returns whether it was open). Both are in the command allowlist until the player calls them.
- Scheme `package`, registered once for every platform with Tauri's asynchronous API. URL shapes: `package://localhost/<prefix>/<path>` on macOS, iOS and Linux, `http://package.localhost/<prefix>/<path>` on Windows and Android. The main window's `frame-src` lists both; the two style hashes are unchanged and the hash check passes.
- A prefix is 128 random bits in hex, bound to one package's extracted directory. Anything outside an open prefix, a closed prefix, a parent step, an absolute path or a link out of the package gets an empty 404. Launches close when the profile stops. Two launches of one package opened at once extract it once (a per-package lock, the note from ticket 03).
- A range request reads only that range; an open-ended or long range is answered with at most 8 MiB. Files above 32 MiB are not sent whole: a request without a range gets 416 with the size, so media can continue by range. HTML is always sent whole, because the bootstrap goes into it.
- Every response, refusals included, carries the content policy (inline script and evaluation allowed; requests, frames, media, fonts, images, workers and forms limited to `'self'`; objects blocked), a permissions policy granting no camera, microphone, location, clipboard read, full screen or hardware access, `nosniff`, `no-store` and `no-referrer`. SVG, XHTML and XML get a `sandbox` policy, since they can run script but cannot carry the bootstrap. Requests carrying `Service-Worker` are refused.
- Every HTML page gets `<script src="/<prefix>/_alexandria/bootstrap.js">` as its first element (after the opening head tag, else after the html tag or doctype, else at the start). It is linked rather than inlined so the page's own charset declaration stays inside the first 1024 bytes, and the page bytes are left untouched so legacy encodings survive. The bootstrap removes the Tauri globals and the native message channels (`webkit.messageHandlers`, `chrome.webview`, `ipc`), peer connections, `BroadcastChannel`, `SharedWorker` and device interfaces; makes service worker registration fail; turns `window.open` and links aimed at another window into a navigation of the same frame; and turns full screen into a `focus-mode-request` message to the host.
- `<prefix>/_alexandria/` is answered from files built into the app and never from the package: `bootstrap.js`, `clear.html`, `scorm12.min.js`, `scorm2004.min.js`. `clear.html` clears local and session storage, IndexedDB, cache storage, service worker registrations and root-path cookies, sends `storage-cleared` to the host, and carries `Clear-Site-Data`. The player (ticket 06) loads it before and after each launch.
- Tests: 21 handler-seam tests in `package_serving/tests.rs` plus a profile-stop test in `profile/lifecycle_tests.rs`. A scratch run of the bootstrap and the clearing page in Chromium and WebKit, not committed, showed each removal, the redirects, the focus request and the clearing working; ticket 08 owns the real escape tests.
- Not established: whether each platform's webview delivers `Range` and `Service-Worker` headers to a custom-scheme handler (Android and iOS are the doubtful ones), whether WKWebView plays `<video>` from a custom scheme at all (ticket 28), whether `Clear-Site-Data` is honoured at these origins, and whether a frame the content makes itself (reached through `frames[i]` without touching the frame element) can reach the native message channels.
- Interpretation: the spec says "path prefix"; the handler uses one host, `localhost`, on every platform, so packages share one origin everywhere and the prefix and storage clearing do the isolating, as the spec requires on all platforms.

Proposed doc edits, not made here:

- `GLOSSARY.md`: add **Package launch**: an open, prefix-bound serving of one package's files.
- `docs/project-structure.md`: list `src-tauri/src/package_serving/` and `commands/package_launches.rs`.
- `docs/plugins.md`: beside the `plugin://` scheme description, note that package content has its own `package` scheme with a looser script policy, confined by the launch prefix, and is never served under `plugin://`.

### Review (2026-11-21)

- The bootstrap was injected after the first `<head` match anywhere in the page, so script before the head, a commented head or `<head` in a string kept it from running first. It now goes at the document start: after only a byte-order mark and, when a doctype follows leading whitespace, comments or an XML prolog, after that doctype (the page keeps its rendering mode). Test: `the_bootstrap_starts_the_document_whatever_comes_before_the_head`.
- The content policy is built from directive parts; a cmi5 launch's endpoint is added by rebuilding it, not by editing the header text.
- Known deviation, as ticket 03 records: a launch waits for the whole archive to be extracted.
