"""Score HookEcho's dealiased velocity against Py-ART's region-based dealiaser (ROADMAP_PARITY M3.1).

Independent of HookEcho's reader and dealiaser: Py-ART reads the same cached Archive II volumes
itself and unfolds each sweep with `pyart.correct.dealias_region_based` at the Nyquist velocity it
decodes from the radial blocks. The Rust backtest
(`crates/wxdata/tests/nyquist_backtest.rs`, run with HOOKECHO_NYQ_EXPORT=<dir>) writes both of
its arms per tilt — unfolded at the largest-|v| estimate ("est") and at the decoded Nyquist
("dec") — and this script reports, per tilt, the share of gates on which each arm lands on the
same fold as Py-ART.

    python -I scripts/corpus/nyquist_pyart.py <export dir> <corpus cache dir> [fixture id ...]

Py-ART is the reference, not the truth: it is one more region-based dealiaser with its own
failure modes. Agreement with it is evidence; disagreement on a tilt is a place to look.
"""

import json
import pathlib
import sys

import numpy as np
import pyart

MANIFEST = pathlib.Path(__file__).resolve().parents[2] / "crates/wxdata/tests/data/corpus/manifest.json"


def load_arm(export, stem, arm, header):
    raw = np.fromfile(export / f"{stem}_{arm}.f32", dtype="<f4")
    return raw.reshape(header["az_bins"], header["gate_count"])


def sweep_times_ms(radar):
    start = pyart.util.datetime_from_radar(radar)
    epoch_ms = (np.datetime64(start.isoformat()) - np.datetime64("1970-01-01T00:00:00")) / np.timedelta64(1, "ms")
    return epoch_ms + radar.time["data"] * 1000.0


def vad_misfit(az_deg, field, vny, stride=4):
    """Share of gates a fold or more off their range ring's VAD sinusoid.

    A truth-free judge of an unfolding: radial velocity of a broadly uniform wind traces
    a + b·cos(az) + c·sin(az) around each ring, and a gate unfolded by the wrong number of intervals
    sits about 2·V_ny off it. Storms are not uniform, so no field scores zero; what matters is the
    comparison between unfoldings of the same sweep. Rings with less than 70% azimuthal coverage are
    skipped, and each fit is repeated once without the gates the first fit put a fold away."""
    th = np.deg2rad(az_deg)
    basis = np.stack([np.ones_like(th), np.cos(th), np.sin(th)], axis=1)
    off = total = 0
    for g in range(0, field.shape[1], stride):
        v = field[:, g]
        ok = np.isfinite(v)
        if ok.sum() < 0.7 * len(v):
            continue
        use = ok.copy()
        for _ in range(2):
            coef, *_ = np.linalg.lstsq(basis[use], v[use], rcond=None)
            res = v - basis @ coef
            use = ok & (np.abs(res) <= vny)
            if use.sum() < 10:
                break
        off += int((ok & (np.abs(res) > vny)).sum())
        total += int(ok.sum())
    return off / max(total, 1), total


