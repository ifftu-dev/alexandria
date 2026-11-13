-- Follow-up questions an interviewer authored for each criterion, as a JSON
-- array of strings. Suggested during the interview when the criterion is not
-- yet covered or a live signal follows an answer on it.
ALTER TABLE interview_criteria ADD COLUMN follow_ups_json TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid(follow_ups_json) AND json_type(follow_ups_json) = 'array');
