import { timingSafeEqual } from "node:crypto";
import type { NextFunction, Request, Response } from "express";
import { extractApiKey } from "./api-key";

declare global {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace Express {
    interface Request {
      merchantId?: string;
    }
  }
}

export function getMerchantApiKeyMappings(
  value = process.env.MERCHANT_API_KEYS
): Record<string, string> {
  if (!value) {
    throw new Error("Merchant API key authentication is not configured");
  }

  let mappings: unknown;
  try {
    mappings = JSON.parse(value);
  } catch {
    throw new Error("MERCHANT_API_KEYS must be a JSON object mapping API keys to merchant IDs");
  }

  if (!mappings || typeof mappings !== "object" || Array.isArray(mappings)) {
    throw new Error("MERCHANT_API_KEYS must be a JSON object mapping API keys to merchant IDs");
  }

  const entries = Object.entries(mappings);
  if (entries.length === 0) {
    throw new Error("MERCHANT_API_KEYS must contain non-empty API keys and merchant IDs");
  }

  const result: Record<string, string> = {};
  for (const [key, merchantId] of entries) {
    if (!key.trim() || key !== key.trim() || typeof merchantId !== "string" ||
        !merchantId.trim() || merchantId !== merchantId.trim()) {
      throw new Error("MERCHANT_API_KEYS must contain non-empty API keys and merchant IDs");
    }
    result[key] = merchantId;
  }
  return result;
}

export function requireMerchantApiKey(req: Request, res: Response, next: NextFunction): void {
  const providedKey = extractApiKey(req);
  if (!providedKey) {
    res.status(401).json({ error: "Missing merchant API key" });
    return;
  }

  let mappings: Record<string, string>;
  try {
    mappings = getMerchantApiKeyMappings();
  } catch (error) {
    console.error("Merchant API key configuration error:", error);
    res.status(500).json({ error: "Merchant API key authentication is not configured" });
    return;
  }

  const provided = Buffer.from(providedKey);
  const merchantId = Object.entries(mappings).find(([configuredKey]) => {
    const configured = Buffer.from(configuredKey);
    return provided.length === configured.length && timingSafeEqual(provided, configured);
  })?.[1];

  if (!merchantId) {
    res.status(401).json({ error: "Invalid merchant API key" });
    return;
  }

  req.merchantId = merchantId;
  next();
}
