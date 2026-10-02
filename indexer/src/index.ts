/**
 * FluxaPay Soroban Event Consumer
 * Subscribes to contract events via stellar-sdk from multiple contracts simultaneously,
 * persists events to PostgreSQL, exposes dead-letter queue retry, and starts the REST API.
 */

import { rpc } from "stellar-sdk";
import { Database } from "./database";
import { ContractEvent, AnyEvent, StreamEvent } from "./types";
import { startServer } from "./server";
import * as dotenv from "dotenv";

dotenv.config();

export interface EventSubscriptionConfig {
  rpcUrl: string;
  contractIds: string[];
  dbConnectionString: string;
  pollInterval: number;
  startLedger: number;
  dlqRetryIntervalMs: number;
  dlqMinAgeSeconds: number;
  apiPort: number;
}

export function loadConfigFromEnv(env: NodeJS.ProcessEnv = process.env): EventSubscriptionConfig {
  const contractIdsSet = new Set<string>();

  // 1. Check individual contract environment variables
  const individualVars = [
    env.PAYMENT_PROCESSOR_CONTRACT_ID,
    env.REFUND_MANAGER_CONTRACT_ID,
    env.MERCHANT_REGISTRY_CONTRACT_ID,
    env.FX_ORACLE_CONTRACT_ID,
    env.PAYMENT_LINK_MANAGER_CONTRACT_ID,
  ];
  for (const val of individualVars) {
    if (val && val.trim()) {
      contractIdsSet.add(val.trim());
    }
  }

  // 2. Check CONTRACT_IDS (comma-separated)
  if (env.CONTRACT_IDS && env.CONTRACT_IDS.trim()) {
    env.CONTRACT_IDS.split(",")
      .map((s) => s.trim())
      .filter(Boolean)
      .forEach((id) => contractIdsSet.add(id));
  }

  // 3. Fallback migration for legacy single FLUXAPAY_CONTRACT_ID
  if (env.FLUXAPAY_CONTRACT_ID && env.FLUXAPAY_CONTRACT_ID.trim()) {
    contractIdsSet.add(env.FLUXAPAY_CONTRACT_ID.trim());
  }

  const contractIds = Array.from(contractIdsSet);

  if (contractIds.length === 0) {
    throw new Error(
      "No contract IDs configured. Please set contract IDs in environment variables " +
      "(PAYMENT_PROCESSOR_CONTRACT_ID, REFUND_MANAGER_CONTRACT_ID, MERCHANT_REGISTRY_CONTRACT_ID, " +
      "FX_ORACLE_CONTRACT_ID, PAYMENT_LINK_MANAGER_CONTRACT_ID, CONTRACT_IDS, or FLUXAPAY_CONTRACT_ID)."
    );
  }

  return {
    rpcUrl: env.SOROBAN_RPC_URL || "http://localhost:8000/soroban/rpc",
    contractIds,
    dbConnectionString:
      env.DATABASE_URL || "postgres://postgres:password@localhost:5432/fluxapay",
    pollInterval: parseInt(env.POLL_INTERVAL_MS || "5000", 10),
    startLedger: parseInt(env.START_LEDGER || "1", 10),
    dlqRetryIntervalMs: parseInt(env.DLQ_RETRY_INTERVAL_MS || "60000", 10),
    dlqMinAgeSeconds: parseInt(env.DLQ_MIN_AGE_SECONDS || "60", 10),
    apiPort: parseInt(env.PORT || env.INDEXER_API_PORT || "3001", 10),
  };
}

export class EventSubscriber {
  private server: rpc.Server;
  private database: Database;
  private config: EventSubscriptionConfig;
  private currentLedger: number;
  private pollTimer: NodeJS.Timeout | null = null;
  private dlqRetryTimer: NodeJS.Timeout | null = null;

  constructor(config: EventSubscriptionConfig, database?: Database) {
    this.config = config;
    const allowHttp = config.rpcUrl.startsWith("http://");
    this.server = new rpc.Server(config.rpcUrl, { allowHttp });
    this.database = database || new Database(config.dbConnectionString);
    this.currentLedger = config.startLedger;
  }

  async initialize(): Promise<void> {
    await this.database.initialize();
    console.log(`Event subscriber initialized with ${this.config.contractIds.length} contract ID(s): ${this.config.contractIds.join(", ")}`);
  }

