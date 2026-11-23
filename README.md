# catalogged

A personal catalogue of games, books, TV shows and movies: what I've consumed and what I want to. Inspired by [erika.florist/catalogue](https://erika.florist/catalogue/).

Rust on Cloudflare (Workers + D1 + R2), server-rendered, designed to run on the free tier. See [docs/completed.md](docs/completed.md) for what is built, [docs/plan.md](docs/plan.md) for what is left, and [docs/free-tier.md](docs/free-tier.md) for the budget rules.

## Layout

| Path | What |
|---|---|
| `crates/core` | Shared types (`Entry`, `NewEntry`, `Kind`, `Status`), `slugify`, and TMDB, Open Library and IGDB URL/parse helpers |
| `crates/worker` | Cloudflare Worker (axum + maud + D1 + R2): cached public pages, covers, and the `/admin` UI + API |
| `crates/cli` | `catalogged` CLI: search providers, add single entries, bulk import, upload covers |
| `migrations/` | D1 SQL migrations |
| `docs/` | `completed.md` (built so far), `plan.md` (remaining work and decisions), `free-tier.md` (budget guards) |

## Prerequisites

- Rust (the `wasm32-unknown-unknown` target is installed via `rust-toolchain.toml`)
- Node + `wrangler` (`npm i -g wrangler`), logged in with `wrangler login`
- `cargo install worker-build`
- A TMDB API key; an IGDB key (Twitch client id/secret) for games; books need no key

## Develop

```sh
cargo test -p catalogged-core                              # shared-crate tests
cargo check -p catalogged-worker --target wasm32-unknown-unknown

cd crates/worker
cp .dev.vars.example .dev.vars                             # then fill in TMDB_API_KEY (and IGDB_CLIENT_ID/SECRET for game search)
wrangler d1 migrations apply catalogged --local            # create the local DB
wrangler dev                                               # run the Worker locally (admin at /admin)
```

## First-time Cloudflare setup

```sh
cd crates/worker
wrangler d1 create catalogged          # paste the database_id into wrangler.toml
wrangler r2 bucket create catalogged-covers
wrangler d1 migrations apply catalogged --remote
wrangler deploy
```

Then:

1. Protect `/admin` and `/admin/*` on your custom domain with a Cloudflare Access application: an **Allow** policy for your email and a **Service Auth** policy for the CLI's service token. Turn off the `workers.dev` URLs.
2. Set the secrets (the admin stays disabled until `ADMIN_ENABLED=1`):
   ```sh
   npx wrangler secret put TMDB_API_KEY
   npx wrangler secret put ADMIN_ENABLED     # enter 1
   ```

## CLI

Copy `.env.example` to `.env` (gitignored) and fill it in: `CATALOGGED_URL`, the Access service token (`CF_SERVICE_CLIENT_ID`/`CF_SERVICE_CLIENT_SECRET`), `TMDB_API_KEY`, and `IGDB_CLIENT_ID`/`IGDB_CLIENT_SECRET` for games. The CLI loads `.env` automatically.

```sh
cargo run -p catalogged-cli -- search --kind movie "10 things i hate about you"   # find an id (kinds: movie, show, book, game)
cargo run -p catalogged-cli -- add --kind book --external-id OL20893680W --status wanted   # Open Library work id
cargo run -p catalogged-cli -- add --kind game --external-id 113112 --status consumed      # IGDB id
cargo run -p catalogged-cli -- add --kind movie --source tmdb --external-id 4951  # title, year, cover fetched from TMDB
cargo run -p catalogged-cli -- add --kind movie --title "Some Film" --status consumed --rating 8 --review "Loved it" --date 2026-11-20   # manual entry
cargo run -p catalogged-cli -- edit --kind movie --slug some-film --external-id 603   # set the provider id (empty string clears it)
cargo run -p catalogged-cli -- edit --kind movie --slug some-film --rating 9 --review "Better on rewatch"          # only passed flags change
cargo run -p catalogged-cli -- delete --kind movie --slug some-film --yes                                          # also removes the cover
cargo run -p catalogged-cli -- match log.csv log-matched.csv                      # find provider ids for rows that have none; review before importing
cargo run -p catalogged-cli -- covers --dry-run                                   # find covers for entries that have none (re-runnable; --kind, --limit, --accept-check)
cargo run -p catalogged-cli -- import entries.csv --dry-run
cargo run -p catalogged-cli -- import entries.csv                                 # --no-cover to skip covers, --skip-existing to re-run safely
```

