# Package import as a pure function

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md

Spec: Package storage and import (first four bullets and the shared-crate bullet); ADR 0004.

- In the desktop-free shared crate: archive bytes in, either a parsed package description or a stable refusal code out. No storage, no commands.
- Detects the standard and edition. Parses a SCORM 1.2 manifest into an activity tree and checks that every launchable resource exists in the archive. SCORM 2004 and cmi5 are detected and refused as unsupported until tickets 09 and 24 add them.
- Refusal codes: not an archive, no manifest, manifest unparseable, unsupported standard or edition, launch resource missing, launch resource remote, unsafe entry path, size or count limit exceeded.
- Bounded, path-safe extraction to a caller-supplied directory: absolute paths, parent traversal and links refused; uncompressed size, entry count and compression ratio capped at the spec's starting limits.

Done when: fixture archives built in the tests produce each refusal code and a correct activity tree for single- and multi-lesson SCORM 1.2 packages; a hostile archive cannot write outside the target directory or exceed a limit.

## Notes

2026-11-20: built.

- `alexandria_studio::package`: `import_package(archive, &limits)` returns a `PackageDescription` or a `Refusal`; `extract_package(archive, target_dir, &limits, progress)` writes the files. Tests are in `crates/alexandria-studio/tests/package_import.rs`, every archive assembled in the test.
- Refusal codes, as stored and matched: `not_an_archive`, `no_manifest`, `manifest_unparseable`, `unsupported_standard`, `launch_resource_missing`, `launch_resource_remote`, `unsafe_entry_path`, `limit_exceeded`. A refusal also carries an English detail for whoever has to fix the package, and the standard and edition when import got far enough to recognise them.
- The description holds the standard, the edition (SCORM 2004 only), the manifest identifier, the title, the activity tree, and the file count and decompressed size as measured. The tree's root is the default organization. A launchable activity carries the archive path to open, the query and fragment to append, whether it is a SCO or an asset, and the manifest's launch data, mastery score, time limit and time limit action.
- `Limits::default()` is the spec's starting point: 1 GiB, 20,000 entries, 200:1. A caller raises a limit by passing its own `Limits`.
- Decision: import decompresses every entry once to measure it. Declared sizes are not trusted, and this is what makes "it imported" mean "it will extract under the same limits".
- Decision: the compression ratio is the total decompressed size over the length of the archive, not per entry. One blank image does not sink a package; entries that share compressed data cannot hide behind their own declared sizes.
- Decision: links, device files, absolute paths, drive paths, `..`, control characters, names over 255 bytes, and two entries that would land on one file on a case-blind filesystem are all `unsafe_entry_path`. Every name is checked before the first file is written. Extraction never creates a link and ignores the permission bits an archive carries.
- Decision: lenient where content in the wild is sloppy and nothing is at stake: elements are found by name whatever their namespace address, backslashes in entry names and addresses are separators, a launch address that differs from an archive file only by case resolves to that file, a resource with no `scormtype` is an asset, and a manifest may open with a byte order mark or declare ISO-8859-1. Strict where state depends on it: a missing or repeated item identifier and a mastery score that is not a number from 0 to 100 are refused.
- Decision: a manifest that is well-formed but lacks something the standard requires is `manifest_unparseable` too. A package with no launchable item is `launch_resource_missing`.
- Deviation from the ticket's wording: the archive is passed as any seekable reader, not a byte slice, so a large package does not have to sit in memory. A caller holding bytes wraps them in a cursor.
- Addition: extraction has two failures that are not refusals, a directory that is not empty and a disk error.
- Dependencies: `zip` 9 with Deflate only, and `quick-xml` 0.41. These are the releases that fix the advisories the audit configuration accepts for older copies elsewhere in the tree, and `ZipArchive::extract` is never called, so that configuration is unchanged and `cargo audit` reports nothing new.
- Known gaps: file names are matched as written, so an archive whose names are in a different Unicode normalisation form from its manifest is refused as `launch_resource_missing`; UTF-16 manifests are refused as `manifest_unparseable`; names Windows reserves surface as a disk error on Windows.

Proposed doc edits, not made here:

- `GLOSSARY.md`: add **Standards package** and **Activity tree** from the spec's vocabulary now that code uses them.
- `docs/architecture.md`: when storage and serving land (tickets 03 and 04) and the document gains its section on standards packages, name `alexandria_studio::package` as where parsing and extraction live.
