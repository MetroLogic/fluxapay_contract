import { DELIVERY_LOG_RETENTION_DAYS, WebhookStore } from "./store";

/**
 * Delivery log retention sweep (Issue #810).
 *
 * The log is a debugging aid, not a ledger: it grows with every delivery
 * attempt and nothing else bounds it. Without this it becomes the largest
 * table in the database and stays that way.
 */

/** How often the sweep runs. Daily is well inside a 30-day window. */
export const PURGE_INTERVAL_MS = 24 * 60 * 60 * 1000;

export interface RetentionJobHandle {
  stop: () => void;
  /** Run one sweep immediately. Exposed for tests and manual operation. */
  runOnce: () => Promise<number>;
}

export function startDeliveryLogRetentionJob(
  store: WebhookStore,
  options: {
    retentionDays?: number;
    intervalMs?: number;
    logger?: { info: (msg: string) => void; error: (msg: string) => void };
  } = {},
): RetentionJobHandle {
  const retentionDays = options.retentionDays ?? DELIVERY_LOG_RETENTION_DAYS;
  const intervalMs = options.intervalMs ?? PURGE_INTERVAL_MS;
  const logger = options.logger ?? console;

  const runOnce = async (): Promise<number> => {
    try {
      const removed = await store.purgeExpiredDeliveries(retentionDays);
      if (removed > 0) {
        logger.info(
          `[webhooks] purged ${removed} delivery log rows older than ${retentionDays} days`,
        );
      }
      return removed;
    } catch (error) {
      // A failed sweep must not take the indexer down; the next one retries.
      logger.error(
        `[webhooks] delivery log purge failed: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
      return 0;
    }
  };

  const timer = setInterval(() => void runOnce(), intervalMs);
  // Do not hold the process open for a maintenance timer.
  timer.unref?.();

  return {
    stop: () => clearInterval(timer),
    runOnce,
  };
}
