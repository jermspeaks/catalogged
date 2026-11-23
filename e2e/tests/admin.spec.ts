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
  await page.goto('/?q=e2e%20manual');
  await expect(page.locator('#grid li img')).toHaveCount(1);
  expect(await page.locator('#grid li img').evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth > 0)).toBe(true);
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
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('#msg')).toContainText('Saved');
  expect((await entry(request, 'movie', slug)).body).toMatchObject({ status: 'consumed', rating: 8, year: 2019, consumed_at: '2024-05-06', review: 'Worth it.' });

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
    r.fulfill({ json: { kind: 'show', id: '1396', title: 'E2E Lookup Show', year: 2008, cover_url: null, thumb_url: null, overview: null } }));
  await page.goto('/admin');
  await page.locator('summary', { hasText: 'Add by id' }).click();
  await page.selectOption('#idkind', 'movie'); // the lookup says it is really a show
  await page.fill('#extid', 'tt0903747');
  await page.getByRole('button', { name: 'Look up and add' }).click();
  await expect(page.locator('#msg')).toContainText('Added "E2E Lookup Show"');
  expect((await entry(request, 'show', 'e2e-lookup-show')).body).toMatchObject({ source: 'tmdb', external_id: '1396' });
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
