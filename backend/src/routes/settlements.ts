import { createHmac, timingSafeEqual } from "crypto";
import { Router, type Request, type Response, type NextFunction } from "express";

/**
 * Issue #1152: Backend settlements router module.
 *
 * The backend registers /settlements routes in index.ts but the settlements
 * module was missing. This module exports createSettlementRouter, providing
 * endpoints for merchant fiat settlement (SEP-6/SEP-24 offramp coordination)
 * and on-chain settlement views.
 *
 * Endpoints:
 *   GET  /settlements/:id          – fetch a settlement by id
 *   GET  /settlements              – list settlements for the authenticated merchant
 *   POST /settlements/:id/payout   – initiate a fiat payout for a settlement
 *
 * Merchant authentication is enforced on every endpoint via a SEP-10 JWT
 * bearer token (same scheme used by the invoices, analytics, and export
 * routes). The JWT subject becomes the merchant identity.
 */

export interface SettlementRouterConfig {
  networkPassphrase: string;
  homeDomain: string;
  /** Shared secret used to verify SEP-10 JWTs. When omitted, auth checks
   *  only verify the token is present (useful for tests). */
  jwtSecret?: string;
}

interface SettlementRecord {
  settlement_id: string;
  merchant_id: string;
  amount_usdc: string;
  fiat_currency: string;
  fiat_amount: string;
  status: "Pending" | "Processing" | "Completed" | "Failed";
  payout_channel_id: string | null;
  tx_hash: string | null;
  created_at: string;
  settled_at: string | null;
}

interface PayoutRequest {
  payout_channel_id?: string;
}

/**
 * In-memory store keyed by settlement id. In production this would be the
 * indexer/database; for now it lets the routes return meaningful responses
 * and makes the module unit-testable.
 */
const settlements = new Map<string, SettlementRecord>();

let settlementCounter = 0;

function createSettlement(
  merchantId: string,
  amountUsdc: string,
  fiatCurrency: string,
  fiatAmount: string,
): SettlementRecord {
  settlementCounter += 1;
  const id = `stl_${Date.now()}_${settlementCounter}`;
  const record: SettlementRecord = {
    settlement_id: id,
    merchant_id: merchantId,
    amount_usdc: amountUsdc,
    fiat_currency: fiatCurrency,
    fiat_amount: fiatAmount,
    status: "Pending",
    payout_channel_id: null,
    tx_hash: null,
    created_at: new Date().toISOString(),
    settled_at: null,
  };
  settlements.set(id, record);
  return record;
}

// Seed a few records so list/get endpoints are not empty in dev.
createSettlement("GDEV_MERCHANT_1", "100.00", "NGN", "155000.00");
createSettlement("GDEV_MERCHANT_2", "50.00", "GHS", "750.00");

/**
 * Verify a SEP-10 JWT and return its payload, or null when the token is
 * missing, malformed, expired, or signature-invalid.
 *
 * The JWT format is header.payload.signature, all base64url. The signature is
 * HMAC-SHA256(header.payload, jwtSecret) — exactly as the auth router issues
 * it. When no jwtSecret is configured, only the token's presence is checked.
 */
function verifyJwt(token: string, jwtSecret?: string): { sub: string; merchant_id?: string } | null {
  if (!jwtSecret) {
    // No secret configured — accept any non-empty token (test mode).
    const parts = token.split(".");
    if (parts.length < 2) return null;
    try {
      const payload = JSON.parse(Buffer.from(parts[1], "base64url").toString());
      if (!payload.sub) return null;
      return payload;
    } catch {
      return null;
    }
  }

  const parts = token.split(".");
  if (parts.length !== 3) return null;

  const header = parts[0];
  const payloadB64 = parts[1];
  const signature = parts[2];

  // Reconstruct the signing input.
  const signingInput = `${header}.${payloadB64}`;
  const expectedSignature = createHmac("sha256", jwtSecret)
    .update(signingInput)
    .digest("base64url");

  const sigBuffer = Buffer.from(signature);
  const expectedBuffer = Buffer.from(expectedSignature);
  if (sigBuffer.length !== expectedBuffer.length) return null;
  if (!timingSafeEqual(sigBuffer, expectedBuffer)) return null;

  try {
    const payload = JSON.parse(Buffer.from(payloadB64, "base64url").toString());
    if (!payload.sub) return null;

    // Check expiry (exp is in seconds since epoch).
    if (payload.exp && typeof payload.exp === "number") {
      const now = Math.floor(Date.now() / 1000);
      if (payload.exp < now) return null;
    }

    return payload;
  } catch {
    return null;
  }
}

/**
 * Express middleware: require a valid SEP-10 bearer token. Attaches
 * `req.merchantId` on success; responds 401 on failure.
 */
