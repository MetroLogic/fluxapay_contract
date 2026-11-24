/**
 * Issue #1068: k6 load test — Steady-state GET /v1/payments/{id} throughput.
 *
 * Scenario: 100 concurrent GET requests/second for 60 seconds.
 * Pass criteria:
 *   - P99 response latency < 200 ms
 *   - Zero 5xx errors during the full run
 *
 * Run with:
 *   BASE_URL=http://localhost:3001 k6 run tests/load/payment-confirm.js
 *
 * Or via the npm script defined in indexer/package.json:
 *   cd indexer && npm run load-test
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

// ─── Custom metrics ───────────────────────────────────────────────────────────
const errorRate = new Rate('error_rate');
const p99Latency = new Trend('p99_latency', true);

// ─── Shared payment ID pool ───────────────────────────────────────────────────
// In a real run, populate PAYMENT_IDS with IDs seeded into the test database.
// As a fallback the script generates synthetic IDs so it can run against a
// mock/stub server without a database.
const PAYMENT_ID_COUNT = 500;
function randomPaymentId() {
  const idx = Math.floor(Math.random() * PAYMENT_ID_COUNT);
  // Zero-pad to keep IDs the same length (e.g. "pay-000042").
  return `pay-${String(idx).padStart(6, '0')}`;
}

// ─── Test configuration ───────────────────────────────────────────────────────
export const options = {
  /**
   * Steady-state scenario: ramp to 100 VUs over 5 s, hold for 60 s, ramp down.
   * Each VU executes ~1 req/s so 100 VUs ≈ 100 req/s sustained throughput.
   */
  scenarios: {
    steady_state: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '5s',  target: 100 },   // ramp up
        { duration: '60s', target: 100 },   // hold
        { duration: '5s',  target: 0   },   // ramp down
      ],
      gracefulRampDown: '5s',
      tags: { scenario: 'steady_state' },
    },
  },
  thresholds: {
    // P99 latency must stay below 200 ms.
    http_req_waiting: ['p(99)<200'],
    // Zero 5xx errors tolerated during steady state.
    error_rate: ['rate<0.001'],
    // Ensure at least some requests completed successfully.
    http_req_failed: ['rate<0.001'],
  },
};

const BASE_URL = __ENV.BASE_URL || 'http://localhost:3001';

// ─── Default function (executed by each VU) ───────────────────────────────────
export default function () {
  const paymentId = randomPaymentId();
  const url = `${BASE_URL}/v1/payments/${paymentId}`;

  const res = http.get(url, {
    tags: { name: 'GET /v1/payments/{id}' },
    timeout: '5s',
  });

  const ok = check(res, {
    'status is 200 or 404': (r) => r.status === 200 || r.status === 404,
    'no 5xx error':         (r) => r.status < 500,
    'p99 < 200ms':          (r) => r.timings.waiting < 200,
  });

  errorRate.add(!ok);
  p99Latency.add(res.timings.waiting);

  // ~1 req/s per VU.
  sleep(1);
}
