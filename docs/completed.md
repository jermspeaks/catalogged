# Completed work

What has been built, in order. For what is left, see [plan.md](plan.md). For the free-tier rules every change must respect, see [free-tier.md](free-tier.md).

_Last reviewed: 2026-11-23, after PR #16._

## Status at a glance

| Area | State |
|---|---|
| Code on `main` | Everything below, through PR #19 (creator field and matching) |
| Production | Deployed from `main` (PR #19) at https://catalog.05152024.xyz; `workers.dev` and preview URLs are off. All migrations applied, through `0003` |
| Data | 202 entries: the 2010 and 2011 logs are imported plus a few added by hand; 193 have covers, 192 a provider id and 197 a creator. Later years not yet imported |
| Infrastructure | D1 `catalogged`, R2 `catalogged-covers`, Worker `catalogged`, Access in front of `/admin`, billing alert set, Workers Free plan |

## Infrastructure and security

- Cloudflare D1 database and R2 bucket created; Worker deployed on the custom domain `catalog.05152024.xyz`. The `workers.dev` URLs are switched off.
- Cloudflare Access protects `admin` and `admin/*` on the custom domain: an **Allow** policy for the owner's email and a **Service Auth** policy for the CLI's service token. The public site is open.
- `ADMIN_ENABLED` kill switch (secret): every admin route returns 503 unless it is `1`. It is a backstop, not a replacement for Access.
- Secrets set on the Worker: `ADMIN_ENABLED`, `TMDB_API_KEY`. `ACCESS_TEAM_DOMAIN` / `ACCESS_AUD` are plain vars in `wrangler.toml` (identifiers, not secrets). IGDB secrets for admin game search are not set (the CLI does not need them).
- Free-tier budget documented with guard rules ([free-tier.md](free-tier.md)), a 300 KB cover cap enforced server-side, and a low billing alert in the dashboard.

## Code, by PR

### PR #1 + initial commits: scaffold
- Cargo workspace: `crates/core` (shared types, no runtime deps), `crates/worker` (axum + maud on workers-rs), `crates/cli` (`catalogged`).
- D1 schema (`migrations/0001_init.sql`): `entries` with kind/status CHECK constraints, `UNIQUE(kind, slug)`, and a unique index on the upstream id.
- Worker list page and `POST /admin/api/entries`; CLI `add` and CSV `import`.
- Wrangler config, README, `CLAUDE.md`, plan.

### Direct commit on `main`: CLI hardening
- The CLI never follows redirects: an Access login redirect is an error, not a false "added". It loads `.env` and uses `CF_SERVICE_CLIENT_ID` / `CF_SERVICE_CLIENT_SECRET`.

### PR #2: TMDB, covers, caching, admin UI
- **TMDB provider** (`crates/core/src/tmdb.rs`): URL building, v4-bearer / v3-key auth, response parsing. The CLI and Worker each do their own HTTP.
- **CLI:** `search`; `add --source tmdb --external-id N` and CSV `import` fetch title, year and poster from TMDB themselves (title/year may be blank). This resolves the earlier open question about `add` fetching metadata.
- **Covers:** the CLI downloads the poster, converts it to WebP (`image` + `webp`, <= 300 KB) and uploads it; the Worker validates (WebP magic bytes, size) and stores it in R2. ~41 KB per cover in practice.
- **Edge caching:** public GETs use the Cache API (pages 5 min, covers 24 h).
- **Admin UI** (`/admin`): TMDB search, add, and browser-side canvas WebP conversion and upload (the Worker's ~10 ms free CPU limit rules out encoding there).

### PR #3: reviews, dates, edit/delete, pagination (merged, not deployed)
- Migration `0002`: `notes` renamed to `review`; indexes on `updated_at` so lists read only the rows they return.
- `consumed_at` (`YYYY-MM-DD`) is the date logged/watched; `consumed` defaults to today.
- `GET`/`PUT`/`DELETE /admin/api/entries/{kind}/{slug}`; delete also removes the cover from R2.
- Admin UI: per-entry edit form and Delete, date field on add, paginated list with title filter.
- CLI: `edit`, `delete --yes`, `add --review --date`; CSV gains `review` and `consumed_at` (`notes` still accepted).
- Public site: detail page `/entry/{kind}/{slug}` with the review; home paginated at 60 per page with a kind filter; the cache key varies only on `kind` and `page`.
- Validation in `crates/core`: rating 1-10, valid dates, review length.

### PRs #5 to #8: seed import, covers domain, public grid, books and games
- `import` accepts id-only sheets (IMDb ids resolved via TMDB); covers served from `covers.05152024.xyz` via `COVERS_BASE_URL`; public grid with client-side search/filter/sort from `/index.json`, palette and dark mode; Open Library and IGDB providers in core, CLI and admin search.

### PR #9: admin gaps, public fixes
- Add by id, manual add, edit page with cover upload and confirmed delete, admin theme and navigation, favicon and manifest, content-hashed covers, cover aspect ratio fix on the grid.

### PRs #10 to #12: e2e tests, hardening, workers.dev off
- `e2e/`: Playwright against a local `wrangler dev` with a throwaway D1/R2 and a fake Access issuer; providers mocked (see README).
- Access JWT verified inside the Worker for every `/admin` route (fails closed); `scripts/backup.sh`, Time Travel and the monitoring checklist in `docs/operations.md`.
- `workers_dev = false` and `preview_urls = false`, so a deploy cannot re-enable the `workers.dev` URL.

### PRs #13 to #16: matching, covers, provider ids, cache purge
- `catalogged match in.csv out.csv`: finds provider ids for id-less rows, with a review column (`high` / `check` / `none`). Picking logic in `crates/core/src/matching.rs`.
- Edit page: Find a cover (provider search) and Use a link to an image (Worker relay `/admin/api/image`: https public hosts, images only, 10 MB cap).
- `catalogged covers` (bulk cover finder for cover-less entries; ignores records newer than the logged year) and `--ids-only` (backfill ids). `PUT` on an entry now takes `source` and `external_id`; the edit page has a provider-id field.
- Writes purge the cached public pages they change (per data centre); the e2e suite runs with the cache on.

### Edit page polish
- Edit page: cover column beside a two-column field grid (stacks on narrow screens); toast messages with a spinner while working (errors styled, successes auto-hide) and buttons disabled while a save, search or upload runs.
- Find a cover fills the provider id and year when the entry has none (press Save to keep them).
- Placeholder image (an SVG in the palette, no R2 cost) for entries without a cover: server-rendered list, script grid, detail page and edit page.

### Creator field and better matching
- Migration `0003`: `entries.creator` (author, director or creator, developer). Providers fill it: Open Library authors (details now come from a key search), TMDB directors (`append_to_response=credits`; show creators for TV), IGDB developers. It is in the create and edit APIs, the edit page (labelled Author / Director / Creator / Developer by kind), manual add, the public detail page and public search (`/index.json` carries it).
- Matching (`crates/core/src/matching.rs`): case, punctuation, `&`, a leading article, `Part N` and roman numerals are ignored for an exact match; a title that matches up to a subtitle, or within a typo, is only ever a suggestion; the creator breaks ties between same-titled books and demotes a mismatch; a unique title with no year to compare counts as sure only if the matched record has a year. Records released after the year the entry was logged are ignored.
- CLI: `match` fills creator, accepts `author` or `creator`, and has `--interactive` (numbered candidates, Enter accepts the suggestion, `s` skips, `q` quits keeping the rows done); `covers --creators` backfills creators for entries that already have a provider id; `add`/`edit --creator`.
- Matching also retries a title without a parenthetical, with the parenthetical alone and without a subtitle, and treats number words as digits ("Seven" / "7"). On the real 2011 sheet: 66 sure matches, 48 to check, 10 none, of 124.
- On the real 2010 sheet: 49 sure matches, 19 to check and 7 with no match, out of 75 (before: 41, 18 and 16).

### Log imports
- 2010 to 2016 of the owner's log are imported (755 entries by 2026-11-23): sheets from `scripts/log2csv.py`, ids from `match -i`, then `import` with a backup and a 10-row trial each time. The generator handles month headings with and without the year and markdown escapes.
- Admin list filter: `/admin?missing=id|cover|any` (a dropdown beside the title filter) lists entries without a provider id and/or a cover; each row also shows "no id" and "no cover".

### Identify (edit page)
- The edit page's cover finder is now **Identify**: search the provider (prefilled with the title), pick a record, and its title, year, creator, provider id and cover replace the current ones and the entry is saved in one step (rating, review, status and date are kept; the slug never changes). A record without a cover keeps the current cover. This replaces "Find a cover"; the CLI side is plan P2.

## Discovery items done (triaged 2026-11-23)

- Seed the initial database (2010 imported; later years in plan.md).
- Dark mode with system selection; theme picker on public and admin pages.
- Favicon and manifest.
- Link back to the public site from admin; Edit button on each detail page.
- Delete from the edit page behind a confirm dialog.
- E2E testing on a dev version covering the admin and public pages.
- Bulk cover finder (`catalogged covers`).
- Placeholder image; form feedback on the edit page; edit form grid layout; auto-fill the provider id when picking a cover (edit page polish).
- Colour palette: now only in the CSS (`crates/worker/src/public_ui.rs`); removed from discovery.md.
- Duplicates are rejected: the database enforces one entry per slug and per provider id, and the admin shows the 409. (A warning *before* adding is still open in plan.md.)

## Known gaps in what is "done"

- The admin JavaScript is covered by e2e tests with providers mocked; real provider responses in the browser (TMDB / Open Library / IGDB search from `/admin`) have only been exercised by the owner.
- The manual-cover backup path in `scripts/backup.sh` ran for real on 2026-11-23 (it copied the manually uploaded covers); restoring from it has not been tried.
- Public pages are purged on every admin write (home, kind pages, `/index.json`, the entry page, replaced cover paths), in the data centre that handled the write; other locations expire on the normal TTL.
