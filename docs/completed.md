# Completed work

What has been built, in order. For what is left, see [plan.md](plan.md). For the free-tier rules every change must respect, see [free-tier.md](free-tier.md).

_Last reviewed: 2026-11-23._

## Status at a glance

| Area | State |
|---|---|
| Code on `main` | Everything below, through PR #3 |
| Production | Running the PR #2 code at https://catalog.05152024.xyz. **PR #3 is merged but not deployed, and D1 migration `0002` is not yet applied remotely** |
| Infrastructure | D1 `catalogged`, R2 `catalogged-covers`, Worker `catalogged`, Access in front of `/admin`, billing alert set, Workers Free plan |

## Infrastructure and security

- Cloudflare D1 database and R2 bucket created; Worker deployed on the custom domain `catalog.05152024.xyz`. The `workers.dev` URLs are switched off.
- Cloudflare Access protects `admin` and `admin/*` on the custom domain: an **Allow** policy for the owner's email and a **Service Auth** policy for the CLI's service token. The public site is open.
- `ADMIN_ENABLED` kill switch (secret): every admin route returns 503 unless it is `1`. It is a backstop, not a replacement for Access.
- Secrets set on the Worker: `ADMIN_ENABLED`, `TMDB_API_KEY`.
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

## Known gaps in what is "done"

- The admin page's JavaScript (search, Add, canvas WebP, edit, delete) has never been run in a browser by the author of the code. The endpoints it calls were tested; the page itself needs a manual click-through.
- Public pages and covers can be stale for up to 5 minutes / 24 hours after an edit or delete (no cache purge).
