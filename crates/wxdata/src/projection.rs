//! Projected coordinate systems for GIS import (ROADMAP_NEW I2): the three projections nearly
//! every U.S. GIS file is in — Transverse Mercator (every UTM zone and the Transverse Mercator
//! State Plane zones), Lambert Conformal Conic (the other State Plane zones and many statewide
//! systems) and Albers Equal Area (the national CONUS systems, EPSG:5070 and friends) — inverted
//! to longitude/latitude with the ellipsoidal formulas from Snyder's *Map Projections: A Working
//! Manual* (USGS PP 1395) and the EPSG guidance note 7-2.
//!
//! A projection is read from a `.prj` (ESRI or OGC WKT: `PROJECTION`, its `PARAMETER`s, the
//! linear `UNIT` and the `SPHEROID`) or from an EPSG code (a GeoJSON file's legacy `crs` member).
//! The datum rule is the importers' own: NAD 83 (any realisation) and WGS 84 are taken as the
//! same datum, a metre apart over the U.S. and invisible at map scale; anything older (NAD 27)
//! is refused rather than drawn tens of metres off. An unknown projection is a named error.

use anyhow::{anyhow, bail, Result};

/// A reference ellipsoid: semi-major axis (metres) and flattening.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipsoid {
    pub a: f64,
    pub f: f64,
}

impl Ellipsoid {
    /// GRS 80, NAD 83's ellipsoid (WGS 84's differs in the ninth digit of the flattening).
    pub const GRS80: Ellipsoid = Ellipsoid {
        a: 6_378_137.0,
        f: 1.0 / 298.257_222_101,
    };

    fn e2(self) -> f64 {
        self.f * (2.0 - self.f)
    }

    fn e(self) -> f64 {
        self.e2().sqrt()
    }
}

/// Which projection, with the parameters its formulas need (angles in radians).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    TransverseMercator {
        lat0: f64,
        lon0: f64,
        k0: f64,
    },
    /// Two standard parallels, or one (`sp1 == sp2`) with a scale factor `k0` at it.
    LambertConformalConic {
        lat0: f64,
        lon0: f64,
        sp1: f64,
        sp2: f64,
        k0: f64,
    },
    Albers {
        lat0: f64,
        lon0: f64,
        sp1: f64,
        sp2: f64,
    },
}

/// A projected coordinate system: projection, ellipsoid, false origin (metres) and the linear
/// unit its coordinates are written in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    pub kind: Kind,
    pub ellipsoid: Ellipsoid,
    pub false_easting: f64,
    pub false_northing: f64,
    /// Metres per coordinate unit: 1 for metres, 1200/3937 for U.S. survey feet.
    pub unit: f64,
}

/// Meridian arc length from the equator to latitude `phi` (Snyder 3-21).
fn meridian_arc(el: Ellipsoid, phi: f64) -> f64 {
    let e2 = el.e2();
    let (e4, e6) = (e2 * e2, e2 * e2 * e2);
    el.a * ((1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0) * phi
        - (3.0 * e2 / 8.0 + 3.0 * e4 / 32.0 + 45.0 * e6 / 1024.0) * (2.0 * phi).sin()
        + (15.0 * e4 / 256.0 + 45.0 * e6 / 1024.0) * (4.0 * phi).sin()
        - (35.0 * e6 / 3072.0) * (6.0 * phi).sin())
}

/// Snyder's `m` (14-15).
fn m(el: Ellipsoid, phi: f64) -> f64 {
    phi.cos() / (1.0 - el.e2() * phi.sin().powi(2)).sqrt()
}

/// Snyder's `t` (15-9).
fn t(el: Ellipsoid, phi: f64) -> f64 {
    let e = el.e();
    let s = phi.sin();
    (std::f64::consts::FRAC_PI_4 - phi / 2.0).tan() / ((1.0 - e * s) / (1.0 + e * s)).powf(e / 2.0)
}

/// Snyder's `q` (3-12).
fn q(el: Ellipsoid, phi: f64) -> f64 {
    let (e, e2) = (el.e(), el.e2());
    let s = phi.sin();
    (1.0 - e2) * (s / (1.0 - e2 * s * s) - (1.0 / (2.0 * e)) * ((1.0 - e * s) / (1.0 + e * s)).ln())
}

