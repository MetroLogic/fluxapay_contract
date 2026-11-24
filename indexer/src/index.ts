/**
 * FluxaPay Soroban Event Consumer
 * Subscribes to contract events via stellar-sdk from multiple contracts simultaneously,
 * persists events to PostgreSQL, exposes dead-letter queue retry, and starts the REST API.
 */

import { SorobanRpc as rpc, scValToNative } from "stellar-sdk";
import { Database } from "./database";
import { ContractEvent, AnyEvent, StreamEvent } from "./types";
import { AnchorWithdrawPayload, SettlementService } from "./settlement";
import { startServer } from "./server";
import { sseManager } from "./sse";
import {
  buildEnvelope,
  deliverOnce,
  WebhookStore as PgWebhookStore,
  type FetchLike,
  type WebhookEventType,
  type WebhookStore,
} from "./webhooks";
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

export type ReplayWebhookDispatcher = (event: AnyEvent) => Promise<void>;

export interface EventSubscriberDependencies {
  eventSource?: Pick<rpc.Server, "getEvents">;
  replayWebhookDispatcher?: ReplayWebhookDispatcher;
  webhookFetch?: FetchLike;
}

function getWebhookEventType(event: AnyEvent): WebhookEventType | null {
  const [type, subtype] = event.topic;
  const eventTypes: Record<string, WebhookEventType> = {
    "PAYMENT/CREATED": "payment.created",
    "PAYMENT/PENDING": "payment.pending",
    "PAYMENT/CONFIRMED": "payment.confirmed",
    // The contract's actual topic for a confirmed payment is "VERIFIED" (see
    // `fluxapay::PaymentProcessor::verify_payment`/`confirm_payment`); kept
    // alongside "PAYMENT/CONFIRMED" above in case any emitter path uses that
    // literal topic instead.
    "PAYMENT/VERIFIED": "payment.confirmed",
    "PAYMENT/SETTLED": "payment.confirmed",
    "PAYMENT/PARTIALLY_PAID": "payment.partially_paid",
    "PAYMENT/OVERPAID": "payment.overpaid",
    "PAYMENT/FAILED": "payment.failed",
    "PAYMENT/CANCELLED": "payment.cancelled",
    "REFUND/CREATED": "refund.created",
    "REFUND/COMPLETED": "refund.completed",
    // Issue #1039: all 6 dispute lifecycle events, broadcast over SSE (and
    // available to webhooks) with a schema matching `dispute.*` elsewhere.
    "DISPUTE/CREATED": "dispute.opened",
    "DISPUTE/REVIEWED": "dispute.responded",
    "DISPUTE/ESCALATED": "dispute.escalated",
    "DISPUTE/VOTE_CAST": "dispute.vote_cast",
    "DISPUTE/RESOLVED": "dispute.resolved",
    "DISPUTE/AUTO_RESOLVED": "dispute.auto_resolved",
  };
  return eventTypes[`${type}/${subtype}`] ?? null;
}

/**
 * Issue #1039: resolve the merchant that owns `event` for live SSE
 * broadcasting. Mirrors the replay webhook dispatcher's `merchant_id` /
 * `payment_id` lookup, plus a `dispute_id` fallback for dispute events that
 * carry only a dispute ID (`DISPUTE/VOTE_CAST`, `DISPUTE/AUTO_RESOLVED`).
 */
async function resolveMerchantIdForEvent(
  database: Database,
  value: Record<string, unknown>,
): Promise<string | null> {
  if (typeof value.merchant_id === "string") {
    return value.merchant_id;
  }

  let paymentId = typeof value.payment_id === "string" ? value.payment_id : null;
  if (!paymentId && typeof value.dispute_id === "string") {
    const dispute = (await database.getDisputeById(value.dispute_id)) as
      | { payment_id?: unknown }
      | null;
    if (dispute && typeof dispute.payment_id === "string") {
      paymentId = dispute.payment_id;
    }
  }

  if (!paymentId) return null;

  const payment = (await database.getPaymentById(paymentId)) as
    | { merchant_id?: unknown }
    | null;
  return payment && typeof payment.merchant_id === "string" ? payment.merchant_id : null;
}

