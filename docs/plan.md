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

### 4. Open Library provider (books): built
- Entries use the Open Library **work** id (e.g. `OL20893680W`), stable across editions (changed from the ISBN idea: ISBN is per edition). Search, details and cover (large size, converted to WebP) work in the CLI (`search --kind book`, `add --kind book --external-id OL...`, CSV import) and in the admin search. No key; requests carry an identifying User-Agent. Work records often lack a publish year, so the year can be blank.

### 5. IGDB provider (games): built
- Twitch client-credentials token, then Apicalypse queries; cover from the IGDB image CDN (`t_cover_big`). Works in the CLI (`IGDB_CLIENT_ID`/`IGDB_CLIENT_SECRET` in `.env`) and the admin search.
- **(you)** For the admin search on production, set the two Worker secrets (`wrangler secret put IGDB_CLIENT_ID` / `IGDB_CLIENT_SECRET`) when you want game search in `/admin`; the CLI does not need them. I have not set any.
- The Worker fetches a fresh token per admin search (2 subrequests, limit 50). Cache it only if searches become frequent.
- IGDB data quirk: unreleased/TBD games can carry placeholder release years.

### 6. Admin gaps: built
- `/admin` has Add by id (TMDB, IMDb `tt`, Open Library work id, IGDB id), Add manually (no provider, optional cover file), and a catalogue list linking to a per-entry edit page `/admin/edit/{kind}/{slug}`: all fields, cover upload/replace, and Delete behind a confirm dialog. Detail pages link to the edit page (Access gates it). Admin pages share the theme picker and link back to the public site; favicon and web manifest added.
- Covers now use content-hashed keys (`kind/slug-<hash>.webp`) with an immutable one-year cache; replacing a cover writes a new key and deletes the old object, so stale images are impossible. Older unhashed keys keep working until replaced.
- Still optional: purge cached public pages after an edit or delete (they refresh within 5 minutes).

### 6b. Matching and cover search: built
- `catalogged match in.csv out.csv` fills provider ids for id-less rows (see README). Pure picking logic is in `crates/core/src/matching.rs` (exact normalised title; year within 1 separates remakes; otherwise `check`). Not done: fuzzy matching for misspelt titles; those rows come back `none` with candidates to choose from.
- Edit page: search a provider for a cover, or paste an image link. `GET /admin/api/image?url=` relays the remote image (https, public hostname, image/* only, 10 MB cap; one subrequest and no decoding, so no CPU cost). The browser converts to WebP as before.
- Still open (in discovery.md): a placeholder image for entries without a cover, and a bulk cover finder for existing entries with no cover.

### 7. Public site: grid, filter, sort, search
- Built (first cut): `/index.json` (one compact JSON array per entry, assembled by SQLite so the Worker does not decode rows; cached 15 min, so a cache miss reads the table once, about 5k rows at the limit). A small script on `/` loads it once and does search, kind/status filter and sort (recent, date logged, title, year, rating) in the browser, with "Show more" in batches of 60 and the state kept in the URL. The server-rendered first page and kind links remain as the no-JS fallback. Zero D1 reads per interaction.
- Built: palette from discovery.md, dark mode following the system, with a Theme picker (system / light / dark) saved in `localStorage`.
- Still to do: measure the real `/index.json` CPU time and size with a few thousand rows (seed first); not yet exercised in a browser; richer detail pages; per-kind and per-status counts; lazy cover sizing. FTS5 on D1 stays unverified and is only a fallback if the JSON approach proves too heavy.

### E2E tests: built
- `e2e/` (Playwright, 20 tests). See README. Not covered: real providers (mocked on purpose), Cloudflare Access, the production cache layer, and CI (none yet; run `npm test` before merging Worker or UI changes).

### 8. Hardening and operations: built (needs your two values)
- Access JWT verified inside the Worker for all `/admin` routes (fails closed; e2e-tested with a fake issuer: missing, expired, wrong audience/issuer, wrong key, tampered, `alg: none`).
- **(you)** `ACCESS_TEAM_DOMAIN` and `ACCESS_AUD` in `wrangler.toml` (see [operations.md](operations.md)); until set, `/admin` answers 503 once this is deployed. Do not deploy before they are filled in.
- Backups: `scripts/backup.sh` (D1 SQL export + manual covers; verified to restore into a fresh database), Time Travel documented (7 days on Free).
- Free-tier monitoring checklist in operations.md. Real usage numbers have not been reviewed yet (needs a couple of weeks of traffic and the seed import).

## Open questions

- **Covers domain:** confirmed. Waiting on the dashboard step above.
- **Dark mode** (added to discovery.md): light, dark and follow-the-system. Plan: CSS `prefers-color-scheme` variables for the default, plus an optional toggle stored in `localStorage`. Do it with the public site in item 7.
- **Colour palette** (added to discovery.md): `#c6d8ff #71a9f7 #6b5ca5 #72195a #4c1036`. Use it for the public site theme (item 7); the dark palette needs its own contrast check.
- **E2E testing** (added to discovery.md): a dev environment where every admin function and the public read pages can be exercised. Plan: a local `wrangler dev` run against a throwaway local D1 and R2 (no production data, no cost), driven by a scripted smoke test of the admin API and public pages; browser-level tests for the admin JS later. Scheduled after item 7.
