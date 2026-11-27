import { Router, Request, Response } from 'express';
import { z } from 'zod';
import { prisma } from '../lib/prisma';
import { requireAuth } from '../middleware/auth';
import { logger } from '../lib/logger';

const router = Router();

const REDELIVER_LIMIT = 10;
const REDELIVER_WINDOW_MS = 60 * 60 * 1000;

const listDeliveriesQuery = z.object({
  limit: z.coerce.number().int().min(1).max(100).default(50),
  page: z.coerce.number().int().min(1).default(1),
});

/**
 * GET /v1/webhooks/deliveries
 * Lists the authenticated merchant's recent webhook delivery attempts.
 */
router.get('/deliveries', requireAuth, async (req: Request, res: Response) => {
  const parsed = listDeliveriesQuery.safeParse(req.query);
  if (!parsed.success) {
    return res.status(400).json({ error: 'invalid_query', details: parsed.error.flatten() });
  }

  const { limit, page } = parsed.data;
  const merchantId = req.merchant!.id;

  try {
    const [deliveries, total] = await Promise.all([
      prisma.webhookDeliveryLog.findMany({
        where: { merchantId },
        orderBy: { createdAt: 'desc' },
        skip: (page - 1) * limit,
        take: limit,
        select: {
          id: true,
          eventType: true,
          endpointUrl: true,
          statusCode: true,
          attemptCount: true,
          deliveredAt: true,
          payload: true,
          createdAt: true,
        },
      }),
      prisma.webhookDeliveryLog.count({ where: { merchantId } }),
    ]);

    return res.json({
      data: deliveries,
      pagination: {
        page,
        limit,
        total,
        totalPages: Math.ceil(total / limit),
      },
    });
  } catch (err) {
    logger.error({ err, merchantId }, 'failed to list webhook deliveries');
    return res.status(500).json({ error: 'internal_error' });
  }
});

/**
 * POST /v1/webhooks/deliveries/:id/redeliver
 * Triggers a one-time redelivery of a failed webhook.
 * Rate-limited to 10 redeliveries per hour per merchant.
 */
router.post('/deliveries/:id/redeliver', requireAuth, async (req: Request, res: Response) => {
  const merchantId = req.merchant!.id;
  const { id } = req.params;

  try {
    const delivery = await prisma.webhookDeliveryLog.findFirst({
      where: { id, merchantId },
    });

    if (!delivery) {
      return res.status(404).json({ error: 'delivery_not_found' });
    }

    const windowStart = new Date(Date.now() - REDELIVER_WINDOW_MS);
    const recentRedeliveries = await prisma.webhookDeliveryLog.count({
      where: {
        merchantId,
        redeliveredAt: { gte: windowStart },
      },
    });

    if (recentRedeliveries >= REDELIVER_LIMIT) {
      return res.status(429).json({
        error: 'rate_limited',
        message: `Redelivery is limited to ${REDELIVER_LIMIT} per hour per merchant.`,
      });
    }

    const updated = await prisma.webhookDeliveryLog.update({
      where: { id: delivery.id },
      data: {
        redeliveredAt: new Date(),
        attemptCount: { increment: 1 },
      },
    });

    // Enqueue the redelivery job for the worker to process.
    await prisma.webhookDeliveryJob.create({
      data: {
        deliveryLogId: delivery.id,
        merchantId,
        endpointUrl: delivery.endpointUrl,
        eventType: delivery.eventType,
        payload: delivery.payload,
        status: 'pending',
      },
    });

    return res.status(202).json({
      data: {
        id: updated.id,
        status: 'queued',
        attemptCount: updated.attemptCount,
        redeliveredAt: updated.redeliveredAt,
      },
    });
  } catch (err) {
    logger.error({ err, merchantId, deliveryId: id }, 'failed to redeliver webhook');
    return res.status(500).json({ error: 'internal_error' });
  }
});

export default router;
