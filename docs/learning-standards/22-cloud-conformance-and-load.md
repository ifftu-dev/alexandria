# Conformance suite in continuous integration and the load test

Triage: ready-for-agent
Repos: alexandria-cloud
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/21-cloud-document-resources.md

Spec: Record store (hosted service) → "Queries are designed for size"; Testing Decisions → record store seam; What conformance means here → xAPI row.

- The official open-source xAPI conformance suite runs against a local instance for 1.0.3 and 2.0 in continuous integration. A regression fails the build.
- A load test at a stated record count, with the count, the queries and the timings recorded. Statement queries stay within a stated bound as the store grows.
- The statement builder's output from the app is fed to the record store so the two halves are shown to agree, including that the confirmed activity identifiers are accepted.

Done when: the suite passes for both versions in continuous integration, the load test result is recorded here, and any requirement not met is listed as a known gap.
