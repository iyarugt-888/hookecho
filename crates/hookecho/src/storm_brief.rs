//! The Storm Digest's evidence: what the app has measured and computed about the storms in view,
//! gathered into one structure (`app/digest_brief.rs` fills it) and written out two ways — a
//! compact fact sheet for the AI to analyse ([`Brief::fact_sheet`]), and an offline summary for
//! when no AI key is set ([`Brief::summary`]).
//!
//! Radar and algorithm output only: SCIT cell attributes and the severity score, ProbSevere, the
//! core's dual-pol statistics, the rotation / debris / hail-spike / ZDR-column detectors, trends,
//! the melting layer and lightning. Warnings and watches are deliberately not part of it — the
//! digest is an independent read of the data, not a restatement of what was issued.
//!
//! Everything is optional: a value the app does not have is left out of the sheet rather than
//! written as zero, and the sheet says when a detector had nothing to scan.

use chrono::{DateTime, Utc};
use std::fmt::Write as _;

/// Signatures further than this from every storm core are listed on their own.
pub const ATTACH_KM: f64 = 10.0;

/// Everything gathered for one digest.
#[derive(Debug, Default, Clone)]
pub struct Brief {
    /// The radar the volume came from ("KTLX").
    pub site: Option<String>,
    /// The volume's scan time.
    pub scan: Option<DateTime<Utc>>,
    /// The volume coverage pattern, as the volume names it ("VCP 212 (Precipitation)").
    pub vcp: Option<String>,
    /// The tilt the core statistics were read from (degrees).
    pub tilt_deg: Option<f32>,
    /// Freezing level and −20 °C level, metres above sea level.
    pub freezing_m: Option<f64>,
    pub minus20_m: Option<f64>,
    /// Melting layer read off the CC field: height above radar level (km) and the mean CC there.
    pub bright_band: Option<(f64, f32)>,
    /// Whether the volume scanned velocity / dual-pol at all (a detector with no input says so).
    pub has_velocity: bool,
    pub has_dualpol: bool,
    /// Storms in view, most severe first.
    pub storms: Vec<StormBrief>,
    /// Signatures in view that sit near no storm core.
    pub loose: Vec<Signature>,
    /// GOES lightning flashes in view in the last five minutes, when the feed is on.
    pub flashes_5min: Option<usize>,
    /// Storms the SCIT table tracks outside the view (counted, not described).
    pub storms_elsewhere: usize,
    /// What could not be checked this time, and why ("ZDR columns not checked: no freezing level
    /// yet"), so an absent signature is not read as a negative.
    pub notes: Vec<String>,
}

/// One storm cell.
#[derive(Debug, Default, Clone)]
pub struct StormBrief {
    /// The SCIT cell id ("K4"), or for a detection the radar reported without a parent cell, its
    /// kind ("Mesocyclone").
    pub id: String,
    /// A radar detection without a parent storm cell, rather than a tracked storm.
    pub standalone: bool,
    pub lon: f64,
    pub lat: f64,
    /// "14 km NW of Moore", when a town label is near.
    pub place: Option<String>,
    /// Range (km) and bearing (degrees) from the radar.
    pub from_radar: Option<(f64, f64)>,
    /// Composite severity 0–100 and the evidence behind it, as "label: detail".
    pub score: Option<u8>,
    pub score_reasons: Vec<String>,
    pub max_dbz: Option<f32>,
    pub max_dbz_hgt_kft: Option<f32>,
    pub top_kft: Option<f32>,
    pub base_kft: Option<f32>,
    pub vil: Option<f32>,
    pub poh: Option<i32>,
    pub posh: Option<i32>,
    pub hail_in: Option<f32>,
    /// The radar algorithm's own TVS / mesocyclone flags.
    pub tvs: bool,
    pub meso: bool,
    /// Moving toward (degrees) at (knots).
    pub motion: Option<(f32, f32)>,
    /// ProbSevere's dominant probability over the cell (%).
    pub prob_severe: Option<u8>,
    /// The core statistics table: label and value.
    pub core: Vec<(String, String)>,
    /// Detector signatures within [`ATTACH_KM`] of the core.
    pub signatures: Vec<Signature>,
    /// How the cell changed over the recent scans.
    pub trend: Option<Trend>,
    /// GOES flashes within [`ATTACH_KM`] in the last five minutes.
    pub flashes_5min: Option<usize>,
}

