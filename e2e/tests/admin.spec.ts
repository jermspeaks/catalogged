import { test, expect } from '@playwright/test';
import { png, seed, entry } from './helpers';

const upload = (name: string) => ({ name, mimeType: 'image/png', buffer: png() });

test('admin page has navigation and a persistent theme picker', async ({ page }) => {
  await page.goto('/admin');
  await expect(page.getByRole('link', { name: '← Public site' })).toHaveAttribute('href', '/');
  await page.selectOption('#theme', 'dark');
  await page.goto('/admin');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.selectOption('#theme', 'auto');
});

test('manual add with a cover, then it shows in the list and on the public grid', async ({ page, request }) => {
  await page.goto('/admin');
  await page.locator('summary', { hasText: 'Add manually' }).click();
  const form = page.locator('#manual');
  await form.locator('select[name=kind]').selectOption('book');
  await form.locator('input[name=title]').fill('E2E Manual Book');
  await form.locator('input[name=year]').fill('2023');
  await form.locator('input[name=cover]').setInputFiles(upload('c.png'));
  await form.getByRole('button', { name: 'Add' }).click();
  await expect(page.locator('#msg')).toContainText('Added "E2E Manual Book"');

  const { body } = await entry(request, 'book', 'e2e-manual-book');
  expect(body.cover_key).toMatch(/^book\/e2e-manual-book-[0-9a-f]{8}\.webp$/);
  const cover = await request.get(`/covers/${body.cover_key}`);
  expect(cover.status()).toBe(200);
  expect(cover.headers()['content-type']).toBe('image/webp');
  expect((await cover.body()).subarray(8, 12).toString()).toBe('WEBP'); // the browser really encoded WebP

  await page.goto('/admin?q=e2e manual');
  await expect(page.getByRole('link', { name: 'E2E Manual Book' })).toBeVisible();
  await page.goto('/wanted?q=e2e%20manual'); // the admin form's default status is wanted
  await expect(page.locator('#grid li img')).toHaveCount(1);
  expect(await page.locator('#grid li img').evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth > 0)).toBe(true);
});

test('cover upload still produces WebP when the browser cannot encode it (Safari, iOS)', async ({ page, request }) => {
  // Safari's canvas.toBlob ignores 'image/webp' and returns a PNG; do the same here.
  await page.addInitScript(() => {
    const orig = HTMLCanvasElement.prototype.toBlob;
    HTMLCanvasElement.prototype.toBlob = function (cb: BlobCallback, type?: string, q?: any) { return orig.call(this, cb, 'image/png', q); };
  });
  const wasm = page.waitForResponse((r) => r.url().endsWith('/admin/assets/webp_enc.wasm') && r.ok());
  await page.goto('/admin');
  await page.locator('summary', { hasText: 'Add manually' }).click();
  const form = page.locator('#manual');
  await form.locator('select[name=kind]').selectOption('book');
  await form.locator('input[name=title]').fill('E2E Safari Book');
  await form.locator('input[name=cover]').setInputFiles(upload('s.png'));
  await form.getByRole('button', { name: 'Add' }).click();
  await expect(page.locator('#msg')).toContainText('Added "E2E Safari Book"');
  await wasm; // the fallback encoder was really used
  const { body } = await entry(request, 'book', 'e2e-safari-book');
  const cover = await request.get(`/covers/${body.cover_key}`);
  expect(cover.status()).toBe(200);
  expect((await cover.body()).subarray(8, 12).toString()).toBe('WEBP');
});

