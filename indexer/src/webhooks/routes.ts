import { randomBytes } from "node:crypto";
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

/** Documented per-endpoint retry defaults (see docs/webhooks.md). */
export const RETRY_POLICY_DEFAULTS = {
  max_retries: 5,
  initial_delay_ms: 1000,
  backoff_multiplier: 2,
  max_delay_ms: 3_600_000,
} as const;

export const RETRY_POLICY_BOUNDS = {
  max_retries: { min: 0, max: 20 },
  initial_delay_ms: { min: 100, max: 3_600_000 },
  backoff_multiplier: { min: 1, max: 10 },
  max_delay_ms: { min: 100, max: 86_400_000 },
} as const;

export interface RetryPolicyInput {
  max_retries?: number;
  initial_delay_ms?: number;
  backoff_multiplier?: number;
  max_delay_ms?: number;
}

/**
 * Validate a partial retry policy. Returns an error message, or null if valid.
 * Omitted fields are allowed here; defaults are applied at create time.
 */
export function validateRetryPolicy(value: unknown): string | null {
  if (value === undefined) return null;
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return "retry_policy must be an object.";
  }
  const policy = value as Record<string, unknown>;
  for (const key of Object.keys(policy)) {
    if (!(key in RETRY_POLICY_BOUNDS)) return `retry_policy.${key} is not a supported field.`;
  }
  for (const [key, bounds] of Object.entries(RETRY_POLICY_BOUNDS)) {
    const field = policy[key];
    if (field === undefined) continue;
    if (typeof field !== "number" || !Number.isFinite(field)) {
      return `retry_policy.${key} must be a number.`;
    }
    if (key !== "backoff_multiplier" && !Number.isInteger(field)) {
      return `retry_policy.${key} must be an integer.`;
    }
    if (field < bounds.min || field > bounds.max) {
      return `retry_policy.${key} must be between ${bounds.min} and ${bounds.max}.`;
    }
  }
  const merged = { ...RETRY_POLICY_DEFAULTS, ...policy } as Record<string, number>;
  if (merged.max_delay_ms < merged.initial_delay_ms) {
    return "retry_policy.max_delay_ms must be greater than or equal to initial_delay_ms.";
  }
  return null;
}

