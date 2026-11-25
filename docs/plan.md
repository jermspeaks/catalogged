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

### P4. Track TV shows by season (M, on hold)
From the owner's discovery notes (2026-11-23): a series can be watched over years, so a show should record each season with its own date. Today a show is one entry with one `consumed_at`, and the log's season text sits in `review` ("Season 4"). Needs a design first: probably a small `seasons` table (entry id, season label, date) or a JSON column on the entry, shown on the detail page, with the entry's own date following the latest season; a migration, the admin edit form and `/index.json` (keep it small) are affected. Also settle how `import` treats a repeated show (today `--skip-existing` drops the later season). Ask the owner before building.

### P5. Public site (M, on hold)
Per-kind and per-status counts; richer detail pages (year, source link, provider overview); lazy cover sizing; show the creator on grid cards. Optional until the catalogue is populated, since the grid is the main surface.

### P6. Housekeeping
- Open Library edition ids (`OL…M`): design agreed, to build in its own session. (1) Reject `OL…M` in `EditEntry::validate_for` with a message pointing at the work id (`OL…W`). (2) Resolve an edition to its work automatically (`/books/OL…M.json` has a `works` link) when an id is typed or pasted, in the admin id fields and in the CLI. (3) For books Open Library lacks, keep Add manually and add an optional source link (`source_url`) on the entry, which also serves the "publisher site" idea in discovery.md. Fix the four existing edition-id books on their edit pages once (2) exists.
- Steam import (discovery.md): `scripts/steam2csv.py` is built (rules in its docstring). Next, owner steps: `catalogged match seeds/steam.csv seeds/steam-matched.csv -i` (57 to check, 12 none), then the usual `import --dry-run`, `scripts/backup.sh`, a 10-row trial and the rest with `--skip-existing`. Unplayed purchases import as `wanted`, which shows only on `/wanted`.
- GitHub Action running the e2e suite on pull requests (on hold; run `npm test` in `e2e/` before merging Worker or UI changes today).
- Admin game search in production needs the two IGDB Worker secrets: set (owner, 2026-11-24).
- Review real free-tier numbers after a few weeks of traffic (on hold).

## Known limits (accepted)

- Cache purges clear one data centre; other locations lag up to the TTL (5 min; 15 for `/index.json`). Later pages of the no-JS list are never purged.
- IGDB unreleased games can carry placeholder release years.
- `match` and `covers` only auto-accept an exact normalised title (case, punctuation, `&`, a leading article and `Part N` ignored). Subtitle and typo matches are suggestions that need a human (`match --interactive`).
- Open Library work records often lack a publish year, so a year can be blank.
