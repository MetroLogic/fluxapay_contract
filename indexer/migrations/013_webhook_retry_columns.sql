-- Migration 013: Webhook delivery retry columns (Issue #961).
-- next_retry_at schedules exponential backoff; status is promoted to
-- permanently_failed after MAX_RETRY_ATTEMPTS. retry_count exists from 003.
ALTER TABLE dead_letter_events ADD COLUMN IF NOT EXISTS next_retry_at TIMESTAMPTZ;
ALTER TABLE dead_letter_events ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'pending';
CREATE INDEX IF NOT EXISTS idx_dlq_next_retry_at
  ON dead_letter_events (next_retry_at)
  WHERE status IS DISTINCT FROM 'permanently_failed';
