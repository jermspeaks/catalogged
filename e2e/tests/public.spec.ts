import { test, expect } from '@playwright/test';
import { seed as seedStrict, Seed } from './helpers';
import type { APIRequestContext } from '@playwright/test';

// Playwright re-runs beforeAll in a fresh worker after any failure, so tolerate rows that already exist.
const seed = (r: APIRequestContext, e: Seed) => seedStrict(r, e).catch(() => '');

// Titles share a prefix so the tests can isolate their own rows with the search box.
test.beforeAll(async ({ request }) => {
  await seed(request, { kind: 'movie', title: 'Zzpub Alpha', status: 'consumed', rating: 9, year: 2001, review: 'First paragraph.\n\nSecond paragraph.', consumed_at: '2024-03-01' });
  await seed(request, { kind: 'book', title: 'Zzpub Beta', status: 'wanted', year: 1999 });
  await seed(request, { kind: 'game', title: 'Zzpub Gamma', status: 'in_progress', rating: 7, year: 2020 });
  await seed(request, { kind: 'show', title: 'Zzpub Delta <b>bold</b>', status: 'consumed', rating: 5, year: 2010, consumed_at: '2025-01-01' });
});

const titles = (page: import('@playwright/test').Page) => page.locator('#grid li a > div:not(.ph)').allTextContents();

test('grid loads, controls appear, search narrows results', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#controls')).toBeVisible();
  await page.fill('input[name=q]', 'zzpub');
  await expect(page.locator('#count')).toHaveText('2 entries'); // Alpha and Delta; the wanted and in-progress ones are on their own pages
  await page.fill('input[name=q]', 'ZZPUB alp'); // case-insensitive
  await expect(page.locator('#count')).toHaveText('1 entry');
  expect(await titles(page)).toEqual(['Zzpub Alpha (2001)']);
  await page.fill('input[name=q]', 'no such title');
  await expect(page.locator('#count')).toHaveText('0 entries');
});

test('titles are rendered as text, not HTML', async ({ page }) => {
  await page.goto('/');
  await page.fill('input[name=q]', 'delta');
  await expect(page.locator('#grid li')).toHaveCount(1);
  expect(await titles(page)).toEqual(['Zzpub Delta <b>bold</b> (2010)']);
  await expect(page.locator('#grid b')).toHaveCount(0);
});

test('kind filter, with state kept in the URL; consumed, in progress and wanted are separate pages', async ({ page }) => {
  await page.goto('/');
  await page.fill('input[name=q]', 'zzpub');
  await expect(page.locator('select[name=status]')).toHaveCount(0); // status is the top navigation, not a filter
  await page.selectOption('select[name=kind]', 'show');
  await expect(page.locator('#count')).toHaveText('1 entry');
  await expect(page).toHaveURL(/q=zzpub/);
  await expect(page).toHaveURL(/kind=show/);
  await page.reload(); // reload restores the same view
  await expect(page.locator('input[name=q]')).toHaveValue('zzpub');
  await expect(page.locator('#count')).toHaveText('1 entry');

  const tabs = page.locator('nav.tabs');
  await expect(tabs.getByRole('link')).toHaveText(['Consumed', 'In progress', 'Wanted']);
  await expect(tabs.locator('[aria-current=page]')).toHaveText('Consumed');
  await tabs.getByRole('link', { name: 'Wanted' }).click();
  await expect(page).toHaveURL(/\/wanted$/);
  await expect(tabs.locator('[aria-current=page]')).toHaveText('Wanted');
  await page.fill('input[name=q]', 'zzpub');
  await expect(page.locator('#count')).toHaveText('1 entry');
  expect((await titles(page)).map((t) => t.split(' ')[1])).toEqual(['Beta']);
  await tabs.getByRole('link', { name: 'In progress' }).click();
  await expect(page).toHaveURL(/\/in-progress$/);
  await page.fill('input[name=q]', 'zzpub');
  await expect(page.locator('#count')).toHaveText('1 entry');
  expect((await titles(page)).map((t) => t.split(' ')[1])).toEqual(['Gamma']);
  await expect(page.locator('#grid li small')).toHaveText('game · 😄'); // the tab says the status, so cards do not
});

