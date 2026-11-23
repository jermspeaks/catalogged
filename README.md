# catalogged

A personal catalogue of games, books, TV shows and movies: what I've consumed and what I want to. Inspired by [erika.florist/catalogue](https://erika.florist/catalogue/).

Rust on Cloudflare (Workers + D1 + R2), server-rendered, designed to run on the free tier. See [docs/plan.md](docs/plan.md) for the architecture and roadmap.

## Layout

| Path | What |
|---|---|
| `crates/core` | Shared types (`Entry`, `NewEntry`, `Kind`, `Status`) and `slugify` |
| `crates/worker` | Cloudflare Worker (axum + maud + D1 + R2): public pages and `/admin` |
| `crates/cli` | `catalogged` CLI: add single entries and bulk import |
| `migrations/` | D1 SQL migrations |
| `docs/` | Plan and design notes |

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
wrangler d1 migrations apply catalogged --local            # create the local DB
wrangler dev                                               # run the Worker locally
```

## First-time Cloudflare setup

```sh
cd crates/worker
wrangler d1 create catalogged          # paste the database_id into wrangler.toml
wrangler r2 bucket create catalogged-covers
wrangler d1 migrations apply catalogged --remote
wrangler deploy
```

Then protect `/admin/*` with a Cloudflare Access application, and create a service token for the CLI.

## CLI

```sh
export CATALOGGED_URL=https://<your-worker>.workers.dev
export CF_SERVICE_CLIENT_ID=...  CF_SERVICE_CLIENT_SECRET=...

cargo run -p catalogged-cli -- add --kind movie --title "10 Things I Hate About You" \
  --status wanted --source tmdb --external-id 4951
cargo run -p catalogged-cli -- import entries.csv --dry-run
cargo run -p catalogged-cli -- import entries.csv
```

CSV columns: `kind,title,status,rating,notes,year,source,external_id`.
