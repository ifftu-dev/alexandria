# Importing cmi5 packages

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/02-package-import.md

Spec: cmi5 → first bullet; Package storage and import.

- Ticket 02's import accepts a cmi5 package: parses the course structure into the activity tree, with blocks, assignable units, launch methods, completion criteria and mastery scores.
- Only assignable units packaged in the archive are supported. A course structure that launches a remote unit is refused with the existing "launch resource remote" code.
- The same refusal codes and extraction limits apply.

Done when: fixture course structures, including the published cmi5 examples, parse to the expected tree, and a remote unit is refused.

## Notes

2026-11-20: built.

- `alexandria_studio::package::import_package` now imports an archive with `cmi5.xml` at the top. Parsing is in `package/cmi5.rs`; tests in `crates/alexandria-studio/tests/cmi5_import.rs`, every archive built in the test. The zip fixture both test files use moved to `tests/support/zip_fixture.rs`.
- Tree: the course is the root (its `id` is the package identifier), each block a heading, each assignable unit a launchable activity whose kind is the new `assignable_unit`. A unit's launch carries the archive path, the URL's own query and fragment, `launchParameters` as launch data and `masteryScore` on cmi5's 0 to 1 scale. New optional `cmi5` fields carry the rest: on each block and unit its objective references, on each unit `moveOn` and `launchMethod` (defaults filled in), `entitlementKey` and `activityType`; on the description the course's objectives.
- Remote units: one unit with a scheme or host (`https:`, `//host`, an application protocol) refuses the whole package as `launch_resource_remote`, although cmi5 allows a package to mix packaged and remote units.
- Deviation, the spec wins: the examples in the cmi5 specification (section 15) launch only remote units, so as published they are refused. They are tested both ways: refused as published, and parsed to the expected tree with their unit URLs rewritten to packaged paths. The CATAPULT LMS test suite's course structures supply the other fixtures, including its import failures (non-absolute identifiers, repeated identifiers, a URL query that uses a launch parameter name, a missing file, no `cmi5.xml` at the top).
- Decision: strict where the launch or stored state depends on it. Identifiers must be absolute IRIs and unique across the course, blocks, units and objectives; `masteryScore` must be a plain decimal from 0 to 1; `moveOn` and `launchMethod` must be values the schema lists; a unit URL whose query already uses `endpoint`, `fetch`, `actor`, `registration` or `activityId` is refused. All of these are `manifest_unparseable`.
- Decision: lenient where nothing is at stake. Elements in another namespace than the course structure's are vendor extensions and ignored; enumerated values match without regard to case; a missing title falls back to the identifier; a block with no members is accepted; objective references are kept as written and not checked against the defined objectives; a URL starting with `/` names a file at the top of the package.
- Not parsed: descriptions, every language of a title after the first, and vendor extensions.
- Not supported: a standalone `cmi5.xml` outside an archive. cmi5 requires its units to be remote, so it could not be accepted anyway; it is refused as `not_an_archive`.
- Known gap: refusal details from the XML parser say "manifest" for a course structure too.

Proposed doc edits, not made here:

- `GLOSSARY.md`: add **Assignable unit** (a cmi5 unit of content the player launches) beside the standards package terms ticket 02 proposed.
