import type { Pool } from "pg";
import { deliverOnce, type FetchLike, type WebhookEnvelope } from "./delivery";
import { WebhookStore } from "./store";

/** Webhook delivery retry with exponential backoff + DLQ promotion (#961). */
export const WEBHOOK_RETRY_INTERVAL_MS = Number(process.env.WEBHOOK_RETRY_INTERVAL_MS) || 60_000;
export const MAX_RETRY_ATTEMPTS = 7;
export const BASE_DELAY_MS = 60_000;
// 7 attempts: immediately -> 1m -> 5m -> 30m -> 2h -> 8h -> 24h. Realizes the
// exponential backoff delay = BASE_DELAY * 2^attempt, capped per the spec so a
// delivery is never deferred past 24h. schedule[0]=0 is the immediate first try.
export const BACKOFF_SCHEDULE_MS = [0, 60_000, 300_000, 1_800_000, 7_200_000, 28_800_000, 86_400_000];

export function computeBackoffDelayMs(attempt: number): number | null {
  if (attempt < 0 || attempt >= MAX_RETRY_ATTEMPTS) return null;
  return BACKOFF_SCHEDULE_MS[attempt] ?? BACKOFF_SCHEDULE_MS[BACKOFF_SCHEDULE_MS.length - 1];
}

export interface RetryJobHandle {
  stop: () => void;
  runOnce: () => Promise<void>;
}

export function startWebhookRetryDaemon(
  pool: Pool,
  store: WebhookStore,
  options: { intervalMs?: number; fetch?: FetchLike; logger?: Pick<Console, "info" | "error"> } = {},
): RetryJobHandle {
  const intervalMs = options.intervalMs ?? WEBHOOK_RETRY_INTERVAL_MS;
  const doFetch = options.fetch ?? (globalThis.fetch as unknown as FetchLike);
  const logger = options.logger ?? console;

  const runOnce = async () => {
    const { rows } = await pool.query(
      `SELECT id, event_id, raw_data->>'endpoint_id' AS endpoint_id, raw_data, retry_count
         FROM dead_letter_events
        WHERE status IS DISTINCT FROM 'permanently_failed'
          AND (next_retry_at IS NULL OR next_retry_at <= NOW())
        ORDER BY COALESCE(next_retry_at, created_at) ASC LIMIT 100`,
    );
    for (const item of rows as any[]) {
      const endpointId: string | null = item.endpoint_id;
      if (!endpointId) continue; // not a webhook delivery; event-retry path owns it
      try {
        const endpoint = await store.getEndpoint(endpointId);
        if (!endpoint || !endpoint.enabled) throw new Error(`endpoint ${endpointId} unavailable`);
        const { attempt } = await deliverOnce(endpoint, item.raw_data as WebhookEnvelope, item.retry_count + 1, { fetch: doFetch });
        await store.recordAttempt(attempt);
        if (attempt.success) {
          await pool.query(`DELETE FROM dead_letter_events WHERE id = $1`, [item.id]);
          continue;
        }
        throw new Error(attempt.responseBody ?? `HTTP ${attempt.httpStatus ?? "no-response"}`);
      } catch (err) {
        const next = item.retry_count + 1;
        const msg = err instanceof Error ? err.message : String(err);
        const backoff = computeBackoffDelayMs(next);
        if (backoff === null) {
          await pool.query(
            `UPDATE dead_letter_events SET retry_count=$2, status='permanently_failed', error=$3 WHERE id=$1`,
            [item.id, next, msg],
          );
          logger.error(`[webhooks] DLQ ${item.event_id} permanently failed after ${next} attempts`);
        } else {
          await pool.query(
            `UPDATE dead_letter_events SET retry_count=$2, next_retry_at=NOW()+($3||' milliseconds')::interval, error=$4 WHERE id=$1`,
            [item.id, next, String(backoff), msg],
          );
        }
      }
    }
  };

  const timer = setInterval(() => void runOnce().catch((e) => logger.error(`[webhooks] retry sweep failed: ${e instanceof Error ? e.message : String(e)}`)), intervalMs);
  timer.unref?.();
  return { stop: () => clearInterval(timer), runOnce };
}
