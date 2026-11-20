# Isolation proof and the SCORM 1.2 conformance matrix

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/06-player.md

Spec: Testing Decisions → player seam; What conformance means here → SCORM 1.2 row.

- Hostile fixture packages, each trying one escape, with a test asserting it failed: a network request, a peer connection, a frame navigation, a pop-up, reading another launch's address, reading storage left by an earlier launch, reaching the command bridge, covering the label in and out of focus mode.
- A SCORM 1.2 conformance matrix: every mandatory and optional run-time data model element and every API call and error code mapped to an automated test, using published sample packages plus small purpose-built ones.
- Ranged media requests and commit coalescing asserted at this seam.
- Runs in Chromium and WebKit in continuous integration alongside the existing desktop interface checks.

Done when: every row of the matrix names a passing test, every escape fixture fails to escape in both engines, and the matrix is committed beside the tests.

## Notes

2026-11-20, closed.

- Matrix: `tests/desktop-ui/scorm12-conformance.md`, beside `scorm12-conformance.spec.ts` (probe package `tests/desktop-ui/scorm12-conformance/`). Every API call, error code (0, 101, 201, 202, 203, 301, 401, 402, 403, 404, 405) and data model element, mandatory and optional, names a passing test in Chromium and WebKit. Library gaps 3 to 5 are rows marked known deviation (suspend data to 64000, identifiers with spaces, timespan hours above 9999, unchecked response patterns, 401 for an unknown leaf, stale diagnostic text); the nesting limit is a row marked known limit. A test fails if a row names a test that does not exist.
- Error code after `LMSFinish`: 301 confirmed. SCORM 1.2 has no terminated state; after `LMSFinish` the API is back to not initialised, so every call but the three error functions fails with 301, and nothing more reaches the host. `LMSInitialize` after finish stays the library's 101.
- Isolation: `tests/desktop-ui/package-isolation.spec.ts` with the hostile package `tests/desktop-ui/package-isolation/`, one lesson per escape, plus a control proving the checks see an escape without the bootstrap. The harness (`packageHarness.ts`) cannot run the Rust handler: it reads `CONTENT_POLICY`, `PERMISSIONS_POLICY`, `DOCUMENT_POLICY_SUFFIX` and `ServeLimits::DEFAULT` out of `handler.rs` and the app's `frame-src` out of `tauri.conf.json`, so a change there changes what the browser enforces here. The handler's preload rewrite is mirrored in the harness and pinned by its own Rust test.
- Escapes found and fixed:
  - Frames content makes (an empty frame read through `frames[i]` or by name, `srcdoc` frames, javascript: documents, frames in shadow trees) had the peer connection and the command bridge back. The bootstrap now scrubs a new frame from its synchronous `load` event, gives `srcdoc` documents the bootstrap, watches shadow roots, and scrubs once per document (a frame scrubbed at its first blank page keeps its window for the next page).
  - A javascript: address replaced a page with one the bootstrap never ran in. Pages are now served with `require-trusted-types-for 'script'` (handler `DOCUMENT_POLICY_SUFFIX`); the bootstrap's default policy runs such an address without letting its result replace the page, and passes every other sink through. WebKit keeps a stale policy on a reused blank window, so the policy is not installed on a frame that is about to navigate.
  - WebKit sends `<link rel=preload as=fetch>` to any origin regardless of `connect-src`. The handler renames that `as` in served documents (pages and, now always sent whole, SVG/XHTML/XML) and the bootstrap blocks setting it at run time.
  - On a fresh attempt the package frame kept its name and its history, which named the earlier launch's prefix. The player now makes a new frame element per session and the clearing page clears the frame name.
  - Clicking a lesson while the resume lesson was starting could leave the table of contents and the frame on different lessons. Only the latest lesson request launches now.
- Ticket 06 spec flakiness: its checks read the lesson frame before the replacing lesson loaded. Fixed by waiting for the replacing page; 96 of 96 runs passed in both engines.
- Deviation from the ticket: no published sample package is committed. The standards body's 1.2 test suite needs a Java applet; purpose-built packages only.
- Residual, not fixed: a closed declarative shadow root holding a `srcdoc` frame is out of the bootstrap's reach; a WebKit fetch preload in an XML document parsed through `responseXML` and adopted into a page. A webview-level switch for peer connections would close the first structurally. The harness origin is not a secure context, so service worker registration is covered by the Rust handler test and the bootstrap, not here.
- CI: the specs run in the existing `desktop-ui` job in both engines; the change classifier runs the backend description test for the two new fixture directories.

Proposed doc edits, not made here:

- `docs/learning-standards-spec.md`, Serving and isolation: pages carry the Trusted Types requirement; fetch preloads are stripped; a new frame per launch.
- `docs/desktop-ui-validation.md`: the package harness, its mirrored headers and the `?package=` fixtures.
- The conformance statement, when written: link the matrix.
