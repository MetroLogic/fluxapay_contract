import { createHmac, randomBytes } from "crypto";
import { Router, type Request, type Response } from "express";
import {
  Keypair,
  Operation,
  Transaction,
  TransactionBuilder,
  rpc,
} from "@stellar/stellar-sdk";

export interface AuthRouterConfig {
  serverPublicKey: string;
  serverSecretKey: string;
  jwtSecret: string;
  networkPassphrase: string;
  rpcUrl?: string;
  homeDomain?: string;
  jwtExpirySecs?: number;
  resolveMerchantId?: (account: string) => Promise<string | undefined> | string | undefined;
}

interface IssuedChallenge {
  account: string;
  expiresAt: number;
}

export function createAuthRouter(config: AuthRouterConfig): Router {
  const router = Router();
  const serverKeypair = Keypair.fromSecret(config.serverSecretKey);
  if (serverKeypair.publicKey() !== config.serverPublicKey) {
    throw new Error("SEP-10 server public and secret keys do not match");
  }
  const rpcUrl = config.rpcUrl || "https://soroban-testnet.stellar.org";
  const stellarRpc = new rpc.Server(rpcUrl, { allowHttp: rpcUrl.startsWith("http://") });
  const challenges = new Map<string, IssuedChallenge>();

  const issueChallenge = async (req: Request, res: Response) => {
    const account = req.query.account;
    if (typeof account !== "string" || account.length === 0) {
      res.status(400).json({ error: "Missing required query param: account" });
      return;
    }
    try {
      Keypair.fromPublicKey(account);
      const serverAccount = await stellarRpc.getAccount(config.serverPublicKey);
      const now = Math.floor(Date.now() / 1000);
      const expiresAt = now + 300;
      const nonce = randomBytes(32).toString("base64");
      const domain = config.homeDomain || "fluxapay.stellar.org";
      const transaction = new TransactionBuilder(serverAccount, {
        fee: "100",
        networkPassphrase: config.networkPassphrase,
        timebounds: { minTime: now, maxTime: expiresAt },
      })
        .addOperation(Operation.manageData({
          name: `${domain} auth`,
          value: nonce,
          source: account,
        }))
        .build();
      transaction.sign(serverKeypair);
      const challenge = transaction.toXDR();
      challenges.set(transaction.hash().toString("hex"), { account, expiresAt });
      res.status(200).json({
        transaction: challenge,
        network_passphrase: config.networkPassphrase,
        expires_at: expiresAt,
      });
    } catch (error) {
      res.status(400).json({ error: error instanceof Error ? error.message : "Failed to generate challenge" });
    }
  };

  const authenticate = async (req: Request, res: Response) => {
    const signedXdr = req.body?.transaction ?? req.body?.signed_xdr;
    if (typeof signedXdr !== "string") {
      res.status(400).json({ error: "Body must include the signed challenge transaction" });
      return;
    }
    try {
      const transaction = TransactionBuilder.fromXDR(signedXdr, config.networkPassphrase) as Transaction;
      const challengeId = transaction.hash().toString("hex");
      const issued = challenges.get(challengeId);
      if (!issued) {
        res.status(401).json({ error: "Unknown or already-used SEP-10 challenge" });
        return;
      }
      challenges.delete(challengeId);
      const now = Math.floor(Date.now() / 1000);
      if (issued.expiresAt < now) {
        res.status(401).json({ error: "SEP-10 challenge expired" });
        return;
      }
      if (transaction.operations.length !== 1 || transaction.operations[0].type !== "manageData") {
        res.status(401).json({ error: "Invalid SEP-10 challenge operation" });
        return;
      }
      const operation = transaction.operations[0];
      if (operation.source !== issued.account || transaction.source !== config.serverPublicKey) {
        res.status(401).json({ error: "SEP-10 challenge account mismatch" });
        return;
      }

      const hash = transaction.hash();
      const clientKeypair = Keypair.fromPublicKey(issued.account);
      const hasClientSignature = transaction.signatures.some((signature) => {
        try {
          return clientKeypair.verify(hash, signature.signature());
        } catch {
          return false;
        }
      });
      const hasServerSignature = transaction.signatures.some((signature) => {
        try {
          return serverKeypair.verify(hash, signature.signature());
        } catch {
          return false;
        }
      });
      if (!hasClientSignature || !hasServerSignature) {
        res.status(401).json({ error: "Invalid SEP-10 challenge signature" });
        return;
      }

      const merchantId = await config.resolveMerchantId?.(issued.account);
      const expiry = config.jwtExpirySecs || 86_400;
      const issuedAt = Math.floor(Date.now() / 1000);
      const payload = {
        iss: config.homeDomain || "fluxapay.stellar.org",
        sub: issued.account,
        merchant_id: merchantId || issued.account,
        iat: issuedAt,
        exp: issuedAt + expiry,
      };
      const header = Buffer.from(JSON.stringify({ alg: "HS256", typ: "JWT" })).toString("base64url");
      const body = Buffer.from(JSON.stringify(payload)).toString("base64url");
      const unsignedToken = `${header}.${body}`;
      const signature = createHmac("sha256", config.jwtSecret)
        .update(unsignedToken)
        .digest("base64url");
      const token = `${unsignedToken}.${signature}`;
      res.status(200).json({ token, sub: issued.account, expires_in: expiry });
    } catch (error) {
      res.status(401).json({ error: error instanceof Error ? error.message : "Authentication failed" });
    }
  };

  router.get("/", issueChallenge);
  router.get("/challenge", issueChallenge);
  router.post("/", authenticate);
  router.post("/token", authenticate);
  return router;
}