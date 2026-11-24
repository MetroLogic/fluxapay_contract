-- Issue #1067: Invoice lifecycle — add due_date, paid_at, voided_at, linked_payment_id columns.
-- Issue #1066: Fee collection history table for platform-fee ring-buffer reporting.

-- Add lifecycle columns to the invoices table.
ALTER TABLE invoices
  ADD COLUMN IF NOT EXISTS due_date        TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS paid_at         TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS voided_at       TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS linked_payment_id VARCHAR(255);

-- Index on merchant_id + status for the filtered GET /v1/invoices endpoint.
CREATE INDEX IF NOT EXISTS idx_invoice_merchant_id ON invoices (merchant_id);
CREATE INDEX IF NOT EXISTS idx_invoice_status      ON invoices (status);

-- Issue #1066: Platform fee collection history table (ring-buffer, cap enforced on-chain).
CREATE TABLE IF NOT EXISTS fee_collection_history (
  id           SERIAL PRIMARY KEY,
  payment_id   VARCHAR(255) NOT NULL,
  fee_amount   BIGINT       NOT NULL,
  token        VARCHAR(255) NOT NULL,
  collected_at TIMESTAMPTZ  NOT NULL,
  created_at   TIMESTAMPTZ  DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_fee_payment_id    ON fee_collection_history (payment_id);
CREATE INDEX IF NOT EXISTS idx_fee_collected_at  ON fee_collection_history (collected_at);
