/**
 * Issue #672: SEP-10 auth configuration for the indexer REST API.
 */
export interface SEP10AuthConfig {
  /** SEP-10 Web Auth server endpoint clients use to fetch/submit challenges. */
  webAuthEndpoint: string;
  /** Home domain asserted in the SEP-10 challenge (`manage_data` name). */
  homeDomain: string;
  /** Stellar public key used as HMAC key material to verify issued JWTs. */
  serverPublicKey: string;
  /** Stellar accounts (G...) treated as admins for admin-scoped endpoints. */
  adminAccounts: Set<string>;
}

export function loadSEP10AuthConfig(env: NodeJS.ProcessEnv = process.env): SEP10AuthConfig {
  const webAuthEndpoint =
    env.STELLAR_WEB_AUTH_ENDPOINT || "https://testanchor.stellar.org/auth";
  const homeDomain = env.STELLAR_HOME_DOMAIN || "fluxapay.stellar.org";
  const serverPublicKey = env.STELLAR_SERVER_PUBLIC_KEY || "";
  const adminAccounts = new Set(
    (env.INDEXER_ADMIN_ACCOUNTS || "")
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean),
  );

  return { webAuthEndpoint, homeDomain, serverPublicKey, adminAccounts };
}
