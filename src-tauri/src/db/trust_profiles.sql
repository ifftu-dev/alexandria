-- Sentinel trust profile, chosen by the backend at session start and locked
-- for the session (see `crate::sentinel::trust_profile`). The selection record
-- (attestation result, capability report, benchmark, floor, exclusions) is
-- folded into the commitment chain before any snapshot.
ALTER TABLE integrity_sessions ADD COLUMN trust_profile TEXT
    CHECK (trust_profile IS NULL OR trust_profile IN ('core', 'attested', 'camera', 'full'));
ALTER TABLE integrity_sessions ADD COLUMN profile_selection_json TEXT
    CHECK (profile_selection_json IS NULL OR json_valid(profile_selection_json));
-- What the learner consented to for this session, per capability.
ALTER TABLE integrity_sessions ADD COLUMN consent_camera INTEGER NOT NULL DEFAULT 0
    CHECK (consent_camera IN (0, 1));
ALTER TABLE integrity_sessions ADD COLUMN consent_microphone INTEGER NOT NULL DEFAULT 0
    CHECK (consent_microphone IN (0, 1));
ALTER TABLE integrity_sessions ADD COLUMN consent_companion_phone INTEGER NOT NULL DEFAULT 0
    CHECK (consent_companion_phone IN (0, 1));
-- The policy floor as judged at attempt start or interview-check acceptance.
-- Issuance judges it again and refuses unless the two agree.
ALTER TABLE integrity_sessions ADD COLUMN policy_floor_json TEXT
    CHECK (policy_floor_json IS NULL OR json_valid(policy_floor_json));
