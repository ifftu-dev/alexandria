# SCORM 2004 runtime, sequencing and navigation

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/06-player.md, docs/learning-standards/09-scorm2004-import.md

Spec: Runtime; Attempts and progress → sequencing state and SCORM 2004 rollup; user stories 5 and 6.

- The wrapper exposes the SCORM 2004 API object from the vendored library beside the 1.2 one, chosen by the package's standard.
- The sequencing engine comes from the library. Its global objective and tracking state is stored per attempt in a sequencing state table and restored on resume.
- The player provides previous, continue, suspend and exit. Each is enabled, disabled or hidden as the engine and the package's presentation settings say.
- A package element is complete when the root activity's completion status is completed; score and time roll up as for 1.2.

Done when: a multi-activity SCORM 2004 package with sequencing rules runs in the harness, a locked activity cannot be reached, suspend and resume restore the engine's state, and host-seam tests cover the rollup.
