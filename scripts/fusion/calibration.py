"""Can a Tornado ID marker's evidence score be worded as a likelihood? (1008.md G3)

    python scripts/fusion/calibration.py --train DIR [DIR...] --test DIR [DIR...]

Reads the `tornado_marker` rows of backtest exports (what the map draws; see markers.py). A
marker is *verified* when it matched a tornado truth (report or surveyed path). Two units are
scored:

- **marker**: each drawn marker, with its own score;
- **episode**: each track's markers taken together (the event's track id), with the track's
  highest score, verified when any of its markers was. This is what a person sees as "one
  tornado marker" over several scans.

On the training sets it fits two maps from score to the observed frequency of verification:
isotonic (pool-adjacent-violators, monotone, no shape assumed) and Platt (a logistic in the
score). On the held-out sets it prints a reliability table for the raw score, both maps and the
base rate: per score bin, how many, the mean stated value, the observed frequency with a 95%
Wilson interval; and the Brier score and its reliability term. Nothing here changes the app:
whether a tier may be worded as a likelihood is decided from these tables.

Pure Python, no numpy, so it runs wherever the backtest does.
"""

import collections
import csv
import math
import os
import sys

BINS = [0.0, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0001]


def load(dirs):
    markers = []
    for d in dirs:
        name = os.path.basename(os.path.normpath(d))
        with open(os.path.join(d, "candidates.csv"), encoding="utf-8") as f:
            for r in csv.DictReader(f):
                if r["detector"] != "tornado_marker":
                    continue
                markers.append({
                    "set": name,
                    "event": r["event"],
                    "track": r.get("track_id") or "",
                    "score": float(r["final_score"]),
                    "tier": r["tier"],
                    "hit": bool(r["matched_truths"]),
                })
    return markers


def episodes(markers):
    tracks = collections.OrderedDict()
    for m in markers:
        key = (m["set"], m["event"], m["track"] or id(m))
        e = tracks.setdefault(key, {"score": 0.0, "hit": False, "tier": m["tier"]})
        e["score"] = max(e["score"], m["score"])
        e["hit"] |= m["hit"]
    return list(tracks.values())


def isotonic(rows):
    """Pool-adjacent-violators on (score, hit): a non-decreasing step map, as (upper score,
    value) blocks. Ties in score are pooled first."""
    pts = sorted((r["score"], 1.0 if r["hit"] else 0.0) for r in rows)
    blocks = []  # [sum, count, max score]
    for s, y in pts:
        if blocks and blocks[-1][2] == s:
            blocks[-1][0] += y
            blocks[-1][1] += 1
        else:
            blocks.append([y, 1, s])
        while len(blocks) > 1 and blocks[-2][0] / blocks[-2][1] > blocks[-1][0] / blocks[-1][1]:
            a = blocks.pop()
            blocks[-1][0] += a[0]
            blocks[-1][1] += a[1]
            blocks[-1][2] = a[2]
    return [(b[2], b[0] / b[1]) for b in blocks]


def apply_isotonic(blocks, s):
    if not blocks:
        return float("nan")
    for upper, v in blocks:
        if s <= upper:
            return v
    return blocks[-1][1]


def platt(rows, iters=100):
    """Logistic p = 1 / (1 + exp(-(a s + b))) by Newton's method, with Platt's target smoothing
    so a perfectly separable set does not run off to infinity."""
    n1 = sum(1 for r in rows if r["hit"])
    n0 = len(rows) - n1
    t1, t0 = (n1 + 1) / (n1 + 2), 1 / (n0 + 2)
    data = [(r["score"], t1 if r["hit"] else t0) for r in rows]
    a, b = 0.0, math.log((n1 + 1) / (n0 + 1))
    for _ in range(iters):
        ga = gb = haa = hab = hbb = 0.0
        for s, t in data:
            p = 1 / (1 + math.exp(-(a * s + b)))
            d = p - t
            w = max(p * (1 - p), 1e-9)
            ga += d * s
            gb += d
            haa += w * s * s
            hab += w * s
            hbb += w
        det = haa * hbb - hab * hab
        if abs(det) < 1e-12:
            break
        da = (hbb * ga - hab * gb) / det
        db = (haa * gb - hab * ga) / det
        a, b = a - da, b - db
        if abs(da) + abs(db) < 1e-9:
            break
    return a, b


