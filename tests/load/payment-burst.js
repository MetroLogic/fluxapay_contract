/**
 * Issue #1068: k6 load test — Burst POST /v1/payments ingestion.
 *
 * Scenario: 500 concurrent POST /v1/payments requests fired within 5 seconds.
 * Pass criteria:
 *   - < 1% error rate across all burst requests
 *   - No 5xx responses
 *
 * Run with:
 *   BASE_URL=http://localhost:3001 k6 run tests/load/payment-burst.js
 */

import http from 'k6/http';
import { check } from 'k6';
import { Rate } from 'k6/metrics';

const errorRate = new Rate('burst_error_rate');

export const options = {
  scenarios: {
    burst: {
      /**
       * "shared-iterations" distributes exactly 500 iterations across as many
       * VUs as possible, spawning all 500 within the 5-second window.
       */
      executor: 'shared-iterations',
      vus: 500,
      iterations: 500,
      maxDuration: '30s',
      tags: { scenario: 'burst' },
    },
  },
  thresholds: {
    // Allow at most 1% error rate across the burst.
    burst_error_rate: ['rate<0.01'],
    http_req_failed:  ['rate<0.01'],
  },
};

const BASE_URL = __ENV.BASE_URL || 'http://localhost:3001';

let iterCounter = 0;

export default function () {
  const idx = ++iterCounter;
  const payload = JSON.stringify({
    payment_id:      `burst-pay-${__VU}-${idx}`,
    merchant_id:     `merchant-burst-${__VU % 10}`,
    amount:          1000,
    currency:        'USDC',
    deposit_address: `GADDR${String(__VU).padStart(50, '0')}`,
    duration_secs:   3600,
  });

  const res = http.post(`${BASE_URL}/v1/payments`, payload, {
    headers: { 'Content-Type': 'application/json' },
    tags:    { name: 'POST /v1/payments' },
    timeout: '10s',
  });

  const ok = check(res, {
    'status is 2xx or 4xx': (r) => r.status < 500,
    'no 5xx error':         (r) => r.status !== 500,
  });

  errorRate.add(!ok);
}
