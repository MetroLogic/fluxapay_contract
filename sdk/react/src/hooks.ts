import * as React from "react";
import type {
  PaymentCharge,
  Merchant,
  Refund,
  CreatePaymentParams,
  SubscriptionPlan,
} from "@fluxapay/sdk";
import { useFluxapayClient } from "./FluxapayProvider.js";
import { useAsync, type AsyncState } from "./useAsync.js";

export type { SubscriptionPlan } from "@fluxapay/sdk";

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

// -- Issue #679: Subscription plan hooks ---------------------------------------

/** Fetch a single subscription plan by ID. Re-fetches whenever `planId` changes. */
export function useSubscriptionPlan(planId: string | undefined): AsyncState<SubscriptionPlan> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getSubscriptionPlan(planId as string) as unknown as Promise<SubscriptionPlan>,
    [planId],
    !!planId,
  );
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
  error: Error | undefined;
}

/** Create a subscription plan. Returns a `mutate` function and current transaction status. */
export function useCreateSubscriptionPlan(): UseCreateSubscriptionPlanResult {
  const client = useFluxapayClient();
  const [data, setData] = React.useState<void>(undefined);
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<Error | undefined>(undefined);

  const mutate = React.useCallback(
    async (params: UseCreateSubscriptionPlanParams) => {
      setStatus("loading");
      setError(undefined);
      try {
        await client.createSubscriptionPlan(params);
        setData(undefined);
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
  error: Error | undefined;
}

/** Subscribe a payer to an existing subscription plan. */
export function useSubscribeToPlan(): UseSubscribeToPlanResult {
  const client = useFluxapayClient();
  const [data, setData] = React.useState<void>(undefined);
  const [status, setStatus] = React.useState<MutationStatus>("idle");
  const [error, setError] = React.useState<Error | undefined>(undefined);

  const mutate = React.useCallback(
    async (params: UseSubscribeToPlanParams) => {
      setStatus("loading");
      setError(undefined);
      try {
        await client.subscribeToPlan(params);
        setData(undefined);
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

  return { mutate, data, status, loading: status === "loading", error };
}