test('no-JS pages list only their own status', async ({ browser }) => {
  const ctx = await browser.newContext({ javaScriptEnabled: false });
  const p = await ctx.newPage();
  const html = async (path: string) => (await p.goto(path), await p.content());
  const wanted = await html('/wanted');
  expect(wanted).toContain('Zzpub Beta');
  expect(wanted).not.toContain('Zzpub Alpha');
  expect(wanted).not.toContain('Zzpub Gamma');
  const progress = await html('/in-progress');
  expect(progress).toContain('Zzpub Gamma');
  expect(progress).not.toContain('Zzpub Beta');
  const home = await html('/');
  expect(home).toContain('Zzpub Alpha');
  expect(home).not.toContain('Zzpub Beta');
  await ctx.close();
});

test('sorting', async ({ page, request }) => {
  await seed(request, { kind: 'movie', title: 'Zzsort Alpha', rating: 9, year: 2001, consumed_at: '2020-01-01' });
  await seed(request, { kind: 'movie', title: 'Zzsort Beta', rating: 3, year: 2010, consumed_at: '2023-01-01' });
  await seed(request, { kind: 'movie', title: 'Zzsort Gamma', year: 1999, consumed_at: '2021-01-01' });
  const names = async () => (await titles(page)).map((t) => t.split(' ')[1]);
  await page.goto('/?q=zzsort&sort=title');
  await expect(page.locator('#count')).toHaveText('3 entries');
  expect(await names()).toEqual(['Alpha', 'Beta', 'Gamma']);
  await page.selectOption('select[name=sort]', 'rating');
  expect(await names()).toEqual(['Alpha', 'Beta', 'Gamma']);
  await page.selectOption('select[name=sort]', 'year');
  expect(await names()).toEqual(['Beta', 'Alpha', 'Gamma']);
  await page.selectOption('select[name=sort]', 'logged');
  expect(await names()).toEqual(['Beta', 'Gamma', 'Alpha']); // newest logged first
});

test('detail page shows the review and an Edit link', async ({ page }) => {
  await page.goto('/?q=alpha');
  await page.click('#grid li a');
  await expect(page).toHaveURL(/\/entry\/movie\/zzpub-alpha/);
  await expect(page.locator('h1')).toContainText('Zzpub Alpha');
  await expect(page.getByText('Second paragraph.')).toBeVisible();
  await expect(page.getByRole('link', { name: 'Edit' })).toHaveAttribute('href', '/admin/edit/movie/zzpub-alpha');
});

test('theme picker applies and persists', async ({ page }) => {
  await page.goto('/');
  await page.selectOption('#theme', 'dark');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(page.locator('#theme')).toHaveValue('dark');
  await page.selectOption('#theme', 'auto');
  await expect(page.locator('html')).not.toHaveAttribute('data-theme', /.+/);
});

test('works without JavaScript (server-rendered grid)', async ({ browser }) => {
  const ctx = await browser.newContext({ javaScriptEnabled: false });
  const page = await ctx.newPage();
  await page.goto('/');
  await expect(page.locator('#grid li').first()).toBeVisible();
  await expect(page.locator('#controls')).toBeHidden(); // controls need JS
  await ctx.close();
});

test('unknown entry is a 404', async ({ request }) => {
  expect((await request.get('/entry/movie/nope-nope')).status()).toBe(404);
});

test('entries without a cover show a placeholder (server-rendered list, grid and detail page)', async ({ page, request }) => {
  const slug = await seed(request, { kind: 'game', title: 'E2E No Cover Game' });
  await page.goto('/?kind=game');
  await expect(page.locator('#grid li', { hasText: 'E2E No Cover Game' }).locator('.ph')).toBeVisible();
  await page.goto(`/entry/game/${slug}`);
  await expect(page.locator('.cover.ph')).toBeVisible();
  const html = await (await request.get('/?kind=game')).text();
  expect(html).toContain('class="ph"'); // present without JavaScript too
});

