"""Fit the tornado fusion weights (detectionplan.md Phases 7-8) from a backtest export.

    hookecho --headless-backtest-file docs/backtest-events.txt --export DIR
    python scripts/fusion/fit.py DIR/candidates.csv

Reads the `tornado_fusion` rows (one per tracked LLSD rotation column per volume) and their
`f_*` features, labelled by whether the column verified against a tornado report or surveyed
track. Fits an L2-regularised logistic regression (Newton's method, standardised features, no
dependencies) and reports how it does when every event is held out in turn: the plan forbids
letting a tornado's consecutive volumes sit in both training and test, so folds are whole events.
It compares the held-out ranking with single-feature rankings on the same rows, then fits on every
event and prints the weights in the features' own units, as Rust, for `tornado_fusion::WEIGHTS`.

The score this produces is an evidence score. A corpus of a few events cannot calibrate a
probability, and nothing here claims to.
"""

import csv
import math
import sys

L2 = 1.0  # ridge strength on the standardised weights (not the bias)

# The sign physics expects of each weight: +1 evidence for a tornado, -1 against, 0 left out.
# A free fit to a few events learns their quirks instead: on the first corpus it gave range the
# largest weight, negative (where the reports happened to be; LLSD underestimates shear far out,
# so physically the same measured shear means *more* rotation there), made hail beside debris a
# sign *for* a tornado, and split correlated shear features into opposite signs. A feature that
# still comes out with the wrong sign is dropped and the rest refitted.
SIGNS = {
    "low_level_shear": +1,
    "max_shear": +1,
    "depth_km": +1,
    "tilts": +1,
    "rooted": +1,
    "cyclonic": +1,
    "persisted_2": +1,
    "persisted_3": +1,
    "shear_trend": +1,
    "debris": +1,
    "debris_hail": -1,
    "range_100km": 0,
    "weak_echo_root": -1,
    "stationary": -1,
    # Storm mode (round three): measured, left out until the line-or-cell question is settled.
    "echo_length_100km": 0,
    "echo_aspect": 0,
    # Near-ground flow beside the column (round three): measured, left out until tested.
    "near_wind_10ms": 0,
    "near_inbound_10ms": 0,
}


def load(path):
    rows = [r for r in csv.DictReader(open(path, encoding="utf-8")) if r["detector"] == "tornado_fusion"]
    global ALL_ROWS
    ALL_ROWS = rows
    names = [k[2:] for k in rows[0].keys() if k.startswith("f_")] if rows else []
    xs = [[float(r["f_" + n]) for n in names] for r in rows]
    ys = [1.0 if r["matched_report"] == "true" or r["matched_survey"] == "true" else 0.0 for r in rows]
    groups = [r["event"] for r in rows]
    return names, xs, ys, groups


def standardise(xs):
    n, k = len(xs), len(xs[0])
    mean = [sum(x[j] for x in xs) / n for j in range(k)]
    sd = [math.sqrt(sum((x[j] - mean[j]) ** 2 for x in xs) / n) or 1.0 for j in range(k)]
    return mean, sd


def solve(a, b):
    """Gaussian elimination with partial pivoting; a is n×n, b length n."""
    n = len(b)
    m = [row[:] + [b[i]] for i, row in enumerate(a)]
    for c in range(n):
        p = max(range(c, n), key=lambda r: abs(m[r][c]))
        m[c], m[p] = m[p], m[c]
        piv = m[c][c]
        if abs(piv) < 1e-12:
            continue
        for r in range(n):
            if r != c:
                f = m[r][c] / piv
                for j in range(c, n + 1):
                    m[r][j] -= f * m[c][j]
    return [m[i][n] / m[i][i] if abs(m[i][i]) > 1e-12 else 0.0 for i in range(n)]


def sigmoid(z):
    return 1.0 / (1.0 + math.exp(-max(-40.0, min(40.0, z))))


