-- Package attempts: one learner's runs through a standards package under an
-- enrolment, with the run-time state the content stored. Device-local: none
-- of these tables sync. Element progress, which does sync, is rolled up from
-- them. See `crate::packages::attempts`.
CREATE TABLE package_attempts (
    id TEXT PRIMARY KEY,
    enrollment_id TEXT NOT NULL REFERENCES enrollments(id) ON DELETE CASCADE,
    element_id TEXT NOT NULL REFERENCES course_elements(id) ON DELETE CASCADE,
    package_hash TEXT NOT NULL REFERENCES packages(hash),                           -- the archive the element carried when the attempt began
    attempt_number INTEGER NOT NULL CHECK (attempt_number >= 1),                    -- 1 for the first attempt at the element under this enrolment
    status TEXT NOT NULL CHECK (status IN ('in_progress', 'completed', 'closed')),  -- completed: the package reported completion; closed: replaced by a fresh attempt
    started_at TEXT NOT NULL,
    ended_at TEXT,                                                                  -- when the attempt completed or was closed
    updated_at TEXT NOT NULL,
    UNIQUE (enrollment_id, element_id, attempt_number)
);

-- Per attempt and activity, the run-time data as the content last stored it,
-- and what the host read from it.
CREATE TABLE package_activity_state (
    attempt_id TEXT NOT NULL REFERENCES package_attempts(id) ON DELETE CASCADE,
    activity_id TEXT NOT NULL,                                                      -- activity identifier from the package's activity tree
    runtime_json TEXT NOT NULL CHECK (json_valid(runtime_json)),                    -- the last commit's run-time data, as the content stored it
    complete INTEGER NOT NULL DEFAULT 0 CHECK (complete IN (0, 1)),                 -- whether the activity counts as complete under its standard's rule
    score REAL CHECK (score IS NULL OR (score >= 0 AND score <= 1)),               -- reported score normalised to 0..1, self-reported
    total_seconds REAL NOT NULL DEFAULT 0 CHECK (total_seconds >= 0),               -- sum of the reported times of finished sessions
    session_seconds REAL NOT NULL DEFAULT 0 CHECK (session_seconds >= 0),           -- reported time of the session still open, folded into total_seconds when it ends
    updated_at TEXT NOT NULL,
    PRIMARY KEY (attempt_id, activity_id)
) WITHOUT ROWID;

-- Per attempt, the SCORM 2004 global objective and activity tracking state.
CREATE TABLE package_sequencing_state (
    attempt_id TEXT PRIMARY KEY REFERENCES package_attempts(id) ON DELETE CASCADE,
    state_json TEXT NOT NULL CHECK (json_valid(state_json)),                        -- global objectives and tracking state, as the sequencing engine stores it
    updated_at TEXT NOT NULL
);

-- A package result is a self-report from untrusted script. A package element
-- is never graded, never skill-tagged (a tag would feed its score into
-- consistency evidence) and never carries grading configuration.
CREATE TRIGGER package_element_never_graded BEFORE INSERT ON element_submissions
WHEN (SELECT element_type FROM course_elements WHERE id = NEW.element_id) = 'package'
BEGIN
    SELECT RAISE(ABORT, 'package_result_self_reported: a package element is never graded');
END;

CREATE TRIGGER package_element_never_skill_tagged BEFORE INSERT ON element_skill_tags
WHEN (SELECT element_type FROM course_elements WHERE id = NEW.element_id) = 'package'
BEGIN
    SELECT RAISE(ABORT, 'package_result_self_reported: a package element carries no skill tag');
END;

CREATE TRIGGER package_element_no_grading_insert BEFORE INSERT ON course_elements
WHEN NEW.element_type = 'package'
    AND (NEW.plugin_cid IS NOT NULL OR NEW.plugin_config_cid IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'package_result_self_reported: a package element carries no grading configuration');
END;

CREATE TRIGGER package_element_no_grading_update BEFORE UPDATE OF element_type, plugin_cid, plugin_config_cid ON course_elements
WHEN NEW.element_type = 'package'
    AND (NEW.plugin_cid IS NOT NULL OR NEW.plugin_config_cid IS NOT NULL
         OR EXISTS (SELECT 1 FROM element_skill_tags WHERE element_id = NEW.id))
BEGIN
    SELECT RAISE(ABORT, 'package_result_self_reported: a package element carries no grading configuration');
END;
