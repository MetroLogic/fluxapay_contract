import { Router, Request, Response } from 'express';
import { db } from '../db';

const router = Router();

const DEFAULT_PAGE = 1;
const DEFAULT_LIMIT = 25;
const MAX_LIMIT = 100;

function parsePositiveInt(value: unknown, fallback: number): number | null {
  if (value === undefined || value === null || value === '') return fallback;
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 1) return null;
  return parsed;
}

router.get('/v1/transactions', async (req: Request, res: Response) => {
  const page = parsePositiveInt(req.query.page, DEFAULT_PAGE);
  if (page === null) {
    return res.status(400).json({ error: 'Invalid `page` parameter: must be a positive integer' });
  }

  const limit = parsePositiveInt(req.query.limit, DEFAULT_LIMIT);
  if (limit === null) {
    return res.status(400).json({ error: 'Invalid `limit` parameter: must be a positive integer' });
  }
  if (limit > MAX_LIMIT) {
    return res.status(400).json({ error: `Invalid \`limit\` parameter: must not exceed ${MAX_LIMIT}` });
  }

  const merchantId = typeof req.query.merchant_id === 'string' ? req.query.merchant_id : undefined;
  const offset = (page - 1) * limit;

  const whereClause = merchantId ? 'WHERE merchant_id = $1' : '';
  const filterParams = merchantId ? [merchantId] : [];

  const countResult = await db.query(
    `SELECT COUNT(*)::int AS total FROM transactions ${whereClause}`,
    filterParams
  );
  const total: number = countResult.rows[0]?.total ?? 0;

  const dataResult = await db.query(
    `SELECT * FROM transactions ${whereClause}
     ORDER BY created_at DESC
     LIMIT $${filterParams.length + 1} OFFSET $${filterParams.length + 2}`,
    [...filterParams, limit, offset]
  );

  const totalPages = total === 0 ? 0 : Math.ceil(total / limit);

  return res.json({
    data: dataResult.rows,
    page,
    limit,
    total,
    total_pages: totalPages,
  });
});

export default router;
