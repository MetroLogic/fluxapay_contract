/**
 * Strongly typed on-chain event payloads and discrimination helper for FluxaPay contracts.
 *
 * References:
 * - docs/events.md
 * - fluxapay/EVENTS.md
 * - fluxapay/src/events.rs
 */

// ==========================================
// Base Event Interfaces
// ==========================================

export interface BaseFluxapayEvent<TType extends string, TNamespace extends string, TAction extends string, TPayload> {
  type: TType;
  namespace: TNamespace;
  action: TAction;
  contractId?: string;
  ledger?: number;
  txHash?: string;
  timestamp?: number;
  payload: TPayload;
  raw: unknown;
}

// ==========================================
// 1. Payment Events (fluxapay/src/payment_processor.rs)
// ==========================================

/**
 * Emitted when a new payment charge is created.
 * Source: fluxapay/src/payment_processor.rs, docs/events.md
 */
export interface PaymentCreatedPayload {
  payment_id: string;
  merchant_id: string;
  amount: bigint;
  metadata?: Record<string, string> | null;
}
export type PaymentCreatedEvent = BaseFluxapayEvent<"PAYMENT/CREATED", "PAYMENT", "CREATED", PaymentCreatedPayload>;

/**
 * Emitted when a payment deposit is confirmed on-chain.
 * Source: fluxapay/src/payment_processor.rs, docs/events.md
 */
export interface PaymentConfirmedPayload {
  payment_id: string;
  merchant_id: string;
  amount: bigint;
}
export type PaymentConfirmedEvent = BaseFluxapayEvent<"PAYMENT/CONFIRMED", "PAYMENT", "CONFIRMED", PaymentConfirmedPayload>;

/**
 * Emitted when a payment is verified on-chain.
 * Source: fluxapay/src/payment_processor.rs, fluxapay/EVENTS.md
 */
export interface PaymentVerifiedPayload {
  payment_id: string;
  merchant_id: string;
  amount: bigint;
  amount_received: bigint;
}
export type PaymentVerifiedEvent = BaseFluxapayEvent<"PAYMENT/VERIFIED", "PAYMENT", "VERIFIED", PaymentVerifiedPayload>;

/**
 * Emitted when funds are settled and swept to the merchant.
 * Source: fluxapay/src/payment_processor.rs, docs/events.md
 */
export interface PaymentSettledPayload {
  payment_id: string;
  merchant_id: string;
  net_amount: bigint;
}
export type PaymentSettledEvent = BaseFluxapayEvent<"PAYMENT/SETTLED", "PAYMENT", "SETTLED", PaymentSettledPayload>;

/**
 * Emitted when a payment is cancelled before expiry.
 * Source: fluxapay/src/payment_processor.rs, docs/events.md
 */
export interface PaymentCancelledPayload {
  payment_id: string;
  merchant_id: string;
  cancelled_by: string;
  amount?: bigint;
}
export type PaymentCancelledEvent = BaseFluxapayEvent<"PAYMENT/CANCELLED", "PAYMENT", "CANCELLED", PaymentCancelledPayload>;

/**
 * Emitted when a payment charge expires.
 * Source: fluxapay/src/payment_processor.rs, docs/events.md
 */
export interface PaymentExpiredPayload {
  payment_id: string;
  merchant_id?: string;
  amount?: bigint;
}
export type PaymentExpiredEvent = BaseFluxapayEvent<"PAYMENT/EXPIRED", "PAYMENT", "EXPIRED", PaymentExpiredPayload>;

/**
 * Emitted when received payment is underpaid beyond tolerance.
 * Source: fluxapay/src/payment_processor.rs, fluxapay/EVENTS.md
 */
export interface PaymentPartiallyPaidPayload {
  payment_id: string;
  merchant_id: string;
  amount: bigint;
  amount_received: bigint;
}
export type PaymentPartiallyPaidEvent = BaseFluxapayEvent<"PAYMENT/PARTIALLY_PAID", "PAYMENT", "PARTIALLY_PAID", PaymentPartiallyPaidPayload>;

/**
 * Emitted when received payment is overpaid beyond tolerance.
 * Source: fluxapay/src/payment_processor.rs, fluxapay/EVENTS.md
 */
export interface PaymentOverpaidPayload {
  payment_id: string;
  merchant_id: string;
  amount: bigint;
  amount_received: bigint;
}
export type PaymentOverpaidEvent = BaseFluxapayEvent<"PAYMENT/OVERPAID", "PAYMENT", "OVERPAID", PaymentOverpaidPayload>;

/**
 * Emitted when payment verification fails.
 * Source: fluxapay/src/payment_processor.rs, fluxapay/EVENTS.md
 */
export interface PaymentFailedPayload {
  payment_id: string;
  merchant_id: string;
  amount: bigint;
  amount_received: bigint;
}
export type PaymentFailedEvent = BaseFluxapayEvent<"PAYMENT/FAILED", "PAYMENT", "FAILED", PaymentFailedPayload>;

// ==========================================
// 2. Refund Events (fluxapay/src/refund_manager.rs)
// ==========================================

/**
 * Emitted when a refund request is initiated.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface RefundRequestedPayload {
  refund_id: string;
  payment_id: string;
  amount: bigint;
  requester: string;
}
export type RefundRequestedEvent = BaseFluxapayEvent<"REFUND/REQUESTED", "REFUND", "REQUESTED", RefundRequestedPayload>;

/**
 * Emitted when a refund is created.
 * Source: fluxapay/src/refund_manager.rs, fluxapay/EVENTS.md
 */
export interface RefundCreatedPayload {
  payment_id: string;
  refund_id: string;
  refund_amount: bigint;
}
export type RefundCreatedEvent = BaseFluxapayEvent<"REFUND/CREATED", "REFUND", "CREATED", RefundCreatedPayload>;

/**
 * Emitted when a refund is processed.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface RefundProcessedPayload {
  refund_id: string;
  payment_id: string;
  amount: bigint;
}
export type RefundProcessedEvent = BaseFluxapayEvent<"REFUND/PROCESSED", "REFUND", "PROCESSED", RefundProcessedPayload>;

/**
 * Emitted when a refund completes and tokens are returned.
 * Source: fluxapay/src/refund_manager.rs, fluxapay/EVENTS.md
 */
export interface RefundCompletedPayload {
  payment_id: string;
  refund_id: string;
  refund_amount: bigint;
}
export type RefundCompletedEvent = BaseFluxapayEvent<"REFUND/COMPLETED", "REFUND", "COMPLETED", RefundCompletedPayload>;

