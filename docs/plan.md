# Plan

What is built lives in [completed.md](completed.md). This file is what remains, in priority order, plus the decisions that shape it. The free-tier rules in [free-tier.md](free-tier.md) apply to every item. Last reviewed: 2026-11-23, after the edit page polish.

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
| Metadata | TMDB (movies, shows), Open Library (books), IGDB (games) | Lookups in the CLI and the admin search; matching in `crates/core` |

## Standing decisions

- Covers are served from the public bucket domain, not the Worker. No collections. `add` and `import` fetch metadata themselves.
- Statuses are `wanted | in_progress | consumed`; `consumed_at` is the date logged; slugs never change on edit.
- Seeding is an import, not a special mechanism: sheets go through `match` (find ids), a human review, then `import`. Re-watches and re-plays are not tracked: one entry per work.
- Public site: a simple grid with search, filter and sort. No attempt to copy erika.florist's design.
- Deploys are separate from merges and happen only when asked.

## Priorities

Ordered by value for the owner's actual goal (a complete, good-looking catalogue) against effort. The edit page polish (layout, toasts and loading states, id fill on cover pick, placeholder image) is done and recorded in completed.md. Sizes: S under half a day, M about a day, L more. Reorder freely.

### P1. Import the rest of the log (M)
The 2010 sheet is in; 2011 to 2026 are not. The owner's log lives in `~/projects/jermspeaks.github.io/src/content/log/` (one markdown file per year, with books, movies, some games and shows; later years carry TMDB links and Letterboxd links).
- Move the log-to-CSV generator into the repo (it is a throwaway script in `/tmp` today) and extend it to every year's formats (links, `🔁` rewatch marks, `🎮`/`📺` lines with year ranges, season numbers).
- Run `match`, review, then `import` year by year (dry run, 10 rows, the rest with `--skip-existing`; `scripts/backup.sh` first). Mind the D1 write budget: 5,000 entries is about half of one day's writes; import a year or two per run.
- Dependency: P2 makes the review step much less painful. P3 should follow the first large import. The edit page polish (done) makes fixing leftovers by hand quicker.

### P2. Better matching, with a creator field (S to M)
About half the 2010 rows came back `none` or `check`. Causes seen: misspelt titles ("Momento"), subtitles or part numbers ("Red Cliff: Part 1", "The Godfather: Part I"), a leading "The", "&" versus "and", and books with no year or author on file.
- Fuzzy title matching in `crates/core/src/matching.rs` (token overlap or edit distance, with a threshold; anything fuzzy is always `check`, never `high`).
- Store a generic **creator** on every entry (author, director or studio; decided 2026-11-23) so books can be matched by author and the field can be shown on the entry. Needs a migration (`creator TEXT`), the field in the entry/edit APIs and edit page, `creator` and the old `author` column accepted in CSV import, and providers filling it (Open Library authors, TMDB director, IGDB developer).
- Interactive review: print each `check`/`none` row with candidates and let the owner pick by number, instead of hand-editing the CSV.

### P3. Measure at scale (S, after a big import)
Nothing has been measured above 79 entries. Before approaching 5,000: size and CPU time of `/index.json`, D1 rows read per day, R2 operations (see the monitoring checklist in [operations.md](operations.md)). If `/index.json` is too heavy, options are trimming fields, paging it, or FTS5 on D1 (unverified).

### P4. Warn before adding a duplicate (S)
The database already rejects duplicates (409). In the admin search results, mark titles already in the catalogue ("in catalogue", linking to its edit page) and disable Add, instead of failing after the click. Needs the existing list (`/index.json`) loaded on the admin page.

### P5. Public site (M, as wanted)
Per-kind and per-status counts; richer detail pages (year, source link, provider overview); lazy cover sizing. Optional until the catalogue is populated, since the grid is the main surface.

### P6. Housekeeping (S each)
- GitHub Action running the e2e suite on pull requests (none exists; run `npm test` in `e2e/` before merging Worker or UI changes today).
- PNG and apple-touch icons (the favicon and manifest use an SVG only).
- Test the manual-cover path of `scripts/backup.sh` once a manually uploaded cover exists.
- Admin game search in production needs the two IGDB Worker secrets (`wrangler secret put IGDB_CLIENT_ID` / `IGDB_CLIENT_SECRET`); owner action, only if game search in `/admin` is wanted.
- Review real free-tier numbers after a few weeks of traffic.

## Known limits (accepted)

- Cache purges clear one data centre; other locations lag up to the TTL (5 min; 15 for `/index.json`). Later pages of the no-JS list are never purged.
- IGDB unreleased games can carry placeholder release years.
- `match` and `covers` need an exact normalised title; everything else is listed for a human.
- Open Library work records often lack a publish year, so a year can be blank.
