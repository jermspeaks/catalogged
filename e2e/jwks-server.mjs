// Serves the fake Access public keys for the e2e Worker (ACCESS_JWKS_URL). Run by playwright.config.ts.
import http from 'node:http';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '.keys');
const priv = crypto.createPrivateKey(fs.readFileSync(path.join(dir, 'good.pem')));
const body = JSON.stringify({ keys: [{ ...crypto.createPublicKey(priv).export({ format: 'jwk' }), kid: 'e2e-key-1', alg: 'RS256', use: 'sig' }] });
http.createServer((_, res) => { res.setHeader('content-type', 'application/json'); res.end(body); }).listen(8790, '127.0.0.1');
