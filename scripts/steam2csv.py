#!/usr/bin/env python3
"""Turn pasted Steam pages into an import sheet.

Usage: python3 -I scripts/steam2csv.py seeds/steam.txt seeds/steam-play.txt seeds/steam.csv

  steam.txt       the purchase history page (store.steampowered.com/account/licenses), pasted as text:
                  `Date <tab> Item <tab> Acquisition Method` rows.
  steam-play.txt  the library page with playtime (name, "TOTAL PLAYED 12.3 hours LAST PLAYED Mar 17, 2019").

Rules: every game with any playtime is `consumed` (consumed_at = last played, blank if Steam shows none;
a date without a year is the current year), with the hours in `review`. Purchased games with no playtime
are `wanted`. Complimentary packages, free promotions and guest passes are skipped, as are bundles, DLC, soundtracks and ads. Edition suffixes
("- Standard Edition") and the trademark signs are dropped so IGDB can match. Run `catalogged match -i` next.
"""
import csv, re, sys
from datetime import datetime

purchases_path, play_path, dst = sys.argv[1:4]
YEAR = datetime.now().year


def clean(t):
    t = t.replace("®", "").replace("™", "").replace("’", "'")
    t = re.sub(r"\s*-?\s*(Standard|Deluxe|Digital Deluxe|Ultimate)\s+Edition$", "", t, flags=re.I)
    t = re.sub(r"\s+(Retail|\(WW\)|\(RoW\))$", "", t.strip())
    return re.sub(r"\s+", " ", t).strip()


# Things in the purchase history that are not a game of their own: bundles, packs, DLC, soundtracks, ads.
NOT_A_GAME = re.compile(r"bundle|starter pack|\d-pack|four pack|triple pack|complete pack|\bDLCs?\b|soundtrack|^HB tier|advertising|backer key", re.I)


def key(t):
    return re.sub(r"[^\w]+", "", clean(t).lower())


def iso(s):
    s = (s or "").strip()
    for fmt, suffix in (("%b %d, %Y", ""), ("%b %d %Y", f" {YEAR}")):
        try:
            return datetime.strptime(s.replace(",", "") + suffix if suffix else s, fmt).strftime("%Y-%m-%d")
        except ValueError:
            pass
    return ""


def hours_text(n, unit):
    n = float(n.replace(",", ""))
    if unit.startswith("minute"):
        return f"{n:g} minute{'s' if n != 1 else ''} on Steam"
    return f"{n:g} hour{'s' if n != 1 else ''} on Steam"


# playtime: the line before "TOTAL PLAYED" is the game name
lines = [l.strip() for l in open(play_path, encoding="utf-8").read().split("\n")]
played = {}
for i, l in enumerate(lines):
    m = re.match(r"TOTAL PLAYED\s*([\d.,]+) (hours?|minutes?)(?:LAST PLAYED(.*))?$", l)
    if m and i:
        played.setdefault(key(lines[i - 1]), (clean(lines[i - 1]), hours_text(m.group(1), m.group(2)), iso(m.group(3))))

# purchases
SKIP_METHOD = {"Complimentary", "Gift/Guest Pass"}
bought, not_games = {}, []
for l in open(purchases_path, encoding="utf-8").read().split("\n"):
    parts = [p.strip() for p in l.split("\t")]
    if len(parts) != 3 or not re.match(r"^[A-Z][a-z]{2} \d+, \d{4}$", parts[0]):
        continue
    _, item, method = parts
    if method in SKIP_METHOD or re.search(r"Free Promotional Package", item):
        continue
    if NOT_A_GAME.search(item):
        not_games.append(clean(item))
        continue
    bought.setdefault(key(item), clean(item))

rows = []
for k, (title, review, date) in played.items():
    rows.append({"kind": "game", "title": title, "status": "consumed", "review": review, "consumed_at": date})
for k, title in bought.items():
    if k not in played:
        rows.append({"kind": "game", "title": title, "status": "wanted", "review": "", "consumed_at": ""})

with open(dst, "w", newline="", encoding="utf-8") as f:
    w = csv.DictWriter(f, fieldnames=["kind", "title", "status", "review", "consumed_at"])
    w.writeheader()
    w.writerows(rows)
n_c = sum(r["status"] == "consumed" for r in rows)
print(f"{len(rows)} games -> {dst}: {n_c} consumed, {len(rows) - n_c} wanted")
print(f"  skipped as not a game of their own: {len(not_games)} ({', '.join(not_games[:6])}, ...)")
print(f"  played but not in the purchase list: {sum(k not in bought for k in played)} (free to play, gifts, promos)")
