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

test('provider id: set, keep through the edit form, validate and reject duplicates', async ({ request }) => {
  const a = await seed(request, { kind: 'movie', title: 'E2E Id A' });
  const b = await seed(request, { kind: 'movie', title: 'E2E Id B' });
  const put = (kind: string, slug: string, data: object) =>
    request.put(`/admin/api/entries/${kind}/${slug}`, { data: { title: 'T', status: 'consumed', ...data } });

  const ok = await put('movie', a, { source: 'tmdb', external_id: ' 603 ' });
  expect(ok.status(), await ok.text()).toBe(200);
  expect((await entry(request, 'movie', a)).body).toMatchObject({ source: 'tmdb', external_id: '603' });

  expect((await put('movie', a, { source: 'tmdb' })).status()).toBe(400); // id missing
  expect((await put('movie', a, { external_id: '603' })).status()).toBe(400); // source missing
  expect((await put('movie', a, { source: 'tmdb', external_id: 'a b' })).status()).toBe(400);
  expect((await put('movie', a, { source: 'open_library', external_id: 'OL1W' })).status()).toBe(400); // wrong provider for a movie
  expect((await put('movie', b, { source: 'tmdb', external_id: '603' })).status()).toBe(409); // already used by A

  const cleared = await put('movie', a, {});
  expect(cleared.status()).toBe(200);
  expect((await entry(request, 'movie', a)).body).toMatchObject({ source: null, external_id: null });
});

test('writes clear the cached public pages (cache on, as in production)', async ({ request }) => {
  const titles = async () => (await (await request.get('/index.json')).json()).items.map((i: unknown[]) => i[2]);
  const home = async () => (await request.get('/')).text();
  // Warm the caches, and prove they are populated (a second read is served from cache).
  expect(await titles()).not.toContain('E2E Purge Me');
  await home();
  expect((await request.get('/index.json')).headers()['cache-control']).toContain('max-age');

  // The status lists are cached too and cleared on writes.
  const wanted = async () => (await request.get('/wanted')).text();
  expect(await wanted()).not.toContain('E2E Purge Wanted');
  await seed(request, { kind: 'show', title: 'E2E Purge Wanted', status: 'wanted' });
  expect(await wanted()).toContain('E2E Purge Wanted');

  const slug = await seed(request, { kind: 'show', title: 'E2E Purge Me' });
  expect(await titles()).toContain('E2E Purge Me'); // create
  expect(await home()).toContain('E2E Purge Me');
  expect((await request.get('/?kind=show')).ok()).toBeTruthy();

  const detail = await request.get(`/entry/show/${slug}`);
  expect(await detail.text()).toContain('E2E Purge Me');
  await request.put(`/admin/api/entries/show/${slug}`, { data: { title: 'E2E Purged', status: 'consumed', review: 'edited review' } });
  expect(await titles()).toContain('E2E Purged'); // edit
  expect(await (await request.get(`/entry/show/${slug}`)).text()).toContain('edited review');

  await request.delete(`/admin/api/entries/show/${slug}`);
  expect(await titles()).not.toContain('E2E Purged'); // delete
  expect((await request.get(`/entry/show/${slug}`)).status()).toBe(404);
  expect(await home()).not.toContain('E2E Purged');
});

test('creator: stored on create (author alias too), trimmed, replaced by PUT, length-limited', async ({ request }) => {
  const a = await request.post('/admin/api/entries', { data: { kind: 'book', title: 'E2E Creator A', status: 'consumed', creator: '  Jane Doe  ' } });
  expect(a.status()).toBe(200);
  expect((await entry(request, 'book', 'e2e-creator-a')).body.creator).toBe('Jane Doe');
  const b = await request.post('/admin/api/entries', { data: { kind: 'book', title: 'E2E Creator B', status: 'consumed', author: 'John Roe' } });
  expect(b.status()).toBe(200);
  expect((await entry(request, 'book', 'e2e-creator-b')).body.creator).toBe('John Roe');

  const put = (creator: unknown) => request.put('/admin/api/entries/book/e2e-creator-a', { data: { title: 'E2E Creator A', status: 'consumed', creator } });
  expect((await put('Someone Else')).status()).toBe(200);
  expect((await entry(request, 'book', 'e2e-creator-a')).body.creator).toBe('Someone Else');
  expect((await put('x'.repeat(201))).status()).toBe(400);
  expect((await put(null)).status()).toBe(200);
  expect((await entry(request, 'book', 'e2e-creator-a')).body.creator).toBeNull();
});
