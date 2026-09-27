//! Point soundings from HRRR pressure-level analysis: fetch TMP/DPT/UGRD/VGRD at a curated set
//! of mandatory levels via `.idx` byte-range requests, sample the nearest grid point, and return
//! a vertical profile for a Skew-T / hodograph. Reuses the HRRR bucket + gribberish decode.

use crate::alerts::USER_AGENT;
use chrono::{DateTime, Datelike, Timelike, Utc};
use futures_util::StreamExt;

const BUCKET: &str = "https://noaa-hrrr-bdp-pds.s3.amazonaws.com";

/// Mandatory pressure levels (hPa), surface-up. Kept small so a click is ~48 range fetches. The
/// top two are above the tropopause on purpose: an uncapped plains parcel is still buoyant at
/// 200 hPa, and without an equilibrium level the effective-layer shear and STP have nothing to
/// measure against and come back absent.
const LEVELS_HPA: &[u32] = &[1000, 925, 850, 700, 600, 500, 400, 300, 250, 200, 150, 100];

/// One level of the sounding.
#[derive(Debug, Clone, Copy)]
pub struct SoundingLevel {
    pub pressure_hpa: f64,
    pub temp_c: f64,
    pub dewpt_c: f64,
    pub u_ms: f64,
    pub v_ms: f64,
}

/// A lifted surface parcel: the energy numbers, the three level heights, and the trace itself.
#[derive(Debug, Clone)]
pub struct Parcel {
    /// J/kg, positive.
    pub cape: f64,
    /// J/kg, negative (or zero) — the cap the parcel has to break through.
    pub cin: f64,
    pub lcl_m: f64,
    /// `None` when the parcel is never buoyant.
    pub lfc_m: Option<f64>,
    /// `None` when the parcel is still buoyant at the top of the profile.
    pub el_m: Option<f64>,
    /// Parcel temperature (°C) at each of the sounding's levels, in the same order.
    pub trace_c: Vec<f64>,
}

/// Which air a parcel is lifted from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParcelKind {
    /// The surface level as it is.
    #[default]
    SurfaceBased,
    /// The lowest 100 hPa mixed: mean potential temperature and mean mixing ratio, lifted from
    /// the surface — what a well-mixed afternoon boundary layer actually feeds a storm.
    MixedLayer,
    /// The level in the lowest 300 hPa whose parcel has the most CAPE — the elevated
    /// instability over a stable surface layer that surface-based numbers miss.
    MostUnstable,
}

impl ParcelKind {
    pub const ALL: [ParcelKind; 3] = [Self::SurfaceBased, Self::MixedLayer, Self::MostUnstable];

    /// The usual prefix: SB, ML, MU.
    pub fn short(self) -> &'static str {
        match self {
            Self::SurfaceBased => "SB",
            Self::MixedLayer => "ML",
            Self::MostUnstable => "MU",
        }
    }
}

/// A vertical profile at a point.
pub struct Sounding {
    pub lon: f64,
    pub lat: f64,
    pub run: DateTime<Utc>,
    /// Forecast hour this profile is valid at (0 = the analysis).
    pub fh: u8,
    /// Levels ordered surface (highest pressure) first.
    pub levels: Vec<SoundingLevel>,
}

impl Sounding {
    /// Bulk wind shear magnitude (knots) between the lowest and ~500 hPa (≈ 0–6 km) levels.
    pub fn bulk_shear_kt(&self) -> Option<f64> {
        let sfc = self.levels.first()?;
        let top = self.levels.iter().find(|l| l.pressure_hpa <= 500.0)?;
        let (du, dv) = (top.u_ms - sfc.u_ms, top.v_ms - sfc.v_ms);
        Some((du * du + dv * dv).sqrt() * 1.943_844)
    }
}

/// Severe-weather composite indices derived from the profile (feature FF). These are the
/// *fixed-layer* published forms, computed from the 10 mandatory levels. The effective-layer
/// forms live beside them in [`crate::severe::effective_indices`], solved off the same profile —
/// still coarse at 10 levels, but the same method the gridded layers use.
#[derive(Debug, Clone, Copy)]
pub struct Indices {
    /// Surface-based CAPE (J/kg).
    pub sbcape: f64,
    /// LCL height (m AGL) of the surface parcel.
    pub lcl_m: f64,
    /// 0–1 km storm-relative helicity (m²/s², Bunkers right-mover motion).
    pub srh1: f64,
    /// 0–3 km storm-relative helicity (m²/s²).
    pub srh3: f64,
    /// 0–6 km bulk shear (kt).
    pub shear6_kt: f64,
    /// Supercell composite parameter (Thompson 2004 fixed-layer form).
    pub scp: f64,
    /// Significant tornado parameter (fixed-layer form).
    pub stp: f64,
    /// 0–1 km energy-helicity index.
    pub ehi1: f64,
}

const RD: f64 = 287.04; // J/(kg·K)
const G: f64 = 9.80665;
const KAPPA: f64 = 0.2854;

/// Saturation vapor pressure (hPa) over water — Bolton (1980).
///
/// Clamped at -120 °C: the formula's denominator goes through zero at -243.5 °C, and a model's
/// "no moisture here" dewpoint (HRRR writes 0 K at 100 hPa) must read as no vapour, not infinity.
fn e_sat_hpa(t_c: f64) -> f64 {
    let t_c = t_c.max(-120.0);
    6.112 * ((17.67 * t_c) / (t_c + 243.5)).exp()
}

/// Parcel temperature (K) after pseudoadiabatic ascent from `(p0, t0k)` to `p1` (hPa, p1 < p0),
/// stepped in small pressure increments.
fn moist_ascent_k(p0: f64, t0k: f64, p1: f64) -> f64 {
    moist_adiabat_k(p0, t0k, p1)
}

/// Temperature (K) along the pseudoadiabat through `(p0, t0k)` at `p1` (hPa), up or down: a
/// rising saturated parcel, or a descending one kept saturated by evaporation (a downdraft).
fn moist_adiabat_k(p0: f64, t0k: f64, p1: f64) -> f64 {
    let mut t = t0k;
    let mut p = p0;
    const LV: f64 = 2.501e6;
    const CPD: f64 = 1005.7;
    const EPS: f64 = 0.622;
    // dT/dp (K/hPa) along the pseudoadiabat at (p, t).
    let slope = |p: f64, t: f64| {
        let tc = t - 273.15;
        let rs = EPS * e_sat_hpa(tc) / (p - e_sat_hpa(tc)).max(1.0);
        (1.0 / p) * (RD * t + LV * rs) / (CPD + LV * LV * rs * EPS / (RD * t * t))
    };
    // Midpoint steps: a parcel taken up and brought back down lands where it started, which a
    // plain Euler step does not quite manage.
    while (p - p1).abs() > 1e-9 {
        let dp = (p1 - p).clamp(-5.0, 5.0);
        let k1 = slope(p, t);
        let k2 = slope(p + dp / 2.0, t + k1 * dp / 2.0);
        t += k2 * dp;
        p += dp;
    }
    t
}

/// Bolton's (1980) LCL of air at `t_c`/`td_c`: its temperature (K) and pressure (hPa).
fn lcl(p_hpa: f64, t_c: f64, td_c: f64) -> (f64, f64) {
    let tk = t_c + 273.15;
    let e = e_sat_hpa(td_c).max(1e-3);
    let t_lcl = 2840.0 / (3.5 * tk.ln() - e.ln() - 4.805) + 55.0;
    (t_lcl, p_hpa * (t_lcl / tk).powf(1.0 / KAPPA))
}

/// Wet-bulb temperature (K) by Normand's rule: lift to the LCL, come back down the moist
/// adiabat to the starting pressure.
fn wet_bulb_k(p_hpa: f64, t_c: f64, td_c: f64) -> f64 {
    let (t_lcl, p_lcl) = lcl(p_hpa, t_c, td_c);
    moist_adiabat_k(p_lcl, t_lcl, p_hpa)
}

