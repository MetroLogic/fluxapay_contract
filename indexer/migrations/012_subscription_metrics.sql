-- Issue #1083: retain plan pricing and subscription lifecycle history for metrics.

CREATE TABLE IF NOT EXISTS subscription_plans (
  plan_id VARCHAR(255) PRIMARY KEY,
  merchant_id VARCHAR(255) NOT NULL,
  amount BIGINT NOT NULL,
  interval_secs BIGINT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_subscription_plans_merchant
  ON subscription_plans(merchant_id);

ALTER TABLE subscriptions
  ADD COLUMN IF NOT EXISTS merchant_id VARCHAR(255),
  ADD COLUMN IF NOT EXISTS plan_id VARCHAR(255);

CREATE INDEX IF NOT EXISTS idx_subscriptions_merchant_status
  ON subscriptions(merchant_id, status);

CREATE TABLE IF NOT EXISTS subscription_metric_events (
  event_id VARCHAR(255) PRIMARY KEY,
  subscription_id VARCHAR(255) NOT NULL,
  merchant_id VARCHAR(255) NOT NULL,
  plan_id VARCHAR(255),
  event_type VARCHAR(50) NOT NULL,
  occurred_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_subscription_metric_events_merchant_time
  ON subscription_metric_events(merchant_id, occurred_at);

CREATE INDEX IF NOT EXISTS idx_subscription_metric_events_subscription_time
  ON subscription_metric_events(subscription_id, occurred_at);
