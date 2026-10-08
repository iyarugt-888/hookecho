//! Grid-relative winds on a Lambert conformal grid, turned to east and north.
//!
//! NCEP's Lambert-grid models (HRRR, RAP, NAM and its nest) publish `UGRD`/`VGRD` relative to
//! the grid's own x and y axes, not to east and north; the message says so in its grid
//! definition's resolution-and-component flags (GRIB2 code table 3.3, bit 5). On a Lambert cone
//! the grid's y axis points at the cone's apex, so away from the central meridian `LoV` it leans
//! off true north by the convergence angle `n·(λ − LoV)`, where `n` is the cone constant: about
//! 14° over the US east coast for the HRRR. Using the components as east/north turns every wind
//! by that much.
//!
//! The rotation (the inverse of resolving an east/north wind onto the grid axes):
//! `u_e = u·cos α + v·sin α`, `v_n = −u·sin α + v·cos α`, `α = n·(λ − LoV)`.
//!
//! Read from the message's own grid definition (template 3.30) rather than a table of models, so
//! a file that says its winds are already earth-relative is left alone.

/// A Lambert grid whose winds are relative to the grid: what the rotation needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LambertWinds {
    /// The cone constant `n` (`sin φ` for a tangent cone).
    pub cone: f64,
    /// The central meridian `LoV`, degrees east (−180..180).
    pub lov_deg: f64,
}

impl LambertWinds {
    /// From the cone's standard parallels and central meridian (degrees).
    pub fn new(latin1_deg: f64, latin2_deg: f64, lov_deg: f64) -> Self {
        let (p1, p2) = (latin1_deg.to_radians(), latin2_deg.to_radians());
        let cone = if (p1 - p2).abs() < 1e-9 {
            p1.sin()
        } else {
            let t = |p: f64| (std::f64::consts::FRAC_PI_4 + p / 2.0).tan();
            (p1.cos() / p2.cos()).ln() / (t(p2) / t(p1)).ln()
        };
        Self {
            cone,
            lov_deg: wrap180(lov_deg),
        }
    }

    /// The convergence angle at longitude `lon_deg`, radians.
    pub fn angle(&self, lon_deg: f64) -> f64 {
        self.cone * wrap180(lon_deg - self.lov_deg).to_radians()
    }

    /// One grid-relative `(u, v)` at `lon_deg`, as east and north.
    pub fn to_earth(&self, u: f64, v: f64, lon_deg: f64) -> (f64, f64) {
        let (s, c) = self.angle(lon_deg).sin_cos();
        (u * c + v * s, -u * s + v * c)
    }

    /// Rotate whole native grids in place (`lons` per point); non-finite pairs are left as they
    /// are.
    pub fn rotate(&self, u: &mut [f64], v: &mut [f64], lons: &[f64]) {
        for ((u, v), lon) in u.iter_mut().zip(v.iter_mut()).zip(lons) {
            if u.is_finite() && v.is_finite() && lon.is_finite() {
                (*u, *v) = self.to_earth(*u, *v, *lon);
            }
        }
    }
}

fn wrap180(d: f64) -> f64 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// GRIB2's signed 32-bit: the top bit is the sign, the rest the magnitude.
fn be_i32_signmag(b: &[u8], at: usize) -> Option<i64> {
    let raw = be_u32(b, at)?;
    let mag = i64::from(raw & 0x7fff_ffff);
    Some(if raw & 0x8000_0000 != 0 { -mag } else { mag })
}

