# Plan

What is built lives in [completed.md](completed.md). This file is what remains, in priority order, plus the decisions that shape it. The free-tier rules in [free-tier.md](free-tier.md) apply to every item. Last reviewed: 2026-11-23, after the full log import, default sort, Identify and the card polish.

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
- Public site: three lists (consumed on `/`, `/in-progress`, `/wanted`) in a top navigation, each a simple grid with search, kind filter and sort. No attempt to copy erika.florist's design.
- Deploys are separate from merges and happen only when asked.

## Priorities

Ordered by value for the owner's actual goal (a complete, good-looking catalogue) against effort. Sizes: S under half a day, M about a day, L more. Reorder freely.

### P1. Import the log: done; loose ends remain (S)
All of the owner's log (2010 to 2026) is imported: 1,876 entries on 2026-11-23 (1,449 movies, 396 books, 26 shows, 5 games). The generator and procedure are in [completed.md](completed.md); `scripts/log2csv.py` and the per-year steps (generate, `match -i`, `import --dry-run`, `scripts/backup.sh`, a 10-row trial, then the rest with `--skip-existing`) stay valid for new log files.
- Lessons: the log marks TV with 🎥 (Letterboxd), so shows land as movies and `match` skips them; spot them before matching (a title TMDB finds only as TV) and set `kind` to `show`. Open Library returns 503s now and then: re-run `import --skip-existing`. A wrong hand-pick imports quietly, so check the 10-row trial; the sheet's `creator` is copied onto the entry.
- Loose ends (use `/admin?missing=id|cover|any`, Identify on the edit page, or `catalogged identify`): books and a few films with no provider id or cover (the log has no author for 2018's books); four books with Open Library edition ids (`OL…M`); entries with an id but no year or creator; shows watched as 🎥 in 2010 to 2020 that were imported as movies. The owner's dev journal (`~/Documents/dev-journal/Atlas/Sources/Books/`, `isbn13` in the YAML) resolves most id-less books through Open Library's ISBN search; it also has a `rating` field. 🎵 concerts and 🎤 musicals in the log are not imported.

### P2. Measure at scale: done (2026-11-23, 1,949 entries); re-check near 5,000
`/index.json` is 326 KB raw and 94 KB brotli on the wire (about 167 bytes per entry, so about 840 KB raw and 240 KB compressed at 5,000). Rebuilding it is one D1 query: 1,949 rows read, 31 ms in D1; the whole request took 0.17 s from the owner's machine. At most one rebuild per edge location per 15 minutes (plus one after each admin write): about 190k rows a day per busy location now, about 480k at 5,000 entries, against 5M a day, so roughly ten locations could rebuild all day and stay under. The Worker only wraps the string SQLite builds, so CPU should be a few ms; not measured directly (check "CPU time p99" in the dashboard after a few days of traffic). No change needed; if it ever is, trim fields, page it or add FTS5. Public list pages still read about 60 rows each.

### P3. Warn before adding a duplicate (S)
The database already rejects duplicates (409). In the admin search results, mark titles already in the catalogue ("in catalogue", linking to its edit page) and disable Add, instead of failing after the click. Needs the existing list (`/index.json`) loaded on the admin page.

### P4. Public site (M, as wanted)
Per-kind and per-status counts; richer detail pages (year, source link, provider overview); lazy cover sizing; show the creator on grid cards. Optional until the catalogue is populated, since the grid is the main surface.

### P5. Housekeeping (S each)
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
