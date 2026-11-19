# Importing SCORM 2004 packages

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/02-package-import.md

Spec: Package storage and import; What conformance means here → SCORM 2004 row.

- Ticket 02's import accepts SCORM 2004 manifests from the 2nd, 3rd and 4th Editions and reports the edition.
- The activity tree carries what the sequencing engine needs: sequencing definitions, objectives, rollup and navigation presentation settings, and shared sequencing collections.
- The same refusal codes and extraction limits apply.

Done when: fixture manifests from each edition parse to the expected tree, with sequencing information intact, and malformed sequencing is refused with a stable code.
