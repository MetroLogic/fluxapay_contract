-- Migration 003: Dead-Letter Queue and Dispute Status Updates Schema
-- Issue #617: Adds dead_letter_events table for failed event retention and retry
-- Issue #615: Adds escalated and resolved_at columns to disputes table
-- Issue #618: Adds contract_id column to contract_events for multi-contract tracking

CREATE TABLE IF NOT EXISTS dead_letter_events (
  id SERIAL PRIMARY KEY,
  event_id VARCHAR(255) UNIQUE NOT NULL,
  raw_data JSONB NOT NULL,
  error TEXT NOT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  retry_count INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_dlq_event_id ON dead_letter_events (event_id);
CREATE INDEX IF NOT EXISTS idx_dlq_created_at ON dead_letter_events (created_at);
CREATE INDEX IF NOT EXISTS idx_dlq_retry_count ON dead_letter_events (retry_count);

ALTER TABLE disputes ADD COLUMN IF NOT EXISTS escalated BOOLEAN DEFAULT false;
ALTER TABLE disputes ADD COLUMN IF NOT EXISTS resolved_at TIMESTAMP;

ALTER TABLE contract_events ADD COLUMN IF NOT EXISTS contract_id VARCHAR(255);
CREATE INDEX IF NOT EXISTS idx_contract_id ON contract_events (contract_id);
