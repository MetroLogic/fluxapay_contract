/**
 * API Key authentication & scope enforcement middleware for the Indexer REST API.
 * Supports request headers `x-api-key` or `Authorization: Bearer <api_key>`.
 * Rejects missing, invalid, or insufficiently scoped credentials.
 *
 * Scopes supported (Issue #854):
 * - `read:payments`: GET payment, list payments, refunds, disputes
 * - `write:payments`: payment creation/mutation
 * - `read:analytics`: events and analytics queries
 * - `manage:webhooks`: create, update, delete webhook endpoints
 * - `admin`: full unrestricted access
 *
 * Structured audit logging (Issue #970): API key lifecycle events are recorded
 * to an append-only `audit_log` table via `recordAuditEvent`.
 */

import type { Request, Response, NextFunction } from "express";

export interface ScopedApiKey {
  key: string;
  merchantId?: string;
  scopes: string[];
}

export type AuditEventType =
  | "api_key.created"
  | "api_key.revoked"
  | "api_key.rotated"
  | "api_key.auth_failed";

export interface AuditEvent {
  event_type: AuditEventType;
  merchant_id: string | null;
  key_id: string | null;
  actor_id: string | null;
  ip_address: string | null;
  user_agent: string | null;
  timestamp: string;
}

// In-memory registry of scoped API keys
const apiKeyRegistry: Map<string, ScopedApiKey> = new Map();

// Append-only in-memory audit log (mirrors the `audit_log` table).
const auditLog: AuditEvent[] = [];

export function getExpectedApiKey(): string | undefined {
  return process.env.API_KEY || process.env.INDEXER_API_KEY;
}

/**
 * Append an audit event. Records are immutable once written; there is no
 * update or delete path exposed for audit entries.
 */
export function recordAuditEvent(
  eventType: AuditEventType,
  details: {
    merchantId?: string | null;
    keyId?: string | null;
    actorId?: string | null;
    ipAddress?: string | null;
    userAgent?: string | null;
  } = {},
): AuditEvent {
  const event: AuditEvent = {
    event_type: eventType,
    merchant_id: details.merchantId ?? null,
    key_id: details.keyId ?? null,
    actor_id: details.actorId ?? null,
    ip_address: details.ipAddress ?? null,
    user_agent: details.userAgent ?? null,
    timestamp: new Date().toISOString(),
  };
  auditLog.push(event);
  return event;
}

/**
 * Query audit events, optionally filtered by merchant and/or event type.
 * Returns a paginated slice plus the total matching count.
 */
export function queryAuditEvents(options: {
  merchantId?: string | null;
  eventType?: AuditEventType | null;
  limit?: number;
  offset?: number;
} = {}): { events: AuditEvent[]; total: number } {
  const { merchantId, eventType } = options;
  const limit = Math.max(1, Math.min(options.limit ?? 50, 200));
  const offset = Math.max(0, options.offset ?? 0);

  const filtered = auditLog.filter((event) => {
    if (merchantId && event.merchant_id !== merchantId) return false;
    if (eventType && event.event_type !== eventType) return false;
    return true;
  });

  return { events: filtered.slice(offset, offset + limit), total: filtered.length };
}

export function clearAuditLog(): void {
  auditLog.length = 0;
}

function requestContext(req: Request): {
  ipAddress: string | null;
  userAgent: string | null;
  actorId: string | null;
} {
  const forwarded = req.headers["x-forwarded-for"];
  const ipAddress =
    (typeof forwarded === "string" && forwarded.split(",")[0].trim()) ||
    req.ip ||
    null;
  const userAgent = typeof req.headers["user-agent"] === "string" ? req.headers["user-agent"] : null;
  const actorId =
    (typeof req.headers["x-actor-id"] === "string" && req.headers["x-actor-id"]) || null;
  return { ipAddress, userAgent, actorId };
}

export function registerApiKey(apiKey: ScopedApiKey, req?: Request): void {
  apiKeyRegistry.set(apiKey.key, apiKey);
  const ctx = req ? requestContext(req) : { ipAddress: null, userAgent: null, actorId: null };
  recordAuditEvent("api_key.created", {
    merchantId: apiKey.merchantId ?? null,
    keyId: apiKey.key,
    actorId: ctx.actorId,
    ipAddress: ctx.ipAddress,
    userAgent: ctx.userAgent,
  });
}

export function revokeApiKey(key: string, req?: Request): boolean {
  const existing = apiKeyRegistry.get(key);
  if (!existing) return false;
  apiKeyRegistry.delete(key);
  const ctx = req ? requestContext(req) : { ipAddress: null, userAgent: null, actorId: null };
  recordAuditEvent("api_key.revoked", {
    merchantId: existing.merchantId ?? null,
    keyId: key,
    actorId: ctx.actorId,
    ipAddress: ctx.ipAddress,
    userAgent: ctx.userAgent,
  });
  return true;
}

