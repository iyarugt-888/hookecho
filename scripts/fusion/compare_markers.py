"""Compare the app's markers between two backtest exports of the same manifest, over the events
both runs actually processed (a window one run could not load is left out of both).

    python scripts/fusion/compare_markers.py BASE_DIR NEW_DIR

Radar-hours for the common events are taken from the base run, after checking that both runs
read the same volumes for each of them (the distinct volumes in their candidate rows); events
whose volumes differ are reported and left out too. Prints, for each run, markers, false markers
per radar-hour, false episodes per radar-hour and truths found, at every marker and at Likely and
up, as markers.py does.
"""

import collections
import csv
import json
import os
import sys


def load(d):
    summary = json.load(open(os.path.join(d, "summary.json"), encoding="utf-8"))
    rows = collections.defaultdict(list)
    vols = collections.defaultdict(set)
    for r in csv.DictReader(open(os.path.join(d, "candidates.csv"), encoding="utf-8")):
        vols[r["event"]].add(r["volume"])
        if r["detector"] == "tornado_marker":
            rows[r["event"]].append(r)
    return summary, rows, vols


def matched(r):
    return [(r["event"], t) for t in r["matched_truths"].split(";") if t]


def stats(rows, events, truths, hours, likely):
    out = []
    for label, keep in (("all", lambda r: True), ("Likely+", likely)):
        kept = [r for e in events for r in rows.get(e, []) if keep(r)]
        false = sum(1 for r in kept if not r["matched_truths"])
        found = {t for r in kept for t in matched(r)} & truths
        tracks = collections.defaultdict(bool)
        for r in kept:
            tracks[(r["event"], r["track_id"])] |= bool(r["matched_truths"])
        fe = sum(1 for v in tracks.values() if not v)
        out.append(
            f"  {label:8} markers {len(kept):5d}  false {false:5d} ({false / hours:5.2f}/h)  "
            f"false episodes {fe:4d} ({fe / hours:4.2f}/h)  found {len(found):4d} of "
            f"{len(truths)} (POD {len(found) / max(len(truths), 1):.3f})"
        )
    return out


def main():
    base, new = sys.argv[1], sys.argv[2]
    sb, rb, vb = load(base)
    sn, rn, vn = load(new)
    common = [e for e in sb["events"] if e in sn["events"]]
    same = [e for e in common if vb.get(e, set()) == vn.get(e, set())]
    differ = sorted(set(common) - set(same))
    only = sorted(set(sb["events"]) ^ set(sn["events"]))
    total_vols = sum(len(vb.get(e, ())) for e in sb["events"])
    kept_vols = sum(len(vb.get(e, ())) for e in same)
    hours = sb["detectors"]["tornado_fusion"]["radar_hours"] * kept_vols / max(total_vols, 1)
    truths = {
        (e, i) for e, ids in sb["tornado_truth_minutes"].items() if e in same for i in ids
    }
    print(f"{len(same)} events compared ({len(only)} in one run only, {len(differ)} with "
          f"different volumes left out), {hours:.1f} radar-hours, {len(truths)} truths")
    likely = lambda r: r["tier"] != "Tornado possible"  # noqa: E731
    for name, rows in (("base", rb), ("new", rn)):
        print(name)
        for line in stats(rows, same, truths, hours, likely):
            print(line)


if __name__ == "__main__":
    main()