CSV columns: `kind,title,status,rating,review,year,source,external_id,consumed_at` (`notes` is accepted as an alias for `review`). `add` logs a `consumed` entry with no date as today; `import` leaves the date blank. Every column is optional except that a row needs a `title` or an `external_id`. An IMDb id (`tt...`) in `external_id` fills in kind, title and year from TMDB; for any other provider id set `kind` (the source follows from it: movie/show = TMDB, book = Open Library work id, game = IGDB id). `status` defaults to `consumed`. `--skip-existing` makes a re-run safe. See `docs/seed-example.csv`. Covers are downloaded from the provider (TMDB, Open Library or IGDB), converted to WebP (<= 300 KB), and uploaded to R2. A cover failure never loses the entry.

`match` is for sheets with titles but no ids (for example a reading log). It searches TMDB / Open Library / IGDB for each row without an `external_id` (books also use an `author` column) and writes a new CSV with `source`, `external_id` and `year` filled in, plus `match` (`high`: exact title and year agree; `check`: exact title but several records share it, so confirm the pick; `none`: nothing matched), `match_title` and `candidates` (top three, as `id: title (year)`). Fix the `check` and `none` rows by hand (copy an id from `candidates`, or clear `external_id` to add the row without a provider), then run `import`. Rows that already have an id pass through. Nothing is sent to the Worker. Entries with an id get covers on import; entries without one get none, and you can add one from the edit page.

`covers` fills in covers for entries that already exist without one (for example title-only imports). It reads the public `/index.json`, searches the provider by title for each cover-less entry, and uploads the cover of an exact-title match. Results released after the year you logged the entry are ignored (a 2010 log entry is not the 2025 remake). Ambiguous matches are listed, not guessed, unless you pass `--accept-check`; fix those on the entry's edit page (Find a cover). It also stores the provider id (and the year, if the entry has none) on the entry; `--ids-only` does just that for entries that already have a cover but no id. `/index.json` is cached up to 15 minutes, so entries added in the last few minutes may not show yet.

## Admin UI

`/admin` (behind Cloudflare Access): search (movies, shows, books, games) and Add; Add by id (TMDB, IMDb `tt...`, Open Library work id, IGDB id); Add manually with an optional cover file. Each catalogue row opens an edit page with every field, a cover upload/replace, a provider id field (TMDB id, Open Library work id or IGDB id; the provider follows from the kind), Find a cover (searches the same provider as adding, prefilled with the title), Use a link to an image (the Worker relays the file because most hosts block browsers from reading it; https hostnames only, images up to 10 MB), and Delete behind a confirm dialog. The browser converts covers to WebP with a canvas (scaled to <= 500 px wide) and uploads them. Game search in production needs the IGDB Worker secrets (see docs/plan.md item 5).

## End-to-end tests

```sh
cd e2e
npm install && npx playwright install chromium   # once
npm test                                          # starts its own wrangler dev on :8788, throwaway D1/R2 in e2e/.state
```

Covers the admin API (validation, duplicates, cover rules and hashed keys), the admin pages in a real browser (manual add with cover, edit, cover replace, confirmed delete, add by id and search with providers mocked), and the public grid (search, filters, sorting, theme, no-JS fallback). Nothing touches production or any Cloudflare service; the first run compiles the Worker, so allow a few minutes.