test('search matches the creator as well as the title, and the detail page shows it', async ({ page, request }) => {
  const slug = await seed(request, { kind: 'book', title: 'Zzcreator Book', creator: 'Octavia Butler' });
  await page.goto('/');
  await page.locator('#controls input[name=q]').fill('butler');
  await expect(page.locator('#grid li', { hasText: 'Zzcreator Book' })).toHaveCount(1);
  await page.goto(`/entry/book/${slug}`);
  await expect(page.locator('body')).toContainText('Octavia Butler');
});

test('default order is date logged, newest first, undated last (grid and no-JS list)', async ({ page, browser, request }) => {
  await seed(request, { kind: 'book', title: 'zzlog Old', consumed_at: '2019-05-01' });
  await seed(request, { kind: 'book', title: 'zzlog New', consumed_at: '2024-02-10' });
  await seed(request, { kind: 'book', title: 'zzlog Undated' });
  await seed(request, { kind: 'book', title: 'zzlog Mid', consumed_at: '2021-07-04' });
  const order = ['New', 'Mid', 'Old', 'Undated'];

  await page.goto('/?q=zzlog');
  await expect(page.locator('select[name=sort]')).toHaveValue('logged');
  expect((await titles(page)).map((t) => t.split(' ')[1])).toEqual(order);
  expect(new URL(page.url()).searchParams.has('sort')).toBe(false); // default is not put in the URL

  const ctx = await browser.newContext({ javaScriptEnabled: false });
  const p = await ctx.newPage();
  await p.goto('/');
  const html = await p.content();
  const pos = order.map((n) => html.indexOf(`zzlog ${n}`));
  expect(pos.every((i) => i >= 0)).toBe(true);
  expect([...pos].sort((a, b) => a - b)).toEqual(pos);
  await ctx.close();
});

test('cards and detail pages: faces for ratings, month and year logged, no status label on cards', async ({ page, browser }) => {
  const small = (p: import('@playwright/test').Page, t: string) => p.locator('#grid li', { hasText: t }).locator('small');
  await page.goto('/?q=zzpub');
  await expect(small(page, 'Zzpub Alpha')).toHaveText('movie · 😍 · Mar 2024'); // 9/10 logged 2024-03-01; "consumed" is assumed
  await expect(small(page, 'Zzpub Alpha').locator('[title="9/10"]')).toHaveCount(1);
  await expect(page.locator('#grid')).not.toContainText('/10');
  await expect(page.locator('#grid')).not.toContainText('consumed');

  const ctx = await browser.newContext({ javaScriptEnabled: false }); // the server-rendered list says the same
  const p = await ctx.newPage();
  await p.goto('/');
  await expect(small(p, 'Zzpub Alpha')).toHaveText('movie · 😍 · Mar 2024');
  await p.goto('/wanted');
  await expect(small(p, 'Zzpub Beta')).toHaveText('book');
  await ctx.close();

  await page.goto('/?q=zzpub alpha');
  await page.locator('#grid li a').first().click();
  await expect(page.locator('p', { hasText: 'movie' }).first()).toHaveText('movie · 😍 · Mar 2024');
  await page.goto('/entry/game/zzpub-gamma'); // detail pages do name a status other than consumed
  await expect(page.locator('p', { hasText: 'game' }).first()).toHaveText('game · in progress · 😄');
  await expect(page.getByRole('link', { name: '← all' })).toHaveAttribute('href', '/in-progress');
});

test('every rating from 1 to 10 has its face', async ({ request }) => {
  const want = ['😖😖', '😖', '🙁🙁', '🙁', '😐', '🙂', '😄', '😄😄', '😍', '🤩'];
  for (let r = 1; r <= 10; r++) await seed(request, { kind: 'book', title: `Zzrate ${r}`, rating: r });
  const html = await (await request.get('/entry/book/zzrate-7')).text();
  expect(html).toContain('😄');
  for (let r = 1; r <= 10; r++) {
    const page = await (await request.get(`/entry/book/zzrate-${r}`)).text();
    expect(page, `rating ${r}`).toContain(`title="${r}/10"`);
    expect(page).toContain(`>${want[r - 1]}<`);
  }
});
