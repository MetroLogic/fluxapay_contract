import type { Express, NextFunction, Request, Response } from "express";
import {
  buildEnvelope,
  buildTestPayload,
  deliverOnce,
  type FetchLike,
} from "./delivery";
import {
  DELIVERY_LOG_RETENTION_DAYS,
  MAX_DELIVERY_PAGE_SIZE,
  TEST_DELIVERY_LIMIT_PER_HOUR,
  WebhookStore,
} from "./store";
import { isWebhookEventType, WEBHOOK_EVENT_TYPES } from "./types";
import { getPlatformEd25519PublicKey } from "./signing";

/**
 * Webhook HTTP surface (Issues #775, #808, #810).
 *
 * `GET  /webhooks/public-key`                  — Ed25519 active public key (#775)
 * `POST /v1/webhooks/test`                      — synthetic delivery (#808)
 * `GET  /v1/webhooks/:endpointId/deliveries`    — delivery history  (#810)
 */

export interface WebhookRouteDeps {
  store: WebhookStore;
  /** Injectable so tests do not make real network calls. */
  fetch?: FetchLike;
  /** Resolve the merchant the request is authenticated as. */
  merchantIdFromRequest: (req: Request) => string | null;
}

function error(res: Response, status: number, code: string, message: string) {
  return res.status(status).json({ error: { code, message } });
}

export function registerWebhookRoutes(app: Express, deps: WebhookRouteDeps): void {
  const { store, merchantIdFromRequest } = deps;
  const doFetch = deps.fetch ?? (globalThis.fetch as unknown as FetchLike);

  // ── GET /webhooks/public-key (Issue #775) ───────────────────────────────
  const getPublicKeyHandler = (_req: Request, res: Response) => {
    const publicKey = getPlatformEd25519PublicKey();
    return res.status(200).json({
      algorithm: "ed25519",
      public_key: publicKey,
      publicKey: publicKey,
    });
  };

  app.get("/webhooks/public-key", getPublicKeyHandler);
  app.get("/v1/webhooks/public-key", getPublicKeyHandler);

  /**
   * Resolve an endpoint and confirm the caller owns it.
   *
   * A missing endpoint and an endpoint belonging to another merchant both
   * return 404. Distinguishing them would let any authenticated merchant
   * enumerate which endpoint ids exist.
   */
  async function resolveOwnedEndpoint(req: Request, res: Response, endpointId: string) {
    const merchantId = merchantIdFromRequest(req);
    if (!merchantId) {
      error(res, 401, "Unauthenticated", "A merchant API key is required.");
      return null;
    }

    const endpoint = await store.getEndpoint(endpointId);
    if (!endpoint || endpoint.merchantId !== merchantId) {
      error(res, 404, "EndpointNotFound", `No webhook endpoint ${endpointId}.`);
      return null;
    }

    return endpoint;
  }

  // ── POST /v1/webhooks/test (Issue #808) ──────────────────────────────────
  app.post(
    "/v1/webhooks/test",
    async (req: Request, res: Response, next: NextFunction) => {
      try {
        const { endpoint_id: endpointId, event_type: eventType } = req.body ?? {};

        if (typeof endpointId !== "string" || !endpointId) {
          return error(res, 400, "InvalidRequest", "endpoint_id is required.");
        }
        if (typeof eventType !== "string" || !eventType) {
          return error(res, 400, "InvalidRequest", "event_type is required.");
        }
        if (!isWebhookEventType(eventType)) {
          // Signing a typo and letting the merchant wonder why nothing
          // arrived is the failure this endpoint exists to prevent.
          return error(
            res,
            400,
            "UnsupportedEventType",
            `event_type must be one of: ${WEBHOOK_EVENT_TYPES.join(", ")}`,
          );
        }

        const endpoint = await resolveOwnedEndpoint(req, res, endpointId);
        if (!endpoint) return;

        if (!endpoint.enabled) {
          return error(
            res,
            409,
            "EndpointDisabled",
            "This endpoint is disabled. Re-enable it before sending a test.",
          );
        }

        const recentTests = await store.countRecentTestDeliveries(endpoint.id);
        if (recentTests >= TEST_DELIVERY_LIMIT_PER_HOUR) {
          res.setHeader("Retry-After", "3600");
          return error(
            res,
            429,
            "RateLimited",
            `At most ${TEST_DELIVERY_LIMIT_PER_HOUR} test deliveries per endpoint per hour.`,
          );
        }

        // livemode: false, and the payload carries `test_` identifiers, so a
        // handler that ignores the flag still cannot mistake this for real.
        const envelope = buildEnvelope(eventType, buildTestPayload(eventType), false);
        const { attempt } = await deliverOnce(endpoint, envelope, 1, { fetch: doFetch });
        const deliveryId = await store.recordAttempt(attempt);

        // 200 even when the merchant's endpoint rejected it: the test itself
        // succeeded, and its outcome is the answer the caller asked for.
        return res.status(200).json({
          delivery_id: deliveryId,
          event_type: envelope.type,
          livemode: false,
          delivered: attempt.success,
          http_status: attempt.httpStatus,
          duration_ms: attempt.durationMs,
          response_body: attempt.responseBody,
        });
      } catch (err) {
        return next(err);
      }
    },
  );

  // ── GET /v1/webhooks/:endpointId/deliveries (Issue #810) ─────────────────
  app.get(
    "/v1/webhooks/:endpointId/deliveries",
    async (req: Request, res: Response, next: NextFunction) => {
      try {
        const endpoint = await resolveOwnedEndpoint(req, res, req.params.endpointId);
        if (!endpoint) return;

        const rawLimit = req.query.limit;
        let limit: number | undefined;
        if (rawLimit !== undefined) {
          const parsed = Number(rawLimit);
          if (!Number.isInteger(parsed) || parsed < 1 || parsed > MAX_DELIVERY_PAGE_SIZE) {
            return error(
              res,
              400,
              "InvalidRequest",
              `limit must be an integer between 1 and ${MAX_DELIVERY_PAGE_SIZE}.`,
            );
          }
          limit = parsed;
        }

        const rows = await store.listDeliveries({
          endpointId: endpoint.id,
          paymentId:
            typeof req.query.payment_id === "string" ? req.query.payment_id : undefined,
          limit,
          before: typeof req.query.before === "string" ? req.query.before : undefined,
        });

        return res.status(200).json({
          data: rows,
          // Cursor for the next page, absent on the last one. Keyset rather
          // than offset so new deliveries arriving mid-listing cannot make a
          // row the merchant is hunting for slip past unseen.
          next_before: rows.length > 0 ? rows[rows.length - 1].delivered_at : null,
          retention_days: DELIVERY_LOG_RETENTION_DAYS,
        });
      } catch (err) {
        return next(err);
      }
    },
  );
}
