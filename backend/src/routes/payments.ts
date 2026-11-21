import { Router, Request, Response, NextFunction } from 'express';
import { z } from 'zod';
import { prisma } from '../lib/prisma';
import { createPaymentOnChain } from '../services/soroban';
import { logger } from '../lib/logger';

const router = Router();

const UUID_REGEX =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

const IDEMPOTENCY_TTL_MS = 24 * 60 * 60 * 1000;

const createPaymentSchema = z.object({
  amount: z.number().int().positive(),
  currency: z.string().min(1),
  orderId: z.string().min(1),
  clientToken: z.string().optional(),
});

function extractIdempotencyKey(req: Request): string | null {
  const raw = req.header('Idempotency-Key');
  if (!raw) return null;
  const trimmed = raw.trim();
  if (!UUID_REGEX.test(trimmed)) {
    logger.warn({ key: trimmed }, 'Ignoring invalid Idempotency-Key header');
    return null;
  }
  return trimmed.toLowerCase();
}

router.post('/v1/payments', async (req: Request, res: Response, next: NextFunction) => {
  try {
    const parsed = createPaymentSchema.safeParse(req.body);
    if (!parsed.success) {
      return res.status(400).json({ error: 'Invalid request body', details: parsed.error.flatten() });
    }

    const idempotencyKey = extractIdempotencyKey(req);

    if (idempotencyKey) {
      const existing = await prisma.idempotencyKey.findUnique({
        where: { key: idempotencyKey },
      });

      if (existing && existing.expiresAt.getTime() > Date.now()) {
        res.setHeader('Idempotency-Replayed', 'true');
        return res.status(200).json(existing.responseBody);
      }

      if (existing) {
        await prisma.idempotencyKey.delete({ where: { key: idempotencyKey } });
      }
    }

    const payment = await createPaymentOnChain({
      amount: parsed.data.amount,
      currency: parsed.data.currency,
      orderId: parsed.data.orderId,
      clientToken: parsed.data.clientToken ?? idempotencyKey ?? undefined,
    });

    const responseBody = {
      id: payment.id,
      orderId: payment.orderId,
      amount: payment.amount,
      currency: payment.currency,
      status: payment.status,
      createdAt: payment.createdAt,
    };

    if (idempotencyKey) {
      const expiresAt = new Date(Date.now() + IDEMPOTENCY_TTL_MS);
      await prisma.idempotencyKey.upsert({
        where: { key: idempotencyKey },
        create: {
          key: idempotencyKey,
          responseBody,
          statusCode: 201,
          expiresAt,
        },
        update: {
          responseBody,
          statusCode: 201,
          expiresAt,
        },
      });
    }

    return res.status(201).json(responseBody);
  } catch (err) {
    return next(err);
  }
});

export default router;
