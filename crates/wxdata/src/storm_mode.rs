//! The shape of the echo a circulation sits in: a long, narrow line (a QLCS) or a compact cell
//! (a supercell). detectionplan.md, round three: tornadoes in a line are often weak, shallow and
//! debris-free, so the fusion sees them as rotation of the same strength as a hail storm's
//! mesocyclone. The echo they are embedded in is one thing the two do not share.
//!
//! [`EchoObjects::label`] labels every connected region of a sweep at or above a reflectivity
//! threshold once (4-connected on the polar grid, wrapping in azimuth), and measures each one's
//! area and principal axes on the ground. [`EchoObjects::shape_at`] gives a position the shape of
//! the object under it, or of the strongest gate of an object within a search radius when the
//! position itself is in weaker echo (a circulation sits at a cell's edge, in its notch).
//!
//! Length and width are the principal axes of the object's gates weighted by their ground area,
//! scaled so a uniform rectangle reads its own sides: `√(12·λ)` for each covariance eigenvalue.

use crate::level2::BinnedSweep;

/// Reflectivity at and above which echo counts as convective core, dBZ.
pub const CORE_DBZ: f32 = 40.0;
/// How far from a circulation to look for its core, km.
pub const SEARCH_KM: f32 = 5.0;

/// The ground shape of one echo object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchoShape {
    pub length_km: f32,
    pub width_km: f32,
    pub area_km2: f32,
}

impl EchoShape {
    /// Length over width; 1 for a circle, large for a line.
    pub fn aspect(&self) -> f32 {
        self.length_km / self.width_km.max(0.1)
    }
}

/// The labelled echo objects of one sweep.
pub struct EchoObjects<'a> {
    sweep: &'a BinnedSweep,
    /// Per gate, azimuth-major: the object's index + 1, or 0 below the threshold.
    labels: Vec<u32>,
    shapes: Vec<EchoShape>,
}