export function createReplayWebhookDispatcher(
  database: Database,
  store: WebhookStore,
  fetch: FetchLike = globalThis.fetch as unknown as FetchLike,
): ReplayWebhookDispatcher {
  return async (event) => {
    const eventType = getWebhookEventType(event);
    if (!eventType) return;

    const value = event.value as Record<string, unknown>;
    let merchantId =
      typeof value.merchant_id === "string" ? value.merchant_id : null;
    if (!merchantId && typeof value.payment_id === "string") {
      const payment = (await database.getPaymentById(value.payment_id)) as
        | { merchant_id?: unknown }
        | null;
      merchantId =
        payment && typeof payment.merchant_id === "string" ? payment.merchant_id : null;
    }
    if (!merchantId) {
      console.warn(`Cannot dispatch replay webhook for event ${event.id}: merchant not found`);
      return;
    }

    const endpoints = await store.listEnabledEndpointsForEvent(merchantId, eventType);
    const envelope = buildEnvelope(
      eventType,
      { ...value, replayed: true },
      true,
      Date.now,
      { eventId: event.id, replayed: true },
    );

    for (const endpoint of endpoints) {
      const { attempt } = await deliverOnce(endpoint, envelope, 1, { fetch });
      await store.recordAttempt(attempt);
    }
  };
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
  private server: Pick<rpc.Server, "getEvents">;
  private database: Database;
  private config: EventSubscriptionConfig;
  private currentLedger: number;
  private settlementService: SettlementService;
  private pollTimer: NodeJS.Timeout | null = null;
  private dlqRetryTimer: NodeJS.Timeout | null = null;
  private replayWebhookDispatcher?: ReplayWebhookDispatcher;

  constructor(
    config: EventSubscriptionConfig,
    database?: Database,
    dependencies: EventSubscriberDependencies = {},
  ) {
    this.config = config;
    const allowHttp = config.rpcUrl.startsWith("http://");
    this.server = dependencies.eventSource ?? new rpc.Server(config.rpcUrl, { allowHttp });
    this.database = database || new Database(config.dbConnectionString);
    this.currentLedger = config.startLedger;
    this.settlementService = new SettlementService(this.database);
    this.settlementService.onSettlementEvent((event) => {
      console.log(
        `${event.type} payment=${event.paymentId} anchorTxn=${event.anchorTxnId ?? "n/a"}${
          event.reason ? ` reason=${event.reason}` : ""
        }`
      );
    });
    if (dependencies.replayWebhookDispatcher) {
      this.replayWebhookDispatcher = dependencies.replayWebhookDispatcher;
    } else if (typeof this.database.getPool === "function") {
      this.replayWebhookDispatcher = createReplayWebhookDispatcher(
        this.database,
        new PgWebhookStore(this.database.getPool()),
        dependencies.webhookFetch,
      );
    }
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
            await this.dispatchLiveSSE(parsedEvent);
          }
        }

        // Settlement offramp: PaymentProcessor::settle_payment emits
        // (PAYMENT, ANCHOR_WITHDRAW, merchant_id, anchor_domain) whenever
        // the settled merchant has an AnchorConfig on file. Trigger the
        // SEP-6 withdrawal flow for it, independent of whether the raw
        // event was also persisted to contract_events above.
        const anchorWithdraw = this.tryParseAnchorWithdraw(event);
        if (anchorWithdraw) {
          await this.settlementService.settle(anchorWithdraw);
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

  /**
   * Issue #1039: broadcast a just-stored event over SSE to the owning
   * merchant's connected clients (e.g. a dispute detail page subscribed via
   * `GET /v1/events/stream?dispute_id=...`). Failures here are logged, not
   * thrown — a broadcast problem must never roll back the DB write or land
   * the event in the dead-letter queue, since it was already stored fine.
   */
  private async dispatchLiveSSE(event: AnyEvent): Promise<void> {
    try {
      const eventType = getWebhookEventType(event);
      if (!eventType) return;

      const value = event.value as Record<string, unknown>;
      const merchantId = await resolveMerchantIdForEvent(this.database, value);
      if (!merchantId) return;

      sseManager.broadcast(merchantId, eventType, value);
    } catch (error) {
      console.error(`Error broadcasting SSE for event ${event.id}:`, error);
    }
  }

  parseEvent(event: any): AnyEvent | null {
    try {
      const eventId = `${event.ledger}-${event.txHash}-${event.id}`;
      const parsedLedgerTimestamp = event.ledgerClosedAt
        ? Math.floor(Date.parse(event.ledgerClosedAt) / 1000)
        : 0;
      const timestamp = event.timestamp || parsedLedgerTimestamp || Math.floor(Date.now() / 1000);
      const ledger = typeof event.ledger === "number" ? event.ledger : parseInt(event.ledger, 10);
      const txHash = event.txHash || "";
      const contractId = String(event.contractId || event.contract_id || "");

      const rawTopics: unknown[] = Array.isArray(event.topic) ? event.topic : [];
      const topics = rawTopics.map((topic: unknown) => {
        const nativeTopic =
          topic &&
          typeof topic === "object" &&
          "switch" in topic &&
          typeof topic.switch === "function"
            ? scValToNative(topic as Parameters<typeof scValToNative>[0])
            : topic;
        return String(nativeTopic);
      });
      if (topics.length < 2) {
        console.warn("Invalid event topics:", topics);
        return null;
      }

      let value: Record<string, unknown> = {};
      if (event.value) {
        try {
          if (topics[0] === "PAYMENT" && topics[1] === "RETRY_CREATED") {
            // Issue #1037: PAYMENT/RETRY_CREATED carries a positional 2-tuple
            // (original_payment_id, new_payment_id) rather than the usual
            // named-field map, so it needs its own conversion.
            value = this.parsePaymentRetryCreatedValue(event.value);
          } else {
            value = this.scValToObject(event.value, topics, rawTopics);
          }
        } catch (e) {
          console.warn("Could not parse event value:", e);
        }
      }

      const decodeNativeValue = (rawValue: unknown): unknown => {
        if (typeof rawValue === "string") {
          return scValToNative(xdr.ScVal.fromXDR(rawValue, "base64"));
        }
        if (rawValue && typeof rawValue === "object" && "switch" in rawValue) {
          return scValToNative(rawValue as xdr.ScVal);
        }
        return rawValue;
      };

      if (topics[0] === "FX_RATE" && topics[1] === "UPDATED") {
        const decoded = decodeNativeValue(event.value);

        const fields = Array.isArray(decoded)
          ? { pair: decoded[0], rate: decoded[1], decimals: decoded[2] }
          : decoded;
        if (!fields || typeof fields !== "object") {
          throw new Error("FX_RATE/UPDATED event has an invalid payload");
        }

        const fx = fields as { pair?: unknown; rate?: unknown; decimals?: unknown };
        const pair = String(fx.pair ?? "");
        const rate = String(fx.rate ?? "");
        const decimals = Number(fx.decimals);
        if (!pair || !/^[A-Z0-9]+_[A-Z0-9]+$/.test(pair) ||
            !/^[1-9]\d*$/.test(rate) || !Number.isInteger(decimals) ||
            decimals < 0 || decimals > 38) {
          throw new Error("FX_RATE/UPDATED event contains an invalid pair or rate");
        }
        value = { pair, rate, decimals };
      } else if (topics[0] === "DISPUTE" && topics[1] === "ADJUSTMENT") {
        const decoded = decodeNativeValue(event.value);
        if (!Array.isArray(decoded) || decoded.length !== 4) {
          throw new Error("DISPUTE/ADJUSTMENT event has an invalid payload");
        }
        value = {
          dispute_id: String(decoded[0]),
          payment_id: String(decoded[1]),
          refund_id: decoded[2] == null ? null : String(decoded[2]),
          amount: String(decoded[3]),
        };
      } else if (topics[0] === "PAYMENT" && topics[1] === "FEE_TOTAL") {
        const paymentId = topics[2] || "";
        const amount = String(decodeNativeValue(event.value));
        if (!paymentId || !/^\d+$/.test(amount)) {
          throw new Error("PAYMENT/FEE_TOTAL event has an invalid payment ID or amount");
        }
        value = { payment_id: paymentId, amount };
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

  /**
   * Issue #1037: Convert the (original_payment_id, new_payment_id) tuple
   * emitted by PAYMENT/RETRY_CREATED into a named value object. Handles both
   * a plain array of two strings and an array of ScVal-like entries (each
   * carrying its string under `.value` once run through the same basic
   * unwrapping as `scValToObject`).
   */
  private parsePaymentRetryCreatedValue(raw: any): Record<string, unknown> {
    const unwrap = (entry: any): string =>
      typeof entry === "string" ? entry : String(entry?.value ?? entry ?? "");

    const tuple = Array.isArray(raw) ? raw : raw?.raw;
    if (Array.isArray(tuple) && tuple.length >= 2) {
      return {
        original_payment_id: unwrap(tuple[0]),
        new_payment_id: unwrap(tuple[1]),
      };
    }

    console.warn("Could not parse PAYMENT/RETRY_CREATED value as a 2-tuple:", raw);
    return {};
   * Decode a raw RPC event into an `AnchorWithdrawPayload` if (and only if)
   * its topics match `(PAYMENT, ANCHOR_WITHDRAW, merchant_id, anchor_domain)`.
   * Returns `null` for every other event type.
   *
   * Unlike `scValToObject` (a best-effort placeholder used for the generic
   * dedup path), this uses `stellar-sdk`'s `scValToNative` to fully decode
   * the topic and value ScVals, since the settlement flow depends on exact
   * field values (amount, endpoints, payout address) rather than a loose
   * approximation.
   */
  private tryParseAnchorWithdraw(event: any): AnchorWithdrawPayload | null {
    try {
      const rawTopics = Array.isArray(event.topic) ? event.topic : [];
      if (rawTopics.length < 4) return null;

      const topic0 = scValToNative(rawTopics[0]);
      const topic1 = scValToNative(rawTopics[1]);
      if (topic0 !== "PAYMENT" || topic1 !== "ANCHOR_WITHDRAW") return null;

      const merchantId = String(scValToNative(rawTopics[2]));
      const anchorDomain = String(scValToNative(rawTopics[3]));

      // Payload tuple order per fluxapay/src/lib.rs settle_payment:
      // (payment_id, amount, currency, payout_addr, sep6_endpoint,
      //  sep24_endpoint, supported_currencies, ledger_timestamp)
      const decodedValue = scValToNative(event.value) as unknown[];
      if (!Array.isArray(decodedValue) || decodedValue.length < 8) return null;

      const [
        paymentId,
        amount,
        currency,
        payoutAddress,
        sep6Endpoint,
        sep24Endpoint,
        supportedCurrencies,
        ledgerTimestamp,
      ] = decodedValue;

      return {
        paymentId: String(paymentId),
        merchantId,
        amount: Number(amount),
        currency: String(currency),
        payoutAddress: String(payoutAddress),
        anchorDomain,
        sep6Endpoint: String(sep6Endpoint ?? ""),
        sep24Endpoint: String(sep24Endpoint ?? ""),
        supportedCurrencies: Array.isArray(supportedCurrencies)
          ? supportedCurrencies.map(String)
          : [],
        ledgerTimestamp: Number(ledgerTimestamp),
      };
    } catch (error) {
      // Not every event matches the anchor-withdraw shape; failing to
      // decode is expected for the vast majority of events and is not an
      // error on its own.
      return null;
    }
  }

  scValToObject(
    scval: unknown,
    topics: string[] = [],
    rawTopics: unknown[] = [],
  ): Record<string, unknown> {
    const value =
      scval &&
      typeof scval === "object" &&
      "switch" in scval &&
      typeof scval.switch === "function"
        ? scValToNative(scval as Parameters<typeof scValToNative>[0])
        : scval;
    if (value && typeof value === "object" && !Array.isArray(value)) {
      return value as Record<string, unknown>;
    }
    if (!Array.isArray(value)) {
      return value === undefined ? {} : { value };
    }

    const topic = `${topics[0]}/${topics[1]}`;
    switch (topic) {
      case "PAYMENT/CREATED":
        return {
          payment_id: value[0],
          merchant_id: value[1],
          amount: value[2],
          metadata: value[3],
        };
      case "PAYMENT/FAILED":
      case "PAYMENT/OVERPAID":
        return {
          payment_id: value[0],
          merchant_id: value[1],
          amount: value[2],
          amount_received: value[3],
        };
      // Issue #1035: merchant_id travels in topics[2] (not value) for this
      // event, and the 4th value element is the optional payer_muxed_id
      // (null for a non-muxed G-address payer).
      case "PAYMENT/VERIFIED":
        return {
          payment_id: value[0],
          merchant_id: topics[2],
          amount: value[1],
          amount_received: value[2],
          payer_muxed_id: value[3] ?? null,
        };
      case "PAYMENT/SETTLED":
        return {
          payment_id: value[0],
          amount: value[1],
          merchant_id: topics[2],
        };
      case "PAYMENT/CANCELLED":
        return {
          payment_id: value[0],
          authority: value[1],
          merchant_id: topics[2],
        };
      case "REFUND/CREATED":
      case "REFUND/COMPLETED":
        return {
          payment_id: value[0],
          refund_id: value[1],
          amount: value[2],
        };
      case "DISPUTE/CREATED":
      case "DISPUTE/RESOLVED":
      case "DISPUTE/REJECTED":
        return { dispute_id: value[0], payment_id: value[1] };
      case "DISPUTE/ESCALATED":
        return {
          payment_id: value[0],
          dispute_id: value[1],
          amount: value[2],
        };
      case "LINK/CREATED":
        return { link_id: value[0], merchant_id: value[1] };
      case "LINK/USED":
        return {
          link_id: value[0],
          payer: value[1],
          amount: value[2],
          payment_id: value[3],
          metadata: value[4],
        };
      case "SUBSCRIPTION/PLAN_CREATED":
        return {
          plan_id: value[0],
          merchant_id: value[1],
          amount: value[2],
          interval_secs: value[3],
        };
      case "SUBSCRIPTION/PLAN_DEACTIVATED":
        return { plan_id: value[0], merchant_id: value[1] };
      case "SUBSCRIPTION/CREATED":
        return { subscription_id: value[0], payer: value[1], plan_id: value[2] };
      case "SUBSCRIPTION/TRIAL_STARTED":
        return {
          subscription_id: value[0],
          payer: value[1],
          plan_id: value[2],
          trial_ends_at: value[3],
        };
      case "SUBSCRIPTION/TRIAL_ENDED":
        return { subscription_id: value[0], payer: value[1], plan_id: value[2] };
      case "SUBSCRIPTION/CHARGED":
        return {
          subscription_id: value[0],
          payer: value[1],
          amount: value[2],
          cycle: value[3],
        };
      case "SUBSCRIPTION/DUNNING_TRIGGERED":
        return {
          subscription_id: value[0],
          payer: value[1],
          dunning_attempts: value[2],
          next_retry_at: value[3],
        };
      case "SUBSCRIPTION/CHARGE_FAILED":
        return {
          subscription_id: value[0],
          payer: value[1],
          amount: value[2],
          dunning_attempts: value[3],
        };
      case "SUBSCRIPTION/PAUSED":
      case "SUBSCRIPTION/RESUMED":
        return { subscription_id: value[0], payer: value[1] };
      case "SUBSCRIPTION/CANCELLED":
      case "SUBSCRIPTION/EXPIRED":
        return { subscription_id: value[0], payer: value[1] };
      case "SUBSCRIPTION/COMPLETED":
        return { subscription_id: value[0], total_cycles: value[1] };
      default:
        return {
          raw: value,
          ...(rawTopics[2] === undefined ? {} : { topic_value: topics[2] }),
        };
    }
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
    onProgress?: (progress: {
      processed: number;
      total: number;
      stored: number;
      skipped_duplicates?: number;
      currentLedger?: number;
    }) => void,
  ): Promise<{ processed: number; stored: number; skipped_duplicates: number; total: number }> {
    const totalLedgers = Math.max(1, toLedger - fromLedger + 1);
    let processed = 0;
    let stored = 0;
    let skippedDuplicates = 0;
    let currentStart = fromLedger;
    let cursor: string | undefined;
    let cursorLedger: number | undefined;

    while (currentStart <= toLedger) {
      const request: Parameters<rpc.Server["getEvents"]>[0] = {
        filters: [
          {
            type: "contract",
            contractIds: this.config.contractIds,
          },
        ],
        startLedger: currentStart,
        cursor,
        limit: 100,
      };

      const response = await this.server.getEvents(request);
      if (!response.events || response.events.length === 0) {
        if (cursor && cursorLedger !== undefined) {
          currentStart = cursorLedger + 1;
          cursor = undefined;
          cursorLedger = undefined;
          continue;
        }
        if (response.latestLedger && response.latestLedger < toLedger) {
          currentStart = response.latestLedger + 1;
        } else {
          break;
        }
      } else {
        for (const event of response.events) {
          const ledger = typeof event.ledger === "number" ? event.ledger : parseInt(event.ledger, 10);
          if (!Number.isSafeInteger(ledger) || ledger < currentStart) {
            throw new Error(`Soroban RPC returned invalid replay event ledger: ${event.ledger}`);
          }
          if (ledger > toLedger) break;

          processed++;
          const parsedEvent = this.parseEvent(event);
          if (parsedEvent) {
            const wasStored = await this.database.storeEvent(parsedEvent);
            if (wasStored) {
              stored++;
              await this.replayWebhookDispatcher?.(parsedEvent);
            } else {
              skippedDuplicates++;
            }
          }
        }

        const lastLedger = response.events[response.events.length - 1].ledger;
        const lastNum = typeof lastLedger === "number" ? lastLedger : parseInt(lastLedger, 10);
        if (!Number.isSafeInteger(lastNum) || lastNum < currentStart) {
          throw new Error(`Soroban RPC returned invalid replay page ledger: ${lastLedger}`);
        }
        if (lastNum > toLedger) {
          break;
        }
        if (response.events.length >= 100) {
          const nextCursor = response.events[response.events.length - 1].pagingToken;
          if (!nextCursor) {
            throw new Error("Soroban RPC replay page is full but has no paging token");
          }
          cursor = nextCursor;
          cursorLedger = lastNum;
        } else {
          currentStart = Math.max(currentStart + 1, lastNum + 1);
          cursor = undefined;
          cursorLedger = undefined;
          if (lastNum >= toLedger) break;
        }
      }

      if (onProgress) {
        onProgress({
          processed,
          total: totalLedgers,
          stored,
          skipped_duplicates: skippedDuplicates,
          currentLedger: Math.min(toLedger, currentStart),
        });
      }
    }

    if (onProgress) {
      onProgress({
        processed,
        total: totalLedgers,
        stored,
        skipped_duplicates: skippedDuplicates,
        currentLedger: toLedger,
      });
    }

    return {
      processed,
      stored,
      skipped_duplicates: skippedDuplicates,
      total: totalLedgers,
    };
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
