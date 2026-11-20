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
