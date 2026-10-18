-- Migration 006: Webhook Dead-Letter Queue with Exponential Backoff Retry
-- Issue #961: Failed webhook deliveries are retried with exponential backoff.
--
-- This table is distinct from `dead_letter_events`, which stores raw Soroban
-- contract events that failed to index. This table tracks *webhook delivery*
-- failures so the retry daemon knows what to pick up and when.

CREATE TABLE IF NOT EXISTS webhook_dlq (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- The endpoint this delivery is bound to. Cascade-delete so removing a
    -- webhook endpoint also clears its queued retries.
    endpoint_id    TEXT NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    merchant_id    TEXT NOT NULL,
    event_type     TEXT NOT NULL,
    payment_id     TEXT,
    -- The full signed envelope that failed. Stored so retries send exactly the
    -- same payload the merchant's handler originally saw — idempotency keys
    -- embedded in the body remain stable across attempts.
    payload        JSONB NOT NULL,
    -- How many delivery attempts have been made (0 = never tried).
    retry_count    INT NOT NULL DEFAULT 0,
    -- When the next retry should be attempted. NULL means "retry immediately".
    next_retry_at  TIMESTAMPTZ,
    -- pending: waiting for the next retry window.
    -- permanently_failed: exhausted all attempts; merchant notified.
    status         TEXT NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending', 'permanently_failed')),
    -- The error from the most recent attempt (HTTP status line or network msg).
    last_error     TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- The retry daemon's primary query: pending items whose window has arrived.
CREATE INDEX IF NOT EXISTS idx_webhook_dlq_due
    ON webhook_dlq(next_retry_at, status)
    WHERE status = 'pending';

-- Merchant dashboard: all DLQ entries for one merchant, newest first.
CREATE INDEX IF NOT EXISTS idx_webhook_dlq_merchant
    ON webhook_dlq(merchant_id, created_at DESC);

-- Link from an endpoint's history view into the DLQ.
CREATE INDEX IF NOT EXISTS idx_webhook_dlq_endpoint
    ON webhook_dlq(endpoint_id, created_at DESC);

COMMENT ON TABLE webhook_dlq IS
    'Dead-letter queue for failed webhook deliveries (Issue #961). '
    'Retry daemon picks up rows where status=pending AND next_retry_at <= NOW().';

COMMENT ON COLUMN webhook_dlq.retry_count IS
    'Number of delivery attempts made so far. Max is 7; after that status=permanently_failed.';

COMMENT ON COLUMN webhook_dlq.next_retry_at IS
    'Earliest time the daemon should retry. NULL means retry is due immediately.';

COMMENT ON COLUMN webhook_dlq.status IS
    'pending: in the retry queue. permanently_failed: exhausted all 7 attempts.';
