import { Buffer } from "buffer";
import { Address } from "@stellar/stellar-sdk";
import {
  AssembledTransaction,
  Client as ContractClient,
  ClientOptions as ContractClientOptions,
  MethodOptions,
  Result,
  Spec as ContractSpec,
} from "@stellar/stellar-sdk/contract";
import type {
  u32,
  i32,
  u64,
  i64,
  u128,
  i128,
  u256,
  i256,
  Option,
  Duration,
} from "@stellar/stellar-sdk/contract";
export * from "@stellar/stellar-sdk";
export * as contract from "@stellar/stellar-sdk/contract";
export * as rpc from "@stellar/stellar-sdk/rpc";

if (typeof window !== "undefined") {
  //@ts-ignore Buffer exists
  window.Buffer = window.Buffer || Buffer;
}

export const AccessControlError = {
  1: { message: "Unauthorized" },
  2: { message: "RoleAlreadyGranted" },
  3: { message: "RoleNotGranted" },
  4: { message: "CannotRenounceAdmin" },
  5: { message: "InvalidAdmin" },
};

export type AccessControlDataKey =
  | { tag: "Role"; values: readonly [string, string] }
  | { tag: "Admin"; values: void };

export interface FeeConfig {
  platform_fee_bps: i64;
  fixed_fee: i128;
  fee_recipient: Option<string>;
}

export type MaybeFeeConfig =
  | { tag: "None"; values: void }
  | { tag: "Some"; values: readonly [FeeConfig] };

export interface CreatePaymentArgs {
  payment_id: string;
  merchant_id: string;
  amount: i128;
  currency: string;
  deposit_address: string;
  expires_at: Option<u64>;
  duration_secs: Option<u64>;
  memo: Option<string>;
  memo_type: Option<string>;
  token_address: Option<string>;
  client_token: Option<string>;
  metadata_hash: Option<Buffer>;
  /**
   * Optional payment metadata.
   * Limits: ≤20 keys; each key ≤64 chars; each value ≤256 chars.
   * On-chain errors: MetadataTooLarge (#49), MetadataValueTooLong (#47).
   */
  metadata: Option<Record<string, string>>;
  fee_waiver_code: Option<string>;
}

export interface Merchant {
  active: boolean;
  business_name: string;
  created_at: u64;
  merchant_id: string;
  settlement_currency: string;
  verified: boolean;
  fee_config?: MaybeFeeConfig;
}

export const MerchantError = {
  1: { message: "MerchantAlreadyExists" },
  2: { message: "MerchantNotFound" },
  3: { message: "Unauthorized" },
  4: { message: "NotVerified" },
  5: { message: "AdminAlreadySet" },
};

export type MerchantDataKey =
  | { tag: "Merchant"; values: readonly [string] }
  | { tag: "Admin"; values: void };

export interface Refund {
  amount: i128;
  created_at: u64;
  payment_id: string;
  processed_at: Option<u64>;
  reason: string;
  refund_id: string;
  requester: string;
  status: RefundStatus;
}

export interface Dispute {
  amount: i128;
  created_at: u64;
  dispute_id: string;
  disputer: string;
  evidence: string;
  payment_id: string;
  reason: string;
  refund_id: Option<string>;
  resolution_notes: Option<string>;
  resolved_at: Option<u64>;
  status: DisputeStatus;
}

export const FluxaError = {
  1: { message: "PaymentNotFound" },
  2: { message: "PaymentAlreadyExists" },
  3: { message: "InvalidAmount" },
  4: { message: "AccessControlError" },
  5: { message: "PaymentExpired" },
  6: { message: "PaymentAlreadyProcessed" },
  7: { message: "InvalidPaymentId" },
  8: { message: "RefundNotFound" },
  9: { message: "RefundAlreadyProcessed" },
  10: { message: "Unauthorized" },
  11: { message: "DisputeNotFound" },
  12: { message: "DisputeAlreadyResolved" },
};

export type FluxaDataKey =
  | { tag: "Payment"; values: readonly [string] }
  | { tag: "MerchantPayments"; values: readonly [string] }
  | { tag: "Refund"; values: readonly [string] }
  | { tag: "PaymentRefunds"; values: readonly [string] }
  | { tag: "RefundCounter"; values: void }
  | { tag: "Dispute"; values: readonly [string] }
  | { tag: "PaymentDisputes"; values: readonly [string] }
  | { tag: "DisputeCounter"; values: void }
  | { tag: "UsdcToken"; values: void };

export type RefundStatus =
  | { tag: "Pending"; values: void }
  | { tag: "Completed"; values: void }
  | { tag: "Rejected"; values: void }
  | { tag: "Cancelled"; values: void };

export type DisputeStatus =
  | { tag: "Open"; values: void }
  | { tag: "UnderReview"; values: void }
  | { tag: "Resolved"; values: void }
  | { tag: "Rejected"; values: void };

export interface PaymentCharge {
  amount: i128;
  confirmed_at: Option<u64>;
  created_at: u64;
  currency: string;
  deposit_address: string;
  expires_at: u64;
  merchant_id: string;
  /** ID of the payment link that created this payment via `use_link`, for tracing
   *  a payment back to its source link. `undefined`/`None` for payments created
   *  directly via `create_payment`/`swap_and_pay` (issue #668). */
  payment_link_id: Option<string>;
  payer_address: Option<string>;
  payment_id: string;
  status: PaymentStatus;
  transaction_hash: Option<Buffer>;
}

