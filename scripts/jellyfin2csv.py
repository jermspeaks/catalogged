#!/usr/bin/env python3
"""Export a Jellyfin user's watch history as an import sheet (read-only: GET requests only).

Usage:
  JELLYFIN_API_KEY=... python3 -I scripts/jellyfin2csv.py --url http://host:8096 --user USER_ID seeds/jellyfin.csv
The key is read from the JELLYFIN_API_KEY environment variable, or from a JELLYFIN_API_KEY= line in .env.
Movies with a play become `consumed` dated by their last play. A series becomes `consumed` when every
episode is played, otherwise `in_progress` (the date is the newest played episode). --min-percent drops
series that are barely started. Rows carry the TMDB id (or an IMDb tt id when there is no TMDB id), so
`catalogged import` needs no title matching. Overlap with the public catalogue is reported, not removed:
use `import --skip-existing`.
"""
import argparse, csv, json, os, re, sys, urllib.request
from datetime import datetime


def key():
    k = os.environ.get("JELLYFIN_API_KEY")
    if not k and os.path.exists(".env"):
        for line in open(".env"):
            m = re.match(r"\s*(?:export\s+)?JELLYFIN_API_KEY=(.*)", line)
            if m:
                k = m.group(1).strip().strip("'\"")
    if not k:
        sys.exit("JELLYFIN_API_KEY is not set")
    return k


def get(base, token, path):
    req = urllib.request.Request(base + path, headers={"Authorization": f'MediaBrowser Token="{token}"'})
    return json.load(urllib.request.urlopen(req, timeout=120))


def items(base, token, user, types, extra="", fields="ProviderIds,ProductionYear"):
    q = f"/Users/{user}/Items?Recursive=true&IncludeItemTypes={types}&Fields={fields}{extra}"
    return get(base, token, q)["Items"]


def local_date(s):
    if not s:
        return ""
    return datetime.fromisoformat(s[:26].rstrip("Z") + "+00:00").astimezone().date().isoformat()


def ident(it, kind):
    p = it.get("ProviderIds") or {}
    if p.get("Tmdb"):
        return kind, p["Tmdb"]
    if p.get("Imdb"):
        return "", p["Imdb"]  # an IMDb id fills in kind and title from TMDB
    return kind, ""


def norm(t):
    return re.sub(r"[^a-z0-9]+", " ", t.lower()).strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--url", default=os.environ.get("JELLYFIN_URL", "http://jeremy-and-morgan-ser.local:8096"))
    ap.add_argument("--user", required=True)
    ap.add_argument("--catalogue", default="https://catalog.05152024.xyz/index.json")
    ap.add_argument("--min-percent", type=float, default=0.0, help="drop series with less than this %% of episodes played")
    a = ap.parse_args()
    tok = key()

    rows = {}  # (kind, dedupe id) -> row

    def put(kind, it, status, date):
        k, ext = ident(it, kind)
        row = {"kind": k, "title": it["Name"], "status": status, "year": it.get("ProductionYear") or "",
               "external_id": ext, "consumed_at": date, "_kind": kind}
        dk = (kind, ext or norm(it["Name"]))
        old = rows.get(dk)
        # duplicates (the same title twice in the library): prefer consumed, then the later date
        if not old or (status, date) > (old["status"], old["consumed_at"]):
            rows[dk] = row

    for m in items(a.url, tok, a.user, "Movie", "&Filters=IsPlayed"):
        put("movie", m, "consumed", local_date((m.get("UserData") or {}).get("LastPlayedDate")))

    last = {}
    for e in items(a.url, tok, a.user, "Episode", "&Filters=IsPlayed", fields="ProviderIds"):
        d = local_date((e.get("UserData") or {}).get("LastPlayedDate"))
        if e.get("SeriesId") and d > last.get(e["SeriesId"], ""):
            last[e["SeriesId"]] = d
        elif e.get("SeriesId"):
            last.setdefault(e["SeriesId"], d)
    partial = []
    for s in items(a.url, tok, a.user, "Series"):
        if s["Id"] not in last:
            continue
        u = s.get("UserData") or {}
        pct = u.get("PlayedPercentage") or 0
        done = u.get("Played") or u.get("UnplayedItemCount") == 0
        if not done:
            if pct < a.min_percent:
                continue
            partial.append((pct, s["Name"]))
        put("show", s, "consumed" if done else "in_progress", last[s["Id"]])

    out = sorted(rows.values(), key=lambda r: (r["consumed_at"], r["title"]))
    cols = ["kind", "title", "status", "year", "external_id", "consumed_at"]
    with open(a.out, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=cols, extrasaction="ignore")
        w.writeheader()
        w.writerows(out)

    try:
        req = urllib.request.Request(a.catalogue, headers={"User-Agent": "catalogged-script"})
        have = {(e[0], norm(e[2])) for e in json.load(urllib.request.urlopen(req, timeout=60))["items"]}
        dup = [r for r in out if (r["_kind"], norm(r["title"])) in have]
    except Exception as ex:
        dup = None
        print("could not read the catalogue:", ex)
    n = lambda s: sum(1 for r in out if r["status"] == s)
    print(f"{len(out)} rows -> {a.out}: {sum(1 for r in out if r['_kind']=='movie')} movies, "
          f"{sum(1 for r in out if r['_kind']=='show')} shows ({n('consumed')} consumed incl. movies, {n('in_progress')} in progress)")
    print("no provider id:", sum(1 for r in out if not r["external_id"]), "| no date:", sum(1 for r in out if not r["consumed_at"]))
    if dup is not None:
        print(f"already in the catalogue by title: {len(dup)} (import --skip-existing will drop them)")
    if partial:
        print("partly watched series (percent played):")
        for pct, name in sorted(partial, reverse=True):
            print(f"  {pct:5.1f}%  {name}")


main()
