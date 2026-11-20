/**
 * Issue #1155: input validation and sanitization middleware.
 *
 * Provides:
 *  - validateBody(schema)  — Zod schema validation for request bodies
 *  - sanitizeStrings()     — trims and strips null bytes from all string fields
 *  - payloadSizeGuard()    — rejects oversized payloads before parsing
 *  - Pre-built schemas for every mutation endpoint
 *
 * All validation failures return HTTP 400 with a structured error body:
 *   { error: "validation_error", details: ZodFlattenedError }
 *
 * Oversized payloads return HTTP 413 before the body is parsed.
 */

import { Request, Response, NextFunction } from "express";
import { z, ZodTypeAny, ZodError } from "zod";

// ---------------------------------------------------------------------------
// Core middleware factory
// ---------------------------------------------------------------------------

/**
 * Returns an Express middleware that validates `req.body` against `schema`.
 * On success `req.body` is replaced with the parsed (coerced) value.
 * On failure returns 400 with flattened Zod errors.
 */
export function validateBody<T extends ZodTypeAny>(schema: T) {
  return (req: Request, res: Response, next: NextFunction): void => {
    const result = schema.safeParse(req.body);
    if (!result.success) {
      res.status(400).json({
        error: "validation_error",
        details: (result.error as ZodError).flatten(),
      });
      return;
    }
    req.body = result.data;
    next();
  };
}

// ---------------------------------------------------------------------------
// String sanitization middleware
// ---------------------------------------------------------------------------

/**
 * Recursively trims whitespace and strips null bytes (\x00) from every
 * string value in `req.body`. Operates in-place after `express.json()` has
 * already parsed the body.
 */
export function sanitizeStrings() {
  return (req: Request, _res: Response, next: NextFunction): void => {
    if (req.body && typeof req.body === "object") {
      req.body = deepSanitize(req.body);
    }
    next();
  };
}

function deepSanitize(value: unknown): unknown {
  if (typeof value === "string") {
    return value.replace(/\x00/g, "").trim();
  }
  if (Array.isArray(value)) {
    return value.map(deepSanitize);
  }
  if (value !== null && typeof value === "object") {
    const result: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      result[k] = deepSanitize(v);
    }
    return result;
  }
  return value;
}

// ---------------------------------------------------------------------------
// Payload size guard
// ---------------------------------------------------------------------------

const DEFAULT_MAX_BODY_BYTES = 64 * 1024; // 64 KB

/**
 * Rejects requests whose `Content-Length` exceeds `maxBytes` before the body
 * is read. Complements `express.json({ limit })` for defence-in-depth.
 */
export function payloadSizeGuard(maxBytes: number = DEFAULT_MAX_BODY_BYTES) {
  return (req: Request, res: Response, next: NextFunction): void => {
    const contentLength = req.header("content-length");
    if (contentLength !== undefined && Number(contentLength) > maxBytes) {
      res.status(413).json({
        error: "payload_too_large",
        message: `Request body must not exceed ${maxBytes} bytes`,
      });
      return;
    }
    next();
  };
}

// ---------------------------------------------------------------------------
// Reusable field primitives
// ---------------------------------------------------------------------------

/** Stellar public key: 56-character G… string */
const stellarPublicKey = z
  .string()
  .min(1, "account is required")
  .max(60)
  .regex(/^G[A-Z2-7]{55}$/, "must be a valid Stellar public key (G…)");

/** Non-empty string with an upper-bound length */
const shortString = (max = 255) =>
  z.string().min(1).max(max).transform((s) => s.trim());

/** Decimal amount string — positive, up to 16 integer digits + 7 decimal */
const decimalAmount = z
  .string()
  .regex(
    /^\d{1,16}(\.\d{1,7})?$/,
    "amount must be a positive decimal (e.g. 100.0000000)",
  );

/** ISO-8601 datetime */
const isoDateTime = z.string().datetime({ message: "must be an ISO-8601 datetime" });

// ---------------------------------------------------------------------------
// Request body schemas — one per mutation endpoint
// ---------------------------------------------------------------------------

/**
 * POST /auth/token  — exchange a signed SEP-10 challenge for a JWT
 */