/// The rotation a GRIB2 message's winds need: `Some` only for a Lambert conformal grid
/// (template 3.30) whose resolution-and-component flags say u/v are relative to the grid. A
/// lat/lon grid, or Lambert winds already earth-relative, give `None`.
pub fn from_message(raw: &[u8]) -> Option<LambertWinds> {
    // Section 0 is 16 bytes; every later section starts with its length and number.
    let mut at = 16usize;
    while at + 5 <= raw.len() {
        let len = be_u32(raw, at)? as usize;
        let num = raw[at + 4];
        if len < 5 || &raw[at..at + 4] == b"7777" {
            return None;
        }
        if num == 3 {
            let s = raw.get(at..at + len)?;
            let template = u16::from_be_bytes(s.get(12..14)?.try_into().ok()?);
            if template != 30 {
                return None;
            }
            // Octet 47: resolution and component flags; bit 5 (0x08) set = grid-relative.
            if s.get(46)? & 0x08 == 0 {
                return None;
            }
            let lov = f64::from(be_u32(s, 51)?) * 1e-6;
            let latin1 = be_i32_signmag(s, 65)? as f64 * 1e-6;
            let latin2 = be_i32_signmag(s, 69)? as f64 * 1e-6;
            return Some(LambertWinds::new(latin1, latin2, lov));
        }
        at += len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A section 3 holding template 3.30 with the HRRR's parameters, in a minimal message.
    fn message(flags: u8, template: u16) -> Vec<u8> {
        let mut m = b"GRIB".to_vec();
        m.extend([0u8; 12]); // the rest of section 0
        let mut s3 = vec![0u8; 81];
        s3[0..4].copy_from_slice(&81u32.to_be_bytes());
        s3[4] = 3;
        s3[12..14].copy_from_slice(&template.to_be_bytes());
        s3[46] = flags;
        s3[51..55].copy_from_slice(&262_500_000u32.to_be_bytes()); // LoV 262.5°E = 97.5°W
        s3[65..69].copy_from_slice(&38_500_000u32.to_be_bytes());
        s3[69..73].copy_from_slice(&38_500_000u32.to_be_bytes());
        m.extend(s3);
        m.extend(b"7777");
        m
    }

    #[test]
    fn the_grid_definition_says_whether_and_how_to_rotate() {
        let w = from_message(&message(0x08, 30)).expect("grid-relative Lambert");
        assert!((w.lov_deg + 97.5).abs() < 1e-9, "{}", w.lov_deg);
        assert!((w.cone - 38.5f64.to_radians().sin()).abs() < 1e-12);
        assert_eq!(
            from_message(&message(0x00, 30)),
            None,
            "already earth-relative"
        );
        assert_eq!(from_message(&message(0x08, 0)), None, "a lat/lon grid");
        assert_eq!(from_message(b"GRIB"), None);
    }

    #[test]
    fn the_rotation_matches_the_lambert_grid_axes() {
        // Resolve an east/north wind onto the grid's axes from the projection itself (Snyder's
        // Lambert conformal: θ = n·(λ − λ0), grid +y toward the apex), then turn it back.
        let w = LambertWinds::new(38.5, 38.5, -97.5);
        for lon in [-122.0, -97.5, -75.0, -67.0] {
            let theta = w.angle(lon);
            // The local east and north unit vectors in grid coordinates.
            let east = (theta.cos(), theta.sin());
            let north = (-theta.sin(), theta.cos());
            let (ue, vn) = (7.0, -3.0);
            let (ug, vg) = (ue * east.0 + vn * north.0, ue * east.1 + vn * north.1);
            let (u2, v2) = w.to_earth(ug, vg, lon);
            assert!(
                (u2 - ue).abs() < 1e-9 && (v2 - vn).abs() < 1e-9,
                "{lon}: {u2},{v2}"
            );
        }
        // On the central meridian nothing turns; on the east coast about 14°.
        assert_eq!(w.angle(-97.5), 0.0);
        assert!((w.angle(-75.0).to_degrees() - 14.0).abs() < 0.1);
        // A secant cone: the cone constant lies between the two parallels' sines.
        let s = LambertWinds::new(25.0, 50.0, -95.0);
        assert!(s.cone > 25f64.to_radians().sin() && s.cone < 50f64.to_radians().sin());
    }

    #[test]
    fn whole_grids_rotate_and_gaps_stay_gaps() {
        let w = LambertWinds::new(38.5, 38.5, -97.5);
        let (mut u, mut v) = (vec![10.0, f64::NAN, 0.0], vec![0.0, 5.0, 10.0]);
        w.rotate(&mut u, &mut v, &[-75.0, -75.0, -97.5]);
        let a = w.angle(-75.0);
        assert!((u[0] - 10.0 * a.cos()).abs() < 1e-9 && (v[0] + 10.0 * a.sin()).abs() < 1e-9);
        assert!(
            u[1].is_nan() && v[1] == 5.0,
            "a gap is not invented into a wind"
        );
        assert_eq!((u[2], v[2]), (0.0, 10.0));
    }
}