/// Lambert's cone constant `n`, `F` and `rho0` (Snyder 15-8, 15-10, 15-11, with `k0`).
fn lcc_constants(el: Ellipsoid, lat0: f64, sp1: f64, sp2: f64, k0: f64) -> (f64, f64, f64) {
    let n = if (sp1 - sp2).abs() < 1e-12 {
        sp1.sin()
    } else {
        (m(el, sp1).ln() - m(el, sp2).ln()) / (t(el, sp1).ln() - t(el, sp2).ln())
    };
    let f = m(el, sp1) / (n * t(el, sp1).powf(n));
    let rho0 = el.a * f * k0 * t(el, lat0).powf(n);
    (n, f, rho0)
}

/// Albers' `n`, `C` and `rho0` (Snyder 14-14, 14-13, 14-12).
fn albers_constants(el: Ellipsoid, lat0: f64, sp1: f64, sp2: f64) -> (f64, f64, f64) {
    let (m1, m2) = (m(el, sp1), m(el, sp2));
    let (q1, q2) = (q(el, sp1), q(el, sp2));
    let n = if (sp1 - sp2).abs() < 1e-12 {
        sp1.sin()
    } else {
        (m1 * m1 - m2 * m2) / (q2 - q1)
    };
    let c = m1 * m1 + n * q1;
    let rho0 = el.a * (c - n * q(el, lat0)).max(0.0).sqrt() / n;
    (n, c, rho0)
}

impl Projection {
    /// Coordinates in this system (its own unit) to `[lon, lat]` in degrees.
    pub fn inverse(&self, x: f64, y: f64) -> [f64; 2] {
        let el = self.ellipsoid;
        let (e, e2) = (el.e(), el.e2());
        let x = x * self.unit - self.false_easting;
        let y = y * self.unit - self.false_northing;
        let (lon, lat) = match self.kind {
            Kind::TransverseMercator { lat0, lon0, k0 } => {
                let ep2 = e2 / (1.0 - e2);
                let mm = meridian_arc(el, lat0) + y / k0;
                let mu = mm
                    / (el.a * (1.0 - e2 / 4.0 - 3.0 * e2 * e2 / 64.0 - 5.0 * e2.powi(3) / 256.0));
                let e1 = (1.0 - (1.0 - e2).sqrt()) / (1.0 + (1.0 - e2).sqrt());
                let phi1 = mu
                    + (3.0 * e1 / 2.0 - 27.0 * e1.powi(3) / 32.0) * (2.0 * mu).sin()
                    + (21.0 * e1 * e1 / 16.0 - 55.0 * e1.powi(4) / 32.0) * (4.0 * mu).sin()
                    + (151.0 * e1.powi(3) / 96.0) * (6.0 * mu).sin()
                    + (1097.0 * e1.powi(4) / 512.0) * (8.0 * mu).sin();
                let (s, c) = phi1.sin_cos();
                let c1 = ep2 * c * c;
                let t1 = (s / c).powi(2);
                let n1 = el.a / (1.0 - e2 * s * s).sqrt();
                let r1 = el.a * (1.0 - e2) / (1.0 - e2 * s * s).powf(1.5);
                let d = x / (n1 * k0);
                let lat = phi1
                    - (n1 * (s / c) / r1)
                        * (d * d / 2.0
                            - (5.0 + 3.0 * t1 + 10.0 * c1 - 4.0 * c1 * c1 - 9.0 * ep2) * d.powi(4)
                                / 24.0
                            + (61.0 + 90.0 * t1 + 298.0 * c1 + 45.0 * t1 * t1
                                - 252.0 * ep2
                                - 3.0 * c1 * c1)
                                * d.powi(6)
                                / 720.0);
                let lon = lon0
                    + (d - (1.0 + 2.0 * t1 + c1) * d.powi(3) / 6.0
                        + (5.0 - 2.0 * c1 + 28.0 * t1 - 3.0 * c1 * c1
                            + 8.0 * ep2
                            + 24.0 * t1 * t1)
                            * d.powi(5)
                            / 120.0)
                        / c;
                (lon, lat)
            }
            Kind::LambertConformalConic {
                lat0,
                lon0,
                sp1,
                sp2,
                k0,
            } => {
                let (n, f, rho0) = lcc_constants(el, lat0, sp1, sp2, k0);
                let sign = n.signum();
                let rho = sign * (x * x + (rho0 - y).powi(2)).sqrt();
                let theta = (sign * x).atan2(sign * (rho0 - y));
                let tt = (rho / (el.a * f * k0)).powf(1.0 / n);
                let mut phi = std::f64::consts::FRAC_PI_2 - 2.0 * tt.atan();
                for _ in 0..15 {
                    let s = phi.sin();
                    phi = std::f64::consts::FRAC_PI_2
                        - 2.0 * (tt * ((1.0 - e * s) / (1.0 + e * s)).powf(e / 2.0)).atan();
                }
                (theta / n + lon0, phi)
            }
            Kind::Albers {
                lat0,
                lon0,
                sp1,
                sp2,
            } => {
                let (n, c, rho0) = albers_constants(el, lat0, sp1, sp2);
                let sign = n.signum();
                let rho = (x * x + (rho0 - y).powi(2)).sqrt();
                let theta = (sign * x).atan2(sign * (rho0 - y));
                let qq = (c - rho * rho * n * n / (el.a * el.a)) / n;
                let mut phi = (qq / 2.0).clamp(-1.0, 1.0).asin();
                for _ in 0..15 {
                    let (s, co) = phi.sin_cos();
                    let es = e * s;
                    phi += (1.0 - es * es).powi(2) / (2.0 * co)
                        * (qq / (1.0 - e2) - s / (1.0 - es * es)
                            + (1.0 / (2.0 * e)) * ((1.0 - es) / (1.0 + es)).ln());
                }
                (lon0 + theta / n, phi)
            }
        };
        [normalize_lon(lon.to_degrees()), lat.to_degrees()]
    }

