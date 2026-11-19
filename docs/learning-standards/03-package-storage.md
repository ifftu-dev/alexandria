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