/**
 * Emitted when a refund is rejected by operator.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface RefundRejectedPayload {
  refund_id: string;
  payment_id: string;
  refund_amount?: bigint;
}
export type RefundRejectedEvent = BaseFluxapayEvent<"REFUND/REJECTED", "REFUND", "REJECTED", RefundRejectedPayload>;

// ==========================================
// 3. Dispute Events (fluxapay/src/refund_manager.rs)
// ==========================================

/**
 * Emitted when a dispute is opened.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface DisputeCreatedPayload {
  payment_id: string;
  dispute_id: string;
  amount?: bigint;
}
export type DisputeCreatedEvent = BaseFluxapayEvent<"DISPUTE/CREATED", "DISPUTE", "CREATED", DisputeCreatedPayload>;

/**
 * Emitted when a dispute is under review.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface DisputeReviewedPayload {
  payment_id: string;
  dispute_id: string;
}
export type DisputeReviewedEvent = BaseFluxapayEvent<"DISPUTE/REVIEWED", "DISPUTE", "REVIEWED", DisputeReviewedPayload>;

/**
 * Emitted when a dispute is resolved in favour of buyer or merchant.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface DisputeResolvedPayload {
  payment_id: string;
  dispute_id: string;
  ruling?: string;
}
export type DisputeResolvedEvent = BaseFluxapayEvent<"DISPUTE/RESOLVED", "DISPUTE", "RESOLVED", DisputeResolvedPayload>;

/**
 * Emitted when a dispute is rejected.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface DisputeRejectedPayload {
  payment_id: string;
  dispute_id: string;
}
export type DisputeRejectedEvent = BaseFluxapayEvent<"DISPUTE/REJECTED", "DISPUTE", "REJECTED", DisputeRejectedPayload>;

/**
 * Emitted when a dispute is escalated to arbitration.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface DisputeEscalatedPayload {
  payment_id: string;
  dispute_id: string;
  amount: bigint;
}
export type DisputeEscalatedEvent = BaseFluxapayEvent<"DISPUTE/ESCALATED", "DISPUTE", "ESCALATED", DisputeEscalatedPayload>;

/**
 * Emitted when a dispute bond is returned to disputer or merchant.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface DisputeBondReturnedPayload {
  dispute_id: string;
  recipient: string;
  amount: bigint;
}
export type DisputeBondReturnedEvent = BaseFluxapayEvent<"DISPUTE/BOND_RETURNED", "DISPUTE", "BOND_RETURNED", DisputeBondReturnedPayload>;

/**
 * Emitted when a dispute bond is forfeited to treasury.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface DisputeBondForfeitedPayload {
  dispute_id: string;
  recipient: string;
  amount: bigint;
}
export type DisputeBondForfeitedEvent = BaseFluxapayEvent<"DISPUTE/BOND_FORFEITED", "DISPUTE", "BOND_FORFEITED", DisputeBondForfeitedPayload>;

// ==========================================
// 4. Merchant Events (fluxapay/src/merchant_registry.rs)
// ==========================================

/**
 * Emitted when a new merchant registers on-chain.
 * Source: fluxapay/src/merchant_registry.rs, docs/events.md
 */
export interface MerchantRegisteredPayload {
  merchant_id: string;
  business_name?: string;
  settlement_currency?: string;
}
export type MerchantRegisteredEvent = BaseFluxapayEvent<"MERCHANT/REGISTERED", "MERCHANT", "REGISTERED", MerchantRegisteredPayload>;

/**
 * Emitted when merchant details are updated.
 * Source: fluxapay/src/merchant_registry.rs, docs/events.md
 */
export interface MerchantUpdatedPayload {
  merchant_id: string;
}
export type MerchantUpdatedEvent = BaseFluxapayEvent<"MERCHANT/UPDATED", "MERCHANT", "UPDATED", MerchantUpdatedPayload>;

/**
 * Emitted when merchant KYC is verified.
 * Source: fluxapay/src/merchant_registry.rs, docs/events.md
 */
export interface MerchantVerifiedPayload {
  merchant_id: string;
}
export type MerchantVerifiedEvent = BaseFluxapayEvent<"MERCHANT/VERIFIED", "MERCHANT", "VERIFIED", MerchantVerifiedPayload>;

/**
 * Emitted when merchant is suspended.
 * Source: fluxapay/src/merchant_registry.rs, docs/events.md
 */
export interface MerchantSuspendedPayload {
  merchant_id: string;
  reason?: string;
}
export type MerchantSuspendedEvent = BaseFluxapayEvent<"MERCHANT/SUSPENDED", "MERCHANT", "SUSPENDED", MerchantSuspendedPayload>;

/**
 * Emitted when suspended merchant is reinstated.
 * Source: fluxapay/src/merchant_registry.rs, docs/events.md
 */
export interface MerchantReinstatedPayload {
  merchant_id: string;
  reinstated_by?: string;
}
export type MerchantReinstatedEvent = BaseFluxapayEvent<"MERCHANT/REINSTATED", "MERCHANT", "REINSTATED", MerchantReinstatedPayload>;

/**
 * Emitted when a merchant's KYC tier is upgraded.
 * Source: fluxapay/src/merchant_registry.rs, fluxapay/EVENTS.md
 */
export interface KycTierUpgradedPayload {
  merchant_id: string;
  old_tier: string | number;
  new_tier: string | number;
}
export type KycTierUpgradedEvent = BaseFluxapayEvent<"KYC/TIER_UPGRADED", "KYC", "TIER_UPGRADED", KycTierUpgradedPayload>;

// ==========================================
// 5. Payment Link Events (fluxapay/src/payment_link.rs)
// ==========================================

/**
 * Emitted when a payment link is created.
 * Source: fluxapay/src/payment_link.rs, docs/events.md
 */
export interface LinkCreatedPayload {
  link_id: string;
  merchant_id: string;
}
export type LinkCreatedEvent = BaseFluxapayEvent<"LINK/CREATED", "LINK", "CREATED", LinkCreatedPayload>;

/**
 * Emitted when a payment link is paid by a payer.
 * Source: fluxapay/src/payment_link.rs, docs/events.md
 */
export interface LinkUsedPayload {
  link_id: string;
  payer: string;
  amount: bigint;
  payment_id: string;
  metadata?: Record<string, string> | null;
}
export type LinkUsedEvent = BaseFluxapayEvent<"LINK/USED", "LINK", "USED", LinkUsedPayload>;

/**
 * Emitted when a payment link is deactivated.
 * Source: fluxapay/src/payment_link.rs, docs/events.md
 */
export interface LinkDeactivatedPayload {
  link_id: string;
}
export type LinkDeactivatedEvent = BaseFluxapayEvent<"LINK/DEACTIVATED", "LINK", "DEACTIVATED", LinkDeactivatedPayload>;

/**
 * Emitted when a payment link expires.
 * Source: fluxapay/src/payment_link.rs, docs/events.md
 */
export interface LinkExpiredPayload {
  link_id: string;
}
export type LinkExpiredEvent = BaseFluxapayEvent<"LINK/EXPIRED", "LINK", "EXPIRED", LinkExpiredPayload>;

/**
 * Emitted when a payment link view is recorded.
 * Source: fluxapay/src/payment_link.rs, docs/events.md
 */
export interface LinkViewedPayload {
  link_id: string;
}
export type LinkViewedEvent = BaseFluxapayEvent<"LINK/VIEWED", "LINK", "VIEWED", LinkViewedPayload>;

// ==========================================
// 6. Subscription Events (fluxapay/src/subscription.rs, refund_manager.rs)
// ==========================================

