//! Backtest: dealiasing at the decoded Nyquist velocity against the largest-|v| estimate it used
//! before, on every velocity tilt of the cached real volumes (ROADMAP_PARITY M3.1, gap 5). Both
//! arms unfold the same raw radials on the same path; only the interval differs.
#[path = "support/corpus.rs"]
pub mod corpus;

use wxdata::dealias::NyquistSource;
use wxdata::level2::{self, BinnedSweep, Moment};

fn values(s: &BinnedSweep) -> Vec<Option<f32>> {
    let span = s.value_max - s.value_min;
    s.data
        .iter()
        .map(|&c| (c >= 2).then(|| s.value_min + (c - 2) as f32 / 253.0 * span))
        .collect()
}

/// Neighbouring gates (along the radial and across azimuth) that differ by more than `nyq`:
/// a correctly unfolded field has few; every wrongly folded region adds its whole boundary.
fn jumps(s: &BinnedSweep, nyq: f32) -> usize {
    let v = values(s);
    let (az, g) = (s.az_bins, s.gate_count);
    let mut n = 0;
    for a in 0..az {
        for i in 0..g {
            let Some(x) = v[a * g + i] else { continue };
            let right = (i + 1 < g).then(|| v[a * g + i + 1]).flatten();
            let next = v[((a + 1) % az) * g + i];
            n += [right, next]
                .into_iter()
                .flatten()
                .filter(|y| (x - y).abs() > nyq)
                .count();
        }
    }
    n
}

fn decoded(s: &BinnedSweep) -> Option<f32> {
    let mut k: Vec<f32> = s
        .row_nyquist_mps
        .iter()
        .copied()
        .filter(|x| x.is_finite() && *x > 0.0)
        .collect();
    k.sort_by(f32::total_cmp);
    k.get(k.len() / 2).copied()
}

/// Both arms' unfolded fields as little-endian f32 (NaN = no data), row-major azimuth × gate, with
/// a JSON header, for scoring against an independent dealiaser (scripts/corpus/nyquist_pyart.py).
fn export(
    dir: &std::path::Path,
    id: &str,
    tilt: usize,
    raw: &BinnedSweep,
    old: &BinnedSweep,
    new: &BinnedSweep,
) {
    std::fs::create_dir_all(dir).unwrap();
    for (arm, s) in [("raw", raw), ("est", old), ("dec", new)] {
        let bytes: Vec<u8> = values(s)
            .iter()
            .flat_map(|v| v.unwrap_or(f32::NAN).to_le_bytes())
            .collect();
        std::fs::write(dir.join(format!("{id}_{tilt:02}_{arm}.f32")), bytes).unwrap();
    }
    let times: Vec<i64> = new.bin_time_ms.clone();
    let header = format!(
        "{{\"id\":\"{id}\",\"tilt\":{tilt},\"az_bins\":{},\"gate_count\":{},\"first_gate_km\":{},\"gate_interval_km\":{},\"elevation_deg\":{},\"nyquist_est\":{},\"nyquist_dec\":{},\"t_min_ms\":{},\"t_max_ms\":{}}}",
        new.az_bins,
        new.gate_count,
        new.first_gate_km,
        new.gate_interval_km,
        new.elevation_deg,
        old.nyquist_ms,
        new.nyquist_ms,
        times.iter().filter(|&&t| t > 0).min().copied().unwrap_or(0),
        times.iter().max().copied().unwrap_or(0),
    );
    std::fs::write(dir.join(format!("{id}_{tilt:02}.json")), header).unwrap();
}

const VOLUMES: [&str; 13] = [
    "moore-2013",
    "mayfield-2021",
    "el-reno-2013",
    "washington-2013",
    "vilonia-2014",
    "nashville-2020",
    "joplin-2011",
    "derecho-2020",
    "denver-hail-2017",
    "clear-air-2019",
    "iowa-qlcs-2021",
    "tlx-clutter-2020",
    "moore-2013",
];

