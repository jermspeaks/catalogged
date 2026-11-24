#!/usr/bin/env python3
"""Turn a Stremio library export (seeds/stremio.json) into an import sheet for review.

Usage:
  python3 -I scripts/stremio2csv.py seeds/stremio.json seeds/stremio.csv
The input is {"result": [["tmdb:123" | "tt0123456", timestamp_ms], ...]}. Each id is looked up on TMDB (GET
only; the key is read from TMDB_API_KEY or a TMDB_API_KEY= line in .env) to fill in kind, title and year.
Output columns: kind,title,status,year,external_id,consumed_at, then review columns the importer ignores:
  stremio_id, note. `note` flags rows to check: `ambiguous` (a bare tmdb id that is both a movie and a show;
  the movie is picked), `episode` (the id is an episode, not a title; the show is used), `not found`.
Dates are local, from the timestamp. Many ids share a few timestamps (a library sync, not a watch), so
days with >= --burst rows are marked `bulk date?`.
Nothing is sent to the Worker. Import later with `catalogged import seeds/stremio.csv --skip-existing`.
"""
import argparse, csv, json, os, re, sys, urllib.parse, urllib.request, urllib.error
from collections import Counter
from datetime import datetime

API = "https://api.themoviedb.org/3"


def key():
    k = os.environ.get("TMDB_API_KEY")
    if not k and os.path.exists(".env"):
        for line in open(".env"):
            m = re.match(r"\s*(?:export\s+)?TMDB_API_KEY=(.*)", line)
            if m:
                k = m.group(1).strip().strip("'\"")
    if not k:
        sys.exit("TMDB_API_KEY is not set")
    return k


def get(path, tok):
    url, headers = API + path, {"User-Agent": "catalogged-script"}
    if tok.count(".") == 2:  # v4 read-access token
        headers["Authorization"] = "Bearer " + tok
    else:
        url += ("&" if "?" in url else "?") + "api_key=" + urllib.parse.quote(tok)
    try:
        return json.load(urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=30))
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return None
        raise


def year(d):
    return (d or "")[:4]


def lookup(sid, tok):
    """-> (kind, tmdb_id, title, year, note)"""
    if sid.startswith("tt"):
        r = get(f"/find/{sid}?external_source=imdb_id", tok) or {}
        if r.get("movie_results"):
            m = r["movie_results"][0]
            return "movie", m["id"], m["title"], year(m.get("release_date")), ""
        if r.get("tv_results"):
            s = r["tv_results"][0]
            return "show", s["id"], s["name"], year(s.get("first_air_date")), ""
        if r.get("tv_episode_results"):
            e = r["tv_episode_results"][0]
            s = get(f"/tv/{e['show_id']}", tok) or {}
            return "show", e["show_id"], s.get("name", ""), year(s.get("first_air_date")), "episode"
        return "", "", "", "", "not found"
    num = sid.split(":", 1)[1]
    m, s = get(f"/movie/{num}", tok), get(f"/tv/{num}", tok)
    if m and s:
        return "movie", num, m["title"], year(m.get("release_date")), \
            f"ambiguous: also show '{s['name']}' ({year(s.get('first_air_date'))})"
    if m:
        return "movie", num, m["title"], year(m.get("release_date")), ""
    if s:
        return "show", num, s["name"], year(s.get("first_air_date")), ""
    return "", "", "", "", "not found"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("out")
    ap.add_argument("--burst", type=int, default=15, help="days with this many rows or more look like a bulk sync, not watching")
    a = ap.parse_args()
    tok = key()
    data = json.load(open(a.src))["result"]

    rows, seen = [], {}
    for sid, ts in data:
        kind, tid, title, yr, note = lookup(sid, tok)
        dt = datetime.fromtimestamp(ts / 1000)
        row = {"kind": kind, "title": title, "status": "consumed", "year": yr,
               "external_id": tid or (sid if sid.startswith("tt") else ""),
               "consumed_at": dt.date().isoformat(), "stremio_id": sid, "note": note, "_min": dt.strftime("%Y-%m-%d %H:%M")}
        dk = (kind, str(tid)) if tid else (sid,)
        old = seen.get(dk)
        if old:  # same title twice (e.g. a tmdb id and its tt id): keep the later date
            if row["consumed_at"] > old["consumed_at"]:
                old.update(consumed_at=row["consumed_at"], _min=row["_min"])
            old["note"] = (old["note"] + "; " if old["note"] else "") + "duplicate id merged"
            continue
        seen[dk] = row
        rows.append(row)

    per_day = Counter(r["consumed_at"] for r in rows)
    for r in rows:
        if per_day[r["consumed_at"]] >= a.burst:
            r["note"] = (r["note"] + "; " if r["note"] else "") + "bulk date?"
    rows.sort(key=lambda r: (r["consumed_at"], r["title"]))
    cols = ["kind", "title", "status", "year", "external_id", "consumed_at", "stremio_id", "note"]
    with open(a.out, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=cols, extrasaction="ignore")
        w.writeheader()
        w.writerows(rows)
    c = Counter(r["kind"] or "unresolved" for r in rows)
    flag = lambda s: sum(1 for r in rows if s in r["note"])
    print(f"{len(data)} ids -> {len(rows)} rows in {a.out}: {dict(c)}")
    print(f"ambiguous: {flag('ambiguous')} | episode ids: {flag('episode')} | not found: {flag('not found')} | "
          f"bulk date?: {flag('bulk date')} | duplicates merged: {flag('duplicate')}")


main()
