import { Request, Response, NextFunction } from 'express';
import { Pool } from 'pg';

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const TTL_HOURS = 24;

export interface IdempotencyRecord {
  key: string;
  status_code: number;
  response_body: unknown;
  created_at: Date;
  expires_at: Date;
}

/**
 * Extracts and validates the Idempotency-Key header.
 * Returns the normalized key, or null when the header is missing/invalid.
 * The header is optional for backwards compatibility.
 */
export function extractIdempotencyKey(req: Request): string | null {
  const raw = req.header('Idempotency-Key');
  if (!raw) return null;
  const trimmed = raw.trim();
  if (!UUID_RE.test(trimmed)) return null;
  return trimmed.toLowerCase();
}

/**
 * Creates Express middleware that enforces idempotency for a given route.
 *
 * - Missing/invalid Idempotency-Key: request proceeds normally.
 * - First request with a valid key: response is captured and persisted with a 24h TTL.
 * - Duplicate request within TTL: cached response is replayed with HTTP 200 and
 *   an `Idempotency-Replayed: true` header, without invoking the handler.
 */
export function idempotencyMiddleware(pool: Pool) {
  return async function idempotency(req: Request, res: Response, next: NextFunction) {
    const key = extractIdempotencyKey(req);
    if (!key) return next();

    try {
      const existing = await pool.query<IdempotencyRecord>(
        `SELECT key, status_code, response_body, created_at, expires_at
           FROM idempotency_keys
          WHERE key = $1 AND expires_at > NOW()
          LIMIT 1`,
        [key],
      );

      if (existing.rows.length > 0) {
        const record = existing.rows[0];
        res.setHeader('Idempotency-Replayed', 'true');
        return res.status(200).json(record.response_body);
      }
    } catch (err) {
      return next(err);
    }

    // Capture the response so it can be persisted for future replays.
    const originalJson = res.json.bind(res);
    res.json = (body: unknown) => {
      const statusCode = res.statusCode;
      // Only persist successful responses; failures should be retryable.
      if (statusCode >= 200 && statusCode < 300) {
        pool
          .query(
            `INSERT INTO idempotency_keys (key, status_code, response_body, created_at, expires_at)
             VALUES ($1, $2, $3, NOW(), NOW() + ($4 || ' hours')::interval)
             ON CONFLICT (key) DO NOTHING`,
            [key, statusCode, JSON.stringify(body), TTL_HOURS],
          )
          .catch((err) => {
            // Persistence failure must not break the response already being sent.
            // eslint-disable-next-line no-console
            console.error('Failed to persist idempotency key', err);
          });
      }
      return originalJson(body);
    };

    return next();
  };
}
