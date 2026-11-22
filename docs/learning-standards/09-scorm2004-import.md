# Importing SCORM 2004 packages

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/02-package-import.md

Spec: Package storage and import; What conformance means here → SCORM 2004 row.

- Ticket 02's import accepts SCORM 2004 manifests from the 2nd, 3rd and 4th Editions and reports the edition.
- The activity tree carries what the sequencing engine needs: sequencing definitions, objectives, rollup and navigation presentation settings, and shared sequencing collections.
- The same refusal codes and extraction limits apply.

Done when: fixture manifests from each edition parse to the expected tree, with sequencing information intact, and malformed sequencing is refused with a stable code.

## Notes

2026-11-20: built.

- `import_package` now imports SCORM 2004 and reports the edition (`2nd`, `3rd`, `4th`). Tests are in `crates/alexandria-studio/tests/scorm2004_import.rs`: one fixture per edition, a stored-JSON round trip, 28 malformed-sequencing cases, and the shared refusals with the edition attached.
- Each `Activity` gains four optional fields: `sequencing` (the `imsss:sequencing` element with its ADL extensions), `completion_threshold`, `hide_lms_ui` and `shared_data`. The types are in the public module `alexandria_studio::package::sequencing`. The root organization carries sequencing too.
- The JSON follows the manifest: one object per element, snake_case keys, and enumerated values spelled as the manifest spells them (`"objectiveMeasureGreaterThan"`). An element the manifest leaves out is absent, and the engine applies the standard's default. An element that is present has every attribute filled in, so a reader never needs to know a default.
- For each lesson, `Launch` carries what the run-time must be given before the lesson starts: `launch_data` from `adlcp:dataFromLMS`, `time_limit_action` from `adlcp:timeLimitAction`, `max_time_allowed` from the attempt absolute duration limit, and `mastery_score` holding the scaled passing score (the primary objective's minimum normalized measure, set only when that objective is satisfied by measure, on a scale of -1 to 1).
- Decision: sequencing collections are applied at import, element by element: an item's own elements win, and the collection supplies the rest. The description does not keep the collections separately; an activity's `sequencing.collection` records which one it used. This differs from the ticket's wording ("shared sequencing collections" carried as such). Doing the merge here keeps it tested in one place, because the runtime library's own merge was never checked.
- Decision: sequencing is read strictly. A value outside its vocabulary or range, a rule with no condition or no action, an action the rule's kind may not have, a malformed duration, a missing primary objective or objective id, a repeated objective or collection id, or a reference to an objective or collection that does not exist is refused as `manifest_unparseable`. Case in enumerated values is ignored, and booleans accept `true`, `false`, `1`, `0`.
- Decision: a 2nd or 3rd Edition completion threshold, given as the element's text, reads as `completed_by_measure: true` with that number. This means the same thing as the 4th Edition form, so the player needs no edition logic.
- Decision: a manifest that names no edition (`1.3`, or no schema version) imports with `edition` absent. A manifest that names an edition this build does not know (`2004 5th Edition`) is refused as `unsupported_standard`, as the spec's refusal list asks.
- Elements from any edition are accepted in any edition's manifest; one runtime serves all three.
- Not read: `adlseq:objectivesGlobalToSystem` and `adlcp:sharedDataGlobalToSystem` (the runtime library has no slot for them, and the host scopes global objectives to the package by design); `imsss:auxiliaryResources` (SCORM 2004 tells a player to ignore it); the IMS limit conditions SCORM 2004 does not use (only `attemptLimit` and `attemptAbsoluteDurationLimit` are read); organizations other than the default.
- The SCORM 1.2 walk moved from `scorm12.rs` into `scorm.rs` and is now shared through a small `Version` hook. SCORM 1.2 behaviour is unchanged, and its tests pass untouched apart from the old "SCORM 2004 is refused" test, which now covers only the unknown edition.

Proposed doc edits, not made here:

- `GLOSSARY.md`: add **Sequencing collection** if the player or matrix uses the term.
- Ticket 11's conformance matrix: record that collections are merged at import, and that global-to-system objectives and shared data are scoped to the package.