/// A detector's hit, already put in words.
#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    pub kind: SignatureKind,
    /// The measurements, in words ("62 kt Vrot, 4 tilts 0.4–3.1 km, cyclonic, 78% confidence").
    pub detail: String,
    /// Distance from the storm's core (km); for a loose signature, from the radar.
    pub km: f64,
    /// Compass point from the core (or the radar).
    pub bearing: f64,
    /// 0–1 detector confidence, where the detector has one; used to order signatures.
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureKind {
    Rotation,
    Debris,
    HailSpike,
    ZdrColumn,
}

impl SignatureKind {
    pub fn label(self) -> &'static str {
        match self {
            SignatureKind::Rotation => "rotation couplet",
            SignatureKind::Debris => "debris signature (TDS)",
            SignatureKind::HailSpike => "three-body scatter spike (hail)",
            SignatureKind::ZdrColumn => "ZDR column (updraft)",
        }
    }
}

/// First and last of a cell's samples over the recent scans.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trend {
    pub minutes: i64,
    pub scans: usize,
    pub dbz: Option<(f32, f32)>,
    pub top_kft: Option<(f32, f32)>,
    pub vil: Option<(f32, f32)>,
    pub severity: Option<(u8, u8)>,
}

const KT_PER_MS: f32 = 1.943_844;

/// A compass point for a bearing in degrees.
pub fn compass(deg: f64) -> &'static str {
    const P: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    P[(((deg.rem_euclid(360.0) + 11.25) / 22.5) as usize) % 16]
}

/// A rotation couplet in words. Its `confirmation` (reports and warnings) is left out on purpose.
pub fn rotation(h: &wxdata::rotation::CoupletHit) -> String {
    let mut s = format!(
        "{:.0} kt rotational velocity ({:.0} kt gate-to-gate), {} tilt{} from {:.1} to {:.1} km",
        h.vrot_ms * KT_PER_MS,
        h.g2g_ms * KT_PER_MS,
        h.tilts,
        if h.tilts == 1 { "" } else { "s" },
        h.base_km,
        h.top_km,
    );
    match h.rooted {
        Some(true) => s.push_str(", reaches the lowest tilt"),
        Some(false) => s.push_str(", aloft only (not in the lowest tilt)"),
        None => {}
    }
    s.push_str(match h.sense {
        wxdata::rotation::Sense::Anticyclonic => ", anticyclonic",
        _ => ", cyclonic",
    });
    if let Some(d) = h.debris_confidence {
        let _ = write!(
            s,
            ", debris beside it (evidence {})",
            wxdata::evidence::out_of_100(d)
        );
    }
    let _ = write!(
        s,
        ", {:.0} km from radar, evidence score {}",
        h.range_km,
        wxdata::evidence::out_of_100(h.confidence)
    );
    s
}

/// A debris signature in words.
pub fn debris(h: &wxdata::tds::TdsHit) -> String {
    let mut s = format!(
        "CC as low as {:.2} (mean {:.2}) in {:.0} dBZ echo (max {:.0}), {:.1} km², {} tilt{} up to {:.1} km",
        h.min_cc,
        h.mean_cc,
        h.mean_z,
        h.max_z,
        h.area_km2,
        h.tilts,
        if h.tilts == 1 { "" } else { "s" },
        h.top_km,
    );
    if let Some(c) = h.contrast {
        let _ = write!(s, ", CC {c:.2} higher around it");
    }
    if let Some(z) = h.zdr_db {
        let _ = write!(s, ", mean ZDR {z:.1} dB");
    }
    match h.rotation_ms {
        Some(v) => {
            let _ = write!(s, ", rotation beside it ({:.0} kt)", v * KT_PER_MS);
        }
        None if h.unrotated => s.push_str(", no rotation beside it"),
        None => {}
    }
    let _ = write!(
        s,
        ", evidence score {}",
        wxdata::evidence::out_of_100(h.confidence)
    );
    s
}

/// A hail spike in words.
pub fn hail_spike(h: &wxdata::dualpol::TbssHit) -> String {
    format!(
        "{:.1} km spike behind a {:.0} dBZ core, CC down to {:.2}",
        h.len_km, h.core_dbz, h.min_cc
    )
}

/// A ZDR column in words. Heights here are above radar level.
pub fn zdr_column(h: &wxdata::dualpol::ZdrColumnHit) -> String {
    format!(
        "ZDR up to {:.1} dB reaching {:.1} km above the freezing level (top {:.1} km)",
        h.max_zdr, h.depth_km, h.top_km
    )
}

fn opt<T: std::fmt::Display>(v: Option<T>, unit: &str) -> Option<String> {
    v.map(|v| format!("{v}{unit}"))
}

