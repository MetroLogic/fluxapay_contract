import * as React from "react";
import { FluxapayError } from "@fluxapay/sdk";
import type {
  PaymentCharge,
  PaymentStatus,
  Merchant,
  Refund,
  CreatePaymentParams,
  SubscriptionPlan,
  Invoice,
  LineItem,
  InvoiceStatus,
  CreateInvoiceParams,
  PaymentLink,
  Dispute,
  DisputeStatus,
  MerchantAnalytics,
  PaymentStream,
  CreateStreamParams,
} from "@fluxapay/sdk";
import { useFluxapayClient } from "./FluxapayProvider.js";
import { useAsync, type AsyncState } from "./useAsync.js";

export type { SubscriptionPlan, PaymentStream, CreateStreamParams } from "@fluxapay/sdk";
export type { Invoice, LineItem, InvoiceStatus, Dispute, DisputeStatus, MerchantAnalytics };

export interface CreateDisputeParams {
  paymentId: string;
  amount: bigint;
  reason: string;
  evidence: string;
  disputer: string;
}

function toFluxapayError(error: unknown): FluxapayError {
  if (error instanceof FluxapayError) {
    return error;
  }

  if (error instanceof Error) {
    return new FluxapayError(0, "UnknownFluxapayError", error.message, error);
  }

  return new FluxapayError(0, "UnknownFluxapayError", String(error), error);
}

/** Fetch a single payment by id. Re-fetches whenever `paymentId` changes. */
export function usePayment(paymentId: string | undefined): AsyncState<PaymentCharge, FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getPayment(paymentId as string) as unknown as Promise<PaymentCharge>,
    [paymentId],
    !!paymentId,
  );
}

/** Fetch a single merchant by id. Re-fetches whenever `merchantId` changes. */
export function useMerchant(merchantId: string | undefined): AsyncState<Merchant, FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getMerchant(merchantId as string) as unknown as Promise<Merchant>,
    [merchantId],
    !!merchantId,
  );
}

export interface UseMerchantPaymentsOptions {
  offset?: number;
  limit?: number;
  statusFilter?: PaymentStatus;
}

export function useMerchantPayments(
  merchantId: string | undefined,
  options?: UseMerchantPaymentsOptions,
): AsyncState<PaymentCharge[], FluxapayError> {
  const client = useFluxapayClient();
  const offset = options?.offset ?? 0;
  const limit = options?.limit ?? 20;
  const statusFilter = options?.statusFilter;

  return useAsync(
    async () => {
      const idsTx = await client.contract.get_merchant_payments_paginated({
        merchant_id: merchantId as string,
        offset,
        limit,
        status_filter: statusFilter ?? null,
      });
      const ids = (idsTx as unknown as { result: string[] }).result;
      const payments = await Promise.all(ids.map((id) => client.getPayment(id)));
      return payments as unknown as PaymentCharge[];
    },
    [merchantId, offset, limit, statusFilter],
    !!merchantId,
  );
}

export interface UseMerchantLinksOptions {
  offset?: number;
  limit?: number;
  activeOnly?: boolean;
}

export function useMerchantLinks(
  merchantId: string | undefined,
  options?: UseMerchantLinksOptions,
): AsyncState<PaymentLink[], FluxapayError> {
  const client = useFluxapayClient();
  const offset = options?.offset ?? 0;
  const limit = options?.limit ?? 100;
  const activeOnly = options?.activeOnly ?? false;

  return useAsync(
    () =>
      client.getMerchantLinks(merchantId as string, {
        offset,
        limit,
        activeOnly,
      }) as unknown as Promise<PaymentLink[]>,
    [merchantId, offset, limit, activeOnly],
    !!merchantId,
  );
}

/** Fetch a single refund by id. Re-fetches whenever `refundId` changes. */
export function useRefund(refundId: string | undefined): AsyncState<Refund, FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getRefund(refundId as string) as unknown as Promise<Refund>,
    [refundId],
    !!refundId,
  );
}

/** Fetch a single dispute by id. Re-fetches whenever `disputeId` changes. */
export function useDispute(disputeId: string | undefined): AsyncState<Dispute, FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getDispute(disputeId as string) as unknown as Promise<Dispute>,
    [disputeId],
    !!disputeId,
  );
}

