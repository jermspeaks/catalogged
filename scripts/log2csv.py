#!/usr/bin/env python3
"""Turn one year of the owner's markdown media log into a CSV for `catalogged match` / `import`.

    python3 -I scripts/log2csv.py <log.md> <year> <out.csv>

Reads lines like `- 01 🎥 Title (2010) (MOVIE)`, `- 19 📕 Title by Author`, `- 12 🎮 Title (1992)` and
`- 24 📺 Title, Season 3 (2023-2024)` under `## Month YYYY` headings. TMDB links, Letterboxd links and
the 🔁 rewatch mark are understood. The day in old years is a placeholder (the log says the day was never
tracked), so dates are only month-accurate. Output columns: kind,title,status,rating,review,year,source,
external_id,consumed_at,creator. Generated sheets contain personal data: keep them in `seeds/` (gitignored).
"""
import csv
import re
import sys
from collections import Counter

MONTHS = {m: i + 1 for i, m in enumerate("January February March April May June July August September October November December".split())}
KIND = {"🎥": "movie", "📕": "book", "🎮": "game", "📺": "show"}
COLS = "kind,title,status,rating,review,year,source,external_id,consumed_at,creator".split(",")


def parse(src: str, year: str) -> list[dict]:
    rows, month = [], None
    for line in open(src, encoding="utf-8"):
        line = line.rstrip("\n")
        h = re.match(r"## (\w+) (\d{4})", line)
        if h and h.group(1) in MONTHS and h.group(2) == year:
            month = MONTHS[h.group(1)]
            continue
        m = re.match(r"- (\d{2}) (🎥|📕|🎮|📺)\s*(🔁\s*)?(.*)$", line)
        if not (m and month):
            continue
        day, emoji, _rewatch, rest = m.groups()
        kind = KIND[emoji]
        tmdb = re.search(r"themoviedb\.org/(movie|tv)/(\d+)", rest)
        # Drop markdown links and bare [Letterboxd](url) style suffixes.
        rest = re.sub(r"\s*\[[^\]]*\]\([^)]*\)", "", rest)
        rest = re.sub(r"\s*\[[^\]]*\]\S*", "", rest)
        rest = re.sub(r"\s*\((MOVIE|SHOW|TV)\)\s*$", "", rest).strip()
        creator = ""
        if kind == "book" and " by " in rest:
            rest, creator = rest.rsplit(" by ", 1)
        y = ""
        ym = re.search(r"\s*\((\d{4})(?:-\d{4})?\)\s*$", rest)
        if ym:
            y = ym.group(1)
            rest = rest[: ym.start()]
        rows.append(dict(kind=kind, title=rest.strip(), status="consumed", rating="", review="",
                         year=y, source="tmdb" if tmdb else "", external_id=tmdb.group(2) if tmdb else "",
                         consumed_at=f"{year}-{month:02d}-{day}", creator=creator.strip()))
    return rows


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    src, year, out = sys.argv[1:]
    rows = parse(src, year)
    with open(out, "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, COLS)
        w.writeheader()
        w.writerows(rows)
    print(len(rows), dict(Counter(r["kind"] for r in rows)))