test('edit page: save fields, replace the cover, then delete through the dialog', async ({ page, request }) => {
  const slug = await seed(request, { kind: 'movie', title: 'E2E Edit Me', status: 'wanted' });
  await page.goto(`/admin/edit/movie/${slug}`);
  await expect(page.locator('h1')).toContainText('E2E Edit Me');

  await page.selectOption('select[name=status]', 'consumed');
  await page.fill('input[name=rating]', '8');
  await page.fill('input[name=year]', '2019');
  await page.fill('input[name=consumed_at]', '2024-05-06');
  await page.fill('textarea[name=review]', 'Worth it.');
  await page.fill('input[name=external_id]', '12345');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('#msg')).toContainText('Saved');
  expect((await entry(request, 'movie', slug)).body).toMatchObject({ status: 'consumed', rating: 8, year: 2019, consumed_at: '2024-05-06', review: 'Worth it.', source: 'tmdb', external_id: '12345' });
  await page.reload();
  await expect(page.locator('input[name=external_id]')).toHaveValue('12345'); // shown on the edit page

  // validation error surfaces instead of failing silently
  await page.fill('input[name=rating]', '99');
  await page.evaluate(() => (document.querySelector('input[name=rating]') as HTMLInputElement).removeAttribute('max'));
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('#msg')).toContainText('Failed');

  // cover upload, then replace: the URL changes with the content
  await page.setInputFiles('#coverfile', upload('a.png'));
  await expect(page.locator('#msg')).toContainText('Cover updated');
  const first = await page.locator('#coverimg').getAttribute('src');
  await page.setInputFiles('#coverfile', { name: 'b.png', mimeType: 'image/png', buffer: png(80, 120, [10, 200, 30]) });
  await expect.poll(() => page.locator('#coverimg').getAttribute('src')).not.toBe(first);
  const second = (await page.locator('#coverimg').getAttribute('src'))!;
  expect((await request.get(first!)).status()).toBe(404);
  expect((await request.get(second)).status()).toBe(200);

  // delete: cancel keeps it, confirm removes it
  await page.getByRole('button', { name: 'Delete…' }).click();
  await expect(page.locator('#confirm')).toBeVisible();
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.locator('#confirm')).toBeHidden();
  expect((await entry(request, 'movie', slug)).status).toBe(200);
  await page.getByRole('button', { name: 'Delete…' }).click();
  await page.getByRole('button', { name: 'Yes, delete' }).click();
  await expect(page).toHaveURL(/\/admin$/);
  expect((await entry(request, 'movie', slug)).status).toBe(404);
  expect((await request.get(second)).status()).toBe(404);
});

test('search and add (provider mocked): entry gets provider ids and a converted cover', async ({ page, request }) => {
  await page.route('**/mock-poster.png', (r) => r.fulfill({ contentType: 'image/png', body: png(200, 300) }));
  await page.route('**/admin/api/search*', (r) =>
    r.fulfill({ json: [{ id: '4242', title: 'E2E Mock Movie', year: 2001, cover_url: '/mock-poster.png', thumb_url: '/mock-poster.png', overview: null }] }));
  await page.goto('/admin');
  await page.selectOption('#kind', 'movie');
  await page.selectOption('#status', 'consumed');
  await page.fill('#q', 'mock');
  await page.getByRole('button', { name: 'Search' }).click();
  await page.locator('#results li button', { hasText: 'Add' }).click();
  await expect(page.locator('#msg')).toContainText('Added "E2E Mock Movie"');
  const { body } = await entry(request, 'movie', 'e2e-mock-movie');
  expect(body).toMatchObject({ source: 'tmdb', external_id: '4242', status: 'consumed', year: 2001 });
  expect(body.consumed_at).toMatch(/^\d{4}-\d{2}-\d{2}$/); // consumed defaults to today
  expect(body.cover_key).toBeTruthy();
});

test('add by id (provider mocked): uses the kind the lookup returns', async ({ page, request }) => {
  await page.route('**/admin/api/lookup*', (r) =>
    r.fulfill({ json: { kind: 'show', id: '1396', title: 'E2E Lookup Show', year: 2008, cover_url: null, thumb_url: null, overview: null, creator: 'Vince Gilligan' } }));
  await page.goto('/admin');
  await page.locator('summary', { hasText: 'Add by id' }).click();
  await page.selectOption('#idkind', 'movie'); // the lookup says it is really a show
  await page.fill('#extid', 'tt0903747');
  await page.getByRole('button', { name: 'Look up and add' }).click();
  await expect(page.locator('#msg')).toContainText('Added "E2E Lookup Show"');
  expect((await entry(request, 'show', 'e2e-lookup-show')).body).toMatchObject({ source: 'tmdb', external_id: '1396', creator: 'Vince Gilligan' }); // the provider's creator is stored
});

