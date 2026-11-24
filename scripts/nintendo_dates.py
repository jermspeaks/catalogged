#!/usr/bin/env python3
"""Parse a pasted Nintendo purchase history; match to catalogue games by title.

Usage: python3 -I scripts/nintendo_dates.py seeds/nintendo-games.txt index.json out.csv
Writes slug,title,consumed_at (earliest purchase date) and prints unmatched titles on both sides.
"""
import csv, json, re, sys, unicodedata
from datetime import datetime

def norm(s):
    s = re.sub(r"[®™]", "", s.replace("’", "'"))
    s = unicodedata.normalize("NFKD", s)
    s = "".join(c for c in s if not unicodedata.combining(c))
    s = re.sub(r"[®™]", "", s).lower()
    s = re.sub(r"\bfor nintendo switch\b", "", s)
    return re.sub(r"[^a-z0-9]+", " ", s).strip()

lines = [l.strip() for l in open(sys.argv[1], encoding="utf-8")]
date_re = re.compile(r"^[A-Z][a-z]+ \d\d, \d{4}$")
purchases, cur = [], None
for i, l in enumerate(lines):
    if date_re.match(l):
        cur = datetime.strptime(l, "%B %d, %Y").date().isoformat()
    elif l == "View details" and cur:
        purchases.append((cur, lines[i - 1]))
        cur = None
idx = json.load(open(sys.argv[2]))
idx = idx["items"]
games = [e for e in idx if e[0] == "game"]
by = {norm(e[2]): e for e in games}
best, miss = {}, []
for d, t in purchases:
    n = norm(t)
    e = by.get(n)
    if not e:
        # allow "<catalogue title> - Complete Edition" style extras
        c = [g for k, g in by.items() if len(k) > 3 and (n.startswith(k + " ") or k.startswith(n + " "))]
        e = c[0] if len(c) == 1 else None
    if not e:
        miss.append((d, t)); continue
    if e[1] not in best or d < best[e[1]][0]:
        best[e[1]] = (d, e[2])
with open(sys.argv[3], "w", newline="") as f:
    w = csv.writer(f); w.writerow(["slug", "title", "consumed_at"])
    for s, (d, t) in sorted(best.items(), key=lambda x: x[1][0]): w.writerow([s, t, d])
print(len(purchases), "purchases;", len(best), "games dated")
print("unmatched purchases:"); [print(" ", d, t) for d, t in miss]
print("games without a date:"); [print(" ", g[2]) for g in games if g[1] not in best]
