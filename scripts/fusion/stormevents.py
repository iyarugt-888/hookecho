"""Verify Tornado detection against NOAA's Storm Events database: the official record of every US
tornado, with where it began and ended (detectionplan.md, "Storm Events as truth").

    python scripts/fusion/stormevents.py fetch 2018 2019 ...      # once: the yearly detail files
    python scripts/fusion/stormevents.py DIR [DIR...]             # exports with tornado_marker rows

The backtest's own truth is local storm reports and NWS damage-survey paths, counted per report or
path. Here every tornado counts once: those on the ground while a window was scanned and within 150 km of its
radar, and found when a marker lies within 10 km of its begin-end segment between 15 minutes
before it began and 15 after it ended. Printed per EF rating, with the markers that matched no
report or survey but lie on a Storm Events track (truth the reports missed).

The detail files (`StormEvents_details-ftp_v1.0_dYYYY_c*.csv.gz`, ~10-16 MB each, public domain)
go in target/backtest-cache/stormevents/. Times there are local standard time; CZ_TIMEZONE
("CST-6") gives the offset.
"""

import collections
import csv
import datetime as dt
import glob
import gzip
import math
import os
import re
import sys
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sample_days import RADARS  # noqa: E402

CACHE = "target/backtest-cache/stormevents"
BASE = "https://www.ncei.noaa.gov/pub/data/swdi/stormevents/csvfiles/"
RADIUS_KM, WINDOW_MIN, RANGE_KM = 10.0, 15, 150.0
# Corpus radars outside the sampler's list (positions from the site registry, nexrad-model).
MORE_RADARS = {
    "KATX": (48.1944, -122.4958), "KBOX": (41.9558, -71.1369), "KCLX": (32.6556, -81.0425),
    "KGRK": (30.7217, -97.3828), "KMHX": (34.7761, -76.8764), "KMKX": (42.9678, -88.5506),
    "KMUX": (37.1553, -121.8983), "KPBZ": (40.5317, -80.0183),
}
MON = {m: i + 1 for i, m in enumerate("JAN FEB MAR APR MAY JUN JUL AUG SEP OCT NOV DEC".split())}


def fetch(years):
    os.makedirs(CACHE, exist_ok=True)
    listing = urllib.request.urlopen(BASE, timeout=120).read().decode("utf-8", "replace")
    for y in years:
        names = sorted(set(re.findall(rf"StormEvents_details-ftp_v1\.0_d{y}_c\d+\.csv\.gz", listing)))
        if not names:
            print(f"{y}: no detail file listed")
            continue
        name = names[-1]
        path = os.path.join(CACHE, name)
        if os.path.exists(path):
            print(f"{y}: {name} already cached")
            continue
        urllib.request.urlretrieve(BASE + name, path)
        print(f"{y}: {name} ({os.path.getsize(path)} bytes)")


