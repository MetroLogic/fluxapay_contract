import * as React from "react";
import type {
  PaymentCharge,
  Merchant,
  Refund,
  CreatePaymentParams,
  Invoice,
  LineItem,
  InvoiceStatus,
  CreateInvoiceParams,
} from "@fluxapay/sdk";
import { useFluxapayClient } from "./FluxapayProvider.js";
import { useAsync, type AsyncState } from "./useAsync.js";

export type { Invoice, LineItem, InvoiceStatus };

/** Fetch a single payment by id. Re-fetches whenever `paymentId` changes. */
export function usePayment(paymentId: string | undefined): AsyncState<PaymentCharge> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getPayment(paymentId as string) as unknown as Promise<PaymentCharge>,
    [paymentId],
    !!paymentId,
  );
}

/** Fetch a single merchant by id. Re-fetches whenever `merchantId` changes. */
export function useMerchant(merchantId: string | undefined): AsyncState<Merchant> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getMerchant(merchantId as string) as unknown as Promise<Merchant>,
    [merchantId],
    !!merchantId,
  );
}

export interface UseMerchantPaymentsOptions {
  /** Number of payments to skip. Defaults to 0. */
  offset?: number;
  /** Max number of payments to return. Defaults to 20. */
  limit?: number;
}

/**
 * Fetch the paginated list of payments for a merchant, resolving each
 * payment id returned by the contract into its full `PaymentCharge` record.
 */
export function useMerchantPayments(
  merchantId: string | undefined,
  options?: UseMerchantPaymentsOptions,
): AsyncState<PaymentCharge[]> {
  const client = useFluxapayClient();
  const offset = options?.offset ?? 0;
  const limit = options?.limit ?? 20;

  return useAsync(
    async () => {
      const idsTx = await client.contract.get_merchant_payments_paginated({
        merchant_id: merchantId as string,
        offset,
        limit,
      });
      const ids = (idsTx as unknown as { result: string[] }).result;
      const payments = await Promise.all(ids.map((id) => client.getPayment(id)));
      return payments as unknown as PaymentCharge[];
    },
    [merchantId, offset, limit],
    !!merchantId,
  );
}

/** Fetch a single refund by id. Re-fetches whenever `refundId` changes. */
export function useRefund(refundId: string | undefined): AsyncState<Refund> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getRefund(refundId as string) as unknown as Promise<Refund>,
    [refundId],
    !!refundId,
  );
}

export type MutationStatus = "idle" | "loading" | "success" | "error";

export interface UseCreatePaymentResult {
  mutate: (params: CreatePaymentParams) => Promise<PaymentCharge>;
  data: PaymentCharge | undefined;
  status: MutationStatus;
  loading: boolean;
  error: Error | undefined;
}

/** Create a payment. Returns a `mutate` function and the current transaction status. */
export function useCreatePayment(): UseCreatePaymentResult {
  const client = useFluxapayClient();
  const [data, setData] = React.useState<PaymentCharge | undefined>(undefined);
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<Error | undefined>(undefined);

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
        const normalized = err instanceof Error ? err : new Error(String(err));
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
export function useInvoice(invoiceId: string | undefined): AsyncState<Invoice> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getInvoice(invoiceId as string),
    [invoiceId],
    !!invoiceId,
  );
}

/** Fetch the list of invoice ids for a merchant. Re-fetches whenever `merchantId` changes. */
export function useMerchantInvoices(merchantId: string | undefined): AsyncState<string[]> {
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
  error: Error | undefined;
}

/** Create an invoice. Returns a `mutate` function and the current mutation status. */
export function useCreateInvoice(): UseCreateInvoiceResult {
  const client = useFluxapayClient();
  const [data, setData] = React.useState<Invoice | undefined>(undefined);
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<Error | undefined>(undefined);

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
        const normalized = err instanceof Error ? err : new Error(String(err));
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
  error: Error | undefined;
}

/** Mark an invoice as paid. Returns a `mutate` function and the current mutation status. */
export function useMarkInvoicePaid(): UseMarkInvoicePaidResult {
  const client = useFluxapayClient();
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<Error | undefined>(undefined);

  const mutate = React.useCallback(
    async (invoiceId: string) => {
      setStatus("loading");
      setError(undefined);
      try {
        await client.markInvoicePaid(invoiceId);
        setStatus("success");
      } catch (err) {
        const normalized = err instanceof Error ? err : new Error(String(err));
        setError(normalized);
        setStatus("error");
        throw normalized;
      }
    },
    [client],
  );

  return { mutate, status, loading: status === "loading", error };
}
