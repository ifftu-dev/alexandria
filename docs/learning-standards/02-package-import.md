# Package import as a pure function

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md

Spec: Package storage and import (first four bullets and the shared-crate bullet); ADR 0004.

- In the desktop-free shared crate: archive bytes in, either a parsed package description or a stable refusal code out. No storage, no commands.
- Detects the standard and edition. Parses a SCORM 1.2 manifest into an activity tree and checks that every launchable resource exists in the archive. SCORM 2004 and cmi5 are detected and refused as unsupported until tickets 09 and 24 add them.
- Refusal codes: not an archive, no manifest, manifest unparseable, unsupported standard or edition, launch resource missing, launch resource remote, unsafe entry path, size or count limit exceeded.
- Bounded, path-safe extraction to a caller-supplied directory: absolute paths, parent traversal and links refused; uncompressed size, entry count and compression ratio capped at the spec's starting limits.

Done when: fixture archives built in the tests produce each refusal code and a correct activity tree for single- and multi-lesson SCORM 1.2 packages; a hostile archive cannot write outside the target directory or exceed a limit.
