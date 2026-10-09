"""What separates weak tornadic rotation from ordinary rotation? (1008.md G2)

    python scripts/fusion/weak_rotation.py --train DIR [DIR...] --test DIR [DIR...]

Most tornadoes the markers miss had a credible cyclonic column with low-level shear under the
rotation-only bar (0.010-0.018 s-1), where weak tornadoes and ordinary storm rotation overlap
(detectionplan.md, "What the radar showed at the missed tornadoes"). This takes every fused
column (`tornado_fusion` rows) that is cyclonic, 15-150 km out and in that band, labels it by
whether it matched a tornado truth, and reports for each candidate feature the AUC (how often a
verified column outranks an unverified one; 0.5 is no information) on the training exports and
on the held-out ones, with the direction fixed on training. A feature is worth a rule only if it
holds out of sample.

Features: near-storm STP (`env_stp`, the HRRR inflow), storm shape (`f_echo_aspect`, line-like
echoes are long and thin; `f_echo_length_100km`), persistence (`f_persisted_2`, `f_persisted_3`,
`track_age_volumes`), depth and tilts, rooting, peak shear and range.
"""

import csv
import os
import sys

FEATURES = [
    ("env_stp", "near-storm STP"),
    ("f_echo_aspect", "echo aspect (line-like)"),
    ("f_echo_length_100km", "echo length"),
    ("f_persisted_2", "seen 2+ passes"),
    ("f_persisted_3", "seen 3+ passes"),
    ("track_age_volumes", "track age (volumes)"),
    ("f_depth_km", "column depth"),
    ("f_tilts", "tilts"),
    ("f_rooted", "rooted"),
    ("f_max_shear", "peak shear"),
    ("f_low_level_shear", "low-level shear"),
    ("range_km", "range"),
    ("final_score", "fused score"),
]


def num(v):
    try:
        x = float(v)
    except (TypeError, ValueError):
        return None
    return x if x == x else None


def load(dirs):
    rows = []
    for d in dirs:
        with open(os.path.join(d, "candidates.csv"), encoding="utf-8") as f:
            for r in csv.DictReader(f):
                if r["detector"] != "tornado_fusion":
                    continue
                shear = num(r["f_low_level_shear"])
                rng = num(r["range_km"])
                if shear is None or rng is None or not (1.0 <= shear < 1.8):
                    continue
                if num(r["f_cyclonic"]) != 1.0 or not (15.0 <= rng <= 150.0):
                    continue
                rows.append(r)
    return rows


def auc(pos, neg):
    """Mann-Whitney AUC with ties counted half; None without both classes."""
    if not pos or not neg:
        return None
    allv = sorted([(v, 1) for v in pos] + [(v, 0) for v in neg])
    rank, i, rsum = 1, 0, 0.0
    while i < len(allv):
        j = i
        while j < len(allv) and allv[j][0] == allv[i][0]:
            j += 1
        avg = (rank + rank + (j - i) - 1) / 2
        rsum += avg * sum(1 for k in range(i, j) if allv[k][1] == 1)
        rank += j - i
        i = j
    n1, n0 = len(pos), len(neg)
    return (rsum - n1 * (n1 + 1) / 2) / (n1 * n0)


def split(rows, key):
    pos, neg = [], []
    for r in rows:
        v = num(r.get(key))
        if v is None:
            continue
        (pos if r["matched_truths"] else neg).append(v)
    return pos, neg


def main():
    a = sys.argv[1:]
    if "--train" not in a or "--test" not in a:
        sys.exit(__doc__)
    i, j = a.index("--train"), a.index("--test")
    train = load(a[i + 1:j] if i < j else a[i + 1:])
    test = load(a[j + 1:i] if j < i else a[j + 1:])
    for name, rows in (("train", train), ("held out", test)):
        n1 = sum(1 for r in rows if r["matched_truths"])
        print(f"{name}: {len(rows)} weak cyclonic columns, {n1} verified ({n1 / max(len(rows), 1):.3f})")
    print(f"\n{'feature':28} {'train AUC':>10} {'held-out AUC':>13}  (direction from train)")
    for key, label in FEATURES:
        tr = auc(*split(train, key))
        if tr is None:
            continue
        flip = tr < 0.5
        te = auc(*split(test, key))
        show = lambda x: "  n/a" if x is None else f"{(1 - x) if flip else x:.3f}"  # noqa: E731
        print(f"{label:28} {show(tr):>10} {show(te):>13}  {'lower' if flip else 'higher'} is tornadic")


if __name__ == "__main__":
    main()