test('search marks results already in the catalogue and links to them instead of offering Add (provider mocked)', async ({ page, request }) => {
  const slug = await seed(request, { kind: 'movie', title: 'E2E Owned Film', source: 'tmdb', external_id: '88001' });
  await page.route('**/admin/api/search*', (r) => r.fulfill({ json: [
    { id: '88001', title: 'E2E Owned Film', year: 2001, cover_url: null, thumb_url: null, overview: null },
    { id: '88002', title: 'E2E Fresh Film', year: 2002, cover_url: null, thumb_url: null, overview: null },
  ] }));
  await page.goto('/admin');
  await page.selectOption('#kind', 'movie');
  await page.fill('#q', 'e2e film');
  await page.getByRole('button', { name: 'Search' }).click();
  const owned = page.locator('#results li', { hasText: 'E2E Owned Film' });
  await expect(owned.getByRole('link', { name: 'in catalogue' })).toHaveAttribute('href', `/admin/edit/movie/${slug}`);
  await expect(owned.getByRole('button')).toHaveCount(0);
  await expect(page.locator('#results li', { hasText: 'E2E Fresh Film' }).getByRole('button', { name: 'Add' })).toBeVisible();
});

test('add by id of something already in the catalogue says so and does not add (provider mocked)', async ({ page, request }) => {
  const slug = await seed(request, { kind: 'show', title: 'E2E Owned Show', source: 'tmdb', external_id: '88003' });
  await page.route('**/admin/api/lookup*', (r) =>
    r.fulfill({ json: { kind: 'show', id: '88003', title: 'E2E Owned Show Renamed', year: 2008, cover_url: null, thumb_url: null, overview: null, creator: null } }));
  await page.goto('/admin');
  await page.locator('summary', { hasText: 'Add by id' }).click();
  await page.selectOption('#idkind', 'show');
  await page.fill('#extid', '88003');
  await page.getByRole('button', { name: 'Look up and add' }).click();
  await expect(page.locator('#msg')).toContainText('already in the catalogue');
  await expect(page.locator('#msg a')).toHaveAttribute('href', `/admin/edit/show/${slug}`);
  expect((await entry(request, 'show', 'e2e-owned-show-renamed')).status).toBe(404);
});

test('owned endpoint: finds ids per kind, ignores other kinds and blanks', async ({ request }) => {
  await seed(request, { kind: 'book', title: 'E2E Owned Book', source: 'open_library', external_id: 'OL88004W' });
  const r = await request.get('/admin/api/owned?kind=book&ids=OL88004W,OL1W,,');
  expect(await r.json()).toEqual({ OL88004W: 'e2e-owned-book' });
  expect(await (await request.get('/admin/api/owned?kind=movie&ids=OL88004W')).json()).toEqual({});
  expect(await (await request.get('/admin/api/owned?kind=movie&ids=')).json()).toEqual({});
});

test('adding a duplicate reports the error', async ({ page, request }) => {
  await seed(request, { kind: 'movie', title: 'E2E Already There' });
  await page.goto('/admin');
  await page.locator('summary', { hasText: 'Add manually' }).click();
  const form = page.locator('#manual');
  await form.locator('select[name=kind]').selectOption('movie');
  await form.locator('input[name=title]').fill('E2E Already There');
  await form.getByRole('button', { name: 'Add' }).click();
  await expect(page.locator('#msg')).toContainText('409');
});

