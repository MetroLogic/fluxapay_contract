import { Router, Request, Response } from "express";
import { SEP10Authenticator } from "../../../sdk/src/sep10";

/**
 * Issue #675: SEP-10 challenge + token endpoints for merchant API auth.
 *
 * GET  /auth/challenge?account=G...   -> { transaction, network_passphrase }
 * POST /auth/token { transaction, account } -> { token }
 *
 * The client-side counterpart lives in `sdk/src/sep10.ts`
 * (`SEP10Authenticator` / the FluxaPay SDK's `initSEP10` /
 * `generateSEP10Challenge` / `authorizeSEP10` helpers).
 */

export interface AuthRouterConfig {
  serverPublicKey: string;
  networkPassphrase: string;
  homeDomain?: string;
  /**
   * Resolve a merchant id for a given Stellar public key, e.g. by looking
   * it up in the MerchantRegistry contract. Defaults to using the public
   * key itself as the merchant id when no resolver is provided.
   */
  resolveMerchantId?: (account: string) => Promise<string | undefined> | string | undefined;
}

export function createAuthRouter(config: AuthRouterConfig): Router {
  const router = Router();
  const authenticator = new SEP10Authenticator(
    config.serverPublicKey,
    config.networkPassphrase,
    config.homeDomain,
  );

  /**
   * GET /auth/challenge?account=G...
   * Issues a SEP-10 challenge transaction XDR for the given account.
   * Expires in 5 minutes (see `SEP10Authenticator.generateChallenge`).
   */
  router.get("/challenge", (req: Request, res: Response) => {
    const account = req.query.account;
    if (typeof account !== "string" || account.length === 0) {
      return res.status(400).json({ error: "Missing required query param: account" });
    }

    try {
      const { challenge, networkPassphrase } = authenticator.generateChallenge(account);
      return res.status(200).json({
        transaction: challenge,
        network_passphrase: networkPassphrase,
      });
    } catch (err) {
      return res.status(400).json({
        error: err instanceof Error ? err.message : "Failed to generate challenge",
      });
    }
  });

  /**
   * POST /auth/token
   * Body: { transaction: <signed challenge XDR>, account: <G...> }
   * Verifies the signed challenge and returns a JWT containing
   * `merchant_id`, `iat`, and `exp` claims.
   */
  router.post("/token", async (req: Request, res: Response) => {
    const { transaction, account } = req.body ?? {};
    if (typeof transaction !== "string" || typeof account !== "string") {
      return res.status(400).json({ error: "Body must include `transaction` and `account`" });
    }

    try {
      const merchantId = await config.resolveMerchantId?.(account);
      // `SEP10Authenticator.authenticate` verifies the signed challenge and
      // mints the JWT in one call; the challenge XDR argument is unused by
      // `verifyChallengeSignature` internally, so passing the signed
      // transaction for both is safe here.
      const { jwt } = authenticator.authenticate(transaction, transaction, account, merchantId);
      return res.status(200).json({ token: jwt });
    } catch (err) {
      return res.status(401).json({
        error: err instanceof Error ? err.message : "Authentication failed",
      });
    }
  });

  return router;
}