/// Equivalent potential temperature (K), Bolton (1980) eq. 43 — for ranking levels only.
fn theta_e_k(p_hpa: f64, t_c: f64, td_c: f64) -> f64 {
    let tk = t_c + 273.15;
    let e = e_sat_hpa(td_c);
    let r = 0.622 * e / (p_hpa - e).max(1.0);
    let (t_lcl, _) = lcl(p_hpa, t_c, td_c);
    tk * (1000.0 / p_hpa).powf(0.2854 * (1.0 - 0.28 * r))
        * ((3.376 / t_lcl - 0.00254) * r * 1000.0 * (1.0 + 0.81 * r)).exp()
}

impl Sounding {
    /// Geometric height (m AGL) of each level via hypsometric integration.
    /// `// ponytail: dry temperature stands in for virtual temperature (~1% height error).`
    pub fn heights_m(&self) -> Vec<f64> {
        let mut h = Vec::with_capacity(self.levels.len());
        let mut z = 0.0;
        for i in 0..self.levels.len() {
            if i > 0 {
                let (a, b) = (&self.levels[i - 1], &self.levels[i]);
                let t_mean = (a.temp_c + b.temp_c) / 2.0 + 273.15;
                z += RD * t_mean / G * (a.pressure_hpa / b.pressure_hpa).ln();
            }
            h.push(z);
        }
        h
    }

    /// Height (m AGL) where the profile cools through `t_c` on its way up, linear in height
    /// between the two levels that bracket it — the 0 °C and −20 °C levels the hail algorithm
    /// (`crate::derived::hail`) weights by. The *highest* crossing, not the first: a shallow cold
    /// layer near the ground can dip under freezing and back, and the melting level that matters
    /// to falling hail is the one above it. A profile already at or below `t_c` at the surface
    /// with no warm layer above is `0.0`; one warmer than `t_c` all the way up is `None`.
    pub fn isotherm_height_m(&self, t_c: f64) -> Option<f64> {
        let h = self.heights_m();
        let mut found = None;
        for i in 1..self.levels.len() {
            let (a, b) = (self.levels[i - 1].temp_c, self.levels[i].temp_c);
            if a > t_c && b <= t_c {
                let k = (a - t_c) / (a - b);
                found = Some(h[i - 1] + (h[i] - h[i - 1]) * k);
            }
        }
        found.or_else(|| self.levels.first().filter(|l| l.temp_c <= t_c).map(|_| 0.0))
    }

    /// Surface-based CAPE (J/kg) and LCL height (m AGL) via a stepped pseudoadiabatic parcel.
    pub fn sb_parcel(&self) -> Option<(f64, f64)> {
        let p = self.parcel()?;
        Some((p.cape, p.lcl_m))
    }

    /// Full surface-based parcel: CAPE, CIN, and the LCL/LFC/EL heights, plus the parcel's own
    /// temperature at each level so the Skew-T can draw the trace the numbers came from.
    pub fn parcel(&self) -> Option<Parcel> {
        self.parcel_from(0)
    }

    /// The same lift, starting from level `i` instead of the surface. This is what an effective
    /// inflow layer is made of: a parcel from each candidate level, kept when it has enough CAPE
    /// and little enough CIN.
    pub fn parcel_from(&self, i: usize) -> Option<Parcel> {
        let l = self.levels.get(i)?;
        self.lift_from(i, l.temp_c, l.dewpt_c)
    }

    /// Lift air of temperature `t_c` and dewpoint `td_c` from level `i`'s pressure, through the
    /// real environment above it.
    fn lift_from(&self, i: usize, t_c: f64, td_c: f64) -> Option<Parcel> {
        let sfc = self.levels.get(i)?;
        if self.levels.len() - i < 3 {
            return None;
        }
        let tk = t_c + 273.15;
        // Bolton (1980) LCL temperature from T and the vapor pressure at Td.
        let e = e_sat_hpa(td_c).max(1e-3);
        let t_lcl = 2840.0 / (3.5 * tk.ln() - e.ln() - 4.805) + 55.0;
        let p_lcl = sfc.pressure_hpa * (t_lcl / tk).powf(1.0 / KAPPA);
        let lcl_m = RD * (tk + t_lcl) / 2.0 / G * (sfc.pressure_hpa / p_lcl).ln();

        // Parcel temperature at each level: dry below the LCL, pseudoadiabatic above.
        let parcel_k = |p: f64| -> f64 {
            if p >= p_lcl {
                tk * (p / sfc.pressure_hpa).powf(KAPPA)
            } else {
                moist_ascent_k(p_lcl, t_lcl, p)
            }
        };
        let heights = self.heights_m();
        // Trapezoidal CAPE over positive-buoyancy layers: Rd Σ (Tp−Te) Δln p. CIN is the same sum
        // over the negative ones, but only up to the LFC — negative area above the EL is not what
        // holds a parcel down.
        let (mut cape, mut cin) = (0.0, 0.0);
        let (mut lfc_m, mut el_m) = (None, None);
        for (k, w) in self.levels[i..].windows(2).enumerate() {
            let (lo, hi) = (&w[0], &w[1]);
            let b_lo = parcel_k(lo.pressure_hpa) - (lo.temp_c + 273.15);
            let b_hi = parcel_k(hi.pressure_hpa) - (hi.temp_c + 273.15);
            let dlnp = (lo.pressure_hpa / hi.pressure_hpa).ln();
            cape += RD * (b_lo.max(0.0) + b_hi.max(0.0)) / 2.0 * dlnp;
            if lfc_m.is_none() {
                cin += RD * (b_lo.min(0.0) + b_hi.min(0.0)) / 2.0 * dlnp;
            }
            // Buoyancy crossings, linearly interpolated in height across the layer.
            let (h_lo, h_hi) = (heights[i + k], heights[i + k + 1]);
            if b_lo <= 0.0 && b_hi > 0.0 {
                let k = (-b_lo / (b_hi - b_lo)).clamp(0.0, 1.0);
                lfc_m.get_or_insert(h_lo + (h_hi - h_lo) * k);
            }
            if b_lo > 0.0 && b_hi <= 0.0 && lfc_m.is_some() {
                let k = (b_lo / (b_lo - b_hi)).clamp(0.0, 1.0);
                el_m = Some(h_lo + (h_hi - h_lo) * k);
            }
        }
        // A parcel buoyant right off the surface has its LFC there.
        if lfc_m.is_none() && cape > 0.0 {
            lfc_m = Some(0.0);
        }
        let trace_c = self
            .levels
            .iter()
            .map(|l| parcel_k(l.pressure_hpa) - 273.15)
            .collect();
        Some(Parcel {
            cape,
            cin,
            lcl_m,
            lfc_m,
            el_m,
            trace_c,
        })
    }

    /// The parcel of `kind` and the index of the level it starts from (its trace below that
    /// level means nothing and is not drawn).
    pub fn parcel_of(&self, kind: ParcelKind) -> Option<(Parcel, usize)> {
        match kind {
            ParcelKind::SurfaceBased => Some((self.parcel()?, 0)),
            ParcelKind::MixedLayer => {
                let sfc = self.levels.first()?;
                let layer: Vec<&SoundingLevel> = self
                    .levels
                    .iter()
                    .filter(|l| l.pressure_hpa >= sfc.pressure_hpa - 100.0)
                    .collect();
                let n = layer.len() as f64;
                let theta = layer
                    .iter()
                    .map(|l| (l.temp_c + 273.15) * (1000.0 / l.pressure_hpa).powf(KAPPA))
                    .sum::<f64>()
                    / n;
                let r = layer
                    .iter()
                    .map(|l| {
                        let e = e_sat_hpa(l.dewpt_c);
                        0.622 * e / (l.pressure_hpa - e).max(1.0)
                    })
                    .sum::<f64>()
                    / n;
                // The mixed air brought to the surface pressure: its temperature from the mean
                // theta, its dewpoint from the mean mixing ratio (vapour pressure inverted
                // through Bolton's formula).
                let p0 = sfc.pressure_hpa;
                let t_c = theta * (p0 / 1000.0).powf(KAPPA) - 273.15;
                let e = r * p0 / (0.622 + r);
                let ln = (e / 6.112).ln();
                let td_c = (243.5 * ln / (17.67 - ln)).min(t_c);
                Some((self.lift_from(0, t_c, td_c)?, 0))
            }
            ParcelKind::MostUnstable => {
                let sfc = self.levels.first()?.pressure_hpa;
                (0..self.levels.len())
                    .filter(|&i| self.levels[i].pressure_hpa >= sfc - 300.0)
                    .filter_map(|i| Some((self.parcel_from(i)?, i)))
                    .max_by(|a, b| a.0.cape.total_cmp(&b.0.cape))
            }
        }
    }

