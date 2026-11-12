-- Backend flags (an AI surface reached during an attempt) raised before a
-- session's chain has its first link. Committing one then would make it the
-- chain's first link and block the trust profile selection and a phone
-- pairing, which must come first. They wait here, in order, and are
-- committed right after the selection, or ahead of the first window or the
-- session's end when no profile is chosen.
ALTER TABLE integrity_sessions ADD COLUMN pending_backend_flags TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid(pending_backend_flags));
