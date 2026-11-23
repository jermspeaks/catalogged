# Operations

## Access verification inside the Worker

Cloudflare Access is the real gate for `/admin`. The Worker also checks the signed token Access
adds to every request that passes it (`Cf-Access-Jwt-Assertion`), so a mistake in the Access app
(a path left out, a policy removed) cannot expose the admin. This covers browsers and the CLI's
service token alike.

Checks: RS256 signature against the team's published keys (cached 1 h), issuer, audience, `exp`,
`nbf`. It **fails closed**: with the settings below missing, every `/admin` request answers 503.

Set these two plain vars in `crates/worker/wrangler.toml` (they are identifiers, not secrets):

| Var | Where to find it |
|---|---|
| `ACCESS_TEAM_DOMAIN` | `https://<team>.cloudflareaccess.com`; Zero Trust, Settings, Custom Pages shows the team name |
| `ACCESS_AUD` | Zero Trust, Access, Applications, the catalogged app, Overview, "Application Audience (AUD) Tag" |

**Order matters when deploying:** fill both in in the same change that ships this code, or `/admin` is
locked (503) until you do. One AUD is supported, so the Access application must be a single app that
covers both `admin` and `admin/*` (as set up now). If you ever split it into two apps, each has its
own AUD and the Worker would reject the other one; ask me to accept a list first.

Local dev sets `ACCESS_JWT_OPTIONAL=1` in `.dev.vars` to skip the check. Never set it in production.
The e2e suite runs with real verification against a fake issuer (`e2e/tests/access.spec.ts`).

If you are ever locked out: fix the vars and redeploy, or temporarily disable the check by
removing the `/admin` handling in `crates/worker/src/access.rs` only as a last resort. Public pages
are unaffected either way.

Key rotation: Cloudflare rotates the signing keys occasionally. The Worker caches the key set for
an hour, so after a rotation `/admin` can answer 403 ("unknown signing key") for up to an hour.

## Backups

D1 has two layers; use both.

1. **Time Travel (automatic, free).** Restore to any minute in the last **7 days** on the Free
   plan. Nothing to set up. Check and restore (this overwrites the live database, so be sure):
   ```sh
   cd crates/worker
   npx wrangler d1 time-travel info catalogged                      # current bookmark
   npx wrangler d1 time-travel restore catalogged --timestamp=<unix-seconds>
   ```
2. **SQL export (yours to keep, no time limit).**
   ```sh
   scripts/backup.sh            # writes backups/catalogged-<time>.sql (gitignored)
   ```
   It also copies covers of entries with no provider (manual uploads), which cannot be re-fetched.
   Provider covers come back by re-importing, so they are not copied. Do it monthly, and before
   any migration or bulk import. The export briefly blocks the database; at this size that is
   instant.

Restoring from an export into the live database (after a disaster, or a bad migration):
```sh
cd crates/worker
npx wrangler d1 execute catalogged --remote --file ../../backups/catalogged-<time>.sql
```
The dump is `CREATE TABLE IF NOT EXISTS`-style for migrations bookkeeping but plain `CREATE TABLE`
for `entries`, so restore into an empty database: create a new one
(`npx wrangler d1 create catalogged-restore`), run the file against it, then point `database_id`
in `wrangler.toml` at it and deploy. The dump was verified to load into a fresh local database
(row counts matched). Manual covers: upload `backups/covers/<key>` back to the R2 bucket with
`npx wrangler r2 object put catalogged-covers/<key> --file ...`.

## Monitoring free-tier use

Look at these once a fortnight until you trust the numbers (no API token needed, dashboard only):

- Workers & Pages, catalogged, Metrics: requests per day (cap 100,000), CPU time p99 (limit 10 ms).
- D1, catalogged, Metrics: rows read per day (cap 5,000,000) and rows written (cap 100,000).
- R2, catalogged-covers, Metrics: Class A and B operations per month, storage (cap 10 GB).
- Billing: the low-spend alert you set up is the backstop for R2, the only thing that can bill.

Expected at personal scale: a few hundred requests a day, tens of thousands of D1 rows read at
most (each `/index.json` rebuild reads the whole table, at most every 15 min per edge location).
If a number is far above that, check for a loop in a script before anything else.
