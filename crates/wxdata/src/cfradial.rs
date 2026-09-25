//! CF/Radial 1.4 export of a radar volume (ROADMAP_NEW M5) — the community format Py-ART,
//! LROSE/Radx, wradlib and the NCAR tools read — in the NetCDF classic format written by
//! [`crate::netcdf::NcFile`].
//!
//! What is written is the app's **binned** volume: every tilt of every moment given, each resampled
//! to a fixed set of azimuth bins and gates, with 8-bit values. That is the data every display,
//! detector and probe here works from, so an export matches what was analysed — but it is not the
//! raw Level II moment words, and a study that needs those should read the archive file itself
//! (named in the analysis export's provenance). Each field is stored as bytes with `scale_factor` /
//! `add_offset`, which carries the binned values exactly: a gate's byte is its binned code shifted
//! into the signed range, and the two attributes turn it back into the value.
//!
//! Layout, per the CF/Radial 1.4 conventions: dimensions `time` (every ray of every sweep, sweep by
//! sweep), `range` (one gate spacing shared by all fields) and `sweep`; per-ray `time`, `azimuth`
//! and `elevation`; per-sweep number, mode, fixed angle and first/last ray; the radar's location;
//! and each moment as `(time, range)`. A moment a tilt did not carry, or a gate past its sweep's
//! reach, is the fill value.

use crate::level2::{BinnedSweep, Moment};
use crate::netcdf::{texts, Attr, Data, NcFile, Var};
use chrono::{DateTime, Utc};

/// Fixed width of the string variables (`sweep_mode`, the time-coverage strings).
const STRING_LENGTH: usize = 32;
/// The byte that marks "no value": below threshold, range folded, or not scanned.
const FILL: i8 = -128;

/// Where the radar is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Site<'a> {
    pub id: &'a str,
    pub lat: f64,
    pub lon: f64,
    /// Antenna altitude above mean sea level, metres.
    pub altitude_m: f64,
}

/// The CF/Radial field name, standard name and units for a moment.
fn field_meta(m: Moment) -> (&'static str, &'static str, &'static str) {
    match m {
        Moment::Reflectivity => ("DBZ", "equivalent_reflectivity_factor", "dBZ"),
        Moment::Velocity => (
            "VEL",
            "radial_velocity_of_scatterers_away_from_instrument",
            "m/s",
        ),
        Moment::SpectrumWidth => ("WIDTH", "doppler_spectrum_width", "m/s"),
        Moment::DifferentialReflectivity => ("ZDR", "log_differential_reflectivity_hv", "dB"),
        Moment::DifferentialPhase => ("PHIDP", "differential_phase_hv", "degrees"),
        Moment::SpecificDifferentialPhase => {
            ("KDP", "specific_differential_phase_hv", "degrees/km")
        }
        Moment::CorrelationCoefficient => ("RHOHV", "cross_correlation_ratio_hv", "unitless"),
    }
}

/// A fixed-width, NUL-padded string for a `char(..., string_length)` variable.
fn fixed(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.truncate(STRING_LENGTH);
    v.resize(STRING_LENGTH, 0);
    v
}

/// The step between binned codes, and the offset that makes `value = offset + step * byte`
/// where `byte = code - 128` — so a binned code is stored unchanged apart from the shift.
fn scale_of(s: &BinnedSweep) -> (f32, f32) {
    let step = (s.value_max - s.value_min) / 253.0;
    (step, s.value_min + 126.0 * step)
}