impl StormBrief {
    fn heading(&self) -> String {
        let mut h = if self.standalone {
            format!("{} (radar detection, no tracked cell)", self.id)
        } else {
            format!("Storm {}", self.id)
        };
        if let Some(p) = &self.place {
            let _ = write!(h, " — {p}");
        }
        if let Some((km, b)) = self.from_radar {
            let _ = write!(h, " ({km:.0} km {} of the radar)", compass(b));
        }
        h
    }

    fn attributes(&self) -> Vec<String> {
        let f0 = |v: Option<f32>| v.map(|v| format!("{v:.0}"));
        let mut a = Vec::new();
        if let Some(s) = self.score {
            a.push(format!("severity {s}/100"));
        }
        if let Some(d) = f0(self.max_dbz) {
            let at = self
                .max_dbz_hgt_kft
                .map(|h| format!(" at {h:.0} kft"))
                .unwrap_or_default();
            a.push(format!("max {d} dBZ{at}"));
        }
        a.extend(opt(f0(self.top_kft), " kft top").map(|s| format!("echo {s}")));
        a.extend(opt(f0(self.base_kft), " kft base").map(|s| format!("echo {s}")));
        a.extend(f0(self.vil).map(|v| format!("VIL {v} kg/m²")));
        a.extend(opt(self.poh, "%").map(|s| format!("POH {s}")));
        a.extend(opt(self.posh, "%").map(|s| format!("POSH {s}")));
        if let Some(h) = self.hail_in.filter(|h| *h > 0.0) {
            a.push(format!("max expected hail {h:.2} in"));
        }
        if self.tvs {
            a.push("radar TVS flag".into());
        }
        if self.meso {
            a.push("radar mesocyclone flag".into());
        }
        if let Some(p) = self.prob_severe {
            a.push(format!("ProbSevere {p}%"));
        }
        match self.motion {
            Some((to, kt)) => a.push(format!("moving toward {} at {kt:.0} kt", compass(to as f64))),
            None => a.push("motion unknown".into()),
        }
        if let Some(n) = self.flashes_5min {
            a.push(format!("{n} lightning flashes nearby in 5 min"));
        }
        a
    }
}

fn trend_line(t: &Trend) -> Option<String> {
    let mut parts = Vec::new();
    let pair = |name: &str, v: Option<(f32, f32)>, unit: &str| {
        v.map(|(a, b)| format!("{name} {a:.0}→{b:.0}{unit}"))
    };
    parts.extend(pair("max dBZ", t.dbz, ""));
    parts.extend(pair("top", t.top_kft, " kft"));
    parts.extend(pair("VIL", t.vil, ""));
    parts.extend(
        t.severity
            .map(|(a, b)| format!("severity {a}→{b}")),
    );
    (!parts.is_empty()).then(|| {
        format!(
            "over the last {} min ({} scans): {}",
            t.minutes,
            t.scans,
            parts.join(", ")
        )
    })
}

fn signature_line(s: &Signature, from: &str) -> String {
    let at = if s.km < 1.0 {
        format!("at the {from}")
    } else {
        format!("{:.0} km {} of the {from}", s.km, compass(s.bearing))
    };
    format!("{} {at}: {}", s.kind.label(), s.detail)
}