def epoch_min(t):
    return int((t - dt.datetime(1970, 1, 1)).total_seconds() // 60)


def utc(stamp, zone):
    """'28-APR-14 19:25:00' in the local standard time of zone 'CST-6', as UTC."""
    day, hms = stamp.split(" ")
    dd, mon, yy = day.split("-")
    h, m, _ = hms.split(":")
    t = dt.datetime(2000 + int(yy), MON[mon.upper()], int(dd), int(h), int(m))
    return t - dt.timedelta(hours=int(zone[3:]) if len(zone) > 3 else 0)


def tornadoes():
    """(begin minute, end minute, begin (lon, lat), end (lon, lat), rating) for every tornado."""
    out = []
    for path in sorted(glob.glob(os.path.join(CACHE, "StormEvents_details-*.csv.gz"))):
        with gzip.open(path, "rt", encoding="latin-1") as fh:
            for r in csv.DictReader(fh):
                if r["EVENT_TYPE"] != "Tornado":
                    continue
                try:
                    b = (float(r["BEGIN_LON"]), float(r["BEGIN_LAT"]))
                    e = (float(r["END_LON"] or r["BEGIN_LON"]), float(r["END_LAT"] or r["BEGIN_LAT"]))
                    t0 = epoch_min(utc(r["BEGIN_DATE_TIME"], r["CZ_TIMEZONE"]))
                    t1 = epoch_min(utc(r["END_DATE_TIME"], r["CZ_TIMEZONE"]))
                except (ValueError, KeyError, IndexError):
                    continue
                out.append((t0, max(t0, t1), b, e, r["TOR_F_SCALE"] or "?"))
    return out


def ground_km(a, b):
    la1, lo1, la2, lo2 = map(math.radians, (a[1], a[0], b[1], b[0]))
    h = math.sin((la2 - la1) / 2) ** 2 + math.cos(la1) * math.cos(la2) * math.sin((lo2 - lo1) / 2) ** 2
    return 6371 * 2 * math.asin(math.sqrt(h))


def segment_km(p, a, b):
    """Distance (km) from p to the segment a-b, on a flat projection about p."""
    k = math.cos(math.radians(p[1]))
    ax, ay = (a[0] - p[0]) * 111.32 * k, (a[1] - p[1]) * 110.57
    bx, by = (b[0] - p[0]) * 111.32 * k, (b[1] - p[1]) * 110.57
    dx, dy = bx - ax, by - ay
    length2 = dx * dx + dy * dy
    t = 0.0 if length2 == 0 else max(0.0, min(1.0, -(ax * dx + ay * dy) / length2))
    return math.hypot(ax + t * dx, ay + t * dy)


def on_track(minute, p, t):
    return t[0] - WINDOW_MIN <= minute <= t[1] + WINDOW_MIN and segment_km(p, t[2], t[3]) <= RADIUS_KM


def evaluate(dirs):
    tors = tornadoes()
    if not tors:
        sys.exit(f"no Storm Events files in {CACHE}: run `stormevents.py fetch YEAR...` first")
    radars = {**{k: (v[0], v[1]) for k, v in RADARS.items()}, **MORE_RADARS}
    total, found = collections.Counter(), collections.Counter()
    unreported = collections.Counter()
    skipped = set()
    for d in dirs:
        windows = collections.defaultdict(lambda: [10**12, 0])
        marks = collections.defaultdict(list)
        for r in csv.DictReader(open(os.path.join(d, "candidates.csv"), encoding="utf-8")):
            w = windows[r["event"]]
            m = int(r["minute"])
            w[0], w[1] = min(w[0], m), max(w[1], m)
            if r["detector"] == "tornado_marker":
                marks[r["event"]].append((m, (float(r["lon"]), float(r["lat"])), r))
        for event, (a, b) in windows.items():
            site = event[:4]
            if site not in radars:
                skipped.add(site)
                continue
            lat, lon = radars[site]
            # On the ground while the window was scanned: one that began after its last volume
            # or ended before its first cannot be seen in it.
            near = [t for t in tors if t[1] >= a and t[0] <= b
                    and min(ground_km((lon, lat), t[2]), ground_km((lon, lat), t[3])) <= RANGE_KM]
            for t in near:
                total[t[4]] += 1
                if any(on_track(m, p, t) for m, p, _ in marks[event]):
                    found[t[4]] += 1
            for m, p, r in marks[event]:
                if not r["matched_truths"] and any(on_track(m, p, t) for t in near):
                    tier = "Likely+" if r["tier"] != "Tornado possible" else "Possible"
                    unreported[tier] += 1
    n, f = sum(total.values()), sum(found.values())
    print(f"{', '.join(dirs)}")
    print(f"  Storm Events tornadoes in a window, within {RANGE_KM:.0f} km: {n}; "
          f"found by a marker: {f} ({f / max(n, 1):.0%})")
    for rating in sorted(total):
        print(f"    {rating:4} {found[rating]:4d} of {total[rating]:4d}")
    print(f"  markers matching no report or survey but on a Storm Events track: "
          f"Possible {unreported['Possible']}, Likely+ {unreported['Likely+']}")
    if skipped:
        print(f"  skipped (no radar position): {', '.join(sorted(skipped))}")


if __name__ == "__main__":
    if sys.argv[1:2] == ["fetch"]:
        fetch(sys.argv[2:])
    elif sys.argv[1:]:
        evaluate(sys.argv[1:])
    else:
        sys.exit(__doc__)