/// The bytes of a CF/Radial 1.4 file of the volume: `tilts[i]` holds the sweeps of tilt `i` (low
/// to high), one per moment, any subset; `volume_time` stands in for rays with no time of their
/// own. The range grid is the first sweep's gate spacing, reaching as far as any sweep does.
/// `None` when there is nothing to write or the file would pass the classic format's 2 GiB.
pub fn write(
    site: Site,
    volume_time: DateTime<Utc>,
    tilts: &[Vec<BinnedSweep>],
) -> Option<Vec<u8>> {
    let first = tilts.iter().flatten().next()?;
    let (r0, dr) = (first.first_gate_km, first.gate_interval_km);
    if dr <= 0.0 {
        return None;
    }
    let reach = tilts
        .iter()
        .flatten()
        .map(|s| s.first_gate_km + s.gate_count as f32 * s.gate_interval_km)
        .fold(0.0f32, f32::max);
    let nrange = (((reach - r0) / dr).ceil() as usize).max(1);
    // Moments present anywhere, in the usual order; each is one field.
    let moments: Vec<Moment> = Moment::ALL
        .into_iter()
        .filter(|m| tilts.iter().flatten().any(|s| s.moment == *m))
        .collect();
    // The reference scale for each moment: the first sweep of it.
    let scales: Vec<(f32, f32)> = moments
        .iter()
        .map(|m| scale_of(tilts.iter().flatten().find(|s| s.moment == *m).unwrap()))
        .collect();

    // Rays: each tilt's azimuth bins, from its sweep with the most of them.
    let mut ray_time: Vec<DateTime<Utc>> = Vec::new();
    let (mut azimuth, mut elevation) = (Vec::new(), Vec::new());
    let (mut starts, mut ends, mut fixed_angle) = (Vec::new(), Vec::new(), Vec::new());
    let mut fields: Vec<Vec<i8>> = vec![Vec::new(); moments.len()];
    for sweeps in tilts.iter().filter(|t| !t.is_empty()) {
        let ray_ref = sweeps.iter().max_by_key(|s| s.az_bins).unwrap();
        let n = ray_ref.az_bins;
        starts.push(azimuth.len() as i32);
        fixed_angle.push(ray_ref.elevation_deg);
        for a in 0..n {
            let az = (a as f64 + 0.5) * 360.0 / n as f64;
            azimuth.push(az as f32);
            elevation.push(ray_ref.elevation_deg);
            let t = ray_ref
                .bin_time_ms
                .get(a)
                .copied()
                .filter(|t| *t > 0)
                .and_then(DateTime::from_timestamp_millis)
                .unwrap_or(volume_time);
            ray_time.push(t);
            for (mi, m) in moments.iter().enumerate() {
                let out = &mut fields[mi];
                let Some(s) = sweeps.iter().find(|s| s.moment == *m) else {
                    out.extend(std::iter::repeat_n(FILL, nrange));
                    continue;
                };
                let bin = ((az / 360.0 * s.az_bins as f64) as usize) % s.az_bins.max(1);
                let same_scale = scale_of(s) == scales[mi];
                let (step, offset) = scales[mi];
                for g in 0..nrange {
                    let r = r0 + g as f32 * dr;
                    let gi = ((r - s.first_gate_km) / s.gate_interval_km).round();
                    let code = if gi < 0.0 || gi as usize >= s.gate_count {
                        0
                    } else {
                        s.data[bin * s.gate_count + gi as usize]
                    };
                    out.push(if code < 2 {
                        FILL
                    } else if same_scale {
                        (code as i16 - 128) as i8
                    } else {
                        // A sweep binned over another range (a dealiased velocity can be): its
                        // value, re-expressed on the field's own scale.
                        let v =
                            s.value_min + (code - 2) as f32 * (s.value_max - s.value_min) / 253.0;
                        ((v - offset) / step).round().clamp(-127.0, 127.0) as i8
                    });
                }
            }
        }
        ends.push(azimuth.len() as i32 - 1);
    }
    if azimuth.is_empty() {
        return None;
    }
    let start = *ray_time.iter().min()?;
    let end = *ray_time.iter().max()?;
    let iso = |t: DateTime<Utc>| t.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let (nrays, nsweeps) = (azimuth.len(), starts.len());

    const TIME: usize = 0;
    const RANGE: usize = 1;
    const SWEEP: usize = 2;
    const STRLEN: usize = 3;
    let scalar = |name: &str, attrs, data| Var {
        name: name.into(),
        dims: Vec::new(),
        attrs,
        data,
    };
    let string = |name: &str, s: &str| Var {
        name: name.into(),
        dims: vec![STRLEN],
        attrs: Vec::new(),
        data: Data::Char(fixed(s)),
    };
    let mut vars = vec![
        scalar("volume_number", Vec::new(), Data::Int(vec![0])),
        string("platform_type", "fixed"),
        string("instrument_type", "radar"),
        string("primary_axis", "axis_z"),
        string("time_coverage_start", &iso(start)),
        string("time_coverage_end", &iso(end)),
        scalar(
            "latitude",
            texts(&[("units", "degrees_north"), ("standard_name", "latitude")]),
            Data::Double(vec![site.lat]),
        ),
        scalar(
            "longitude",
            texts(&[("units", "degrees_east"), ("standard_name", "longitude")]),
            Data::Double(vec![site.lon]),
        ),
        scalar(
            "altitude",
            texts(&[("units", "meters"), ("standard_name", "altitude")]),
            Data::Double(vec![site.altitude_m]),
        ),
        Var {
            name: "sweep_number".into(),
            dims: vec![SWEEP],
            attrs: texts(&[("long_name", "sweep_index_number_0_based")]),
            data: Data::Int((0..nsweeps as i32).collect()),
        },
        Var {
            name: "sweep_mode".into(),
            dims: vec![SWEEP, STRLEN],
            attrs: texts(&[("long_name", "scan_mode_for_sweep")]),
            data: Data::Char(
                (0..nsweeps)
                    .flat_map(|_| fixed("azimuth_surveillance"))
                    .collect(),
            ),
        },
        Var {
            name: "fixed_angle".into(),
            dims: vec![SWEEP],
            attrs: texts(&[
                ("long_name", "ray_target_fixed_angle"),
                ("units", "degrees"),
            ]),
            data: Data::Float(fixed_angle),
        },
        Var {
            name: "sweep_start_ray_index".into(),
            dims: vec![SWEEP],
            attrs: texts(&[("long_name", "index_of_first_ray_in_sweep")]),
            data: Data::Int(starts),
        },
        Var {
            name: "sweep_end_ray_index".into(),
            dims: vec![SWEEP],
            attrs: texts(&[("long_name", "index_of_last_ray_in_sweep")]),
            data: Data::Int(ends),
        },
        Var {
            name: "time".into(),
            dims: vec![TIME],
            attrs: texts(&[
                ("standard_name", "time"),
                ("long_name", "time_in_seconds_since_volume_start"),
                ("units", &format!("seconds since {}", iso(start))),
                ("calendar", "gregorian"),
            ]),
            data: Data::Double(
                ray_time
                    .iter()
                    .map(|t| (*t - start).num_milliseconds() as f64 / 1000.0)
                    .collect(),
            ),
        },
        Var {
            name: "range".into(),
            dims: vec![RANGE],
            attrs: {
                let mut a = texts(&[
                    ("standard_name", "projection_range_coordinate"),
                    ("long_name", "range_to_measurement_volume"),
                    ("units", "meters"),
                    ("spacing_is_constant", "true"),
                    ("axis", "radial_range_coordinate"),
                ]);
                a.push((
                    "meters_to_center_of_first_gate".into(),
                    Attr::Float(r0 * 1000.0),
                ));
                a.push(("meters_between_gates".into(), Attr::Float(dr * 1000.0)));
                a
            },
            data: Data::Float((0..nrange).map(|g| (r0 + g as f32 * dr) * 1000.0).collect()),
        },
        Var {
            name: "azimuth".into(),
            dims: vec![TIME],
            attrs: texts(&[
                ("standard_name", "ray_azimuth_angle"),
                ("long_name", "azimuth_angle_from_true_north"),
                ("units", "degrees"),
                ("axis", "radial_azimuth_coordinate"),
            ]),
            data: Data::Float(azimuth),
        },
        Var {
            name: "elevation".into(),
            dims: vec![TIME],
            attrs: texts(&[
                ("standard_name", "ray_elevation_angle"),
                ("long_name", "elevation_angle_from_horizontal_plane"),
                ("units", "degrees"),
                ("axis", "radial_elevation_coordinate"),
            ]),
            data: Data::Float(elevation),
        },
    ];
    for ((m, data), (step, offset)) in moments.iter().zip(fields).zip(&scales) {
        let (name, standard, units) = field_meta(*m);
        let mut attrs = texts(&[
            ("long_name", crate_long_name(*m)),
            ("standard_name", standard),
            ("units", units),
            ("coordinates", "time range"),
        ]);
        attrs.push(("scale_factor".into(), Attr::Float(*step)));
        attrs.push(("add_offset".into(), Attr::Float(*offset)));
        attrs.push(("_FillValue".into(), Attr::Byte(FILL)));
        vars.push(Var {
            name: name.into(),
            dims: vec![TIME, RANGE],
            attrs,
            data: Data::Byte(data),
        });
    }
    NcFile {
        dims: vec![
            ("time".into(), nrays),
            ("range".into(), nrange),
            ("sweep".into(), nsweeps),
            ("string_length".into(), STRING_LENGTH),
        ],
        globals: texts(&[
            ("Conventions", "CF/Radial instrument_parameters"),
            ("version", "1.4"),
            ("title", &format!("{} volume {}", site.id, iso(volume_time))),
            ("institution", ""),
            ("references", ""),
            (
                "source",
                "NOAA NEXRAD Level II, binned by HookEcho (8-bit per field)",
            ),
            ("history", "written by HookEcho"),
            (
                "comment",
                "Resampled to fixed azimuth bins and gates; not the raw moment words",
            ),
            ("instrument_name", site.id),
            ("platform_is_mobile", "false"),
            ("time_coverage_start", &iso(start)),
            ("time_coverage_end", &iso(end)),
        ]),
        vars,
    }
    .to_bytes()
}

