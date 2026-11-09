-- A push failure is stored as a stable code (`last_error`) with its raw text
-- beside it (`last_error_detail`). Free-text errors written before codes
-- existed are dropped rather than guessed at.
ALTER TABLE push_registrations ADD COLUMN last_error_detail TEXT;
ALTER TABLE push_operation_state ADD COLUMN last_error_detail TEXT;
UPDATE push_registrations SET last_error = NULL;
UPDATE push_operation_state SET last_error = NULL, error_origin = NULL;
