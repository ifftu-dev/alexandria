# Confirm and vendor the SCORM runtime library

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md

Spec: Runtime; Further Notes → Runtime library.

- Confirm the candidate runtime library against the wrapper design: one page at the package origin holds the run-time API objects, content in a nested frame finds them by each standard's discovery algorithm and calls them synchronously, and the host receives state asynchronously.
- Record its licence, the exact version pinned, and what it covers for SCORM 1.2, SCORM 2004 (2nd to 4th Edition) and sequencing and navigation. List every gap found.
- Vendor the pinned build into the repository with its licence file. No package-manager fetch at build or run time.
- If it falls short, say where and whether an upstream fix or a vendored patch closes the gap. Do not start a rewrite.

Done when: the pinned library and its licence are vendored; a minimal page loads it and a one-file SCORM 1.2 fixture initialises, sets a value, commits and terminates against it in the browser harness; the findings are appended here under Notes.

## Notes

2026-11-20: findings.

- **Library and version.** `scorm-again` 3.5.0, MIT licence, no run-time dependencies. Vendored under the app's resources directory with its licence, a checksum file and a README that records provenance, the integrity hash of the published tarball, update steps and the rule for patches. The two classic-script builds (SCORM 1.2 and SCORM 2004) are used; they need no module handling and contain no script evaluation. Upstream releases often, so every upgrade is treated as a behaviour change and the conformance matrices are rerun.
- **Wrapper design confirmed** in Chromium and WebKit through the browser harness: one page at the package origin holds the run-time API, a lesson in a nested frame (depth 1 and 2) finds it by the published discovery algorithm and calls it synchronously, prior state is loaded before the lesson starts, and the host at another origin receives state on commit and on terminate by message. The library's own cross-frame pair is not used: it answers from a cache, which the standards do not allow.
- **SCORM 1.2 coverage.** All eight functions and every data model group, with read-only, write-only, vocabulary and range checks. Gaps: (1) a lesson that never sets its status finishes as not attempted; closed without a patch by a setting plus one event handler, verified in eight cases, and the upstream fix is one condition; (2) calls after finish succeed; closed by a facade in the wrapper, ticket 08 picks the error code; (3) documented relaxations (longer suspend data, looser identifiers and numbers, unvalidated response patterns), recorded as known deviations, not patched; (4) one error code differs from the standards body's suite for an unknown element; (5) a cosmetic diagnostic message. A package with several lessons gets no help from the published builds: the wrapper and host own the table of contents, navigation, one session per lesson and rollup. Ticket 05 must decide the rollup rule; all launchable lessons completed or passed is proposed.
- **SCORM 2004 run-time coverage.** All eight functions, the state errors, the 4th Edition data model, status derived from passing score and completion threshold. One behaviour serves the 2nd to 4th Editions. Gaps: completion and success status reads leave a stale error code (upstream fix or patch; the facade can reset it meanwhile); the navigation-validity elements look ahead too little (drive the player from the library's accurate preview call; content that reads them needs a fix or patch); launch data per activity is the host's job, so ticket 09's tree must carry it.
- **Sequencing and navigation.** Every process of the pseudo-code is present; about sixty scenarios were verified against the pinned build. Gaps that need an upstream fix or a vendored patch: a forward-traversal stop rule does not stop choice, and the choice constraint is applied to the wrong activity. None stops a package from running; ticket 11 lists them as known gaps until fixed. A patch means rebuilding the scripts from the tagged source with the patch kept beside them.
- **Limit of nesting the wrapper in the app.** A lesson whose discovery does not stop at the wrapper climbs to the app window and gets a security error where an ordinary system would find nothing. Conforming discovery is unaffected. Recorded for ticket 08's matrix.
- **Also added.** The CI change classifier runs the browser checks when the vendored directory changes, with a test.
- Proposed doc edits (not made): name the vendored runtime and its update rule in the architecture document and the third-party notices when the player lands.