/// A readable name for a moment's field.
fn crate_long_name(m: Moment) -> &'static str {
    match m {
        Moment::Reflectivity => "reflectivity",
        Moment::Velocity => "radial_velocity",
        Moment::SpectrumWidth => "spectrum_width",
        Moment::DifferentialReflectivity => "differential_reflectivity",
        Moment::DifferentialPhase => "differential_phase",
        Moment::SpecificDifferentialPhase => "specific_differential_phase",
        Moment::CorrelationCoefficient => "correlation_coefficient",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netcdf::spec_read::parse;

    /// A sweep of `moment` at `elev` with `az_bins` x `gates`, each gate's code from `f`.
    fn sweep(
        moment: Moment,
        elev: f32,
        az_bins: usize,
        gates: usize,
        f: impl Fn(usize, usize) -> u8,
    ) -> BinnedSweep {
        let (lo, hi) = moment.value_range();
        let mut data = vec![0u8; az_bins * gates];
        for a in 0..az_bins {
            for g in 0..gates {
                data[a * gates + g] = f(a, g);
            }
        }
        BinnedSweep {
            moment,
            az_bins,
            gate_count: gates,
            data,
            first_gate_km: 2.0,
            gate_interval_km: 0.25,
            radar_lat: 35.333,
            radar_lon: -97.278,
            elevation_deg: elev,
            value_min: lo,
            value_max: hi,
            ..Default::default()
        }
    }

    fn site() -> Site<'static> {
        Site {
            id: "KTLX",
            lat: 35.333,
            lon: -97.278,
            altitude_m: 384.0,
        }
    }

    #[test]
    fn a_volume_walks_by_the_spec_with_every_cfradial_coordinate() {
        let t = "2013-05-20T20:08:00Z".parse().unwrap();
        // Tilt 0: REF and VEL, 360 rays; tilt 1: REF only, 720 rays and shorter reach.
        let tilts = vec![
            vec![
                sweep(Moment::Reflectivity, 0.5, 360, 40, |a, g| {
                    ((a + g) % 250 + 2) as u8
                }),
                sweep(
                    Moment::Velocity,
                    0.5,
                    360,
                    30,
                    |_, g| if g == 0 { 1 } else { 130 },
                ),
            ],
            vec![sweep(Moment::Reflectivity, 1.5, 720, 20, |_, _| 100)],
        ];
        let bytes = write(site(), t, &tilts).unwrap();
        let p = parse(&bytes);
        let dim = |n: &str| p.dims.iter().find(|d| d.0 == n).unwrap().1;
        assert_eq!(dim("time"), 360 + 720);
        assert_eq!(dim("range"), 40);
        assert_eq!(dim("sweep"), 2);
        assert_eq!(p.globals["Conventions"], "CF/Radial instrument_parameters");
        assert_eq!(p.globals["version"], "1.4");
        for v in [
            "time",
            "range",
            "azimuth",
            "elevation",
            "latitude",
            "longitude",
            "altitude",
            "sweep_number",
            "sweep_mode",
            "fixed_angle",
            "sweep_start_ray_index",
            "sweep_end_ray_index",
            "time_coverage_start",
            "DBZ",
            "VEL",
        ] {
            p.var(v);
        }
        let ints = |v: &str, n: usize| -> Vec<i32> {
            let b = p.var(v).begin;
            (0..n)
                .map(|i| i32::from_be_bytes(bytes[b + i * 4..b + i * 4 + 4].try_into().unwrap()))
                .collect()
        };
        assert_eq!(ints("sweep_start_ray_index", 2), vec![0, 360]);
        assert_eq!(ints("sweep_end_ray_index", 2), vec![359, 1079]);
        let range0 = f32::from_be_bytes(bytes[p.var("range").begin..][..4].try_into().unwrap());
        assert_eq!(range0, 2000.0);
        assert_eq!(p.var("range").attrs["meters_between_gates"], "250");

        // Values come back through scale_factor/add_offset exactly as binned.
        let dbz = p.var("DBZ");
        assert_eq!(dbz.dims, vec![0, 1]);
        assert_eq!(dbz.kind, 1, "stored as bytes");
        assert_eq!(dbz.vsize, (360 + 720) * 40);
        let step: f32 = dbz.attrs["scale_factor"].parse().unwrap();
        let offset: f32 = dbz.attrs["add_offset"].parse().unwrap();
        assert_eq!(dbz.attrs["_FillValue"], "-128");
        let at =
            |var: &str, ray: usize, gate: usize| bytes[p.var(var).begin + ray * 40 + gate] as i8;
        let s = &tilts[0][0];
        let want = |code: u8| s.value_min + (code - 2) as f32 * (s.value_max - s.value_min) / 253.0;
        let got = offset + step * at("DBZ", 7, 5) as f32;
        // Ray 7, gate 5 was binned as code (7 + 5) + 2 = 14.
        assert!((got - want(14)).abs() < 1e-3, "{got}");
        // Past the second tilt's 20 gates is fill; so is VEL where the tilt had none, and a
        // below-threshold gate.
        assert_eq!(at("DBZ", 360, 25), FILL);
        assert_eq!(at("VEL", 400, 10), FILL, "tilt 1 has no velocity");
        assert_eq!(at("VEL", 3, 0), FILL, "code 1 (folded) is no value");
        assert_ne!(at("VEL", 3, 5), FILL);
    }

    #[test]
    fn nothing_to_write_is_none() {
        let t = "2013-05-20T20:08:00Z".parse().unwrap();
        assert!(write(site(), t, &[]).is_none());
        assert!(write(site(), t, &[vec![]]).is_none());
    }
}
