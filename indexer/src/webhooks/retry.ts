import { randomUUID } from "node:crypto";
import { deliverOnce, type FetchLike } from "./delivery";
import {
  DEFAULT_WEBHOOK_CLAIM_LEASE_MS,
  MAX_WEBHOOK_CLAIM_BATCH,
  WebhookStore,
} from "./store";

/** Webhook delivery retry with exponential backoff + DLQ promotion (#961). */
export const WEBHOOK_RETRY_INTERVAL_MS = Number(process.env.WEBHOOK_RETRY_INTERVAL_MS) || 60_000;
export const MAX_RETRY_ATTEMPTS = 7;
export const BASE_DELAY_MS = 60_000;
// 7 attempts: immediately -> 1m -> 5m -> 30m -> 2h -> 8h -> 24h. Realizes the
// exponential backoff delay = BASE_DELAY * 2^attempt, capped per the spec so a
// delivery is never deferred past 24h. schedule[0]=0 is the immediate first try.
export const BACKOFF_SCHEDULE_MS = [0, 60_000, 300_000, 1_800_000, 7_200_000, 28_800_000, 86_400_000];

/**
 * Lease held on a claimed DLQ row (#1258). Must outlast one delivery attempt
 * (10s timeout) plus its bookkeeping so a healthy worker is never overtaken
 * mid-attempt, while a crashed worker's rows are recovered by a later sweep.
 */
export const WEBHOOK_CLAIM_LEASE_MS =
  Number(process.env.WEBHOOK_CLAIM_LEASE_MS) || DEFAULT_WEBHOOK_CLAIM_LEASE_MS;

export function computeBackoffDelayMs(attempt: number): number | null {
  if (attempt < 0 || attempt >= MAX_RETRY_ATTEMPTS) return null;
  return BACKOFF_SCHEDULE_MS[attempt] ?? BACKOFF_SCHEDULE_MS[BACKOFF_SCHEDULE_MS.length - 1];
}

export interface RetryJobHandle {
  stop: () => void;
  runOnce: () => Promise<void>;
}

export interface WebhookRetryDaemonOptions {
  intervalMs?: number;
  /** Lease duration for each claimed row. */
  leaseMs?: number;
  /** Maximum rows claimed per sweep. */
  batchSize?: number;
  /** Stable identity for this worker; defaults to a random id per daemon. */
  workerId?: string;
  fetch?: FetchLike;
  logger?: Pick<Console, "info" | "error">;
}

/**
 * Background worker that retries failed webhook deliveries from the DLQ.
 *
 * Each sweep claims due rows atomically (#1258): a row is delivered by exactly
 * one worker, and every terminal write is fenced on the claim token so a
 * worker whose lease expired cannot overwrite the work of its successor.
 */
export function startWebhookRetryDaemon(
  store: WebhookStore,
  options: WebhookRetryDaemonOptions = {},
): RetryJobHandle {
  const intervalMs = options.intervalMs ?? WEBHOOK_RETRY_INTERVAL_MS;
  const leaseMs = options.leaseMs ?? WEBHOOK_CLAIM_LEASE_MS;
  const batchSize = options.batchSize ?? MAX_WEBHOOK_CLAIM_BATCH;
  const workerId = options.workerId ?? `webhook-retry-${randomUUID()}`;
  const doFetch = options.fetch ?? (globalThis.fetch as unknown as FetchLike);
  const logger = options.logger ?? console;

  const runOnce = async () => {
    const claimed = await store.claimDueWebhookRetries(workerId, {
      limit: batchSize,
      leaseMs,
    });

    for (const job of claimed) {
      const nextCount = job.retryCount + 1;
      try {
        const endpoint = await store.getEndpoint(job.endpointId);
        if (!endpoint || !endpoint.enabled) {
          throw new Error(`endpoint ${job.endpointId} unavailable`);
        }

        const { attempt } = await deliverOnce(endpoint, job.payload, nextCount, {
          fetch: doFetch,
        });
        await store.recordAttempt(attempt);

        if (attempt.success) {
          const released = await store.markWebhookRetryDelivered(job.id, job.claimToken);
          if (!released) {
            logger.error(
              `[webhooks] DLQ ${job.eventId} was delivered but its claim (${job.claimToken}) was no longer held; leaving it to the reclaiming worker`,
            );
          }
          continue;
        }

        throw new Error(attempt.responseBody ?? `HTTP ${attempt.httpStatus ?? "no-response"}`);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        const backoff = computeBackoffDelayMs(nextCount);

        if (backoff === null) {
          const failed = await store.failWebhookRetryPermanently(job.id, job.claimToken, {
            retryCount: nextCount,
            error: msg,
          });
          if (failed) {
            logger.error(
              `[webhooks] DLQ ${job.eventId} permanently failed after ${nextCount} attempts`,
            );
          } else {
            logger.error(
              `[webhooks] DLQ ${job.eventId} lost its claim before permanent-failure could be recorded`,
            );
          }
          continue;
        }

        const scheduled = await store.scheduleWebhookRetry(job.id, job.claimToken, {
          retryCount: nextCount,
          error: msg,
          delayMs: backoff,
        });
        if (!scheduled) {
          logger.error(
            `[webhooks] DLQ ${job.eventId} lost its claim before its retry could be scheduled`,
          );
        }
      }
    }
  };

  const timer = setInterval(
    () =>
      void runOnce().catch((e) =>
        logger.error(`[webhooks] retry sweep failed: ${e instanceof Error ? e.message : String(e)}`),
      ),
    intervalMs,
  );
  timer.unref?.();
  return { stop: () => clearInterval(timer), runOnce };
}
