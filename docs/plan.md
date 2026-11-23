# Plan

## Goals

- A catalogue of games, books, shows and movies, both consumed and wanted.
- Public read-only site; private admin for adding and editing; a CLI for single and bulk adds.
- Rust where practical. Cloudflare only. $0 or as close as possible.

See [free-tier.md](free-tier.md) for the budget and guard rules.

## Architecture

| Layer | Choice | Notes |
|---|---|---|
| Runtime | Rust Worker (`workers-rs` + `axum`) | Compiled to WASM via `worker-build` |
| Rendering | `maud` SSR, HTMX in admin | No hydrating framework |
| Data | D1 | Schema in `migrations/`; `entries` table, `UNIQUE(kind, slug)` and unique upstream id |
| Images | R2, WebP | Converted **in the browser** (canvas `toBlob('image/webp')`) before upload. The free Worker CPU limit (~10 ms) rules out encoding in the Worker |
| Auth | Cloudflare Access on `/admin/*` | Browser: Access login. CLI: Access service token headers |
| Caching | Cache API for public pages | Keeps D1 reads near zero |

### Write path

Both the admin UI and the CLI write through `POST /admin/api/entries` (`NewEntry` JSON), so validation and slug generation live in one place. Duplicates return 409.

### Metadata sources

| Kind | Source | Auth |
|---|---|---|
| movie, show | TMDB | API key (have it) |
| game | IGDB | Twitch OAuth client credentials (to obtain) |
| book | Open Library | none (can be slow; use a long timeout) |

### Differences from erika.florist

That site keeps markdown files per entry and regenerates data and covers with a Rust `xtask` plus CI commits. Here entries live in D1 and are created through the admin or CLI; there is no CI content pipeline. Her entry shape (`title`, `status`, one external id) is the model for ours. Her statuses are `planned`/etc.; ours are `wanted | in_progress | consumed`.

## Roadmap

1. **Scaffold** (done): workspace, schema, Worker with list page and create endpoint, CLI `add`/`import`.
2. **Cloudflare setup:** create D1 and R2 via wrangler or the Cloudflare MCP; first deploy; Access policy and service token.
3. **Metadata providers:** TMDB search and fetch, then Open Library, then IGDB. Put these in `crates/core` (or a new crate) so the Worker and CLI share them.
4. **Admin UI:** search, pick, prefill, set status/rating/notes, cover upload with browser-side WebP.
5. **Covers:** R2 upload endpoint; CLI fetches upstream covers and uploads them (CLI can convert to WebP natively with the `image` crate).
6. **Public site:** per-kind pages, filters by status, entry detail, edge caching.
7. **Edit/delete** entries; ratings and consumed dates.

## Open questions

- Reading covers back: serve R2 objects through the Worker or a public bucket domain?
- Do collections (as on erika.florist) matter?
- Should CLI `add` accept `--source tmdb --external-id` and fetch the metadata itself (roadmap 3)?
