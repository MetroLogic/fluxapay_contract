# Sponsored meta-transactions

A merchant can authorize a payment action off-chain. A relayer submits that
authorization and pays the network fee, so the merchant does not need a funded
account to create a charge, authorize a refund, rotate a webhook key, or set a
fee recipient.

The relayer calls `execute_meta_transaction(relayer, payload, signature)` on
`PaymentProcessor`. The contract checks the payload, verifies the merchant's
Ed25519 signature, consumes a per-merchant nonce, and runs the action as that
merchant.

## Payload

`MetaTransactionPayload`:

| Field | Type | Meaning |
| --- | --- | --- |
| `merchant_id` | `Address` | Merchant account (`G...`). The signature must come from this account's Ed25519 key. |
| `target_function` | `Symbol` | Action name. See the table below. |
| `call_data` | `Bytes` | XDR encoding of the action arguments. |
| `nonce` | `u64` | Next sequence number for this merchant. The first action uses `0`. |
| `deadline` | `u64` | Ledger timestamp. The call is accepted while `timestamp <= deadline`. |
| `domain_separator` | `BytesN<32>` | Domain the merchant signed. Must match the contract's separator. |

Signing with the `G...` account key is the meta-transaction identity. It is
separate from Stellar signer weights on that account.

## Domain separator

```text
SHA-256(utf8 "FluxaPay" || contract_id_hash || network_id)
```

`contract_id_hash` is the 32-byte hash inside the contract address.
`network_id` is SHA-256 of the network passphrase (`env.ledger().network_id()`).
A signature for one contract or network is rejected on any other with
`InvalidDomain`, before the signature is checked.

## Signed bytes

The Ed25519 message is a single SHA-256 digest. The contract does not hash
that digest again.

```text
domain_separator (32)
|| merchant_pubkey (32)
|| u32be(name_len) || utf8 function name
|| u32be(call_data_len) || call_data
|| u64be(nonce)
|| u64be(deadline)
```

Integers are big-endian. `merchant_pubkey` is the raw Ed25519 public key of
`merchant_id`.

## Nonce

Each merchant has one strictly increasing sequence stored in persistent
storage, starting at `0`. A payload whose nonce is not the current value is
rejected with `InvalidNonce` and is not consumed. After a valid signature the
nonce advances by one, including when the inner action fails, so a failing
payload cannot be replayed. Nonce entries are bumped for about a year
(6,307,200 ledgers).

Envelope errors (expired deadline, wrong domain, unknown function, signature
failure, wrong nonce) do not consume the nonce. A signature that does not
match `merchant_id` traps inside `ed25519_verify`.

## Actions

| `target_function` | `call_data` |
| --- | --- |
| `create_charge` | XDR of `CreatePaymentArgs`. `args.merchant_id` must equal `payload.merchant_id`. |
| `authorize_refund` | XDR of `RefundAuthRequest { payment_id, refund_amount, reason }`. |
| `rotate_webhook_key` | XDR of `BytesN<32>`. |
| `set_fee_recipient` | XDR of `Address`. |

The same four actions are also direct contract methods (`create_payment`,
`authorize_refund`, `rotate_webhook_key`, `set_fee_recipient`). A direct call
still requires the merchant's Soroban authorization. A meta-transaction
supplies that authorization from the verified signature, so only the relayer
authorizes the outer call.

`create_charge` runs the existing `create_payment` checks (merchant role,
amount, payment id, expiry, rate limit, and the rest). The meta-transaction
does not skip them.

## Events

Both events use topics `("META_TX", action)` and the data tuple
`(relayer, merchant_id, target_function, nonce, gas)`.

| Action | When |
| --- | --- |
| `EXECUTED` | The inner action succeeded. |
| `FAILED` | The signature was valid and the nonce was consumed, but the inner action failed. |

`gas` is a deterministic resource estimate (instruction units of 10,000,
ledger reads, ledger writes, events, and the resulting resource fee in
stroops). It uses the same fee schedule as `GasEstimator`.

## TypeScript

`sdk/src/metaTx.ts` builds the payload, the domain separator, and the digest,
and signs with a Stellar `Keypair`:

```ts
import { Keypair } from "@stellar/stellar-sdk";
import {
  META_TX_FUNCTIONS,
  buildMetaTransactionPayload,
  signMetaTransaction,
} from "@fluxapay/sdk";

const merchant = Keypair.fromSecret("S...");
const payload = buildMetaTransactionPayload({
  merchantId: merchant.publicKey(),
  targetFunction: META_TX_FUNCTIONS.createCharge,
  callData, // XDR bytes of CreatePaymentArgs
  nonce: 0n,
  deadline: BigInt(Math.floor(Date.now() / 1000) + 600),
  contractId, // C...
  networkPassphrase,
});
const signature = signMetaTransaction(payload, merchant);
```

The relayer then calls `execute_meta_transaction` with `payload` and the
64-byte `signature`.