export function rotateApiKey(oldKey: string, newKey: ScopedApiKey, req?: Request): boolean {
  const existing = apiKeyRegistry.get(oldKey);
  if (!existing) return false;
  apiKeyRegistry.delete(oldKey);
  apiKeyRegistry.set(newKey.key, newKey);
  const ctx = req ? requestContext(req) : { ipAddress: null, userAgent: null, actorId: null };
  recordAuditEvent("api_key.rotated", {
    merchantId: newKey.merchantId ?? existing.merchantId ?? null,
    keyId: newKey.key,
    actorId: ctx.actorId,
    ipAddress: ctx.ipAddress,
    userAgent: ctx.userAgent,
  });
  return true;
}

export function clearRegisteredApiKeys(): void {
  apiKeyRegistry.clear();
}

export function getKeyScopes(key: string): string[] | null {
  // Check explicit registry
  const registered = apiKeyRegistry.get(key);
  if (registered) {
    return registered.scopes;
  }

  // Check env API_KEY (defaults to admin scope for backwards compatibility)
  const envKey = getExpectedApiKey();
  if (envKey && key === envKey) {
    return ["admin"];
  }

  // Check API_KEYS JSON in environment if set
  if (process.env.API_KEYS) {
    try {
      const parsed = JSON.parse(process.env.API_KEYS);
      if (Array.isArray(parsed)) {
        const match = parsed.find((item: any) => item.key === key);
        if (match && Array.isArray(match.scopes)) {
          return match.scopes;
        }
      }
    } catch {
      // Ignore parse errors in environment string
    }
  }

  return null;
}

export function getKeyMerchantId(key: string): string | null {
  const registered = apiKeyRegistry.get(key);
  if (registered?.merchantId) {
    return registered.merchantId;
  }

  if (process.env.API_KEYS) {
    try {
      const parsed = JSON.parse(process.env.API_KEYS);
      if (Array.isArray(parsed)) {
        const match = parsed.find((item: any) => item.key === key);
        if (typeof match?.merchantId === "string") return match.merchantId;
        if (typeof match?.merchant_id === "string") return match.merchant_id;
      }
    } catch {
      // Match getKeyScopes behavior for malformed API_KEYS configuration.
    }
  }

  return null;
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
  if (!expectedKey && apiKeyRegistry.size === 0) {
    if (process.env.ALLOW_ANONYMOUS_API === "true") {
      next();
      return;
    }
    res.status(401).json({ error: "API key authentication not configured" });
    return;
  }

  const providedKey = extractApiKey(req);
  if (!providedKey) {
    res.status(401).json({ error: "Missing API key" });
    return;
  }

  const scopes = getKeyScopes(providedKey);
  if (!scopes) {
    const ctx = requestContext(req);
    recordAuditEvent("api_key.auth_failed", {
      merchantId: getKeyMerchantId(providedKey),
      keyId: providedKey,
      actorId: ctx.actorId,
      ipAddress: ctx.ipAddress,
      userAgent: ctx.userAgent,
    });
    res.status(401).json({ error: "Invalid API key" });
    return;
  }

  (req as any).apiKeyScopes = scopes;
  next();
}

export function getExpectedAdminApiKey(): string | undefined {
  return process.env.ADMIN_API_KEY || process.env.INDEXER_ADMIN_API_KEY || getExpectedApiKey();
}

export function requireAdminApiKey(req: Request, res: Response, next: NextFunction): void {
  const expectedAdminKey = getExpectedAdminApiKey();

  if (!expectedAdminKey) {
    if (process.env.ALLOW_ANONYMOUS_API === "true") {
      next();
      return;
    }
    res.status(401).json({ error: "Admin API key authentication not configured" });
    return;
  }

  const providedKey = req.headers["x-admin-api-key"] || req.headers["x-admin-key"] || extractApiKey(req);
  const keyStr = typeof providedKey === "string" ? providedKey.trim() : null;

  if (!keyStr) {
    res.status(401).json({ error: "Missing admin API key" });
    return;
  }

  if (keyStr !== expectedAdminKey) {
    res.status(401).json({ error: "Invalid admin API key" });
    return;
  }

  next();
}

/**
 * Middleware that verifies the caller's API key contains the required scope.
 * Admin scope grants access to all operations.
 * If insufficient, returns 403 Insufficient Scope with required_scope.
 */
export function requireScope(requiredScope: string) {
  return (req: Request, res: Response, next: NextFunction): void => {
    const expectedKey = getExpectedApiKey();
    if (!expectedKey && apiKeyRegistry.size === 0 && process.env.ALLOW_ANONYMOUS_API === "true") {
      next();
      return;
    }

    const providedKey = extractApiKey(req);
    if (!providedKey) {
      res.status(401).json({ error: "Missing API key" });
      return;
    }

    const scopes = getKeyScopes(providedKey);
    if (!scopes) {
      const ctx = requestContext(req);
      recordAuditEvent("api_key.auth_failed", {
        merchantId: getKeyMerchantId(providedKey),
        keyId: providedKey,
        actorId: ctx.actorId,
        ipAddress: ctx.ipAddress,
        userAgent: ctx.userAgent,
      });
      res.status(401).json({ error: "Invalid API key" });
      return;
    }

    (req as any).apiKeyScopes = scopes;

    if (scopes.includes("admin") || scopes.includes(requiredScope)) {
      next();
      return;
    }

    res.status(403).json({
      error: "Insufficient Scope",
      required_scope: requiredScope,
    });
  };
}