    /// `[lon, lat]` in degrees to coordinates in this system (its own unit): the inverse of
    /// [`Self::inverse`], for tests and round trips.
    pub fn forward(&self, lon: f64, lat: f64) -> [f64; 2] {
        let el = self.ellipsoid;
        let (e2, phi) = (el.e2(), lat.to_radians());
        let lam = lon.to_radians();
        let (x, y) = match self.kind {
            Kind::TransverseMercator { lat0, lon0, k0 } => {
                let ep2 = e2 / (1.0 - e2);
                let (s, c) = phi.sin_cos();
                let n = el.a / (1.0 - e2 * s * s).sqrt();
                let tt = (s / c).powi(2);
                let cc = ep2 * c * c;
                let a = (lam - lon0) * c;
                let x = k0
                    * n
                    * (a + (1.0 - tt + cc) * a.powi(3) / 6.0
                        + (5.0 - 18.0 * tt + tt * tt + 72.0 * cc - 58.0 * ep2) * a.powi(5) / 120.0);
                let y = k0
                    * (meridian_arc(el, phi) - meridian_arc(el, lat0)
                        + n * (s / c)
                            * (a * a / 2.0
                                + (5.0 - tt + 9.0 * cc + 4.0 * cc * cc) * a.powi(4) / 24.0
                                + (61.0 - 58.0 * tt + tt * tt + 600.0 * cc - 330.0 * ep2)
                                    * a.powi(6)
                                    / 720.0));
                (x, y)
            }
            Kind::LambertConformalConic {
                lat0,
                lon0,
                sp1,
                sp2,
                k0,
            } => {
                let (n, f, rho0) = lcc_constants(el, lat0, sp1, sp2, k0);
                let rho = el.a * f * k0 * t(el, phi).powf(n);
                let theta = n * (lam - lon0);
                (rho * theta.sin(), rho0 - rho * theta.cos())
            }
            Kind::Albers {
                lat0,
                lon0,
                sp1,
                sp2,
            } => {
                let (n, c, rho0) = albers_constants(el, lat0, sp1, sp2);
                let rho = el.a * (c - n * q(el, phi)).max(0.0).sqrt() / n;
                let theta = n * (lam - lon0);
                (rho * theta.sin(), rho0 - rho * theta.cos())
            }
        };
        [
            (x + self.false_easting) / self.unit,
            (y + self.false_northing) / self.unit,
        ]
    }
}

fn normalize_lon(lon: f64) -> f64 {
    (lon + 180.0).rem_euclid(360.0) - 180.0
}

