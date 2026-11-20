/**
 * Issue #962: merchant payout channel configuration.
 *
 * POST   /v1/merchants/:id/payout-channels  — add a payout channel
 * GET    /v1/merchants/:id/payout-channels  — list payout channels
 *
 * Account numbers are stored encrypted (AES-256-GCM) using the key supplied
 * in config. The settlement service reads the default channel at payout time.
 */

import { Router, Request, Response } from "express";
import { createCipheriv, createDecipheriv, randomBytes } from "crypto";
import {
  validateBody,
  sanitizeStrings,
  createPayoutChannelSchema,
} from "../middleware/validateBody";

export interface PayoutChannelsRouterConfig {
  encryptionKey: string;
}

// ---------------------------------------------------------------------------
// In-memory store (replace with DB in production)
// ---------------------------------------------------------------------------

interface StoredChannel {
  id: string;
  merchantId: string;
  type: string;
  encryptedAccountNumber: string; // hex: iv(24) + authTag(32) + ciphertext
  routingCode?: string;
  bankName?: string;
  currency: string;
  isDefault: boolean;
  createdAt: string;
}

const channelStore = new Map<string, StoredChannel[]>();

// ---------------------------------------------------------------------------
// Encryption helpers
// ---------------------------------------------------------------------------

function deriveKey(secret: string): Buffer {
  // Simple fixed-length key derivation — use a proper KDF in production.
  const buf = Buffer.alloc(32, 0);
  Buffer.from(secret, "utf8").copy(buf);
  return buf;
}

function encrypt(plaintext: string, keySecret: string): string {
  const key = deriveKey(keySecret);
  const iv = randomBytes(12);
  const cipher = createCipheriv("aes-256-gcm", key, iv);
  const encrypted = Buffer.concat([
    cipher.update(plaintext, "utf8"),
    cipher.final(),
  ]);
  const tag = cipher.getAuthTag();
  return Buffer.concat([iv, tag, encrypted]).toString("hex");
}

function decrypt(hex: string, keySecret: string): string {
  const buf = Buffer.from(hex, "hex");
  const iv = buf.subarray(0, 12);
  const tag = buf.subarray(12, 28);
  const ciphertext = buf.subarray(28);
  const key = deriveKey(keySecret);
  const decipher = createDecipheriv("aes-256-gcm", key, iv);
  decipher.setAuthTag(tag);
  return Buffer.concat([
    decipher.update(ciphertext),
    decipher.final(),
  ]).toString("utf8");
}

// ---------------------------------------------------------------------------
// Router factory
// ---------------------------------------------------------------------------

export function createPayoutChannelsRouter(
  config: PayoutChannelsRouterConfig,
): Router {
  const router = Router();

  /**
   * POST /v1/merchants/:id/payout-channels
   * Add a new payout channel for the merchant.
   */
  router.post(
    "/:id/payout-channels",
    sanitizeStrings(),
    validateBody(createPayoutChannelSchema),
    (req: Request, res: Response) => {
      const merchantId = req.params.id;
      const {
        type,
        account_number,
        routing_code,
        bank_name,
        currency,
        is_default,
      } = req.body;

      const channels = channelStore.get(merchantId) ?? [];

      // If new channel is default, clear existing defaults.
      if (is_default) {
        for (const ch of channels) ch.isDefault = false;
      }

      const channel: StoredChannel = {
        id: randomBytes(8).toString("hex"),
        merchantId,
        type,
        encryptedAccountNumber: encrypt(account_number, config.encryptionKey),
        routingCode: routing_code,
        bankName: bank_name,
        currency,
        isDefault: Boolean(is_default),
        createdAt: new Date().toISOString(),
      };

      channels.push(channel);
      channelStore.set(merchantId, channels);

      return res.status(201).json({
        id: channel.id,
        merchant_id: merchantId,
        type: channel.type,
        // Return masked account number for confirmation.
        account_number_last4: account_number.slice(-4),
        currency: channel.currency,
        is_default: channel.isDefault,
        created_at: channel.createdAt,
      });
    },
  );

  /**
   * GET /v1/merchants/:id/payout-channels
   * List the merchant's configured payout channels (account numbers masked).
   */
  router.get("/:id/payout-channels", (req: Request, res: Response) => {
    const merchantId = req.params.id;
    const channels = channelStore.get(merchantId) ?? [];

    const data = channels.map((ch) => {
      let last4 = "****";
      try {
        const plain = decrypt(ch.encryptedAccountNumber, config.encryptionKey);
        last4 = plain.slice(-4);
      } catch {
        // Decryption failed — return masked placeholder.
      }
      return {
        id: ch.id,
        type: ch.type,
        account_number_last4: last4,
        routing_code: ch.routingCode ?? null,
        bank_name: ch.bankName ?? null,
        currency: ch.currency,
        is_default: ch.isDefault,
        created_at: ch.createdAt,
      };
    });

    return res.status(200).json({ data, total: data.length });
  });

  return router;
}
