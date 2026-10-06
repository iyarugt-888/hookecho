//! The storm history (ROADMAP_PARITY M2.1) replayed on real days of SCIT, from the Unidata Level 3
//! archive (`unidata-nexrad-level3.s3.amazonaws.com`), fetched 2026-10-06:
//!
//! - every KTLX NST product for 2024-05-06 (47, 20:42–23:54 UTC, an Oklahoma severe day):
//!   `data/scit/tlx_nst_2024_05_06.zip`, 106,245 bytes, SHA-256 `a8263efc…cae78`
//! - every KDMX NST product for 2024-05-21 from 14:00 UTC (95, 14:05–23:57 UTC, the Iowa outbreak
//!   that produced the Greenfield tornado — a dense field of cells): `data/scit/
//!   dmx_nst_2024_05_21_pm.zip`, 338,290 bytes, SHA-256 `baede0d1…1be8`
//!
//! There is no hand-labelled truth for which cell is which storm, so the check is agreement with
//! SCIT's own continuity: when one SCIT ID appears in consecutive products within plausible reach,
//! SCIT is saying it tracked one storm, and the history should link those observations to one
//! storm too. A disagreement is either SCIT recycling an ID onto a new cell (which the history is
//! meant to refuse) or the history losing a storm SCIT kept. The report below sweeps the reach,
//! so the limits in use can be seen against the alternatives.

use std::collections::HashMap;
use wxdata::storm_history::{AssociationParams, ObservationRef, Source, StormHistory, StormId};

type Scan = (i64, Vec<wxdata::level3::Cell>);

fn scans(zip: &[u8]) -> Vec<Scan> {
    let mut out: Vec<Scan> = wxdata::zip::entries(zip)
        .unwrap()
        .iter()
        .map(|e| {
            let bytes = wxdata::zip::read(zip, e, 1 << 20).unwrap();
            let (t, cells) = wxdata::level3::decode_nst(&bytes).unwrap();
            (
                t.timestamp(),
                cells.into_iter().filter(|c| !c.id.is_empty()).collect(),
            )
        })
        .collect();
    out.sort_by_key(|s| s.0);
    out
}

fn motion(c: &wxdata::level3::Cell) -> Option<(f64, f64)> {
    let (deg, kt) = (c.mvt_deg? as f64, c.mvt_kt? as f64);
    let ms = kt * 0.514_444;
    Some((ms * deg.to_radians().sin(), ms * deg.to_radians().cos()))
}

/// What a replay found.
struct Replay {
    /// SCIT-continuity pairs, and those the history kept as one storm.
    pairs: usize,
    agree: usize,
    /// Links that contradict SCIT: a storm took a cell under a new ID while the cell with its
    /// old ID is still in the same product — two cells SCIT says are distinct, made one storm.
    contradictions: usize,
    storms: usize,
    tentative: usize,
}

/// Replay the day with `params`.
fn replay(scans: &[Scan], params: AssociationParams) -> Replay {
    let mut h = StormHistory::new(params);
    let mut prev: HashMap<String, (StormId, [f64; 2], i64)> = HashMap::new();
    let (mut pairs, mut agree, mut contradictions) = (0, 0, 0);
    let mut last_id: HashMap<StormId, (String, [f64; 2], i64)> = HashMap::new();
    for (t, cells) in scans {
        let obs: Vec<ObservationRef> = cells
            .iter()
            .map(|c| ObservationRef {
                source: Source::Scit,
                site: String::new(),
                provider_id: Some(c.id.clone()),
                time: *t,
                lon: c.lon,
                lat: c.lat,
                provider_motion_ms: motion(c),
            })
            .collect();
        let report = h.update(*t, &obs);
        let mut now = HashMap::new();
        for (c, s) in cells.iter().zip(&report.storms) {
            // The storm took a cell under a new ID while the cell with its old ID is in this
            // product within what 40 m/s covers of where the storm was: SCIT says those are
            // two cells, plausibly this storm's, and the history chose the other. (An old ID
            // reappearing farther than that is SCIT recycling it, which the history refuses.)
            if let Some((old, at, t0)) = last_id.get(s) {
                let reach = 40.0 / 1000.0 * (t - t0) as f64 + 5.0;
                let plausible = cells
                    .iter()
                    .any(|o| o.id == *old && haversine_km(*at, [o.lon, o.lat]) <= reach);
                if *old != c.id && plausible {
                    contradictions += 1;
                }
            }
            if let Some((was, at, t0)) = prev.get(&c.id) {
                // The same ID one product later, within what 40 m/s covers: SCIT's continuity.
                let km = haversine_km(*at, [c.lon, c.lat]);
                if km <= 40.0 / 1000.0 * (t - t0) as f64 + 5.0 {
                    pairs += 1;
                    if was == s {
                        agree += 1;
                    }
                }
            }
            now.insert(c.id.clone(), (*s, [c.lon, c.lat], *t));
        }
        for (c, s) in cells.iter().zip(&report.storms) {
            last_id.insert(*s, (c.id.clone(), [c.lon, c.lat], *t));
        }
        prev = now;
    }
    let tentative = h
        .storms()
        .iter()
        .flat_map(|s| &s.associations)
        .filter(|a| a.confidence == wxdata::storm_history::Confidence::Tentative)
        .count();
    Replay {
        pairs,
        agree,
        contradictions,
        storms: h.storms().len(),
        tentative,
    }
}

fn haversine_km(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (p0, p1) = (a[1].to_radians(), b[1].to_radians());
    let (dp, dl) = (p1 - p0, (b[0] - a[0]).to_radians());
    let h = (dp / 2.0).sin().powi(2) + p0.cos() * p1.cos() * (dl / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().asin()
}

#[test]
fn the_history_follows_scits_own_continuity_on_real_days() {
    for (name, zip, products) in [
        (
            "KTLX 2024-05-06",
            &include_bytes!("data/scit/tlx_nst_2024_05_06.zip")[..],
            47,
        ),
        (
            "KDMX 2024-05-21 pm",
            &include_bytes!("data/scit/dmx_nst_2024_05_21_pm.zip")[..],
            95,
        ),
    ] {
        let scans = scans(zip);
        assert_eq!(scans.len(), products, "{name}");
        let cells: usize = scans.iter().map(|s| s.1.len()).sum();
        let most = scans.iter().map(|s| s.1.len()).max().unwrap_or(0);
        eprintln!(
            "{name}: {} products, {cells} cell observations, up to {most} at once",
            scans.len()
        );
        for base_km in [2.0, 3.0, 5.0, 8.0, 12.0] {
            let params = AssociationParams {
                base_km,
                ..Default::default()
            };
            let r = replay(&scans, params);
            eprintln!(
                "  base {base_km:>4} km: {}/{} SCIT-continuity pairs kept together ({:.1}%), {} \
                 contradicting SCIT, {} storms, {} tentative links",
                r.agree,
                r.pairs,
                100.0 * r.agree as f64 / r.pairs.max(1) as f64,
                r.contradictions,
                r.storms,
                r.tentative
            );
        }
        let r = replay(&scans, AssociationParams::default());
        assert!(
            r.pairs > 100,
            "{name}: a real day has many continued cells: {}",
            r.pairs
        );
        assert!(
            r.agree as f64 >= 0.95 * r.pairs as f64,
            "{name}: the limits in use keep SCIT's continued cells together: {}/{}",
            r.agree,
            r.pairs
        );
        assert_eq!(
            r.contradictions, 0,
            "{name}: and never make two of SCIT's cells one storm"
        );
    }
}
