//! Vertical association of rotation objects into columns (detectionplan.md Phase 4).
//!
//! [`crate::rotation::detect_volume`] merges per-tilt couplets by single linkage: A near B and B
//! near C put A, B and C in one couplet however far A is from C, so three displaced detections
//! can read as a deep, rooted column. Here a column grows one tilt at a time, lowest first, and an
//! object joins a column only if it is consistent with *every* member already in it (complete
//! linkage): within [`ColumnParams::base_offset_km`] plus [`ColumnParams::tilt_km_per_km`] for
//! each kilometre of height between them, the slack a real mesocyclone needs to lean with height.
//! It must also turn the same way, and a column takes at most one object per tilt.
//!
//! A column is described by physical height, not by how many tilts it spans: four high tilts are
//! not a circulation rooted below 1 km. Heights are beam-centre heights above the radar, taken as
//! the height above ground; the radar's own tower and the terrain between are ignored.
//!
//! A tornado can clear the echo from its own core, and its lowest-tilt shear can sit over almost
//! none (Mayfield's strongest lowest-tilt gate: 3–12 dBZ), so the object there never forms with
//! echo required. [`columns_with_support`] takes, per tilt, objects found without that screen;
//! after the columns are built from credible objects, each column may take one on a tilt it is
//! missing, by the same rules. Support never starts a column, so shear in clear air with nothing
//! above it is still nothing; a member that came from it is [`ColumnMember::weak_echo`].
//!
//! [`from_sweeps`] runs the whole per-volume pipeline (field, objects with and without the echo
//! screen, columns) with the default parameters, so the app and the backtest compute exactly the
//! same columns.

use crate::level2::BinnedSweep;
use crate::rotation::Sense;
use crate::rotation_objects::RotationObject;

/// Version of the association rules, recorded with anything derived from them.
pub const ALGORITHM_VERSION: &str = "llsd-columns-1";

/// The least depth (km) a column needs before its lean is measured.
pub const MIN_LEAN_DEPTH_KM: f32 = 0.5;

/// Height layers the plan describes columns by, km above radar level.
pub const LOW_LEVEL_KM: (f32, f32) = (0.0, 2.0);
pub const TRANSITION_KM: (f32, f32) = (2.0, 3.0);
pub const MID_LEVEL_KM: (f32, f32) = (3.0, 6.0);

/// How objects are associated. Provisional, for the backtest to tune.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnParams {
    /// Horizontal distance (km) two members may be apart at the same height: an object's centre
    /// moves about this much from tilt to tilt with sampling and the LLSD kernel alone.
    pub base_offset_km: f32,
    /// Extra horizontal distance (km) allowed per kilometre of height between two members: a
    /// mesocyclone leaning up to 45°.
    pub tilt_km_per_km: f32,
    /// A member's peak shear may be at most this many times another's (either way).
    pub max_shear_ratio: f32,
}

impl Default for ColumnParams {
    fn default() -> Self {
        ColumnParams {
            base_offset_km: 3.0,
            tilt_km_per_km: 1.0,
            max_shear_ratio: 6.0,
        }
    }
}

/// One object in a column, and which tilt (index into the caller's tilts, lowest first) it is on.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnMember {
    pub tilt: usize,
    pub object: RotationObject,
    /// Joined from the support objects: storm context came from the column, not this tilt.
    pub weak_echo: bool,
}

/// One rotation followed up through the tilts.
#[derive(Debug, Clone, PartialEq)]
pub struct RotationColumn {
    /// Position of the lowest member.
    pub lon: f64,
    pub lat: f64,
    pub sense: Sense,
    /// Lowest first.
    pub members: Vec<ColumnMember>,
    /// Beam heights of the lowest and highest members, km above radar level.
    pub base_km: f32,
    pub top_km: f32,
    pub depth_km: f32,
    /// A member on the lowest tilt the caller gave: the circulation reaches the lowest sample.
    pub rooted: bool,
    /// Strongest peak shear (s⁻¹) of the members in each layer, if any member is in it.
    pub low_level_azshear: Option<f32>,
    pub transition_azshear: Option<f32>,
    pub mid_level_azshear: Option<f32>,
    /// Strongest peak shear of any member.
    pub max_azshear: f32,
    /// Peak shear integrated over height (s⁻¹·km), trapezoids between members: depth and strength
    /// together. A single member has none.
    pub integrated_azshear: f32,
    /// Horizontal offset of the top member from the base per kilometre of height, and the bearing
    /// (degrees clockwise from north) it leans toward. `None` for a column shallower than
    /// [`MIN_LEAN_DEPTH_KM`]: there the members' sideways jitter, up to
    /// [`ColumnParams::base_offset_km`], over a few hundred metres of height reads as any lean
    /// at all (Mayfield: 2.6 km over 0.2 km).
    pub lean_km_per_km: Option<f32>,
    pub lean_bearing_deg: Option<f32>,
}