    /// Wind (u, v) linearly interpolated to height `z_m` AGL.
    fn wind_at(&self, heights: &[f64], z_m: f64) -> Option<(f64, f64)> {
        let i = heights.iter().position(|&h| h >= z_m)?;
        if i == 0 {
            let l = self.levels.first()?;
            return Some((l.u_ms, l.v_ms));
        }
        let (h0, h1) = (heights[i - 1], heights[i]);
        let (a, b) = (&self.levels[i - 1], &self.levels[i]);
        let k = ((z_m - h0) / (h1 - h0).max(1e-6)).clamp(0.0, 1.0);
        Some((
            a.u_ms + (b.u_ms - a.u_ms) * k,
            a.v_ms + (b.v_ms - a.v_ms) * k,
        ))
    }

    /// Wind (u, v, m/s) at `z_m` above ground, interpolated between levels; `None` above the top.
    pub fn wind_at_height(&self, z_m: f64) -> Option<(f64, f64)> {
        self.wind_at(&self.heights_m(), z_m)
    }

    /// Bunkers left-mover storm motion: the right mover's mirror across the 0–6 km mean wind.
    pub fn bunkers_lm(&self) -> Option<(f64, f64)> {
        let (ru, rv) = self.bunkers_rm()?;
        let (mu, mv) = self.mean_wind_0_6()?;
        Some((2.0 * mu - ru, 2.0 * mv - rv))
    }

    /// The 0–6 km mean wind, sampled every 500 m.
    fn mean_wind_0_6(&self) -> Option<(f64, f64)> {
        let h = self.heights_m();
        let (mut mu, mut mv, mut n) = (0.0, 0.0, 0);
        for z in (0..=6000).step_by(500) {
            if let Some((u, v)) = self.wind_at(&h, z as f64) {
                mu += u;
                mv += v;
                n += 1;
            }
        }
        (n > 0).then(|| (mu / n as f64, mv / n as f64))
    }

    /// Precipitable water (mm): the column's water vapour, `∫ q dp / g` from the surface up.
    pub fn pwat_mm(&self) -> Option<f64> {
        if self.levels.len() < 2 {
            return None;
        }
        let q = |l: &SoundingLevel| {
            let e = e_sat_hpa(l.dewpt_c);
            0.622 * e / (l.pressure_hpa - 0.378 * e).max(1.0)
        };
        let total: f64 = self
            .levels
            .windows(2)
            .map(|w| (q(&w[0]) + q(&w[1])) / 2.0 * (w[0].pressure_hpa - w[1].pressure_hpa) * 100.0)
            .sum();
        // kg/m² of water is mm of depth.
        Some(total / G)
    }

    /// Downdraft CAPE (J/kg): a parcel from the level of lowest equivalent potential temperature
    /// in the lowest 400 hPa, at its wet-bulb temperature, brought down the moist adiabat to the
    /// surface; the energy is how much colder than the air it stays on the way (SHARPpy's
    /// method, from the mandatory levels). Large values mean strong evaporatively driven
    /// downdrafts and outflow.
    pub fn dcape(&self) -> Option<f64> {
        let sfc = self.levels.first()?;
        let top = sfc.pressure_hpa - 400.0;
        let (start, lvl) = self
            .levels
            .iter()
            .enumerate()
            .filter(|(_, l)| l.pressure_hpa >= top)
            .min_by(|a, b| {
                let ta = theta_e_k(a.1.pressure_hpa, a.1.temp_c, a.1.dewpt_c);
                let tb = theta_e_k(b.1.pressure_hpa, b.1.temp_c, b.1.dewpt_c);
                ta.total_cmp(&tb)
            })?;
        if start == 0 {
            return Some(0.0);
        }
        let t0 = wet_bulb_k(lvl.pressure_hpa, lvl.temp_c, lvl.dewpt_c);
        let parcel = |p: f64| moist_adiabat_k(lvl.pressure_hpa, t0, p);
        let mut total = 0.0;
        for w in self.levels[..=start].windows(2) {
            let (lo, hi) = (&w[0], &w[1]);
            let b_lo = (lo.temp_c + 273.15) - parcel(lo.pressure_hpa);
            let b_hi = (hi.temp_c + 273.15) - parcel(hi.pressure_hpa);
            total += RD * (b_lo.max(0.0) + b_hi.max(0.0)) / 2.0
                * (lo.pressure_hpa / hi.pressure_hpa).ln();
        }
        Some(total)
    }

    /// Environmental lapse rate (°C/km) between two heights above ground.
    pub fn lapse_rate_c_km(&self, z0_m: f64, z1_m: f64) -> Option<f64> {
        let h = self.heights_m();
        let temp_at = |z: f64| {
            let i = h.iter().position(|&hz| hz >= z)?;
            if i == 0 {
                return self.levels.first().map(|l| l.temp_c);
            }
            let k = (z - h[i - 1]) / (h[i] - h[i - 1]).max(1e-6);
            let (a, b) = (&self.levels[i - 1], &self.levels[i]);
            Some(a.temp_c + (b.temp_c - a.temp_c) * k)
        };
        Some((temp_at(z0_m)? - temp_at(z1_m)?) / ((z1_m - z0_m) / 1000.0))
    }

    /// Environmental lapse rate (°C/km) between two pressure levels in the profile, such as the
    /// 700–500 hPa mid-level rate.
    pub fn lapse_rate_between_c_km(&self, p0_hpa: f64, p1_hpa: f64) -> Option<f64> {
        let h = self.heights_m();
        let find = |p: f64| {
            let i = self
                .levels
                .iter()
                .position(|l| (l.pressure_hpa - p).abs() < 0.5)?;
            Some((self.levels[i].temp_c, h[i]))
        };
        let ((t0, z0), (t1, z1)) = (find(p0_hpa)?, find(p1_hpa)?);
        (z1 > z0).then(|| (t0 - t1) / ((z1 - z0) / 1000.0))
    }

    /// Bunkers right-mover storm motion: 0–6 km mean wind plus 7.5 m/s at right angles to the
    /// 0–6 km shear vector.
    pub fn bunkers_rm(&self) -> Option<(f64, f64)> {
        let h = self.heights_m();
        let (mut mu, mut mv, mut n) = (0.0, 0.0, 0);
        for z in (0..=6000).step_by(500) {
            if let Some((u, v)) = self.wind_at(&h, z as f64) {
                mu += u;
                mv += v;
                n += 1;
            }
        }
        if n == 0 {
            return None;
        }
        let (mu, mv) = (mu / n as f64, mv / n as f64);
        let (u0, v0) = self.wind_at(&h, 0.0)?;
        let (u6, v6) = self.wind_at(&h, 6000.0)?;
        let (su, sv) = (u6 - u0, v6 - v0);
        let mag = (su * su + sv * sv).sqrt().max(1e-6);
        // Right of the shear vector: rotate −90° → (sv, −su).
        Some((mu + 7.5 * sv / mag, mv - 7.5 * su / mag))
    }

    /// Storm-relative helicity (m²/s²) over 0..`depth_m`, relative to the Bunkers right mover.
    pub fn srh(&self, depth_m: f64) -> Option<f64> {
        self.srh_relative(depth_m, self.bunkers_rm()?)
    }

