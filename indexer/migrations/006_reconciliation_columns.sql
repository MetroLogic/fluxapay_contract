-- Migration 006: Reconciliation report columns + settlements table (Issue #840)
-- Extends payments with fiat/settlement metadata used by CSV export.
-- Adds settlements table so settlement records appear alongside payments/refunds.

ALTER TABLE payments ADD COLUMN IF NOT EXISTS fiat_amount TEXT;
ALTER TABLE payments ADD COLUMN IF NOT EXISTS fiat_currency VARCHAR(16);
ALTER TABLE payments ADD COLUMN IF NOT EXISTS settled_at TIMESTAMP;
ALTER TABLE payments ADD COLUMN IF NOT EXISTS customer_ref VARCHAR(255);
ALTER TABLE payments ADD COLUMN IF NOT EXISTS tags TEXT;

CREATE TABLE IF NOT EXISTS settlements (
  id SERIAL PRIMARY KEY,
  payment_id VARCHAR(255) NOT NULL,
  merchant_id VARCHAR(255) NOT NULL,
  amount BIGINT NOT NULL,
  status VARCHAR(50),
  created_at TIMESTAMP,
  settled_at TIMESTAMP,
  fiat_amount TEXT,
  fiat_currency VARCHAR(16),
  customer_ref VARCHAR(255),
  tags TEXT
);

CREATE INDEX IF NOT EXISTS idx_settlements_merchant_id ON settlements (merchant_id);
CREATE INDEX IF NOT EXISTS idx_settlements_created_at ON settlements (created_at);
CREATE INDEX IF NOT EXISTS idx_payments_settled_at ON payments (settled_at);
CREATE INDEX IF NOT EXISTS idx_payments_created_at ON payments (created_at);
