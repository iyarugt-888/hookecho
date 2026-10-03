"""Lead time along a storm's own track (detectionplan.md, Phase 9/11 finding).

    hookecho --headless-backtest-file docs/backtest-leadtime.txt 16 --export DIR
    python scripts/fusion/lead_time.py DIR

The backtest's own lead times count a detection only inside the matching window (15 minutes) at
the report's place, so they cannot exceed the window. Here, for the fused Tornado ID, a tornado
report or surveyed path found at a threshold is traced back along the tracker: its lead is from
the first volume in which any track that matched it was already at or above the threshold. The
original Tornado ID has no tracks, so it gets only the window-limited lead, labelled as such.

Only truths a detector found are counted, so this says how early, not how often (POD says that).
"""

import csv
import json
import os
import statistics
import sys


def leads(rows, truth_minutes, threshold, tracked):
    out = []
    by_event = {}
    for r in rows:
        by_event.setdefault(r["event"], []).append(r)
    for event, rs in by_event.items():
        minutes = truth_minutes.get(event, {})
        hot = [r for r in rs if float(r["final_score"]) >= threshold]
        for tid, t_minute in minutes.items():
            matched = [r for r in hot if tid in (r["matched_truths"] or "").split(";")]
            if not matched:
                continue
            if tracked:
                tracks = {r["track_id"] for r in matched if r["track_id"]}
                first = min(int(r["minute"]) for r in hot if r["track_id"] in tracks)
            else:
                first = min(int(r["minute"]) for r in matched)
            out.append(t_minute - first)
    return out


def describe(xs):
    if not xs:
        return "none found"
    q = sorted(xs)
    ge10 = sum(1 for x in xs if x >= 10) / len(xs)
    return (f"n {len(xs):3d}  median {statistics.median(xs):5.1f} min  "
            f"q75 {q[3 * (len(q) - 1) // 4]:4d}  max {q[-1]:4d}  >= 10 min {ge10:.0%}")


def main():
    d = sys.argv[1]
    summary = json.load(open(os.path.join(d, "summary.json"), encoding="utf-8"))
    truth_minutes = summary["tornado_truth_minutes"]
    rows = list(csv.DictReader(open(os.path.join(d, "candidates.csv"), encoding="utf-8")))
    fused = [r for r in rows if r["detector"] == "tornado_fusion"]
    original = [r for r in rows if r["detector"] == "tornado_id"]
    total = sum(len(v) for v in truth_minutes.values())
    print(f"{len(truth_minutes)} events, {total} tornado reports and paths")
    for t in (0.3, 0.6):
        print(f"\nthreshold {t}")
        print(f"  fused, along its track     {describe(leads(fused, truth_minutes, t, True))}")
        print(f"  fused, window-limited      {describe(leads(fused, truth_minutes, t, False))}")
        print(f"  original, window-limited   {describe(leads(original, truth_minutes, t, False))}")


if __name__ == "__main__":
    main()
