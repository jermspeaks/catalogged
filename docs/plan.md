# Plan

What is built lives in [completed.md](completed.md). This file is what remains, plus the decisions that shape it. The free-tier rules in [free-tier.md](free-tier.md) apply to every item.

## Goals

- A catalogue of games, books, shows and movies, both consumed and wanted.
- Public read-only site; private admin for adding and editing; a CLI for single and bulk adds.
- Rust where practical. Cloudflare only. $0 or as close as possible. Sized for roughly 5,000 entries at most.

## Architecture (current)

| Layer | Choice | Notes |
|---|---|---|
| Runtime | Rust Worker (`workers-rs` + `axum`) | Compiled to WASM via `worker-build` |
| Rendering | `maud` SSR; vanilla JS in the admin page | No hydrating framework (HTMX was dropped: a few dozen lines of JS were enough) |
| Data | D1 | Schema in `migrations/`; `entries` table, `UNIQUE(kind, slug)` and a unique upstream id |
| Images | R2, WebP | Converted in the browser (admin) or in the CLI; the Worker never encodes (free CPU limit ~10 ms) |
| Auth | Cloudflare Access on `/admin`, `/admin/*` | Browser: Access login. CLI: service-token headers |
| Caching | Cache API for public pages | Keeps D1 reads near zero |

Write path: the admin UI and CLI both go through `/admin/api/entries...`, so validation and slug generation live in one place. Metadata providers are split: URL building and parsing in `crates/core`, HTTP in the CLI and Worker.

## Decisions

Answers to the earlier open questions (2026-11-23):

1. **Covers are served from a public R2 bucket on a custom domain**, not through the Worker. Details are in item 2 below.
2. **No collections.** D1 plus R2 is the model; an entry stands alone. If curated lists are ever wanted, a `lists` table is a small addition, but it is not planned.
3. **`add --source ... --external-id` fetches metadata itself.** Already done for TMDB (PR #2). Open Library and IGDB must follow the same pattern when they land, for both `add` and `import`.

Other standing decisions: statuses are `wanted | in_progress | consumed`; `consumed_at` is the date logged; the slug never changes on edit; entries differ from erika.florist in living in D1 rather than markdown files with a CI pipeline.

## Remaining work

Roughly in order. Items marked **(you)** need something from the owner.

### 1. Ship what is merged
- Apply migration `0002` to the remote database, **then** deploy (the new code reads `review`). Order matters.
- **(you)** Click through `/admin` in a browser: search, add, edit, delete, and cover upload. This has never been exercised.

### 2. Covers on a public bucket domain
- **(you)** Pick a subdomain (e.g. `covers.05152024.xyz`), connect it to the `catalogged-covers` bucket as a custom domain, and leave the `r2.dev` URL disabled.
- Add a `COVERS_BASE_URL` var; pages emit `{COVERS_BASE_URL}/{cover_key}`. Keep the Worker `/covers/...` route as the local-dev fallback when the var is unset.
- Make cover keys immutable by including a short content hash (e.g. `movie/the-matrix-3fa9c1d2.webp`), computed in the Worker with WebCrypto at upload. Replacing a cover then writes a new key and deletes the old one, so a long CDN TTL can never serve a stale image. Needs a migration only if existing keys must be rewritten (they can stay as-is until replaced).
- Edge caching on that domain via a Cache Rule so repeat views are not R2 reads; confirm the exact free-tier behaviour of Cache Rules on R2 custom domains in the docs first, and check `docs/free-tier.md` Class B limits (10M reads/month).
- Consider a rate-limit or hotlink rule on the covers hostname, since the bucket is public (verify what the free plan offers).

### 3. Open Library provider (books)
- `crates/core`: search and lookup URLs and parsing; ISBN as the external id (as erika.florist does).
- CLI: `search --kind book`, `add --source open_library --external-id <isbn>`, CSV import; covers from Open Library (large size), converted to WebP. Use a long HTTP timeout (the service can hang).
- Admin UI: book search alongside movie/show.

### 4. IGDB provider (games)
- **(you)** Obtain a Twitch client id and secret; store as Worker secrets and in `.env`.
- Twitch OAuth client-credentials token handling: cache the token (Cache API) rather than fetching one per request, to stay inside the free subrequest limits.
- Same CLI/admin integration as books; covers from the IGDB image CDN.

### 5. Admin gaps
- Manual cover upload/replace for any entry (needed for anything without a provider cover).
- Add-by-id and manual (no provider) entry in the admin UI, mirroring the CLI.
- Optional: purge the cached public pages for an entry after an edit or delete.

### 6. Public site
- Status filter (wanted / in progress / consumed) and sort options; a nicer layout and visual design (inspired by erika.florist).
- Title search. A `LIKE` filter scans the table (fine for the one-user admin, not for public traffic at 5k rows); use SQLite FTS5 or an indexed prefix search if this goes public. Verify D1 FTS5 support first.
- Basic stats (counts per kind and status) if wanted.

### 7. Hardening and operations
- Verify the Cloudflare Access JWT (`Cf-Access-Jwt-Assertion`) inside the Worker as defense in depth, so a misconfigured Access app cannot expose `/admin`.
- A repeatable D1 backup (`wrangler d1 export`) on a schedule or by hand; know how to use D1 Time Travel.
- Revisit `docs/free-tier.md` numbers against real usage after the first couple of weeks (D1 row metrics, R2 operations).

## Open questions

- **Seeding (`docs/discovery.md`):** the one-line note says "Seed the initial database with the collections." Given that collections are out, does this mean importing existing entries (for example a starter list, or a CSV of what is already consumed)? If so, where does that data live, and should the CLI `import` handle it?
- **Covers subdomain:** which hostname?
- **Public site design:** how closely should it follow erika.florist, or is a simple grid enough for now?
- **Search:** is title search on the public site needed, or only in the admin?
