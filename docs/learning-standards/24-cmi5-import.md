# Importing cmi5 packages

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/02-package-import.md

Spec: cmi5 → first bullet; Package storage and import.

- Ticket 02's import accepts a cmi5 package: parses the course structure into the activity tree, with blocks, assignable units, launch methods, completion criteria and mastery scores.
- Only assignable units packaged in the archive are supported. A course structure that launches a remote unit is refused with the existing "launch resource remote" code.
- The same refusal codes and extraction limits apply.

Done when: fixture course structures, including the published cmi5 examples, parse to the expected tree, and a remote unit is refused.
