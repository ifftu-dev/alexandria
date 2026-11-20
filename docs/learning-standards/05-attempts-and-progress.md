# Package attempts, progress rollup and credential exclusions

Triage: done
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

2026-11-20: built.

- `crate::packages::attempts` (`src-tauri/src/packages/attempts/`) holds storage and the attempt lifecycle; the standard-specific reading of run-time data, the initial run-time data, the learner identifier and the rollup rule are in `alexandria_studio::package::progress` (ADR 0004). Commands are in `commands/package_attempts.rs`. Host-seam tests: `packages/attempts/tests.rs` (12) and the credential tests at the end of `commands/completion.rs` (5).
- Migration 45 `package_attempts` adds `package_attempts`, `package_activity_state` and `package_sequencing_state` (created for ticket 10, unused here) and four triggers. Other tickets add migrations in parallel; renumber at merge. `docs/database-schema.md` is regenerated with it, as CI requires.
- Commands: `package_attempt_open`, `package_attempt_fresh`, `package_attempt_initialise`, `package_attempt_commit`, `package_attempt_terminate`. All profile-scoped and allowlisted until the player calls them.
- Attempt status is `in_progress`, `completed` (the package reported completion; still the attempt a launch resumes, for review) or `closed` (replaced by a fresh attempt; refuses commits).
- SCORM 1.2 rollup, including a package with several lessons (gap 6 of ticket 01): the element is complete when every launchable activity is complete, a lesson by a status of `completed` or `passed` and a shown-only asset by being launched. The score is the mean of the lesson scores reported in the current attempt, each normalised from its own `min`..`max` (0 to 100 when unset). Time spent is the sum of every session of every attempt at the element, to the nearest second. A completed element stays completed through a fresh attempt.
- A session's time is counted once: commits carry the session so far, a commit marked as the content's finish adds it to the total, and a session never finished is added at the next launch or at termination.
- The extracted files are dropped at termination of a completed attempt, never on a commit, because the frame may still be reading them.
- Learner identifier: a BLAKE3 keyed hash of the package hash under a key derived (`derive_key`, fixed context string) from the learner's Ed25519 signing key; 32 hex characters. Stable per package, different across packages and learners, not computable without the key.
- Deviation from the ticket's wording, as decided above: the signed course document carries no per-element skill tag or grading configuration, so document validation has nothing to refuse. The refusal is in the database instead, which every writer goes through (course editor, tagging, grading, hydration): triggers abort a graded submission, a skill tag, or a plugin or plugin configuration on a package element with `package_result_self_reported`. The assessed types are `ASSESSED_ELEMENT_TYPES` in `domain/course_document.rs`.
- A course made only of package elements: `claim_course_completion` refuses with an error naming self-reported package results. `get_course_completion_status` is unchanged (not ready, nothing required), so the player's automatic claim on the last element will report that error until ticket 06 skips the claim for such a course. The rule still needs the owner's confirmation.
- Known gaps: SCORM 2004 and cmi5 run-time data are stored but never read, so those packages never complete until tickets 10 and 25 fill their arms in `progress`. A commit is one whole state, so a commit arriving after a later one would overwrite it; the wrapper must send commits one at a time. `update_progress` rewrites `completed_at` on every update of a completed element, so commits after completion move it.

Proposed doc edits, not made here:

- `GLOSSARY.md`: **Package attempt** as the spec's vocabulary defines it.
- `docs/learning-standards-spec.md` → Attempts and progress: record the SCORM 1.2 several-lesson rule and the asset rule above, and that a course made only of packages issues no credential (once confirmed).
- `docs/architecture.md`: package attempts are device-local; only element progress syncs.