#[test]
#[ignore = "large cached fixtures: provision explicitly before running (release build)"]
fn cached_decoded_nyquist_unfolds_no_worse_than_the_estimate() {
    let m = corpus::manifest();
    let (mut tilts, mut decoded_used, mut differ_tilts) = (0, 0, 0);
    let (mut jumps_est_total, mut jumps_dec_total) = (0usize, 0usize);
    let (mut worse, mut gaps) = (Vec::new(), Vec::new());
    let mut seen = std::collections::HashSet::new();
    for id in VOLUMES {
        if !seen.insert(id) {
            continue;
        }
        let f = m.fixtures.iter().find(|f| f.id == id).unwrap();
        let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
        for tilt in 0..level2::elevation_angles(&scan).len() {
            let Ok(new) = level2::bin_scan_opts(&scan, Moment::Velocity, tilt, true) else {
                continue;
            };
            let old = level2::bin_scan_dealiased_by_estimate(&scan, tilt).unwrap();
            tilts += 1;
            if let Some(dir) = std::env::var_os("HOOKECHO_NYQ_EXPORT") {
                let raw = level2::bin_scan_opts(&scan, Moment::Velocity, tilt, false).unwrap();
                export(std::path::Path::new(&dir), id, tilt, &raw, &old, &new);
            }
            let truth = decoded(&new);
            if new.nyquist_source == NyquistSource::Decoded {
                decoded_used += 1;
            }
            // A gate whose fold differs moves by a whole interval; a sub-step change of interval
            // only nudges unfolded gates by a fraction of a m/s, which is not a different answer.
            let (vn, vo) = (values(&new), values(&old));
            let differ = vn
                .iter()
                .zip(&vo)
                .filter(
                    |(a, b)| matches!((a, b), (Some(a), Some(b)) if (a - b).abs() > new.nyquist_ms),
                )
                .count();
            let filled = new.data.iter().filter(|&&c| c >= 2).count();
            // Jumps are counted against the decoded Nyquist where there is one: that is the
            // interval the radar actually folded at, whichever arm is being judged.
            let judge = truth.unwrap_or(new.nyquist_ms);
            let (je, jd) = (jumps(&old, judge), jumps(&new, judge));
            jumps_est_total += je;
            jumps_dec_total += jd;
            if differ > 0 {
                differ_tilts += 1;
            }
            // Within noise of the estimate: the two intervals differ by under 1%, so the
            // unfoldings differ only where a boundary vote sits on the rounding line.
            if jd > je + 50 + je / 10 {
                worse.push(format!("{id} tilt {tilt}: {jd} jumps vs {je}"));
            }
            // Every row has a radial (the 1-degree binning gap, ROADMAP_PARITY M3.1).
            if !new.bin_time_ms.is_empty() {
                let empty = new.bin_time_ms.iter().filter(|&&t| t <= 0).count();
                if empty > 0 {
                    gaps.push(format!("{id} tilt {tilt}: {empty} rows with no radial"));
                }
            }
            eprintln!(
                "{id} tilt {tilt:2} ({:4.1}°): decoded {truth:?} est {:.2} → {:.2} {:?}; {differ}/{filled} gates differ; jumps est {je} dec {jd}",
                new.elevation_deg, old.nyquist_ms, new.nyquist_ms, new.nyquist_source
            );
        }
    }
    eprintln!(
        "{tilts} velocity tilts; decoded used on {decoded_used}; {differ_tilts} tilts unfold differently; jumps est {jumps_est_total} dec {jumps_dec_total}"
    );
    assert!(tilts >= 150, "the backtest must cover the corpus: {tilts}");
    // All but the one sectorized volume (vilonia-2014, whose rows differ in PRF) unfold at the
    // decoded value.
    assert!(
        decoded_used >= 140,
        "decoded Nyquist used on {decoded_used} tilts"
    );
    assert!(gaps.is_empty(), "{gaps:#?}");
    assert!(worse.is_empty(), "{worse:#?}");
    assert!(
        jumps_dec_total * 100 <= jumps_est_total * 102,
        "decoded {jumps_dec_total} vs estimate {jumps_est_total}"
    );
    // The dealiaser itself: neighbouring gates more than V_ny apart after unfolding, summed over
    // the corpus. 512,079 with the 1-degree binning gaps, 240,920 with the neighbour-difference
    // regions once the gaps were closed, 25,114 with Py-ART's banded regions and merge.
    assert!(
        jumps_dec_total <= 30_000,
        "dealiasing left {jumps_dec_total} fold-sized jumps"
    );
}