impl Brief {
    /// The facts for the AI, one per line, grouped by storm. Plain text rather than JSON: shorter,
    /// and every value carries its unit.
    pub fn fact_sheet(&self) -> String {
        let mut o = String::new();
        let site = self.site.as_deref().unwrap_or("unknown radar");
        let _ = write!(o, "Radar {site}");
        if let Some(t) = self.scan {
            let _ = write!(o, ", volume scanned {}", t.format("%Y-%m-%d %H:%M UTC"));
        }
        if let Some(v) = &self.vcp {
            let _ = write!(o, ", {v}");
        }
        o.push('\n');
        if let Some(t) = self.tilt_deg {
            let _ = writeln!(
                o,
                "Core statistics read from the {t:.1}° tilt within 8 km of each core. Dual-pol                  values are from the ≥40 dBZ core. ΔV is the spread between the strongest inbound                  and outbound velocity in that box — broad flow as much as rotation; rotation is                  what the couplet detector reports."
            );
        }
        for n in &self.notes {
            let _ = writeln!(o, "{n}.");
        }
        if !self.has_velocity {
            o.push_str("This volume has no velocity data, so rotation could not be scanned.\n");
        }
        if !self.has_dualpol {
            o.push_str(
                "This volume has no dual-polarization data, so debris, hail-spike and ZDR-column \
                 detectors could not run.\n",
            );
        }
        match (self.freezing_m, self.minus20_m) {
            (Some(f), Some(m)) => {
                let _ = writeln!(o, "Freezing level {f:.0} m MSL, −20 °C level {m:.0} m MSL.");
            }
            (Some(f), None) => {
                let _ = writeln!(o, "Freezing level {f:.0} m MSL.");
            }
            _ => {}
        }
        if let Some((h, cc)) = self.bright_band {
            let _ = writeln!(o, "Melting layer seen in CC at {h:.1} km above the radar (mean CC {cc:.2}).");
        }
        if let Some(n) = self.flashes_5min {
            let _ = writeln!(o, "GOES lightning in view, last 5 min: {n} flashes.");
        }
        if self.storms.is_empty() {
            o.push_str("No tracked storm cells in view.\n");
        }
        for s in &self.storms {
            o.push('\n');
            let _ = writeln!(o, "{}", s.heading());
            let _ = writeln!(o, "- {}", s.attributes().join("; "));
            if !s.score_reasons.is_empty() {
                let _ = writeln!(o, "- severity evidence: {}", s.score_reasons.join("; "));
            }
            if !s.core.is_empty() {
                let core: Vec<String> = s.core.iter().map(|(l, v)| format!("{l} {v}")).collect();
                let _ = writeln!(o, "- core: {}", core.join("; "));
            }
            if let Some(t) = s.trend.as_ref().and_then(trend_line) {
                let _ = writeln!(o, "- trend {t}");
            }
            if s.signatures.is_empty() {
                o.push_str("- no detector signatures near this core\n");
            }
            for sig in &s.signatures {
                let _ = writeln!(o, "- {}", signature_line(sig, "core"));
            }
        }
        if !self.loose.is_empty() {
            o.push_str("\nSignatures in view not near any tracked storm core:\n");
            for sig in &self.loose {
                let _ = writeln!(o, "- {}", signature_line(sig, "radar"));
            }
        }
        if self.storms_elsewhere > 0 {
            let _ = writeln!(o, "\n{} more tracked storms outside the view.", self.storms_elsewhere);
        }
        o
    }

