/**
 * FluxaPay Indexer Event Types
 * Defines the structure of events emitted by the FluxaPay contract
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
  topic: ["PAYMENT", "CREATED" | "CONFIRMED" | "SETTLED" | "FAILED"];
  value: {
    payment_id: string;
    merchant_id: string;
    amount: number;
    currency: string;
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
  topic: ["DISPUTE", "CREATED" | "RESOLVED" | "ESCALATED"];
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
  topic: ["MERCHANT", "REGISTERED" | "VERIFIED" | "SUSPENDED"];
  value: {
    merchant_id: string;
    status: string;
  };
}

export interface StreamEvent extends ContractEvent {
  topic: ["STREAM", "CREATED" | "CLOSED" | "PAUSED" | "RESUMED"];
  value: {
    stream_id: string;
    sender: string;
    receiver: string;
    amount: number;
  };
}

export interface SubscriptionEvent extends ContractEvent {
  topic: ["SUBSCRIPTION", "CREATED" | "ACTIVE" | "CANCELLED" | "PAUSED"];
  value: {
    subscription_id: string;
    payer: string;
    status: string;
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

export type AnyEvent =
  | PaymentEvent
  | RefundEvent
  | DisputeEvent
  | DisputeBondEvent
  | MerchantEvent
  | StreamEvent
  | SubscriptionEvent
  | InvoiceEvent;
