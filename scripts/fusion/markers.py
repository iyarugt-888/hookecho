"""Verify what the map draws: the `tornado_marker` rows of one or more backtest exports
(detectionplan.md, "The backtest exports the app's markers").

    hookecho --headless-backtest-file docs/backtest-sample-2021.txt --export DIR
    python scripts/fusion/markers.py DIR [DIR...]

Each `tornado_marker` row is one Tornado detection marker the app would draw for that volume,
from radar evidence only (no report or warning confirmation): `llsd_analyst::circulations_with`
with the default rotation-only bar, the storm-core rule, the wind-turbine mask and the 15 km fold.
So this needs no re-derivation of those rules, and stays right when they change.

Prints, for the markers and for the original Tornado ID (`tornado_id` rows), false markers per
radar-hour and the share of tornado truths found, at every marker (Possible and up) and at Likely
and up; then why each missed truth was missed, from the best fused column (`tornado_fusion` row,
15-150 km) that matched it, if any.
"""

import collections
import csv
import json
import os
import sys


def load(dirs):
    rows, hours, truths = collections.defaultdict(list), 0.0, set()
    for d in dirs:
        summary = json.load(open(os.path.join(d, "summary.json"), encoding="utf-8"))
        if "tornado_marker" not in summary["detectors"]:
            sys.exit(f"{d}: no tornado_marker detector in this export; it predates them, rerun the "
                     "backtest")
        hours += summary["detectors"]["tornado_fusion"]["radar_hours"]
        for event, ids in summary["tornado_truth_minutes"].items():
            truths |= {(event, i) for i in ids}
        for r in csv.DictReader(open(os.path.join(d, "candidates.csv"), encoding="utf-8")):
            rows[r["detector"]].append(r)
    return rows, hours, truths


def matched(r):
    return [(r["event"], t) for t in r["matched_truths"].split(";") if t]


def rates(name, rows, hours, truths, likely):
    """False per radar-hour and truths found, at every row and at Likely and up. Where the rows
    carry their track (`track_id`, markers since the SAILS steps), also false episodes: tracks
    none of whose markers matched a truth, which do not multiply when the markers are drawn at
    every low-level pass rather than once a volume."""
    for label, keep in (("all", lambda r: True), ("Likely+", likely)):
        kept = [r for r in rows if keep(r)]
        false = sum(1 for r in kept if not r["matched_truths"])
        found = {t for r in kept for t in matched(r)} & truths
        episodes = ""
        if kept and all(r.get("track_id") for r in kept):
            tracks = collections.defaultdict(bool)
            for r in kept:
                tracks[(r["event"], r["track_id"])] |= bool(r["matched_truths"])
            fe = sum(1 for v in tracks.values() if not v)
            episodes = f"  false episodes {fe:4d} ({fe / hours:4.2f}/h)"
        print(f"  {name:9} {label:8} markers {len(kept):5d}  false {false:5d} "
              f"({false / hours:5.2f}/h)  found {len(found):4d} of {len(truths)} "
              f"(POD {len(found) / max(len(truths), 1):.2f}){episodes}")


def f(r, k):
    return float(r[k] or 0)


def why(r):
    """The first rule, in the app's order, that keeps this fused column from being a marker."""
    core = f(r, "f_echo_length_100km") * 100 >= 0.1
    if float(r["final_score"]) >= 0.3:
        return "evidence >= 0.3 but no storm core" if not core else "shown, but folded or masked"
    if f(r, "f_debris") > 0:
        return "evidence < 0.3, weak debris beside it (no rotation-only lift)"
    if f(r, "f_rooted") == 0:
        return "evidence < 0.3, not rooted in the lowest tilt"
    if f(r, "f_cyclonic") == 0:
        return "evidence < 0.3, anticyclonic"
    if float(r["range_km"]) < 40:
        return "evidence < 0.3, within 40 km (no rotation-only lift)"
    if f(r, "f_low_level_shear") < 1.8:
        return "evidence < 0.3, low-level shear under 0.018 s-1"
    if not core:
        return "lift-eligible but no storm core"
    return "lift-eligible: turbine mask or folded"


def main():
    dirs = sys.argv[1:]
    if not dirs:
        sys.exit(__doc__)
    rows, hours, truths = load(dirs)
    marks = rows["tornado_marker"]
    print(f"{', '.join(dirs)}: {hours:.1f} radar-hours, {len(truths)} tornado truths")
    rates("markers", marks, hours, truths, lambda r: r["tier"] != "Tornado possible")
    rates("original", rows["tornado_id"], hours, truths, lambda r: float(r["final_score"]) >= 0.6)

    found = {t for r in marks for t in matched(r)}
    columns = collections.defaultdict(list)
    for r in rows["tornado_fusion"]:
        for t in matched(r):
            columns[t].append(r)
    missed = sorted(truths - found)
    reasons = collections.Counter()
    for t in missed:
        near = columns.get(t)
        if not near:
            reasons["no rotation column near it (15-150 km)"] += 1
        else:
            reasons[why(max(near, key=lambda r: float(r["final_score"])))] += 1
    print(f"\n  {len(missed)} truths missed, by the best fused column near each:")
    for reason, n in reasons.most_common():
        print(f"    {n:4d}  {reason}")


if __name__ == "__main__":
    main()