    /// Storm-relative helicity (m²/s²) over 0..`depth_m` for a given storm motion (u, v, m/s):
    /// what an analyst's own motion estimate makes of the same hodograph.
    pub fn srh_relative(&self, depth_m: f64, motion: (f64, f64)) -> Option<f64> {
        let h = self.heights_m();
        let (cu, cv) = motion;
        let mut total = 0.0;
        let step = 250.0;
        let mut z = 0.0;
        while z + step <= depth_m + 1e-6 {
            let (u0, v0) = self.wind_at(&h, z)?;
            let (u1, v1) = self.wind_at(&h, z + step)?;
            total += (u1 - cu) * (v0 - cv) - (u0 - cu) * (v1 - cv);
            z += step;
        }
        Some(total)
    }

    /// All fixed-layer composite indices, or `None` when the profile is too short.
    pub fn indices(&self) -> Option<Indices> {
        let (sbcape, lcl_m) = self.sb_parcel()?;
        let srh1 = self.srh(1000.0)?;
        let srh3 = self.srh(3000.0)?;
        let shear6_kt = self.bulk_shear_kt()?;
        let shear6_ms = shear6_kt / 1.943_844;
        // The same functions the gridded layers use. The point and the grid used to carry two
        // copies of these constants and a comment asking the next reader to keep them in step.
        let scp = crate::severe::scp(sbcape, srh3, shear6_ms);
        let stp = crate::severe::stp(sbcape, srh1, shear6_ms, lcl_m);
        let ehi1 = crate::severe::ehi1(sbcape, srh1);
        Some(Indices {
            sbcape,
            lcl_m,
            srh1,
            srh3,
            shear6_kt,
            scp,
            stp,
            ehi1,
        })
    }
}

impl Sounding {
    /// The profile as CSV: the derived indices first, then a blank line, then one row per level.
    /// Two blocks in one file because that is what the numbers are — a summary and the profile it
    /// came from — and a spreadsheet opens it either way.
    pub fn to_csv(&self) -> String {
        let mut s = String::from("index,value\n");
        let mut put = |name: &str, v: String| {
            s.push_str(&format!("{name},{v}\n"));
        };
        if let Some(ix) = self.indices() {
            let p = self.parcel();
            put("sbcape_jkg", format!("{:.0}", ix.sbcape));
            if let Some(p) = p.as_ref() {
                put("sbcin_jkg", format!("{:.0}", p.cin));
            }
            put("lcl_m", format!("{:.0}", ix.lcl_m));
            for (name, v) in [
                ("lfc_m", p.as_ref().and_then(|p| p.lfc_m)),
                ("el_m", p.as_ref().and_then(|p| p.el_m)),
            ] {
                put(name, v.map_or(String::new(), |v| format!("{v:.0}")));
            }
            put("srh_0_1_m2s2", format!("{:.0}", ix.srh1));
            put("srh_0_3_m2s2", format!("{:.0}", ix.srh3));
            put("shear_0_6_kt", format!("{:.0}", ix.shear6_kt));
            put("scp", format!("{:.2}", ix.scp));
            put("stp", format!("{:.2}", ix.stp));
            put("ehi_0_1", format!("{:.2}", ix.ehi1));
        }
        let opt =
            |v: Option<f64>, digits: usize| v.map_or(String::new(), |v| format!("{v:.digits$}"));
        put("pwat_mm", opt(self.pwat_mm(), 1));
        put("dcape_jkg", opt(self.dcape(), 0));
        put(
            "lapse_0_3km_c_km",
            opt(self.lapse_rate_c_km(0.0, 3000.0), 1),
        );
        put(
            "lapse_700_500_c_km",
            opt(self.lapse_rate_between_c_km(700.0, 500.0), 1),
        );
        for t in [0, -10, -20, -30] {
            put(
                &format!("height_{}c_m", t.to_string().replace('-', "m")),
                opt(self.isotherm_height_m(t as f64), 0),
            );
        }
        if let Some((u, v)) = self.bunkers_rm() {
            put("bunkers_rm_u_ms", format!("{u:.1}"));
            put("bunkers_rm_v_ms", format!("{v:.1}"));
        }
        // Empty where the column has no effective inflow layer, or no equilibrium level to
        // measure the shear layer against — the same absence the panel shows as an em dash.
        let eff = crate::severe::effective_indices(self);
        put(
            "esrh_m2s2",
            eff.map_or(String::new(), |e| format!("{:.0}", e.esrh)),
        );
        put(
            "ebwd_kt",
            eff.and_then(|e| e.ebwd_kt)
                .map_or(String::new(), |v| format!("{v:.0}")),
        );
        put(
            "stp_effective",
            eff.and_then(|e| e.stp_eff)
                .map_or(String::new(), |v| format!("{v:.2}")),
        );

        s.push_str("\npressure_hpa,height_m,temp_c,dewpt_c,u_ms,v_ms\n");
        let heights = self.heights_m();
        for (i, l) in self.levels.iter().enumerate() {
            s.push_str(&format!(
                "{:.0},{:.0},{:.1},{:.1},{:.1},{:.1}\n",
                l.pressure_hpa, heights[i], l.temp_c, l.dewpt_c, l.u_ms, l.v_ms
            ));
        }
        s
    }
}

/// How many of the 40 range requests are in flight at once. The same trade the gridded HRRR
/// fetch makes: enough to hide latency, not enough to look like a scraper.
const SOUNDING_CONCURRENCY: usize = 8;

/// Which model a point sounding is read from. Each is its pressure-level GRIB2 file on NOAA's
/// open-data buckets, sampled at the nearest grid point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SoundingModel {
    /// HRRR `wrfprs`: 3 km, hourly cycles, 18 h (48 h at 00/06/12/18Z).
    #[default]
    Hrrr,
    /// RAP `awp130pgrb`: 13 km, hourly cycles, 21 h (51 h at 03/09/15/21Z). Moisture is RH, and
    /// U and V share one GRIB message (read apart by [`crate::grib_split`]).
    Rap,
    /// NAM 3 km CONUS nest: its own dynamical core, 6-hourly cycles to 60 h.
    NamNest,
}

impl SoundingModel {
    pub const ALL: [SoundingModel; 3] = [Self::Hrrr, Self::Rap, Self::NamNest];

    /// Whether this build can decode the model's files: RAP's are JPEG 2000 packed, and the web
    /// build leaves that codec out (see wxdata's Cargo.toml), so a browser cannot read them.
    pub fn available(self) -> bool {
        !(cfg!(target_arch = "wasm32") && self == Self::Rap)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Hrrr => "HRRR",
            Self::Rap => "RAP",
            Self::NamNest => "NAM 3 km",
        }
    }

    fn url(self, run: DateTime<Utc>, fh: u8) -> String {
        let date = format!("{:04}{:02}{:02}", run.year(), run.month(), run.day());
        let h = run.hour();
        match self {
            Self::Hrrr => format!("{BUCKET}/hrrr.{date}/conus/hrrr.t{h:02}z.wrfprsf{fh:02}.grib2"),
            Self::Rap => format!(
                "https://noaa-rap-pds.s3.amazonaws.com/rap.{date}/rap.t{h:02}z.awp130pgrbf{fh:02}.grib2"
            ),
            Self::NamNest => format!(
                "https://noaa-nam-pds.s3.amazonaws.com/nam.{date}/nam.t{h:02}z.conusnest.hiresf{fh:02}.tm00.grib2"
            ),
        }
    }

    /// Hours between cycles.
    fn cycle_hours(self) -> u32 {
        match self {
            Self::NamNest => 6,
            _ => 1,
        }
    }

    /// The longest forecast hour a run from `run` publishes.
    pub fn max_fh(self, run: DateTime<Utc>) -> u32 {
        match self {
            Self::Hrrr => {
                if run.hour().is_multiple_of(6) {
                    48
                } else {
                    18
                }
            }
            Self::Rap => {
                if run.hour() % 6 == 3 {
                    51
                } else {
                    21
                }
            }
            Self::NamNest => 60,
        }
    }

    fn domain(self) -> crate::model::Domain {
        match self {
            Self::Hrrr => crate::hrrr::Model::HrrrPressure.def().domain,
            Self::Rap => crate::hrrr::Model::Rap.def().domain,
            Self::NamNest => crate::hrrr::Model::NamNest.def().domain,
        }
    }
}

