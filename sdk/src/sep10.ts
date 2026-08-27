import {
  Keypair,
  TransactionBuilder,
  Networks,
  Transaction,
  FeeBumpTransaction,
} from "@stellar/stellar-sdk";
import * as crypto from "crypto";

export interface SEP10ChallengeResponse {
  challenge: string;
  networkPassphrase: string;
}

export interface SEP10AuthenticatedResponse {
  jwt: string;
  challenge: string;
}

/**
 * Issue #490: Stellar SEP-10 Web Authentication Bridge
 *
 * Implements the Stellar Web Authentication Standard (SEP-10) for merchant API access.
 * This allows merchants to authenticate using their Stellar keypair and receive a JWT
 * for subsequent API calls.
 */
export class SEP10Authenticator {
  private serverPublicKey: string;
  private serverKeypair: Keypair;
  private networkPassphrase: string;
  private homeDomain: string;

  constructor(
    serverPublicKey: string,
    networkPassphrase: string,
    homeDomain: string = "fluxapay.stellar.org",
  ) {
    this.serverPublicKey = serverPublicKey;
    this.serverKeypair = Keypair.fromPublicKey(serverPublicKey);
    this.networkPassphrase = networkPassphrase;
    this.homeDomain = homeDomain;
  }

  /**
   * Generate a SEP-10 authentication challenge for a merchant keypair.
   */
  generateChallenge(clientPublicKey: string): SEP10ChallengeResponse {
    const clientKeypair = Keypair.fromPublicKey(clientPublicKey);

    const serverKeypair = Keypair.random();
    const now = Math.floor(Date.now() / 1000);
    // Issue #675: challenge transactions expire in 5 minutes.
    const timeout = 5 * 60;

    const transaction = new TransactionBuilder(clientKeypair, {
      fee: "100",
      networkPassphrase: this.networkPassphrase,
      timebounds: {
        minTime: now,
        maxTime: now + timeout,
      },
    })
      .addOperation(
        {
          type: "manageData",
          name: `${this.homeDomain} auth`,
          value: crypto.randomBytes(64).toString("base64"),
        },
      )
      .build();

    transaction.sign(serverKeypair);

    return {
      challenge: transaction.toEnvelope().toXDR("base64"),
      networkPassphrase: this.networkPassphrase,
    };
  }

  /**
   * Verify and process a signed SEP-10 challenge.
   * Returns true if the signature is valid.
   */
  verifyChallengeSignature(
    challengeXdr: string,
    signedXdr: string,
    clientPublicKey: string,
  ): boolean {
    try {
      const transaction = TransactionBuilder.fromXDR(signedXdr, this.networkPassphrase) as
        | Transaction
        | FeeBumpTransaction;

      const sourceAccount = (transaction as any).source;
      if (sourceAccount !== clientPublicKey) {
        return false;
      }

      const signatures = (transaction as any).signatures;
      if (!signatures || signatures.length === 0) {
        return false;
      }

      const clientKeypair = Keypair.fromPublicKey(clientPublicKey);
      const transactionHash = transaction.hash();

      for (const sig of signatures) {
        try {
          clientKeypair.verify(transactionHash, sig);
          return true;
        } catch {
          continue;
        }
      }

      return false;
    } catch {
      return false;
    }
  }

  /**
   * Generate a JWT token after successful SEP-10 authentication.
   * In production, this should be done server-side with proper key management.
   *
   * Issue #675: `merchantId` is included as a `merchant_id` claim so the
   * FluxaPay backend can identify the calling merchant on subsequent API
   * calls. Falls back to `clientPublicKey` when no separate merchant id is
   * available.
   */
  generateJWT(
    clientPublicKey: string,
    expiresInSeconds: number = 86400, // 24 hours
    merchantId?: string,
  ): string {
    const now = Math.floor(Date.now() / 1000);
    const payload = {
      iss: this.serverPublicKey,
      sub: clientPublicKey,
      merchant_id: merchantId ?? clientPublicKey,
      iat: now,
      exp: now + expiresInSeconds,
    };

    const header = {
      alg: "HS256",
      typ: "JWT",
    };

    const headerB64 = Buffer.from(JSON.stringify(header)).toString("base64url");
    const payloadB64 = Buffer.from(JSON.stringify(payload)).toString("base64url");

    const message = `${headerB64}.${payloadB64}`;
    const signature = crypto
      .createHmac("sha256", Buffer.from(this.serverPublicKey, "base64"))
      .update(message)
      .digest("base64url");

    return `${message}.${signature}`;
  }

  /**
   * Complete SEP-10 authentication flow: verify challenge and return JWT.
   *
   * Issue #675: `merchantId` (when known, e.g. from a merchant registry
   * lookup keyed by `clientPublicKey`) is embedded in the JWT's
   * `merchant_id` claim.
   */
  authenticate(
    challengeXdr: string,
    signedXdr: string,
    clientPublicKey: string,
    merchantId?: string,
  ): SEP10AuthenticatedResponse {
    if (!this.verifyChallengeSignature(challengeXdr, signedXdr, clientPublicKey)) {
      throw new Error("Invalid SEP-10 challenge signature");
    }

    const jwt = this.generateJWT(clientPublicKey, undefined, merchantId);
    return {
      jwt,
      challenge: challengeXdr,
    };
  }
}
