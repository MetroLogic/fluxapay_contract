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

// Issue #960: opaque base64-encoded JSON cursor { created_at, payment_id }.
function decodeCursor(cursor: string): { created_at: string; payment_id: string } | null {
  try {
    const parsed = JSON.parse(Buffer.from(cursor, 'base64').toString('utf8'));
    if (parsed && typeof parsed.created_at === 'string' && typeof parsed.payment_id === 'string') {
      return { created_at: parsed.created_at, payment_id: parsed.payment_id };
    }
    return null;
  } catch {
    return null;
  }
}

function encodeCursor(row: { created_at: unknown; payment_id: unknown }): string {
  return Buffer.from(
    JSON.stringify({ created_at: row.created_at, payment_id: row.payment_id }),
    'utf8',
  ).toString('base64');
}

router.get('/v1/transactions', async (req: Request, res: Response) => {
  const limit = parsePositiveInt(req.query.limit, DEFAULT_LIMIT);
  if (limit === null) {
    return res.status(400).json({ error: 'Invalid `limit` parameter: must be a positive integer' });
  }
  if (limit > MAX_LIMIT) {
    return res.status(400).json({ error: `Invalid \`limit\` parameter: must not exceed ${MAX_LIMIT}` });
  }

  const merchantId = typeof req.query.merchant_id === 'string' ? req.query.merchant_id : undefined;

  // Cursor mode (Issue #960): WHERE (created_at, payment_id) < (cursor_time, cursor_id)
  // is served from the index and stays fast at high offsets. page/offset below stay for compat.
  if (typeof req.query.after === 'string' && req.query.after !== '') {
    const cursor = decodeCursor(req.query.after);
    if (!cursor) {
      return res.status(400).json({ error: 'Invalid `after` cursor: expected base64-encoded JSON { created_at, payment_id }' });
    }

    let where = 'WHERE (created_at, payment_id) < ($1, $2)';
    const params: unknown[] = [cursor.created_at, cursor.payment_id];
    if (merchantId) {
      where = 'WHERE merchant_id = $1 AND (created_at, payment_id) < ($2, $3)';
      params.unshift(merchantId);
    }
    params.push(limit);

    const dataResult = await db.query(
      `SELECT * FROM transactions ${where} ORDER BY created_at DESC, payment_id DESC LIMIT $${params.length}`,
      params,
    );
    const rows = dataResult.rows;
    const hasMore = rows.length === limit;
    const nextCursor = hasMore && rows.length > 0 ? encodeCursor(rows[rows.length - 1]) : null;
    return res.json({ data: rows, next_cursor: nextCursor, has_more: hasMore });
  }

  // Backwards-compatible offset pagination.
  const page = parsePositiveInt(req.query.page, DEFAULT_PAGE);
  if (page === null) {
    return res.status(400).json({ error: 'Invalid `page` parameter: must be a positive integer' });
  }

  const offset = (page - 1) * limit;
  const whereClause = merchantId ? 'WHERE merchant_id = $1' : '';
  const filterParams = merchantId ? [merchantId] : [];

  const countResult = await db.query(
    `SELECT COUNT(*)::int AS total FROM transactions ${whereClause}`,
    filterParams,
  );
  const total: number = countResult.rows[0]?.total ?? 0;

  const dataResult = await db.query(
    `SELECT * FROM transactions ${whereClause}
     ORDER BY created_at DESC
     LIMIT $${filterParams.length + 1} OFFSET $${filterParams.length + 2}`,
    [...filterParams, limit, offset],
  );

  const totalPages = total === 0 ? 0 : Math.ceil(total / limit);
  return res.json({ data: dataResult.rows, page, limit, total, total_pages: totalPages });
});

export default router;
