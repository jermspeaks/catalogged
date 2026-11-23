import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

// Throwaway RSA keys for the fake Access issuer. Generated on first use, never committed.
const dir = path.resolve(__dirname, '.keys');

function load(name: string) {
  const file = path.join(dir, `${name}.pem`);
  if (!fs.existsSync(file)) {
    fs.mkdirSync(dir, { recursive: true });
    const { privateKey } = crypto.generateKeyPairSync('rsa', { modulusLength: 2048 });
    try { fs.writeFileSync(file, privateKey.export({ type: 'pkcs8', format: 'pem' }), { flag: 'wx' }); } catch { /* another process won the race */ }
  }
  return crypto.createPrivateKey(fs.readFileSync(file));
}

export const TEAM = 'https://e2e.cloudflareaccess.com';
export const AUD = 'e2e-aud-tag';
export const KID = 'e2e-key-1';
export const goodKey = () => load('good');
export const otherKey = () => load('other'); // a key the Worker does not trust

export function jwks() {
  const jwk = crypto.createPublicKey(goodKey()).export({ format: 'jwk' });
  return { keys: [{ ...jwk, kid: KID, alg: 'RS256', use: 'sig' }] };
}

const b64 = (o: object | Buffer) => Buffer.from(Buffer.isBuffer(o) ? o : JSON.stringify(o)).toString('base64url');

export function mint(opts: { aud?: string | string[]; iss?: string; exp?: number; key?: crypto.KeyObject; alg?: string; kid?: string } = {}) {
  const now = Math.floor(Date.now() / 1000);
  const header = { alg: opts.alg ?? 'RS256', kid: opts.kid ?? KID, typ: 'JWT' };
  const payload = { aud: [opts.aud ?? AUD], iss: opts.iss ?? TEAM, exp: opts.exp ?? now + 3600, iat: now, sub: 'e2e', common_name: 'e2e-service-token' };
  const input = `${b64(header)}.${b64(payload)}`;
  const sig = header.alg === 'none' ? Buffer.alloc(0) : crypto.sign('sha256', Buffer.from(input), opts.key ?? goodKey());
  return `${input}.${b64(sig)}`;
}
