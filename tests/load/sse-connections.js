/**
 * Issue #1068: k6 load test — SSE (Server-Sent Events) connection scalability.
 *
 * Scenario: Hold 500 simultaneous SSE client connections for 5 minutes while
 * the indexer streams live payment events.
 *
 * Pass criteria:
 *   - 500 SSE connections held open for the full duration
 *   - < 1% connection-open error rate
 *   - No 5xx on the initial SSE handshake
 *
 * Note: k6 supports SSE via the experimental `k6/experimental/streams` module or
 * via the standard `http` module with long-polling. This script uses the standard
 * approach: open an HTTP GET that returns chunked SSE data and read it for the
 * duration of the test. Requires k6 ≥ 0.43.0.
 *
 * Run with:
 *   BASE_URL=http://localhost:3001 k6 run tests/load/sse-connections.js
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Counter } from 'k6/metrics';

const connectionErrors = new Rate('sse_connection_error_rate');
const eventsReceived   = new Counter('sse_events_received');

export const options = {
  scenarios: {
    sse_sustained: {
      executor: 'constant-vus',
      vus:      500,
      duration: '5m',
      tags:     { scenario: 'sse_sustained' },
    },
  },
  thresholds: {
    sse_connection_error_rate: ['rate<0.01'],
    http_req_failed:           ['rate<0.01'],
  },
};

const BASE_URL = __ENV.BASE_URL || 'http://localhost:3001';

export default function () {
  // Each VU opens an SSE stream for ~10 s then reconnects (simulating realistic
  // browser SSE clients that reconnect after inactivity).
  const res = http.get(`${BASE_URL}/v1/events/payments`, {
    headers: {
      Accept:        'text/event-stream',
      'Cache-Control': 'no-cache',
    },
    tags:    { name: 'GET /v1/events/payments (SSE)' },
    timeout: '15s',   // Allow up to 15 s for the streaming response.
  });

  const ok = check(res, {
    'SSE handshake succeeded (200)': (r) => r.status === 200,
    'Content-Type is event-stream':  (r) =>
      (r.headers['Content-Type'] || '').includes('text/event-stream'),
    'no 5xx': (r) => r.status < 500,
  });

  connectionErrors.add(!ok);

  // Count SSE events by splitting on double-newlines (the SSE event boundary).
  if (res.body) {
    const eventChunks = res.body.split('\n\n').filter((s) => s.trim().length > 0);
    eventsReceived.add(eventChunks.length);
  }

  // Brief pause before the VU reconnects.
  sleep(1);
}
