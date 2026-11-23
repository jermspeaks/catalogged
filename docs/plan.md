# Plan

What is built lives in [completed.md](completed.md). This file is what remains, plus the decisions that shape it. The free-tier rules in [free-tier.md](free-tier.md) apply to every item.

## Goals

- A catalogue of games, books, shows and movies, both consumed and wanted.
- Public read-only site; private admin for adding and editing; a CLI for single and bulk adds.
- Rust where practical. Cloudflare only. $0 or as close as possible. Sized for roughly 5,000 entries at most.

## Architecture (current)

| Layer | Choice | Notes |
|---|---|---|
| Runtime | Rust Worker (`workers-rs` + `axum`) | Compiled to WASM via `worker-build` |
| Rendering | `maud` SSR; vanilla JS in the admin page | No hydrating framework (HTMX was dropped: a few dozen lines of JS were enough) |
| Data | D1 | Schema in `migrations/`; `entries` table, `UNIQUE(kind, slug)` and a unique upstream id |
| Images | R2, WebP | Converted in the browser (admin) or in the CLI; the Worker never encodes (free CPU limit ~10 ms) |
| Auth | Cloudflare Access on `/admin`, `/admin/*` | Browser: Access login. CLI: service-token headers |
| Caching | Cache API for public pages | Keeps D1 reads near zero |

Write path: the admin UI and CLI both go through `/admin/api/entries...`, so validation and slug generation live in one place. Metadata providers are split: URL building and parsing in `crates/core`, HTTP in the CLI and Worker.

## Decisions

Answers to the earlier open questions (2026-11-23):

