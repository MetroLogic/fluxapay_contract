/**
 * Unit tests for swap_and_pay wrappers (Issue #577), batch stream operations (Issue #576),
 * and dispute status filtering (Issue #575).
 */
import { test, describe } from "node:test";
import assert from "node:assert/strict";

import {
  FluxapayClient,
  ArbitrageDetectedError,
  SwapPathInvalidError,
  OraclePriceDeviationError,
  type SwapAndPayParams,
  type SwapAndPayMultiRouteParams,
} from "../src/index.js";

function makeMockClient() {
  const calls: { method: string; args: any }[] = [];
  const client = new FluxapayClient({
    contractId: "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    rpcUrl: "http://localhost:8000",
    network: "testnet",
  });

  const mockContract: any = {
    options: {
      contractId: "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
      networkPassphrase: "Test SDF Network ; September 2015",
    },
    swap_and_pay: async (params: any) => {
      calls.push({ method: "swap_and_pay", args: params });
      return {
        payment_id: params.args.payment_id,
        amount: params.args.amount,
        status: "Pending",
      };
    },
    swap_and_pay_multi_route: async (params: any) => {
      calls.push({ method: "swap_and_pay_multi_route", args: params });
      return {
        payment_id: params.args.payment_id,
        amount: params.args.amount,
        status: "Pending",
      };
    },
    cancel_multiple_streams: async (params: any) => {
      calls.push({ method: "cancel_multiple_streams", args: params });
      return params.stream_ids;
    },
    batch_withdraw_to: async (params: any) => {
      calls.push({ method: "batch_withdraw_to", args: params });
      return params.withdrawals.map((w: any) => w.stream_id);
    },
    top_up_multiple_streams: async (params: any) => {
      calls.push({ method: "top_up_multiple_streams", args: params });
      return null;
    },
    get_payment_disputes: async (params: any) => {
      calls.push({ method: "get_payment_disputes", args: params });
      return [
        { id: "disp_1", payment_id: params.payment_id, status: "Open" },
        { id: "disp_2", payment_id: params.payment_id, status: "UnderReview" },
      ];
    },
    get_payment_disputes_by_status: async (params: any) => {
      calls.push({ method: "get_payment_disputes_by_status", args: params });
      return [
        { id: "disp_1", payment_id: params.payment_id, status: params.status },
      ];
    },
  };

  (client as any).contract = mockContract;
  return { client, calls, mockContract };
}

describe("swapAndPay SDK wrappers (Issue #577)", () => {
  test("swapAndPay correctly passes arguments to contract", async () => {
    const { client, calls } = makeMockClient();
    const params: SwapAndPayParams = {
      payer: "G_PAYER",
      merchantId: "G_MERCHANT",
      paymentId: "swap_001",
      dexRouter: "C_ROUTER",
      path: ["C_TOKEN_IN", "C_USDC"],
      amountIn: 10_000n,
      amountOutMin: 9_900n,
      deadline: 1700000000,
    };

    const res = await client.swapAndPay(params);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "swap_and_pay");
    assert.equal(calls[0].args.args.payer, "G_PAYER");
    assert.equal(calls[0].args.args.payment_id, "swap_001");
    assert.equal(calls[0].args.args.token_in, "C_TOKEN_IN");
    assert.equal(calls[0].args.args.amount_in, 10_000n);
    assert.equal(calls[0].args.args.amount_out_min, 9_900n);
    assert.equal(res.payment_id, "swap_001");
  });

  test("swapAndPayMultiRoute correctly passes routes to contract", async () => {
    const { client, calls } = makeMockClient();
    const params: SwapAndPayMultiRouteParams = {
      payer: "G_PAYER",
      merchantId: "G_MERCHANT",
      paymentId: "multi_swap_001",
      dexRouter: "C_ROUTER_1",
      path: ["C_TOKEN_IN", "C_USDC"],
      amountIn: 10_000n,
      amountOutMin: 9_900n,
      routes: [
        { router: "C_ROUTER_1", path: ["C_TOKEN_IN", "C_USDC"], amountIn: 5_000n },
        { router: "C_ROUTER_2", path: ["C_TOKEN_IN", "C_USDC"], amountIn: 5_000n },
      ],
    };

    const res = await client.swapAndPayMultiRoute(params);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "swap_and_pay_multi_route");
    assert.equal(calls[0].args.routes.length, 2);
    assert.equal(calls[0].args.routes[0].router, "C_ROUTER_1");
    assert.equal(calls[0].args.routes[1].router, "C_ROUTER_2");
    assert.equal(calls[0].args.min_output_amount, 9_900n);
    assert.equal(res.payment_id, "multi_swap_001");
  });

  test("swap errors are mapped to specific FluxapayError subclasses", async () => {
    const { client, mockContract } = makeMockClient();

    mockContract.swap_and_pay = async () => {
      const err = new Error("ArbitrageDetected");
      (err as any).code = 27;
      throw err;
    };
    await assert.rejects(
      () =>
        client.swapAndPay({
          payer: "G_PAYER",
          merchantId: "G_MERCHANT",
          paymentId: "swap_err",
          dexRouter: "C_ROUTER",
          path: ["C_TOKEN_IN", "C_USDC"],
          amountIn: 100n,
          amountOutMin: 90n,
        }),
      (err: any) => err instanceof ArbitrageDetectedError && err.code === 27,
    );

    mockContract.swap_and_pay = async () => {
      const err = new Error("SwapPathInvalid");
      (err as any).code = 28;
      throw err;
    };
    await assert.rejects(
      () =>
        client.swapAndPay({
          payer: "G_PAYER",
          merchantId: "G_MERCHANT",
          paymentId: "swap_err",
          dexRouter: "C_ROUTER",
          path: [],
          amountIn: 100n,
          amountOutMin: 90n,
        }),
      (err: any) => err instanceof SwapPathInvalidError && err.code === 28,
    );

    mockContract.swap_and_pay = async () => {
      const err = new Error("OraclePriceDeviation");
      (err as any).code = 29;
      throw err;
    };
    await assert.rejects(
      () =>
        client.swapAndPay({
          payer: "G_PAYER",
          merchantId: "G_MERCHANT",
          paymentId: "swap_err",
          dexRouter: "C_ROUTER",
          path: ["C_TOKEN_IN", "C_USDC"],
          amountIn: 100n,
          amountOutMin: 90n,
        }),
      (err: any) => err instanceof OraclePriceDeviationError && err.code === 29,
    );
  });
});

