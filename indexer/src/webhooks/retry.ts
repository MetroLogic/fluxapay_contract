import { randomUUID } from "node:crypto";
import { deliverOnce, type FetchLike } from "./delivery";
import {
  DEFAULT_WEBHOOK_CLAIM_LEASE_MS,
  MAX_WEBHOOK_CLAIM_BATCH,
  WebhookStore,
} from "./store";

/** Webhook delivery retry with exponential backoff + DLQ promotion (#961). */
export const WEBHOOK_RETRY_INTERVAL_MS = Number(process.env.WEBHOOK_RETRY_INTERVAL_MS) || 60_000;

/**
 * Default per-endpoint retry policy. Endpoints that do not configure their own
 * policy use these values, which reproduce the original hard-coded schedule:
 * immediate first try, then 1m, 5m, 30m, 2h, 8h, 24h.
 */
export const DEFAULT_MAX_RETRIES = 6;
export const DEFAULT_INITIAL_DELAY_MS = 60_000;
export const DEFAULT_BACKOFF_MULTIPLIER = 5;
export const DEFAULT_MAX_DELAY_MS = 86_400_000;

/** Hard bounds enforced when validating endpoint-supplied policy values. */
export const RETRY_POLICY_BOUNDS = {
  maxRetries: { min: 0, max: 20 },
  initialDelayMs: { min: 1_000, max: 86_400_000 },
  backoffMultiplier: { min: 1, max: 10 },
  maxDelayMs: { min: 1_000, max: 86_400_000 },
} as const;

export interface WebhookRetryPolicy {
  /** Number of retries after the initial attempt. */
  maxRetries: number;
  /** Delay before the first retry, in milliseconds. */
  initialDelayMs: number;
  /** Multiplier applied to the delay for each subsequent retry. */
  backoffMultiplier: number;
  /** Upper bound on any single retry delay, in milliseconds. */
  maxDelayMs: number;
}

export const DEFAULT_WEBHOOK_RETRY_POLICY: Readonly<WebhookRetryPolicy> = Object.freeze({
  maxRetries: DEFAULT_MAX_RETRIES,
  initialDelayMs: DEFAULT_INITIAL_DELAY_MS,
  backoffMultiplier: DEFAULT_BACKOFF_MULTIPLIER,
  maxDelayMs: DEFAULT_MAX_DELAY_MS,
});

/**
 * Validates a partial retry policy, filling missing fields with defaults.
 * Throws a RangeError describing the first out-of-bounds or non-integer field.
 */
export function resolveWebhookRetryPolicy(
  input: Partial<WebhookRetryPolicy> | null | undefined,
): WebhookRetryPolicy {
  const policy: WebhookRetryPolicy = { ...DEFAULT_WEBHOOK_RETRY_POLICY, ...(input ?? {}) };

  const checkInt = (name: string, value: number, min: number, max: number) => {
    if (!Number.isInteger(value) || value < min || value > max) {
      throw new RangeError(`${name} must be an integer between ${min} and ${max}, got ${value}`);
    }
  };

  checkInt("max_retries", policy.maxRetries, RETRY_POLICY_BOUNDS.maxRetries.min, RETRY_POLICY_BOUNDS.maxRetries.max);
  checkInt("initial_delay_ms", policy.initialDelayMs, RETRY_POLICY_BOUNDS.initialDelayMs.min, RETRY_POLICY_BOUNDS.initialDelayMs.max);
  checkInt("max_delay_ms", policy.maxDelayMs, RETRY_POLICY_BOUNDS.maxDelayMs.min, RETRY_POLICY_BOUNDS.maxDelayMs.max);
  if (
    !Number.isFinite(policy.backoffMultiplier) ||
    policy.backoffMultiplier < RETRY_POLICY_BOUNDS.backoffMultiplier.min ||
    policy.backoffMultiplier > RETRY_POLICY_BOUNDS.backoffMultiplier.max
  ) {
    throw new RangeError(
      `backoff_multiplier must be between ${RETRY_POLICY_BOUNDS.backoffMultiplier.min} and ${RETRY_POLICY_BOUNDS.backoffMultiplier.max}, got ${policy.backoffMultiplier}`,
    );
  }
  if (policy.maxDelayMs < policy.initialDelayMs) {
    throw new RangeError("max_delay_ms must be greater than or equal to initial_delay_ms");
  }
  return policy;
}

/**
 * Delay before retry number `retryIndex` (0-based: 0 is the first retry),
 * computed as initialDelay * multiplier^retryIndex, capped at maxDelay.
 * Returns null when retries are exhausted.
 */
export function computeRetryDelayMs(policy: WebhookRetryPolicy, retryIndex: number): number | null {
  if (!Number.isInteger(retryIndex) || retryIndex < 0 || retryIndex >= policy.maxRetries) return null;
  const raw = policy.initialDelayMs * Math.pow(policy.backoffMultiplier, retryIndex);
  return Math.min(Math.round(raw), policy.maxDelayMs);
}

/**
 * Lease held on a claimed DLQ row (#1258). Must outlast one delivery attempt
 * (10s timeout) plus its bookkeeping so a healthy worker is never overtaken
 * mid-attempt, while a crashed worker's rows are recovered by a later sweep.
 */
export const WEBHOOK_CLAIM_LEASE_MS =
  Number(process.env.WEBHOOK_CLAIM_LEASE_MS) || DEFAULT_WEBHOOK_CLAIM_LEASE_MS;

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
 *
 * Retry count and delays are governed by the owning endpoint's policy; an
 * endpoint with no stored policy falls back to DEFAULT_WEBHOOK_RETRY_POLICY.
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
        const policy = resolveWebhookRetryPolicy(endpoint.retryPolicy);

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
        const endpoint = await store.getEndpoint(job.endpointId).catch(() => null);
        const policy = resolveWebhookRetryPolicy(endpoint?.retryPolicy);
        // retryIndex is the 0-based index of the retry that would run next.
        const backoff = computeRetryDelayMs(policy, job.retryCount);

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
