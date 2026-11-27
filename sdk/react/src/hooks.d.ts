import type { PaymentCharge, PaymentStatus, Merchant, Refund, CreatePaymentParams, SubscriptionPlan, Invoice, LineItem, InvoiceStatus, CreateInvoiceParams, PaymentLink, Dispute, DisputeStatus, MerchantAnalytics, FluxapayError } from "@fluxapay/sdk";
import { type AsyncState } from "./useAsync.js";
export type { SubscriptionPlan } from "@fluxapay/sdk";
export type { Invoice, LineItem, InvoiceStatus, Dispute, DisputeStatus, MerchantAnalytics };
export interface CreateDisputeParams {
    paymentId: string;
    amount: bigint;
    reason: string;
    evidence: string;
    disputer: string;
}
/** Fetch a single payment by id. Re-fetches whenever `paymentId` changes. */
export declare function usePayment(paymentId: string | undefined): AsyncState<PaymentCharge, FluxapayError>;
/** Fetch a single merchant by id. Re-fetches whenever `merchantId` changes. */
export declare function useMerchant(merchantId: string | undefined): AsyncState<Merchant, FluxapayError>;
export interface UseMerchantPaymentsOptions {
    offset?: number;
    limit?: number;
    statusFilter?: PaymentStatus;
}
export declare function useMerchantPayments(merchantId: string | undefined, options?: UseMerchantPaymentsOptions): AsyncState<PaymentCharge[], FluxapayError>;
export interface UseMerchantLinksOptions {
    offset?: number;
    limit?: number;
    activeOnly?: boolean;
}
export declare function useMerchantLinks(merchantId: string | undefined, options?: UseMerchantLinksOptions): AsyncState<PaymentLink[], FluxapayError>;
/** Fetch a single refund by id. Re-fetches whenever `refundId` changes. */
export declare function useRefund(refundId: string | undefined): AsyncState<Refund, FluxapayError>;
/** Fetch a single dispute by id. Re-fetches whenever `disputeId` changes. */
export declare function useDispute(disputeId: string | undefined): AsyncState<Dispute, FluxapayError>;
/** Fetch all disputes for a payment. Re-fetches when `paymentId` changes. */
export declare function usePaymentDisputes(paymentId: string | undefined): AsyncState<Dispute[], FluxapayError>;
/** Fetch merchant analytics over a timestamp range. Re-fetches when any input changes. */
export declare function useMerchantAnalytics(merchantId: string | undefined, from: number, to: number): AsyncState<MerchantAnalytics, FluxapayError>;
export type MutationStatus = "idle" | "loading" | "success" | "error";
export interface UseCreatePaymentResult {
    mutate: (params: CreatePaymentParams) => Promise<PaymentCharge>;
    data: PaymentCharge | undefined;
    status: MutationStatus;
    loading: boolean;
    error: FluxapayError | undefined;
}
export declare function useCreatePayment(): UseCreatePaymentResult;
export interface UseCreateDisputeResult {
    mutate: (params: CreateDisputeParams) => Promise<string>;
    data: string | undefined;
    status: MutationStatus;
    loading: boolean;
    error: FluxapayError | undefined;
}
export declare function useCreateDispute(): UseCreateDisputeResult;
export interface UseCreateSubscriptionPlanParams {
    merchant: string;
    planId: string;
    name: string;
    description: string;
    amount: bigint;
    currency: string;
    billingInterval: "Daily" | "Weekly" | "Monthly" | "Annually";
}
export interface UseCreateSubscriptionPlanResult {
    mutate: (params: UseCreateSubscriptionPlanParams) => Promise<void>;
    data: void;
    status: MutationStatus;
    loading: boolean;
    error: FluxapayError | undefined;
}
/** Fetch a single subscription plan by ID. Re-fetches whenever `planId` changes. */
export declare function useSubscriptionPlan(planId: string | undefined): AsyncState<SubscriptionPlan, FluxapayError>;
/** Create a subscription plan. Returns a mutate function and the current status. */
export declare function useCreateSubscriptionPlan(): UseCreateSubscriptionPlanResult;
export interface UseSubscribeToPlanParams {
    payer: string;
    planId: string;
    paymentId: string;
}
export interface UseSubscribeToPlanResult {
    mutate: (params: UseSubscribeToPlanParams) => Promise<void>;
    data: void;
    status: MutationStatus;
    loading: boolean;
    error: FluxapayError | undefined;
}
export declare function useSubscribeToPlan(): UseSubscribeToPlanResult;
/** Fetch a single invoice by id. Re-fetches whenever `invoiceId` changes. */
export declare function useInvoice(invoiceId: string | undefined): AsyncState<Invoice, FluxapayError>;
/** Fetch the list of invoice ids for a merchant. Re-fetches whenever `merchantId` changes. */
export declare function useMerchantInvoices(merchantId: string | undefined): AsyncState<string[], FluxapayError>;
export interface UseCreateInvoiceResult {
    mutate: (params: CreateInvoiceParams) => Promise<Invoice>;
    data: Invoice | undefined;
    status: MutationStatus;
    loading: boolean;
    error: FluxapayError | undefined;
}
/** Create an invoice. Returns a mutate function and the current mutation status. */
export declare function useCreateInvoice(): UseCreateInvoiceResult;
export interface UseMarkInvoicePaidResult {
    mutate: (invoiceId: string) => Promise<void>;
    status: MutationStatus;
    loading: boolean;
    error: FluxapayError | undefined;
}
/** Mark an invoice as paid. Returns a mutate function and the current mutation status. */
export declare function useMarkInvoicePaid(): UseMarkInvoicePaidResult;