impl RotationColumn {
    pub fn tilts(&self) -> usize {
        self.members.len()
    }
}

fn km(a: &RotationObject, b: &RotationObject) -> f64 {
    crate::tds::ground_km((a.lon, a.lat), (b.lon, b.lat))
}

/// Whether `o` may share a column with `m`.
fn compatible(o: &RotationObject, m: &RotationObject, p: &ColumnParams) -> bool {
    let dh = (o.beam_height_km - m.beam_height_km).abs();
    let allowed = p.base_offset_km + p.tilt_km_per_km * dh;
    let (hi, lo) = if o.max_azshear >= m.max_azshear {
        (o.max_azshear, m.max_azshear)
    } else {
        (m.max_azshear, o.max_azshear)
    };
    o.sense == m.sense && km(o, m) <= allowed as f64 && hi <= p.max_shear_ratio * lo.max(1e-9)
}

/// Associate objects up through the tilts. `tilts[i]` holds tilt `i`'s objects, lowest tilt
/// first; pass the objects to consider (usually only the credible ones). Columns come back
/// strongest first.
///
/// Tilt by tilt from the lowest, strongest object first, each object joins the compatible column
/// it is nearest to (one object per tilt per column), or starts a new one. "Compatible" is against
/// every member, so a column never chains away from where it started.
pub fn columns(tilts: &[Vec<RotationObject>], p: &ColumnParams) -> Vec<RotationColumn> {
    columns_with_support(tilts, &[], p)
}

/// One volume's rotation columns from its `(velocity, reflectivity)` tilt pairs, lowest tilt first:
/// the LLSD field of each velocity tilt, the credible objects on it, the objects found without the
/// echo screen whose only artifact is having none (to root a column in weak echo), and the columns
/// with that support, all with default parameters. Velocity should be dealiased.
pub fn from_sweeps(pairs: &[(BinnedSweep, BinnedSweep)]) -> Vec<RotationColumn> {
    use crate::rotation_objects::{objects, Artifact, ObjectParams};
    let (tilts, support): (Vec<Vec<RotationObject>>, Vec<Vec<RotationObject>>) = pairs
        .iter()
        .map(|(vel, z)| {
            let field = crate::azshear::llsd(vel, &crate::azshear::LlsdParams::default());
            let credible = objects(&field, vel, z, &ObjectParams::default())
                .into_iter()
                .filter(|o| o.credible())
                .collect();
            let relaxed = ObjectParams {
                require_echo: false,
                ..ObjectParams::default()
            };
            let support = objects(&field, vel, z, &relaxed)
                .into_iter()
                .filter(|o| o.artifacts.iter().all(|a| *a == Artifact::NoEcho))
                .collect();
            (credible, support)
        })
        .unzip();
    columns_with_support(&tilts, &support, &ColumnParams::default())
}