    /// The offline digest: the most significant storms in a sentence or two each.
    pub fn summary(&self) -> String {
        let mut o = String::new();
        if let Some(t) = self.scan {
            let _ = write!(
                o,
                "{} scan at {}. ",
                self.site.as_deref().unwrap_or("Radar"),
                t.format("%H:%M UTC")
            );
        }
        if self.storms.is_empty() && self.loose.is_empty() {
            o.push_str("No tracked storms or radar signatures in view.");
            return o;
        }
        let n = self.storms.len();
        if n > 0 {
            let _ = write!(o, "{n} storm{} in view.", if n == 1 { "" } else { "s" });
        }
        for s in self.storms.iter().take(3) {
            o.push_str("\n\n");
            o.push_str(&s.heading());
            o.push_str(": ");
            o.push_str(&s.attributes().join(", "));
            o.push('.');
            if let Some(t) = s.trend.as_ref().and_then(trend_line) {
                let _ = write!(o, " Trend {t}.");
            }
            for sig in s.signatures.iter().take(3) {
                let _ = write!(o, " {}.", capitalise(&signature_line(sig, "core")));
            }
        }
        if n > 3 {
            let _ = write!(o, "\n\n{} weaker storm{} not described.", n - 3, if n == 4 { "" } else { "s" });
        }
        if !self.loose.is_empty() {
            let _ = write!(
                o,
                "\n\n{} signature{} away from any tracked storm.",
                self.loose.len(),
                if self.loose.len() == 1 { "" } else { "s" }
            );
        }
        o
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// First and last values over samples within `window_min` of the newest one.
pub fn trend_of(samples: &[crate::ui::cell_window::CellSample], window_min: i64) -> Option<Trend> {
    let newest = samples.iter().filter_map(|s| s.time).max()?;
    let mut recent: Vec<&crate::ui::cell_window::CellSample> = samples
        .iter()
        .filter(|s| s.time.is_some_and(|t| (newest - t).num_minutes() <= window_min))
        .collect();
    recent.sort_by_key(|s| s.time);
    if recent.len() < 2 {
        return None;
    }
    let ends = |f: fn(&crate::ui::cell_window::CellSample) -> Option<f32>| {
        let a = recent.iter().find_map(|s| f(s))?;
        let b = recent.iter().rev().find_map(|s| f(s))?;
        Some((a, b))
    };
    let first = recent.first().and_then(|s| s.time)?;
    Some(Trend {
        minutes: (newest - first).num_minutes(),
        scans: recent.len(),
        dbz: ends(|s| s.dbz),
        top_kft: ends(|s| s.top),
        vil: ends(|s| s.vil),
        severity: ends(|s| s.severity.map(f32::from)).map(|(a, b)| (a as u8, b as u8)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storm() -> StormBrief {
        StormBrief {
            id: "K4".into(),
            place: Some("6 km SW of Moore".into()),
            from_radar: Some((24.0, 200.0)),
            score: Some(81),
            max_dbz: Some(66.0),
            max_dbz_hgt_kft: Some(21.0),
            top_kft: Some(48.0),
            vil: Some(62.0),
            posh: Some(70),
            hail_in: Some(1.75),
            meso: true,
            motion: Some((60.0, 28.0)),
            core: vec![("CC (p5)".into(), "0.71".into())],
            signatures: vec![Signature {
                kind: SignatureKind::Rotation,
                detail: "61 kt rotational velocity".into(),
                km: 2.0,
                bearing: 225.0,
                confidence: Some(0.8),
            }],
            trend: Some(Trend {
                minutes: 20,
                scans: 5,
                dbz: Some((58.0, 66.0)),
                top_kft: None,
                vil: Some((40.0, 62.0)),
                severity: Some((55, 81)),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn the_fact_sheet_carries_every_measurement_with_its_unit() {
        let b = Brief {
            site: Some("KTLX".into()),
            has_velocity: true,
            has_dualpol: true,
            freezing_m: Some(4100.0),
            storms: vec![storm()],
            ..Default::default()
        };
        let s = b.fact_sheet();
        for want in [
            "Radar KTLX",
            "Storm K4 — 6 km SW of Moore (24 km SSW of the radar)",
            "severity 81/100",
            "max 66 dBZ at 21 kft",
            "echo 48 kft top",
            "VIL 62",
            "POSH 70%",
            "max expected hail 1.75 in",
            "radar mesocyclone flag",
            "moving toward ENE at 28 kt",
            "core: CC (p5) 0.71",
            "max dBZ 58→66",
            "severity 55→81",
            "rotation couplet 2 km SW of the core: 61 kt",
            "Freezing level 4100 m MSL",
        ] {
            assert!(s.contains(want), "missing {want:?} in\n{s}");
        }
        assert!(!s.contains("warning"), "warnings are not part of the brief:\n{s}");
    }

    #[test]
    fn a_detection_without_a_cell_is_named_by_its_kind() {
        let m = StormBrief {
            id: "Mesocyclone".into(),
            standalone: true,
            ..Default::default()
        };
        assert_eq!(m.heading(), "Mesocyclone (radar detection, no tracked cell)");
    }

    #[test]
    fn missing_inputs_are_said_not_zeroed() {
        let b = Brief::default();
        let s = b.fact_sheet();
        assert!(s.contains("no velocity data"), "{s}");
        assert!(s.contains("no dual-polarization data"), "{s}");
        assert!(s.contains("No tracked storm cells in view"), "{s}");
        let bare = StormBrief {
            id: "A1".into(),
            ..Default::default()
        };
        let line = bare.attributes().join("; ");
        assert_eq!(line, "motion unknown");
    }

    #[test]
    fn the_offline_summary_leads_with_the_strongest_storms() {
        let mut b = Brief {
            storms: vec![storm(); 5],
            ..Default::default()
        };
        b.storms[0].id = "K4".into();
        let s = b.summary();
        assert!(s.starts_with("5 storms in view."), "{s}");
        assert!(s.contains("Storm K4"), "{s}");
        assert!(s.contains("Rotation couplet 2 km SW of the core"), "{s}");
        assert!(s.contains("2 weaker storms not described"), "{s}");
    }

    #[test]
    fn a_trend_reads_the_window_ending_at_the_newest_scan() {
        use crate::ui::cell_window::CellSample;
        let t0 = chrono::Utc::now();
        let at = |m: i64, dbz: f32| CellSample {
            vil: None,
            top: Some(30.0 + m as f32),
            dbz: Some(dbz),
            severity: None,
            time: Some(t0 + chrono::Duration::minutes(m)),
            dbz_hgt: None,
        };
        let samples = vec![at(0, 40.0), at(40, 50.0), at(50, 55.0), at(60, 60.0)];
        let t = trend_of(&samples, 30).expect("trend");
        assert_eq!(t.minutes, 20);
        assert_eq!(t.scans, 3);
        assert_eq!(t.dbz, Some((50.0, 60.0)));
        assert_eq!(t.vil, None);
        assert!(trend_of(&samples[..1], 30).is_none());
    }

    #[test]
    fn compass_points_wrap() {
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(359.0), "N");
        assert_eq!(compass(-90.0), "W");
        assert_eq!(compass(202.5), "SSW");
    }
}
