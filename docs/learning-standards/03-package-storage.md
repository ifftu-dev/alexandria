# Package storage, cache and opening a file from disk

Triage: ready-for-agent
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
