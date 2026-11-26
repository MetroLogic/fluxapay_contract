ALTER TABLE webhook_endpoints
    ADD COLUMN IF NOT EXISTS signing_algorithm TEXT NOT NULL DEFAULT 'hmac_sha256';

ALTER TABLE webhook_endpoints
    DROP CONSTRAINT IF EXISTS webhook_endpoints_signing_algorithm_check;

ALTER TABLE webhook_endpoints
    ADD CONSTRAINT webhook_endpoints_signing_algorithm_check
    CHECK (signing_algorithm IN ('hmac_sha256', 'ed25519'));
