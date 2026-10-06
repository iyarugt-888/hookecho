"""Test the near-storm environment against the verified detections (detectionplan.md, "Environment").

    cargo run --release -p wxdata --example env_sample -- ENV.csv DIR/candidates.csv ...
    python scripts/fusion/environment.py report ENV.csv DIR [DIR...]
    python scripts/fusion/environment.py apply ENV.csv GATE PROMOTE MIN_EVIDENCE IN_DIR OUT_DIR

`report` joins each export's rows to their sampled environment (`wxdata::near_storm`: inflow CAPE,
CIN, helicity, shear, LCL height and STP from the HRRR run an hour before) and prints, for the
app's markers (`tornado_marker` rows), how each variable separates verified markers from false
ones; then what two simple uses of it would do to false markers per radar-hour and POD:
  - a gate: hide a marker whose inflow STP is below GATE;
  - a promotion: raise a Possible marker to Likely when its inflow STP is at least PROMOTE and its
    fused evidence (`final_score`) at least MIN_EVIDENCE (0.4 in the sweep: the rotation-only
    lifts, under 0.3, are most of the false markers).
Rows with no environment (before the HRRR archive, or off its grid) are left as they are and
counted.

`apply` writes OUT_DIR as a copy of IN_DIR's export with one gate and one promotion applied to its
markers (0 and inf turn either off), so markers.py and stormevents.py score it unchanged, lead
time included.
"""

import collections
import csv
import json
import math
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fit import auc  # noqa: E402

FIELDS = ["sbcape", "mlcape", "mlcin", "srh1", "srh3", "shear6", "lcl_m", "stp", "stp_point"]
# Which way is favourable for a tornado: more of it (+1) or less (-1, the LCL height).
SIGN = {"lcl_m": -1}
LIKELY = "Tornado likely"


def load_env(path):
    env, missing_hours = {}, 0
    for line in open(path, encoding="utf-8"):
        f = line.rstrip("\n").split(",")
        if f[0] == "#missing":
            missing_hours += 1
            continue
        if f[0] == "minute" or len(f) < 15 or f[5] == "":
            continue
        env[(f[0], f[1], f[2])] = dict(zip(FIELDS, map(float, f[5:14])))
    return env, missing_hours


def env_of(env, r):
    return env.get((r["minute"], r["lon"], r["lat"]))


def load(dirs):
    rows, hours, truths = collections.defaultdict(list), 0.0, set()
    for d in dirs:
        summary = json.load(open(os.path.join(d, "summary.json"), encoding="utf-8"))
        hours += summary["detectors"]["tornado_fusion"]["radar_hours"]
        for event, ids in summary["tornado_truth_minutes"].items():
            truths |= {(event, i) for i in ids}
        for r in csv.DictReader(open(os.path.join(d, "candidates.csv"), encoding="utf-8")):
            if r["detector"] in ("tornado_marker", "tornado_fusion"):
                rows[r["detector"]].append(r)
    return rows, hours, truths


def matched(r):
    return [(r["event"], t) for t in r["matched_truths"].split(";") if t]


def rates(marks, hours, truths):
    """(false per radar-hour, POD) at every marker and at Likely and up."""
    out = []
    for keep in (lambda r: True, lambda r: r["tier"] != "Tornado possible"):
        kept = [r for r in marks if keep(r)]
        false = sum(1 for r in kept if not r["matched_truths"])
        found = {t for r in kept for t in matched(r)} & truths
        out.append((false / hours, len(found) / max(len(truths), 1)))
    return out


def transform(marks, env, gate, promote, min_evidence=0.0):
    """The markers a gate and a promotion leave, as new rows."""
    out = []
    for r in marks:
        e = env_of(env, r)
        if e is not None and e["stp"] < gate:
            continue
        if (e is not None and e["stp"] >= promote and r["tier"] == "Tornado possible"
                and float(r["final_score"]) >= min_evidence):
            r = dict(r, tier=LIKELY)
        out.append(r)
    return out