def fit(xs, ys, names=None):
    """Logistic regression on standardised features, each weight held to its sign in SIGNS
    (when `names` is given); returns (mean, sd, bias, weights)."""
    k = len(xs[0])
    use = [True] * k if names is None else [SIGNS.get(n, 0) != 0 for n in names]
    while True:
        model = fit_free(xs, ys, use)
        if names is None:
            return model
        wrong = [j for j, wj in enumerate(model[3]) if use[j] and wj * SIGNS[names[j]] < 0]
        if not wrong:
            return model
        # Drop the most wrong first, then refit: one at a time, so a correlated partner can
        # take up what it carried.
        worst = max(wrong, key=lambda j: abs(model[3][j]))
        use[worst] = False


def fit_free(xs, ys, use):
    """Logistic regression on the standardised features marked in `use` (the rest weigh 0)."""
    mean, sd = standardise(xs)
    zs = [[1.0] + [(x[j] - mean[j]) / sd[j] if use[j] else 0.0 for j in range(len(x))] for x in xs]
    k = len(zs[0])
    beta = [0.0] * k
    for _ in range(50):
        grad = [0.0] * k
        hess = [[0.0] * k for _ in range(k)]
        for z, y in zip(zs, ys):
            p = sigmoid(sum(b * v for b, v in zip(beta, z)))
            w = p * (1 - p)
            for i in range(k):
                grad[i] += (y - p) * z[i]
                for j in range(i, k):
                    hess[i][j] += w * z[i] * z[j]
        for i in range(k):
            for j in range(i):
                hess[i][j] = hess[j][i]
        for i in range(1, k):
            grad[i] -= L2 * beta[i]
            hess[i][i] += L2
            if not use[i - 1]:
                grad[i], beta[i] = 0.0, 0.0
                hess[i] = [0.0] * k
                hess[i][i] = 1.0
                for r in range(k):
                    if r != i:
                        hess[r][i] = 0.0
        step = solve(hess, grad)
        beta = [b + s for b, s in zip(beta, step)]
        if max(abs(s) for s in step) < 1e-8:
            break
    return mean, sd, beta[0], beta[1:]


def predict(model, x):
    mean, sd, b, w = model
    return sigmoid(b + sum(wj * (x[j] - mean[j]) / sd[j] for j, wj in enumerate(w)))


def auc(scores, ys):
    """Probability a random verified row outranks a random unverified one (ties count half)."""
    pairs = sorted(zip(scores, ys))
    pos = sum(ys)
    neg = len(ys) - pos
    if pos == 0 or neg == 0:
        return float("nan")
    rank_sum, i = 0.0, 0
    while i < len(pairs):
        j = i
        while j < len(pairs) and pairs[j][0] == pairs[i][0]:
            j += 1
        avg = (i + j + 1) / 2.0
        rank_sum += avg * sum(1 for t in range(i, j) if pairs[t][1] == 1.0)
        i = j
    return (rank_sum - pos * (pos + 1) / 2.0) / (pos * neg)


def top(scores, ys, groups, k):
    order = sorted(range(len(scores)), key=lambda i: -scores[i])[:k]
    ver = sum(ys[i] for i in order)
    events = len({groups[i] for i in order if ys[i] == 1.0})
    return ver, events


def report_matrix(rows, scores, summary, thresholds):
    """Report-level verification of `rows` scored by `scores`: distinct tornado reports and
    surveyed paths found (from each row's matched_truths), against every one the events have."""
    events = sum(c["tornado_reports"] + c["tornado_surveys"] for c in summary["truth_counts"].values())
    hours = summary["detectors"]["tornado_fusion"]["radar_hours"]
    out = []
    for t in thresholds:
        kept = [r for r, s in zip(rows, scores) if s >= t]
        ver = sum(1 for r in kept if r.get("matched_truths"))
        found = len({(r["event"], i) for r in kept for i in (r.get("matched_truths") or "").split(";") if i})
        det = len(kept)
        far = (det - ver) / det if det else None
        csi = found / (events + det - ver) if events + det - ver else None
        out.append((t, det, ver, found / events if events else None, far, csi, (det - ver) / hours))
    return out


