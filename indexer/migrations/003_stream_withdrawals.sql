-- Issue #614: stream withdrawals tracking (STREAM/WITHDRAWN)
-- Stores each withdrawal event against a stream for audit log and history

CREATE TABLE IF NOT EXISTS stream_withdrawals (
  id SERIAL PRIMARY KEY,
  stream_id VARCHAR(255) NOT NULL,
  recipient VARCHAR(255),
  amount BIGINT,
  remaining_deposit BIGINT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  INDEX idx_stream_withdrawal_stream_id (stream_id)
);
