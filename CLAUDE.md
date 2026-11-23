# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

Greenfield. No code, build, lint, or test commands exist yet. Update this file once the project is scaffolded (add the real commands, including how to run a single test).

## What this is

A personal catalogue of video games, books, TV shows, and movies: media consumed plus a "wants" library. Public read-only site plus an admin UI for adding entries. Spend $0 (or as close as possible).

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

## Open questions

- Whether a CLI is also wanted for adding single entries (web admin is the baseline).
