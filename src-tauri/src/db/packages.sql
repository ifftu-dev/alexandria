-- Standards packages (SCORM 1.2, SCORM 2004, cmi5) known to this profile,
-- with the description import parsed from each archive. The archive is the
-- kept copy; the extracted files are a rebuildable cache outside the database.
-- See `crate::packages`.
CREATE TABLE packages (
    hash TEXT PRIMARY KEY CHECK (length(hash) = 64),                                  -- BLAKE3 of the archive's own bytes (hex); a package element's content hash
    standard TEXT NOT NULL CHECK (standard IN ('scorm12', 'scorm2004', 'cmi5')),
    edition TEXT CHECK (edition IS NULL OR edition IN ('2nd', '3rd', '4th')),        -- SCORM 2004 only
    title TEXT NOT NULL,
    description_json TEXT NOT NULL CHECK (json_valid(description_json)),             -- parsed package description, including the activity tree
    archive_bytes INTEGER NOT NULL CHECK (archive_bytes >= 0),
    location TEXT NOT NULL CHECK (location IN ('draft', 'store')),                   -- draft: the profile's packages directory, never served; store: the served content store
    imported_at TEXT NOT NULL,
    last_opened_at TEXT                                                               -- last extraction for a launch; the idle check drops the extracted files after a period without one
);
