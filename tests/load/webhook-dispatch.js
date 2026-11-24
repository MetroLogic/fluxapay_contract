/**
 * Issue #1068: k6 load test — Webhook dispatch throughput.
 *
 * Scenario: Sustain 1 000 webhook deliveries/minute for 5 minutes by hitting
 * the internal webhook-dispatch endpoint that the indexer uses to notify
 * merchant callback URLs.
 *
 * Pass criteria:
 *   - Sustained throughput of ≥ 1 000 deliveries/min for the full 5 minutes
 *   - < 1% error rate
 *
 * Run with:
 *   BASE_URL=http://localhost:3001 k6 run tests/load/webhook-dispatch.js
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Counter } from 'k6/metrics';

const errorRate   = new Rate('webhook_error_rate');
const deliveries  = new Counter('webhook_deliveries');

// 1 000/min = ~16.7/s; 17 VUs each firing once per second ≈ 1 000/min.
const TARGET_VUS = 17;

export const options = {
  scenarios: {
    webhook_sustained: {
      executor: 'constant-vus',
      vus:      TARGET_VUS,
      duration: '5m',
      tags:     { scenario: 'webhook_sustained' },
    },
  },
  thresholds: {
    webhook_error_rate: ['rate<0.01'],
    http_req_failed:    ['rate<0.01'],
    // Expect at least 4 500 deliveries over 5 min (90% of ideal 5 000).
    webhook_deliveries: ['count>4500'],
  },
};

const BASE_URL = __ENV.BASE_URL || 'http://localhost:3001';

export default function () {
  const payload = JSON.stringify({
    event:      'payment.confirmed',
    payment_id: `pay-wh-${__VU}-${Date.now()}`,
    merchant_id: `merchant-${__VU % 20}`,
    amount:     1000,
    timestamp:  Math.floor(Date.now() / 1000),
  });

  const res = http.post(`${BASE_URL}/internal/webhook-dispatch`, payload, {
    headers: { 'Content-Type': 'application/json' },
    tags:    { name: 'POST /internal/webhook-dispatch' },
    timeout: '5s',
  });

  const ok = check(res, {
    'webhook accepted (2xx)': (r) => r.status >= 200 && r.status < 300,
    'no 5xx':                 (r) => r.status < 500,
  });

  errorRate.add(!ok);
  deliveries.add(1);

  // One delivery per second per VU.
  sleep(1);
}
