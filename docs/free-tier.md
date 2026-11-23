# Free-tier budget

Hard rule: stay inside Cloudflare's free tier. Limits below are from Cloudflare's published pricing/limits docs (checked 2026-11-22); re-verify before relying on them.

| Resource | Free limit | What happens at the limit |
|---|---|---|
| Worker requests | 100,000 / day | Requests fail (no charge) while on the Workers **Free** plan |
| Worker CPU | 10 ms / request | Request fails. Never encode images or do heavy work in the Worker |
| Subrequests | 50 / request | Request fails |
| D1 rows read | 5 million / day | Queries fail (no charge) on the Free plan |
| D1 rows written | 100,000 / day | Queries fail (no charge) on the Free plan |
| D1 storage | 5 GB total | Writes fail |
| R2 storage | 10 GB-month | **Billed** beyond this |
| R2 Class A (writes/lists) | 1 million / month | **Billed** beyond this |
| R2 Class B (reads) | 10 million / month | **Billed** beyond this |
| R2 egress | free | n/a |

R2 is the only place a bill can appear, so it gets explicit guards. Workers and D1 stay hard-capped only if the account is **never upgraded to Workers Paid**.

## Rules

1. Do not subscribe to Workers Paid. On Free, Workers and D1 stop instead of billing.
2. Cover images: WebP only, capped at `MAX_COVER_BYTES` (`crates/core`), enforced by the upload endpoint. At ~100 KB a cover, 10 GB holds ~100k covers, so storage is not a realistic risk. Per-object overwrite is a Class A op, so don't re-upload covers in loops.
3. Public pages must be edge-cached (Cache API) so visitors do not each cost D1 reads. The home page currently reads up to 200 rows per uncached request: at that size, ~25k uncached requests/day would exhaust the 5M read budget. **Done:** the Worker serves public GETs from the Cache API (home 5 min, covers 24 h), so uncached D1/R2 reads are roughly (edge locations) x (requests per TTL window), not one per visit. Admin pages are never cached and read D1 per load.
4. Always paginate and `LIMIT` list queries; add indexes for any filter so reads count index hits, not full scans (D1 bills rows scanned).
5. Bulk imports (CLI) write one row per entry: 100k/day is ample, but do not loop retries on failures.
6. Serve covers through the Worker with long `Cache-Control` / Cache API, so repeat views are not R2 Class B reads.

## Alerts (manual, in the dashboard)

- Billing > Notifications: set a usage-based billing alert on R2 and a budget threshold at a very low amount (e.g. $1), so any overage is noticed immediately.
- Check R2 and D1 usage after the first week of real use (D1: Metrics > Row Metrics).