  async start(): Promise<void> {
    console.log(`Starting event subscription from ledger ${this.currentLedger}`);

    // Main subscription loop
    this.pollTimer = setInterval(async () => {
      try {
        await this.pollEvents();
      } catch (error) {
        console.error("Error polling events:", error);
      }
    }, this.config.pollInterval);

    // Auto-retry DLQ loop
    this.dlqRetryTimer = setInterval(async () => {
      try {
        await this.retryDLQEvents(false);
      } catch (error) {
        console.error("Error retrying DLQ events:", error);
      }
    }, this.config.dlqRetryIntervalMs);

    // Graceful shutdown
    process.on("SIGINT", async () => {
      await this.shutdown();
    });
  }

  async pollEvents(): Promise<void> {
    try {
      const request: Parameters<rpc.Server["getEvents"]>[0] = {
        filters: [
          {
            type: "contract",
            contractIds: this.config.contractIds,
          },
        ],
        startLedger: this.currentLedger,
        limit: 100,
      };

      const response = await this.server.getEvents(request);

      if (!response.events || response.events.length === 0) {
        if (response.latestLedger) {
          this.currentLedger = response.latestLedger;
        }
        return;
      }

      console.log(`Found ${response.events.length} events across configured contracts`);

      await this.processBatch(response.events);

      if (response.latestLedger) {
        this.currentLedger = response.latestLedger + 1;
      }
    } catch (error) {
      console.error("Error in pollEvents:", error);
    }
  }

  /**
   * Process a batch of events with error isolation so one event failure does not
   * roll back or cancel processing for the remainder of the batch.
   */
  async processBatch(events: any[]): Promise<{ processed: number; stored: number; errors: number }> {
    let processed = 0;
    let stored = 0;
    let errors = 0;

    for (const event of events) {
      processed++;
      const eventId = `${event.ledger}-${event.txHash}-${event.id || Date.now()}`;
      try {
        const parsedEvent = this.parseEvent(event);
        if (parsedEvent) {
          const wasStored = await this.database.storeEvent(parsedEvent);
          if (wasStored) {
            stored++;
            console.log(`✓ Stored event ${parsedEvent.id} from contract ${parsedEvent.contractId}`);
          }
        }
      } catch (error: any) {
        errors++;
        console.error(`Error processing event ${eventId}:`, error);
        await this.database.storeDeadLetterEvent(
          eventId,
          event,
          error.message || String(error)
        );
      }
    }

    return { processed, stored, errors };
  }

  parseEvent(event: any): AnyEvent | null {
    try {
      const eventId = `${event.ledger}-${event.txHash}-${event.id}`;
      const timestamp = event.timestamp || Math.floor(Date.now() / 1000);
      const ledger = typeof event.ledger === "number" ? event.ledger : parseInt(event.ledger, 10);
      const txHash = event.txHash || "";
      const contractId = event.contractId || event.contract_id || "";

      const topics = Array.isArray(event.topic) ? event.topic : [];
      if (topics.length < 2) {
        console.warn("Invalid event topics:", topics);
        return null;
      }

      let value: Record<string, unknown> = {};
      if (event.value) {
        try {
          value = this.scValToObject(event.value);
        } catch (e) {
          console.warn("Could not parse event value:", e);
        }
      }

      // Issue #766: Guard against null memo in STREAM/WITHDRAWN event handler
      // When a stream withdrawal is triggered programmatically (no memo provided),
      // destructuring or reading memo must never throw TypeError: Cannot read properties of null.
      if (topics[0] === "STREAM" && topics[1] === "WITHDRAWN") {
        let memo: string | null = null;
        if (event.transaction && event.transaction.memo) {
          memo = event.transaction.memo.value != null
            ? event.transaction.memo.value.toString()
            : (event.transaction.memo.toString?.() ?? null);
        } else if (event.memo) {
          memo = typeof event.memo === "object" && event.memo?.value != null
            ? event.memo.value.toString()
            : (event.memo.toString?.() ?? null);
        } else if (value && typeof value === "object" && (value as any).memo !== undefined) {
          const m = (value as any).memo;
          memo = m && typeof m === "object" && m.value != null
            ? m.value.toString()
            : (m != null ? String(m) : null);
        }
        value.memo = memo;
      }

      const baseEvent: ContractEvent = {
        id: eventId,
        timestamp,
        ledger,
        txHash,
        contractId,
        topic: topics,
        value,
      };

      return baseEvent as AnyEvent;
    } catch (error) {
      console.error("Error parsing event:", error);
      return null;
    }
  }

  scValToObject(scval: any): Record<string, unknown> {
    if (typeof scval === "string" || typeof scval === "number") {
      return { value: scval };
    }
    if (scval && typeof scval === "object") {
      if (scval.constructor === Object) {
        return scval;
      }
    }
    return { raw: scval };
  }