def main():
    export = pathlib.Path(sys.argv[1])
    cache = pathlib.Path(sys.argv[2])
    only = set(sys.argv[3:])
    fixtures = {f["id"]: f for f in json.loads(MANIFEST.read_text())["fixtures"]}
    headers = sorted(export.glob("*.json"))
    by_id = {}
    for h in headers:
        meta = json.loads(h.read_text())
        by_id.setdefault(meta["id"], []).append((h.stem, meta))
    totals = {"est": [0, 0], "dec": [0, 0]}
    vad_totals = {"raw": [0.0, 0], "est": [0.0, 0], "dec": [0.0, 0], "pyart": [0.0, 0]}
    rows = []
    for fid, tilts in sorted(by_id.items()):
        if only and fid not in only:
            continue
        radar = pyart.io.read_nexrad_archive(str(cache / fixtures[fid]["path"]))
        try:
            ref = pyart.correct.dealias_region_based(radar, vel_field="velocity")
        except Exception as e:  # Py-ART wants one Nyquist per sweep, as HookEcho's unfolding does
            print(f"{fid}: Py-ART declines the volume: {e}")
            continue
        times = sweep_times_ms(radar)
        ranges_km = radar.range["data"] / 1000.0
        vel = radar.fields["velocity"]["data"]
        nyq = radar.instrument_parameters["nyquist_velocity"]["data"]
        for stem, meta in sorted(tilts, key=lambda t: t[1]["tilt"]):
            # The Py-ART sweep that is this tilt: same elevation, carrying velocity, and acquired
            # inside the rows' own clock span.
            best = None
            for s in range(radar.nsweeps):
                a, b = radar.sweep_start_ray_index["data"][s], radar.sweep_end_ray_index["data"][s] + 1
                if abs(radar.fixed_angle["data"][s] - meta["elevation_deg"]) > 0.3:
                    continue
                if np.ma.count(vel[a:b]) == 0:
                    continue
                mid = float(np.median(times[a:b]))
                if meta["t_min_ms"] - 30000 <= mid <= meta["t_max_ms"] + 30000:
                    best = (a, b)
            if best is None:
                print(f"{stem}: no matching Py-ART sweep")
                continue
            a, b = best
            az = radar.azimuth["data"][a:b]
            rows_idx = (np.floor((az % 360.0) / (360.0 / meta["az_bins"])).astype(int)) % meta["az_bins"]
            gates = np.rint((ranges_km - meta["first_gate_km"]) / meta["gate_interval_km"]).astype(int)
            ok_g = (gates >= 0) & (gates < meta["gate_count"])
            pv = np.ma.filled(ref["data"][a:b].astype("f4"), np.nan)[:, ok_g]
            rv = np.ma.filled(vel[a:b].astype("f4"), np.nan)[:, ok_g]
            vny = float(np.median(nyq[a:b]))
            line = [stem, f"{meta['elevation_deg']:.1f}", f"Py-ART V_ny {vny:.2f}"]
            for arm in ("est", "dec"):
                ours = load_arm(export, stem, arm, meta)[rows_idx][:, gates[ok_g]]
                both = np.isfinite(ours) & np.isfinite(pv)
                same = np.abs(ours - pv) <= vny
                n, k = int(both.sum()), int((same & both).sum())
                totals[arm][0] += k
                totals[arm][1] += n
                line.append(f"{arm} {k / max(n, 1):.4f} of {n}")
                if arm == "est":
                    # Alignment check: before any unfolding, the two readers hold the same field
                    # (to HookEcho's 1 m/s display quantization) at the same gates.
                    our_raw = load_arm(export, stem, "raw", meta)[rows_idx][:, gates[ok_g]]
                    both_raw = np.isfinite(our_raw) & np.isfinite(rv)
                    aligned = both_raw & (np.abs(our_raw - rv) <= 1.5)
                    line.append(f"aligned {aligned.sum() / max(both_raw.sum(), 1):.4f}")
            fields = {
                "raw": rv,
                "est": load_arm(export, stem, "est", meta)[rows_idx][:, gates[ok_g]],
                "dec": load_arm(export, stem, "dec", meta)[rows_idx][:, gates[ok_g]],
                "pyart": pv,
            }
            vad = []
            for name, fld in fields.items():
                share, n = vad_misfit(az, fld, vny)
                vad_totals[name][0] += share * n
                vad_totals[name][1] += n
                vad.append(f"{name} {share:.4f}")
            line.append("VAD-misfit " + " ".join(vad))
            rows.append(line)
            print("  ".join(line), flush=True)
    for name, (k, n) in vad_totals.items():
        print(f"VAD {name}: {k / max(n, 1):.5f} of {n} ring gates a fold or more off the sinusoid")
    for arm, (k, n) in totals.items():
        print(f"TOTAL {arm}: {k / max(n, 1):.5f} of {n} gates on Py-ART's fold")


if __name__ == "__main__":
    main()