describe("batch stream SDK methods (Issue #576)", () => {
  test("cancelMultipleStreams calls contract entrypoint", async () => {
    const { client, calls } = makeMockClient();
    await client.cancelMultipleStreams("G_SENDER", ["stream_1", "stream_2"]);

    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "cancel_multiple_streams");
    assert.equal(calls[0].args.sender, "G_SENDER");
    assert.deepEqual(calls[0].args.stream_ids, ["stream_1", "stream_2"]);
  });

  test("batchWithdrawTo calls contract entrypoint with formatted withdrawals", async () => {
    const { client, calls } = makeMockClient();
    await client.batchWithdrawTo("G_RECEIVER", ["stream_1", "stream_2"]);

    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "batch_withdraw_to");
    assert.equal(calls[0].args.recipient, "G_RECEIVER");
    assert.equal(calls[0].args.withdrawals.length, 2);
    assert.equal(calls[0].args.withdrawals[0].stream_id, "stream_1");
    assert.equal(calls[0].args.withdrawals[1].stream_id, "stream_2");
  });

  test("topUpMultipleStreams calls contract entrypoint with tuples", async () => {
    const { client, calls } = makeMockClient();
    await client.topUpMultipleStreams("G_SENDER", [
      { streamId: "stream_1", amount: 100n },
      { streamId: "stream_2", amount: 200n },
    ]);

    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "top_up_multiple_streams");
    assert.equal(calls[0].args.sender, "G_SENDER");
    assert.deepEqual(calls[0].args.top_ups, [
      ["stream_1", 100n],
      ["stream_2", 200n],
    ]);
  });
});

describe("dispute status filtering (Issue #575)", () => {
  test("getPaymentDisputes without status returns all disputes", async () => {
    const { client, calls } = makeMockClient();
    const disputes = await client.getPaymentDisputes("pay_123");

    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "get_payment_disputes");
    assert.equal(disputes.length, 2);
  });

  test("getPaymentDisputes with status delegates to getPaymentDisputesByStatus", async () => {
    const { client, calls } = makeMockClient();
    const disputes = await client.getPaymentDisputes("pay_123", "Open");

    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "get_payment_disputes_by_status");
    assert.equal(calls[0].args.status, "Open");
    assert.equal(disputes.length, 1);
  });

  test("getPaymentDisputesByStatus calls contract method directly", async () => {
    const { client, calls } = makeMockClient();
    const disputes = await client.getPaymentDisputesByStatus("pay_123", "UnderReview");

    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "get_payment_disputes_by_status");
    assert.equal(calls[0].args.status, "UnderReview");
    assert.equal(disputes.length, 1);
  });
});