/// Fetch a sounding at `(lon, lat)` from the most recent HRRR pressure-level analysis (f00).
pub async fn fetch(http: &reqwest::Client, lon: f64, lat: f64) -> anyhow::Result<Sounding> {
    fetch_at(http, lon, lat, 0).await
}

/// Fetch a sounding valid `fh` hours into the most recent usable HRRR run.
pub async fn fetch_at(
    http: &reqwest::Client,
    lon: f64,
    lat: f64,
    fh: u8,
) -> anyhow::Result<Sounding> {
    fetch_model_at(http, SoundingModel::Hrrr, lon, lat, fh).await
}

/// Fetch a sounding valid `fh` hours into the most recent usable run of `model`, clamped to what
/// that run publishes.
pub async fn fetch_model_at(
    http: &reqwest::Client,
    model: SoundingModel,
    lon: f64,
    lat: f64,
    fh: u8,
) -> anyhow::Result<Sounding> {
    anyhow::ensure!(
        model.available(),
        "{} soundings need the desktop app: the browser build cannot decode its JPEG 2000 files",
        model.label()
    );
    anyhow::ensure!(
        model.domain().contains(lon, lat),
        "{} sounding location {lat:.3}, {lon:.3} is outside the published model domain",
        model.label()
    );
    let step = model.cycle_hours();
    let now = Utc::now();
    let newest = (now - chrono::Duration::hours(1))
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap()
        .with_nanosecond(0)
        .unwrap();
    let newest = newest - chrono::Duration::hours((newest.hour() % step) as i64);
    let mut last_err = None;
    // Six cycles back: six hours for the hourly models, a day and a half for the NAM.
    for back in 0..6 {
        let run = newest - chrono::Duration::hours((back * step) as i64);
        let fh = (fh as u32).min(model.max_fh(run)) as u8;
        match fetch_run(http, model, run, lon, lat, fh).await {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no {} run found", model.label())))
}

/// The same point at the same valid time from the HRRR run an hour before `current`'s.
pub async fn fetch_previous_run(
    http: &reqwest::Client,
    current: &Sounding,
) -> anyhow::Result<Sounding> {
    fetch_previous_model_run(http, SoundingModel::Hrrr, current).await
}

/// The same point at the same valid time from `model`'s previous cycle (an hour earlier for the
/// hourly models, six for the NAM), for seeing how the forecast changed between cycles. The
/// earlier run's forecast hour is longer by the gap; a run that does not reach that far is an
/// error, not a quietly different valid time.
pub async fn fetch_previous_model_run(
    http: &reqwest::Client,
    model: SoundingModel,
    current: &Sounding,
) -> anyhow::Result<Sounding> {
    let step = model.cycle_hours();
    let run = current.run - chrono::Duration::hours(step as i64);
    let fh = current.fh as u32 + step;
    let max_fh = model.max_fh(run);
    anyhow::ensure!(
        fh <= max_fh,
        "the {}Z run stops at f{max_fh}, short of this valid time",
        run.format("%H")
    );
    fetch_run(http, model, run, current.lon, current.lat, fh as u8).await
}

/// Dewpoint (°C) from temperature (°C) and relative humidity (%), inverting Bolton's saturation
/// vapour pressure — for models that publish RH on pressure levels rather than dewpoint.
fn dewpoint_from_rh(t_c: f64, rh: f64) -> f64 {
    let e = e_sat_hpa(t_c) * (rh.clamp(0.5, 100.0) / 100.0);
    let ln = (e / 6.112).ln();
    (243.5 * ln / (17.67 - ln)).min(t_c)
}

/// Where a variable sits in the `.idx`: its message's byte range and which field of that message
/// it is (U and V share one message in the RAP and NAM files).
fn locate(idx: &str, var: &str, level: &str) -> Option<(u64, Option<u64>, usize)> {
    let lines: Vec<&str> = idx.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() < 5 || f[3] != var || f[4] != level {
            continue;
        }
        let start: u64 = f[1].parse().ok()?;
        let end = lines[i + 1..]
            .iter()
            .filter_map(|n| n.split(':').nth(1))
            .filter_map(|s| s.parse::<u64>().ok())
            .find(|&o| o > start);
        return Some((start, end, crate::grib_split::subfield_of(f[0])));
    }
    None
}

async fn fetch_run(
    http: &reqwest::Client,
    model: SoundingModel,
    run: DateTime<Utc>,
    lon: f64,
    lat: f64,
    fh: u8,
) -> anyhow::Result<Sounding> {
    let base = model.url(run, fh);
    let idx = http
        .get(crate::net::fetch_url(&format!("{base}.idx")))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // Fetch all (var, level) messages, several at a time. Awaiting them one by one made these
    // forty range requests serial in everything but name — a click cost forty round trips.
    // Slot index rather than the variable name: a `&'static str` in the job tuple makes the
    // stream's closure higher-ranked over a lifetime and the whole future stops being `Send`.
    // Slot 1 is moisture: dewpoint where the file has it, else relative humidity (RAP).
    const VARS: [&str; 4] = ["TMP", "DPT", "UGRD", "VGRD"];
    let mut jobs: Vec<(u32, usize, u64, Option<u64>, usize, bool)> = Vec::new();
    for &hpa in LEVELS_HPA {
        let level = format!("{hpa} mb");
        for (i, var) in VARS.iter().enumerate() {
            let (found, is_rh) = match locate(&idx, var, &level) {
                Some(r) => (Some(r), false),
                None if *var == "DPT" => (locate(&idx, "RH", &level), true),
                None => (None, false),
            };
            if let Some((start, end, sub)) = found {
                jobs.push((hpa, i, start, end, sub, is_rh));
            }
        }
    }
    // `base` is cloned per job rather than borrowed: a borrow here makes the whole future
    // higher-ranked over the borrow's lifetime, which the app's `spawn` can't prove is `Send`.
    let results: Vec<_> =
        futures_util::stream::iter(jobs.into_iter().map(|(hpa, slot, start, end, sub, is_rh)| {
            let (base, http) = (base.clone(), http.clone());
            async move {
                let v = sample_message(&http, &base, start, end, sub, lon, lat)
                    .await
                    .ok();
                (hpa, slot, v, is_rh)
            }
        }))
        .buffered(SOUNDING_CONCURRENCY)
        .collect()
        .await;

    let mut by_level: std::collections::BTreeMap<u32, [Option<f64>; 4]> =
        std::collections::BTreeMap::new();
    let mut rh_levels = std::collections::BTreeSet::new();
    for (hpa, slot, val, is_rh) in results {
        by_level.entry(hpa).or_insert([None; 4])[slot] = val;
        if is_rh {
            rh_levels.insert(hpa);
        }
    }

    // Assemble complete levels (surface-first = highest pressure first).
    let mut levels: Vec<SoundingLevel> = Vec::new();
    for (&hpa, vals) in by_level.iter().rev() {
        if let [Some(t), Some(m), Some(u), Some(v)] = *vals {
            let temp_c = t - 273.15; // grib TMP/DPT are Kelvin
            let dewpt_c = if rh_levels.contains(&hpa) {
                dewpoint_from_rh(temp_c, m)
            } else {
                m - 273.15
            };
            levels.push(SoundingLevel {
                pressure_hpa: hpa as f64,
                temp_c,
                dewpt_c,
                u_ms: u,
                v_ms: v,
            });
        }
    }
    anyhow::ensure!(levels.len() >= 3, "sounding has too few complete levels");
    Ok(Sounding {
        lon,
        lat,
        run,
        fh,
        levels,
    })
}

/// Range-GET one GRIB2 message, decode it, and return the value at the grid point nearest
/// `(lon, lat)`.
async fn sample_message(
    http: &reqwest::Client,
    base: &str,
    start: u64,
    end: Option<u64>,
    sub: usize,
    lon: f64,
    lat: f64,
) -> anyhow::Result<f64> {
    let range = match end {
        Some(e) => format!("bytes={start}-{}", e - 1),
        None => format!("bytes={start}-"),
    };
    let bytes = http
        .get(crate::net::fetch_url(base))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .header("Range", range)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    // Off the async worker: each of these decodes a full HRRR level and scans ~1.9M grid points,
    // so leaving them here made forty concurrent fetches finish one decode at a time.
    crate::task::blocking(move || {
        crate::task::guarded(|| {
            // A later field of a multi-field message is rebuilt as a message of its own; the
            // decoder would otherwise hand back the first field (V read as U).
            if sub == 0 {
                sample_nearest(&bytes, lon, lat)
            } else {
                let one = crate::grib_split::extract_field(&bytes, sub)
                    .ok_or_else(|| anyhow::anyhow!("no field {sub} in that message"))?;
                sample_nearest(&one, lon, lat)
            }
        })
        .unwrap_or_else(|_| anyhow::bail!("grib decode panicked"))
    })
    .await?
}

fn sample_nearest(raw: &[u8], lon: f64, lat: f64) -> anyhow::Result<f64> {
    use gribberish::data_message::DataMessage;
    use gribberish::message::read_message;
    let msg = read_message(raw, 0).ok_or_else(|| anyhow::anyhow!("no GRIB2 message"))?;
    let dm = DataMessage::try_from(&msg).map_err(|e| anyhow::anyhow!("decode: {e:?}"))?;
    let (lats, lons) = dm.metadata.latlng();
    let data = dm.data;
    anyhow::ensure!(
        lats.len() == data.len() && lons.len() == data.len(),
        "latlng/data mismatch"
    );
    let mut best = None;
    let mut best_d = f64::MAX;
    for k in 0..data.len() {
        if !data[k].is_finite() || !lats[k].is_finite() || !lons[k].is_finite() {
            continue;
        }
        let dlon = (lons[k] - lon) * (lat.to_radians().cos());
        let d = dlon * dlon + (lats[k] - lat).powi(2);
        if d < best_d {
            best_d = d;
            best = Some(data[k]);
        }
    }
    best.ok_or_else(|| anyhow::anyhow!("no finite grid point"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_rejects_locations_outside_hrrr_before_network() {
        let err = match fetch_at(&reqwest::Client::new(), -150.0, 60.0, 0).await {
            Ok(_) => panic!("out-of-domain sounding should fail before fetching"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("outside the published model domain"));
    }

    #[test]
    fn idx_finds_var_at_level() {
        use crate::hrrr::field_byte_range as range;
        let idx = "1:0:d=2026:TMP:500 mb:anl:\n\
                   2:1000:d=2026:DPT:500 mb:anl:\n\
                   3:2500:d=2026:UGRD:500 mb:anl:\n";
        assert_eq!(range(idx, "TMP", "500 mb"), Some((0, Some(1000))));
        assert_eq!(range(idx, "DPT", "500 mb"), Some((1000, Some(2500))));
        assert_eq!(range(idx, "UGRD", "500 mb"), Some((2500, None)));
        assert_eq!(range(idx, "TMP", "850 mb"), None);
    }

    /// RAP's idx lists sibling fields sharing one offset. The old local parser took the very next
    /// line's offset and built an empty range; the shared one skips to the next distinct offset.
    #[test]
    fn idx_skips_submessage_siblings() {
        use crate::hrrr::field_byte_range as range;
        let idx = "1:0:d=2026:TMP:500 mb:anl:\n\
                   2:1000:d=2026:VUCSH:0-6000 m above ground:anl:\n\
                   3:1000:d=2026:VVCSH:0-6000 m above ground:anl:\n\
                   4:4000:d=2026:UGRD:500 mb:anl:\n";
        assert_eq!(
            range(idx, "VUCSH", "0-6000 m above ground"),
            Some((1000, Some(4000)))
        );
    }

    #[test]
    fn bulk_shear_computes() {
        let s = Sounding {
            lon: -97.0,
            lat: 35.0,
            run: Utc::now(),
            fh: 0,
            levels: vec![
                SoundingLevel {
                    pressure_hpa: 1000.0,
                    temp_c: 20.0,
                    dewpt_c: 18.0,
                    u_ms: 0.0,
                    v_ms: 0.0,
                },
                SoundingLevel {
                    pressure_hpa: 500.0,
                    temp_c: -10.0,
                    dewpt_c: -20.0,
                    u_ms: 20.0,
                    v_ms: 0.0,
                },
            ],
        };
        // 20 m/s shear ≈ 38.9 kt.
        let sh = s.bulk_shear_kt().unwrap();
        assert!((sh - 38.9).abs() < 0.5, "shear ~38.9 kt, got {sh}");
    }

    /// A classic unstable, veering Great-Plains profile.
    fn supercell_profile() -> Sounding {
        let mk = |p, t, td, u, v| SoundingLevel {
            pressure_hpa: p,
            temp_c: t,
            dewpt_c: td,
            u_ms: u,
            v_ms: v,
        };
        Sounding {
            lon: -97.0,
            lat: 35.0,
            run: Utc::now(),
            fh: 0,
            levels: vec![
                mk(1000.0, 30.0, 22.0, 0.0, 8.0),
                mk(925.0, 24.0, 19.0, 6.0, 12.0),
                mk(850.0, 20.0, 16.0, 10.0, 14.0),
                mk(700.0, 10.0, 2.0, 14.0, 16.0),
                mk(500.0, -8.0, -20.0, 20.0, 18.0),
                mk(400.0, -18.0, -32.0, 24.0, 18.0),
                mk(300.0, -32.0, -45.0, 27.0, 17.0),
                mk(250.0, -42.0, -55.0, 28.0, 16.0),
                mk(200.0, -52.0, -60.0, 29.0, 15.0),
                mk(150.0, -58.0, -66.0, 30.0, 14.0),
                mk(100.0, -55.0, -68.0, 31.0, 13.0),
            ],
        }
    }

    #[test]
    fn heights_increase_monotonically() {
        let h = supercell_profile().heights_m();
        assert_eq!(h[0], 0.0);
        assert!(h.windows(2).all(|w| w[1] > w[0]));
        // 500 hPa sits near 5.5–6 km in a warm airmass.
        assert!(
            (4800.0..6500.0).contains(&h[4]),
            "500 hPa height {:.0}",
            h[4]
        );
    }

    #[test]
    fn isotherm_heights_bracket_the_right_levels() {
        let s = supercell_profile();
        let h = s.heights_m();
        // 0 °C falls between 700 hPa (+10) and 500 hPa (−8); −20 °C between 400 (−18) and 300.
        let h0 = s.isotherm_height_m(0.0).unwrap();
        assert!(h[3] < h0 && h0 < h[4], "{h0} not in {}..{}", h[3], h[4]);
        let hm20 = s.isotherm_height_m(-20.0).unwrap();
        assert!(
            h[5] < hm20 && hm20 < h[6],
            "{hm20} not in {}..{}",
            h[5],
            h[6]
        );
        assert!(hm20 > h0);
        // Colder than +40 °C all the way up: that level is the ground itself. Warmer than −80 °C
        // all the way up: there is no such level in the profile.
        assert_eq!(s.isotherm_height_m(40.0), Some(0.0));
        assert_eq!(s.isotherm_height_m(-80.0), None);

        // A shallow cold layer at the ground is not the melting level: the crossing above the
        // warm nose is.
        let mut nose = supercell_profile();
        nose.levels[0].temp_c = -2.0;
        nose.levels[1].temp_c = 3.0;
        let n0 = nose.isotherm_height_m(0.0).unwrap();
        assert!(n0 > nose.heights_m()[3], "{n0}");
    }

    #[test]
    fn supercell_profile_yields_severe_indices() {
        let s = supercell_profile();
        let ix = s.indices().expect("indices");
        assert!(
            (300.0..6000.0).contains(&ix.sbcape),
            "CAPE plausible: {:.0}",
            ix.sbcape
        );
        assert!(
            (200.0..2500.0).contains(&ix.lcl_m),
            "LCL plausible: {:.0}",
            ix.lcl_m
        );
        assert!(
            ix.srh1 > 0.0,
            "veering profile → positive 0-1 km SRH: {:.0}",
            ix.srh1
        );
        assert!(
            ix.srh3 >= ix.srh1,
            "deeper layer accumulates at least as much: {:.0} vs {:.0}",
            ix.srh3,
            ix.srh1
        );
        assert!(
            (20.0..70.0).contains(&ix.shear6_kt),
            "0-6 shear: {:.0} kt",
            ix.shear6_kt
        );
        assert!(ix.scp > 0.0 && ix.stp > 0.0 && ix.ehi1 > 0.0);
    }

    #[test]
    fn supercell_profile_has_an_effective_inflow_layer() {
        let s = supercell_profile();
        let eff = crate::severe::effective_indices(&s).expect("effective layer");
        assert!(
            eff.esrh > 0.0,
            "veering profile → positive effective SRH: {:.0}",
            eff.esrh
        );
        // The profile now reaches the stratosphere, so the parcel has an equilibrium level and
        // the EL-dependent fields are solvable rather than honestly absent.
        let ebwd = eff.ebwd_kt.expect("effective bulk shear");
        assert!((10.0..90.0).contains(&ebwd), "EBWD: {ebwd:.0} kt");
        assert!(eff.stp_eff.expect("effective STP") > 0.0);
    }

    #[test]
    fn csv_carries_the_indices_and_the_profile() {
        let s = supercell_profile();
        let csv = s.to_csv();
        let (top, bottom) = csv.split_once("\n\n").expect("two blocks");
        assert!(top.starts_with("index,value\n"));
        assert!(top.lines().any(|l| l.starts_with("stp,")));
        assert!(top
            .lines()
            .any(|l| l.starts_with("ebwd_kt,") && l != "ebwd_kt,"));
        let mut rows = bottom.lines();
        assert_eq!(
            rows.next().unwrap(),
            "pressure_hpa,height_m,temp_c,dewpt_c,u_ms,v_ms"
        );
        assert_eq!(rows.next().unwrap(), "1000,0,30.0,22.0,0.0,8.0");
        assert_eq!(rows.count(), s.levels.len() - 1);
    }

    #[test]
    fn stable_profile_has_no_effective_layer() {
        // No parcel clears 100 J/kg, so there is no inflow layer to solve over.
        let mk = |p, t, td| SoundingLevel {
            pressure_hpa: p,
            temp_c: t,
            dewpt_c: td,
            u_ms: 5.0,
            v_ms: 0.0,
        };
        let s = Sounding {
            lon: 0.0,
            lat: 0.0,
            run: Utc::now(),
            fh: 0,
            levels: vec![
                mk(1000.0, 5.0, -10.0),
                mk(925.0, 8.0, -12.0),
                mk(850.0, 10.0, -15.0),
                mk(700.0, 4.0, -20.0),
                mk(500.0, -8.0, -30.0),
            ],
        };
        assert!(crate::severe::effective_indices(&s).is_none());
    }

    #[test]
    fn stable_profile_has_no_cape() {
        // Cold, dry surface under warmer air aloft: no positive buoyancy anywhere.
        let mk = |p, t, td| SoundingLevel {
            pressure_hpa: p,
            temp_c: t,
            dewpt_c: td,
            u_ms: 0.0,
            v_ms: 0.0,
        };
        let s = Sounding {
            lon: 0.0,
            lat: 0.0,
            run: Utc::now(),
            fh: 0,
            levels: vec![
                mk(1000.0, -5.0, -20.0),
                mk(850.0, 5.0, -15.0),
                mk(500.0, -10.0, -30.0),
            ],
        };
        let (cape, _) = s.sb_parcel().unwrap();
        assert!(cape < 10.0, "inversion profile CAPE ~0, got {cape:.1}");
    }

    #[test]
    fn parcel_reports_cin_lfc_and_el() {
        // A capped profile: warm, moist surface under a sharp inversion, then a cold mid-level.
        let lv = |p: f64, t: f64, d: f64| SoundingLevel {
            pressure_hpa: p,
            temp_c: t,
            dewpt_c: d,
            u_ms: 0.0,
            v_ms: 0.0,
        };
        let s = Sounding {
            lon: 0.0,
            lat: 0.0,
            run: Utc::now(),
            fh: 0,
            levels: vec![
                lv(1000.0, 30.0, 22.0),
                lv(925.0, 26.0, 18.0),
                lv(850.0, 22.0, 10.0),
                lv(700.0, 6.0, -5.0),
                lv(500.0, -14.0, -25.0),
                lv(300.0, -46.0, -60.0),
                lv(200.0, -54.0, -70.0),
            ],
        };
        let p = s.parcel().expect("profile is long enough");
        assert!(p.cape > 0.0, "cape {}", p.cape);
        assert!(p.cin <= 0.0, "cin {}", p.cin);
        assert!(p.lcl_m > 0.0 && p.lcl_m < 4000.0, "lcl {}", p.lcl_m);
        // The trace has one temperature per level, and the parcel cools with height.
        assert_eq!(p.trace_c.len(), s.levels.len());
        assert!(p.trace_c[0] > *p.trace_c.last().unwrap());
        // LFC, when it exists, is at or above the LCL is not guaranteed by geometry alone, but it
        // must sit inside the profile.
        if let Some(lfc) = p.lfc_m {
            assert!((0.0..20000.0).contains(&lfc), "lfc {lfc}");
        }
        // sb_parcel still answers what it always did.
        let (cape, lcl) = s.sb_parcel().unwrap();
        assert_eq!((cape, lcl), (p.cape, p.lcl_m));
    }
}

#[cfg(test)]
mod f8_tests {
    use super::*;

    /// A warm, moist, sheared plains profile on the mandatory levels.
    fn plains() -> Sounding {
        let lv = |p: f64, t: f64, td: f64, u: f64, v: f64| SoundingLevel {
            pressure_hpa: p,
            temp_c: t,
            dewpt_c: td,
            u_ms: u,
            v_ms: v,
        };
        Sounding {
            lon: -97.5,
            lat: 35.2,
            run: chrono::DateTime::UNIX_EPOCH,
            fh: 0,
            levels: vec![
                lv(1000.0, 30.0, 21.0, 2.0, 8.0),
                lv(925.0, 25.0, 18.0, 6.0, 14.0),
                lv(850.0, 21.0, 12.0, 10.0, 16.0),
                lv(700.0, 9.0, -2.0, 16.0, 14.0),
                lv(600.0, 0.0, -12.0, 20.0, 12.0),
                lv(500.0, -9.0, -25.0, 24.0, 10.0),
                lv(400.0, -21.0, -38.0, 28.0, 10.0),
                lv(300.0, -37.0, -50.0, 32.0, 8.0),
                lv(250.0, -47.0, -58.0, 34.0, 8.0),
                lv(200.0, -55.0, -65.0, 34.0, 6.0),
            ],
        }
    }

    #[test]
    fn a_zero_kelvin_dewpoint_reads_as_no_vapour() {
        let mut s = plains();
        let pw = s.pwat_mm().unwrap();
        s.levels.last_mut().unwrap().dewpt_c = -273.15;
        let dry_top = s.pwat_mm().unwrap();
        assert!(
            dry_top.is_finite() && (dry_top - pw).abs() < 0.5,
            "{dry_top} vs {pw}"
        );
    }

    #[test]
    fn precipitable_water_is_a_humid_summer_amount() {
        let pw = plains().pwat_mm().unwrap();
        assert!((30.0..55.0).contains(&pw), "{pw} mm");
        // Drying every level lowers it.
        let mut dry = plains();
        dry.levels.iter_mut().for_each(|l| l.dewpt_c -= 15.0);
        assert!(dry.pwat_mm().unwrap() < pw / 2.0);
    }

    #[test]
    fn downdraft_cape_is_positive_under_a_dry_mid_level() {
        let d = plains().dcape().unwrap();
        assert!((300.0..2000.0).contains(&d), "{d} J/kg");
        // Moister mid-levels: a warmer wet bulb to start from, so a weaker downdraft.
        let mut moist = plains();
        moist
            .levels
            .iter_mut()
            .filter(|l| l.pressure_hpa <= 850.0)
            .for_each(|l| {
                l.dewpt_c = (l.dewpt_c + 8.0).min(l.temp_c);
            });
        assert!(
            moist.dcape().unwrap() < d,
            "{} vs {d}",
            moist.dcape().unwrap()
        );
    }

    #[test]
    fn a_saturated_parcel_goes_down_the_moist_adiabat_it_came_up() {
        let up = moist_adiabat_k(900.0, 290.0, 500.0);
        let down = moist_adiabat_k(500.0, up, 900.0);
        assert!((down - 290.0).abs() < 0.05, "{down}");
        // Normand's rule: the wet bulb sits between the dewpoint and the temperature.
        let tw = wet_bulb_k(1000.0, 30.0, 20.0) - 273.15;
        assert!(
            (20.0..30.0).contains(&tw) && (tw - 23.0).abs() < 1.5,
            "{tw}"
        );
    }

    #[test]
    fn lapse_rates_and_the_left_mover_read_sensibly() {
        let s = plains();
        let mid = s.lapse_rate_between_c_km(700.0, 500.0).unwrap();
        assert!((6.0..8.5).contains(&mid), "{mid} C/km");
        let low = s.lapse_rate_c_km(0.0, 3000.0).unwrap();
        assert!((5.0..9.9).contains(&low), "{low} C/km");
        let (rm, lm) = (s.bunkers_rm().unwrap(), s.bunkers_lm().unwrap());
        let mean = s.mean_wind_0_6().unwrap();
        assert!(((rm.0 + lm.0) / 2.0 - mean.0).abs() < 1e-9, "mirror images");
        // Veering winds: more helicity for the right mover than the left.
        assert!(s.srh_relative(3000.0, rm).unwrap() > s.srh_relative(3000.0, lm).unwrap());
        assert_eq!(s.srh(3000.0), s.srh_relative(3000.0, rm));
        let csv = s.to_csv();
        for key in [
            "pwat_mm,",
            "dcape_jkg,",
            "lapse_700_500_c_km,",
            "height_m20c_m,",
        ] {
            assert!(csv.contains(key), "{key} missing from\n{csv}");
        }
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn a_live_hrrr_profile_reads_all_the_new_numbers() {
        let s = fetch(&reqwest::Client::new(), -97.5, 35.2).await.unwrap();
        let (pw, dc) = (s.pwat_mm().unwrap(), s.dcape().unwrap());
        let (lr03, lr75) = (
            s.lapse_rate_c_km(0.0, 3000.0).unwrap(),
            s.lapse_rate_between_c_km(700.0, 500.0).unwrap(),
        );
        eprintln!(
            "HRRR {} f{:02} at OKC: PWAT {pw:.1} mm, DCAPE {dc:.0} J/kg, LR0-3 {lr03:.1}, LR700-500 {lr75:.1} C/km, 0C {:?} m, -20C {:?} m, RM {:?}, LM {:?}",
            s.run,
            s.fh,
            s.isotherm_height_m(0.0).map(|v| v.round()),
            s.isotherm_height_m(-20.0).map(|v| v.round()),
            s.bunkers_rm(),
            s.bunkers_lm(),
        );
        assert!((1.0..80.0).contains(&pw));
        assert!((0.0..3000.0).contains(&dc));
        assert!((2.0..11.0).contains(&lr75));
    }

    #[test]
    fn the_three_parcels_differ_the_way_they_should() {
        let s = plains();
        let (sb, _) = s.parcel_of(ParcelKind::SurfaceBased).unwrap();
        let (ml, _) = s.parcel_of(ParcelKind::MixedLayer).unwrap();
        let (mu, _) = s.parcel_of(ParcelKind::MostUnstable).unwrap();
        // A superadiabatic, moist surface: mixing it with drier air above lowers the CAPE.
        assert!(ml.cape < sb.cape, "ML {} < SB {}", ml.cape, sb.cape);
        assert!(mu.cape >= sb.cape, "MU {} >= SB {}", mu.cape, sb.cape);
        // A cold, stable surface layer under warm moist air: the most unstable parcel is aloft.
        let mut elevated = plains();
        elevated.levels[0].temp_c = 12.0;
        elevated.levels[0].dewpt_c = 10.0;
        elevated.levels[1].temp_c = 24.0;
        elevated.levels[1].dewpt_c = 20.0;
        let (sb, _) = elevated.parcel_of(ParcelKind::SurfaceBased).unwrap();
        let (mu, start) = elevated.parcel_of(ParcelKind::MostUnstable).unwrap();
        assert!(start > 0, "lifted from above the surface");
        assert!(
            mu.cape > sb.cape + 100.0,
            "MU {} vs SB {}",
            mu.cape,
            sb.cape
        );
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn the_previous_run_is_the_same_valid_time_an_hour_older() {
        let http = reqwest::Client::new();
        let now = fetch_at(&http, -97.5, 35.2, 3).await.unwrap();
        let before = fetch_previous_run(&http, &now).await.unwrap();
        let valid = |s: &Sounding| s.run + chrono::Duration::hours(s.fh as i64);
        assert_eq!(valid(&before), valid(&now));
        assert_eq!(before.run, now.run - chrono::Duration::hours(1));
        eprintln!(
            "run {} f{} vs run {} f{}: 500 hPa {:.1} vs {:.1} C",
            now.run, now.fh, before.run, before.fh, now.levels[5].temp_c, before.levels[5].temp_c
        );
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn rap_and_the_nam_nest_give_soundings_with_real_winds() {
        let http = reqwest::Client::new();
        let hrrr = fetch(&http, -97.5, 35.2).await.unwrap();
        for model in [SoundingModel::Rap, SoundingModel::NamNest] {
            let s = fetch_model_at(&http, model, -97.5, 35.2, 1).await.unwrap();
            assert!(
                s.levels.len() >= 8,
                "{}: {} levels",
                model.label(),
                s.levels.len()
            );
            // U and V really are two fields: the old trap reads V as a copy of U everywhere.
            assert!(
                s.levels.iter().any(|l| (l.u_ms - l.v_ms).abs() > 0.5),
                "{}: u == v at every level",
                model.label()
            );
            let (m5, h5) = (
                s.levels.iter().find(|l| l.pressure_hpa == 500.0).unwrap(),
                hrrr.levels
                    .iter()
                    .find(|l| l.pressure_hpa == 500.0)
                    .unwrap(),
            );
            eprintln!(
                "{} run {} f{}: 500 hPa T {:.1} Td {:.1} wind ({:.1},{:.1}) vs HRRR T {:.1} Td {:.1} ({:.1},{:.1}); PWAT {:.1} mm",
                model.label(), s.run, s.fh, m5.temp_c, m5.dewpt_c, m5.u_ms, m5.v_ms,
                h5.temp_c, h5.dewpt_c, h5.u_ms, h5.v_ms, s.pwat_mm().unwrap_or(f64::NAN)
            );
            // Models disagree, but not by a different airmass at 500 hPa.
            assert!((m5.temp_c - h5.temp_c).abs() < 4.0);
            assert!((m5.u_ms - h5.u_ms).abs() < 12.0 && (m5.v_ms - h5.v_ms).abs() < 12.0);
        }
    }

    #[test]
    fn a_dewpoint_comes_back_from_its_relative_humidity() {
        for (t, td) in [(30.0, 20.0), (0.0, -12.0), (-40.0, -45.0)] {
            let rh = 100.0 * e_sat_hpa(td) / e_sat_hpa(t);
            assert!((dewpoint_from_rh(t, rh) - td).abs() < 0.01, "{t} {td}");
        }
        assert!((dewpoint_from_rh(10.0, 100.0) - 10.0).abs() < 1e-9);
    }
}