1. **Covers are served from a public R2 bucket on a custom domain**, not through the Worker. Details are in item 2 below.
2. **No collections.** D1 plus R2 is the model; an entry stands alone. If curated lists are ever wanted, a `lists` table is a small addition, but it is not planned.
3. **`add --source ... --external-id` fetches metadata itself.** Already done for TMDB (PR #2). Open Library and IGDB must follow the same pattern when they land, for both `add` and `import`.

4. **Seeding is an import, not a special mechanism.** The starter data is a CSV of TMDB or IMDb ids (from another project of yours), loaded with the existing CLI `import`. Format and details are in item 2 below.
5. **Public site: a simple grid** with basic filtering, sorting and **title search on the public site**. No attempt to copy erika.florist's design for now.

Other standing decisions: statuses are `wanted | in_progress | consumed`; `consumed_at` is the date logged; the slug never changes on edit; entries differ from erika.florist in living in D1 rather than markdown files with a CI pipeline.

## Remaining work

Roughly in order. Items marked **(you)** need something from the owner.

### 1. Ship what is merged
- Done: migration `0002` applied and PR #3 deployed (by the owner).
- **(you)** Click through `/admin` in a browser: search, add, edit, delete, and cover upload. This has never been exercised.

### 2. Seed import (CSV of TMDB / IMDb ids)
- Built: `catalogged import` accepts a sheet of any shape in [seed-example.csv](seed-example.csv). A row needs a `title` or an `external_id`; an IMDb id (`tt...`) resolves through TMDB's `/find` to the kind, TMDB id, title and year (stored as `source=tmdb`, so de-duplication keeps working). Blank `status` means consumed; a blank `consumed_at` stays blank (imports never default to today).
- `--dry-run` resolves ids and prints what would be added without touching the Worker; `--skip-existing` treats a 409 as skipped so the same file can be re-run; a summary lists every failed row; there is a 250 ms pause per row.
- **(you)** Fill in the sheet in whatever order suits you, then run `--dry-run` first. Do a small batch (10 rows) for real before the full file.
- Budget (estimates, not measured): each entry costs roughly 10 D1 row writes (the insert plus index entries, and a second write when the cover is attached) and one R2 write. 5,000 entries is therefore about half of one day's 100k D1 write budget: run it in one or two batches, and do not retry in a loop. See [free-tier.md](free-tier.md).

### 3. Covers on a public bucket domain
- **Recommendation: yes, use `covers.05152024.xyz`** (pending your confirmation after the trade-offs below). The main reason: every request that reaches the Worker counts against the free 100k requests/day, even a cache hit, so a page of 60 covers served through the Worker costs 61 Worker requests; from a bucket domain it costs 1.
- **(you)** Connect the subdomain In the dashboard: R2, `catalogged-covers`, Settings, Custom Domains, Add `covers.05152024.xyz`. Leave the `r2.dev` URL disabled.
- Built and set to `https://covers.05152024.xyz` (domain connected and verified serving WebP with a 24 h cache header): `COVERS_BASE_URL` var; pages emit `{COVERS_BASE_URL}/{cover_key}` when it is set, else the Worker `/covers/...` route (kept as the local-dev fallback). Once the domain works, a one-line PR sets it to `https://covers.05152024.xyz`.
- Make cover keys immutable by including a short content hash (e.g. `movie/the-matrix-3fa9c1d2.webp`), computed in the Worker with WebCrypto at upload. Replacing a cover then writes a new key and deletes the old one, so a long CDN TTL can never serve a stale image. Needs a migration only if existing keys must be rewritten (they can stay as-is until replaced).
- Edge caching on that domain is zone-level (Cache Rules), separate from the Worker's Cache API. Confirm the default caching behaviour for `.webp` on an R2 custom domain, and set a rule if needed, so repeat views are not R2 Class B reads (free: 10M/month).
- Consider a rate-limit or hotlink rule on the covers hostname, since the bucket is public (verify what the free plan offers).

Trade-offs, for the record:

| | Worker serves covers (today) | Public bucket on `covers.05152024.xyz` |
|---|---|---|
| Worker requests (100k/day cap) | 1 per image, even on cache hits | 0 per image |
| Setup | none | one subdomain + dashboard step |
| Stale or removed images | Worker cache TTL (24 h) | CDN TTL; immutable hashed keys solve replace, a delete can linger until TTL (purge possible) |
| Exposure | reads go through your code | anyone can fetch any object and hotlink; reads over 10M/month would bill |
| Local dev | works | needs the Worker route as a fallback |
| Cost at low traffic | $0 | $0 |

At personal-site traffic both are fine; the bucket domain mainly buys headroom against the Worker request cap and removes the 61x multiplier per page.

### 4. Open Library provider (books)
- `crates/core`: search and lookup URLs and parsing; ISBN as the external id (as erika.florist does).
- CLI: `search --kind book`, `add --source open_library --external-id <isbn>`, CSV import; covers from Open Library (large size), converted to WebP. Use a long HTTP timeout (the service can hang).
- Admin UI: book search alongside movie/show.

### 5. IGDB provider (games)
- **(you)** Obtain a Twitch client id and secret; store as Worker secrets and in `.env`.
- Twitch OAuth client-credentials token handling: cache the token (Cache API) rather than fetching one per request, to stay inside the free subrequest limits.
- Same CLI/admin integration as books; covers from the IGDB image CDN.

### 6. Admin gaps
- Manual cover upload/replace for any entry (needed for anything without a provider cover).
- Add-by-id and manual (no provider) entry in the admin UI, mirroring the CLI.
- Optional: purge the cached public pages for an entry after an edit or delete.

### 7. Public site: grid, filter, sort, search
- Built (first cut): `/index.json` (one compact JSON array per entry, assembled by SQLite so the Worker does not decode rows; cached 15 min, so a cache miss reads the table once, about 5k rows at the limit). A small script on `/` loads it once and does search, kind/status filter and sort (recent, date logged, title, year, rating) in the browser, with "Show more" in batches of 60 and the state kept in the URL. The server-rendered first page and kind links remain as the no-JS fallback. Zero D1 reads per interaction.
- Built: palette from discovery.md, dark mode following the system, with a Theme picker (system / light / dark) saved in `localStorage`.
- Still to do: measure the real `/index.json` CPU time and size with a few thousand rows (seed first); not yet exercised in a browser; richer detail pages; per-kind and per-status counts; lazy cover sizing. FTS5 on D1 stays unverified and is only a fallback if the JSON approach proves too heavy.

### 8. Hardening and operations
- Verify the Cloudflare Access JWT (`Cf-Access-Jwt-Assertion`) inside the Worker as defense in depth, so a misconfigured Access app cannot expose `/admin`.
- A repeatable D1 backup (`wrangler d1 export`) on a schedule or by hand; know how to use D1 Time Travel.
- Revisit `docs/free-tier.md` numbers against real usage after the first couple of weeks (D1 row metrics, R2 operations).

## Open questions

- **Covers domain:** confirmed. Waiting on the dashboard step above.
- **Dark mode** (added to discovery.md): light, dark and follow-the-system. Plan: CSS `prefers-color-scheme` variables for the default, plus an optional toggle stored in `localStorage`. Do it with the public site in item 7.
- **Colour palette** (added to discovery.md): `#c6d8ff #71a9f7 #6b5ca5 #72195a #4c1036`. Use it for the public site theme (item 7); the dark palette needs its own contrast check.
- **E2E testing** (added to discovery.md): a dev environment where every admin function and the public read pages can be exercised. Plan: a local `wrangler dev` run against a throwaway local D1 and R2 (no production data, no cost), driven by a scripted smoke test of the admin API and public pages; browser-level tests for the admin JS later. Scheduled after item 7.
