import "dotenv/config";
import express from "express";
import { Networks } from "@stellar/stellar-sdk";
import { createAuthRouter } from "./routes/auth";

/**
 * Issue #675: FluxaPay backend — currently exposes the SEP-10 merchant
 * authentication endpoints (`/auth/challenge`, `/auth/token`).
 */

const app = express();
app.use(express.json());

const PORT = process.env.PORT ? Number(process.env.PORT) : 3001;
const SERVER_PUBLIC_KEY = process.env.SEP10_SERVER_PUBLIC_KEY;
const NETWORK_PASSPHRASE = process.env.STELLAR_NETWORK === "mainnet"
  ? Networks.PUBLIC
  : Networks.TESTNET;
const HOME_DOMAIN = process.env.SEP10_HOME_DOMAIN || "fluxapay.stellar.org";

if (!SERVER_PUBLIC_KEY) {
  throw new Error("SEP10_SERVER_PUBLIC_KEY env var is required to start the backend.");
}

app.use(
  "/auth",
  createAuthRouter({
    serverPublicKey: SERVER_PUBLIC_KEY,
    networkPassphrase: NETWORK_PASSPHRASE,
    homeDomain: HOME_DOMAIN,
    // TODO(#675): resolve the merchant id via MerchantRegistryClient instead
    // of falling back to the account's own public key.
  }),
);

app.get("/health", (_req, res) => res.status(200).json({ status: "ok" }));

if (require.main === module) {
  app.listen(PORT, () => {
    console.log(`FluxaPay backend listening on :${PORT}`);
  });
}

export { app };
