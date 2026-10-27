-- Persist the exact versioned PUT payload so unchanged retries reuse its version.
ALTER TABLE push_registrations ADD COLUMN put_body TEXT;