/**
 * Emitted when a customer subscribes to a plan.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface SubscriptionCreatedPayload {
  subscription_id: string;
  payer: string;
  merchant_id?: string;
  plan_id?: string;
  amount?: bigint;
}
export type SubscriptionCreatedEvent = BaseFluxapayEvent<"SUBSCRIPTION/CREATED", "SUBSCRIPTION", "CREATED", SubscriptionCreatedPayload>;

/**
 * Emitted when a subscription recurring payment is charged.
 * Source: fluxapay/src/refund_manager.rs, fluxapay/EVENTS.md
 */
export interface SubscriptionChargedPayload {
  subscription_id: string;
  payer: string;
  merchant_id: string;
  amount: bigint;
  total_payments: number;
}
export type SubscriptionChargedEvent = BaseFluxapayEvent<"SUBSCRIPTION/CHARGED", "SUBSCRIPTION", "CHARGED", SubscriptionChargedPayload>;

/**
 * Emitted when a subscription is cancelled.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface SubscriptionCancelledPayload {
  subscription_id: string;
  payer?: string;
  cancelled_by?: string;
}
export type SubscriptionCancelledEvent = BaseFluxapayEvent<"SUBSCRIPTION/CANCELLED", "SUBSCRIPTION", "CANCELLED", SubscriptionCancelledPayload>;

/**
 * Emitted when a subscription reaches its max payments / expiration.
 * Source: fluxapay/src/refund_manager.rs, docs/events.md
 */
export interface SubscriptionExpiredPayload {
  subscription_id: string;
  payer: string;
}
export type SubscriptionExpiredEvent = BaseFluxapayEvent<"SUBSCRIPTION/EXPIRED", "SUBSCRIPTION", "EXPIRED", SubscriptionExpiredPayload>;

// ==========================================
// 7. Stream Events (fluxapay/src/stream.rs)
// ==========================================

/**
 * Emitted when a continuous payment stream is initialized.
 * Source: fluxapay/src/stream.rs, docs/events.md
 */
export interface StreamCreatedPayload {
  stream_id: string;
  sender: string;
  receiver?: string;
  deposit?: bigint;
  amount?: bigint;
}
export type StreamCreatedEvent = BaseFluxapayEvent<"STREAM/CREATED", "STREAM", "CREATED", StreamCreatedPayload>;

/**
 * Emitted when a stream is topped up with additional deposit.
 * Source: fluxapay/src/stream.rs, docs/events.md
 */
export interface StreamToppedUpPayload {
  stream_id: string;
  sender: string;
  amount: bigint;
}
export type StreamToppedUpEvent = BaseFluxapayEvent<"STREAM/TOPPED_UP", "STREAM", "TOPPED_UP", StreamToppedUpPayload>;

/**
 * Emitted when accrued stream funds are withdrawn by receiver.
 * Source: fluxapay/src/stream.rs, docs/events.md, Issue #766
 */
export interface StreamWithdrawnPayload {
  stream_id?: string;
  receiver?: string;
  destination?: string;
  amount: bigint;
  remaining?: bigint;
  remaining_deposit?: bigint;
  memo?: string | null;
}
export type StreamWithdrawnEvent = BaseFluxapayEvent<"STREAM/WITHDRAWN", "STREAM", "WITHDRAWN", StreamWithdrawnPayload>;

/**
 * Emitted when a stream is cancelled.
 * Source: fluxapay/src/stream.rs, docs/events.md
 */
export interface StreamCancelledPayload {
  stream_id: string;
  sender: string;
  accrued: bigint;
  refund: bigint;
}
export type StreamCancelledEvent = BaseFluxapayEvent<"STREAM/CANCELLED", "STREAM", "CANCELLED", StreamCancelledPayload>;

/**
 * Emitted when a stream is paused.
 * Source: fluxapay/src/stream.rs, docs/events.md
 */
export interface StreamPausedPayload {
  stream_id: string;
  sender: string;
}
export type StreamPausedEvent = BaseFluxapayEvent<"STREAM/PAUSED", "STREAM", "PAUSED", StreamPausedPayload>;

/**
 * Emitted when a paused stream is resumed.
 * Source: fluxapay/src/stream.rs, docs/events.md
 */
export interface StreamResumedPayload {
  stream_id: string;
  sender: string;
}
export type StreamResumedEvent = BaseFluxapayEvent<"STREAM/RESUMED", "STREAM", "RESUMED", StreamResumedPayload>;

/**
 * Emitted when stream rate per second is updated.
 * Source: fluxapay/src/stream.rs, docs/events.md
 */
export interface StreamRateUpdatedPayload {
  stream_id: string;
  sender: string;
  old_rate: bigint;
  new_rate: bigint;
  surplus: bigint;
}
export type StreamRateUpdatedEvent = BaseFluxapayEvent<"STREAM/RATE_UPDATED", "STREAM", "RATE_UPDATED", StreamRateUpdatedPayload>;

/**
 * Emitted when stream rate per second is decreased.
 * Source: fluxapay/src/stream.rs, docs/events.md
 */
export interface StreamRateDecreasedPayload {
  stream_id: string;
  sender: string;
  old_rate: bigint;
  new_rate: bigint;
  surplus: bigint;
}
export type StreamRateDecreasedEvent = BaseFluxapayEvent<"STREAM/RATE_DECREASED", "STREAM", "RATE_DECREASED", StreamRateDecreasedPayload>;

/**
 * Emitted when a stream milestone is approved.
 * Source: fluxapay/src/stream.rs, docs/events.md
 */
export interface StreamMilestoneApprovedPayload {
  stream_id: string;
  sender: string;
}
export type StreamMilestoneApprovedEvent = BaseFluxapayEvent<"STREAM/MILESTONE_APPROVED", "STREAM", "MILESTONE_APPROVED", StreamMilestoneApprovedPayload>;

/**
 * Emitted when a stream receiver sets a custom payout destination.
 * Source: fluxapay/src/stream.rs, docs/events.md
 */
export interface StreamDestinationSetPayload {
  stream_id: string;
  recipient: string;
  destination: string;
}
export type StreamDestinationSetEvent = BaseFluxapayEvent<"STREAM/DESTINATION_SET", "STREAM", "DESTINATION_SET", StreamDestinationSetPayload>;

/**
 * Emitted when a stream completes and is closed.
 * Source: fluxapay/src/stream.rs, docs/events.md
 */
export interface StreamClosedPayload {
  stream_id: string;
  sender: string;
  receiver: string;
  residual: bigint;
}
export type StreamClosedEvent = BaseFluxapayEvent<"STREAM/CLOSED", "STREAM", "CLOSED", StreamClosedPayload>;

// ==========================================
// 8. Oracle / FX Rate Events (fluxapay/src/fx_oracle.rs)
// ==========================================

/**
 * Emitted when an FX exchange rate is updated.
 * Source: fluxapay/src/fx_oracle.rs, docs/events.md
 */
export interface RateUpdatedPayload {
  pair: string;
  rate: bigint;
  timestamp: bigint;
}
export type RateUpdatedEvent = BaseFluxapayEvent<"RATE/UPDATED", "RATE", "UPDATED", RateUpdatedPayload>;

// ==========================================
// 9. Access Control Events (fluxapay/src/access_control.rs)
// ==========================================

/**
 * Emitted when an administrative role is granted.
 * Source: fluxapay/src/access_control.rs, docs/events.md
 */
