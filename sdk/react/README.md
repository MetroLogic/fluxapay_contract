# @fluxapay/react

React hooks for [`@fluxapay/sdk`](../README.md) — pre-built data fetching and
mutations for payments, refunds, and merchant data. Built for the FluxaPay
merchant dashboard (Next.js), but works in any React app.

## Installation

```bash
npm install @fluxapay/react @fluxapay/sdk
```

## Quick Start

Wrap your app (or dashboard layout) in `FluxapayProvider`:

```tsx
import { FluxapayProvider } from "@fluxapay/react";

export default function App({ children }: { children: React.ReactNode }) {
  return (
    <FluxapayProvider
      config={{
        network: "testnet",
        rpcUrl: "https://soroban-testnet.stellar.org",
        contractId: "C...", // PaymentProcessor contract ID
        merchantRegistryContractId: "C...", // optional
      }}
    >
      {children}
    </FluxapayProvider>
  );
}
```

Then use the hooks anywhere below the provider:

```tsx
import { usePayment, useMerchant, useMerchantPayments, useCreatePayment, useRefund } from "@fluxapay/react";

function PaymentStatus({ paymentId }: { paymentId: string }) {
  const { data: payment, loading, error } = usePayment(paymentId);

  if (loading) return <p>Loading...</p>;
  if (error) return <p>Error: {error.message}</p>;
  return <p>Status: {payment?.status}</p>;
}

function MerchantProfile({ merchantId }: { merchantId: string }) {
  const { data: merchant, loading } = useMerchant(merchantId);
  return loading ? <p>Loading...</p> : <p>{merchant?.business_name}</p>;
}

function RecentPayments({ merchantId }: { merchantId: string }) {
  const { data: payments, loading } = useMerchantPayments(merchantId, { limit: 10 });
  if (loading) return <p>Loading...</p>;
  return (
    <ul>
      {payments?.map((p) => (
        <li key={p.payment_id}>{p.payment_id}: {p.amount.toString()}</li>
      ))}
    </ul>
  );
}

function RefundDetails({ refundId }: { refundId: string }) {
  const { data: refund, loading } = useRefund(refundId);
  return loading ? <p>Loading...</p> : <p>{refund?.reason}</p>;
}

function CreatePaymentForm() {
  const { mutate, loading, error, data } = useCreatePayment();

  const onSubmit = async () => {
    await mutate({
      paymentId: "pay_123",
      merchantId: "G...",
      amount: 1_000_000n,
      currency: "USDC",
      depositAddress: "G...",
    });
  };

  return (
    <button onClick={onSubmit} disabled={loading}>
      {loading ? "Creating..." : "Create payment"}
    </button>
  );
}
```

### Invoices

`useInvoice`, `useMerchantInvoices`, `useCreateInvoice`, and `useMarkInvoicePaid`
manage merchant invoices through the FluxaPay backend (requires `apiUrl` to be
set in `FluxapayConfig`):

```tsx
import {
  useInvoice,
  useMerchantInvoices,
  useCreateInvoice,
  useMarkInvoicePaid,
} from "@fluxapay/react";

function InvoiceDetails({ invoiceId }: { invoiceId: string }) {
  const { data: invoice, loading } = useInvoice(invoiceId);
  return loading ? <p>Loading...</p> : <p>Status: {invoice?.status}</p>;
}

function MerchantInvoiceList({ merchantId }: { merchantId: string }) {
  const { data: invoiceIds, loading } = useMerchantInvoices(merchantId);
  if (loading) return <p>Loading...</p>;
  return (
    <ul>
      {invoiceIds?.map((id) => (
        <li key={id}>{id}</li>
      ))}
    </ul>
  );
}

function CreateInvoiceForm({ merchantId }: { merchantId: string }) {
  const { mutate, loading } = useCreateInvoice();

  const onSubmit = () =>
    mutate({
      merchantId,
      currency: "USDC",
      lineItems: [{ description: "Consulting", quantity: 1, unitAmount: 100_0000000n }],
    });

  return (
    <button onClick={onSubmit} disabled={loading}>
      {loading ? "Creating..." : "Create invoice"}
    </button>
  );
}

function MarkInvoicePaidButton({ invoiceId }: { invoiceId: string }) {
  const { mutate, loading } = useMarkInvoicePaid();
  return (
    <button onClick={() => mutate(invoiceId)} disabled={loading}>
      {loading ? "Saving..." : "Mark as paid"}
    </button>
  );
}
```

## Hooks

All query hooks return a `{ data, loading, error, refetch }` shape, matching
the pattern used by React Query / SWR consumers.

- `usePayment(paymentId)` — fetch a single payment.
- `useMerchant(merchantId)` — fetch a single merchant.
- `useMerchantPayments(merchantId, { offset?, limit? })` — fetch a merchant's paginated payments.
- `useRefund(refundId)` — fetch a single refund.
- `useCreatePayment()` — returns `{ mutate, data, status, loading, error }` for creating a payment.
- `useSubscriptionPlan(planId)` — fetch a subscription plan by ID.
- `useCreateSubscriptionPlan()` — returns `{ mutate, data, status, loading, error }` for creating a plan.
- `useSubscribeToPlan()` — returns `{ mutate, data, status, loading, error }` for subscribing a payer to a plan.

## Subscription Plan Example

```tsx
import {
  useSubscriptionPlan,
  useCreateSubscriptionPlan,
  useSubscribeToPlan,
  type SubscriptionPlan,
} from "@fluxapay/react";

function PlanDetails({ planId }: { planId: string }) {
  const { data: plan, loading } = useSubscriptionPlan(planId);
  if (loading) return <p>Loading plan...</p>;
  return <p>{plan?.name}: {plan?.amount.toString()} {plan?.currency}/{plan?.billingInterval}</p>;
}

function CreatePlanForm({ merchantId }: { merchantId: string }) {
  const { mutate, loading, error } = useCreateSubscriptionPlan();

  const onSubmit = async () => {
    await mutate({
      merchant: merchantId,
      planId: "plan_weekly",
      name: "Weekly Pro",
      description: "Unlimited weekly access",
      amount: 1_000_000n,
      currency: "USDC",
      billingInterval: "Weekly",
    });
  };

  return (
    <button onClick={onSubmit} disabled={loading}>
      {loading ? "Creating..." : "Create plan"}
    </button>
  );
}

function SubscribeButton({ payer, planId }: { payer: string; planId: string }) {
  const { mutate, loading } = useSubscribeToPlan();

  const onSubscribe = async () => {
    await mutate({
      payer,
      planId,
      paymentId: `sub_${planId}_${payer.slice(0, 8)}`,
    });
  };

  return (
    <button onClick={onSubscribe} disabled={loading}>
      {loading ? "Subscribing..." : "Subscribe"}
    </button>
  );
}
```
- `useInvoice(invoiceId)` — fetch a single invoice.
- `useMerchantInvoices(merchantId)` — fetch a merchant's invoice ids.
- `useCreateInvoice()` — returns `{ mutate, data, status, loading, error }` for creating an invoice.
- `useMarkInvoicePaid()` — returns `{ mutate, status, loading, error }` for marking an invoice as paid.

## License

MIT
