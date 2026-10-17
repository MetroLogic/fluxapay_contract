-- Issue #1082: index payment-link ownership, associated payment attempts,
-- and public checkout page views.

CREATE TABLE IF NOT EXISTS payment_links (
  link_id VARCHAR(255) PRIMARY KEY,
  merchant_id VARCHAR(255) NOT NULL,
  created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_payment_links_merchant
  ON payment_links(merchant_id, created_at DESC);

ALTER TABLE payments
  ADD COLUMN IF NOT EXISTS payment_link_id VARCHAR(255);

CREATE INDEX IF NOT EXISTS idx_payments_payment_link_id
  ON payments(payment_link_id);

CREATE TABLE IF NOT EXISTS payment_link_views (
  id SERIAL PRIMARY KEY,
  link_id VARCHAR(255) NOT NULL REFERENCES payment_links(link_id) ON DELETE CASCADE,
  viewed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_payment_link_views_link_time
  ON payment_link_views(link_id, viewed_at DESC);
