-- Webhook endpoint registry and delivery log (Issues #808, #810)
--
-- `docs/webhooks.md` has described merchant webhooks since before any of this
-- existed: there was no endpoint registry, no signing, and no delivery. Both
-- issues assume that machinery, so it lands here with them.

-- ---------------------------------------------------------------------------
-- Endpoint registry
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS webhook_endpoints (
    id              TEXT PRIMARY KEY,
    merchant_id     TEXT NOT NULL,
    url             TEXT NOT NULL,
    -- HMAC-SHA256 signing secret. Never returned by any read endpoint; the
    -- merchant sees it once, at creation.
    signing_secret  TEXT NOT NULL,
    -- Event types this endpoint subscribes to. Empty means all.
    event_types     TEXT[] NOT NULL DEFAULT '{}',
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_webhook_endpoints_merchant
    ON webhook_endpoints(merchant_id);

-- ---------------------------------------------------------------------------
-- Delivery log (Issue #810)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS webhook_delivery_log (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    endpoint_id      TEXT NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    event_type       TEXT NOT NULL,
    payment_id       TEXT,
    attempt_number   INT NOT NULL,
    delivered_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    http_status      INT,
    -- Truncated to 1 KB on write. A merchant's error page can be megabytes of
    -- HTML, and storing it whole would make this table the largest in the
    -- database within a week of one broken endpoint.
    response_body    TEXT,
    duration_ms      INT,
    success          BOOLEAN NOT NULL,
    -- Test deliveries are logged like any other so a merchant can see their
    -- verification attempt, but flagged so they never pollute success rates.
    livemode         BOOLEAN NOT NULL DEFAULT TRUE
);

-- The listing endpoint is always scoped to one endpoint, newest first.
CREATE INDEX IF NOT EXISTS idx_webhook_delivery_endpoint_time
    ON webhook_delivery_log(endpoint_id, delivered_at DESC);

-- Filtering a payment's delivery history is the main debugging query.
CREATE INDEX IF NOT EXISTS idx_webhook_delivery_payment
    ON webhook_delivery_log(payment_id)
    WHERE payment_id IS NOT NULL;

-- Retention sweeps scan by age alone.
CREATE INDEX IF NOT EXISTS idx_webhook_delivery_delivered_at
    ON webhook_delivery_log(delivered_at);

-- ---------------------------------------------------------------------------
-- Test-delivery rate limiting (Issue #808)
-- ---------------------------------------------------------------------------
--
-- Counted from the delivery log rather than a separate counter table: one
-- source of truth, and a counter that drifts from the log is worse than a
-- slightly more expensive query on a path capped at five calls an hour.

COMMENT ON COLUMN webhook_delivery_log.livemode IS
    'FALSE for synthetic deliveries from POST /v1/webhooks/test (Issue #808).';
