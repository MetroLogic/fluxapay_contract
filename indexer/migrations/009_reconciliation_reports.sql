-- Reconciliation report data and rates mirrored from the FXOracle contract.

ALTER TABLE payments ALTER COLUMN amount TYPE NUMERIC(39, 0) USING amount::numeric;
ALTER TABLE refunds ALTER COLUMN amount TYPE NUMERIC(39, 0) USING amount::numeric;
ALTER TABLE disputes ALTER COLUMN amount TYPE NUMERIC(39, 0) USING amount::numeric;

ALTER TABLE payments ADD COLUMN IF NOT EXISTS fiat_amount NUMERIC(38, 10);
ALTER TABLE payments ADD COLUMN IF NOT EXISTS fiat_currency VARCHAR(12);
ALTER TABLE payments ADD COLUMN IF NOT EXISTS fee_amount NUMERIC(39, 0) NOT NULL DEFAULT 0;
ALTER TABLE payments ADD COLUMN IF NOT EXISTS settled_at TIMESTAMP;
ALTER TABLE payments ADD COLUMN IF NOT EXISTS customer_ref VARCHAR(255);
ALTER TABLE payments ADD COLUMN IF NOT EXISTS tags JSONB;

CREATE INDEX IF NOT EXISTS idx_payments_merchant_created_at
  ON payments (merchant_id, created_at);
CREATE INDEX IF NOT EXISTS idx_payments_merchant_settled_at
  ON payments (merchant_id, settled_at);

CREATE TABLE IF NOT EXISTS fx_rate_cache (
  pair VARCHAR(64) PRIMARY KEY,
  rate NUMERIC(39, 0) NOT NULL,
  decimals INTEGER NOT NULL,
  updated_at TIMESTAMP NOT NULL
);

CREATE TABLE IF NOT EXISTS dispute_adjustments (
  dispute_id VARCHAR(255) PRIMARY KEY,
  payment_id VARCHAR(255) NOT NULL,
  refund_id VARCHAR(255),
  amount NUMERIC(39, 0) NOT NULL,
  created_at TIMESTAMP NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dispute_adjustments_payment_id
  ON dispute_adjustments (payment_id);
