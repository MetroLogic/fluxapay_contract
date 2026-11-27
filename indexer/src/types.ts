/**
 * FluxaPay Indexer Event Types
 * Defines the structure of events emitted by FluxaPay contracts
 */

export interface ContractEvent {
  id: string;
  timestamp: number;
  ledger: number;
  txHash: string;
  contractId: string;
  topic: string[];
  value: unknown;
}

export interface PaymentEvent extends ContractEvent {
  topic: ["PAYMENT", "CREATED" | "CONFIRMED" | "SETTLED" | "FAILED" | "CANCELLED"];
  value: {
    payment_id: string;
    merchant_id: string;
    amount: number;
    currency: string;
  };
}

/**
 * Issue #1037: Emitted by both `create_payment` (when called with
 * `retry_of_payment_id`) and `retry_payment` when a new payment is linked to
 * an expired/failed original. Payload is a positional 2-tuple, not the usual
 * named-field map, matching the on-chain `env.events().publish(topic, (a, b))` call.
 */
export interface PaymentRetryCreatedEvent extends ContractEvent {
  topic: ["PAYMENT", "RETRY_CREATED"];
  value: {
    original_payment_id: string;
    new_payment_id: string;
  };
}

export interface PaymentFeeEvent extends ContractEvent {
  topic: ["PAYMENT", "FEE_TOTAL"];
  value: {
    payment_id: string;
    amount: string | number | bigint;
  };
}

export interface RefundEvent extends ContractEvent {
  topic: ["REFUND", "CREATED" | "PROCESSED" | "REJECTED"];
  value: {
    refund_id: string;
    payment_id: string;
    amount: number;
  };
}

export interface DisputeEvent extends ContractEvent {
  topic: ["DISPUTE", "CREATED" | "RESOLVED" | "REJECTED" | "ESCALATED" | "ADJUSTMENT"];
  value: {
    dispute_id: string;
    payment_id: string;
    amount: number;
  };
}

/**
 * Issue #677: dispute bond lifecycle events. Emitted when a bond is
 * released back to its owner (BOND_RETURNED) or forfeited to the treasury
 * (BOND_FORFEITED) after a dispute is resolved or rejected.
 */
export interface DisputeBondEvent extends ContractEvent {
  topic: ["DISPUTE", "BOND_RETURNED" | "BOND_FORFEITED"];
  value: {
    dispute_id: string;
    recipient: string;
    amount: number;
  };
}

export interface MerchantEvent extends ContractEvent {
  topic: ["MERCHANT", "REGISTERED" | "VERIFIED" | "SUSPENDED" | "REINSTATED"];
  value: {
    merchant_id: string;
    status: string;
  };
}

export interface StreamEvent extends ContractEvent {
  topic: ["STREAM", "CREATED" | "CLOSED" | "PAUSED" | "RESUMED" | "CANCELLED" | "WITHDRAWN"];
  value: {
    stream_id: string;
    sender?: string;
    receiver?: string;
    recipient?: string;
    amount?: number;
    remaining_deposit?: number;
    memo?: string | null;
  };
}

export interface SubscriptionEvent extends ContractEvent {
  topic: [
    "SUBSCRIPTION",
    | "PLAN_CREATED"
    | "PLAN_DEACTIVATED"
    | "CREATED"
    | "ACTIVE"
    | "CHARGED"
    | "CHARGE_FAILED"
    | "DUNNING_TRIGGERED"
    | "CANCELLED"
    | "EXPIRED"
    | "COMPLETED"
    | "PAUSED"
    | "RESUMED"
    | "TRIAL_STARTED"
    | "TRIAL_ENDED"
  ];
  value: {
    subscription_id?: string;
    payer?: string;
    status?: string;
    plan_id?: string;
    merchant_id?: string;
    amount?: number;
    interval_secs?: number;
  };
}

export interface InvoiceEvent extends ContractEvent {
  topic: ["INVOICE", "CREATED" | "PAID" | "OVERDUE"];
  value: {
    invoice_id: string;
    merchant_id: string;
    total_amount: number;
  };
}

/**
 * Emitted by `PaymentProcessor::settle_payment` when the settled merchant
 * has an `AnchorConfig` on file (see `merchant_registry.rs`). Consumed by
 * `SettlementService` (`settlement.ts`) to drive the SEP-6/SEP-24
 * USDC-to-fiat offramp. Topics carry `merchant_id` and `anchor_domain` in
 * addition to the type/action pair, so this event is decoded separately
 * from the generic `scValToObject` path — see `tryParseAnchorWithdraw` in
 * `index.ts`.
 */
export interface SettlementAnchorWithdrawEvent extends ContractEvent {
  topic: ["PAYMENT", "ANCHOR_WITHDRAW"];
  value: {
    payment_id: string;
    amount: number;
    currency: string;
    payout_address: string;
    sep6_endpoint: string;
    sep24_endpoint: string;
    supported_currencies: string[];
    ledger_timestamp: number;
  };
}

export interface FXOracleEvent extends ContractEvent {
  topic: ["FX_ORACLE" | "ORACLE" | "FX_RATE", "UPDATED" | "SET" | "REMOVED"];
  value: {
    pair?: string;
    asset?: string;
    rate: string | number | bigint;
    decimals?: number;
  };
}

export interface LinkCreatedEvent extends ContractEvent {
  topic: ["LINK", "CREATED"];
  value: {
    link_id: string;
    merchant_id: string;
  };
}

export interface LinkUsedEvent extends ContractEvent {
  topic: ["LINK", "USED"];
  value: {
    link_id: string;
    payer: string;
    amount: bigint | number;
    payment_id: string;
    metadata?: Record<string, string>;
  };
}

export interface LinkDeactivatedEvent extends ContractEvent {
  topic: ["LINK", "DEACTIVATED"];
  value: {
    link_id: string;
  };
}

export interface LinkExpiredEvent extends ContractEvent {
  topic: ["LINK", "EXPIRED"];
  value: {
    link_id: string;
  };
}

export interface LinkViewedEvent extends ContractEvent {
  topic: ["LINK", "VIEWED"];
  value: {
    link_id: string;
  };
}

export type PaymentLinkEvent =
  | LinkCreatedEvent
  | LinkUsedEvent
  | LinkDeactivatedEvent
  | LinkExpiredEvent
  | LinkViewedEvent;
export type AnyEvent =
  | PaymentEvent
  | PaymentRetryCreatedEvent
  | PaymentFeeEvent
  | RefundEvent
  | DisputeEvent
  | DisputeBondEvent
  | MerchantEvent
  | StreamEvent
  | SubscriptionEvent
  | InvoiceEvent
  | SettlementAnchorWithdrawEvent;
  | FXOracleEvent
  | PaymentLinkEvent;