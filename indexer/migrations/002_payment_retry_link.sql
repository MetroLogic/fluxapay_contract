-- Issue #1037: Link retry payments to their original for reconciliation.
--
-- `retry_of_payment_id` is a soft foreign key into `payments.payment_id`
-- (not a DB-level FK constraint, since payment rows can legitimately arrive
-- out of order from the event stream — the original's row may not exist yet
-- when a retry's PAYMENT/CREATED row lands). The column is populated either
-- immediately, from the `retry_of_payment_id` carried in a payment's own
-- PAYMENT/CREATED event value, or on arrival of the dedicated
-- PAYMENT/RETRY_CREATED event, whichever comes first.

ALTER TABLE payments
  ADD COLUMN IF NOT EXISTS retry_of_payment_id VARCHAR(255);

CREATE INDEX IF NOT EXISTS idx_payments_retry_of_payment_id
  ON payments (retry_of_payment_id);

-- PAYMENT/RETRY_CREATED can arrive before the new payment's own
-- PAYMENT/CREATED event within the same poll batch, in which case the
-- indexer upserts a minimal stub row (payment_id + retry_of_payment_id
-- only) to record the link without waiting. Relax these to nullable so
-- that stub insert doesn't violate NOT NULL; the subsequent PAYMENT/CREATED
-- upsert backfills them.
ALTER TABLE payments ALTER COLUMN merchant_id DROP NOT NULL;
ALTER TABLE payments ALTER COLUMN amount DROP NOT NULL;
