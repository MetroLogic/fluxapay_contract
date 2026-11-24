-- FluxaPay Indexer Initial Schema
-- Creates tables for event dedup and storing typed events

-- Dedup table for all contract events
CREATE TABLE IF NOT EXISTS contract_events (
  id SERIAL PRIMARY KEY,
  event_id VARCHAR(255) UNIQUE NOT NULL,
  event_type VARCHAR(50) NOT NULL,
  ledger INTEGER NOT NULL,
  tx_hash VARCHAR(255),
  timestamp TIMESTAMP NOT NULL,
  data JSONB,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_event_id ON contract_events (event_id);
CREATE INDEX IF NOT EXISTS idx_ledger ON contract_events (ledger);
CREATE INDEX IF NOT EXISTS idx_event_type ON contract_events (event_type);

-- Payment events table
CREATE TABLE IF NOT EXISTS payments (
  id SERIAL PRIMARY KEY,
  payment_id VARCHAR(255) UNIQUE NOT NULL,
  merchant_id VARCHAR(255) NOT NULL,
  amount BIGINT NOT NULL,
  currency VARCHAR(50),
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_payments_payment_id ON payments (payment_id);
CREATE INDEX IF NOT EXISTS idx_payments_merchant_id ON payments (merchant_id);
CREATE INDEX IF NOT EXISTS idx_payments_status ON payments (status);

-- Refund events table
CREATE TABLE IF NOT EXISTS refunds (
  id SERIAL PRIMARY KEY,
  refund_id VARCHAR(255) UNIQUE NOT NULL,
  payment_id VARCHAR(255) NOT NULL,
  amount BIGINT NOT NULL,
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_refunds_refund_id ON refunds (refund_id);
CREATE INDEX IF NOT EXISTS idx_refunds_payment_id ON refunds (payment_id);
CREATE INDEX IF NOT EXISTS idx_refunds_status ON refunds (status);

-- Dispute events table
CREATE TABLE IF NOT EXISTS disputes (
  id SERIAL PRIMARY KEY,
  dispute_id VARCHAR(255) UNIQUE NOT NULL,
  payment_id VARCHAR(255) NOT NULL,
  amount BIGINT NOT NULL,
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_disputes_dispute_id ON disputes (dispute_id);
CREATE INDEX IF NOT EXISTS idx_disputes_payment_id ON disputes (payment_id);
CREATE INDEX IF NOT EXISTS idx_disputes_status ON disputes (status);

-- Merchant events table
CREATE TABLE IF NOT EXISTS merchants (
  id SERIAL PRIMARY KEY,
  merchant_id VARCHAR(255) UNIQUE NOT NULL,
  status VARCHAR(50),
  last_update TIMESTAMP,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_merchants_merchant_id ON merchants (merchant_id);
CREATE INDEX IF NOT EXISTS idx_merchants_status ON merchants (status);

-- Stream events table
CREATE TABLE IF NOT EXISTS streams (
  id SERIAL PRIMARY KEY,
  stream_id VARCHAR(255) UNIQUE NOT NULL,
  sender VARCHAR(255),
  receiver VARCHAR(255),
  amount BIGINT,
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_streams_stream_id ON streams (stream_id);
CREATE INDEX IF NOT EXISTS idx_streams_sender ON streams (sender);
CREATE INDEX IF NOT EXISTS idx_streams_receiver ON streams (receiver);
CREATE INDEX IF NOT EXISTS idx_streams_status ON streams (status);

-- Subscription events table
CREATE TABLE IF NOT EXISTS subscriptions (
  id SERIAL PRIMARY KEY,
  subscription_id VARCHAR(255) UNIQUE NOT NULL,
  payer VARCHAR(255),
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_subscriptions_subscription_id ON subscriptions (subscription_id);
CREATE INDEX IF NOT EXISTS idx_subscriptions_payer ON subscriptions (payer);
CREATE INDEX IF NOT EXISTS idx_subscriptions_status ON subscriptions (status);

-- Invoice events table
CREATE TABLE IF NOT EXISTS invoices (
  id SERIAL PRIMARY KEY,
  invoice_id VARCHAR(255) UNIQUE NOT NULL,
  merchant_id VARCHAR(255),
  total_amount BIGINT,
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_invoices_invoice_id ON invoices (invoice_id);
CREATE INDEX IF NOT EXISTS idx_invoices_merchant_id ON invoices (merchant_id);
CREATE INDEX IF NOT EXISTS idx_invoices_status ON invoices (status);

-- Settlements table: tracks the off-chain SEP-6/SEP-24 USDC-to-fiat offramp
-- triggered by PAYMENT/ANCHOR_WITHDRAW events (see settlement.ts).
CREATE TABLE IF NOT EXISTS settlements (
  id SERIAL PRIMARY KEY,
  payment_id VARCHAR(255) UNIQUE NOT NULL,
  merchant_id VARCHAR(255) NOT NULL,
  anchor_domain VARCHAR(255) NOT NULL,
  anchor_txn_id VARCHAR(255),
  amount BIGINT NOT NULL,
  currency VARCHAR(50),
  status VARCHAR(20) NOT NULL DEFAULT 'pending',
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_settlement_payment_id (payment_id),
  INDEX idx_settlement_merchant_id (merchant_id),
  INDEX idx_settlement_status (status)
);

-- Indexer checkpoint table (tracks sync progress)
CREATE TABLE IF NOT EXISTS indexer_checkpoint (
  id SERIAL PRIMARY KEY,
  contract_id VARCHAR(255) UNIQUE NOT NULL,
  last_processed_ledger INTEGER NOT NULL DEFAULT 0,
  last_update TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
