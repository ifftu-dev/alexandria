-- A subject field's stable atlas slot picks its colour and map region, so
-- adding a field never moves the others. Bundled fields carry their slot in
-- the bundled taxonomy, upserted on every profile open.
ALTER TABLE subject_fields ADD COLUMN atlas_slot INTEGER CHECK (atlas_slot >= 0);
CREATE UNIQUE INDEX idx_subject_fields_atlas_slot ON subject_fields(atlas_slot);