  async retryDLQEvents(forceAll = false): Promise<{ attempted: number; succeeded: number; failed: number }> {
    const records = forceAll
      ? await this.database.getAllDLQEvents(100)
      : await this.database.getEligibleDLQEvents(this.config.dlqMinAgeSeconds, 100);

    let attempted = 0;
    let succeeded = 0;
    let failed = 0;

    for (const record of records) {
      attempted++;
      try {
        const parsedEvent = this.parseEvent(record.raw_data);
        if (!parsedEvent) {
          throw new Error("Unable to parse DLQ raw_data into valid event");
        }
        await this.database.storeEvent(parsedEvent);
        await this.database.removeDeadLetterEvent(record.event_id);
        succeeded++;
        console.log(`✓ Replayed DLQ event: ${record.event_id}`);
      } catch (err: any) {
        failed++;
        await this.database.incrementDLQRetryCount(record.event_id, err.message || String(err));
        console.error(`✗ DLQ replay failed for ${record.event_id}:`, err);
      }
    }

    return { attempted, succeeded, failed };
  }

  /**
   * Issue #858: Replay events from a specified ledger range [fromLedger, toLedger].
   * Fetches contract events via RPC, runs them through the same pipeline as live events,
   * skipping already-persisted events via ON CONFLICT DO NOTHING idempotency.
   */
  async replayLedgerRange(
    fromLedger: number,
    toLedger: number,
    onProgress?: (progress: { processed: number; total: number; stored: number; currentLedger: number }) => void,
  ): Promise<{ processed: number; stored: number; total: number }> {
    const totalLedgers = Math.max(1, toLedger - fromLedger + 1);
    let processed = 0;
    let stored = 0;
    let currentStart = fromLedger;

    while (currentStart <= toLedger) {
      try {
        const request: Parameters<rpc.Server["getEvents"]>[0] = {
          filters: [
            {
              type: "contract",
              contractIds: this.config.contractIds,
            },
          ],
          startLedger: currentStart,
          limit: 100,
        };

        const response = await this.server.getEvents(request);
        if (!response.events || response.events.length === 0) {
          if (response.latestLedger && response.latestLedger < toLedger) {
            currentStart = response.latestLedger + 1;
          } else {
            break;
          }
        } else {
          for (const event of response.events) {
            const ledger = typeof event.ledger === "number" ? event.ledger : parseInt(event.ledger, 10);
            if (ledger > toLedger) break;

            processed++;
            const eventId = `${event.ledger}-${event.txHash}-${event.id || Date.now()}`;
            try {
              const parsedEvent = this.parseEvent(event);
              if (parsedEvent) {
                const wasStored = await this.database.storeEvent(parsedEvent);
                if (wasStored) {
                  stored++;
                }
              }
            } catch (error: any) {
              console.error(`Error processing replay event ${eventId}:`, error);
            }
          }

          const lastLedger = response.events[response.events.length - 1].ledger;
          const lastNum = typeof lastLedger === "number" ? lastLedger : parseInt(lastLedger, 10);
          currentStart = Math.max(currentStart + 1, lastNum + 1);
        }
      } catch (err: any) {
        console.error(`Replay error at ledger ${currentStart}:`, err);
        currentStart++;
      }

      if (onProgress) {
        onProgress({
          processed,
          total: totalLedgers,
          stored,
          currentLedger: Math.min(toLedger, currentStart),
        });
      }
    }

    if (onProgress) {
      onProgress({
        processed,
        total: totalLedgers,
        stored,
        currentLedger: toLedger,
      });
    }

    return { processed, stored, total: totalLedgers };
  }

  async shutdown(): Promise<void> {
    console.log("Shutting down event subscriber...");
    if (this.pollTimer) clearInterval(this.pollTimer);
    if (this.dlqRetryTimer) clearInterval(this.dlqRetryTimer);
    await this.database.close();
  }
}

async function main(): Promise<void> {
  const config = loadConfigFromEnv();
  const subscriber = new EventSubscriber(config);
  await subscriber.initialize();
  await subscriber.start();

  // Start REST API Server alongside subscriber
  const database = (subscriber as any).database;
  await startServer(
    database,
    config.apiPort,
    () => subscriber.retryDLQEvents(true),
    (from, to, onProgress) => subscriber.replayLedgerRange(from, to, onProgress),
  );
}

if (require.main === module) {
  main().catch((error) => {
    console.error("Fatal error:", error);
    process.exit(1);
  });
}
