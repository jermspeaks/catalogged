# Plan

What is built lives in [completed.md](completed.md). This file is what remains, in priority order, plus the decisions that shape it. The free-tier rules in [free-tier.md](free-tier.md) apply to every item. Last reviewed: 2026-11-23, after the creator field and matching work.

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

### P1. Import the rest of the log (M)
The 2010 sheet is in; 2011 to 2026 are not. The owner's log lives in `~/projects/jermspeaks.github.io/src/content/log/` (one markdown file per year, with books, movies, some games and shows; later years carry TMDB links and Letterboxd links).
- Generator: `scripts/log2csv.py <log.md> <year> <out.csv>` (in the repo; sheets go to `seeds/`, which is gitignored because they are personal data). It reads the 2010 and 2011 formats and understands TMDB links, Letterboxd links, `🔁`, `🎮` and `📺` lines; later years (2013 onwards) have not been run through it yet, so check each year's output before matching. Books get a `creator` column from the `by Author` text.
- Per year: generate, `match --interactive` (sure matches are filled in without asking), `import --dry-run`, 10 rows for real, then the rest with `--skip-existing`; `scripts/backup.sh` first. Mind the D1 write budget: 5,000 entries is about half of one day's writes; import a year or two per run.
- Status: creator migration applied and Worker deployed (2026-11-23); creators backfilled for the 2010 entries (76 of 79 have one). 2010 imported. 2011 imported (2026-11-23): 124 rows, 123 added and 1 already present; 9 have no cover or provider id (7 books and 2 movies, mostly typos and foreign titles) and need fixing by hand on the edit page. The catalogue now has 202 entries. Next: 2012 and on, one year at a time.

### P2. Identify (M)
New idea from the owner (2026-11-23): replace the cover finder with an **identify** action that grabs all metadata from the provider and the cover, including the title.
- Edit page: search a provider, pick a record, and apply its title, year, creator, provider id and cover in one step (today Find a cover sets the cover, and fills the id, year and creator only if they are empty).
- CLI: `catalogged covers` grows into (or is joined by) `identify`, which also corrects titles from the provider's spelling. Needs a decision on whether the provider's title should overwrite a hand-typed one (the slug never changes either way).
- Pairs with P1: after each import the leftovers are the rows that did not match, and this is the fast way to fix them by hand.

### P3. Default public sort is date logged (S)
New idea from the owner (2026-11-23). The grid defaults to "recent" (last updated). Make "date logged" the default: the sort control, the order of `/index.json` (it is `updated_at DESC` today), and the server-rendered first page. Entries without a date sort last.

### P4. Measure at scale (S, after a big import)
Nothing has been measured above 79 entries. Before approaching 5,000: size and CPU time of `/index.json` (it now carries the creator too), D1 rows read per day, R2 operations (see the monitoring checklist in [operations.md](operations.md)). If `/index.json` is too heavy, options are trimming fields, paging it, or FTS5 on D1 (unverified).

### P5. Warn before adding a duplicate (S)
The database already rejects duplicates (409). In the admin search results, mark titles already in the catalogue ("in catalogue", linking to its edit page) and disable Add, instead of failing after the click. Needs the existing list (`/index.json`) loaded on the admin page.

### P6. Public site (M, as wanted)
Per-kind and per-status counts; richer detail pages (year, source link, provider overview); lazy cover sizing; show the creator on grid cards. Optional until the catalogue is populated, since the grid is the main surface.

### P7. Housekeeping (S each)
- GitHub Action running the e2e suite on pull requests (none exists; run `npm test` in `e2e/` before merging Worker or UI changes today).
- PNG and apple-touch icons (the favicon and manifest use an SVG only).
- Validate provider ids on write: an Open Library id must look like `OL…W` (a work). One entry was saved with an edition id (`OL22376472M`, "Naked"), which no lookup accepts; its work is `OL2693083W`. Fix that entry by hand on its edit page.
- Admin game search in production needs the two IGDB Worker secrets (`wrangler secret put IGDB_CLIENT_ID` / `IGDB_CLIENT_SECRET`); owner action, only if game search in `/admin` is wanted.
- Review real free-tier numbers after a few weeks of traffic.

## Known limits (accepted)

- Cache purges clear one data centre; other locations lag up to the TTL (5 min; 15 for `/index.json`). Later pages of the no-JS list are never purged.
- IGDB unreleased games can carry placeholder release years.
- `match` and `covers` only auto-accept an exact normalised title (case, punctuation, `&`, a leading article and `Part N` ignored). Subtitle and typo matches are suggestions that need a human (`match --interactive`).
- Open Library work records often lack a publish year, so a year can be blank.