/** Fetch all disputes for a payment. Re-fetches when `paymentId` changes. */
export function usePaymentDisputes(paymentId: string | undefined): AsyncState<Dispute[], FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getPaymentDisputes(paymentId as string) as unknown as Promise<Dispute[]>,
    [paymentId],
    !!paymentId,
  );
}

/** Fetch merchant analytics over a timestamp range. Re-fetches when any input changes. */
export function useMerchantAnalytics(
  merchantId: string | undefined,
  from: number,
  to: number,
): AsyncState<MerchantAnalytics, FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getMerchantAnalytics(merchantId as string, from, to) as Promise<MerchantAnalytics>,
    [merchantId, from, to],
    !!merchantId,
  );
}

/** Fetch a single payment stream by id. Re-fetches whenever `streamId` changes. */
export function useStream(streamId: string | undefined): AsyncState<PaymentStream, FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getStream(streamId as string) as unknown as Promise<PaymentStream>,
    [streamId],
    !!streamId,
  );
}

/** Fetch streams created by a sender, paginated by offset/limit. */
export function useSenderStreams(
  sender: string | undefined,
  offset = 0,
  limit = 100,
): AsyncState<PaymentStream[], FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () =>
      client.getSenderStreams(sender as string, offset, limit) as unknown as Promise<PaymentStream[]>,
    [sender, offset, limit],
    !!sender,
  );
}

export type MutationStatus = "idle" | "loading" | "success" | "error";

export interface UseCreatePaymentResult {
  mutate: (params: CreatePaymentParams) => Promise<PaymentCharge>;
  data: PaymentCharge | undefined;
  status: MutationStatus;
  loading: boolean;
  error: FluxapayError | undefined;
}

export function useCreatePayment(): UseCreatePaymentResult {
  const client = useFluxapayClient();
  const [data, setData] = React.useState<PaymentCharge | undefined>(undefined);
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<FluxapayError | undefined>(undefined);

  const mutate = React.useCallback(
    async (params: CreatePaymentParams) => {
      setStatus("loading");
      setError(undefined);
      try {
        const payment = (await client.createPayment(params)) as unknown as PaymentCharge;
        setData(payment);
        setStatus("success");
        return payment;
      } catch (err) {
        const normalized = toFluxapayError(err);
        setError(normalized);
        setStatus("error");
        throw normalized;
      }
    },
    [client],
  );

  return { mutate, data, status, loading: status === "loading", error };
}

export interface UseCreateStreamResult {
  mutate: (params: CreateStreamParams) => Promise<PaymentStream>;
  data: PaymentStream | undefined;
  status: MutationStatus;
  loading: boolean;
  error: FluxapayError | undefined;
}

export function useCreateStream(): UseCreateStreamResult {
  const client = useFluxapayClient();
  const [data, setData] = React.useState<PaymentStream | undefined>(undefined);
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<FluxapayError | undefined>(undefined);

  const mutate = React.useCallback(
    async (params: CreateStreamParams) => {
      setStatus("loading");
      setError(undefined);
      try {
        const stream = (await client.createStream(params)) as unknown as PaymentStream;
        setData(stream);
        setStatus("success");
        return stream;
      } catch (err) {
        const normalized = toFluxapayError(err);
        setError(normalized);
        setStatus("error");
        throw normalized;
      }
    },
    [client],
  );

  return { mutate, data, status, loading: status === "loading", error };
}

export interface UseCreateDisputeResult {
  mutate: (params: CreateDisputeParams) => Promise<string>;
  data: string | undefined;
  status: MutationStatus;
  loading: boolean;
  error: FluxapayError | undefined;
}

export function useCreateDispute(): UseCreateDisputeResult {
  const client = useFluxapayClient();
  const [data, setData] = React.useState<string | undefined>(undefined);
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<FluxapayError | undefined>(undefined);

  const mutate = React.useCallback(
    async (params: CreateDisputeParams) => {
      setStatus("loading");
      setError(undefined);
      try {
        const result = await client.createDispute(params);
        const disputeId =
          typeof result === "string"
            ? result
            : ((result as { dispute_id?: string } | null)?.dispute_id ??
              (result as { disputeId?: string } | null)?.disputeId ??
              (result as { id?: string } | null)?.id ??
              "");
        setData(disputeId);
        setStatus("success");
        return disputeId;
      } catch (err) {
        const normalized = toFluxapayError(err);
        setError(normalized);
        setStatus("error");
        throw normalized;
      }
    },
    [client],
  );

  return { mutate, data, status, loading: status === "loading", error };
}

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
export function useSubscriptionPlan(planId: string | undefined): AsyncState<SubscriptionPlan, FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getSubscriptionPlan(planId as string) as unknown as Promise<SubscriptionPlan>,
    [planId],
    !!planId,
  );
}

