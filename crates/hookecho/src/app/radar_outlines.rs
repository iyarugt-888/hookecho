//! Radar threshold outlines as vectors, for the map's GeoJSON export (ROADMAP_NEW I6,
//! ROADMAP_PARITY M4.4): the displayed sweep's edges at its moment's thresholds — by default
//! 35/50/60 dBZ, 3 dB ZDR, 2 °/km KDP and 0.80 CC, or the thresholds chosen in Settings
//! (1008.md D3).
//!
//! The sweep is sampled gate by gate (`BinnedSweep::sample_at`, no interpolation: the value the
//! radar recorded under each point) onto a 0.01° latitude/longitude lattice over its coverage, and
//! that lattice is contoured. Inside coverage a gate below the radar's threshold is "no echo" and
//! reads as far below any outline, so an echo's edge closes where the echo ends; outside coverage
//! and at range-folded gates there is no value, so an outline stops there rather than inventing an
//! edge.

use chrono::{DateTime, Utc};
use wxdata::level2::{BinnedSweep, Moment};
use wxdata::mrms::MrmsField;

/// The reflectivity outlines exported, dBZ: convective echo, strong cores, and where hail is
/// commonly looked for.
pub(crate) const REFLECTIVITY_OUTLINES_DBZ: [f32; 3] = [35.0, 50.0, 60.0];

/// The lattice the sweep is sampled onto, degrees (about 1 km).
const RES_DEG: f64 = 0.01;

/// What no echo inside coverage reads as on the lattice, for reflectivity.
const NO_ECHO: f32 = -999.0;

/// A moment's default outline thresholds, in its own units, and which side of each the outline
/// encloses. Reflectivity, ZDR and KDP outline what is at or above (cores, columns, heavy rain);
/// CC outlines what is at or below (debris and non-meteorological echo). Velocity and spectrum
/// width have none by default: a velocity contour is not an object's edge, though a person may
/// still choose one.
pub(crate) fn default_thresholds(moment: Moment) -> &'static [f32] {
    match moment {
        Moment::Reflectivity => &REFLECTIVITY_OUTLINES_DBZ,
        Moment::DifferentialReflectivity => &[3.0],
        Moment::SpecificDifferentialPhase => &[2.0],
        Moment::CorrelationCoefficient => &[0.80],
        _ => &[],
    }
}

/// Which side of a threshold an outline of `moment` encloses.
fn encloses(moment: Moment) -> &'static str {
    match moment {
        Moment::CorrelationCoefficient => "below",
        _ => "above",
    }
}

/// Parse a thresholds field ("35, 50, 60"): every number, in order; `None` if any part is not one.
pub(crate) fn parse_thresholds(text: &str) -> Option<Vec<f32>> {
    text.split([',', ' ', ';'])
        .filter(|t| !t.is_empty())
        .map(|t| t.parse::<f32>().ok().filter(|v| v.is_finite()))
        .collect()
}

/// The sweep on a latitude/longitude lattice over its own coverage: each cell the gate under its
/// centre, [`NO_ECHO`] where the radar saw nothing, NaN outside the sweep or range-folded.
pub(crate) fn sweep_lattice(sweep: &BinnedSweep, time: DateTime<Utc>) -> Option<MrmsField> {
    if sweep.az_bins == 0 || sweep.gate_count == 0 {
        return None;
    }
    let reach_km = f64::from(sweep.first_gate_km)
        + f64::from(sweep.gate_interval_km) * sweep.gate_count as f64;
    let (lat0, lon0) = (f64::from(sweep.radar_lat), f64::from(sweep.radar_lon));
    let dlat = reach_km / 111.2;
    let dlon = reach_km / (111.2 * lat0.to_radians().cos().max(0.05));
    let (west, east) = (lon0 - dlon, lon0 + dlon);
    let (south, north) = ((lat0 - dlat).max(-89.9), (lat0 + dlat).min(89.9));
    let nx = ((east - west) / RES_DEG).ceil().max(2.0) as usize;
    let ny = ((north - south) / RES_DEG).ceil().max(2.0) as usize;
    let (cw, ch) = ((east - west) / nx as f64, (north - south) / ny as f64);
    let no_echo = if sweep.moment == Moment::Reflectivity {
        NO_ECHO
    } else {
        f32::NAN
    };
    let mut values = Vec::with_capacity(nx * ny);
    for r in 0..ny {
        let lat = north - (r as f64 + 0.5) * ch;
        for c in 0..nx {
            let lon = west + (c as f64 + 0.5) * cw;
            values.push(match sweep.sample_at(lon, lat) {
                Some(g) if g.folded => f32::NAN,
                // Only reflectivity reads no echo as below every threshold; ZDR, KDP and CC have
                // no value without echo, so their outlines stop at the echo's edge.
                Some(g) => g.value.unwrap_or(no_echo),
                None => f32::NAN,
            });
        }
    }
    Some(MrmsField {
        values,
        nx,
        ny,
        lon_west: west,
        lon_east: east,
        lat_north: north,
        lat_south: south,
        time,
    })
}