export interface RoleGrantedPayload {
  role: string;
  account: string;
  granted_by?: string;
  timestamp?: bigint;
}
export type RoleGrantedEvent = BaseFluxapayEvent<"ACCESS_CONTROL/ROLE_GRANTED", "ACCESS_CONTROL", "ROLE_GRANTED", RoleGrantedPayload>;

/**
 * Emitted when an administrative role is revoked.
 * Source: fluxapay/src/access_control.rs, docs/events.md
 */
export interface RoleRevokedPayload {
  role: string;
  account: string;
  revoked_by?: string;
  timestamp?: bigint;
}
export type RoleRevokedEvent = BaseFluxapayEvent<"ACCESS_CONTROL/ROLE_REVOKED", "ACCESS_CONTROL", "ROLE_REVOKED", RoleRevokedPayload>;

/**
 * Emitted when an admin ownership transfer is proposed with a timelock.
 * Source: fluxapay/src/access_control.rs, Issue #764
 */
export interface AdminTransferProposedPayload {
  new_admin: string;
  earliest_acceptance_ledger: number;
}
export type AdminTransferProposedEvent = BaseFluxapayEvent<
  "ACCESS_CONTROL/ADMIN_TRANSFER_PROPOSED",
  "ACCESS_CONTROL",
  "ADMIN_TRANSFER_PROPOSED",
  AdminTransferProposedPayload
>;

/**
 * Emitted when an admin ownership transfer completes after timelock.
 * Source: fluxapay/src/access_control.rs, Issue #764
 */
export interface AdminTransferCompletedPayload {
  old_admin: string;
  new_admin: string;
  timestamp?: bigint;
}
export type AdminTransferCompletedEvent = BaseFluxapayEvent<
  "ACCESS_CONTROL/ADMIN_TRANSFER_COMPLETED",
  "ACCESS_CONTROL",
  "ADMIN_TRANSFER_COMPLETED",
  AdminTransferCompletedPayload
>;

/**
 * Emitted when an admin transfer proposal is aborted.
 * Source: fluxapay/src/access_control.rs, Issue #764
 */
export interface AdminTransferCancelledPayload {
  cancelled_by: string;
  timestamp?: bigint;
}
export type AdminTransferCancelledEvent = BaseFluxapayEvent<
  "ACCESS_CONTROL/ADMIN_TRANSFER_CANCELLED",
  "ACCESS_CONTROL",
  "ADMIN_TRANSFER_CANCELLED",
  AdminTransferCancelledPayload
>;

// ==========================================
// 10. Fee Split Events (fluxapay/src/payment_processor.rs)
// ==========================================

/**
 * Emitted when protocol fee split parameters are modified.
 * Source: fluxapay/src/payment_processor.rs, docs/events.md
 */
export interface FeeSplitUpdatedPayload {
  flat_fee: bigint;
  bps: number;
}
export type FeeSplitUpdatedEvent = BaseFluxapayEvent<"FEE_SPLIT/UPDATED", "FEE_SPLIT", "UPDATED", FeeSplitUpdatedPayload>;

// ==========================================
// 11. Treasury Events (fluxapay/src/payment_processor.rs)
// ==========================================

/**
 * Emitted when admin withdraws accumulated fees from treasury.
 * Source: fluxapay/src/payment_processor.rs, docs/events.md
 */
export interface TreasuryWithdrawnPayload {
  admin: string;
  amount: bigint;
}
export type TreasuryWithdrawnEvent = BaseFluxapayEvent<"TREASURY/WITHDRAWN", "TREASURY", "WITHDRAWN", TreasuryWithdrawnPayload>;

// ==========================================
// 12. Contract Upgrade Events
// ==========================================

/**
 * Emitted when contract WASM code is upgraded.
 * Source: fluxapay/src/payment_processor.rs, docs/events.md
 */
export interface ContractUpgradedPayload {
  old_version: string;
  new_version: string;
}
export type ContractUpgradedEvent = BaseFluxapayEvent<"CONTRACT/UPGRADED", "CONTRACT", "UPGRADED", ContractUpgradedPayload>;

// ==========================================
// 13. Invoice Events (fluxapay/src/payment_link.rs)
// ==========================================

/**
 * Emitted when an invoice is created.
 * Source: fluxapay/src/payment_link.rs, docs/events.md
 */
export interface InvoiceCreatedPayload {
  invoice_id: string;
  merchant_id: string;
  amount: bigint;
}
export type InvoiceCreatedEvent = BaseFluxapayEvent<"INVOICE/CREATED", "INVOICE", "CREATED", InvoiceCreatedPayload>;

/**
 * Emitted when an invoice is marked paid.
 * Source: fluxapay/src/payment_link.rs, docs/events.md
 */
export interface InvoicePaidPayload {
  invoice_id: string;
  merchant_id: string;
}
export type InvoicePaidEvent = BaseFluxapayEvent<"INVOICE/PAID", "INVOICE", "PAID", InvoicePaidPayload>;

/**
 * Emitted when an invoice becomes overdue.
 * Source: fluxapay/src/payment_link.rs, docs/events.md
 */
export interface InvoiceOverduePayload {
  invoice_id: string;
  merchant_id: string;
}
export type InvoiceOverdueEvent = BaseFluxapayEvent<"INVOICE/OVERDUE", "INVOICE", "OVERDUE", InvoiceOverduePayload>;

// ==========================================
// 14. Swap Events (fluxapay/src/dex_router.rs)
// ==========================================

/**
 * Emitted when a DEX token swap executes as part of payment.
 * Source: fluxapay/src/payment_processor.rs, fluxapay/EVENTS.md
 */
export interface SwapExecutedPayload {
  payment_id: string;
  sender: string;
  amount_in: bigint;
  amount_out: bigint;
}
export type SwapExecutedEvent = BaseFluxapayEvent<"SWAP/EXECUTED", "SWAP", "EXECUTED", SwapExecutedPayload>;

// ==========================================
// Generic / Unknown Fallback Event
// ==========================================

export interface UnknownFluxapayEvent extends BaseFluxapayEvent<string, string, string, Record<string, unknown>> {}

// ==========================================
// Discriminated Union of All Fluxapay Events
// ==========================================

export type FluxapayEvent =
  | PaymentCreatedEvent
  | PaymentConfirmedEvent
  | PaymentVerifiedEvent
  | PaymentSettledEvent
  | PaymentCancelledEvent
  | PaymentExpiredEvent
  | PaymentPartiallyPaidEvent
  | PaymentOverpaidEvent
  | PaymentFailedEvent
  | RefundRequestedEvent
  | RefundCreatedEvent
  | RefundProcessedEvent
  | RefundCompletedEvent
  | RefundRejectedEvent
  | DisputeCreatedEvent
  | DisputeReviewedEvent
  | DisputeResolvedEvent
  | DisputeRejectedEvent
  | DisputeEscalatedEvent
  | DisputeBondReturnedEvent
  | DisputeBondForfeitedEvent
  | MerchantRegisteredEvent
  | MerchantUpdatedEvent
  | MerchantVerifiedEvent
  | MerchantSuspendedEvent
  | MerchantReinstatedEvent
  | KycTierUpgradedEvent
  | LinkCreatedEvent
  | LinkUsedEvent
  | LinkDeactivatedEvent
  | LinkExpiredEvent
  | LinkViewedEvent
  | SubscriptionCreatedEvent
  | SubscriptionChargedEvent
  | SubscriptionCancelledEvent
  | SubscriptionExpiredEvent
  | StreamCreatedEvent
  | StreamToppedUpEvent
  | StreamWithdrawnEvent
  | StreamCancelledEvent
  | StreamPausedEvent
  | StreamResumedEvent
  | StreamRateUpdatedEvent
  | StreamRateDecreasedEvent
  | StreamMilestoneApprovedEvent
  | StreamDestinationSetEvent
  | StreamClosedEvent
  | RateUpdatedEvent
  | RoleGrantedEvent
  | RoleRevokedEvent
  | AdminTransferProposedEvent
  | AdminTransferCompletedEvent
  | AdminTransferCancelledEvent
  | FeeSplitUpdatedEvent
  | TreasuryWithdrawnEvent
  | ContractUpgradedEvent
  | InvoiceCreatedEvent
  | InvoicePaidEvent
  | InvoiceOverdueEvent
  | SwapExecutedEvent
  | UnknownFluxapayEvent;