test('edit page: identify from a provider search replaces title, year, creator, id and cover in one step (provider mocked)', async ({ page, request }) => {
  const slug = await seed(request, { kind: 'movie', title: 'e2e identfy typo', status: 'consumed', rating: 8, review: 'keep me', creator: 'Wrong Person', year: 1999 });
  await page.route('**/mock-poster.png', (r) => r.fulfill({ contentType: 'image/png', body: png(200, 300) }));
  let searched = '';
  await page.route('**/admin/api/search*', (r) => {
    searched = new URL(r.request().url()).searchParams.get('q') ?? '';
    return r.fulfill({ json: [
      { id: '1', title: 'No Cover Film', year: 2001, cover_url: null, thumb_url: null, overview: null, creator: 'Nobody' },
      { id: '2', title: 'E2E Identify Film', year: 2019, cover_url: 'http://localhost:8788/mock-poster.png', thumb_url: null, overview: null, creator: 'Right Person' },
    ] });
  });
  await page.goto(`/admin/edit/movie/${slug}`);
  await expect(page.locator('#coverq')).toHaveValue('e2e identfy typo'); // prefilled with the title
  await page.locator('summary', { hasText: 'Identify' }).click();
  await page.locator('#coversearch').getByRole('button', { name: 'Search' }).click();
  await expect(page.locator('#coverresults li')).toHaveCount(2); // records without a cover are listed too
  await page.locator('#coverresults li', { hasText: 'E2E Identify Film' }).getByRole('button', { name: 'Use this record' }).click();
  await expect(page.locator('#msg')).toContainText('Identified as "E2E Identify Film" and saved');
  expect(searched).toBe('e2e identfy typo');
  // the form shows the provider's values, and the entry was saved without a separate Save click
  await expect(page.locator('input[name=title]')).toHaveValue('E2E Identify Film');
  await expect(page.locator('input[name=external_id]')).toHaveValue('2');
  await expect(page.locator('input[name=year]')).toHaveValue('2019');
  await expect(page.locator('input[name=creator]')).toHaveValue('Right Person');
  await expect(page.locator('#coverph')).toHaveCount(0); // placeholder replaced by the new cover
  const { body } = await entry(request, 'movie', slug); // the slug never changes
  expect(body).toMatchObject({ title: 'E2E Identify Film', source: 'tmdb', external_id: '2', year: 2019, creator: 'Right Person', rating: 8, review: 'keep me' });
  expect(body.cover_key).toMatch(/^movie\/.+-[0-9a-f]{8}\.webp$/);
  expect((await request.get(`/covers/${body.cover_key}`)).status()).toBe(200);

  // A record without a cover still replaces the metadata and keeps the cover that is there.
  await page.locator('#coverresults li', { hasText: 'No Cover Film' }).getByRole('button', { name: 'Use this record' }).click();
  await expect(page.locator('#msg')).toContainText('no cover');
  const after = (await entry(request, 'movie', slug)).body;
  expect(after).toMatchObject({ title: 'No Cover Film', external_id: '1', creator: 'Nobody', cover_key: body.cover_key });
});

test('edit page: cover from a link falls back to the Worker relay when the host blocks the browser', async ({ page, request }) => {
  const slug = await seed(request, { kind: 'movie', title: 'E2E Cover Link', status: 'consumed' });
  await page.route('https://img.example.test/**', (r) => r.abort()); // no CORS: the direct read fails
  let relayed = '';
  await page.route('**/admin/api/image*', (r) => {
    relayed = new URL(r.request().url()).searchParams.get('url') ?? '';
    return r.fulfill({ contentType: 'image/png', body: png(120, 180) });
  });
  await page.goto(`/admin/edit/movie/${slug}`);
  await page.locator('summary', { hasText: 'Use a link' }).click();
  await page.fill('#coverurl', 'https://img.example.test/poster.png');
  await page.getByRole('button', { name: 'Use link' }).click();
  await expect(page.locator('#msg')).toContainText('Cover updated');
  expect(relayed).toBe('https://img.example.test/poster.png');
  expect((await entry(request, 'movie', slug)).body.cover_key).toBeTruthy();
});

test('image relay refuses unsafe links', async ({ request }) => {
  for (const url of ['http://example.com/a.png', 'https://localhost/a.png', 'https://127.0.0.1/a.png', 'https://example.com:8443/a.png', 'ftp://x.com/a.png']) {
    const r = await request.get(`/admin/api/image?url=${encodeURIComponent(url)}`);
    expect(r.status(), url).toBe(400);
  }
});