/// What the outlines are of, for their properties.
pub(crate) struct OutlineSource<'a> {
    pub site: &'a str,
    pub time: DateTime<Utc>,
}

/// The sweep's outlines at each of `thresholds`, as WGS84 lines carrying the site, product,
/// elevation, scan time, threshold, unit and which side the outline encloses.
pub(crate) fn threshold_outlines(
    sweep: &BinnedSweep,
    src: &OutlineSource,
    thresholds: &[f32],
) -> Vec<wxdata::gis::GisFeature> {
    use serde_json::{Map, Value};
    if thresholds.is_empty() {
        return Vec::new();
    }
    let Some(grid) = sweep_lattice(sweep, src.time) else {
        return Vec::new();
    };
    let num = |v: f64| serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number);
    let mut out = Vec::new();
    for &t in thresholds {
        for l in wxdata::contour::contour_level(&grid, t) {
            let mut p = Map::new();
            p.insert("hookecho".into(), "radar_threshold".into());
            p.insert("site".into(), src.site.into());
            p.insert("product".into(), sweep.moment.short_name().into());
            p.insert(
                "elevation_deg".into(),
                num((f64::from(sweep.elevation_deg) * 100.0).round() / 100.0),
            );
            p.insert("threshold".into(), num(f64::from(t)));
            p.insert("unit".into(), sweep.moment.units().into());
            p.insert("encloses".into(), encloses(sweep.moment).into());
            p.insert("scan_time".into(), src.time.to_rfc3339().into());
            // A ring that comes back to its start encloses echo at or above the threshold; one
            // that does not ends at the edge of coverage or a folded gate.
            let closed = l.pts.first() == l.pts.last();
            p.insert("closed".into(), closed.into());
            out.push(wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::LineString(
                    l.pts.iter().map(|&(lon, lat)| [lon, lat]).collect(),
                ),
                properties: p,
            });
        }
    }
    out
}

