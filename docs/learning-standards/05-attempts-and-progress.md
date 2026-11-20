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

## Notes

2026-11-20: decisions taken after reading the code, before work starts.

- The completion policy format has no field that names an element, so the refusal cannot be written as stated. Read it as: nothing can make a package element assessed. Concretely: the set of assessed element types is defined in one place that every reader uses, the package type is not in it and a test pins that; assembling completion evidence never takes a package element; nothing writes a graded submission for one; and document validation refuses a package element that carries a skill tag or any grading configuration, because a skill tag would feed its self-reported score into consistency evidence.
- A course with no assessed element issues a self-claim credential as soon as its last element completes. A course whose elements are all packages, which is what opening a package file creates, would earn one from a self-report. Rule: package elements do not count toward content-only completion; a course made only of packages issues no credential, and in a mixed course the self-claim is issued as today and carries no package result. This is an assumption for the owner to confirm.
- A package score written to element progress still reaches the instructor's average and the guardian mirror. That is intended: the spec labels it self-reported in the interface (ticket 06).