def print_matrix(title, m):
    print(f"\n{title}")
    print("  thr    det   ver   POD   FAR   CSI   FA/h")
    for t, det, ver, pod, far, csi, fah in m:
        f = lambda v: "  -  " if v is None else f"{v:5.2f}"
        print(f"  {t:.2f} {det:6d} {ver:5d} {f(pod)} {f(far)} {f(csi)} {fah:6.1f}")


def main():
    names, xs, ys, groups = load(sys.argv[1])
    events = sorted(set(groups))
    print(f"{len(xs)} fused rows, {int(sum(ys))} verified, {len(events)} events, features: {', '.join(names)}")
    held = [0.0] * len(xs)
    for e in events:
        train = [i for i, g in enumerate(groups) if g != e]
        model = fit([xs[i] for i in train], [ys[i] for i in train], names)
        for i, g in enumerate(groups):
            if g == e:
                held[i] = predict(model, xs[i])
    rankings = {"fused (held out by event)": held}
    for n in ("low_level_shear", "max_shear", "debris"):
        j = names.index(n)
        rankings[n + " alone"] = [x[j] for x in xs]
    total_events = len({g for g, y in zip(groups, ys) if y == 1.0})
    print(f"\n{'ranking':28} {'AUC':>5}   verified/events among the top 50, 100, 200 rows")
    for name, s in rankings.items():
        cells = []
        for k in (50, 100, 200):
            v, ev = top(s, ys, groups, k)
            cells.append(f"{int(v):3d}/{ev}")
        print(f"{name:28} {auc(s, ys):5.3f}   " + "   ".join(cells) + f"   (of {total_events} events with any)")
    print("\nper held-out event AUC:")
    for e in events:
        idx = [i for i, g in enumerate(groups) if g == e]
        print(f"  {e:26} rows {len(idx):4d} verified {int(sum(ys[i] for i in idx)):3d}  AUC {auc([held[i] for i in idx], [ys[i] for i in idx]):.3f}")
    free = [0.0] * len(xs)
    for e in events:
        train = [i for i, g in enumerate(groups) if g != e]
        model = fit([xs[i] for i in train], [ys[i] for i in train])
        for i, g in enumerate(groups):
            if g == e:
                free[i] = predict(model, xs[i])
    print(f"\nfor comparison, an unconstrained fit held out by event: AUC {auc(free, ys):.3f}")
    # Report-level, out of sample: the held-out fused scores against Tornado ID's own (which is
    # not fitted to anything here), when the export carries matched truth ids.
    import json
    import os
    summary_path = os.path.join(os.path.dirname(sys.argv[1]), "summary.json")
    if ALL_ROWS and "matched_truths" in ALL_ROWS[0] and os.path.exists(summary_path):
        summary = json.load(open(summary_path, encoding="utf-8"))
        thresholds = [0.3, 0.4, 0.5, 0.6, 0.7, 0.8]
        print_matrix("fused, held out by event (report level)", report_matrix(ALL_ROWS, held, summary, thresholds))
        tid = [r for r in csv.DictReader(open(sys.argv[1], encoding="utf-8")) if r["detector"] == "tornado_id"]
        print_matrix("Tornado ID (the app's), same events", report_matrix(tid, [float(r["final_score"]) for r in tid], summary, thresholds))
    mean, sd, b, w = fit(xs, ys, names)
    w_orig = [wj / sd[j] for j, wj in enumerate(w)]
    b_orig = b - sum(wj * mean[j] / sd[j] for j, wj in enumerate(w))
    print("\n// Fitted on every event; paste into tornado_fusion::WEIGHTS.")
    print("pub const WEIGHTS: Weights = Weights {")
    print(f"    bias: {b_orig:.4f},")
    print("    w: [")
    for n, v in zip(names, w_orig):
        print(f"        {v:.4f}, // {n}")
    print("    ],")
    print("};")


if __name__ == "__main__":
    main()