impl super::HookEchoApp {
    /// The active pane's displayed sweep as threshold outlines at its moment's thresholds (the
    /// ones chosen in Settings, else the defaults), for the export.
    pub(crate) fn radar_outline_features(&mut self) -> Vec<wxdata::gis::GisFeature> {
        let moment = self.views[self.active].moment;
        let thresholds = self
            .settings
            .outline_thresholds
            .get(moment.short_name())
            .cloned()
            .unwrap_or_else(|| default_thresholds(moment).to_vec());
        let v = &mut self.views[self.active];
        let (tilt, site) = (v.tilt, v.site.clone().unwrap_or_default());
        let Some(vol) = v.volume.as_mut() else {
            return Vec::new();
        };
        let time = vol.time;
        let Ok(sweep) = vol.binned(moment, tilt, false) else {
            return Vec::new();
        };
        threshold_outlines(sweep, &OutlineSource { site: &site, time }, &thresholds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 0.5° sweep at Oklahoma City: 360 one-degree rows of 1 km gates out to 100 km, with a
    /// 55 dBZ disc 10 km across centred 40 km east of the radar, 20 dBZ around it out to 20 km,
    /// and nothing beyond.
    fn sweep() -> BinnedSweep {
        sweep_of(Moment::Reflectivity, (-32.0, 94.5), 55.0, 20.0)
    }

    /// The same geometry for any moment: `core` inside 5 km, `ring` out to 10 km, no echo
    /// beyond, coded over `(vmin, vmax)`.
    fn sweep_of(moment: Moment, (vmin, vmax): (f32, f32), core_v: f32, ring_v: f32) -> BinnedSweep {
        let (rows, gates) = (360usize, 100usize);
        let code = |dbz: f32| (2.0 + (dbz - vmin) / (vmax - vmin) * 253.0).round() as u8;
        let (lat0, lon0) = (35.33f64, -97.28f64);
        let core = (lon0 + 40.0 / (111.2 * lat0.to_radians().cos()), lat0);
        let mut data = vec![0u8; rows * gates];
        for row in 0..rows {
            let az = (row as f64 + 0.5).to_radians();
            for g in 0..gates {
                let km = g as f64 + 0.5;
                let lat = lat0 + km * az.cos() / 111.2;
                let lon = lon0 + km * az.sin() / (111.2 * lat0.to_radians().cos());
                let (d, _) = crate::geo::great_circle([core.0, core.1], [lon, lat]);
                data[row * gates + g] = if d < 5.0 {
                    code(core_v)
                } else if d < 10.0 {
                    code(ring_v)
                } else {
                    0
                };
            }
        }
        BinnedSweep {
            moment,
            az_bins: rows,
            gate_count: gates,
            data,
            first_gate_km: 0.0,
            gate_interval_km: 1.0,
            radar_lat: lat0 as f32,
            radar_lon: lon0 as f32,
            elevation_deg: 0.5,
            value_min: vmin,
            value_max: vmax,
            ..BinnedSweep::default()
        }
    }

    #[test]
    fn a_core_becomes_one_closed_ring_at_its_own_edge() {
        let s = sweep();
        let t = DateTime::from_timestamp(1_791_385_200, 0).unwrap();
        let src = OutlineSource {
            site: "KTLX",
            time: t,
        };
        let f = threshold_outlines(&s, &src, &[35.0, 60.0]);
        // Nothing reaches 60 dBZ; the 55 dBZ disc gives one closed 35 dBZ ring.
        assert_eq!(f.len(), 1, "{f:?}");
        let p = &f[0].properties;
        assert_eq!(p["site"], "KTLX");
        assert_eq!(p["product"], "REF");
        assert_eq!(p["unit"], "dBZ");
        assert_eq!(p["threshold"], 35.0);
        assert_eq!(p["closed"], true);
        assert_eq!(p["scan_time"], t.to_rfc3339());
        // The ring sits about 5 km from the core's centre all round.
        let wxdata::gis::Geometry::LineString(pts) = &f[0].geometry else {
            panic!("a line");
        };
        let core = (-97.28 + 40.0 / (111.2 * 35.33f64.to_radians().cos()), 35.33);
        for &[lon, lat] in pts {
            let (d, _) = crate::geo::great_circle([core.0, core.1], [lon, lat]);
            assert!((3.5..6.5).contains(&d), "{d} km");
        }
    }

    #[test]
    fn no_echo_is_below_reflectivity_but_missing_for_dual_pol() {
        let t = DateTime::from_timestamp(0, 0).unwrap();
        let g = sweep_lattice(&sweep(), t).unwrap();
        assert!(g.values.contains(&NO_ECHO), "no echo inside coverage");
        assert!(
            g.values.iter().any(|v| v.is_nan()),
            "outside coverage is missing"
        );
        let cc = sweep_of(Moment::CorrelationCoefficient, (0.2, 1.05), 0.70, 0.98);
        let g = sweep_lattice(&cc, t).unwrap();
        assert!(!g.values.contains(&NO_ECHO), "CC has no value without echo");
    }

    /// CC's default outline rings the low-CC core and says it encloses what is below, and does
    /// not ring the echo-free area around the storm (1008.md D3). Velocity has no default.
    #[test]
    fn a_low_cc_core_is_ringed_and_echo_free_air_is_not() {
        let t = DateTime::from_timestamp(0, 0).unwrap();
        let src = OutlineSource {
            site: "KTLX",
            time: t,
        };
        let cc = sweep_of(Moment::CorrelationCoefficient, (0.2, 1.05), 0.70, 0.98);
        let f = threshold_outlines(
            &cc,
            &src,
            default_thresholds(Moment::CorrelationCoefficient),
        );
        assert_eq!(f.len(), 1, "one ring, round the core only: {f:?}");
        let p = &f[0].properties;
        assert_eq!(p["product"], "CC");
        assert_eq!(p["encloses"], "below");
        assert_eq!(p["closed"], true);
        let wxdata::gis::Geometry::LineString(pts) = &f[0].geometry else {
            panic!("a line");
        };
        let core = (-97.28 + 40.0 / (111.2 * 35.33f64.to_radians().cos()), 35.33);
        for &[lon, lat] in pts {
            let (d, _) = crate::geo::great_circle([core.0, core.1], [lon, lat]);
            assert!((3.5..6.5).contains(&d), "{d} km");
        }
        let refl = threshold_outlines(&sweep(), &src, &[35.0]);
        assert_eq!(refl[0].properties["encloses"], "above");
        assert!(default_thresholds(Moment::Velocity).is_empty());
        let vel = sweep_of(Moment::Velocity, (-64.0, 64.0), 30.0, 5.0);
        assert!(threshold_outlines(&vel, &src, default_thresholds(Moment::Velocity)).is_empty());
        // A chosen velocity threshold is still honoured.
        assert!(!threshold_outlines(&vel, &src, &[20.0]).is_empty());
    }

    #[test]
    fn thresholds_parse_as_a_list_of_numbers() {
        assert_eq!(parse_thresholds("35, 50 60"), Some(vec![35.0, 50.0, 60.0]));
        assert_eq!(parse_thresholds(" "), Some(vec![]), "empty is none");
        assert_eq!(parse_thresholds("0.8; 0.9"), Some(vec![0.8, 0.9]));
        assert_eq!(parse_thresholds("35, fifty"), None);
        assert_eq!(parse_thresholds("NaN"), None);
    }

    /// The Mayfield 2021 corpus volume's lowest reflectivity sweep outlined, written as GeoJSON
    /// for an independent reader: `HOOKECHO_OUTLINE_SAMPLE=<path> cargo test -p hookecho --lib
    /// real_volume_outlines -- --ignored --nocapture`.
    #[test]
    #[ignore = "writes a review file"]
    fn real_volume_outlines() {
        use wxdata::level2;
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bytes = std::fs::read(
            repo.join("crates/wxdata/tests/data/corpus/mayfield-2021-first-records.ar2"),
        )
        .unwrap();
        let scan = level2::decode_volume(bytes).unwrap();
        let sweep = level2::bin_scan(&scan, Moment::Reflectivity, 0).unwrap();
        let t = DateTime::from_timestamp(1_639_191_600, 0).unwrap();
        let f = threshold_outlines(
            &sweep,
            &OutlineSource {
                site: "KPAH",
                time: t,
            },
            &REFLECTIVITY_OUTLINES_DBZ,
        );
        let by = |th: f64| f.iter().filter(|x| x.properties["threshold"] == th).count();
        let closed = f.iter().filter(|x| x.properties["closed"] == true).count();
        println!(
            "{} lines: 35 dBZ {}, 50 dBZ {}, 60 dBZ {}; {closed} closed",
            f.len(),
            by(35.0),
            by(50.0),
            by(60.0)
        );
        assert!(by(35.0) > 0 && by(50.0) > 0);
        if let Ok(path) = std::env::var("HOOKECHO_OUTLINE_SAMPLE") {
            std::fs::write(path, wxdata::gis::to_geojson(&f)).unwrap();
        }
    }
}