impl<'a> EchoObjects<'a> {
    /// Label every object at or above `dbz`.
    pub fn label(sweep: &'a BinnedSweep, dbz: f32) -> EchoObjects<'a> {
        let (na, ng) = (sweep.az_bins, sweep.gate_count);
        let mut labels = vec![0u32; na * ng];
        let mut shapes = Vec::new();
        if na == 0 || ng == 0 || sweep.data.len() != na * ng {
            return EchoObjects {
                sweep,
                labels,
                shapes,
            };
        }
        let span = sweep.value_max - sweep.value_min;
        let code = if span > 0.0 {
            (2.0 + ((dbz - sweep.value_min) / span * 253.0).ceil()).clamp(2.0, 255.0) as u8
        } else {
            255
        };
        let dtheta = std::f64::consts::TAU / na as f64;
        let mut stack = Vec::new();
        for start in 0..na * ng {
            if labels[start] != 0 || sweep.data[start] < code {
                continue;
            }
            let id = shapes.len() as u32 + 1;
            labels[start] = id;
            stack.push(start);
            // Area-weighted moments in km about the radar.
            let (mut w, mut sx, mut sy, mut sxx, mut syy, mut sxy) =
                (0.0f64, 0.0, 0.0, 0.0, 0.0, 0.0);
            while let Some(i) = stack.pop() {
                let (a, g) = (i / ng, i % ng);
                let r = (sweep.first_gate_km + g as f32 * sweep.gate_interval_km) as f64;
                let area = r * dtheta * sweep.gate_interval_km as f64;
                let th = (a as f64 + 0.5) * dtheta;
                let (x, y) = (r * th.sin(), r * th.cos());
                w += area;
                sx += area * x;
                sy += area * y;
                sxx += area * x * x;
                syy += area * y * y;
                sxy += area * x * y;
                let next = [
                    (a + 1) % na * ng + g,
                    (a + na - 1) % na * ng + g,
                    if g + 1 < ng { i + 1 } else { usize::MAX },
                    if g > 0 { i - 1 } else { usize::MAX },
                ];
                for j in next {
                    if j != usize::MAX && labels[j] == 0 && sweep.data[j] >= code {
                        labels[j] = id;
                        stack.push(j);
                    }
                }
            }
            let (mx, my) = (sx / w, sy / w);
            let (cxx, cyy, cxy) = (sxx / w - mx * mx, syy / w - my * my, sxy / w - mx * my);
            let half_trace = (cxx + cyy) / 2.0;
            let root = (((cxx - cyy) / 2.0).powi(2) + cxy * cxy).sqrt();
            let (l1, l2) = ((half_trace + root).max(0.0), (half_trace - root).max(0.0));
            shapes.push(EchoShape {
                length_km: (12.0 * l1).sqrt() as f32,
                width_km: (12.0 * l2).sqrt() as f32,
                area_km2: w as f32,
            });
        }
        EchoObjects {
            sweep,
            labels,
            shapes,
        }
    }

    /// How many objects there are.
    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// The object under (`lon`, `lat`), or else the one holding the strongest gate within
    /// `search_km`; `None` with no object that near.
    pub fn shape_at(&self, lon: f64, lat: f64, search_km: f32) -> Option<EchoShape> {
        let s = self.sweep;
        let at = s.sample_at(lon, lat)?;
        let (na, ng) = (s.az_bins, s.gate_count);
        let bin = ((at.azimuth_deg as f64 / 360.0 * na as f64) as usize) % na;
        let here = self.labels[bin * ng + at.gate];
        if here != 0 {
            return Some(self.shapes[here as usize - 1]);
        }
        let dg = (search_km / s.gate_interval_km.max(1e-3)).ceil() as isize;
        let arc_km = (at.range_km.max(1.0) * std::f32::consts::TAU / na as f32).max(1e-3);
        let da = ((search_km / arc_km).ceil() as isize).min(na as isize / 2);
        let mut best: Option<(u8, u32)> = None;
        for oa in -da..=da {
            let a = (bin as isize + oa).rem_euclid(na as isize) as usize;
            for og in -dg..=dg {
                let g = at.gate as isize + og;
                if g < 0 || g >= ng as isize {
                    continue;
                }
                let i = a * ng + g as usize;
                let id = self.labels[i];
                if id == 0 {
                    continue;
                }
                // Within the radius on the ground, not just the polar box.
                let (ga, gr) = (oa as f32 * arc_km, og as f32 * s.gate_interval_km);
                if ga.hypot(gr) > search_km {
                    continue;
                }
                if best.is_none_or(|(v, _)| s.data[i] > v) {
                    best = Some((s.data[i], id));
                }
            }
        }
        best.map(|(_, id)| self.shapes[id as usize - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sweep of `f(x_km, y_km) -> dBZ` (None for no echo), radar at the origin.
    fn sweep(f: impl Fn(f64, f64) -> Option<f32>) -> BinnedSweep {
        let (na, ng, gate) = (720usize, 600usize, 0.25f32);
        let mut data = vec![0u8; na * ng];
        for a in 0..na {
            let th = (a as f64 + 0.5) * std::f64::consts::TAU / na as f64;
            for g in 0..ng {
                let r = 2.125 + g as f64 * gate as f64;
                if let Some(v) = f(r * th.sin(), r * th.cos()) {
                    data[a * ng + g] = (2.0 + ((v + 32.0) / 126.5 * 253.0).round()) as u8;
                }
            }
        }
        BinnedSweep {
            az_bins: na,
            gate_count: ng,
            data,
            first_gate_km: 2.125,
            gate_interval_km: gate,
            radar_lat: 35.0,
            radar_lon: -97.0,
            elevation_deg: 0.5,
            value_min: -32.0,
            value_max: 94.5,
            ..Default::default()
        }
    }

    fn lonlat(x: f64, y: f64) -> (f64, f64) {
        (
            -97.0 + x / (111.32 * 35f64.to_radians().cos()),
            35.0 + y / 110.57,
        )
    }

    #[test]
    fn a_line_reads_long_and_narrow_and_a_cell_round() {
        // A 90 km by 6 km line, north-south, 60 km east; a 16 km cell 60 km west.
        let s = sweep(|x, y| {
            if (x - 60.0).abs() <= 3.0 && y.abs() <= 45.0 {
                Some(50.0)
            } else if (x + 60.0).hypot(y) <= 8.0 {
                Some(55.0)
            } else if x.hypot(y) > 100.0 && x.hypot(y) < 101.0 {
                Some(30.0) // weak echo: below the core threshold
            } else {
                None
            }
        });
        let objs = EchoObjects::label(&s, CORE_DBZ);
        assert_eq!(objs.len(), 2);
        let (lx, ly) = lonlat(60.0, 10.0);
        let line = objs.shape_at(lx, ly, SEARCH_KM).unwrap();
        assert!((line.length_km - 90.0).abs() < 4.0, "{line:?}");
        assert!((line.width_km - 6.0).abs() < 1.5, "{line:?}");
        assert!(line.aspect() > 10.0);
        let (cx, cy) = lonlat(-60.0, 0.0);
        let cell = objs.shape_at(cx, cy, SEARCH_KM).unwrap();
        assert!(cell.aspect() < 1.2, "{cell:?}");
        assert!((cell.area_km2 - 201.0).abs() < 15.0, "{cell:?}");
        // At a cell's edge, in weaker echo, the search finds the cell; far from any, nothing.
        let (ex, ey) = lonlat(-60.0, 11.0);
        assert_eq!(objs.shape_at(ex, ey, SEARCH_KM), Some(cell));
        let (fx, fy) = lonlat(0.0, -60.0);
        assert_eq!(objs.shape_at(fx, fy, SEARCH_KM), None);
    }

    #[test]
    fn an_object_across_north_is_one_object() {
        // A line along x at y = 50 km crosses azimuth 0.
        let s = sweep(|x, y| ((y - 50.0).abs() <= 2.0 && x.abs() <= 20.0).then_some(45.0));
        let objs = EchoObjects::label(&s, CORE_DBZ);
        assert_eq!(objs.len(), 1);
        let (x, y) = lonlat(-15.0, 50.0);
        let line = objs.shape_at(x, y, SEARCH_KM).unwrap();
        assert!((line.length_km - 40.0).abs() < 3.0, "{line:?}");
        assert!(EchoObjects::label(&BinnedSweep::default(), CORE_DBZ).is_empty());
    }
}
