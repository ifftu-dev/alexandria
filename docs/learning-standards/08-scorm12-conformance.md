# Isolation proof and the SCORM 1.2 conformance matrix

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/06-player.md

Spec: Testing Decisions → player seam; What conformance means here → SCORM 1.2 row.

- Hostile fixture packages, each trying one escape, with a test asserting it failed: a network request, a peer connection, a frame navigation, a pop-up, reading another launch's address, reading storage left by an earlier launch, reaching the command bridge, covering the label in and out of focus mode.
- A SCORM 1.2 conformance matrix: every mandatory and optional run-time data model element and every API call and error code mapped to an automated test, using published sample packages plus small purpose-built ones.
- Ranged media requests and commit coalescing asserted at this seam.
- Runs in Chromium and WebKit in continuous integration alongside the existing desktop interface checks.

Done when: every row of the matrix names a passing test, every escape fixture fails to escape in both engines, and the matrix is committed beside the tests.