export const authTokenSchema = z.object({
  /** Base64/XDR encoded signed challenge transaction */
  transaction: z
    .string()
    .min(1, "transaction is required")
    .max(10_000, "transaction XDR must not exceed 10 000 characters"),
  /** Signing account public key (optional — also accepted as query param) */
  account: stellarPublicKey.optional(),
});

/**
 * POST /v1/payments  — create a new payment charge
 */
export const createPaymentSchema = z.object({
  deposit_address: stellarPublicKey,
  amount: decimalAmount,
  asset: shortString(20).optional(),
  memo: shortString(28).optional(),
  metadata: z.record(z.unknown()).optional(),
});

/**
 * POST /v1/payments/:id/verify  — cross-check a customer deposit
 */
export const verifyPaymentSchema = z.object({
  tx_hash: z
    .string()
    .min(64, "tx_hash must be a 64-character hex string")
    .max(64, "tx_hash must be a 64-character hex string")
    .regex(/^[0-9a-fA-F]{64}$/, "tx_hash must be a hex string"),
});

/**
 * POST /v1/invoices  — create an invoice
 */
export const createInvoiceSchema = z.object({
  amount: decimalAmount.optional(),
  stroops: z
    .union([z.string().regex(/^-?\d+$/), z.number().int()])
    .optional(),
  currency: z
    .string()
    .min(3)
    .max(5)
    .regex(/^[A-Z]{3,5}$/, "currency must be a 3-5 character ISO code"),
  description: shortString(500).optional(),
  due_date: isoDateTime.optional(),
  line_items: z
    .array(
      z.object({
        description: shortString(255),
        amount: decimalAmount,
        quantity: z.number().int().min(1).max(10_000).default(1),
      }),
    )
    .max(100)
    .optional(),
}).refine(
  (data) => data.amount !== undefined || data.stroops !== undefined,
  { message: "either amount or stroops is required", path: ["amount"] },
);

/**
 * POST /v1/invoices/preview  — render a preview PDF without persisting
 */
export const invoicePreviewSchema = createInvoiceSchema;

/**
 * POST /v1/webhooks/endpoints  — register a webhook endpoint
 */
export const createWebhookEndpointSchema = z.object({
  url: z.string().url("must be a valid HTTPS URL").max(2048).refine(
    (u) => u.startsWith("https://"),
    "webhook URL must use HTTPS",
  ),
  events: z
    .array(shortString(64))
    .min(1, "at least one event type is required")
    .max(50),
  description: shortString(255).optional(),
  secret: z
    .string()
    .min(16, "webhook secret must be at least 16 characters")
    .max(256)
    .optional(),
});

/**
 * POST /settlements  — initiate a fiat settlement
 */
export const createSettlementSchema = z.object({
  merchant_id: shortString(128),
  amount_usdc: decimalAmount,
  destination_account: stellarPublicKey.optional(),
  reference: shortString(128).optional(),
});

/**
 * POST /v1/merchants/:id/payout-channels  — configure a payout channel
 */
export const createPayoutChannelSchema = z.object({
  type: z.enum(["bank_transfer", "mobile_money", "stellar"]),
  account_number: z.string().min(1).max(64),
  routing_code: shortString(32).optional(),
  bank_name: shortString(128).optional(),
  currency: z
    .string()
    .min(3)
    .max(5)
    .regex(/^[A-Z]{3,5}$/, "currency must be a 3-5 character ISO code"),
  is_default: z.boolean().optional().default(false),
});

export type AuthTokenBody = z.infer<typeof authTokenSchema>;
export type CreatePaymentBody = z.infer<typeof createPaymentSchema>;
export type VerifyPaymentBody = z.infer<typeof verifyPaymentSchema>;
export type CreateInvoiceBody = z.infer<typeof createInvoiceSchema>;
export type CreateWebhookEndpointBody = z.infer<typeof createWebhookEndpointSchema>;
export type CreateSettlementBody = z.infer<typeof createSettlementSchema>;
export type CreatePayoutChannelBody = z.infer<typeof createPayoutChannelSchema>;