test('edit page: placeholder without a cover, toast and busy state while saving', async ({ page, request }) => {
  const slug = await seed(request, { kind: 'book', title: 'E2E Toast Me' });
  await page.goto(`/admin/edit/book/${slug}`);
  await expect(page.locator('#coverph')).toBeVisible();
  await expect(page.locator('#msg')).toBeHidden(); // nothing to say yet

  // Hold the save so the loading state is observable.
  let release!: () => void;
  const gate = new Promise<void>((r) => (release = r));
  await page.route(`**/admin/api/entries/book/${slug}`, async (r) => { if (r.request().method() === 'PUT') await gate; await r.continue(); });
  await page.fill('input[name=rating]', '7');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('#msg')).toHaveClass(/busy/);
  await expect(page.getByRole('button', { name: 'Save' })).toBeDisabled();
  await expect(page.locator('#edit')).toHaveAttribute('aria-busy', 'true');
  release();
  await expect(page.locator('#msg')).toHaveClass(/ok/);
  await expect(page.locator('#msg')).toContainText('Saved');
  await expect(page.getByRole('button', { name: 'Save' })).toBeEnabled();

  await page.fill('input[name=rating]', '99');
  await page.evaluate(() => (document.querySelector('input[name=rating]') as HTMLInputElement).removeAttribute('max'));
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('#msg')).toHaveClass(/err/); // failures are styled, not just text
});

test('creator: shown and saved on the edit page, kind-specific label, manual add, and filled by identify', async ({ page, request }) => {
  const slug = await seed(request, { kind: 'book', title: 'E2E Creator Book', creator: 'Old Author' });
  await page.goto(`/admin/edit/book/${slug}`);
  await expect(page.getByLabel('Author')).toHaveValue('Old Author');
  await page.getByLabel('Author').fill('Ursula K. Le Guin');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('#msg')).toContainText('Saved');
  expect((await entry(request, 'book', slug)).body.creator).toBe('Ursula K. Le Guin');
  await page.getByLabel('Author').fill('');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('#msg')).toContainText('Saved');
  expect((await entry(request, 'book', slug)).body.creator).toBeNull(); // cleared

  const g = await seed(request, { kind: 'game', title: 'E2E Creator Game' });
  await page.goto(`/admin/edit/game/${g}`);
  await expect(page.getByLabel('Developer')).toBeVisible();
  await page.route('**/mock-poster.png', (r) => r.fulfill({ contentType: 'image/png', body: png(100, 150) }));
  await page.route('**/admin/api/search*', (r) =>
    r.fulfill({ json: [{ id: '113112', title: 'E2E Creator Game', year: 2020, cover_url: 'http://localhost:8788/mock-poster.png', thumb_url: null, overview: null, creator: 'Supergiant Games' }] }));
  await page.locator('summary', { hasText: 'Identify' }).click();
  await page.locator('#coversearch').getByRole('button', { name: 'Search' }).click();
  await page.getByRole('button', { name: 'Use this record' }).click();
  await expect(page.getByLabel('Developer')).toHaveValue('Supergiant Games');
  await expect(page.locator('#msg')).toContainText('saved');
  expect((await entry(request, 'game', g)).body.creator).toBe('Supergiant Games');

  await page.goto('/admin');
  await page.locator('summary', { hasText: 'Add manually' }).click();
  await page.locator('#manual select[name=kind]').selectOption('movie');
  await page.fill('#manual input[name=title]', 'E2E Manual Creator');
  await page.fill('#manual input[name=creator]', 'Some Director');
  await page.getByRole('button', { name: 'Add', exact: true }).click();
  await expect(page.locator('#msg')).toContainText('Added "E2E Manual Creator"');
  expect((await entry(request, 'movie', 'e2e-manual-creator')).body.creator).toBe('Some Director');
});

