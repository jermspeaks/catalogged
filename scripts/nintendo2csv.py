#!/usr/bin/env python3
"""Turn a Nintendo account library export (GraphQL JSON) into an import sheet.

Usage: python3 -I scripts/nintendo2csv.py seeds/nintendo-games.json seeds/nintendo-games.csv
Every game becomes a `wanted` entry (owned, not played yet). Run `catalogged match` next.
"""
import csv, json, re, sys

src, dst = sys.argv[1], sys.argv[2]
views = json.load(open(src))["data"]["account"]["vgc"]["vgcViewsV2"]["views"]
seen, rows = set(), []
for v in views:
    t = v["applicationName"].replace("®", "").replace("™", "").replace("’", "'")
    t = re.sub(r"\s+FOR NINTENDO SWITCH$", "", t, flags=re.I)
    t = re.sub(r"\s+for Nintendo Switch$", "", t, flags=re.I)
    t = re.sub(r"\s+", " ", t).strip()
    if t.lower() in seen:
        continue
    seen.add(t.lower())
    rows.append({"kind": "game", "title": t, "status": "wanted"})
with open(dst, "w", newline="") as f:
    w = csv.DictWriter(f, fieldnames=["kind", "title", "status"])
    w.writeheader()
    w.writerows(rows)
print(f"{len(rows)} games -> {dst}")