/** Create a subscription plan. Returns a mutate function and the current status. */
export function useCreateSubscriptionPlan(): UseCreateSubscriptionPlanResult {
  const client = useFluxapayClient();
  const [data, setData] = React.useState<void | undefined>(undefined);
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<FluxapayError | undefined>(undefined);

  const mutate = React.useCallback(
    async (params: UseCreateSubscriptionPlanParams) => {
      setStatus("loading");
      setError(undefined);
      try {
        await client.createSubscriptionPlan(params);
        setData(undefined);
        setStatus("success");
      } catch (err) {
        const normalized = toFluxapayError(err);
        setError(normalized);
        setStatus("error");
        throw normalized;
      }
    },
    [client],
  );

  return { mutate, data, status, loading: status === "loading", error };
}

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

export function useSubscribeToPlan(): UseSubscribeToPlanResult {
  const client = useFluxapayClient();
  const [data, setData] = React.useState<void | undefined>(undefined);
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<FluxapayError | undefined>(undefined);

  const mutate = React.useCallback(
    async (params: UseSubscribeToPlanParams) => {
      setStatus("loading");
      setError(undefined);
      try {
        await client.subscribeToPlan(params);
        setData(undefined);
        setStatus("success");
      } catch (err) {
        const normalized = toFluxapayError(err);
        setError(normalized);
        setStatus("error");
        throw normalized;
      }
    },
    [client],
  );

  return { mutate, data, status, loading: status === "loading", error };
}

/** Fetch a single invoice by id. Re-fetches whenever `invoiceId` changes. */
export function useInvoice(invoiceId: string | undefined): AsyncState<Invoice, FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getInvoice(invoiceId as string),
    [invoiceId],
    !!invoiceId,
  );
}

/** Fetch the list of invoice ids for a merchant. Re-fetches whenever `merchantId` changes. */
export function useMerchantInvoices(merchantId: string | undefined): AsyncState<string[], FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getMerchantInvoices(merchantId as string),
    [merchantId],
    !!merchantId,
  );
}

export interface UseCreateInvoiceResult {
  mutate: (params: CreateInvoiceParams) => Promise<Invoice>;
  data: Invoice | undefined;
  status: MutationStatus;
  loading: boolean;
  error: FluxapayError | undefined;
}

/** Create an invoice. Returns a mutate function and the current mutation status. */
export function useCreateInvoice(): UseCreateInvoiceResult {
  const client = useFluxapayClient();
  const [data, setData] = React.useState<Invoice | undefined>(undefined);
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<FluxapayError | undefined>(undefined);

  const mutate = React.useCallback(
    async (params: CreateInvoiceParams) => {
      setStatus("loading");
      setError(undefined);
      try {
        const invoice = await client.createInvoice(params);
        setData(invoice);
        setStatus("success");
        return invoice;
      } catch (err) {
        const normalized = toFluxapayError(err);
        setError(normalized);
        setStatus("error");
        throw normalized;
      }
    },
    [client],
  );

  return { mutate, data, status, loading: status === "loading", error };
}

export interface UseMarkInvoicePaidResult {
  mutate: (invoiceId: string) => Promise<void>;
  status: MutationStatus;
  loading: boolean;
  error: FluxapayError | undefined;
}

/** Mark an invoice as paid. Returns a mutate function and the current mutation status. */
export function useMarkInvoicePaid(): UseMarkInvoicePaidResult {
  const client = useFluxapayClient();
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<FluxapayError | undefined>(undefined);

  const mutate = React.useCallback(
    async (invoiceId: string) => {
      setStatus("loading");
      setError(undefined);
      try {
        await client.markInvoicePaid(invoiceId);
        setStatus("success");
      } catch (err) {
        const normalized = toFluxapayError(err);
        setError(normalized);
        setStatus("error");
        throw normalized;
      }
    },
    [client],
  );

  return { mutate, status, loading: status === "loading", error };
}

