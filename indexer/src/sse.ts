/**
 * Server-Sent Events (SSE) Manager for FluxaPay Indexer (Issue #855).
 *
 * Provides real-time event streaming for merchant dashboard live updates:
 * - Event filtering via comma-separated `event_types`
 * - Keepalive ping every 30 seconds
 * - Disconnect cleanup to prevent memory leaks
 * - Rate limiting to a maximum of 5 concurrent connections per merchant
 */

import type { Response } from "express";

export interface SSEClient {
  id: string;
  merchantId: string;
  res: Response;
  eventTypes?: Set<string>;
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

    for (const client of this.clients.values()) {
      if (client.merchantId === merchantId) {
        if (!client.eventTypes || client.eventTypes.has(eventType)) {
          try {
            client.res.write(message);
          } catch {
            this.removeClient(client.id);
          }
        }
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
