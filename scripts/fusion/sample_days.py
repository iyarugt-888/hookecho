"""Draw a reproducible random sample of ordinary severe-weather radar windows (detectionplan.md,
Phase 14: calibration needs a corpus whose base rate means something).

    python scripts/fusion/sample_days.py YEAR N SEED > docs/backtest-sample-YEAR.txt
    python scripts/fusion/sample_days.py YEAR N SEED quiet > docs/backtest-quiet-YEAR.txt

Every hand-picked corpus event so far was chosen as a tornado case or a hard negative, which fixes
the base rate by design. Here a radar-day qualifies if any severe report (tornado, hail, wind)
falls within 150 km of the radar between March and August; N of them are drawn at random (seeded),
and each window starts 20 minutes before one of that day's reports, also drawn at random. The
sample is what an operator watching a random severe day would see, tornadoes or not.
Radar-days already in docs/backtest-events.txt are left out.

`quiet` draws the opposite: radar-days with no severe report within 150 km, each window starting at
a random minute of the radar day. Every Tornado detection marker there is a false alarm, so it
measures what clutter, anomalous propagation, birds and ordinary showers cost on the days that
make up most of the year.
"""

import datetime as dt
import json
import math
import random
import re
import sys
import urllib.request

RADARS = {
    "KTLX": (35.333, -97.278), "KFDR": (34.362, -98.976), "KVNX": (36.741, -98.128),
    "KINX": (36.175, -95.565), "KICT": (37.655, -97.443), "KDDC": (37.761, -99.969),
    "KTWX": (38.997, -96.232), "KEAX": (38.810, -94.264), "KSGF": (37.235, -93.400),
    "KLSX": (38.699, -90.683), "KOAX": (41.320, -96.367), "KUEX": (40.321, -98.442),
    "KDMX": (41.731, -93.723), "KDVN": (41.612, -90.581), "KARX": (43.823, -91.191),
    "KMPX": (44.849, -93.566), "KFSD": (43.588, -96.729), "KABR": (45.456, -98.413),
    "KLOT": (41.604, -88.085), "KILX": (40.151, -89.337), "KIND": (39.708, -86.280),
    "KILN": (39.420, -83.822), "KIWX": (41.359, -85.700), "KGRR": (42.894, -85.545),
    "KDTX": (42.700, -83.472), "KPAH": (37.068, -88.772), "KLVX": (37.975, -85.944),
    "KOHX": (36.247, -86.563), "KNQA": (35.345, -89.873), "KLZK": (34.836, -92.262),
    "KSHV": (32.451, -93.841), "KFWS": (32.573, -97.303), "KEWX": (29.704, -98.029),
    "KHGX": (29.472, -95.079), "KLCH": (30.125, -93.216), "KLIX": (30.337, -89.826),
    "KDGX": (32.280, -89.984), "KGWX": (33.897, -88.329), "KBMX": (33.172, -86.770),
    "KHTX": (34.931, -86.084), "KFFC": (33.364, -84.566), "KJGX": (32.675, -83.351),
    "KCAE": (33.949, -81.118), "KGSP": (34.883, -82.220), "KRAX": (35.665, -78.490),
    "KAKQ": (36.984, -77.007), "KLWX": (38.976, -77.487), "KDIX": (39.947, -74.411),
    "KAMA": (35.233, -101.709), "KLBB": (33.654, -101.814), "KGLD": (39.367, -101.700),
    "KFTG": (39.786, -104.546), "KMAF": (31.943, -102.189), "KMLB": (28.113, -80.654),
}


def km(a, b):
    la1, lo1, la2, lo2 = map(math.radians, (*a, *b))
    h = math.sin((la2 - la1) / 2) ** 2 + math.cos(la1) * math.cos(la2) * math.sin((lo2 - lo1) / 2) ** 2
    return 6371 * 2 * math.asin(math.sqrt(h))


def month(year, m):
    a = dt.datetime(year, m, 1)
    b = dt.datetime(year + (m == 12), m % 12 + 1, 1)
    url = (f"https://mesonet.agron.iastate.edu/geojson/lsr.geojson?sts={a:%Y-%m-%dT%H:%MZ}"
           f"&ets={b:%Y-%m-%dT%H:%MZ}&west=-106&east=-73&south=25&north=50")
    with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "hookecho-backtest"}),
                                timeout=180) as r:
        d = json.load(r)
    out = []
    for f in d["features"]:
        p = f["properties"]
        if p.get("lat") is None or p["type"] not in ("T", "H", "G", "D"):
            continue
        out.append((dt.datetime.strptime(p["valid"], "%Y-%m-%dT%H:%M:%SZ"), p["lat"], p["lon"]))
    return out


def main():
    year, n, seed = int(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])
    quiet = sys.argv[4:] == ["quiet"]
    used = set()
    for line in open("docs/backtest-events.txt", encoding="utf-8"):
        m = re.match(r"^(K[A-Z]{3}) (\d{4}-\d\d-\d\d \d\d:\d\d)", line)
        if m:
            # The same 12Z-to-12Z radar day the pool is keyed by: an overnight event (Nashville,
            # 2020-03-03 06:30Z) belongs to the day before its date.
            t = dt.datetime.strptime(m.group(2), "%Y-%m-%d %H:%M")
            used.add((m.group(1), (t - dt.timedelta(hours=12)).strftime("%Y-%m-%d")))
    days = {}
    for m in range(3, 9):
        for t, lat, lon in month(year, m):
            for site, pos in RADARS.items():
                if km(pos, (lat, lon)) <= 150:
                    # A radar day runs 12Z to 12Z, so an evening's storms stay on one day.
                    day = (t - dt.timedelta(hours=12)).strftime("%Y-%m-%d")
                    days.setdefault((site, day), []).append(t)
    if quiet:
        # Every radar-day of the season with no severe report within 150 km.
        start, end = dt.date(year, 3, 1), dt.date(year, 9, 1)
        every = {(site, (start + dt.timedelta(days=i)).strftime("%Y-%m-%d"))
                 for site in RADARS for i in range((end - start).days)}
        pool = sorted(k for k in every if k not in days and k not in used)
        rng = random.Random(seed)
        picks = rng.sample(pool, n)
        print(f"# Quiet radar windows (no severe report within 150 km that radar day), {year} "
              f"March-August, seed {seed} ({n} of {len(pool)} quiet radar-days):")
        print(f"#   python scripts/fusion/sample_days.py {year} {n} {seed} quiet")
        for site, day in sorted(picks):
            t = dt.datetime.strptime(day, "%Y-%m-%d") + dt.timedelta(hours=12, minutes=rng.randrange(1440))
            print(f"{site} {t:%Y-%m-%d %H:%M}   # quiet radar-day")
        return
    pool = sorted(k for k in days if k not in used)
    rng = random.Random(seed)
    picks = rng.sample(pool, n)
    print(f"# Random severe-weather radar windows, {year} March-August, seed {seed} "
          f"({n} of {len(pool)} qualifying radar-days):")
    print(f"#   python scripts/fusion/sample_days.py {year} {n} {seed}")
    for site, day in sorted(picks):
        t = rng.choice(sorted(days[(site, day)])) - dt.timedelta(minutes=20)
        print(f"{site} {t:%Y-%m-%d %H:%M}   # {len(days[(site, day)])} severe reports that radar-day")


if __name__ == "__main__":
    main()
