import { verifySep10Jwt } from './sep10-jwt';

// NOTE: This test file assumes the following exports from './sep10-jwt':
//   - verifySep10Jwt(token: string): Promise<Sep10JwtPayload> | Sep10JwtPayload
// The implementation must validate the JWT 'iss' claim against
// process.env.SEP10_HOME_DOMAIN and throw an error whose message is
// 'Invalid issuer' when the issuer does not match.
//
// The test uses a real EdDSA (Ed25519) keypair so that the signature is
// genuinely valid; only the 'iss' claim is wrong. This proves the issuer
// check is enforced independently of signature verification.

import { generateKeyPairSync, createPrivateKey, createPublicKey } from 'crypto';
import jwt from 'jsonwebtoken';

const HOME_DOMAIN = 'anchor.example.com';
const ATTACKER_DOMAIN = 'evil-anchor.example.net';

function makeEd25519KeyPair() {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  return {
    privateKeyPem: privateKey.export({ type: 'pkcs8', format: 'pem' }).toString(),
    publicKeyPem: publicKey.export({ type: 'spki', format: 'pem' }).toString(),
  };
}

function signToken(privateKeyPem: string, issuer: string): string {
  const privateKey = createPrivateKey(privateKeyPem);
  return jwt.sign(
    {
      iss: issuer,
      sub: 'GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF',
      iat: Math.floor(Date.now() / 1000),
      exp: Math.floor(Date.now() / 1000) + 60,
    },
    privateKey,
    { algorithm: 'EdDSA' },
  );
}

describe('verifySep10Jwt issuer validation', () => {
  const originalHomeDomain = process.env.SEP10_HOME_DOMAIN;
  const originalPublicKey = process.env.SEP10_PUBLIC_KEY;

  const { privateKeyPem, publicKeyPem } = makeEd25519KeyPair();

  beforeEach(() => {
    process.env.SEP10_HOME_DOMAIN = HOME_DOMAIN;
    process.env.SEP10_PUBLIC_KEY = publicKeyPem;
  });

  afterAll(() => {
    if (originalHomeDomain === undefined) {
      delete process.env.SEP10_HOME_DOMAIN;
    } else {
      process.env.SEP10_HOME_DOMAIN = originalHomeDomain;
    }
    if (originalPublicKey === undefined) {
      delete process.env.SEP10_PUBLIC_KEY;
    } else {
      process.env.SEP10_PUBLIC_KEY = originalPublicKey;
    }
  });

  it('accepts a token whose iss matches SEP10_HOME_DOMAIN', async () => {
    const token = signToken(privateKeyPem, HOME_DOMAIN);
    await expect(Promise.resolve(verifySep10Jwt(token))).resolves.toMatchObject({
      iss: HOME_DOMAIN,
    });
  });

  it('rejects a token with a wrong issuer even when the signature is valid', async () => {
    // Signature is produced with the trusted key, so it verifies successfully.
    // Only the 'iss' claim is from an unexpected (attacker-controlled) domain.
    const token = signToken(privateKeyPem, ATTACKER_DOMAIN);

    await expect(Promise.resolve(verifySep10Jwt(token))).rejects.toThrow(
      /Invalid issuer/i,
    );
  });

  it('rejects a token with a missing issuer claim', async () => {
    const privateKey = createPrivateKey(privateKeyPem);
    const token = jwt.sign(
      {
        sub: 'GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF',
        iat: Math.floor(Date.now() / 1000),
        exp: Math.floor(Date.now() / 1000) + 60,
      },
      privateKey,
      { algorithm: 'EdDSA' },
    );

    await expect(Promise.resolve(verifySep10Jwt(token))).rejects.toThrow(
      /Invalid issuer/i,
    );
  });
});
