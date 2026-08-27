# @fluxapay/backend

FluxaPay backend API. Currently exposes the SEP-10 merchant authentication
endpoints described in [docs/quickstart.md](../docs/quickstart.md).

## Setup

```bash
cp .env.example .env   # set SEP10_SERVER_PUBLIC_KEY
npm install
npm run dev
```

## Endpoints

### `GET /auth/challenge?account=G...`

Issues a SEP-10 challenge transaction for the given Stellar account. The
challenge expires in 5 minutes.

**Response `200`**
```json
{
  "transaction": "<base64 challenge transaction XDR>",
  "network_passphrase": "Test SDF Network ; September 2015"
}
```

### `POST /auth/token`

Exchanges a signed SEP-10 challenge for a JWT.

**Request body**
```json
{
  "transaction": "<base64 signed challenge transaction XDR>",
  "account": "G..."
}
```

**Response `200`**
```json
{ "token": "<jwt>" }
```

The JWT payload contains `iss`, `sub`, `merchant_id`, `iat`, and `exp`
claims. Include it as `Authorization: Bearer <token>` on subsequent API
calls.

**Response `401`** — invalid or expired challenge signature.

## Client usage

See `SEP10Authenticator` in [`sdk/src/sep10.ts`](../sdk/src/sep10.ts) for
the client-side counterpart (`generateChallenge`, `authenticate`), and
`FluxapayClient.initSEP10` / `generateSEP10Challenge` / `authorizeSEP10` in
[`sdk/src/index.ts`](../sdk/src/index.ts) for the SDK-level wrapper.

## Known limitations / TODO

- `resolveMerchantId` currently falls back to the account's own public key;
  wiring it up to `MerchantRegistryClient` is left as a follow-up (see
  `backend/src/index.ts`).
- `SEP10Authenticator.generateChallenge` signs with
  `Keypair.fromPublicKey(serverPublicKey)`, which has no secret key and
  therefore cannot actually produce a signature at runtime — this is a
  pre-existing issue in `sdk/src/sep10.ts` (Issue #490) tracked separately
  from #675; the authenticator needs to be constructed with a full signing
  keypair (e.g. from a `SEP10_SERVER_SECRET` env var) to work end-to-end.
