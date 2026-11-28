-- Persist SEP-24 details required to resume anchor status polling after handoff.
ALTER TABLE settlements
  ADD COLUMN IF NOT EXISTS sep24_endpoint TEXT,
  ADD COLUMN IF NOT EXISTS interactive_url TEXT;