def wilson(k, n, z=1.96):
    if n == 0:
        return (float("nan"), float("nan"))
    p = k / n
    c = (p + z * z / (2 * n)) / (1 + z * z / n)
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return (max(0.0, c - h), min(1.0, c + h))


def brier(pairs):
    """Brier score and its reliability term (Murphy), over the bins above."""
    if not pairs:
        return float("nan"), float("nan")
    bs = sum((p - y) ** 2 for p, y in pairs) / len(pairs)
    rel = 0.0
    for lo, hi in zip(BINS, BINS[1:]):
        inb = [(p, y) for p, y in pairs if lo <= p < hi]
        if inb:
            mp = sum(p for p, _ in inb) / len(inb)
            oy = sum(y for _, y in inb) / len(inb)
            rel += len(inb) * (mp - oy) ** 2
    return bs, rel / len(pairs)


def table(title, rows, stated):
    pairs = [(stated(r), 1.0 if r["hit"] else 0.0) for r in rows]
    print(f"    {title}")
    print("      stated      n   mean stated  observed  95% interval")
    for lo, hi in zip(BINS, BINS[1:]):
        inb = [(p, y) for p, y in pairs if lo <= p < hi]
        if not inb:
            continue
        k = int(sum(y for _, y in inb))
        mp = sum(p for p, _ in inb) / len(inb)
        a, b = wilson(k, len(inb))
        print(f"      {lo:.1f}-{min(hi, 1.0):.1f}  {len(inb):5d}   {mp:10.2f}  {k / len(inb):8.2f}  "
              f"[{a:.2f}, {b:.2f}]")
    bs, rel = brier(pairs)
    print(f"      Brier {bs:.3f}, reliability {rel:.3f}")


def report(unit, train, test):
    base = sum(r["hit"] for r in train) / max(len(train), 1)
    iso = isotonic(train)
    a, b = platt(train)
    print(f"\n  {unit}: train {len(train)} ({sum(r['hit'] for r in train)} verified, base rate "
          f"{base:.2f}); held out {len(test)} ({sum(r['hit'] for r in test)} verified, base rate "
          f"{sum(r['hit'] for r in test) / max(len(test), 1):.2f})")
    print("    isotonic map (score up to -> frequency): " + ", ".join(
        f"{u:.2f}->{v:.2f}" for u, v in iso))
    print(f"    Platt: p = 1/(1+exp(-({a:.2f} s + {b:.2f})))")
    table("raw score read as a probability", test, lambda r: r["score"])
    table("isotonic", test, lambda r: apply_isotonic(iso, r["score"]))
    table("Platt", test, lambda r: 1 / (1 + math.exp(-(a * r["score"] + b))))
    table("training base rate", test, lambda r: base)
    tiers = collections.defaultdict(lambda: [0, 0])
    for r in test:
        tiers[r["tier"]][0] += 1
        tiers[r["tier"]][1] += r["hit"]
    print("    held out by tier: " + "; ".join(
        f"{t} {k}/{n} ({k / n:.2f}, 95% {wilson(k, n)[0]:.2f}-{wilson(k, n)[1]:.2f})"
        for t, (n, k) in sorted(tiers.items())))


def main():
    args = sys.argv[1:]
    if "--train" not in args or "--test" not in args:
        sys.exit(__doc__)
    i, j = args.index("--train"), args.index("--test")
    train_dirs = args[i + 1:j] if i < j else args[i + 1:]
    test_dirs = args[j + 1:i] if j < i else args[j + 1:]
    train, test = load(train_dirs), load(test_dirs)
    print(f"train: {', '.join(train_dirs)}\nheld out: {', '.join(test_dirs)}")
    report("markers", train, test)
    report("episodes", episodes(train), episodes(test))


if __name__ == "__main__":
    main()
