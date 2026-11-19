# Package attempts, progress rollup and credential exclusions

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/03-package-storage.md

Spec: Attempts and progress; Credentials; Serving and isolation → pseudonymous learner identifier; ADR 0003.

- Package attempts and activity state tables in the baseline schema.
- Commands to open or resume an attempt, persist a coalesced commit in one write, terminate, and start a fresh attempt. Initialisation data for the runtime: pseudonymous learner identifier derived from the learner's key and the package hash, display name, launch data, mastery score and prior state.
- Rollup into existing element progress for SCORM 1.2: complete on a lesson status of completed or passed; score normalised to the scale element progress uses; time spent as the sum of session times.
- A package result never becomes a graded submission, credential evidence or a reputation input. A completion policy that names a package element as an assessed element is refused by document validation.
- Attempt state is device-local; element progress keeps syncing as today.

Done when: host-seam tests open an attempt, feed commits and termination, and assert stored state, resume, a second attempt, element progress, the identifier being stable per package and different across packages, and the refusal of a completion policy that names a package element.
