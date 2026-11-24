/**
 * Server-Sent Events (SSE) Manager for FluxaPay Indexer (Issue #855).
 *
 * Provides real-time event streaming for merchant dashboard live updates:
 * - Event filtering via comma-separated `event_types`
 * - Issue #1039: optional `payment_id` / `dispute_id` filtering, so a client
 *   can subscribe to just one payment's or dispute's lifecycle (e.g. a
 *   dispute detail page) instead of every event for the merchant.
 * - Keepalive ping every 30 seconds
 * - Disconnect cleanup to prevent memory leaks
 * - Rate limiting to a maximum of 5 concurrent connections per merchant
 */

import type { Response } from "express";

export interface SSEClientFilters {
  paymentId?: string;
  disputeId?: string;
}

export interface SSEClient {
  id: string;
  merchantId: string;
  res: Response;
  eventTypes?: Set<string>;
  paymentId?: string;
  disputeId?: string;
  pingTimer: NodeJS.Timeout;
}

export class SSEManager {
  private clients: Map<string, SSEClient> = new Map();
  private merchantConnectionCounts: Map<string, number> = new Map();
  public static readonly MAX_CONNECTIONS_PER_MERCHANT = 5;

  public canConnect(merchantId: string): boolean {
    const current = this.merchantConnectionCounts.get(merchantId) || 0;
    return current < SSEManager.MAX_CONNECTIONS_PER_MERCHANT;
  }

  public registerClient(
    clientId: string,
    merchantId: string,
    res: Response,
    eventTypes?: string[],
    filters?: SSEClientFilters,
  ): SSEClient | null {
    if (!this.canConnect(merchantId)) {
      return null;
    }

    const currentCount = this.merchantConnectionCounts.get(merchantId) || 0;
    this.merchantConnectionCounts.set(merchantId, currentCount + 1);

    // Keepalive ping every 30 seconds
    const pingTimer = setInterval(() => {
      try {
        res.write("event: ping\ndata: {}\n\n");
      } catch {
        this.removeClient(clientId);
      }
    }, 30_000);

    const client: SSEClient = {
      id: clientId,
      merchantId,
      res,
      eventTypes:
        eventTypes && eventTypes.length > 0
          ? new Set(eventTypes.map((t) => t.trim()))
          : undefined,
      paymentId: filters?.paymentId,
      disputeId: filters?.disputeId,
      pingTimer,
    };

    this.clients.set(clientId, client);
    return client;
  }

  public removeClient(clientId: string): void {
    const client = this.clients.get(clientId);
    if (!client) return;

    clearInterval(client.pingTimer);
    this.clients.delete(clientId);

    const count = this.merchantConnectionCounts.get(client.merchantId) || 1;
    if (count <= 1) {
      this.merchantConnectionCounts.delete(client.merchantId);
    } else {
      this.merchantConnectionCounts.set(client.merchantId, count - 1);
    }
  }

  public broadcast(merchantId: string, eventType: string, data: any): void {
    const payload = typeof data === "string" ? data : JSON.stringify(data);
    const message = `event: ${eventType}\ndata: ${payload}\n\n`;
    const record = data && typeof data === "object" ? (data as Record<string, unknown>) : undefined;
    const paymentId = record?.payment_id != null ? String(record.payment_id) : undefined;
    const disputeId = record?.dispute_id != null ? String(record.dispute_id) : undefined;

    for (const client of this.clients.values()) {
      if (client.merchantId !== merchantId) continue;
      if (client.eventTypes && !client.eventTypes.has(eventType)) continue;
      // Issue #1039: a client that asked for one payment/dispute only gets
      // events carrying that same ID — e.g. `?dispute_id=disp_1` on a
      // dispute detail page shouldn't see every other dispute's events too.
      if (client.paymentId && client.paymentId !== paymentId) continue;
      if (client.disputeId && client.disputeId !== disputeId) continue;

      try {
        client.res.write(message);
      } catch {
        this.removeClient(client.id);
      }
    }
  }

  public getActiveCount(merchantId?: string): number {
    if (merchantId) {
      return this.merchantConnectionCounts.get(merchantId) || 0;
    }
    return this.clients.size;
  }

  public clearAll(): void {
    for (const client of this.clients.values()) {
      clearInterval(client.pingTimer);
    }
    this.clients.clear();
    this.merchantConnectionCounts.clear();
  }
}

export const sseManager = new SSEManager();