// ==========================================
// Helper: Raw Event Parser & Type Discriminator
// ==========================================

export interface RawEventInput {
  topic?: unknown;
  topics?: unknown[];
  value?: unknown;
  contractId?: string;
  contract_id?: string;
  ledger?: number | string;
  txHash?: string;
  tx_hash?: string;
  timestamp?: number | string;
  [key: string]: unknown;
}

/**
 * Normalizes a topic item from Symbol / string / ScVal representation to an uppercase string.
 */
function normalizeTopic(item: unknown): string {
  if (typeof item === "string") return item.toUpperCase();
  if (item && typeof item === "object") {
    if ("value" in item && typeof (item as any).value === "string") {
      return (item as any).value.toUpperCase();
    }
    if (typeof (item as any).toString === "function") {
      const s = (item as any).toString();
      if (s !== "[object Object]") return s.toUpperCase();
    }
  }
  return String(item).toUpperCase();
}

/**
 * Safely converts an amount or numeric field to bigint.
 */
function toBigInt(val: unknown, fallback: bigint = 0n): bigint {
  if (typeof val === "bigint") return val;
  if (typeof val === "number") return BigInt(Math.floor(val));
  if (typeof val === "string") {
    try {
      return BigInt(val);
    } catch {
      return fallback;
    }
  }
  return fallback;
}

/**
 * Helper that discriminates a raw Soroban / Horizon event on the event topic tuple
 * and narrows it to the corresponding strongly typed FluxapayEvent interface.
 *
 * @param rawEvent - Raw event object returned from Soroban RPC getEvents or Horizon event stream
 * @returns The discriminated FluxapayEvent payload
 */
