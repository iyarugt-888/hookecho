"""What would a weak-rotation rule add? (1008.md G2)

    python scripts/fusion/weak_rules.py DIR [DIR...]

For weak cyclonic columns (`tornado_fusion` rows, low-level shear 0.010-0.018 s-1, 15-150 km)
that pass "fused score >= s and near-storm STP >= t", counts over the given exports:

- the tornado truths they match that the app's markers (`tornado_marker` rows) did not find;
- the false tracks they would add: (event, track) whose passing rows matched no truth and that
  the markers did not already draw, per radar-hour.

An upper bound on what such a rule could gain: it ignores the markers' other rules (cores, the
turbine mask, a lift waiting a pass), each of which would remove some of both.
"""

import collections
import csv
import json
import os
import sys

SCORES = [0.10, 0.15, 0.20, 0.25]
STPS = [0.5, 1.0, 2.0, 3.0]


def num(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def main():
    dirs = sys.argv[1:]
    if not dirs:
        sys.exit(__doc__)
    hours, truths, found = 0.0, set(), set()
    marker_tracks = set()
    weak = []
    for d in dirs:
        s = json.load(open(os.path.join(d, "summary.json"), encoding="utf-8"))
        hours += s["detectors"]["tornado_fusion"]["radar_hours"]
        tag = os.path.basename(os.path.normpath(d))
        truths |= {(tag, e, i) for e, ids in s["tornado_truth_minutes"].items() for i in ids}
        for r in csv.DictReader(open(os.path.join(d, "candidates.csv"), encoding="utf-8")):
            m = [(tag, r["event"], t) for t in r["matched_truths"].split(";") if t]
            if r["detector"] == "tornado_marker":
                found |= set(m)
                marker_tracks.add((tag, r["event"], r["track_id"]))
            elif r["detector"] == "tornado_fusion":
                sh, rng = num(r["f_low_level_shear"]), num(r["range_km"])
                if sh is None or rng is None or not (1.0 <= sh < 1.8):
                    continue
                if num(r["f_cyclonic"]) != 1.0 or not (15.0 <= rng <= 150.0):
                    continue
                weak.append((tag, r["event"], r["track_id"], num(r["final_score"]) or 0.0,
                             num(r["env_stp"]), m))
    found &= truths
    print(f"{', '.join(dirs)}\n{hours:.1f} radar-hours, {len(truths)} truths, markers found "
          f"{len(found)} (POD {len(found) / max(len(truths), 1):.3f})\n")
    print(f"{'score>=':>8} {'STP>=':>6} {'new truths':>11} {'POD gain':>9} {'false tracks/h':>15}")
    for sc in SCORES:
        for st in STPS:
            new, tracks = set(), collections.defaultdict(bool)
            for tag, ev, tr, score, stp, m in weak:
                if score < sc or stp is None or stp < st:
                    continue
                key = (tag, ev, tr)
                if key in marker_tracks:
                    continue
                tracks[key] |= bool(m)
                new |= {t for t in m if t in truths and t not in found}
            false = sum(1 for v in tracks.values() if not v)
            print(f"{sc:>8.2f} {st:>6.1f} {len(new):>11d} {len(new) / max(len(truths), 1):>9.3f} "
                  f"{false / hours:>15.2f}")


if __name__ == "__main__":
    main()