test('admin list can show only entries missing an id or a cover', async ({ page, request }) => {
  await seed(request, { kind: 'book', title: 'E2E Miss Bare' });
  const r = await request.post('/admin/api/entries', {
    data: { kind: 'movie', title: 'E2E Miss HasId', status: 'consumed', source: 'tmdb', external_id: '991122' },
  });
  expect(r.status(), await r.text()).toBe(200);

  await page.goto('/admin?q=E2E Miss');
  await expect(page.getByRole('link', { name: 'E2E Miss Bare' })).toBeVisible();
  await expect(page.locator('li', { hasText: 'E2E Miss Bare' })).toContainText('no id');
  await expect(page.locator('li', { hasText: 'E2E Miss HasId' })).not.toContainText('no id');

  await page.goto('/admin?q=E2E Miss&missing=id');
  await expect(page.getByRole('link', { name: 'E2E Miss Bare' })).toBeVisible();
  await expect(page.getByRole('link', { name: 'E2E Miss HasId' })).toHaveCount(0);

  for (const m of ['cover', 'any']) {
    await page.goto(`/admin?q=E2E Miss&missing=${m}`);
    await expect(page.getByRole('link', { name: 'E2E Miss Bare' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'E2E Miss HasId' })).toBeVisible();
  }

  // The dropdown keeps its choice, and an unknown value is ignored.
  await page.goto('/admin?missing=id');
  await expect(page.locator('select[name=missing]')).toHaveValue('id');
  await page.goto('/admin?missing=bogus');
  await expect(page.locator('select[name=missing]')).toHaveValue('');
});

test('edit page: Identify takes a provider id and fetches that record (lookup mocked)', async ({ page, request }) => {
  const slug = await seed(request, { kind: 'movie', title: 'E2E Id Identify' });
  const looked: string[] = [];
  let searches = 0;
  await page.route('**/admin/api/lookup*', (r) => {
    const id = new URL(r.request().url()).searchParams.get('id') ?? '';
    looked.push(id);
    if (id === 'tt0903747') return r.fulfill({ json: { kind: 'show', id: '1396', title: 'A Show', year: 2008, cover_url: null, thumb_url: null, overview: null, creator: 'Someone' } });
    if (id === '7770603') return r.fulfill({ json: { kind: 'movie', id: '7770603', title: 'The Matrix', year: 1999, cover_url: null, thumb_url: null, overview: null, creator: 'The Wachowskis' } });
    return r.fulfill({ status: 502, body: 'nope' });
  });
  await page.route('**/admin/api/search*', (r) => { searches++; return r.fulfill({ json: [{ id: '9', title: '7770603 Film', year: 2020, cover_url: null, thumb_url: null, overview: null }] }); });
  await page.goto(`/admin/edit/movie/${slug}`);
  await page.locator('summary', { hasText: 'Identify' }).click();

  // a title searches only
  await page.fill('#coverq', 'some title');
  await page.locator('#coversearch').getByRole('button', { name: 'Search' }).click();
  await expect(page.locator('#coverresults li')).toHaveCount(1);
  expect(looked).toEqual([]);

  // an IMDb id that names a show cannot be applied to a movie entry
  await page.fill('#coverq', 'tt0903747');
  await page.locator('#coversearch').getByRole('button', { name: 'Search' }).click();
  await expect(page.locator('#coverresults li')).toHaveCount(1);
  await expect(page.locator('#coverresults li')).toContainText('that is a show');
  await expect(page.locator('#coverresults li button')).toBeDisabled();

  // a bare number looks the record up by id and also searches (it could be a title), the id record first
  await page.fill('#coverq', '7770603');
  await page.locator('#coversearch').getByRole('button', { name: 'Search' }).click();
  await expect(page.locator('#coverresults li')).toHaveCount(2);
  await expect(page.locator('#coverresults li').first()).toContainText('[id 7770603] The Matrix');
  expect(searches).toBe(2);
  await page.locator('#coverresults li').first().getByRole('button', { name: 'Use this record' }).click();
  await expect(page.locator('#msg')).toContainText('Identified as "The Matrix"');
  expect((await entry(request, 'movie', slug)).body).toMatchObject({ title: 'The Matrix', source: 'tmdb', external_id: '7770603', year: 1999, creator: 'The Wachowskis' });

  // a failed lookup of a non-numeric id is reported, not turned into a title search
  await page.fill('#coverq', 'tt9999999');
  await page.locator('#coversearch').getByRole('button', { name: 'Search' }).click();
  await expect(page.locator('#msg')).toContainText('Lookup failed');
  expect(searches).toBe(2);
});
