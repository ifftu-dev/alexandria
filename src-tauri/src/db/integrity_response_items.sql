-- Response-process coverage: items whose first visit reached the backend with
-- response timing. Compared with the attempt's served items, it tells a run
-- of response-process signals from one that never happened.
ALTER TABLE integrity_sessions ADD COLUMN response_items INTEGER NOT NULL DEFAULT 0
    CHECK (response_items >= 0);
