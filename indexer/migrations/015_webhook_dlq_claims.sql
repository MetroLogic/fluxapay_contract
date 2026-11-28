-- Migration 015: durable claim/lease for webhook DLQ retries (Issue #1258).
--
-- The webhook retry daemon used to SELECT due rows and then deliver them with
-- no ownership marker, so two indexer instances sweeping at once could both
-- deliver the same event. Each claimed row now carries who owns the attempt
-- (`claimed_by`), a per-claim fencing token (`claim_token`), and a lease
-- (`claim_expires_at`). Only one worker can hold the row at a time, and a row
-- whose worker died is reclaimable as soon as its lease lapses.
--
-- `claimed_at` is kept for operators debugging a stuck delivery; nothing reads
-- it at runtime.

ALTER TABLE dead_letter_events ADD COLUMN IF NOT EXISTS claimed_by TEXT;
ALTER TABLE dead_letter_events ADD COLUMN IF NOT EXISTS claim_token UUID;
ALTER TABLE dead_letter_events ADD COLUMN IF NOT EXISTS claimed_at TIMESTAMPTZ;
ALTER TABLE dead_letter_events ADD COLUMN IF NOT EXISTS claim_expires_at TIMESTAMPTZ;

-- Claim sweeps scan due, unclaimed rows; this partial index keeps the sweep
-- off permanently failed rows, which are never reclaimed.
CREATE INDEX IF NOT EXISTS idx_dlq_claimable
  ON dead_letter_events (next_retry_at)
  WHERE status IS DISTINCT FROM 'permanently_failed';

-- Finding rows whose worker exited is a lease-expiry lookup.
CREATE INDEX IF NOT EXISTS idx_dlq_claim_expires_at
  ON dead_letter_events (claim_expires_at)
  WHERE claim_expires_at IS NOT NULL;

-- down
ALTER TABLE dead_letter_events DROP COLUMN IF EXISTS claim_expires_at;
ALTER TABLE dead_letter_events DROP COLUMN IF EXISTS claimed_at;
ALTER TABLE dead_letter_events DROP COLUMN IF EXISTS claim_token;
ALTER TABLE dead_letter_events DROP COLUMN IF EXISTS claimed_by;
DROP INDEX IF EXISTS idx_dlq_claimable;
DROP INDEX IF EXISTS idx_dlq_claim_expires_at;
