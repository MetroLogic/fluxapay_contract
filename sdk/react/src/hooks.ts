import * as React from "react";
import { FluxapayError } from "@fluxapay/sdk";
import type {
  PaymentCharge,
  PaymentStatus,
  Merchant,
  Refund,
  CreatePaymentParams,
  SubscriptionPlan,
  Subscription,
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
import { computeTotalAccrued } from "@fluxapay/sdk";
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

/** How often {@link useMerchantBalance} re-fetches in the background. */
const MERCHANT_BALANCE_POLL_INTERVAL_MS = 10_000;

export interface UseMerchantBalanceResult {
  balance: bigint | null;
  loading: boolean;
  error: Error | null;
  refresh: () => void;
}

/**
 * Subscribes to a merchant's current pending settlement balance
 * (`FluxapayClient.getMerchantBalance`), re-fetching on a background
 * interval so dashboard widgets pick up new confirmed payments without a
 * manual refresh.
 *
 * The SDK does not yet expose a push-based contract event subscription, so
 * polling is the interim mechanism: call `refresh()` for an immediate
 * update (e.g. right after your own app logic observes a
 * `PAYMENT/CONFIRMED`-style event from a websocket or indexer), and the
 * background interval keeps the value fresh otherwise.
 */
export function useMerchantBalance(merchantId: string | undefined): UseMerchantBalanceResult {
  const client = useFluxapayClient();
  const [balance, setBalance] = React.useState<bigint | null>(null);
  const [loading, setLoading] = React.useState<boolean>(!!merchantId);
  const [error, setError] = React.useState<Error | null>(null);
  const [tick, setTick] = React.useState(0);

  const refresh = React.useCallback(() => setTick((t) => t + 1), []);

  React.useEffect(() => {
    if (!merchantId) {
      setLoading(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);

    client
      .getMerchantBalance(merchantId)
      .then((result) => {
        if (!cancelled) {
          setBalance(BigInt(result ?? 0));
          setLoading(false);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err : new Error(String(err)));
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client, merchantId, tick]);

  // Background poll so the balance reflects settlements that happened
  // without the consumer calling `refresh()` itself.
  React.useEffect(() => {
    if (!merchantId) return;
    const id = setInterval(refresh, MERCHANT_BALANCE_POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [merchantId, refresh]);

  return { balance, loading, error, refresh };
}

export interface UseStreamAccrualResult {
  accrued: bigint | null;
  remainingDeposit: bigint | null;
  loading: boolean;
  error: Error | null;
}

/** Snapshot of a stream's on-chain accrual state, as of the last poll. */
interface StreamCheckpoint {
  accruedAtCheckpoint: bigint;
  lastCheckpointAt: bigint;
  ratePerSecond: bigint;
  remainingDeposit: bigint;
  isActive: boolean;
}

/** How often the animation tick recomputes the extrapolated accrued amount. */
const STREAM_ANIMATION_TICK_MS = 250;

/**
 * Subscribes to a payment stream and returns the live accrued amount,
 * re-fetching the on-chain record every `pollIntervalMs` via
 * `FluxapayClient.getStream`.
 *
 * Between polls, the accrued amount is extrapolated client-side using
 * `computeTotalAccrued` — the same lazy-accrual formula the contract uses
 * (`fluxapay/src/stream.rs`) — driven by a short animation tick, so a
 * stream status UI can show a smoothly increasing number instead of one
 * that jumps only every `pollIntervalMs`.
 */
export function useStreamAccrual(
  streamId: string | undefined,
  pollIntervalMs = 5000,
): UseStreamAccrualResult {
  const client = useFluxapayClient();
  const [accrued, setAccrued] = React.useState<bigint | null>(null);
  const [remainingDeposit, setRemainingDeposit] = React.useState<bigint | null>(null);
  const [loading, setLoading] = React.useState<boolean>(!!streamId);
  const [error, setError] = React.useState<Error | null>(null);

  const checkpointRef = React.useRef<StreamCheckpoint | null>(null);

  const applyCheckpoint = React.useCallback((checkpoint: StreamCheckpoint) => {
    checkpointRef.current = checkpoint;
    setRemainingDeposit(checkpoint.remainingDeposit);

    if (!checkpoint.isActive) {
      setAccrued(checkpoint.accruedAtCheckpoint);
      return;
    }

    const nowSeconds = BigInt(Math.floor(Date.now() / 1000));
    setAccrued(
      computeTotalAccrued(
        checkpoint.accruedAtCheckpoint,
        checkpoint.lastCheckpointAt,
        nowSeconds,
        checkpoint.ratePerSecond,
        checkpoint.remainingDeposit,
      ),
    );
  }, []);

  // Poll the chain for the authoritative checkpoint.
  React.useEffect(() => {
    if (!streamId) {
      setLoading(false);
      return;
    }

    let cancelled = false;

    const poll = () => {
      client
        .getStream(streamId)
        .then((stream: any) => {
          if (cancelled) return;
          applyCheckpoint({
            accruedAtCheckpoint: BigInt(stream.accrued_at_checkpoint ?? 0),
            lastCheckpointAt: BigInt(stream.last_checkpoint_at ?? 0),
            ratePerSecond: BigInt(stream.rate_per_second ?? 0),
            remainingDeposit: BigInt(stream.remaining_deposit ?? 0),
            isActive: stream.status?.tag === "Active",
          });
          setLoading(false);
        })
        .catch((err: unknown) => {
          if (cancelled) return;
          setError(err instanceof Error ? err : new Error(String(err)));
          setLoading(false);
        });
    };

    setLoading(true);
    setError(null);
    poll();
    const pollId = setInterval(poll, pollIntervalMs);

    return () => {
      cancelled = true;
      clearInterval(pollId);
    };
  }, [client, streamId, pollIntervalMs, applyCheckpoint]);

  // Smooth client-side extrapolation between polls.
  React.useEffect(() => {
    if (!streamId) return;
    const tickId = setInterval(() => {
      if (checkpointRef.current) {
        applyCheckpoint(checkpointRef.current);
      }
    }, STREAM_ANIMATION_TICK_MS);
    return () => clearInterval(tickId);
  }, [streamId, applyCheckpoint]);

  return { accrued, remainingDeposit, loading, error };
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

/** Fetch a single subscription by ID. Re-fetches whenever `subscriptionId` changes. */
export function useSubscription(
  subscriptionId: string | undefined,
): AsyncState<Subscription, FluxapayError> {
  const client = useFluxapayClient();
  return useAsync(
    () => client.getSubscription(subscriptionId as string) as Promise<Subscription>,
    [subscriptionId],
    !!subscriptionId,
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

export interface UsePaymentStatusOptions {
  /** Initial polling interval in milliseconds. Defaults to 2000. */
  pollIntervalMs?: number;
  /** Maximum backoff interval cap in milliseconds. Defaults to 30000. */
  maxBackoffMs?: number;
  /** Terminal statuses that stop polling automatically. Defaults to ['confirmed', 'failed', 'expired']. */
  stopOnStatuses?: (PaymentStatus | string)[];
}

export interface UsePaymentStatusResult {
  status: string | undefined;
  payment: PaymentCharge | undefined;
  error: FluxapayError | undefined;
  loading: boolean;
  refetch: () => Promise<void>;
}

/**
 * Issue #769: React hook for real-time payment status polling with exponential backoff.
 * Starts polling immediately, doubles interval after each non-terminal status response up to `maxBackoffMs`.
 * Stops polling when status reaches a terminal state or the component unmounts.
 */
export function usePaymentStatus(
  paymentId: string | undefined,
  options?: UsePaymentStatusOptions,
): UsePaymentStatusResult {
  const client = useFluxapayClient();
  const pollIntervalMs = options?.pollIntervalMs ?? 2000;
  const maxBackoffMs = options?.maxBackoffMs ?? 30000;
  const stopOnStatuses = React.useMemo(() => {
    const list = options?.stopOnStatuses ?? ["confirmed", "failed", "expired"];
    return list.map((s) => (typeof s === "string" ? s.toLowerCase() : String(s).toLowerCase()));
  }, [options?.stopOnStatuses]);

  const [payment, setPayment] = React.useState<PaymentCharge | undefined>(undefined);
  const [status, setStatus] = React.useState<string | undefined>(undefined);
  const [error, setError] = React.useState<FluxapayError | undefined>(undefined);
  const [loading, setLoading] = React.useState<boolean>(!!paymentId);

  const currentIntervalRef = React.useRef<number>(pollIntervalMs);
  const isStoppedRef = React.useRef<boolean>(false);
  const timerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  const isMountedRef = React.useRef<boolean>(true);

  React.useEffect(() => {
    currentIntervalRef.current = pollIntervalMs;
    isStoppedRef.current = false;
  }, [pollIntervalMs, paymentId]);

  const fetchStatus = React.useCallback(
    async (isManualRefetch = false) => {
      if (!paymentId) return;
      if (isManualRefetch) {
        currentIntervalRef.current = pollIntervalMs;
        isStoppedRef.current = false;
      }

      try {
        setLoading(true);
        const res = (await client.getPayment(paymentId)) as unknown as PaymentCharge;
        if (!isMountedRef.current) return;

        setPayment(res);
        setError(undefined);

        let rawStatus: string = "";
        if (typeof res?.status === "string") {
          rawStatus = res.status;
        } else if (
          typeof res?.status === "object" &&
          res?.status !== null &&
          "tag" in (res.status as any)
        ) {
          rawStatus = (res.status as any).tag;
        } else {
          rawStatus = String(res?.status ?? "");
        }

        const normalizedStatus = rawStatus.toLowerCase();
        setStatus(rawStatus);

        if (stopOnStatuses.includes(normalizedStatus)) {
          isStoppedRef.current = true;
        } else {
          currentIntervalRef.current = Math.min(currentIntervalRef.current * 2, maxBackoffMs);
        }
      } catch (err) {
        if (!isMountedRef.current) return;
        const normalized = toFluxapayError(err);
        setError(normalized);
        currentIntervalRef.current = Math.min(currentIntervalRef.current * 2, maxBackoffMs);
      } finally {
        if (isMountedRef.current) {
          setLoading(false);
        }
      }
    },
    [client, paymentId, pollIntervalMs, maxBackoffMs, stopOnStatuses],
  );

  React.useEffect(() => {
    isMountedRef.current = true;
    if (!paymentId) {
      setLoading(false);
      return;
    }

    let isCancelled = false;

    const runPoll = async () => {
      if (isCancelled || isStoppedRef.current) return;
      await fetchStatus(false);
      if (isCancelled || isStoppedRef.current) return;

      timerRef.current = setTimeout(runPoll, currentIntervalRef.current);
    };

    runPoll();

    return () => {
      isCancelled = true;
      isMountedRef.current = false;
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [paymentId, fetchStatus]);

  const refetch = React.useCallback(async () => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    await fetchStatus(true);
  }, [fetchStatus]);

  return { status, payment, error, loading, refetch };
}

export { usePaymentEvents } from "./usePaymentEvents.js";
export type {
  PaymentEvent,
  ConnectionStatus,
  UsePaymentEventsOptions,
  UsePaymentEventsResult,
} from "./usePaymentEvents.js";

