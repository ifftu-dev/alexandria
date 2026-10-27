-- Legacy registrations have no recipient and remain unresolved until PUT succeeds.
ALTER TABLE push_registrations ADD COLUMN recipient TEXT;
ALTER TABLE push_registrations ADD COLUMN pending TEXT NOT NULL DEFAULT 'put' CHECK (pending IN ('put', 'delete', 'none'));
ALTER TABLE push_registrations ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE push_registrations ADD COLUMN last_error TEXT;
ALTER TABLE push_registrations ADD COLUMN network_id TEXT;
CREATE TABLE push_operation_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton=1),
    revision INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    error_origin TEXT
);
INSERT INTO push_operation_state(singleton) VALUES (1);
