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
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_event_id (event_id),
  INDEX idx_ledger (ledger),
  INDEX idx_event_type (event_type)
);

-- Payment events table
CREATE TABLE IF NOT EXISTS payments (
  id SERIAL PRIMARY KEY,
  payment_id VARCHAR(255) UNIQUE NOT NULL,
  merchant_id VARCHAR(255) NOT NULL,
  amount BIGINT NOT NULL,
  currency VARCHAR(50),
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_payment_id (payment_id),
  INDEX idx_merchant_id (merchant_id),
  INDEX idx_status (status)
);

-- Refund events table
CREATE TABLE IF NOT EXISTS refunds (
  id SERIAL PRIMARY KEY,
  refund_id VARCHAR(255) UNIQUE NOT NULL,
  payment_id VARCHAR(255) NOT NULL,
  amount BIGINT NOT NULL,
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_refund_id (refund_id),
  INDEX idx_payment_id (payment_id),
  INDEX idx_status (status)
);

-- Dispute events table
CREATE TABLE IF NOT EXISTS disputes (
  id SERIAL PRIMARY KEY,
  dispute_id VARCHAR(255) UNIQUE NOT NULL,
  payment_id VARCHAR(255) NOT NULL,
  amount BIGINT NOT NULL,
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_dispute_id (dispute_id),
  INDEX idx_payment_id (payment_id),
  INDEX idx_status (status)
);

-- Merchant events table
CREATE TABLE IF NOT EXISTS merchants (
  id SERIAL PRIMARY KEY,
  merchant_id VARCHAR(255) UNIQUE NOT NULL,
  status VARCHAR(50),
  last_update TIMESTAMP,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_merchant_id (merchant_id),
  INDEX idx_status (status)
);

-- Stream events table
CREATE TABLE IF NOT EXISTS streams (
  id SERIAL PRIMARY KEY,
  stream_id VARCHAR(255) UNIQUE NOT NULL,
  sender VARCHAR(255),
  receiver VARCHAR(255),
  amount BIGINT,
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_stream_id (stream_id),
  INDEX idx_sender (sender),
  INDEX idx_receiver (receiver),
  INDEX idx_status (status)
);

-- Subscription events table
CREATE TABLE IF NOT EXISTS subscriptions (
  id SERIAL PRIMARY KEY,
  subscription_id VARCHAR(255) UNIQUE NOT NULL,
  payer VARCHAR(255),
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_subscription_id (subscription_id),
  INDEX idx_payer (payer),
  INDEX idx_status (status)
);

-- Invoice events table
CREATE TABLE IF NOT EXISTS invoices (
  id SERIAL PRIMARY KEY,
  invoice_id VARCHAR(255) UNIQUE NOT NULL,
  merchant_id VARCHAR(255),
  total_amount BIGINT,
  status VARCHAR(50),
  created_at TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_invoice_id (invoice_id),
  INDEX idx_merchant_id (merchant_id),
  INDEX idx_status (status)
);

-- Indexer checkpoint table (tracks sync progress)
CREATE TABLE IF NOT EXISTS indexer_checkpoint (
  id SERIAL PRIMARY KEY,
  contract_id VARCHAR(255) UNIQUE NOT NULL,
  last_processed_ledger INTEGER NOT NULL DEFAULT 0,
  last_update TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
