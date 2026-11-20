# Package storage, cache and opening a file from disk

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/02-package-import.md

Spec: Package storage and import (storage, cache and open-from-disk bullets); Distribution; Attempts and progress → packages table.

- The packages table in the baseline schema, with no data migration.
- A command that imports an archive: runs ticket 02's import, stores the archive in the content store by hash and records the parsed description.
- The archive is the kept copy. Extracted files live in a per-profile cache keyed by package hash, can be dropped and are rebuilt from the archive on demand. Extraction runs off the interface thread and reports progress.
- Opening a package file directly creates a draft course on the device with one package element. It is never announced or served.
- The package element type exists in the course document as a new value of the existing element type field; the document format version does not change.

Done when: host-seam tests import a fixture, find it stored and described, drop the cache and get the same files back, and show that a package opened from disk causes no announcement.

## Notes

2026-11-20: decisions taken after reading the code, before work starts.

- "Stored by hash" means the hash of the archive's own bytes. The existing upload command encrypts with a device key and a fresh nonce, so its hash changes per call and no other device can read the result; do not use it. Add an import path that takes a file location, not the bytes over the command bridge, and never holds the whole archive in memory if the store allows it.
- A draft's archive stays outside the store the node serves from: it is kept as a file in a per-profile packages directory. This is what makes "never served" true, since the node answers any request for a hash it holds. A function moves the archive into the served store, with a pin that is never evicted, when the course is published; ticket 07 calls it from the publish flow.
- For a package received with a course, the archive is in the served store and extraction reads from there.
- The extracted cache gets its own per-profile directory. It must not be the video cache directory, which is wiped on every lock and reachable from the main window's asset scope.
- Dropping the cache on completion or after the idle period is wired here as a function plus the idle check; the completion trigger is called from ticket 05.
- A new table is a new numbered migration, and the generated schema document is regenerated with it.

2026-11-20: built.

- `crate::packages` (`src-tauri/src/packages/`) holds the logic; `commands/packages.rs` is the thin command layer. Host-seam tests are in `packages/tests.rs`, every archive assembled in the test.
- Table `packages` is migration 44: hash, standard, edition, title, the parsed description as JSON (activity tree included), archive size, where the archive is (`draft` or `store`), import time and last launch time. Other tickets add migrations in parallel; renumber at merge.
- The hash is BLAKE3 of the archive's own bytes. Import copies the file into the profile first and hashes and imports that copy, so the kept bytes are the ones that were checked.
- Every import lands as a draft: a file in the profile's new `packages/` directory, never in the content store, so the node cannot serve it and nothing announces it. `packages::publish` moves it into the store with a non-evictable `course` pin and deletes the draft file. This holds for the course editor's import as well as for opening a file, because both are drafts until the course is published.
- `packages::register_stored` records a package whose archive is already in the store (one received with a course). Extraction for a stored archive exports a scratch copy, extracts and deletes it.
- Extracted files live in the profile's new `packagecache/` directory, one directory per hash, written to a scratch name and renamed into place. `drop_extracted` is the completion hook for ticket 05; `drop_idle` is the idle check, run at profile start with a 14-day period (`IDLE_DROP_AFTER`; the spec names no period).
- Opening a package file makes a draft course (provenance `package_file`) with one chapter and one element of type `package`; opening the same file again returns that draft.
- Commands: `package_import`, `package_open_file`, `package_get`, `package_extract` (progress over a channel). All take a file location, never bytes. None has a caller yet; all four are in the command allowlist.
- Deviation from the ticket's wording: "stores the archive in the content store by hash" became "keeps it by hash in the profile's packages directory until publish", as decided above, so that a draft is never served.
- Known gaps: an encrypted archive in the store (a private course) is refused as `not_an_archive` at extraction; decrypting is the course access spec's work. Opening a file through the operating system (file associations, Android content addresses) is not wired; the command takes a path a file picker returns. Two simultaneous extractions of one package both run and one result is kept.

Proposed doc edits, not made here:

- `GLOSSARY.md`: add **Package element** with the `package` element type.
- `docs/multi-user-profiles.md` and `docs/architecture.md`: add `packages/` and `packagecache/` to the per-profile layout, and say the package cache survives a lock while the video cache does not.
- `docs/protocol-specification.md`: list `package` among the course document element types; its content hash is the archive hash.
- `src-tauri/src/db/schema.rs` baseline comment on `course_elements.element_type` lists the old types; it cannot change in an applied migration, so the schema document's description stays stale until a later baseline.

### Review (2026-11-21)

- Packages now reach other devices. `packages::get_or_receive` returns the record, or, when the hash is a package element of a published course on this device and the archive is missing, fetches it through the content resolver (local store, then the providers of the hash over iroh, BLAKE3-verified), checks it with `register_stored` and pins it as `course` content with `auto_unpin = 1` (evictable once no active enrolment needs it; a record whose archive was evicted is fetched again). A hash outside any published course is never fetched. `package_get` uses it; `enroll` prefetches a course's packages in the background. Test: `a_course_published_on_one_device_plays_its_package_on_another`. Gaps: an encrypted (private course) blob is still refused as `not_an_archive`; `package_get` waits on the network while the player shows its loading state.
- Known deviation (spec "the first lesson can start once its own files are extracted"): `ensure_extracted` still extracts the whole archive before the launch opens. Launch-file-first extraction needs the scheme handler to wait on files not yet landed; not done in the review.
- Draft archives a cancelled or replaced pick left are swept at profile start (`sweep_unused_drafts`: draft packages no element and no attempt refers to, with their archive and extracted files).
- `rows` and `draft` return `PackageError` (new `NoIdentity` variant); database steps go through `run_step`. `ArchiveLocation` is renamed `PackageArchiveLocation` to match the TypeScript mirror.
