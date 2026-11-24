-- Issue #766: Guard against null memo in STREAM/WITHDRAWN event handler
-- Add memo column with DEFAULT NULL if not present, and ensure it defaults to NULL without NOT NULL constraint.

ALTER TABLE stream_withdrawals ADD COLUMN IF NOT EXISTS memo VARCHAR(255) DEFAULT NULL;
ALTER TABLE stream_withdrawals ALTER COLUMN memo DROP NOT NULL;
ALTER TABLE stream_withdrawals ALTER COLUMN memo SET DEFAULT NULL;
