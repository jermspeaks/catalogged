# Plan

What is built lives in [completed.md](completed.md). This file is what remains, in priority order, plus the decisions that shape it. The free-tier rules in [free-tier.md](free-tier.md) apply to every item. Last reviewed: 2026-11-23, after the 2010 to 2016 imports.

## Goals

- A catalogue of games, books, shows and movies, both consumed and wanted.
- Public read-only site; private admin for adding and editing; a CLI for single and bulk adds.
- Rust where practical. Cloudflare only. $0 or as close as possible. Sized for roughly 5,000 entries at most.

## Architecture (current)

| Layer | Choice | Notes |
|---|---|---|
| Runtime | Rust Worker (`workers-rs` + `axum`) | Compiled to WASM via `worker-build` |
| Rendering | `maud` SSR; vanilla JS in the admin and the public grid | No hydrating framework |
| Data | D1 | `entries` table, `UNIQUE(kind, slug)` and a unique `(kind, source, external_id)` |
| Images | R2 (public bucket on `covers.05152024.xyz`), WebP | Converted in the browser (admin) or in the CLI; the Worker never encodes |
| Auth | Cloudflare Access on `/admin*`, plus JWT verification in the Worker | Browser: Access login. CLI: service-token headers |
| Caching | Cache API for public pages, purged per data centre on admin writes | Keeps D1 reads near zero |
| Metadata | TMDB (movies, shows), Open Library (books), IGDB (games) | Lookups in the CLI and the admin search; matching in `crates/core`. Providers also supply the creator (director, author, developer) |

## Standing decisions

- Covers are served from the public bucket domain, not the Worker. No collections. `add` and `import` fetch metadata themselves.
- Statuses are `wanted | in_progress | consumed`; `consumed_at` is the date logged; slugs never change on edit.
- Seeding is an import, not a special mechanism: sheets go through `match` (find ids), a human review, then `import`. Re-watches and re-plays are not tracked: one entry per work.
- Public site: a simple grid with search, filter and sort. No attempt to copy erika.florist's design.
- Deploys are separate from merges and happen only when asked.

## Priorities

Ordered by value for the owner's actual goal (a complete, good-looking catalogue) against effort. Sizes: S under half a day, M about a day, L more. Reorder freely.

### P1. Import the rest of the log (M, backlog)
2010 to 2016 are in (755 entries). 2017 to 2026 are not: **backlog**, to resume after the items below. The owner's log lives in `~/projects/jermspeaks.github.io/src/content/log/` (2010 to 2012 are named `2011-01-01-2010.md`, `2012-01-01-2011.md`, `2013-01-01-2012.md`; from 2013 the file is named for its own year, e.g. `2016.md`, `2025-01-10-2025.md`, `2026-01-11-2026.md`).
- Generator: `scripts/log2csv.py <log.md> <year> <out.csv>` (sheets go to `seeds/`, gitignored because they are personal data). It reads the 2010 to 2016 formats (month headings with or without the year) and understands TMDB links, Letterboxd links, `🔁`, `🎮` and `📺` lines; 2017 onwards has not been run through it yet, so check each year's output (row count against the list lines, format surprises) before matching. Books get a `creator` from `by Author`.
- Per year: generate, `match --interactive` (sure matches are filled in without asking), `import --dry-run`, `scripts/backup.sh`, 10 rows for real and a check in D1, then the rest with `--skip-existing`. Mind the D1 write budget: 5,000 entries is about half of one day's writes; a year or two per run is fine.
- Check the 10-row trial for hand-picked mismatches: a wrong pick during `match -i` (for example a 2015 short instead of the 2014 *Birdman*) imports quietly. The CSV `creator` column is copied onto the entry, so correct it in the sheet too.
- Known loose ends from 2010 to 2016 (edit page): entries with no provider id or cover (use the new "missing" filter in `/admin`), four books saved with Open Library edition ids (`OL…M`, which no lookup accepts), nine entries with an id but no year, and six with no creator. Covers whose posters are WebP failed to decode in the CLI (see P8).

### P2. Identify in the CLI (S)
The edit page now has **Identify** (see completed.md): pick a provider record and it replaces the title, year, creator, provider id and cover and saves. Still open is the CLI side: `catalogged identify --kind K --slug S [--external-id ID]` (search by the entry's title and pick, or fetch the given id) applying the same fields, and a bulk mode for the leftovers of an import. It overwrites hand-typed titles on purpose (the slug never changes), so it should show a before and after and ask unless `--yes`.

### P3. Default public sort is date logged (S)
New idea from the owner (2026-11-23). The grid defaults to "recent" (last updated). Make "date logged" the default: the sort control, the order of `/index.json` (it is `updated_at DESC` today), and the server-rendered first page. Entries without a date sort last.

### P4. Public card and detail polish (S)
From the owner's discovery notes (2026-11-23). The status enum is `wanted | in_progress | consumed` (CHECK constraint in `migrations/0001`).
- Stop printing "consumed" on cards and detail pages (it is assumed); only `wanted` and `in progress` need a label. Other states may get their own pages later.
- Show the date logged (`consumed_at`) on cards and the detail page.
- Show ratings as emojis (a few steps, like the reference site) instead of `n/10`. Needs the scale chosen with the owner; the stored rating stays 1 to 10.

### P5. Measure at scale (S, after a big import)
Nothing has been measured above 79 entries. Before approaching 5,000: size and CPU time of `/index.json` (it now carries the creator too), D1 rows read per day, R2 operations (see the monitoring checklist in [operations.md](operations.md)). If `/index.json` is too heavy, options are trimming fields, paging it, or FTS5 on D1 (unverified).

### P6. Warn before adding a duplicate (S)
The database already rejects duplicates (409). In the admin search results, mark titles already in the catalogue ("in catalogue", linking to its edit page) and disable Add, instead of failing after the click. Needs the existing list (`/index.json`) loaded on the admin page.

### P7. Public site (M, as wanted)
Per-kind and per-status counts; richer detail pages (year, source link, provider overview); lazy cover sizing; show the creator on grid cards. Optional until the catalogue is populated, since the grid is the main surface.

### P8. Housekeeping (S each)
- GitHub Action running the e2e suite on pull requests (none exists; run `npm test` in `e2e/` before merging Worker or UI changes today).
- PNG and apple-touch icons (the favicon and manifest use an SVG only).
- Validate provider ids on write: an Open Library id must look like `OL…W` (a work). Edition ids (`OL…M`) were saved by hand for four books and no lookup accepts them; fix those entries on their edit pages, and reject the pattern in `EditEntry::validate_for`.
- The CLI cannot decode WebP posters (TMDB serves a few): the entry is added with its id but no cover (*8½*, *Alps*, *Head*, *I Knew Her Well*, *The Man From U.N.C.L.E.*). Decode WebP, or ask TMDB for a JPEG size.
- Admin game search in production needs the two IGDB Worker secrets (`wrangler secret put IGDB_CLIENT_ID` / `IGDB_CLIENT_SECRET`); owner action, only if game search in `/admin` is wanted.
- Review real free-tier numbers after a few weeks of traffic.

## Known limits (accepted)

- Cache purges clear one data centre; other locations lag up to the TTL (5 min; 15 for `/index.json`). Later pages of the no-JS list are never purged.
- IGDB unreleased games can carry placeholder release years.
- `match` and `covers` only auto-accept an exact normalised title (case, punctuation, `&`, a leading article and `Part N` ignored). Subtitle and typo matches are suggestions that need a human (`match --interactive`).
- Open Library work records often lack a publish year, so a year can be blank.
