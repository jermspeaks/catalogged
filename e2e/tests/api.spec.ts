import { test, expect } from '@playwright/test';
import { fakeWebp, seed, entry } from './helpers';

test('health and kill-switch-on admin', async ({ request }) => {
  expect((await request.get('/healthz')).status()).toBe(200);
  expect((await request.get('/admin')).status()).toBe(200);
});

test('create validates input and rejects duplicates', async ({ request }) => {
  const slug = await seed(request, { kind: 'movie', title: 'API Dupe Test', year: 1999 });
  expect(slug).toBe('api-dupe-test');
  const dupe = await request.post('/admin/api/entries', { data: { kind: 'movie', title: 'API Dupe Test', status: 'wanted' } });
  expect(dupe.status()).toBe(409);
  const badRating = await request.post('/admin/api/entries', { data: { kind: 'movie', title: 'Bad Rating', status: 'consumed', rating: 11 } });
  expect(badRating.status()).toBe(400);
  const blank = await request.post('/admin/api/entries', { data: { kind: 'movie', title: '  ', status: 'wanted' } });
  expect(blank.status()).toBe(400);
  const badDate = await request.post('/admin/api/entries', { data: { kind: 'movie', title: 'Bad Date', status: 'consumed', consumed_at: '2020-13-45' } });
  expect(badDate.status()).toBe(400);
});

test('edit and delete round trip', async ({ request }) => {
  const slug = await seed(request, { kind: 'book', title: 'API Edit Test' });
  const put = await request.put(`/admin/api/entries/book/${slug}`, {
    data: { title: 'API Edit Test', status: 'consumed', rating: 8, year: 2001, review: 'Fine.', consumed_at: '2024-02-03' },
  });
  expect(put.status()).toBe(200);
  const got = (await entry(request, 'book', slug)).body;
  expect(got).toMatchObject({ rating: 8, review: 'Fine.', consumed_at: '2024-02-03', year: 2001 });
  expect((await request.delete(`/admin/api/entries/book/${slug}`)).status()).toBe(204);
  expect((await entry(request, 'book', slug)).status).toBe(404);
});

test('cover upload: validates type and size, hashes the key, replaces and deletes', async ({ request }) => {
  const slug = await seed(request, { kind: 'game', title: 'API Cover Test' });
  const url = `/admin/api/entries/game/${slug}/cover`;
  const hdr = { 'content-type': 'image/webp' };
  expect((await request.put(url, { headers: hdr, data: Buffer.from('not an image') })).status()).toBe(415);
  expect((await request.put(url, { headers: hdr, data: fakeWebp(300 * 1024 + 1) })).status()).toBe(413);

  const first = await (await request.put(url, { headers: hdr, data: fakeWebp(16) })).json();
  expect(first.cover_key).toMatch(new RegExp(`^game/${slug}-[0-9a-f]{8}\\.webp$`));
  const served = await request.get(`/covers/${first.cover_key}`);
  expect(served.status()).toBe(200);
  expect(served.headers()['cache-control']).toContain('immutable');

  const second = await (await request.put(url, { headers: hdr, data: fakeWebp(32) })).json();
  expect(second.cover_key).not.toBe(first.cover_key);
  expect((await request.get(`/covers/${first.cover_key}`)).status()).toBe(404); // old object removed
  expect((await request.get(`/covers/${second.cover_key}`)).status()).toBe(200);

  await request.delete(`/admin/api/entries/game/${slug}`);
  expect((await request.get(`/covers/${second.cover_key}`)).status()).toBe(404); // delete removes the cover
});

test('cover upload for a missing entry is a 404', async ({ request }) => {
  const r = await request.put('/admin/api/entries/movie/does-not-exist/cover', { headers: { 'content-type': 'image/webp' }, data: fakeWebp() });
  expect(r.status()).toBe(404);
});

test('public caching headers are set', async ({ request }) => {
  expect((await request.get('/')).headers()['cache-control']).toContain('max-age=300');
  expect((await request.get('/index.json')).headers()['cache-control']).toContain('max-age=900');
  expect((await request.get('/favicon.svg')).status()).toBe(200);
  expect((await request.get('/manifest.webmanifest')).status()).toBe(200);
});
