# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

Deployed at https://catalog.05152024.xyz (custom domain only; workers.dev is off). TMDB metadata and covers, edge caching, and a first admin UI exist. Open Library and IGDB providers, edit/delete, and richer public pages are still to do. See `docs/completed.md` for what exists, `docs/plan.md` for the remaining roadmap and decisions, and `README.md` for setup.

## Commands

```sh
cargo test -p catalogged-core                      # tests (one test: cargo test -p catalogged-core slugs)
cargo check -p catalogged-worker --target wasm32-unknown-unknown   # the worker only builds for wasm
cargo build -p catalogged-cli                      # binary is `catalogged`
cd crates/worker && wrangler dev                   # run locally (needs worker-build, wrangler, and crates/worker/.dev.vars)
cd crates/worker && wrangler d1 migrations apply catalogged --local
```

## Layout

- `crates/core`: shared types (`Kind`, `Status`, `NewEntry`, ...). Keep it free of Worker/runtime deps. Enum string values (snake_case) must match the CHECK constraints in `migrations/`.
- `crates/worker`: axum on workers-rs. D1 futures are not `Send`, so handlers use `#[worker::send]`. `wrangler.toml` lives here and points at `../../migrations`.
- `crates/cli`: blocking reqwest client posting to `/admin/api/entries` with Cloudflare Access service-token headers. It never follows redirects (an Access login redirect must be an error, not success). It calls TMDB directly with `TMDB_API_KEY` and encodes covers to WebP before uploading.
- TMDB: URL building and JSON parsing live in `crates/core/src/tmdb.rs`; the CLI and Worker each do their own HTTP. The Worker holds `TMDB_API_KEY` as a secret for the admin search proxy.
- Public GETs (not `/admin`) are cached with the Cache API in the `fetch` entrypoint; a response is cached only if it sets `Cache-Control`. Public changes can lag up to 5 minutes.
- The admin UI and CLI share one write endpoint so validation lives in the Worker.

## What this is

A personal catalogue of video games, books, TV shows, and movies: media consumed plus a "wants" library. Public read-only site plus an admin UI for adding entries. Spend $0 (or as close as possible): **stay inside the Cloudflare free tier at all times** and check `docs/free-tier.md` before adding anything that adds reads, writes, uploads or Worker CPU. Never upgrade to Workers Paid (on Free, Workers/D1 hard-stop instead of billing; R2 overage is the one thing that bills).

Inspiration: https://erika.florist/catalogue/ (a local copy is at `~/projects/erika.florist`, useful for layout and data model reference). That site uses markdown files plus a Rust `xtask` and CI auto-commits. This project deliberately differs: entries live in D1 and are added through a web admin, not markdown and CI.

## Decided architecture

- **Runtime:** Cloudflare Worker written in Rust (`workers-rs` + `axum`). Cloudflare is a fixed constraint; stay on the free tier.
- **Rendering:** server-side rendered HTML via `maud` (or `askama`), minimal JS, HTMX for the admin. No hydrating frontend framework.
- **Database:** D1 (SQLite). Entries have a type (game/book/show/movie) and a status (consumed / in progress / wanted), plus rating and notes.
- **Images:** stored in R2 and served as WebP. WebP conversion happens **in the browser** in the admin (canvas `toBlob('image/webp')`) before upload. Do not encode images inside the Worker: the free-tier CPU limit (~10 ms) is too small.
- **Admin auth:** Cloudflare Access protecting `/admin`; do not write custom auth.
- **Public pages:** cache at the edge (Cache API) so visitors rarely hit D1.
- **Metadata sources** (admin flow: search a title, pick a result, prefill metadata and cover, set status, save):
  - Movies/shows: TMDB (API key already available)
  - Games: IGDB (authenticates via Twitch OAuth; API key still to be obtained)
  - Books: Open Library (no key)
- **Tooling:** bulk import/backfills should be a local Rust CLI, not Worker code. The Cloudflare MCP (https://mcp.cloudflare.com/mcp) can be used to create the D1 database and R2 bucket.

- **Deploys are separate from merges.** Do not deploy or change production secrets without being asked; changes go in a PR for review first.
- Schema changes go in a new numbered file in `migrations/` (never edit an applied one), and must be applied to production with `wrangler d1 migrations apply catalogged --remote` **before** deploying code that needs them. List queries must stay index-friendly and paginated: D1 bills rows scanned.
- Reviews live in `entries.review` (renamed from `notes`; the input alias `notes` still works). `consumed_at` is the date logged/watched (`YYYY-MM-DD`).
- **CLI:** `catalogged add` (single) and `catalogged import <csv>` (bulk), both going through the Worker API.