// ---- WKT ----------------------------------------------------------------------------------------

/// Every `KEY["name", n, …]` in `wkt`, case-insensitively on `KEY`, as `(name, numbers)`.
fn wkt_items(wkt: &str, key: &str) -> Vec<(String, Vec<f64>)> {
    let upper = wkt.to_ascii_uppercase();
    let pat = format!("{}[", key.to_ascii_uppercase());
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = upper[from..].find(&pat) {
        let start = from + i + pat.len();
        // The item's own brackets, not its nested ones: stop at the matching close.
        let mut depth = 1;
        let mut end = start;
        for (j, ch) in wkt[start..].char_indices() {
            match ch {
                '[' | '(' => depth += 1,
                ']' | ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + j;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &wkt[start..end.max(start)];
        let name = body.split('"').nth(1).unwrap_or("").to_string();
        let mut numbers = Vec::new();
        let mut depth = 0;
        let mut token = String::new();
        let after_name = body.splitn(3, '"').nth(2).unwrap_or(body);
        for ch in after_name.chars() {
            match ch {
                '[' | '(' => depth += 1,
                ']' | ')' => depth -= 1,
                ',' if depth == 0 => {
                    if let Ok(v) = token.trim().parse::<f64>() {
                        numbers.push(v);
                    }
                    token.clear();
                }
                _ if depth == 0 => token.push(ch),
                _ => {}
            }
        }
        if let Ok(v) = token.trim().parse::<f64>() {
            numbers.push(v);
        }
        out.push((name, numbers));
        from = start;
    }
    out
}

/// Is this datum (by the WKT around it) NAD 83 or WGS 84 — the one datum, at map scale?
pub fn datum_is_modern(wkt: &str) -> bool {
    let upper = wkt.to_ascii_uppercase();
    [
        "WGS_1984",
        "WGS 84",
        "WGS84",
        "NORTH_AMERICAN_1983",
        "NORTH AMERICAN DATUM 1983",
        "NAD83",
        "NAD_1983",
        "NAD 83",
    ]
    .iter()
    .any(|k| upper.contains(k))
}

/// A projected system from a `PROJCS` WKT (ESRI `.prj` or OGC).
pub fn from_wkt(wkt: &str) -> Result<Projection> {
    let name = wkt.split('"').nth(1).unwrap_or("unnamed").to_string();
    if !datum_is_modern(wkt) {
        bail!(
            "\"{name}\" is not on NAD 83 or WGS 84; an older datum can sit tens of metres off, so \
             re-export it on NAD 83 or WGS 84"
        );
    }
    let projection = wkt_items(wkt, "PROJECTION")
        .into_iter()
        .next()
        .map(|(n, _)| n.to_ascii_lowercase().replace([' ', '-'], "_"))
        .ok_or_else(|| anyhow!("\"{name}\" names no projection"))?;
    let params = wkt_items(wkt, "PARAMETER");
    let param = |keys: &[&str]| {
        params.iter().find_map(|(n, v)| {
            let n = n.to_ascii_lowercase().replace(' ', "_");
            keys.iter()
                .any(|k| n == *k)
                .then(|| v.first().copied())
                .flatten()
        })
    };
    let deg = |keys: &[&str]| param(keys).unwrap_or(0.0).to_radians();
    let lat0 = deg(&[
        "latitude_of_origin",
        "latitude_of_center",
        "latitude_of_false_origin",
    ]);
    let lon0 = deg(&[
        "central_meridian",
        "longitude_of_origin",
        "longitude_of_center",
        "longitude_of_false_origin",
    ]);
    let k0 = param(&["scale_factor", "scale_factor_at_natural_origin"]).unwrap_or(1.0);
    let sp1 = param(&["standard_parallel_1", "latitude_of_1st_standard_parallel"]);
    let sp2 = param(&["standard_parallel_2", "latitude_of_2nd_standard_parallel"]);
    // The linear unit is the PROJCS's own UNIT: the last one that is not an angle.
    let unit = wkt_items(wkt, "UNIT")
        .into_iter()
        .rev()
        .find(|(n, v)| {
            let n = n.to_ascii_lowercase();
            !n.contains("degree") && !n.contains("radian") && v.first().is_some_and(|&f| f > 0.01)
        })
        .and_then(|(_, v)| v.first().copied())
        .unwrap_or(1.0);
    let ellipsoid = wkt_items(wkt, "SPHEROID")
        .into_iter()
        .chain(wkt_items(wkt, "ELLIPSOID"))
        .next()
        .and_then(|(_, v)| {
            let (a, inv_f) = (*v.first()?, *v.get(1)?);
            (a > 6.3e6 && a < 6.4e6 && inv_f > 250.0).then(|| Ellipsoid { a, f: 1.0 / inv_f })
        })
        .unwrap_or(Ellipsoid::GRS80);
    let kind = if projection.contains("transverse_mercator") {
        Kind::TransverseMercator { lat0, lon0, k0 }
    } else if projection.contains("lambert_conformal_conic") {
        let (sp1, sp2) = match (sp1, sp2) {
            (Some(a), Some(b)) => (a.to_radians(), b.to_radians()),
            // One standard parallel: the 1SP form, tangent at the origin latitude.
            (Some(a), None) => (a.to_radians(), a.to_radians()),
            _ => (lat0, lat0),
        };
        Kind::LambertConformalConic {
            lat0,
            lon0,
            sp1,
            sp2,
            k0,
        }
    } else if projection.contains("albers") {
        let sp1 = sp1.map_or(lat0, f64::to_radians);
        let sp2 = sp2.map_or(sp1, f64::to_radians);
        Kind::Albers {
            lat0,
            lon0,
            sp1,
            sp2,
        }
    } else {
        bail!(
            "\"{name}\" uses the {projection} projection, which HookEcho cannot invert — \
             re-export it as WGS 84 (EPSG:4326)"
        );
    };
    Ok(Projection {
        kind,
        ellipsoid,
        false_easting: param(&["false_easting", "easting_at_false_origin"]).unwrap_or(0.0) * unit,
        false_northing: param(&["false_northing", "northing_at_false_origin"]).unwrap_or(0.0)
            * unit,
        unit,
    })
}

// ---- EPSG codes ---------------------------------------------------------------------------------

/// How an EPSG code's coordinates are read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Epsg {
    /// Longitude/latitude already (4326, 4269, CRS84).
    LonLat,
    WebMercator,
    Projected(Projection),
}

/// The EPSG codes a U.S. GeoJSON file is likely to name: geographic WGS 84 / NAD 83, Web
/// Mercator, the UTM zones on WGS 84 (326xx north, 327xx south) and NAD 83 (269xx), and the
/// CONUS Albers systems (5070, 6350, 5069). Anything else is `None`.
pub fn from_epsg(code: u32) -> Option<Epsg> {
    let utm = |zone: u32, south: bool| {
        Epsg::Projected(Projection {
            kind: Kind::TransverseMercator {
                lat0: 0.0,
                lon0: ((zone as f64) * 6.0 - 183.0).to_radians(),
                k0: 0.9996,
            },
            ellipsoid: Ellipsoid::GRS80,
            false_easting: 500_000.0,
            false_northing: if south { 10_000_000.0 } else { 0.0 },
            unit: 1.0,
        })
    };
    match code {
        4326 | 4269 | 4979 | 4152 | 6318 => Some(Epsg::LonLat),
        3857 | 900913 | 102100 => Some(Epsg::WebMercator),
        32601..=32660 => Some(utm(code - 32600, false)),
        32701..=32760 => Some(utm(code - 32700, true)),
        26901..=26923 => Some(utm(code - 26900, false)),
        5070 | 6350 | 5069 | 102003 => Some(Epsg::Projected(Projection {
            kind: Kind::Albers {
                lat0: 23f64.to_radians(),
                lon0: (-96f64).to_radians(),
                sp1: 29.5f64.to_radians(),
                sp2: 45.5f64.to_radians(),
            },
            ellipsoid: Ellipsoid::GRS80,
            false_easting: 0.0,
            false_northing: 0.0,
            unit: 1.0,
        })),
        _ => None,
    }
}

/// The EPSG code in a CRS name such as `EPSG:26914`, `urn:ogc:def:crs:EPSG::3857` or
/// `http://www.opengis.net/def/crs/EPSG/0/5070`; `CRS84` counts as 4326.
pub fn epsg_in(name: &str) -> Option<u32> {
    let upper = name.to_ascii_uppercase();
    if upper.contains("CRS84") {
        return Some(4326);
    }
    let i = upper.find("EPSG")?;
    // The code is the last number after "EPSG": `EPSG/0/5070` carries a version first.
    upper[i + 4..]
        .split(|c: char| !c.is_ascii_digit())
        .rfind(|d| !d.is_empty())?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
        (a[0] - b[0]).abs() < tol && (a[1] - b[1]).abs() < tol
    }

    #[test]
    fn the_meridian_arc_to_45_degrees_matches_its_published_length() {
        let s = meridian_arc(Ellipsoid::GRS80, 45f64.to_radians());
        assert!((s - 4_984_944.378).abs() < 0.01, "{s}");
    }

    /// EPSG guidance note 7-2's Transverse Mercator example: the British National Grid on the
    /// Airy 1830 ellipsoid, 50°30'N 0°30'E.
    #[test]
    fn transverse_mercator_matches_the_epsg_worked_example() {
        let p = Projection {
            kind: Kind::TransverseMercator {
                lat0: 49f64.to_radians(),
                lon0: (-2f64).to_radians(),
                k0: 0.999_601_271_7,
            },
            ellipsoid: Ellipsoid {
                a: 6_377_563.396,
                f: 1.0 / 299.324_964_6,
            },
            false_easting: 400_000.0,
            false_northing: -100_000.0,
            unit: 1.0,
        };
        let xy = p.forward(0.5, 50.5);
        assert!(close(xy, [577_274.99, 69_740.50], 0.05), "{xy:?}");
        let ll = p.inverse(577_274.99, 69_740.50);
        assert!(close(ll, [0.5, 50.5], 1e-7), "{ll:?}");
    }

    /// EPSG guidance note 7-2's Lambert Conformal Conic (2SP) example: Texas South Central on
    /// NAD 27 (Clarke 1866) in U.S. survey feet, 28°30'N 96°W.
    #[test]
    fn lambert_conformal_conic_matches_the_epsg_worked_example() {
        let dms = |d: f64, m: f64| (d + m / 60.0).to_radians();
        let p = Projection {
            kind: Kind::LambertConformalConic {
                lat0: dms(27.0, 50.0),
                lon0: (-99f64).to_radians(),
                sp1: dms(28.0, 23.0),
                sp2: dms(30.0, 17.0),
                k0: 1.0,
            },
            ellipsoid: Ellipsoid {
                a: 6_378_206.4,
                f: 1.0 / 294.978_698_2,
            },
            false_easting: 2_000_000.0 * 1200.0 / 3937.0,
            false_northing: 0.0,
            unit: 1200.0 / 3937.0,
        };
        let xy = p.forward(-96.0, 28.5);
        assert!(close(xy, [2_963_503.91, 254_759.80], 0.05), "{xy:?}");
        let ll = p.inverse(2_963_503.91, 254_759.80);
        assert!(close(ll, [-96.0, 28.5], 1e-7), "{ll:?}");
    }

    #[test]
    fn albers_puts_the_origin_at_zero_and_round_trips_across_conus() {
        let Some(Epsg::Projected(p)) = from_epsg(5070) else {
            panic!()
        };
        assert!(close(p.forward(-96.0, 23.0), [0.0, 0.0], 1e-6));
        for (lon, lat) in [(-124.0, 48.0), (-97.5, 35.2), (-70.0, 44.0), (-81.0, 25.0)] {
            let xy = p.forward(lon, lat);
            assert!(
                close(p.inverse(xy[0], xy[1]), [lon, lat], 1e-8),
                "{lon},{lat}"
            );
        }
        // Oklahoma City sits a few hundred kilometres west and ~1,400 km north of the origin.
        let okc = p.forward(-97.5, 35.47);
        assert!(okc[0] < 0.0 && okc[0] > -300_000.0 && (1.3e6..1.5e6).contains(&okc[1]));
    }

    #[test]
    fn utm_zones_come_from_their_epsg_codes() {
        let Some(Epsg::Projected(p)) = from_epsg(26914) else {
            panic!()
        };
        // On the central meridian (99°W) the easting is the false easting exactly.
        let xy = p.forward(-99.0, 35.0);
        assert!((xy[0] - 500_000.0).abs() < 1e-6, "{xy:?}");
        let ll = p.inverse(634_000.0, 3_925_000.0);
        assert!(close(ll, [-97.52, 35.46], 0.02), "OKC in zone 14: {ll:?}");
        assert_eq!(from_epsg(4326), Some(Epsg::LonLat));
        assert_eq!(from_epsg(3857), Some(Epsg::WebMercator));
        assert_eq!(from_epsg(2267), None, "a State Plane code needs its .prj");
        assert_eq!(epsg_in("urn:ogc:def:crs:EPSG::26914"), Some(26914));
        assert_eq!(
            epsg_in("http://www.opengis.net/def/crs/EPSG/0/5070"),
            Some(5070)
        );
        assert_eq!(epsg_in("urn:ogc:def:crs:OGC:1.3:CRS84"), Some(4326));
    }

    const OK_SOUTH_FT: &str = r#"PROJCS["NAD_1983_StatePlane_Oklahoma_South_FIPS_3502_Feet",GEOGCS["GCS_North_American_1983",DATUM["D_North_American_1983",SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Lambert_Conformal_Conic"],PARAMETER["False_Easting",1968500.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",-98.0],PARAMETER["Standard_Parallel_1",33.93333333333333],PARAMETER["Standard_Parallel_2",35.23333333333333],PARAMETER["Latitude_Of_Origin",33.33333333333334],UNIT["Foot_US",0.3048006096012192]]"#;

    #[test]
    fn a_state_plane_prj_reads_its_projection_unit_and_ellipsoid() {
        let p = from_wkt(OK_SOUTH_FT).unwrap();
        assert!((p.unit - 0.304_800_609_601_219_2).abs() < 1e-15);
        assert!(
            (p.false_easting - 600_000.0).abs() < 0.01,
            "1,968,500 ft is 600 km"
        );
        assert!(matches!(p.kind, Kind::LambertConformalConic { .. }));
        // The zone's origin maps back to itself, and Norman lands in Oklahoma.
        let origin = p.inverse(1_968_500.0, 0.0);
        assert!(close(origin, [-98.0, 33.333_333], 1e-6), "{origin:?}");
        let xy = p.forward(-97.44, 35.22);
        let back = p.inverse(xy[0], xy[1]);
        assert!(close(back, [-97.44, 35.22], 1e-8), "{back:?}");
    }

    #[test]
    fn a_utm_prj_and_an_albers_prj_read_too() {
        let utm = r#"PROJCS["NAD_1983_UTM_Zone_14N",GEOGCS["GCS_North_American_1983",DATUM["D_North_American_1983",SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["False_Easting",500000.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",-99.0],PARAMETER["Scale_Factor",0.9996],PARAMETER["Latitude_Of_Origin",0.0],UNIT["Meter",1.0]]"#;
        let Some(Epsg::Projected(want)) = from_epsg(26914) else {
            panic!()
        };
        assert_eq!(from_wkt(utm).unwrap(), want);
        let albers = r#"PROJCS["NAD_1983_Contiguous_USA_Albers",GEOGCS["GCS_North_American_1983",DATUM["D_North_American_1983",SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Albers"],PARAMETER["False_Easting",0.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",-96.0],PARAMETER["Standard_Parallel_1",29.5],PARAMETER["Standard_Parallel_2",45.5],PARAMETER["Latitude_Of_Origin",23.0],UNIT["Meter",1.0]]"#;
        let Some(Epsg::Projected(want)) = from_epsg(5070) else {
            panic!()
        };
        let got = from_wkt(albers).unwrap();
        assert!(close(
            got.inverse(1000.0, 1.4e6),
            want.inverse(1000.0, 1.4e6),
            1e-9
        ));
    }

    #[test]
    fn an_old_datum_or_an_unknown_projection_is_refused_by_name() {
        let nad27 = OK_SOUTH_FT
            .replace("North_American_1983", "North_American_1927")
            .replace("NAD_1983", "NAD_1927");
        assert!(from_wkt(&nad27)
            .unwrap_err()
            .to_string()
            .contains("older datum"));
        let polar = OK_SOUTH_FT.replace("Lambert_Conformal_Conic", "Polar_Stereographic");
        let err = from_wkt(&polar).unwrap_err().to_string();
        assert!(err.contains("polar_stereographic"), "{err}");
    }
}
