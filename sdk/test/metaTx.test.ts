import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { Keypair, StrKey, hash } from "@stellar/stellar-sdk";

import {
  META_TX_FUNCTIONS,
  buildMetaTransactionPayload,
  domainSeparator,
  signMetaTransaction,
  signingDigest,
  signingPreimage,
} from "../src/metaTx.js";

function u32be(value: number): Buffer {
  const buf = Buffer.alloc(4);
  buf.writeUInt32BE(value);
  return buf;
}

function u64be(value: bigint): Buffer {
  const buf = Buffer.alloc(8);
  buf.writeBigUInt64BE(value);
  return buf;
}

describe("sponsored meta-transactions (Issue #893)", () => {
  const merchant = Keypair.random();
  const contractHash = Buffer.alloc(32, 0x22);
  const contractId = StrKey.encodeContract(contractHash);
  const passphrase = "Test SDF Network ; September 2015";

  test("domain separator binds the name, contract, and network", () => {
    const separator = domainSeparator(contractId, passphrase);
    const expected = hash(
      Buffer.concat([
        Buffer.from("FluxaPay", "utf8"),
        contractHash,
        hash(Buffer.from(passphrase, "utf8")),
      ]),
    );
    assert.equal(separator.length, 32);
    assert.deepEqual(separator, expected);

    const otherContract = StrKey.encodeContract(Buffer.alloc(32, 0x23));
    assert.notDeepEqual(domainSeparator(otherContract, passphrase), separator);
    assert.notDeepEqual(
      domainSeparator(contractId, "Public Global Stellar Network ; September 2015"),
      separator,
    );
  });

  test("signing preimage matches the canonical byte layout", () => {
    const callData = Buffer.from([1, 2, 3]);
    const payload = buildMetaTransactionPayload({
      merchantId: merchant.publicKey(),
      targetFunction: META_TX_FUNCTIONS.rotateWebhookKey,
      callData,
      nonce: 7n,
      deadline: 99n,
      contractId,
      networkPassphrase: passphrase,
    });

    const name = Buffer.from("rotate_webhook_key", "utf8");
    const expected = Buffer.concat([
      payload.domainSeparator,
      Buffer.from(StrKey.decodeEd25519PublicKey(merchant.publicKey())),
      u32be(name.length),
      name,
      u32be(callData.length),
      callData,
      u64be(7n),
      u64be(99n),
    ]);
    assert.deepEqual(signingPreimage(payload), expected);
    assert.equal(name.length, 18);

    const digest = signingDigest(payload);
    assert.deepEqual(digest, createHash("sha256").update(expected).digest());
    assert.equal(digest.length, 32);
  });

  test("merchant keypair signs the digest and a different key does not verify", () => {
    const payload = buildMetaTransactionPayload({
      merchantId: merchant.publicKey(),
      targetFunction: META_TX_FUNCTIONS.createCharge,
      callData: Buffer.from("charge"),
      nonce: 0n,
      deadline: 1_700_000_600n,
      contractId,
      networkPassphrase: passphrase,
    });
    const signature = signMetaTransaction(payload, merchant);
    const digest = signingDigest(payload);
    assert.equal(signature.length, 64);
    assert.equal(merchant.verify(digest, signature), true);

    const forged = Buffer.from(signature);
    forged[0] ^= 0xff;
    assert.equal(merchant.verify(digest, forged), false);

    const other = Keypair.random();
    assert.throws(() => signMetaTransaction(payload, other));
  });
});