/// [`columns`], then each column (strongest first) may take one `support[t]` object on each tilt
/// `t` it has no member on, the nearest that is compatible with every member. Each support object
/// joins at most one column, and none starts a column. `support[t]` is usually tilt `t`'s objects
/// found with [`crate::rotation_objects::ObjectParams::require_echo`] off whose only artifact, if
/// any, is [`crate::rotation_objects::Artifact::NoEcho`].
pub fn columns_with_support(
    tilts: &[Vec<RotationObject>],
    support: &[Vec<RotationObject>],
    p: &ColumnParams,
) -> Vec<RotationColumn> {
    let mut cols: Vec<Vec<ColumnMember>> = Vec::new();
    for (t, objs) in tilts.iter().enumerate() {
        let mut order: Vec<usize> = (0..objs.len()).collect();
        order.sort_by(|&a, &b| {
            objs[b]
                .max_azshear
                .total_cmp(&objs[a].max_azshear)
                .then(objs[a].lon.total_cmp(&objs[b].lon))
                .then(objs[a].lat.total_cmp(&objs[b].lat))
        });
        for i in order {
            let o = &objs[i];
            let best = cols
                .iter()
                .enumerate()
                .filter(|(_, c)| c.iter().all(|m| m.tilt != t && compatible(o, &m.object, p)))
                .map(|(ci, c)| {
                    let d = c
                        .iter()
                        .map(|m| km(o, &m.object))
                        .fold(f64::INFINITY, f64::min);
                    (ci, d)
                })
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let member = ColumnMember {
                tilt: t,
                object: o.clone(),
                weak_echo: false,
            };
            match best {
                Some((ci, _)) => cols[ci].push(member),
                None => cols.push(vec![member]),
            }
        }
    }
    // Support, strongest columns first; ties by position so the order never matters.
    let strength = |c: &Vec<ColumnMember>| {
        c.iter()
            .map(|m| m.object.max_azshear)
            .fold(0.0f32, f32::max)
    };
    let mut order: Vec<usize> = (0..cols.len()).collect();
    order.sort_by(|&a, &b| {
        strength(&cols[b])
            .total_cmp(&strength(&cols[a]))
            .then(cols[a][0].object.lon.total_cmp(&cols[b][0].object.lon))
            .then(cols[a][0].object.lat.total_cmp(&cols[b][0].object.lat))
    });
    let mut used: Vec<Vec<bool>> = support.iter().map(|s| vec![false; s.len()]).collect();
    for ci in order {
        for (t, objs) in support.iter().enumerate() {
            if cols[ci].iter().any(|m| m.tilt == t) {
                continue;
            }
            let best = objs
                .iter()
                .enumerate()
                .filter(|(i, o)| {
                    !used[t][*i] && cols[ci].iter().all(|m| compatible(o, &m.object, p))
                })
                .map(|(i, o)| {
                    let d = cols[ci]
                        .iter()
                        .map(|m| km(o, &m.object))
                        .fold(f64::INFINITY, f64::min);
                    (i, d)
                })
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            if let Some((i, _)) = best {
                used[t][i] = true;
                cols[ci].push(ColumnMember {
                    tilt: t,
                    object: objs[i].clone(),
                    weak_echo: true,
                });
            }
        }
    }
    let mut out: Vec<RotationColumn> = cols.into_iter().map(describe).collect();
    out.sort_by(|a, b| {
        b.max_azshear
            .total_cmp(&a.max_azshear)
            .then(b.depth_km.total_cmp(&a.depth_km))
            .then(a.lon.total_cmp(&b.lon))
            .then(a.lat.total_cmp(&b.lat))
    });
    out
}

fn layer_max(members: &[ColumnMember], (lo, hi): (f32, f32)) -> Option<f32> {
    members
        .iter()
        .filter(|m| (lo..hi).contains(&m.object.beam_height_km))
        .map(|m| m.object.max_azshear)
        .fold(None, |acc: Option<f32>, s| {
            Some(acc.map_or(s, |a| a.max(s)))
        })
}

