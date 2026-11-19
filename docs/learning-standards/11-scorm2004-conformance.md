# The SCORM 2004 conformance matrix

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/08-scorm12-conformance.md, docs/learning-standards/10-scorm2004-runtime.md

Spec: Testing Decisions → player seam; Further Notes → Official SCORM test suites.

- First, find out whether any part of the official SCORM 2004 test suites can be driven through the player seam, and record the answer here. The matrix does not depend on it.
- A SCORM 2004 4th Edition conformance matrix: content aggregation, run-time environment, and sequencing and navigation, each mandatory requirement mapped to an automated test. Uses the standards body's published test and sample packages plus small purpose-built ones.
- Packages built for the 2nd and 3rd Editions are shown to run on the same runtime.
- Any requirement the vendored library does not meet is fixed upstream or in a vendored patch, and listed as a known gap until it is.

Done when: every row names a passing test or a recorded gap, in Chromium and WebKit, and the matrix is committed beside the tests.