def report(env_path, dirs):
    env, missing_hours = load_env(env_path)
    rows, hours, truths = load(dirs)
    marks = rows["tornado_marker"]
    with_env = [r for r in marks if env_of(env, r) is not None]
    print(f"{', '.join(dirs)}: {hours:.1f} radar-hours, {len(truths)} truths, {len(marks)} markers, "
          f"{len(with_env)} with an environment ({missing_hours} HRRR hours missing in {env_path})")
    if not with_env:
        return
    y = [1.0 if r["matched_truths"] else 0.0 for r in with_env]
    print(f"\n  markers with an environment: {int(sum(y))} verified, {len(y) - int(sum(y))} false")
    print(f"  {'variable':10} {'AUC':>5}   median verified   median false")
    for k in FIELDS:
        x = [SIGN.get(k, 1) * env_of(env, r)[k] for r in with_env]
        ver = sorted(env_of(env, r)[k] for r, t in zip(with_env, y) if t)
        fal = sorted(env_of(env, r)[k] for r, t in zip(with_env, y) if not t)
        med = lambda v: v[len(v) // 2] if v else float("nan")
        print(f"  {k:10} {auc(x, y):5.3f}   {med(ver):15.2f}   {med(fal):12.2f}")

    fused = [r for r in rows["tornado_fusion"] if env_of(env, r) is not None]
    if fused:
        fy = [1.0 if r["matched_report"] == "true" or r["matched_survey"] == "true" else 0.0 for r in fused]
        print(f"\n  fused columns with an environment: {len(fused)} ({int(sum(fy))} verified); AUC alone:")
        print("   " + "  ".join(f"{k} {auc([SIGN.get(k, 1) * env_of(env, r)[k] for r in fused], fy):.3f}"
                             for k in ("stp", "srh1", "mlcape", "lcl_m", "shear6")))
        print("   for comparison, the fused evidence score "
              f"{auc([float(r['final_score']) for r in fused], fy):.3f}")

    (fa, pod), (lfa, lpod) = rates(marks, hours, truths)
    print(f"\n  as shipped: {fa:.2f} false/h, POD {pod:.2f}; Likely+ {lfa:.3f} false/h, POD {lpod:.2f}")
    print("  gate (hide markers with inflow STP below):")
    for g in (0.05, 0.1, 0.25, 0.5, 1.0):
        (fa, pod), _ = rates(transform(marks, env, g, math.inf), hours, truths)
        print(f"    STP < {g:<4}  {fa:.2f} false/h, POD {pod:.2f}")
    print("  promotion (Possible to Likely at inflow STP of at least, and fused evidence of at least):")
    for p in (1.0, 2.0):
        for ev in (0.0, 0.3, 0.4, 0.5):
            _, (lfa, lpod) = rates(transform(marks, env, 0.0, p, ev), hours, truths)
            print(f"    STP >= {p:<4} evidence >= {ev:<4} Likely+ {lfa:.3f} false/h, POD {lpod:.2f}")


def apply(env_path, gate, promote, min_evidence, src, dst):
    env, _ = load_env(env_path)
    os.makedirs(dst, exist_ok=True)
    shutil.copy(os.path.join(src, "summary.json"), os.path.join(dst, "summary.json"))
    with open(os.path.join(src, "candidates.csv"), encoding="utf-8") as fh:
        reader = csv.DictReader(fh)
        all_rows = list(reader)
        names = reader.fieldnames
    marks = transform([r for r in all_rows if r["detector"] == "tornado_marker"], env, gate, promote,
                      min_evidence)
    rest = [r for r in all_rows if r["detector"] != "tornado_marker"]
    with open(os.path.join(dst, "candidates.csv"), "w", encoding="utf-8", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=names)
        w.writeheader()
        w.writerows(rest + marks)
    print(f"{dst}: {len(marks)} markers kept with gate {gate}, promotion {promote} at evidence {min_evidence}")


if __name__ == "__main__":
    a = sys.argv[1:]
    if a[:1] == ["report"] and len(a) >= 3:
        report(a[1], a[2:])
    elif a[:1] == ["apply"] and len(a) == 7:
        apply(a[1], float(a[2]), float(a[3]), float(a[4]), a[5], a[6])
    else:
        sys.exit(__doc__)
