#!/usr/bin/env python3
"""Turn one year of the owner's markdown media log into a CSV for `catalogged match` / `import`.

    python3 -I scripts/log2csv.py <log.md> <year> <out.csv>

Reads lines like `- 01 🎥 Title (2010) (MOVIE)`, `- 19 📕 Title by Author`, `- 12 🎮 Title (1992)` and
`- 24 📺 Title, Season 3 (2023-2024)` under `## Month YYYY` (or, from 2016, `## Month`) headings. TMDB links, Letterboxd links and
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
MONTHS["Febrary"] = 2  # a typo in the 2019 log
COLS = "kind,title,status,rating,review,year,source,external_id,consumed_at,creator".split(",")


def parse(src: str, year: str) -> list[dict]:
    rows, month = [], None
    for line in open(src, encoding="utf-8"):
        line = line.rstrip("\n")
        h = re.match(r"## (\w+)(?: (\d{4}))?\s*$", line)
        if h and h.group(1) in MONTHS and h.group(2) in (None, year):
            month = MONTHS[h.group(1)]
            continue
        m = re.match(r"- (\d{2}) (?:(🎥|📕|🎮|📺)\s*)?(🔁\s*)?(.*)$", line)
        if not (m and month):
            continue
        day, emoji, _rewatch, rest = m.groups()
        if emoji:
            kind = KIND[emoji]
        else:
            # No emoji (2022: `- 28 Title by Author (BOOK)`): the trailing marker names the kind. Lines with another
            # emoji (🎵 concert, 🎤 musical) and the monthly summary lines have neither, so they are skipped.
            mk = re.search(r"\((MOVIE|SHOW|TV|BOOK|GAME)\)\s*$", rest)
            if not mk:
                continue
            kind = {"MOVIE": "movie", "SHOW": "show", "TV": "show", "BOOK": "book", "GAME": "game"}[mk.group(1)]
        tmdb = re.search(r"themoviedb\.org/(movie|tv)/(\d+)", rest)
        # Drop markdown links (also wrapped in parentheses, as in `([Letterboxd](url))`) and bare [Letterboxd](url) style suffixes.
        rest = re.sub(r"\s*\(\s*\[[^\]]*\]\([^)]*\)\s*\)", "", rest)
        rest = re.sub(r"\s*\[[^\]]*\]\([^)]*\)", "", rest)
        rest = re.sub(r"\s*\[[^\]]*\]\S*", "", rest)
        rest = re.sub(r"\s*\((MOVIE|SHOW|TV|BOOK|GAME)\)\s*$", "", rest).strip()
        rest = re.sub(r"(\s*(🔁|Letterboxd|-))+\s*$", "", rest).strip()  # `Title (2003) 🔁`, `TÁR (2022) Letterboxd`, `Title (2023) -`
        rest = re.sub(r"\\([^\w\s])", r"\1", rest)  # markdown escapes such as Slide\:ology
        creator = ""
        if kind == "book" and " by " in rest:
            rest, creator = rest.rsplit(" by ", 1)
        am = re.search(r"\s+-\s+([^-]+?)\s*-?\s*$", rest) if kind == "book" else None
        if am and not creator:  # `The Psychology of Money (2020) - Morgan Housel -`
            creator, rest = am.group(1), rest[: am.start()]
        season = ""
        if kind == "show":
            # `Title (Season 1) (2024)`, `Title - Season 4 (2022)`, `Title, Seasons 1-3`, `Title (Series 15 - 2024)`:
            # the season goes to `review`; the year is the season's, not the show's, so it is dropped.
            sm = re.search(r"\s*\(((?:all )?(?:Season|Series)s?\b[^)]*)\)", rest) or re.search(r"\s*[-,]\s*((?:Season|Series)s?\s+[\d, -]+?)(?=\s*(\(|$))", rest)
            if sm:
                season = re.sub(r"\s*-?\s*\d{4}$", "", sm.group(1)).strip()
                rest = rest[: sm.start()] + rest[sm.end():]
        rest = re.sub(r'^"(.*)"(?=\s*(\(\d{4}(-\d{4})?\))?\s*$)', r"\1", rest.strip())  # "Now, Voyager" (1942)
        y = ""
        ym = re.search(r"\s*\((\d{4})(?:\s*-\s*\d{4})?\)\s*$", rest)
        if ym:
            y = "" if season else ym.group(1)
            rest = rest[: ym.start()]
        rows.append(dict(kind=kind, title=rest.strip(), status="consumed", rating="", review=season,
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
