-- Issue #1035: propagate the payer's muxed sub-account ID (from an M-address
-- payer in `verify_payment`) through to the indexer for reconciliation and
-- `GET /v1/transactions` filtering.

ALTER TABLE payments
  ADD COLUMN IF NOT EXISTS payer_muxed_id BIGINT;

CREATE INDEX IF NOT EXISTS idx_payments_payer_muxed_id
  ON payments(payer_muxed_id);
