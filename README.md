# catalogged

A personal catalogue of games, books, TV shows and movies: what I've consumed and what I want to. Inspired by [erika.florist/catalogue](https://erika.florist/catalogue/).

Rust on Cloudflare (Workers + D1 + R2), server-rendered, designed to run on the free tier. See [docs/completed.md](docs/completed.md) for what is built, [docs/plan.md](docs/plan.md) for what is left, and [docs/free-tier.md](docs/free-tier.md) for the budget rules.

## Layout

| Path | What |
|---|---|
| `crates/core` | Shared types (`Entry`, `NewEntry`, `Kind`, `Status`), `slugify`, and TMDB URL/parse helpers |
| `crates/worker` | Cloudflare Worker (axum + maud + D1 + R2): cached public pages, covers, and the `/admin` UI + API |
| `crates/cli` | `catalogged` CLI: search TMDB, add single entries, bulk import, upload covers |
| `migrations/` | D1 SQL migrations |
| `docs/` | `completed.md` (built so far), `plan.md` (remaining work and decisions), `free-tier.md` (budget guards) |

## Prerequisites

- Rust (the `wasm32-unknown-unknown` target is installed via `rust-toolchain.toml`)
- Node + `wrangler` (`npm i -g wrangler`), logged in with `wrangler login`
- `cargo install worker-build`
- A TMDB API key; an IGDB key (Twitch client id/secret) when games are wired up

## Develop

```sh
cargo test -p catalogged-core                              # shared-crate tests
cargo check -p catalogged-worker --target wasm32-unknown-unknown

cd crates/worker
cp .dev.vars.example .dev.vars                             # then fill in TMDB_API_KEY
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

Copy `.env.example` to `.env` (gitignored) and fill it in: `CATALOGGED_URL`, the Access service token (`CF_SERVICE_CLIENT_ID`/`CF_SERVICE_CLIENT_SECRET`), and `TMDB_API_KEY`. The CLI loads `.env` automatically.

```sh
cargo run -p catalogged-cli -- search --kind movie "10 things i hate about you"   # find a TMDB id
cargo run -p catalogged-cli -- add --kind movie --source tmdb --external-id 4951  # title, year, cover fetched from TMDB
cargo run -p catalogged-cli -- add --kind movie --title "Some Film" --status consumed --rating 8 --review "Loved it" --date 2026-11-20   # manual entry
cargo run -p catalogged-cli -- edit --kind movie --slug some-film --rating 9 --review "Better on rewatch"          # only passed flags change
cargo run -p catalogged-cli -- delete --kind movie --slug some-film --yes                                          # also removes the cover
cargo run -p catalogged-cli -- import entries.csv --dry-run
cargo run -p catalogged-cli -- import entries.csv                                 # --no-cover to skip covers
```

CSV columns: `kind,title,status,rating,review,year,source,external_id,consumed_at` (`notes` is accepted as an alias for `review`). A `consumed` entry with no date is logged as today. For `source=tmdb` rows, `title` and `year` may be left blank and are filled from TMDB. Covers are downloaded from TMDB, converted to WebP (<= 300 KB), and uploaded to R2. A cover failure never loses the entry.

## Admin UI

`/admin` (behind Cloudflare Access): search TMDB, pick a result, choose a status (and optionally a date; consumed defaults to today), and Add. Every entry in the list below has an edit form (title, status, rating, year, date logged, review) and a Delete button; the list is paginated and filterable by title. The browser converts the poster to WebP with a canvas and uploads it. Games and books come from the CLI until their providers exist.
