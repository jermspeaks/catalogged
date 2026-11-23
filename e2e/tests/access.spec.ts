import { test, expect, request as pwRequest } from '@playwright/test';
import { AUD, mint, otherKey, TEAM } from '../keys';

// A client with no default token, so each test chooses exactly what it sends.
async function client(token?: string) {
  return pwRequest.newContext({ baseURL: 'http://localhost:8788', extraHTTPHeaders: token ? { 'Cf-Access-Jwt-Assertion': token } : {} });
}
const adminStatus = async (token?: string, path = '/admin') => {
  const c = await client(token);
  const s = (await c.get(path)).status();
  await c.dispose();
  return s;
};
const now = () => Math.floor(Date.now() / 1000);

test('a valid token is accepted on pages and API', async () => {
  expect(await adminStatus(mint())).toBe(200);
  expect(await adminStatus(mint(), '/admin/api/entries/movie/none')).toBe(404); // passed Access, then no such entry
});

test('no token is rejected on every admin route', async () => {
  for (const p of ['/admin', '/admin/', '/admin/edit/movie/x', '/admin/api/search?kind=movie&q=a', '/admin/api/entries/movie/x']) {
    expect(await adminStatus(undefined, p), p).toBe(403);
  }
  const c = await client();
  expect((await c.post('/admin/api/entries', { data: { kind: 'movie', title: 'No Token', status: 'wanted' } })).status()).toBe(403);
  expect((await c.delete('/admin/api/entries/movie/x')).status()).toBe(403);
  expect((await c.put('/admin/api/entries/movie/x/cover', { data: 'x' })).status()).toBe(403);
  await c.dispose();
});

test('bad tokens are rejected', async () => {
  const good = mint();
  const [h, p, s] = good.split('.');
  const cases: Record<string, string> = {
    'wrong audience': mint({ aud: 'someone-else' }),
    'wrong issuer': mint({ iss: 'https://evil.cloudflareaccess.com' }),
    expired: mint({ exp: now() - 3600 }),
    'signed by an untrusted key': mint({ key: otherKey() }),
    'unknown kid': mint({ kid: 'nope' }),
    'alg none': mint({ alg: 'none' }),
    'tampered payload': `${h}.${Buffer.from(JSON.stringify({ aud: [AUD], iss: TEAM, exp: now() + 99999, sub: 'x' })).toString('base64url')}.${s}`,
    'tampered signature': `${h}.${p}.${s.slice(0, -4)}AAAA`,
    garbage: 'not-a-jwt',
  };
  for (const [name, token] of Object.entries(cases)) expect(await adminStatus(token), name).toBe(403);
});

test('public routes need no token', async () => {
  const c = await client();
  for (const p of ['/', '/index.json', '/healthz', '/favicon.svg']) expect((await c.get(p)).status(), p).toBe(200);
  await c.dispose();
});