export function registerWebhookRoutes(app: Express, deps: WebhookRouteDeps): void {
  const { store, merchantIdFromRequest } = deps;
  const doFetch = deps.fetch ?? (globalThis.fetch as unknown as FetchLike);

  function publicEndpoint(endpoint: Awaited<ReturnType<WebhookStore["getEndpoint"]>>) {
    if (!endpoint) return null;
    return {
      id: endpoint.id,
      merchant_id: endpoint.merchantId,
      url: endpoint.url,
      event_types: endpoint.eventTypes,
      enabled: endpoint.enabled,
      signing_algorithm: endpoint.signingAlgorithm ?? "hmac_sha256",
    };
  }

  function validateEndpointUrl(value: unknown): value is string {
    if (typeof value !== "string") return false;
    try {
      const url = new URL(value);
      if (url.protocol !== "https:" && url.protocol !== "http:") return false;
      if (!url.hostname || url.username || url.password) return false;
      return true;
    } catch {
      return false;
    }
  }

  function validateEventTypes(value: unknown): value is string[] {
    return Array.isArray(value)
      && value.length <= 100
      && new Set(value).size === value.length
      && value.every((item) => typeof item === "string" && isWebhookEventType(item));
  }

  function getMerchant(req: Request, res: Response): string | null {
    const merchantId = merchantIdFromRequest(req);
    if (!merchantId) {
      error(res, 401, "Unauthenticated", "A merchant API key is required.");
      return null;
    }
    return merchantId;
  }

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

  // ── Merchant-scoped endpoint management ─────────────────────────────────
  app.post(
    "/v1/webhooks",
    async (req: Request, res: Response, next: NextFunction) => {
      try {
        const merchantId = getMerchant(req, res);
        if (!merchantId) return;

        const { url, event_types: eventTypes, signing_algorithm: signingAlgorithm } = req.body ?? {};
        if (!validateEndpointUrl(url)) {
          return error(res, 400, "InvalidEndpointUrl", "url must be a valid HTTP or HTTPS URL.");
        }
        if (eventTypes !== undefined && !validateEventTypes(eventTypes)) {
          return error(
            res,
            400,
            "InvalidEventTypes",
            `event_types must contain unique supported values: ${WEBHOOK_EVENT_TYPES.join(", ")}`,
          );
        }
        if (signingAlgorithm !== undefined
          && signingAlgorithm !== "hmac_sha256"
          && signingAlgorithm !== "ed25519") {
          return error(res, 400, "InvalidSigningAlgorithm", "signing_algorithm must be hmac_sha256 or ed25519.");
        }

        const signingSecret = typeof req.body?.signing_secret === "string" && req.body.signing_secret
          ? req.body.signing_secret
          : randomBytes(32).toString("base64url");
        if (signingSecret.length < 16) {
          return error(res, 400, "InvalidSigningSecret", "signing_secret must be at least 16 characters.");
        }

        const endpoint = await store.createEndpoint({
          merchantId,
          url,
          eventTypes: eventTypes ?? [],
          signingSecret,
          signingAlgorithm,
        });
        return res.status(201).json({
          ...publicEndpoint(endpoint),
          signing_secret: signingSecret,
        });
      } catch (err) {
        return next(err);
      }
    },
  );

  app.get(
    "/v1/webhooks",
    async (req: Request, res: Response, next: NextFunction) => {
      try {
        const merchantId = getMerchant(req, res);
        if (!merchantId) return;
        const endpoints = await store.listEndpoints(merchantId);
        return res.status(200).json({
          data: endpoints.map((endpoint) => publicEndpoint(endpoint)),
        });
      } catch (err) {
        return next(err);
      }
    },
  );

  app.get(
    "/v1/webhooks/:endpointId",
    async (req: Request, res: Response, next: NextFunction) => {
      try {
        const merchantId = getMerchant(req, res);
        if (!merchantId) return;
        const endpoint = await store.getEndpointForMerchant(req.params.endpointId, merchantId);
        if (!endpoint) {
          return error(res, 404, "EndpointNotFound", `No webhook endpoint ${req.params.endpointId}.`);
        }
        return res.status(200).json(publicEndpoint(endpoint));
      } catch (err) {
        return next(err);
      }
    },
  );

  const updateEndpointHandler = async (req: Request, res: Response, next: NextFunction) => {
    try {
      const merchantId = getMerchant(req, res);
      if (!merchantId) return;

      const body = req.body ?? {};
      if (body.url !== undefined && !validateEndpointUrl(body.url)) {
        return error(res, 400, "InvalidEndpointUrl", "url must be a valid HTTP or HTTPS URL.");
      }
      if (body.event_types !== undefined && !validateEventTypes(body.event_types)) {
        return error(
          res,
          400,
          "InvalidEventTypes",
          `event_types must contain unique supported values: ${WEBHOOK_EVENT_TYPES.join(", ")}`,
        );
      }
      if (body.enabled !== undefined && typeof body.enabled !== "boolean") {
        return error(res, 400, "InvalidRequest", "enabled must be a boolean.");
      }
      if (body.signing_algorithm !== undefined
        && body.signing_algorithm !== "hmac_sha256"
        && body.signing_algorithm !== "ed25519") {
        return error(res, 400, "InvalidSigningAlgorithm", "signing_algorithm must be hmac_sha256 or ed25519.");
      }

      let signingSecret: string | undefined;
      if (body.rotate_signing_secret === true) {
        signingSecret = randomBytes(32).toString("base64url");
      } else if (typeof body.signing_secret === "string" && body.signing_secret) {
        if (body.signing_secret.length < 16) {
          return error(res, 400, "InvalidSigningSecret", "signing_secret must be at least 16 characters.");
        }
        signingSecret = body.signing_secret;
      }

      const endpoint = await store.updateEndpoint(req.params.endpointId, merchantId, {
        url: body.url,
        eventTypes: body.event_types,
        enabled: body.enabled,
        signingAlgorithm: body.signing_algorithm,
        signingSecret,
      });
      if (!endpoint) {
        return error(res, 404, "EndpointNotFound", `No webhook endpoint ${req.params.endpointId}.`);
      }

      const response: Record<string, unknown> = publicEndpoint(endpoint) ?? {};
      // Only a newly generated or caller-supplied replacement is ever returned.
      if (signingSecret) response.signing_secret = signingSecret;
      return res.status(200).json(response);
    } catch (err) {
      return next(err);
    }
  };

  app.patch("/v1/webhooks/:endpointId", updateEndpointHandler);
  app.put("/v1/webhooks/:endpointId", updateEndpointHandler);

  const disableEndpointHandler = async (req: Request, res: Response, next: NextFunction) => {
    try {
      const merchantId = getMerchant(req, res);
      if (!merchantId) return;
      const endpoint = await store.updateEndpoint(req.params.endpointId, merchantId, { enabled: false });
      if (!endpoint) {
        return error(res, 404, "EndpointNotFound", `No webhook endpoint ${req.params.endpointId}.`);
      }
      return res.status(200).json(publicEndpoint(endpoint));
    } catch (err) {
      return next(err);
    }
  };

  app.post("/v1/webhooks/:endpointId/disable", disableEndpointHandler);
  app.patch("/v1/webhooks/:endpointId/disable", disableEndpointHandler);

  app.post(
    "/v1/webhooks/:endpointId/rotate-secret",
    async (req: Request, res: Response, next: NextFunction) => {
      try {
        const merchantId = getMerchant(req, res);
        if (!merchantId) return;
        const rotated = await store.rotateSigningSecret(req.params.endpointId, merchantId);
        if (!rotated) {
          return error(res, 404, "EndpointNotFound", `No webhook endpoint ${req.params.endpointId}.`);
        }
        return res.status(200).json({
          ...publicEndpoint(rotated.endpoint),
          signing_secret: rotated.signingSecret,
        });
      } catch (err) {
        return next(err);
      }
    },
  );
  app.post(
    "/v1/webhooks/:endpointId/rotate",
    async (req: Request, res: Response, next: NextFunction) => {
      try {
        const merchantId = getMerchant(req, res);
        if (!merchantId) return;
        const rotated = await store.rotateSigningSecret(req.params.endpointId, merchantId);
        if (!rotated) {
          return error(res, 404, "EndpointNotFound", `No webhook endpoint ${req.params.endpointId}.`);
        }
        return res.status(200).json({
          ...publicEndpoint(rotated.endpoint),
          signing_secret: rotated.signingSecret,
        });
      } catch (err) {
        return next(err);
      }
    },
  );

  app.delete(
    "/v1/webhooks/:endpointId",
    async (req: Request, res: Response, next: NextFunction) => {
      try {
        const merchantId = getMerchant(req, res);
        if (!merchantId) return;
        const deleted = await store.deleteEndpoint(req.params.endpointId, merchantId);
        if (!deleted) {
          return error(res, 404, "EndpointNotFound", `No webhook endpoint ${req.params.endpointId}.`);
        }
        return res.status(204).send();
      } catch (err) {
        return next(err);
      }
    },
  );

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
