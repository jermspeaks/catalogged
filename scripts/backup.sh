#!/bin/sh
# Back up the production catalogue: a full D1 SQL dump plus any covers that cannot be re-fetched
# from a provider (entries with no source, i.e. manual uploads). Read-only against production.
# Usage: scripts/backup.sh [output-dir]        (needs `npx wrangler login`; default ./backups)
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$root/backups}
stamp=$(date +%Y%m%d-%H%M%S)
mkdir -p "$out/covers"
cd "$root/crates/worker"

npx wrangler d1 export catalogged --remote --output "$out/catalogged-$stamp.sql"

# Provider covers can be re-downloaded by re-importing; manual covers cannot, so keep a copy.
base=$(sed -n 's/^COVERS_BASE_URL *= *"\(.*\)".*/\1/p' wrangler.toml)
if [ -n "$base" ]; then
  npx wrangler d1 execute catalogged --remote --json \
    --command "SELECT cover_key FROM entries WHERE cover_key IS NOT NULL AND source IS NULL" |
    python3 -c 'import sys,json
for block in json.load(sys.stdin):
    for r in block.get("results", []): print(r["cover_key"])' |
    while read -r key; do
      mkdir -p "$out/covers/$(dirname "$key")"
      curl -fsS "$base/$key" -o "$out/covers/$key" && echo "saved cover $key"
    done
fi
echo "backup written to $out (sql: catalogged-$stamp.sql)"