fn describe(mut members: Vec<ColumnMember>) -> RotationColumn {
    members.sort_by(|a, b| {
        a.object
            .beam_height_km
            .total_cmp(&b.object.beam_height_km)
            .then(a.tilt.cmp(&b.tilt))
    });
    let (base, top) = (&members[0].object, &members[members.len() - 1].object);
    let depth_km = top.beam_height_km - base.beam_height_km;
    let integrated_azshear = members
        .windows(2)
        .map(|w| {
            let (a, b) = (&w[0].object, &w[1].object);
            0.5 * (a.max_azshear + b.max_azshear) * (b.beam_height_km - a.beam_height_km)
        })
        .sum();
    let (lean_km_per_km, lean_bearing_deg) = if depth_km >= MIN_LEAN_DEPTH_KM {
        let d = crate::tds::ground_km((base.lon, base.lat), (top.lon, top.lat)) as f32;
        let dy = (top.lat - base.lat) * 110.57;
        let dx = (top.lon - base.lon) * 111.32 * ((base.lat + top.lat) * 0.5).to_radians().cos();
        let bearing = dx.atan2(dy).to_degrees().rem_euclid(360.0) as f32;
        (Some(d / depth_km), (d > 0.0).then_some(bearing))
    } else {
        (None, None)
    };
    RotationColumn {
        lon: base.lon,
        lat: base.lat,
        sense: base.sense,
        base_km: base.beam_height_km,
        top_km: top.beam_height_km,
        depth_km,
        rooted: members.iter().any(|m| m.tilt == 0),
        low_level_azshear: layer_max(&members, LOW_LEVEL_KM),
        transition_azshear: layer_max(&members, TRANSITION_KM),
        mid_level_azshear: layer_max(&members, MID_LEVEL_KM),
        max_azshear: members
            .iter()
            .map(|m| m.object.max_azshear)
            .fold(0.0, f32::max),
        integrated_azshear,
        lean_km_per_km,
        lean_bearing_deg,
        members,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An object of `shear` at `(lon, lat)` and beam height `h` km, cyclonic unless said otherwise.
    fn obj(lon: f64, lat: f64, h: f32, shear: f32) -> RotationObject {
        RotationObject {
            lon,
            lat,
            area_km2: 6.0,
            diameter_km: 2.8,
            length_km: 3.0,
            width_km: 2.0,
            max_azshear: shear,
            p90_azshear: shear * 0.9,
            median_azshear: shear * 0.6,
            robust_delta_v_ms: 30.0,
            max_delta_v_ms: 40.0,
            mean_texture_ms: 2.0,
            fit_rmse_ms: 2.0,
            valid_fraction: 1.0,
            fold_share: 0.0,
            fold_crossings: 0.0,
            significance: 20.0,
            range_km: 50.0,
            beam_height_km: h,
            elevation_deg: 0.5,
            sense: Sense::Cyclonic,
            gates: 40,
            radials: 5,
            artifacts: Vec::new(),
        }
    }

    /// Longitude `km` east of -97.0 at 35°N.
    fn east(km: f64) -> f64 {
        -97.0 + km / (111.32 * 35f64.to_radians().cos())
    }

    #[test]
    fn a_vertical_circulation_is_one_rooted_column() {
        let tilts = vec![
            vec![obj(east(0.0), 35.0, 0.6, 0.02)],
            vec![obj(east(0.5), 35.0, 1.1, 0.018)],
            vec![obj(east(1.0), 35.0, 1.8, 0.015)],
            vec![obj(east(1.5), 35.0, 2.6, 0.012)],
        ];
        let cols = columns(&tilts, &ColumnParams::default());
        assert_eq!(cols.len(), 1, "{cols:#?}");
        let c = &cols[0];
        assert_eq!(c.tilts(), 4);
        assert!(c.rooted);
        assert!((c.depth_km - 2.0).abs() < 1e-5);
        assert_eq!(c.low_level_azshear, Some(0.02));
        assert_eq!(c.transition_azshear, Some(0.012));
        assert_eq!(c.mid_level_azshear, None);
        // Leaning east, 1.5 km over 2 km.
        assert!(
            (c.lean_km_per_km.unwrap() - 0.75).abs() < 0.02,
            "{:?}",
            c.lean_km_per_km
        );
        assert!((c.lean_bearing_deg.unwrap() - 90.0).abs() < 1.0);
        let integral =
            0.5 * (0.02 + 0.018) * 0.5 + 0.5 * (0.018 + 0.015) * 0.7 + 0.5 * (0.015 + 0.012) * 0.8;
        assert!((c.integrated_azshear - integral).abs() < 1e-6);
    }

    #[test]
    fn three_chained_but_displaced_detections_are_not_a_deep_column() {
        // Each 3.4 km from the one below, so the outer two are 6.8 km apart over 1 km of height:
        // single linkage chains all three; here the top one is too far from the bottom.
        let tilts = vec![
            vec![obj(east(0.0), 35.0, 0.5, 0.012)],
            vec![obj(east(3.4), 35.0, 1.0, 0.012)],
            vec![obj(east(6.8), 35.0, 1.5, 0.012)],
        ];
        let cols = columns(&tilts, &ColumnParams::default());
        assert!(cols.iter().all(|c| c.tilts() <= 2), "{cols:#?}");
        assert!(cols.iter().all(|c| c.depth_km <= 0.5 + 1e-6));
    }

    #[test]
    fn a_column_needs_one_sense_and_comparable_strength() {
        let mut anti = obj(east(0.3), 35.0, 1.0, 0.015);
        anti.sense = Sense::Anticyclonic;
        let weak = obj(east(0.3), 35.0, 1.0, 0.002);
        for other in [anti, weak] {
            let tilts = vec![vec![obj(east(0.0), 35.0, 0.5, 0.015)], vec![other]];
            assert_eq!(columns(&tilts, &ColumnParams::default()).len(), 2);
        }
    }

    #[test]
    fn neighbouring_storms_keep_their_own_columns() {
        // Two circulations 12 km apart, each seen at three tilts.
        let tilts: Vec<Vec<RotationObject>> = [0.5, 1.0, 1.6]
            .iter()
            .map(|&h| {
                vec![
                    obj(east(0.0), 35.0, h, 0.015),
                    obj(east(12.0), 35.0, h, 0.013),
                ]
            })
            .collect();
        let cols = columns(&tilts, &ColumnParams::default());
        assert_eq!(cols.len(), 2, "{cols:#?}");
        assert!(cols.iter().all(|c| c.tilts() == 3 && c.rooted));
    }

    #[test]
    fn a_mid_level_circulation_is_not_rooted() {
        let tilts = vec![
            vec![],
            vec![obj(east(0.0), 35.0, 3.2, 0.012)],
            vec![obj(east(0.5), 35.0, 4.5, 0.014)],
        ];
        let cols = columns(&tilts, &ColumnParams::default());
        assert_eq!(cols.len(), 1);
        assert!(!cols[0].rooted);
        assert_eq!(cols[0].low_level_azshear, None);
        assert_eq!(cols[0].mid_level_azshear, Some(0.014));
    }

    #[test]
    fn one_object_per_tilt_per_column() {
        // Two objects on the lowest tilt 1 km apart, one above: two columns, the upper object
        // joining the nearer.
        let tilts = vec![
            vec![
                obj(east(0.0), 35.0, 0.5, 0.015),
                obj(east(1.0), 35.0, 0.5, 0.012),
            ],
            vec![obj(east(1.1), 35.0, 1.0, 0.013)],
        ];
        let cols = columns(&tilts, &ColumnParams::default());
        assert_eq!(cols.len(), 2);
        let two = cols.iter().find(|c| c.tilts() == 2).unwrap();
        assert!((two.lon - east(1.0)).abs() < 1e-9, "{two:#?}");
    }

    #[test]
    fn a_column_finds_its_root_in_weak_echo_below_it() {
        // Strong rotation at the second and third tilts; on the lowest, the same circulation
        // only without echo (support), plus unrelated weak-echo shear 20 km away.
        let tilts = vec![
            vec![],
            vec![obj(east(0.5), 35.0, 1.0, 0.03)],
            vec![obj(east(1.0), 35.0, 1.6, 0.025)],
        ];
        let support = vec![
            vec![
                obj(east(0.2), 35.0, 0.5, 0.028),
                obj(east(20.0), 35.0, 0.5, 0.02),
            ],
            vec![],
            vec![],
        ];
        let cols = columns_with_support(&tilts, &support, &ColumnParams::default());
        assert_eq!(cols.len(), 1, "support never starts a column: {cols:#?}");
        let c = &cols[0];
        assert!(c.rooted && c.tilts() == 3);
        assert!(c.members[0].weak_echo && !c.members[1].weak_echo);
        assert_eq!(c.low_level_azshear, Some(0.03));
        // Without support it is the same column, unrooted.
        let plain = columns(&tilts, &ColumnParams::default());
        assert!(!plain[0].rooted && plain[0].tilts() == 2);
    }

    #[test]
    fn a_shallow_column_has_no_lean() {
        // 2 km apart over 0.2 km of height: jitter, not a 10 km/km lean.
        let tilts = vec![
            vec![obj(east(0.0), 35.0, 0.6, 0.02)],
            vec![obj(east(2.0), 35.0, 0.8, 0.02)],
        ];
        let c = &columns(&tilts, &ColumnParams::default())[0];
        assert_eq!(c.tilts(), 2);
        assert_eq!((c.lean_km_per_km, c.lean_bearing_deg), (None, None));
    }

    #[test]
    fn columns_do_not_depend_on_input_order() {
        let a = vec![
            vec![
                obj(east(0.0), 35.0, 0.5, 0.015),
                obj(east(12.0), 35.0, 0.5, 0.013),
            ],
            vec![
                obj(east(12.2), 35.0, 1.0, 0.012),
                obj(east(0.3), 35.0, 1.0, 0.016),
            ],
        ];
        let mut b = a.clone();
        for t in &mut b {
            t.reverse();
        }
        let p = ColumnParams::default();
        assert_eq!(columns(&a, &p), columns(&b, &p));
    }
}