function requireMerchantAuth(jwtSecret?: string) {
  return (req: Request, res: Response, next: NextFunction): void => {
    const authHeader = req.header("authorization") ?? "";
    const [scheme, token] = authHeader.split(" ");
    if (scheme !== "Bearer" || !token) {
      res.status(401).json({ error: "merchant authentication required" });
      return;
    }

    const payload = verifyJwt(token, jwtSecret);
    if (!payload) {
      res.status(401).json({ error: "invalid or expired token" });
      return;
    }

    // Attach merchant identity to the request for downstream handlers.
    (req as Request & { merchantId?: string }).merchantId =
      payload.merchant_id ?? payload.sub;
    next();
  };
/**
 * Issue #828: SEP-6/SEP-24 anchor off-ramp settlement endpoints.
 *
 * POST /settlements  — initiate a fiat off-ramp settlement for a merchant.
 *
 * This module stubs the settlement flow so the backend compiles and starts
 * without an external anchor dependency. A real implementation would call
 * the configured SEP-6/SEP-24 anchor's `/transactions/withdraw` endpoint.
 */

import { Router, Request, Response } from "express";
import {
  validateBody,
  sanitizeStrings,
  createSettlementSchema,
} from "../middleware/validateBody";

export interface SettlementRouterConfig {
  networkPassphrase?: string;
  homeDomain?: string;
  anchorUrl?: string;
}

export function createSettlementRouter(config: SettlementRouterConfig): Router {
  const router = Router();
  const auth = requireMerchantAuth(config.jwtSecret);

  // ------------------------------------------------------------------ GET /settlements/:id

  router.get("/:id", auth, (req: Request, res: Response) => {
    const merchantId = (req as Request & { merchantId?: string }).merchantId;
    const settlement = settlements.get(req.params.id);

    if (!settlement) {
      res.status(404).json({ error: `settlement ${req.params.id} not found` });
      return;
    }

    // A merchant can only view their own settlements.
    if (settlement.merchant_id !== merchantId) {
      res.status(404).json({ error: `settlement ${req.params.id} not found` });
      return;
    }

    res.status(200).json(settlement);
  });

  // ------------------------------------------------------------------ GET /settlements

  router.get("/", auth, (req: Request, res: Response) => {
    const merchantId = (req as Request & { merchantId?: string }).merchantId;

    // Optional status filter.
    const statusFilter = typeof req.query.status === "string" ? req.query.status : undefined;

    let results = Array.from(settlements.values()).filter(
      (s) => s.merchant_id === merchantId,
    );

    if (statusFilter) {
      results = results.filter((s) => s.status === statusFilter);
    }

    // Sort newest first.
    results.sort((a, b) => b.created_at.localeCompare(a.created_at));

    res.status(200).json({ settlements: results, count: results.length });
  });

  // ------------------------------------------------------------------ POST /settlements/:id/payout

  router.post("/:id/payout", auth, (req: Request, res: Response) => {
    const merchantId = (req as Request & { merchantId?: string }).merchantId;
    const settlement = settlements.get(req.params.id);

    if (!settlement) {
      res.status(404).json({ error: `settlement ${req.params.id} not found` });
      return;
    }

    if (settlement.merchant_id !== merchantId) {
      res.status(404).json({ error: `settlement ${req.params.id} not found` });
      return;
    }

    if (settlement.status === "Completed") {
      res.status(409).json({ error: "settlement already completed" });
      return;
    }

    if (settlement.status === "Processing") {
      res.status(409).json({ error: "payout already in progress" });
      return;
    }

    const body = (req.body ?? {}) as PayoutRequest;
    const payoutChannelId =
      typeof body.payout_channel_id === "string" ? body.payout_channel_id : null;

    // Transition to Processing — the SEP-6/SEP-24 anchor offramp coordinator
    // would pick this up and complete the fiat transfer.
    settlement.status = "Processing";
    settlement.payout_channel_id = payoutChannelId;

    res.status(202).json({
      settlement_id: settlement.settlement_id,
      status: settlement.status,
      payout_channel_id: settlement.payout_channel_id,
      message: "payout initiated",
    });
  });

  /**
   * POST /settlements
   *
   * Initiates an off-ramp settlement. Validates the request body then
   * delegates to the configured anchor (or returns a stub response when no
   * anchor is configured).
   */
  router.post(
    "/",
    sanitizeStrings(),
    validateBody(createSettlementSchema),
    async (req: Request, res: Response) => {
      const { merchant_id, amount_usdc, destination_account, reference } =
        req.body;

      if (config.anchorUrl) {
        try {
          const upstream = await fetch(`${config.anchorUrl}/transactions/withdraw`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              merchant_id,
              amount: amount_usdc,
              asset_code: "USDC",
              dest: destination_account,
              network_passphrase: config.networkPassphrase,
              reference,
            }),
          });
          const body = await upstream.json();
          return res.status(upstream.status).json(body);
        } catch (err) {
          return res.status(503).json({ error: "anchor service unavailable" });
        }
      }

      // Stub response — no anchor configured.
      return res.status(202).json({
        settlement_id: `stub-${Date.now()}`,
        merchant_id,
        amount_usdc,
        status: "pending",
        reference: reference ?? null,
        created_at: new Date().toISOString(),
        message:
          "Settlement queued (stub — configure ANCHOR_URL for a real anchor)",
      });
    },
  );

  return router;
}
