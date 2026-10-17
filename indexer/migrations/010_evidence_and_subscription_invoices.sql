ALTER TABLE disputes
  ADD COLUMN IF NOT EXISTS evidence JSONB NOT NULL DEFAULT '[]'::jsonb,
  ADD COLUMN IF NOT EXISTS disputer VARCHAR(255);

ALTER TABLE invoices
  ADD COLUMN IF NOT EXISTS subscription_id VARCHAR(255),
  ADD COLUMN IF NOT EXISTS cycle_number INTEGER,
  ADD COLUMN IF NOT EXISTS invoice_number VARCHAR(64),
  ADD COLUMN IF NOT EXISTS subscriber VARCHAR(255),
  ADD COLUMN IF NOT EXISTS subscriber_email VARCHAR(320),
  ADD COLUMN IF NOT EXISTS billing_period_start TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS billing_period_end TIMESTAMPTZ;

CREATE UNIQUE INDEX IF NOT EXISTS idx_invoice_subscription_cycle
  ON invoices (subscription_id, cycle_number)
  WHERE subscription_id IS NOT NULL AND cycle_number IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_invoice_merchant_number
  ON invoices (merchant_id, invoice_number)
  WHERE invoice_number IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_invoice_subscription
  ON invoices (subscription_id, created_at);

CREATE TABLE IF NOT EXISTS merchant_invoice_sequences (
  merchant_id VARCHAR(255) PRIMARY KEY,
  next_number INTEGER NOT NULL DEFAULT 1
);