export type PaymentStatus =
  | { tag: "Pending"; values: void }
  | { tag: "Confirmed"; values: void }
  | { tag: "Settled"; values: void }
  | { tag: "Expired"; values: void }
  | { tag: "Failed"; values: void }
  | { tag: "PartiallyPaid"; values: void }
  | { tag: "Overpaid"; values: void };

export interface RateData {
  decimals: u32;
  pair: string;
  rate: i128;
  updated_at: u64;
}

export const FXOracleError = {
  1: { message: "RateNotFound" },
  2: { message: "RateStale" },
  3: { message: "Unauthorized" },
};

export type OracleDataKey =
  | { tag: "Rate"; values: readonly [string] }
  | { tag: "StalenessThreshold"; values: void };

export interface Client {
  /**
   * Construct and simulate a get_merchant transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Get merchant info
   */
  get_merchant: (
    { merchant_id }: { merchant_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<Merchant>>>;

  /**
   * Construct and simulate a update_merchant transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Update merchant settings
   */
  update_merchant: (
    {
      merchant_id,
      business_name,
      settlement_currency,
      active,
      payout_address,
      bank_account,
      fee_config,
    }: {
      merchant_id: string;
      business_name: Option<string>;
      settlement_currency: Option<string>;
      active: Option<boolean>;
      payout_address: Option<string>;
      bank_account: Option<string>;
      fee_config: Option<FeeConfig>;
    },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a verify_merchant transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Verify merchant (admin only)
   */
  verify_merchant: (
    { admin, merchant_id }: { admin: string; merchant_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a register_merchant transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Register a new merchant
   */
  register_merchant: (
    {
      merchant_id,
      business_name,
      settlement_currency,
      payout_address,
      bank_account,
      fee_config,
    }: {
      merchant_id: string;
      business_name: string;
      settlement_currency: string;
      payout_address: Option<string>;
      bank_account: Option<string>;
      fee_config: Option<FeeConfig>;
    },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a merchant_initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Initialize the contract with an admin address
   */
  merchant_initialize: (
    { admin }: { admin: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a get_refund transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_refund: (
    { refund_id }: { refund_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<Refund>>>;

  /**
   * Construct and simulate a get_dispute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_dispute: (
    { dispute_id }: { dispute_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<Dispute>>>;

  /**
   * Construct and simulate a create_refund transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  create_refund: (
    {
      payment_id,
      refund_amount,
      reason,
      requester,
    }: {
      payment_id: string;
      refund_amount: i128;
      reason: string;
      requester: string;
    },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<string>>>;

  /**
   * Construct and simulate a get_payment transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_payment: (
    { payment_id }: { payment_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<PaymentCharge>>>;

  /**
   * Construct and simulate a create_dispute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  create_dispute: (
    {
      payment_id,
      amount,
      reason,
      evidence,
      disputer,
    }: {
      payment_id: string;
      amount: i128;
      reason: string;
      evidence: string;
      disputer: string;
    },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<string>>>;

  /**
   * Construct and simulate a process_refund transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  process_refund: (
    { operator, refund_id }: { operator: string; refund_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a reject_dispute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  reject_dispute: (
    {
      operator,
      dispute_id,
      resolution_notes,
    }: { operator: string; dispute_id: string; resolution_notes: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a review_dispute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  review_dispute: (
    { operator, dispute_id }: { operator: string; dispute_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a refund_has_role transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  refund_has_role: (
    { role, account }: { role: string; account: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<boolean>>;

  /**
   * Construct and simulate a refund_get_admin transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  refund_get_admin: (
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Option<string>>>;

  /**
   * Construct and simulate a cancel_payment transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  cancel_payment: (
    { authority, payment_id }: { authority: string; payment_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a create_payment transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  create_payment: (
    args: CreatePaymentArgs,
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<PaymentCharge>>>;

  /**
   * Construct and simulate a expire_payment transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  expire_payment: (
    { payment_id }: { payment_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a settle_payment transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  settle_payment: (
    {
      operator,
      payment_id,
      treasury_address,
    }: { operator: string; payment_id: string; treasury_address: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a verify_payment transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  verify_payment: (
    {
      oracle,
      payment_id,
      transaction_hash,
      payer_address,
      amount_received,
    }: {
      oracle: string;
      payment_id: string;
      transaction_hash: Buffer;
      payer_address: string;
      amount_received: i128;
    },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<PaymentStatus>>>;

  /**
   * Construct and simulate a refund_grant_role transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  refund_grant_role: (
    { admin, role, account }: { admin: string; role: string; account: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a refund_revoke_role transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  refund_revoke_role: (
    { admin, role, account }: { admin: string; role: string; account: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a get_payment_refunds transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_payment_refunds: (
    { payment_id }: { payment_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<Array<Refund>>>>;

  /**
   * Construct and simulate a get_payment_disputes transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_payment_disputes: (
    { payment_id }: { payment_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<Array<Dispute>>>>;

  /**
   * Construct and simulate a refund_renounce_role transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  refund_renounce_role: (
    { account, role }: { account: string; role: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a payment_grant_role transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  payment_grant_role: (
    { admin, role, account }: { admin: string; role: string; account: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a refund_transfer_admin transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  refund_transfer_admin: (
    { current_admin, new_admin }: { current_admin: string; new_admin: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a get_merchant_payments transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_merchant_payments: (
    { merchant_id }: { merchant_id: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Array<string>>>;

  /**
   * Construct and simulate a initialize_refund_manager transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize_refund_manager: (
    {
      admin,
      usdc_token_address,
    }: { admin: string; usdc_token_address: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<null>>;

  /**
   * Construct and simulate a resolve_dispute_with_refund transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  resolve_dispute_with_refund: (
    {
      operator,
      dispute_id,
      resolution_notes,
    }: { operator: string; dispute_id: string; resolution_notes: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<string>>>;

  /**
   * Construct and simulate a initialize_payment_processor transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize_payment_processor: (
    { admin }: { admin: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<null>>;

  /**
   * Construct and simulate a get_merchant_payments_paginated transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_merchant_payments_paginated: (
    {
      merchant_id,
      offset,
      limit,
    }: { merchant_id: string; offset: u32; limit: u32 },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Array<string>>>;

  /**
   * Construct and simulate a get_rate transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_rate: (
    { pair }: { pair: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<RateData>>>;

  /**
   * Construct and simulate a set_rate transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  set_rate: (
    {
      operator,
      pair,
      rate,
      decimals,
    }: { operator: string; pair: string; rate: i128; decimals: u32 },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a oracle_has_role transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  oracle_has_role: (
    { role, account }: { role: string; account: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<boolean>>;

  /**
   * Construct and simulate a get_oracle_admin transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_oracle_admin: (
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Option<string>>>;

  /**
   * Construct and simulate a oracle_grant_role transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  oracle_grant_role: (
    { admin, role, account }: { admin: string; role: string; account: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;

  /**
   * Construct and simulate a oracle_initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  oracle_initialize: (
    { admin, staleness_threshold }: { admin: string; staleness_threshold: u64 },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<null>>;

  /**
   * Construct and simulate a get_settlement_amount transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_settlement_amount: (
    {
      usdc_amount,
      target_currency,
    }: { usdc_amount: i128; target_currency: string },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<i128>>>;

  /**
   * Construct and simulate a get_staleness_threshold transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_staleness_threshold: (
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<u64>>;

  /**
   * Construct and simulate a set_staleness_threshold transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  set_staleness_threshold: (
    { admin, threshold }: { admin: string; threshold: u64 },
    options?: MethodOptions,
  ) => Promise<AssembledTransaction<Result<void>>>;
}
export class Client extends ContractClient {
  static async deploy<T = Client>(
    /** Options for initializing a Client as well as for calling a method, with extras specific to deploying. */
    options: MethodOptions &
      Omit<ContractClientOptions, "contractId"> & {
        /** The hash of the Wasm blob, which must already be installed on-chain. */
        wasmHash: Buffer | string;
        /** Salt used to generate the contract's ID. Passed through to {@link Operation.createCustomContract}. Default: random. */
        salt?: Buffer | Uint8Array;
        /** The format used to decode `wasmHash`, if it's provided as a string. */
        format?: "hex" | "base64";
      },
  ): Promise<AssembledTransaction<T>> {
    return ContractClient.deploy(null, options);
  }
  constructor(public readonly options: ContractClientOptions) {
    super(
      new ContractSpec([
        "AAAABAAAAAAAAAAAAAAAEkFjY2Vzc0NvbnRyb2xFcnJvcgAAAAAABQAAAAAAAAAMVW5hdXRob3JpemVkAAAAAQAAAAAAAAASUm9sZUFscmVhZHlHcmFudGVkAAAAAAACAAAAAAAAAA5Sb2xlTm90R3JhbnRlZAAAAAAAAwAAAAAAAAATQ2Fubm90UmVub3VuY2VBZG1pbgAAAAAEAAAAAAAAAAxJbnZhbGlkQWRtaW4AAAAF",
        "AAAAAgAAAAAAAAAAAAAAFEFjY2Vzc0NvbnRyb2xEYXRhS2V5AAAAAgAAAAEAAAAAAAAABFJvbGUAAAACAAAAEQAAABMAAAAAAAAAAAAAAAVBZG1pbgAAAA==",
        "AAAAAQAAAAAAAAAAAAAACE1lcmNoYW50AAAABgAAAAAAAAAGYWN0aXZlAAAAAAABAAAAAAAAAA1idXNpbmVzc19uYW1lAAAAAAAAEAAAAAAAAAAKY3JlYXRlZF9hdAAAAAAABgAAAAAAAAALbWVyY2hhbnRfaWQAAAAAEwAAAAAAAAATc2V0dGxlbWVudF9jdXJyZW5jeQAAAAAQAAAAAAAAAAh2ZXJpZmllZAAAAAE=",
        "AAAABAAAAAAAAAAAAAAADU1lcmNoYW50RXJyb3IAAAAAAAAFAAAAAAAAABVNZXJjaGFudEFscmVhZHlFeGlzdHMAAAAAAAABAAAAAAAAABBNZXJjaGFudE5vdEZvdW5kAAAAAgAAAAAAAAAMVW5hdXRob3JpemVkAAAAAwAAAAAAAAALTm90VmVyaWZpZWQAAAAABAAAAAAAAAAPQWRtaW5BbHJlYWR5U2V0AAAAAAU=",
        "AAAAAgAAAAAAAAAAAAAAD01lcmNoYW50RGF0YUtleQAAAAACAAAAAQAAAAAAAAAITWVyY2hhbnQAAAABAAAAEwAAAAAAAAAAAAAABUFkbWluAAAA",
        "AAAAAAAAABFHZXQgbWVyY2hhbnQgaW5mbwAAAAAAAAxnZXRfbWVyY2hhbnQAAAABAAAAAAAAAAttZXJjaGFudF9pZAAAAAATAAAAAQAAA+kAAAfQAAAACE1lcmNoYW50AAAH0AAAAA1NZXJjaGFudEVycm9yAAAA",
        "AAAAAAAAABhVcGRhdGUgbWVyY2hhbnQgc2V0dGluZ3MAAAAPdXBkYXRlX21lcmNoYW50AAAAAAQAAAAAAAAAC21lcmNoYW50X2lkAAAAABMAAAAAAAAADWJ1c2luZXNzX25hbWUAAAAAAAPoAAAAEAAAAAAAAAATc2V0dGxlbWVudF9jdXJyZW5jeQAAAAPoAAAAEAAAAAAAAAAGYWN0aXZlAAAAAAPoAAAAAQAAAAEAAAPpAAAD7QAAAAAAAAfQAAAADU1lcmNoYW50RXJyb3IAAAA=",
        "AAAAAAAAABxWZXJpZnkgbWVyY2hhbnQgKGFkbWluIG9ubHkpAAAAD3ZlcmlmeV9tZXJjaGFudAAAAAACAAAAAAAAAAVhZG1pbgAAAAAAABMAAAAAAAAAC21lcmNoYW50X2lkAAAAABMAAAABAAAD6QAAA+0AAAAAAAAH0AAAAA1NZXJjaGFudEVycm9yAAAA",
        "AAAAAAAAABdSZWdpc3RlciBhIG5ldyBtZXJjaGFudAAAAAARcmVnaXN0ZXJfbWVyY2hhbnQAAAAAAAADAAAAAAAAAAttZXJjaGFudF9pZAAAAAATAAAAAAAAAA1idXNpbmVzc19uYW1lAAAAAAAAEAAAAAAAAAATc2V0dGxlbWVudF9jdXJyZW5jeQAAAAAQAAAAAQAAA+kAAAPtAAAAAAAAB9AAAAANTWVyY2hhbnRFcnJvcgAAAA==",
        "AAAAAAAAAC1Jbml0aWFsaXplIHRoZSBjb250cmFjdCB3aXRoIGFuIGFkbWluIGFkZHJlc3MAAAAAAAATbWVyY2hhbnRfaW5pdGlhbGl6ZQAAAAABAAAAAAAAAAVhZG1pbgAAAAAAABMAAAABAAAD6QAAA+0AAAAAAAAH0AAAAA1NZXJjaGFudEVycm9yAAAA",
        "AAAAAQAAAAAAAAAAAAAABlJlZnVuZAAAAAAACAAAAAAAAAAGYW1vdW50AAAAAAALAAAAAAAAAApjcmVhdGVkX2F0AAAAAAAGAAAAAAAAAApwYXltZW50X2lkAAAAAAAQAAAAAAAAAAxwcm9jZXNzZWRfYXQAAAPoAAAABgAAAAAAAAAGcmVhc29uAAAAAAAQAAAAAAAAAAlyZWZ1bmRfaWQAAAAAAAAQAAAAAAAAAAlyZXF1ZXN0ZXIAAAAAAAATAAAAAAAAAAZzdGF0dXMAAAAAB9AAAAAMUmVmdW5kU3RhdHVz",
        "AAAAAQAAAAAAAAAAAAAAB0Rpc3B1dGUAAAAACwAAAAAAAAAGYW1vdW50AAAAAAALAAAAAAAAAApjcmVhdGVkX2F0AAAAAAAGAAAAAAAAAApkaXNwdXRlX2lkAAAAAAAQAAAAAAAAAAhkaXNwdXRlcgAAABMAAAAAAAAACGV2aWRlbmNlAAAAEAAAAAAAAAAKcGF5bWVudF9pZAAAAAAAEAAAAAAAAAAGcmVhc29uAAAAAAAQAAAAAAAAAAlyZWZ1bmRfaWQAAAAAAAPoAAAAEAAAAAAAAAAQcmVzb2x1dGlvbl9ub3RlcwAAA+gAAAAQAAAAAAAAAAtyZXNvbHZlZF9hdAAAAAPoAAAABgAAAAAAAAAGc3RhdHVzAAAAAAfQAAAADURpc3B1dGVTdGF0dXMAAAA=",
        "AAAABAAAAAAAAAAAAAAACkZsdXhhRXJyb3IAAAAAAAwAAAAAAAAAD1BheW1lbnROb3RGb3VuZAAAAAABAAAAAAAAABRQYXltZW50QWxyZWFkeUV4aXN0cwAAAAIAAAAAAAAADUludmFsaWRBbW91bnQAAAAAAAADAAAAAAAAABJBY2Nlc3NDb250cm9sRXJyb3IAAAAAAAQAAAAAAAAADlBheW1lbnRFeHBpcmVkAAAAAAAFAAAAAAAAABdQYXltZW50QWxyZWFkeVByb2Nlc3NlZAAAAAAGAAAAAAAAABBJbnZhbGlkUGF5bWVudElkAAAABwAAAAAAAAAOUmVmdW5kTm90Rm91bmQAAAAAAAgAAAAAAAAAFlJlZnVuZEFscmVhZHlQcm9jZXNzZWQAAAAAAAkAAAAAAAAADFVuYXV0aG9yaXplZAAAAAoAAAAAAAAAD0Rpc3B1dGVOb3RGb3VuZAAAAAALAAAAAAAAABZEaXNwdXRlQWxyZWFkeVJlc29sdmVkAAAAAAAM",
        "AAAAAgAAAAAAAAAAAAAADEZsdXhhRGF0YUtleQAAAAkAAAABAAAAAAAAAAdQYXltZW50AAAAAAEAAAAQAAAAAQAAAAAAAAAQTWVyY2hhbnRQYXltZW50cwAAAAEAAAATAAAAAQAAAAAAAAAGUmVmdW5kAAAAAAABAAAAEAAAAAEAAAAAAAAADlBheW1lbnRSZWZ1bmRzAAAAAAABAAAAEAAAAAAAAAAAAAAADVJlZnVuZENvdW50ZXIAAAAAAAABAAAAAAAAAAdEaXNwdXRlAAAAAAEAAAAQAAAAAQAAAAAAAAAPUGF5bWVudERpc3B1dGVzAAAAAAEAAAAQAAAAAAAAAAAAAAAORGlzcHV0ZUNvdW50ZXIAAAAAAAAAAAAAAAAACVVzZGNUb2tlbgAAAA==",
        "AAAAAgAAAAAAAAAAAAAADFJlZnVuZFN0YXR1cwAAAAMAAAAAAAAAAAAAAAdQZW5kaW5nAAAAAAAAAAAAAAAACUNvbXBsZXRlZAAAAAAAAAAAAAAAAAAACFJlamVjdGVk",
        "AAAAAgAAAAAAAAAAAAAADURpc3B1dGVTdGF0dXMAAAAAAAAEAAAAAAAAAAAAAAAET3BlbgAAAAAAAAAAAAAAC1VuZGVyUmV2aWV3AAAAAAAAAAAAAAAACFJlc29sdmVkAAAAAAAAAAAAAAAIUmVqZWN0ZWQ=",
        "AAAAAQAAAAAAAAAAAAAADVBheW1lbnRDaGFyZ2UAAAAAAAALAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAADGNvbmZpcm1lZF9hdAAAA+gAAAAGAAAAAAAAAApjcmVhdGVkX2F0AAAAAAAGAAAAAAAAAAhjdXJyZW5jeQAAABEAAAAAAAAAD2RlcG9zaXRfYWRkcmVzcwAAAAATAAAAAAAAAApleHBpcmVzX2F0AAAAAAAGAAAAAAAAAAttZXJjaGFudF9pZAAAAAATAAAAAAAAAA1wYXllcl9hZGRyZXNzAAAAAAAD6AAAABMAAAAAAAAACnBheW1lbnRfaWQAAAAAABAAAAAAAAAABnN0YXR1cwAAAAAH0AAAAA1QYXltZW50U3RhdHVzAAAAAAAAAAAAABB0cmFuc2FjdGlvbl9oYXNoAAAD6AAAA+4AAAAg",
        "AAAAAgAAAAAAAAAAAAAADVBheW1lbnRTdGF0dXMAAAAAAAAFAAAAAAAAAAAAAAAHUGVuZGluZwAAAAAAAAAAAAAAAAlDb25maXJtZWQAAAAAAAAAAAAAAAAAAAdTZXR0bGVkAAAAAAAAAAAAAAAAB0V4cGlyZWQAAAAAAAAAAAAAAAAGRmFpbGVkAAA=",
        "AAAAAAAAAAAAAAAKZ2V0X3JlZnVuZAAAAAAAAQAAAAAAAAAJcmVmdW5kX2lkAAAAAAAAEAAAAAEAAAPpAAAH0AAAAAZSZWZ1bmQAAAAAB9AAAAAKRmx1eGFFcnJvcgAA",
        "AAAAAAAAAAAAAAALZ2V0X2Rpc3B1dGUAAAAAAQAAAAAAAAAKZGlzcHV0ZV9pZAAAAAAAEAAAAAEAAAPpAAAH0AAAAAdEaXNwdXRlAAAAB9AAAAAKRmx1eGFFcnJvcgAA",
        "AAAAAAAAAAAAAAANY3JlYXRlX3JlZnVuZAAAAAAAAAQAAAAAAAAACnBheW1lbnRfaWQAAAAAABAAAAAAAAAADXJlZnVuZF9hbW91bnQAAAAAAAALAAAAAAAAAAZyZWFzb24AAAAAABAAAAAAAAAACXJlcXVlc3RlcgAAAAAAABMAAAABAAAD6QAAABAAAAfQAAAACkZsdXhhRXJyb3IAAA==",
        "AAAAAAAAAAAAAAALZ2V0X3BheW1lbnQAAAAAAQAAAAAAAAAKcGF5bWVudF9pZAAAAAAAEAAAAAEAAAPpAAAH0AAAAA1QYXltZW50Q2hhcmdlAAAAAAAH0AAAAApGbHV4YUVycm9yAAA=",
        "AAAAAAAAAAAAAAAOY3JlYXRlX2Rpc3B1dGUAAAAAAAUAAAAAAAAACnBheW1lbnRfaWQAAAAAABAAAAAAAAAABmFtb3VudAAAAAAACwAAAAAAAAAGcmVhc29uAAAAAAAQAAAAAAAAAAhldmlkZW5jZQAAABAAAAAAAAAACGRpc3B1dGVyAAAAEwAAAAEAAAPpAAAAEAAAB9AAAAAKRmx1eGFFcnJvcgAA",
        "AAAAAAAAAAAAAAAOcHJvY2Vzc19yZWZ1bmQAAAAAAAIAAAAAAAAACG9wZXJhdG9yAAAAEwAAAAAAAAAJcmVmdW5kX2lkAAAAAAAAEAAAAAEAAAPpAAAD7QAAAAAAAAfQAAAACkZsdXhhRXJyb3IAAA==",
        "AAAAAAAAAAAAAAAOcmVqZWN0X2Rpc3B1dGUAAAAAAAMAAAAAAAAACG9wZXJhdG9yAAAAEwAAAAAAAAAKZGlzcHV0ZV9pZAAAAAAAEAAAAAAAAAAQcmVzb2x1dGlvbl9ub3RlcwAAABAAAAABAAAD6QAAA+0AAAAAAAAH0AAAAApGbHV4YUVycm9yAAA=",
        "AAAAAAAAAAAAAAAOcmV2aWV3X2Rpc3B1dGUAAAAAAAIAAAAAAAAACG9wZXJhdG9yAAAAEwAAAAAAAAAKZGlzcHV0ZV9pZAAAAAAAEAAAAAEAAAPpAAAD7QAAAAAAAAfQAAAACkZsdXhhRXJyb3IAAA==",
        "AAAAAAAAAAAAAAAPcmVmdW5kX2hhc19yb2xlAAAAAAIAAAAAAAAABHJvbGUAAAARAAAAAAAAAAdhY2NvdW50AAAAABMAAAABAAAAAQ==",
        "AAAAAAAAAAAAAAAQcmVmdW5kX2dldF9hZG1pbgAAAAAAAAABAAAD6AAAABM=",
        "AAAAAAAAAAAAAAAOY2FuY2VsX3BheW1lbnQAAAAAAAIAAAAAAAAACWF1dGhvcml0eQAAAAAAABMAAAAAAAAACnBheW1lbnRfaWQAAAAAABAAAAABAAAD6QAAA+0AAAAAAAAH0AAAAApGbHV4YUVycm9yAAA=",
        "AAAAAAAAAAAAAAAOY3JlYXRlX3BheW1lbnQAAAAAAAYAAAAAAAAACnBheW1lbnRfaWQAAAAAABAAAAAAAAAAC21lcmNoYW50X2lkAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAAAAAAIY3VycmVuY3kAAAARAAAAAAAAAA9kZXBvc2l0X2FkZHJlc3MAAAAAEwAAAAAAAAAKZXhwaXJlc19hdAAAAAAABgAAAAEAAAPpAAAH0AAAAA1QYXltZW50Q2hhcmdlAAAAAAAH0AAAAApGbHV4YUVycm9yAAA=",
        "AAAAAAAAAAAAAAAOZXhwaXJlX3BheW1lbnQAAAAAAAEAAAAAAAAACnBheW1lbnRfaWQAAAAAABAAAAABAAAD6QAAA+0AAAAAAAAH0AAAAApGbHV4YUVycm9yAAA=",
        "AAAAAAAAAAAAAAAOc2V0dGxlX3BheW1lbnQAAAAAAAMAAAAAAAAACG9wZXJhdG9yAAAAEwAAAAAAAAAKcGF5bWVudF9pZAAAAAAAEAAAAAAAAAAQdHJlYXN1cnlfYWRkcmVzcwAAABMAAAABAAAD6QAAA+0AAAAAAAAH0AAAAApGbHV4YUVycm9yAAA=",
        "AAAAAAAAAAAAAAAOdmVyaWZ5X3BheW1lbnQAAAAAAAUAAAAAAAAABm9yYWNsZQAAAAAAEwAAAAAAAAAKcGF5bWVudF9pZAAAAAAAEAAAAAAAAAAQdHJhbnNhY3Rpb25faGFzaAAAA+4AAAAgAAAAAAAAAA1wYXllcl9hZGRyZXNzAAAAAAAAEwAAAAAAAAAPYW1vdW50X3JlY2VpdmVkAAAAAAsAAAABAAAD6QAAB9AAAAANUGF5bWVudFN0YXR1cwAAAAAAB9AAAAAKRmx1eGFFcnJvcgAA",
        "AAAAAAAAAAAAAAARcmVmdW5kX2dyYW50X3JvbGUAAAAAAAADAAAAAAAAAAVhZG1pbgAAAAAAABMAAAAAAAAABHJvbGUAAAARAAAAAAAAAAdhY2NvdW50AAAAABMAAAABAAAD6QAAA+0AAAAAAAAH0AAAAApGbHV4YUVycm9yAAA=",
        "AAAAAAAAAAAAAAAScmVmdW5kX3Jldm9rZV9yb2xlAAAAAAADAAAAAAAAAAVhZG1pbgAAAAAAABMAAAAAAAAABHJvbGUAAAARAAAAAAAAAAdhY2NvdW50AAAAABMAAAABAAAD6QAAA+0AAAAAAAAH0AAAAApGbHV4YUVycm9yAAA=",
        "AAAAAAAAAAAAAAATZ2V0X3BheW1lbnRfcmVmdW5kcwAAAAABAAAAAAAAAApwYXltZW50X2lkAAAAAAAQAAAAAQAAA+kAAAPqAAAH0AAAAAZSZWZ1bmQAAAAAB9AAAAAKRmx1eGFFcnJvcgAA",
        "AAAAAAAAAAAAAAAUZ2V0X3BheW1lbnRfZGlzcHV0ZXMAAAABAAAAAAAAAApwYXltZW50X2lkAAAAAAAQAAAAAQAAA+kAAAPqAAAH0AAAAAdEaXNwdXRlAAAAB9AAAAAKRmx1eGFFcnJvcgAA",
        "AAAAAAAAAAAAAAAUcmVmdW5kX3Jlbm91bmNlX3JvbGUAAAACAAAAAAAAAAdhY2NvdW50AAAAABMAAAAAAAAABHJvbGUAAAARAAAAAQAAA+kAAAPtAAAAAAAAB9AAAAAKRmx1eGFFcnJvcgAA",
        "AAAAAAAAAAAAAAAScGF5bWVudF9ncmFudF9yb2xlAAAAAAADAAAAAAAAAAVhZG1pbgAAAAAAABMAAAAAAAAABHJvbGUAAAARAAAAAAAAAAdhY2NvdW50AAAAABMAAAABAAAD6QAAA+0AAAAAAAAH0AAAAApGbHV4YUVycm9yAAA=",
        "AAAAAAAAAAAAAAAVcmVmdW5kX3RyYW5zZmVyX2FkbWluAAAAAAAAAgAAAAAAAAANY3VycmVudF9hZG1pbgAAAAAAABMAAAAAAAAACW5ld19hZG1pbgAAAAAAABMAAAABAAAD6QAAA+0AAAAAAAAH0AAAAApGbHV4YUVycm9yAAA=",
        "AAAAAAAAAAAAAAAVZ2V0X21lcmNoYW50X3BheW1lbnRzAAAAAAAAAQAAAAAAAAALbWVyY2hhbnRfaWQAAAAAEwAAAAEAAAPqAAAAEA==",
        "AAAAAAAAAAAAAAAZaW5pdGlhbGl6ZV9yZWZ1bmRfbWFuYWdlcgAAAAAAAAIAAAAAAAAABWFkbWluAAAAAAAAEwAAAAAAAAASdXNkY190b2tlbl9hZGRyZXNzAAAAAAATAAAAAA==",
        "AAAAAAAAAAAAAAAbcmVzb2x2ZV9kaXNwdXRlX3dpdGhfcmVmdW5kAAAAAAMAAAAAAAAACG9wZXJhdG9yAAAAEwAAAAAAAAAKZGlzcHV0ZV9pZAAAAAAAEAAAAAAAAAAQcmVzb2x1dGlvbl9ub3RlcwAAABAAAAABAAAD6QAAABAAAAfQAAAACkZsdXhhRXJyb3IAAA==",
        "AAAAAAAAAAAAAAAcaW5pdGlhbGl6ZV9wYXltZW50X3Byb2Nlc3NvcgAAAAEAAAAAAAAABWFkbWluAAAAAAAAEwAAAAA=",
        "AAAAAAAAAAAAAAAfZ2V0X21lcmNoYW50X3BheW1lbnRzX3BhZ2luYXRlZAAAAAADAAAAAAAAAAttZXJjaGFudF9pZAAAAAATAAAAAAAAAAZvZmZzZXQAAAAAAAQAAAAAAAAABWxpbWl0AAAAAAAABAAAAAEAAAPqAAAAEA==",
        "AAAAAQAAAAAAAAAAAAAACFJhdGVEYXRhAAAABAAAAAAAAAAIZGVjaW1hbHMAAAAEAAAAAAAAAARwYWlyAAAAEQAAAAAAAAAEcmF0ZQAAAAsAAAAAAAAACnVwZGF0ZWRfYXQAAAAAAAY=",
        "AAAAAAAAAAAAAAAIZ2V0X3JhdGUAAAABAAAAAAAAAARwYWlyAAAAEQAAAAEAAAPpAAAH0AAAAAhSYXRlRGF0YQAAB9AAAAANRlhPcmFjbGVFcnJvcgAAAA==",
        "AAAAAAAAAAAAAAAIc2V0X3JhdGUAAAAEAAAAAAAAAAhvcGVyYXRvcgAAABMAAAAAAAAABHBhaXIAAAARAAAAAAAAAARyYXRlAAAACwAAAAAAAAAIZGVjaW1hbHMAAAAEAAAAAQAAA+kAAAPtAAAAAAAAB9AAAAANRlhPcmFjbGVFcnJvcgAAAA==",
        "AAAABAAAAAAAAAAAAAAADUZYT3JhY2xlRXJyb3IAAAAAAAADAAAAAAAAAAxSYXRlTm90Rm91bmQAAAABAAAAAAAAAAlSYXRlU3RhbGUAAAAAAAACAAAAAAAAAAxVbmF1dGhvcml6ZWQAAAAD",
        "AAAAAgAAAAAAAAAAAAAADU9yYWNsZURhdGFLZXkAAAAAAAACAAAAAQAAAAAAAAAEUmF0ZQAAAAEAAAARAAAAAAAAAAAAAAASU3RhbGVuZXNzVGhyZXNob2xkAAA=",
        "AAAAAAAAAAAAAAAPb3JhY2xlX2hhc19yb2xlAAAAAAIAAAAAAAAABHJvbGUAAAARAAAAAAAAAAdhY2NvdW50AAAAABMAAAABAAAAAQ==",
        "AAAAAAAAAAAAAAAQZ2V0X29yYWNsZV9hZG1pbgAAAAAAAAABAAAD6AAAABM=",
        "AAAAAAAAAAAAAAARb3JhY2xlX2dyYW50X3JvbGUAAAAAAAADAAAAAAAAAAVhZG1pbgAAAAAAABMAAAAAAAAABHJvbGUAAAARAAAAAAAAAAdhY2NvdW50AAAAABMAAAABAAAD6QAAA+0AAAAAAAAH0AAAAA1GWE9yYWNsZUVycm9yAAAA",
        "AAAAAAAAAAAAAAARb3JhY2xlX2luaXRpYWxpemUAAAAAAAACAAAAAAAAAAVhZG1pbgAAAAAAABMAAAAAAAAAE3N0YWxlbmVzc190aHJlc2hvbGQAAAAABgAAAAA=",
        "AAAAAAAAAAAAAAAVZ2V0X3NldHRsZW1lbnRfYW1vdW50AAAAAAAAAgAAAAAAAAALdXNkY19hbW91bnQAAAAACwAAAAAAAAAPdGFyZ2V0X2N1cnJlbmN5AAAAABEAAAABAAAD6QAAAAsAAAfQAAAADUZYT3JhY2xlRXJyb3IAAAA=",
        "AAAAAAAAAAAAAAAXZ2V0X3N0YWxlbmVzc190aHJlc2hvbGQAAAAAAAAAAAEAAAAG",
        "AAAAAAAAAAAAAAAXc2V0X3N0YWxlbmVzc190aHJlc2hvbGQAAAAAAgAAAAAAAAAFYWRtaW4AAAAAAAATAAAAAAAAAAl0aHJlc2hvbGQAAAAAAAAGAAAAAQAAA+kAAAPtAAAAAAAAB9AAAAANRlhPcmFjbGVFcnJvcgAAAA==",
      ]),
      options,
    );
  }
  public readonly fromJSON = {
    get_merchant: (json: string) => (this as any).txFromJSON(json),
    update_merchant: (json: string) => (this as any).txFromJSON(json),
    verify_merchant: (json: string) => (this as any).txFromJSON(json),
    register_merchant: (json: string) => (this as any).txFromJSON(json),
    merchant_initialize: (json: string) => (this as any).txFromJSON(json),
    get_refund: (json: string) => (this as any).txFromJSON(json),
    get_dispute: (json: string) => (this as any).txFromJSON(json),
    create_refund: (json: string) => (this as any).txFromJSON(json),
    get_payment: (json: string) => (this as any).txFromJSON(json),
    create_dispute: (json: string) => (this as any).txFromJSON(json),
    process_refund: (json: string) => (this as any).txFromJSON(json),
    reject_dispute: (json: string) => (this as any).txFromJSON(json),
    review_dispute: (json: string) => (this as any).txFromJSON(json),
    refund_has_role: (json: string) => (this as any).txFromJSON(json),
    refund_get_admin: (json: string) => (this as any).txFromJSON(json),
    cancel_payment: (json: string) => (this as any).txFromJSON(json),
    create_payment: (json: string) => (this as any).txFromJSON(json),
    expire_payment: (json: string) => (this as any).txFromJSON(json),
    settle_payment: (json: string) => (this as any).txFromJSON(json),
    verify_payment: (json: string) => (this as any).txFromJSON(json),
    refund_grant_role: (json: string) => (this as any).txFromJSON(json),
    refund_revoke_role: (json: string) => (this as any).txFromJSON(json),
    get_payment_refunds: (json: string) => (this as any).txFromJSON(json),
    get_payment_disputes: (json: string) => (this as any).txFromJSON(json),
    refund_renounce_role: (json: string) => (this as any).txFromJSON(json),
    payment_grant_role: (json: string) => (this as any).txFromJSON(json),
    refund_transfer_admin: (json: string) => (this as any).txFromJSON(json),
    get_merchant_payments: (json: string) => (this as any).txFromJSON(json),
    initialize_refund_manager: (json: string) => (this as any).txFromJSON(json),
    resolve_dispute_with_refund: (json: string) =>
      (this as any).txFromJSON(json),
    initialize_payment_processor: (json: string) =>
      (this as any).txFromJSON(json),
    get_merchant_payments_paginated: (json: string) =>
      (this as any).txFromJSON(json),
    get_rate: (json: string) => (this as any).txFromJSON(json),
    set_rate: (json: string) => (this as any).txFromJSON(json),
    oracle_has_role: (json: string) => (this as any).txFromJSON(json),
    get_oracle_admin: (json: string) => (this as any).txFromJSON(json),
    oracle_grant_role: (json: string) => (this as any).txFromJSON(json),
    oracle_initialize: (json: string) => (this as any).txFromJSON(json),
    get_settlement_amount: (json: string) => (this as any).txFromJSON(json),
    get_staleness_threshold: (json: string) => (this as any).txFromJSON(json),
    set_staleness_threshold: (json: string) => (this as any).txFromJSON(json),
  };
}
