/**
 * API Key authentication middleware for the Indexer REST API.
 * Supports request headers `x-api-key` or `Authorization: Bearer <api_key>`.
 * Rejects missing or invalid credentials without logging API keys.
 */

import type { Request, Response, NextFunction } from "express";

export function getExpectedApiKey(): string | undefined {
  return process.env.API_KEY || process.env.INDEXER_API_KEY;
}

export function extractApiKey(req: Request): string | null {
  const headerKey = req.headers["x-api-key"];
  if (typeof headerKey === "string" && headerKey.trim()) {
    return headerKey.trim();
  }

  const authHeader = req.headers.authorization;
  if (authHeader && authHeader.startsWith("Bearer ")) {
    const token = authHeader.slice("Bearer ".length).trim();
    if (token) return token;
  }

  return null;
}

export function requireApiKey(req: Request, res: Response, next: NextFunction): void {
  const expectedKey = getExpectedApiKey();

  // If no API key is set in environment (e.g. unconfigured local dev), permit or reject based on config
  if (!expectedKey) {
    // If process.env explicitly disables auth in dev mode:
    if (process.env.ALLOW_ANONYMOUS_API === "true") {
      next();
      return;
    }
    // Otherwise, if API key is expected by default:
    res.status(401).json({ error: "API key authentication not configured" });
    return;
  }

  const providedKey = extractApiKey(req);
  if (!providedKey) {
    res.status(401).json({ error: "Missing API key" });
    return;
  }

  if (providedKey !== expectedKey) {
    res.status(401).json({ error: "Invalid API key" });
    return;
  }

  next();
}
