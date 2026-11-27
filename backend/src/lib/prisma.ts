/**
 * Prisma client stub.
 *
 * The backend does not yet depend on a live database. This module exports a
 * mock `prisma` object that satisfies the call sites in webhooks.ts so the
 * project compiles without installing @prisma/client.
 *
 * Replace this file with:
 *   import { PrismaClient } from '@prisma/client';
 *   export const prisma = new PrismaClient();
 * once the database is provisioned.
 */

type WhereClause = Record<string, unknown>;
type SelectClause = Record<string, unknown>;
type OrderByClause = Record<string, unknown>;

interface FindManyArgs {
  where?: WhereClause;
  orderBy?: OrderByClause;
  skip?: number;
  take?: number;
  select?: SelectClause;
}

interface FindFirstArgs {
  where?: WhereClause;
}

interface CountArgs {
  where?: WhereClause;
}

interface UpdateArgs {
  where: WhereClause;
  data: Record<string, unknown>;
}

interface CreateArgs {
  data: Record<string, unknown>;
}

function makeModel(name: string) {
  return {
    findMany: async (_args?: FindManyArgs): Promise<unknown[]> => {
      console.warn(`[prisma-stub] ${name}.findMany called — no database configured`);
      return [];
    },
    findFirst: async (_args?: FindFirstArgs): Promise<unknown | null> => {
      console.warn(`[prisma-stub] ${name}.findFirst called — no database configured`);
      return null;
    },
    count: async (_args?: CountArgs): Promise<number> => {
      console.warn(`[prisma-stub] ${name}.count called — no database configured`);
      return 0;
    },
    update: async (args: UpdateArgs): Promise<unknown> => {
      console.warn(`[prisma-stub] ${name}.update called — no database configured`);
      return { ...args.data };
    },
    create: async (args: CreateArgs): Promise<unknown> => {
      console.warn(`[prisma-stub] ${name}.create called — no database configured`);
      return { id: `stub-${Date.now()}`, ...args.data };
    },
  };
}

export const prisma = {
  webhookDeliveryLog: makeModel("webhookDeliveryLog"),
  webhookDeliveryJob: makeModel("webhookDeliveryJob"),
};
