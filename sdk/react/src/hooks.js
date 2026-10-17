import * as React from "react";
import { useFluxapayClient } from "./FluxapayProvider.js";
import { useAsync } from "./useAsync.js";
function toFluxapayError(error) {
    if (error instanceof FluxapayError) {
        return error;
    }
    if (error instanceof Error) {
        return new FluxapayError(0, "UnknownFluxapayError", error.message, error);
    }
    return new FluxapayError(0, "UnknownFluxapayError", String(error), error);
}
/** Fetch a single payment by id. Re-fetches whenever `paymentId` changes. */
export function usePayment(paymentId) {
    const client = useFluxapayClient();
    return useAsync(() => client.getPayment(paymentId), [paymentId], !!paymentId);
}
/** Fetch a single merchant by id. Re-fetches whenever `merchantId` changes. */
export function useMerchant(merchantId) {
    const client = useFluxapayClient();
    return useAsync(() => client.getMerchant(merchantId), [merchantId], !!merchantId);
}
export function useMerchantPayments(merchantId, options) {
    const client = useFluxapayClient();
    const offset = options?.offset ?? 0;
    const limit = options?.limit ?? 20;
    const statusFilter = options?.statusFilter;
    return useAsync(async () => {
        const idsTx = await client.contract.get_merchant_payments_paginated({
            merchant_id: merchantId,
            offset,
            limit,
            status_filter: statusFilter ?? null,
        });
        const ids = idsTx.result;
        const payments = await Promise.all(ids.map((id) => client.getPayment(id)));
        return payments;
    }, [merchantId, offset, limit, statusFilter], !!merchantId);
}
export function useMerchantLinks(merchantId, options) {
    const client = useFluxapayClient();
    const offset = options?.offset ?? 0;
    const limit = options?.limit ?? 100;
    const activeOnly = options?.activeOnly ?? false;
    return useAsync(() => client.getMerchantLinks(merchantId, {
        offset,
        limit,
        activeOnly,
    }), [merchantId, offset, limit, activeOnly], !!merchantId);
}
/** Fetch a single refund by id. Re-fetches whenever `refundId` changes. */
export function useRefund(refundId) {
    const client = useFluxapayClient();
    return useAsync(() => client.getRefund(refundId), [refundId], !!refundId);
}
/** Fetch a single dispute by id. Re-fetches whenever `disputeId` changes. */
export function useDispute(disputeId) {
    const client = useFluxapayClient();
    return useAsync(() => client.getDispute(disputeId), [disputeId], !!disputeId);
}
/** Fetch all disputes for a payment. Re-fetches when `paymentId` changes. */
export function usePaymentDisputes(paymentId) {
    const client = useFluxapayClient();
    return useAsync(() => client.getPaymentDisputes(paymentId), [paymentId], !!paymentId);
}
/** Fetch merchant analytics over a timestamp range. Re-fetches when any input changes. */
export function useMerchantAnalytics(merchantId, from, to) {
    const client = useFluxapayClient();
    return useAsync(() => client.getMerchantAnalytics(merchantId, from, to), [merchantId, from, to], !!merchantId);
}
export function useCreatePayment() {
    const client = useFluxapayClient();
    const [data, setData] = React.useState(undefined);
    const [status, setStatus] = React.useState("idle");
    const [error, setError] = React.useState(undefined);
    const mutate = React.useCallback(async (params) => {
        setStatus("loading");
        setError(undefined);
        try {
            const payment = (await client.createPayment(params));
            setData(payment);
            setStatus("success");
            return payment;
        }
        catch (err) {
            const normalized = toFluxapayError(err);
            setError(normalized);
            setStatus("error");
            throw normalized;
        }
    }, [client]);
    return { mutate, data, status, loading: status === "loading", error };
}
export function useCreateDispute() {
    const client = useFluxapayClient();
    const [data, setData] = React.useState(undefined);
    const [status, setStatus] = React.useState("idle");
    const [error, setError] = React.useState(undefined);
    const mutate = React.useCallback(async (params) => {
        setStatus("loading");
        setError(undefined);
        try {
            const result = await client.createDispute(params);
            const disputeId = typeof result === "string"
                ? result
                : (result?.dispute_id ??
                    result?.disputeId ??
                    result?.id ??
                    "");
            setData(disputeId);
            setStatus("success");
            return disputeId;
        }
        catch (err) {
            const normalized = toFluxapayError(err);
            setError(normalized);
            setStatus("error");
            throw normalized;
        }
    }, [client]);
    return { mutate, data, status, loading: status === "loading", error };
}
/** Fetch a single subscription plan by ID. Re-fetches whenever `planId` changes. */
export function useSubscriptionPlan(planId) {
    const client = useFluxapayClient();
    return useAsync(() => client.getSubscriptionPlan(planId), [planId], !!planId);
}
/** Create a subscription plan. Returns a mutate function and the current status. */
export function useCreateSubscriptionPlan() {
    const client = useFluxapayClient();
    const [data, setData] = React.useState(undefined);
    const [status, setStatus] = React.useState("idle");
    const [error, setError] = React.useState(undefined);
    const mutate = React.useCallback(async (params) => {
        setStatus("loading");
        setError(undefined);
        try {
            await client.createSubscriptionPlan(params);
            setData(undefined);
            setStatus("success");
        }
        catch (err) {
            const normalized = toFluxapayError(err);
            setError(normalized);
            setStatus("error");
            throw normalized;
        }
    }, [client]);
    return { mutate, data, status, loading: status === "loading", error };
}
export function useSubscribeToPlan() {
    const client = useFluxapayClient();
    const [data, setData] = React.useState(undefined);
    const [status, setStatus] = React.useState("idle");
    const [error, setError] = React.useState(undefined);
    const mutate = React.useCallback(async (params) => {
        setStatus("loading");
        setError(undefined);
        try {
            await client.subscribeToPlan(params);
            setData(undefined);
            setStatus("success");
        }
        catch (err) {
            const normalized = toFluxapayError(err);
            setError(normalized);
            setStatus("error");
            throw normalized;
        }
    }, [client]);
    return { mutate, data, status, loading: status === "loading", error };
}
/** Fetch a single invoice by id. Re-fetches whenever `invoiceId` changes. */
export function useInvoice(invoiceId) {
    const client = useFluxapayClient();
    return useAsync(() => client.getInvoice(invoiceId), [invoiceId], !!invoiceId);
}
/** Fetch the list of invoice ids for a merchant. Re-fetches whenever `merchantId` changes. */
export function useMerchantInvoices(merchantId) {
    const client = useFluxapayClient();
    return useAsync(() => client.getMerchantInvoices(merchantId), [merchantId], !!merchantId);
}
/** Create an invoice. Returns a mutate function and the current mutation status. */
export function useCreateInvoice() {
    const client = useFluxapayClient();
    const [data, setData] = React.useState(undefined);
    const [status, setStatus] = React.useState("idle");
    const [error, setError] = React.useState(undefined);
    const mutate = React.useCallback(async (params) => {
        setStatus("loading");
        setError(undefined);
        try {
            const invoice = await client.createInvoice(params);
            setData(invoice);
            setStatus("success");
            return invoice;
        }
        catch (err) {
            const normalized = toFluxapayError(err);
            setError(normalized);
            setStatus("error");
            throw normalized;
        }
    }, [client]);
    return { mutate, data, status, loading: status === "loading", error };
}
/** Mark an invoice as paid. Returns a mutate function and the current mutation status. */
export function useMarkInvoicePaid() {
    const client = useFluxapayClient();
    const [status, setStatus] = React.useState("idle");
    const [error, setError] = React.useState(undefined);
    const mutate = React.useCallback(async (invoiceId) => {
        setStatus("loading");
        setError(undefined);
        try {
            await client.markInvoicePaid(invoiceId);
            setStatus("success");
        }
        catch (err) {
            const normalized = toFluxapayError(err);
            setError(normalized);
            setStatus("error");
            throw normalized;
        }
    }, [client]);
    return { mutate, status, loading: status === "loading", error };
}
//# sourceMappingURL=hooks.js.map