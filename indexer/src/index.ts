/**
 * FluxaPay Soroban Event Consumer
 * Subscribes to contract events via stellar-sdk from multiple contracts simultaneously,
 * persists events to PostgreSQL, exposes dead-letter queue retry, and starts the REST API.
 */

import { rpc } from "stellar-sdk";
import { Database } from "./database";
import { ContractEvent, AnyEvent } from "./types";
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

      for (const event of response.events) {
        const eventId = `${event.ledger}-${event.txHash}-${event.id || Date.now()}`;
        try {
          const parsedEvent = this.parseEvent(event);
          if (parsedEvent) {
            const stored = await this.database.storeEvent(parsedEvent);
            if (stored) {
              console.log(`✓ Stored event ${parsedEvent.id} from contract ${parsedEvent.contractId}`);
            }
          }
        } catch (error: any) {
          console.error(`Error processing event ${eventId}:`, error);
          await this.database.storeDeadLetterEvent(
            eventId,
            event,
            error.message || String(error)
          );
        }
      }

      if (response.latestLedger) {
        this.currentLedger = response.latestLedger + 1;
      }
    } catch (error) {
      console.error("Error in pollEvents:", error);
    }
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
  await startServer(database, config.apiPort, () => subscriber.retryDLQEvents(true));
}

if (require.main === module) {
  main().catch((error) => {
    console.error("Fatal error:", error);
    process.exit(1);
  });
}
