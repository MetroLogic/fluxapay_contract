-- Issue #677: dispute bond lifecycle (DISPUTE/BOND_RETURNED, DISPUTE/BOND_FORFEITED)
-- Tracks bond flow independently of the disputes table since bond events
-- carry a recipient + amount rather than a payment_id/status update.

CREATE TABLE IF NOT EXISTS dispute_bonds (
  id SERIAL PRIMARY KEY,
  dispute_id VARCHAR(255) NOT NULL,
  recipient VARCHAR(255) NOT NULL,
  amount BIGINT NOT NULL,
  status VARCHAR(50) NOT NULL, -- BOND_RETURNED | BOND_FORFEITED
  created_at TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_bond_dispute_id ON dispute_bonds (dispute_id);
CREATE INDEX IF NOT EXISTS idx_bond_status ON dispute_bonds (status);