export function parseFluxapayEvent(rawEvent: RawEventInput): FluxapayEvent {
  const topicsRaw = Array.isArray(rawEvent.topic)
    ? rawEvent.topic
    : Array.isArray(rawEvent.topics)
    ? rawEvent.topics
    : [];

  const namespace = topicsRaw[0] ? normalizeTopic(topicsRaw[0]) : "UNKNOWN";
  const action = topicsRaw[1] ? normalizeTopic(topicsRaw[1]) : "UNKNOWN";
  const typeKey = `${namespace}/${action}`;

  const contractId = rawEvent.contractId || rawEvent.contract_id || undefined;
  const ledger = rawEvent.ledger !== undefined ? Number(rawEvent.ledger) : undefined;
  const txHash = rawEvent.txHash || rawEvent.tx_hash || undefined;
  const timestamp = rawEvent.timestamp !== undefined ? Number(rawEvent.timestamp) : undefined;

  let val: any = rawEvent.value ?? {};
  // If value has a nested .value or is an array
  if (val && typeof val === "object" && "value" in val && typeof val.value === "object" && !Array.isArray(val.value)) {
    val = val.value;
  }

  const base = {
    contractId,
    ledger,
    txHash,
    timestamp,
    raw: rawEvent,
  };

  switch (typeKey) {
    // 1. PAYMENT
    case "PAYMENT/CREATED": {
      const payload: PaymentCreatedPayload = {
        payment_id: val.payment_id || String(topicsRaw[2] || val[0] || ""),
        merchant_id: val.merchant_id || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
        metadata: val.metadata ?? null,
      };
      return { ...base, type: "PAYMENT/CREATED", namespace: "PAYMENT", action: "CREATED", payload };
    }
    case "PAYMENT/CONFIRMED": {
      const payload: PaymentConfirmedPayload = {
        payment_id: val.payment_id || String(topicsRaw[2] || val[0] || ""),
        merchant_id: val.merchant_id || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
      };
      return { ...base, type: "PAYMENT/CONFIRMED", namespace: "PAYMENT", action: "CONFIRMED", payload };
    }
    case "PAYMENT/VERIFIED": {
      const payload: PaymentVerifiedPayload = {
        payment_id: val.payment_id || String(topicsRaw[2] || val[0] || ""),
        merchant_id: val.merchant_id || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
        amount_received: toBigInt(val.amount_received ?? val[3]),
      };
      return { ...base, type: "PAYMENT/VERIFIED", namespace: "PAYMENT", action: "VERIFIED", payload };
    }
    case "PAYMENT/SETTLED": {
      const payload: PaymentSettledPayload = {
        payment_id: val.payment_id || String(topicsRaw[2] || val[0] || ""),
        merchant_id: val.merchant_id || val[1] || "",
        net_amount: toBigInt(val.net_amount ?? val.amount ?? val[2]),
      };
      return { ...base, type: "PAYMENT/SETTLED", namespace: "PAYMENT", action: "SETTLED", payload };
    }
    case "PAYMENT/CANCELLED": {
      const payload: PaymentCancelledPayload = {
        payment_id: val.payment_id || String(topicsRaw[2] || val[0] || ""),
        merchant_id: val.merchant_id || val[1] || "",
        cancelled_by: val.cancelled_by || val[2] || "",
        amount: val.amount !== undefined ? toBigInt(val.amount) : undefined,
      };
      return { ...base, type: "PAYMENT/CANCELLED", namespace: "PAYMENT", action: "CANCELLED", payload };
    }
    case "PAYMENT/EXPIRED": {
      const payload: PaymentExpiredPayload = {
        payment_id: val.payment_id || String(topicsRaw[2] || val[0] || ""),
        merchant_id: val.merchant_id,
        amount: val.amount !== undefined ? toBigInt(val.amount) : undefined,
      };
      return { ...base, type: "PAYMENT/EXPIRED", namespace: "PAYMENT", action: "EXPIRED", payload };
    }
    case "PAYMENT/PARTIALLY_PAID": {
      const payload: PaymentPartiallyPaidPayload = {
        payment_id: val.payment_id || String(topicsRaw[2] || val[0] || ""),
        merchant_id: val.merchant_id || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
        amount_received: toBigInt(val.amount_received ?? val[3]),
      };
      return { ...base, type: "PAYMENT/PARTIALLY_PAID", namespace: "PAYMENT", action: "PARTIALLY_PAID", payload };
    }
    case "PAYMENT/OVERPAID": {
      const payload: PaymentOverpaidPayload = {
        payment_id: val.payment_id || String(topicsRaw[2] || val[0] || ""),
        merchant_id: val.merchant_id || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
        amount_received: toBigInt(val.amount_received ?? val[3]),
      };
      return { ...base, type: "PAYMENT/OVERPAID", namespace: "PAYMENT", action: "OVERPAID", payload };
    }
    case "PAYMENT/FAILED": {
      const payload: PaymentFailedPayload = {
        payment_id: val.payment_id || String(topicsRaw[2] || val[0] || ""),
        merchant_id: val.merchant_id || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
        amount_received: toBigInt(val.amount_received ?? val[3]),
      };
      return { ...base, type: "PAYMENT/FAILED", namespace: "PAYMENT", action: "FAILED", payload };
    }

    // 2. REFUND
    case "REFUND/REQUESTED": {
      const payload: RefundRequestedPayload = {
        refund_id: val.refund_id || val[0] || "",
        payment_id: val.payment_id || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
        requester: val.requester || val[3] || "",
      };
      return { ...base, type: "REFUND/REQUESTED", namespace: "REFUND", action: "REQUESTED", payload };
    }
    case "REFUND/CREATED": {
      const payload: RefundCreatedPayload = {
        payment_id: val.payment_id || val[0] || "",
        refund_id: val.refund_id || val[1] || "",
        refund_amount: toBigInt(val.refund_amount ?? val.amount ?? val[2]),
      };
      return { ...base, type: "REFUND/CREATED", namespace: "REFUND", action: "CREATED", payload };
    }
    case "REFUND/PROCESSED": {
      const payload: RefundProcessedPayload = {
        refund_id: val.refund_id || val[0] || "",
        payment_id: val.payment_id || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
      };
      return { ...base, type: "REFUND/PROCESSED", namespace: "REFUND", action: "PROCESSED", payload };
    }
    case "REFUND/COMPLETED": {
      const payload: RefundCompletedPayload = {
        payment_id: val.payment_id || val[0] || "",
        refund_id: val.refund_id || val[1] || "",
        refund_amount: toBigInt(val.refund_amount ?? val.amount ?? val[2]),
      };
      return { ...base, type: "REFUND/COMPLETED", namespace: "REFUND", action: "COMPLETED", payload };
    }
    case "REFUND/REJECTED": {
      const payload: RefundRejectedPayload = {
        refund_id: val.refund_id || val[0] || "",
        payment_id: val.payment_id || val[1] || "",
        refund_amount: val.refund_amount !== undefined ? toBigInt(val.refund_amount) : undefined,
      };
      return { ...base, type: "REFUND/REJECTED", namespace: "REFUND", action: "REJECTED", payload };
    }

    // 3. DISPUTE
    case "DISPUTE/CREATED": {
      const payload: DisputeCreatedPayload = {
        payment_id: val.payment_id || val[0] || "",
        dispute_id: val.dispute_id || val[1] || "",
        amount: val.amount !== undefined ? toBigInt(val.amount) : undefined,
      };
      return { ...base, type: "DISPUTE/CREATED", namespace: "DISPUTE", action: "CREATED", payload };
    }
    case "DISPUTE/REVIEWED": {
      const payload: DisputeReviewedPayload = {
        payment_id: val.payment_id || val[0] || "",
        dispute_id: val.dispute_id || val[1] || "",
      };
      return { ...base, type: "DISPUTE/REVIEWED", namespace: "DISPUTE", action: "REVIEWED", payload };
    }
    case "DISPUTE/RESOLVED": {
      const payload: DisputeResolvedPayload = {
        payment_id: val.payment_id || val[0] || "",
        dispute_id: val.dispute_id || val[1] || "",
        ruling: val.ruling || val[2] || undefined,
      };
      return { ...base, type: "DISPUTE/RESOLVED", namespace: "DISPUTE", action: "RESOLVED", payload };
    }
    case "DISPUTE/REJECTED": {
      const payload: DisputeRejectedPayload = {
        payment_id: val.payment_id || val[0] || "",
        dispute_id: val.dispute_id || val[1] || "",
      };
      return { ...base, type: "DISPUTE/REJECTED", namespace: "DISPUTE", action: "REJECTED", payload };
    }
    case "DISPUTE/ESCALATED": {
      const payload: DisputeEscalatedPayload = {
        payment_id: val.payment_id || val[0] || "",
        dispute_id: val.dispute_id || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
      };
      return { ...base, type: "DISPUTE/ESCALATED", namespace: "DISPUTE", action: "ESCALATED", payload };
    }
    case "DISPUTE/BOND_RETURNED": {
      const payload: DisputeBondReturnedPayload = {
        dispute_id: val.dispute_id || val[0] || "",
        recipient: val.recipient || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
      };
      return { ...base, type: "DISPUTE/BOND_RETURNED", namespace: "DISPUTE", action: "BOND_RETURNED", payload };
    }
    case "DISPUTE/BOND_FORFEITED": {
      const payload: DisputeBondForfeitedPayload = {
        dispute_id: val.dispute_id || val[0] || "",
        recipient: val.recipient || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
      };
      return { ...base, type: "DISPUTE/BOND_FORFEITED", namespace: "DISPUTE", action: "BOND_FORFEITED", payload };
    }

    // 4. MERCHANT
    case "MERCHANT/REGISTERED": {
      const payload: MerchantRegisteredPayload = {
        merchant_id: val.merchant_id || val[0] || "",
        business_name: val.business_name || val[1],
        settlement_currency: val.settlement_currency || val[2],
      };
      return { ...base, type: "MERCHANT/REGISTERED", namespace: "MERCHANT", action: "REGISTERED", payload };
    }
    case "MERCHANT/UPDATED": {
      const payload: MerchantUpdatedPayload = {
        merchant_id: val.merchant_id || val[0] || "",
      };
      return { ...base, type: "MERCHANT/UPDATED", namespace: "MERCHANT", action: "UPDATED", payload };
    }
    case "MERCHANT/VERIFIED": {
      const payload: MerchantVerifiedPayload = {
        merchant_id: val.merchant_id || val[0] || "",
      };
      return { ...base, type: "MERCHANT/VERIFIED", namespace: "MERCHANT", action: "VERIFIED", payload };
    }
    case "MERCHANT/SUSPENDED": {
      const payload: MerchantSuspendedPayload = {
        merchant_id: val.merchant_id || val[0] || "",
        reason: val.reason || val[1],
      };
      return { ...base, type: "MERCHANT/SUSPENDED", namespace: "MERCHANT", action: "SUSPENDED", payload };
    }
    case "MERCHANT/REINSTATED": {
      const payload: MerchantReinstatedPayload = {
        merchant_id: val.merchant_id || val[0] || "",
        reinstated_by: val.reinstated_by || val[1],
      };
      return { ...base, type: "MERCHANT/REINSTATED", namespace: "MERCHANT", action: "REINSTATED", payload };
    }
    case "KYC/TIER_UPGRADED": {
      const payload: KycTierUpgradedPayload = {
        merchant_id: val.merchant_id || val[0] || "",
        old_tier: val.old_tier || val[1] || "",
        new_tier: val.new_tier || val[2] || "",
      };
      return { ...base, type: "KYC/TIER_UPGRADED", namespace: "KYC", action: "TIER_UPGRADED", payload };
    }

    // 5. LINK
    case "LINK/CREATED": {
      const payload: LinkCreatedPayload = {
        link_id: val.link_id || val[0] || "",
        merchant_id: val.merchant_id || val[1] || "",
      };
      return { ...base, type: "LINK/CREATED", namespace: "LINK", action: "CREATED", payload };
    }
    case "LINK/USED": {
      const payload: LinkUsedPayload = {
        link_id: val.link_id || val[0] || "",
        payer: val.payer || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
        payment_id: val.payment_id || val[3] || "",
        metadata: val.metadata ?? null,
      };
      return { ...base, type: "LINK/USED", namespace: "LINK", action: "USED", payload };
    }
    case "LINK/DEACTIVATED": {
      const payload: LinkDeactivatedPayload = {
        link_id: val.link_id || val[0] || "",
      };
      return { ...base, type: "LINK/DEACTIVATED", namespace: "LINK", action: "DEACTIVATED", payload };
    }
    case "LINK/EXPIRED": {
      const payload: LinkExpiredPayload = {
        link_id: val.link_id || val[0] || "",
      };
      return { ...base, type: "LINK/EXPIRED", namespace: "LINK", action: "EXPIRED", payload };
    }
    case "LINK/VIEWED": {
      const payload: LinkViewedPayload = {
        link_id: val.link_id || val[0] || "",
      };
      return { ...base, type: "LINK/VIEWED", namespace: "LINK", action: "VIEWED", payload };
    }

    // 6. SUBSCRIPTION
    case "SUBSCRIPTION/CREATED": {
      const payload: SubscriptionCreatedPayload = {
        subscription_id: val.subscription_id || val[0] || "",
        payer: val.payer || val[1] || "",
        merchant_id: val.merchant_id,
        plan_id: val.plan_id,
        amount: val.amount !== undefined ? toBigInt(val.amount) : undefined,
      };
      return { ...base, type: "SUBSCRIPTION/CREATED", namespace: "SUBSCRIPTION", action: "CREATED", payload };
    }
    case "SUBSCRIPTION/CHARGED": {
      const payload: SubscriptionChargedPayload = {
        subscription_id: val.subscription_id || val[0] || "",
        payer: val.payer || val[1] || "",
        merchant_id: val.merchant_id || val[2] || "",
        amount: toBigInt(val.amount ?? val[3]),
        total_payments: Number(val.total_payments ?? val[4] ?? 1),
      };
      return { ...base, type: "SUBSCRIPTION/CHARGED", namespace: "SUBSCRIPTION", action: "CHARGED", payload };
    }
    case "SUBSCRIPTION/CANCELLED": {
      const payload: SubscriptionCancelledPayload = {
        subscription_id: val.subscription_id || val[0] || "",
        payer: val.payer,
        cancelled_by: val.cancelled_by || val[1],
      };
      return { ...base, type: "SUBSCRIPTION/CANCELLED", namespace: "SUBSCRIPTION", action: "CANCELLED", payload };
    }
    case "SUBSCRIPTION/EXPIRED": {
      const payload: SubscriptionExpiredPayload = {
        subscription_id: val.subscription_id || val[0] || "",
        payer: val.payer || val[1] || "",
      };
      return { ...base, type: "SUBSCRIPTION/EXPIRED", namespace: "SUBSCRIPTION", action: "EXPIRED", payload };
    }

    // 7. STREAM
    case "STREAM/CREATED": {
      const payload: StreamCreatedPayload = {
        stream_id: val.stream_id || val[0] || "",
        sender: val.sender || val[1] || "",
        receiver: val.receiver || val[2],
        deposit: val.deposit !== undefined ? toBigInt(val.deposit) : undefined,
        amount: val.amount !== undefined ? toBigInt(val.amount) : undefined,
      };
      return { ...base, type: "STREAM/CREATED", namespace: "STREAM", action: "CREATED", payload };
    }
    case "STREAM/TOPPED_UP": {
      const payload: StreamToppedUpPayload = {
        stream_id: val.stream_id || val[0] || "",
        sender: val.sender || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
      };
      return { ...base, type: "STREAM/TOPPED_UP", namespace: "STREAM", action: "TOPPED_UP", payload };
    }
    case "STREAM/WITHDRAWN": {
      const payload: StreamWithdrawnPayload = {
        stream_id: val.stream_id || String(topicsRaw[2] || val[0] || ""),
        receiver: val.receiver || val[1],
        destination: val.destination || val[2],
        amount: toBigInt(val.amount ?? val[3]),
        remaining: val.remaining !== undefined ? toBigInt(val.remaining) : undefined,
        remaining_deposit: val.remaining_deposit !== undefined ? toBigInt(val.remaining_deposit) : undefined,
        memo: val.memo ?? null,
      };
      return { ...base, type: "STREAM/WITHDRAWN", namespace: "STREAM", action: "WITHDRAWN", payload };
    }
    case "STREAM/CANCELLED": {
      const payload: StreamCancelledPayload = {
        stream_id: val.stream_id || val[0] || "",
        sender: val.sender || val[1] || "",
        accrued: toBigInt(val.accrued ?? val[2]),
        refund: toBigInt(val.refund ?? val[3]),
      };
      return { ...base, type: "STREAM/CANCELLED", namespace: "STREAM", action: "CANCELLED", payload };
    }
    case "STREAM/PAUSED": {
      const payload: StreamPausedPayload = {
        stream_id: val.stream_id || val[0] || "",
        sender: val.sender || val[1] || "",
      };
      return { ...base, type: "STREAM/PAUSED", namespace: "STREAM", action: "PAUSED", payload };
    }
    case "STREAM/RESUMED": {
      const payload: StreamResumedPayload = {
        stream_id: val.stream_id || val[0] || "",
        sender: val.sender || val[1] || "",
      };
      return { ...base, type: "STREAM/RESUMED", namespace: "STREAM", action: "RESUMED", payload };
    }
    case "STREAM/RATE_UPDATED": {
      const payload: StreamRateUpdatedPayload = {
        stream_id: val.stream_id || val[0] || "",
        sender: val.sender || val[1] || "",
        old_rate: toBigInt(val.old_rate ?? val[2]),
        new_rate: toBigInt(val.new_rate ?? val[3]),
        surplus: toBigInt(val.surplus ?? val[4]),
      };
      return { ...base, type: "STREAM/RATE_UPDATED", namespace: "STREAM", action: "RATE_UPDATED", payload };
    }
    case "STREAM/RATE_DECREASED": {
      const payload: StreamRateDecreasedPayload = {
        stream_id: val.stream_id || val[0] || "",
        sender: val.sender || val[1] || "",
        old_rate: toBigInt(val.old_rate ?? val[2]),
        new_rate: toBigInt(val.new_rate ?? val[3]),
        surplus: toBigInt(val.surplus ?? val[4]),
      };
      return { ...base, type: "STREAM/RATE_DECREASED", namespace: "STREAM", action: "RATE_DECREASED", payload };
    }
    case "STREAM/MILESTONE_APPROVED": {
      const payload: StreamMilestoneApprovedPayload = {
        stream_id: val.stream_id || val[0] || "",
        sender: val.sender || val[1] || "",
      };
      return { ...base, type: "STREAM/MILESTONE_APPROVED", namespace: "STREAM", action: "MILESTONE_APPROVED", payload };
    }
    case "STREAM/DESTINATION_SET": {
      const payload: StreamDestinationSetPayload = {
        stream_id: val.stream_id || val[0] || "",
        recipient: val.recipient || val[1] || "",
        destination: val.destination || val[2] || "",
      };
      return { ...base, type: "STREAM/DESTINATION_SET", namespace: "STREAM", action: "DESTINATION_SET", payload };
    }
    case "STREAM/CLOSED": {
      const payload: StreamClosedPayload = {
        stream_id: val.stream_id || val[0] || "",
        sender: val.sender || val[1] || "",
        receiver: val.receiver || val[2] || "",
        residual: toBigInt(val.residual ?? val[3]),
      };
      return { ...base, type: "STREAM/CLOSED", namespace: "STREAM", action: "CLOSED", payload };
    }

    // 8. RATE (FX Oracle)
    case "RATE/UPDATED": {
      const payload: RateUpdatedPayload = {
        pair: val.pair || val[0] || "",
        rate: toBigInt(val.rate ?? val[1]),
        timestamp: BigInt(val.timestamp ?? val[2] ?? 0),
      };
      return { ...base, type: "RATE/UPDATED", namespace: "RATE", action: "UPDATED", payload };
    }

    // 9. ACCESS_CONTROL
    case "ACCESS_CONTROL/ROLE_GRANTED": {
      const payload: RoleGrantedPayload = {
        role: val.role || val[0] || "",
        account: val.account || val[1] || "",
        granted_by: val.granted_by || val[2],
        timestamp: val.timestamp !== undefined ? BigInt(val.timestamp) : undefined,
      };
      return { ...base, type: "ACCESS_CONTROL/ROLE_GRANTED", namespace: "ACCESS_CONTROL", action: "ROLE_GRANTED", payload };
    }
    case "ACCESS_CONTROL/ROLE_REVOKED": {
      const payload: RoleRevokedPayload = {
        role: val.role || val[0] || "",
        account: val.account || val[1] || "",
        revoked_by: val.revoked_by || val[2],
        timestamp: val.timestamp !== undefined ? BigInt(val.timestamp) : undefined,
      };
      return { ...base, type: "ACCESS_CONTROL/ROLE_REVOKED", namespace: "ACCESS_CONTROL", action: "ROLE_REVOKED", payload };
    }
    case "ACCESS_CONTROL/ADMIN_TRANSFER_PROPOSED": {
      const payload: AdminTransferProposedPayload = {
        new_admin: val.new_admin || val[0] || "",
        earliest_acceptance_ledger: Number(val.earliest_acceptance_ledger ?? val[1] ?? 0),
      };
      return {
        ...base,
        type: "ACCESS_CONTROL/ADMIN_TRANSFER_PROPOSED",
        namespace: "ACCESS_CONTROL",
        action: "ADMIN_TRANSFER_PROPOSED",
        payload,
      };
    }
    case "ACCESS_CONTROL/ADMIN_TRANSFER_COMPLETED": {
      const payload: AdminTransferCompletedPayload = {
        old_admin: val.old_admin || val[0] || "",
        new_admin: val.new_admin || val[1] || "",
        timestamp: val.timestamp !== undefined ? BigInt(val.timestamp) : undefined,
      };
      return {
        ...base,
        type: "ACCESS_CONTROL/ADMIN_TRANSFER_COMPLETED",
        namespace: "ACCESS_CONTROL",
        action: "ADMIN_TRANSFER_COMPLETED",
        payload,
      };
    }
    case "ACCESS_CONTROL/ADMIN_TRANSFER_CANCELLED": {
      const payload: AdminTransferCancelledPayload = {
        cancelled_by: val.cancelled_by || val[0] || "",
        timestamp: val.timestamp !== undefined ? BigInt(val.timestamp) : undefined,
      };
      return {
        ...base,
        type: "ACCESS_CONTROL/ADMIN_TRANSFER_CANCELLED",
        namespace: "ACCESS_CONTROL",
        action: "ADMIN_TRANSFER_CANCELLED",
        payload,
      };
    }

    // 10. FEE_SPLIT
    case "FEE_SPLIT/UPDATED": {
      const payload: FeeSplitUpdatedPayload = {
        flat_fee: toBigInt(val.flat_fee ?? val[0]),
        bps: Number(val.bps ?? val[1] ?? 0),
      };
      return { ...base, type: "FEE_SPLIT/UPDATED", namespace: "FEE_SPLIT", action: "UPDATED", payload };
    }

    // 11. TREASURY
    case "TREASURY/WITHDRAWN": {
      const payload: TreasuryWithdrawnPayload = {
        admin: val.admin || val[0] || "",
        amount: toBigInt(val.amount ?? val[1]),
      };
      return { ...base, type: "TREASURY/WITHDRAWN", namespace: "TREASURY", action: "WITHDRAWN", payload };
    }

    // 12. CONTRACT
    case "CONTRACT/UPGRADED": {
      const payload: ContractUpgradedPayload = {
        old_version: val.old_version || val[0] || "",
        new_version: val.new_version || val[1] || "",
      };
      return { ...base, type: "CONTRACT/UPGRADED", namespace: "CONTRACT", action: "UPGRADED", payload };
    }

    // 13. INVOICE
    case "INVOICE/CREATED": {
      const payload: InvoiceCreatedPayload = {
        invoice_id: val.invoice_id || val[0] || "",
        merchant_id: val.merchant_id || val[1] || "",
        amount: toBigInt(val.amount ?? val[2]),
      };
      return { ...base, type: "INVOICE/CREATED", namespace: "INVOICE", action: "CREATED", payload };
    }
    case "INVOICE/PAID": {
      const payload: InvoicePaidPayload = {
        invoice_id: val.invoice_id || val[0] || "",
        merchant_id: val.merchant_id || val[1] || "",
      };
      return { ...base, type: "INVOICE/PAID", namespace: "INVOICE", action: "PAID", payload };
    }
    case "INVOICE/OVERDUE": {
      const payload: InvoiceOverduePayload = {
        invoice_id: val.invoice_id || val[0] || "",
        merchant_id: val.merchant_id || val[1] || "",
      };
      return { ...base, type: "INVOICE/OVERDUE", namespace: "INVOICE", action: "OVERDUE", payload };
    }

    // 14. SWAP
    case "SWAP/EXECUTED": {
      const payload: SwapExecutedPayload = {
        payment_id: val.payment_id || val[0] || "",
        sender: val.sender || val[1] || "",
        amount_in: toBigInt(val.amount_in ?? val[2]),
        amount_out: toBigInt(val.amount_out ?? val[3]),
      };
      return { ...base, type: "SWAP/EXECUTED", namespace: "SWAP", action: "EXECUTED", payload };
    }

    default: {
      return {
        ...base,
        type: typeKey,
        namespace,
        action,
        payload: typeof val === "object" && val !== null ? val : { raw: val },
      };
    }
  }
}
