//! What a model file actually holds (ROADMAP_PARITY M5.3): its GRIB2 `.idx` inventory read into
//! fields with a classified level and timing, and a vetted table of the NCEP parameter
//! abbreviations whose quantity and units are known.
//!
//! A line's parameter, level and timing are taken exactly as the file names them. Only
//! parameters in [`VETTED`] get a quantity, a unit and a display conversion; everything else —
//! including the local `var discipline=… parm=…` entries — is listed as unsupported rather than
//! given a guessed unit or palette. Two fields compare only when they are the same quantity at
//! the same level over the same kind of interval of the same length.

/// The physical quantity behind an NCEP abbreviation, and how it is shown: `display = native *
/// scale + offset`, in `unit`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    pub var: &'static str,
    pub name: &'static str,
    /// As the GRIB2 table gives it.
    pub native_unit: &'static str,
    /// As it is drawn, probed and exported.
    pub unit: &'static str,
    pub scale: f32,
    pub offset: f32,
    /// One horizontal component of a vector, and its partner's abbreviation. The components are
    /// as the file defines them: the HRRR's and NAM's are relative to their Lambert grid, not to
    /// east and north, so they are named u and v, never east and north.
    pub component: Option<(Component, &'static str)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    U,
    V,
}

impl Quantity {
    pub fn to_display(&self, native: f32) -> f32 {
        native * self.scale + self.offset
    }
}

const fn q(
    var: &'static str,
    name: &'static str,
    native_unit: &'static str,
    unit: &'static str,
    scale: f32,
    offset: f32,
) -> Quantity {
    Quantity {
        var,
        name,
        native_unit,
        unit,
        scale,
        offset,
        component: None,
    }
}

/// The NCEP GRIB2 abbreviations whose meaning and units are vetted (NCEP's GRIB2 parameter
/// tables, product discipline 0). Kept short on purpose: an entry here is a claim about units.
pub const VETTED: &[Quantity] = &[
    q("TMP", "Temperature", "K", "°C", 1.0, -273.15),
    q("DPT", "Dew point", "K", "°C", 1.0, -273.15),
    q("RH", "Relative humidity", "%", "%", 1.0, 0.0),
    q("SPFH", "Specific humidity", "kg/kg", "g/kg", 1000.0, 0.0),
    q("HGT", "Geopotential height", "gpm", "dam", 0.1, 0.0),
    Quantity {
        component: Some((Component::U, "VGRD")),
        ..q("UGRD", "Wind u component", "m/s", "kt", 1.943_844, 0.0)
    },
    Quantity {
        component: Some((Component::V, "UGRD")),
        ..q("VGRD", "Wind v component", "m/s", "kt", 1.943_844, 0.0)
    },
    q("WIND", "Wind speed", "m/s", "kt", 1.943_844, 0.0),
    q("GUST", "Wind gust", "m/s", "kt", 1.943_844, 0.0),
    q("APCP", "Precipitation", "kg/m²", "mm", 1.0, 0.0),
    q(
        "PRATE",
        "Precipitation rate",
        "kg/m²/s",
        "mm/h",
        3600.0,
        0.0,
    ),
    q("PRMSL", "Sea-level pressure", "Pa", "hPa", 0.01, 0.0),
    q("MSLMA", "Sea-level pressure (MAPS)", "Pa", "hPa", 0.01, 0.0),
    q("PRES", "Pressure", "Pa", "hPa", 0.01, 0.0),
    q("CAPE", "CAPE", "J/kg", "J/kg", 1.0, 0.0),
    q("CIN", "CIN", "J/kg", "J/kg", 1.0, 0.0),
    q(
        "HLCY",
        "Storm-relative helicity",
        "m²/s²",
        "m²/s²",
        1.0,
        0.0,
    ),
    q(
        "MXUPHL",
        "Updraft helicity, max",
        "m²/s²",
        "m²/s²",
        1.0,
        0.0,
    ),
    q("PWAT", "Precipitable water", "kg/m²", "mm", 1.0, 0.0),
    q("REFC", "Composite reflectivity", "dB", "dBZ", 1.0, 0.0),
    q("REFD", "Reflectivity", "dB", "dBZ", 1.0, 0.0),
    q("VIS", "Visibility", "m", "km", 0.001, 0.0),
    q("TCDC", "Total cloud cover", "%", "%", 1.0, 0.0),
    q("LCDC", "Low cloud cover", "%", "%", 1.0, 0.0),
    q("MCDC", "Middle cloud cover", "%", "%", 1.0, 0.0),
    q("HCDC", "High cloud cover", "%", "%", 1.0, 0.0),
    q("HPBL", "Boundary layer height", "m", "m", 1.0, 0.0),
    q("SNOD", "Snow depth", "m", "cm", 100.0, 0.0),
    q("WEASD", "Snow water equivalent", "kg/m²", "mm", 1.0, 0.0),
    // A temperature difference: no offset.
    q("LFTX", "Lifted index", "K", "K", 1.0, 0.0),
    q("4LFTX", "Best lifted index", "K", "K", 1.0, 0.0),
    q(
        "VIL",
        "Vertically integrated liquid",
        "kg/m²",
        "kg/m²",
        1.0,
        0.0,
    ),
    q("RETOP", "Echo top", "m", "km", 0.001, 0.0),
];

/// The vetted quantity for an abbreviation, if there is one.
pub fn vetted(var: &str) -> Option<&'static Quantity> {
    VETTED.iter().find(|q| q.var == var)
}

/// Where a field is, as classified from the `.idx` level text. `Other` keeps the text.
#[derive(Debug, Clone, PartialEq)]
pub enum Level {
    /// An isobaric level, hPa.
    Pressure(f32),
    /// A height above ground, m.
    AboveGround(f32),
    /// A layer between two heights above ground, m, in the file's order.
    LayerAboveGround(f32, f32),
    Surface,
    MeanSeaLevel,
    /// The whole column ("entire atmosphere", with or without "considered as a single layer").
    Column,
    Other(String),
}

impl Level {
    pub fn parse(text: &str) -> Level {
        let t = text.trim();
        let num = |s: &str| s.trim().parse::<f32>().ok();
        if let Some(p) = t.strip_suffix(" mb").and_then(num) {
            return Level::Pressure(p);
        }
        if let Some(rest) = t.strip_suffix(" m above ground") {
            if let Some(h) = num(rest) {
                return Level::AboveGround(h);
            }
            if let Some((a, b)) = rest.split_once('-') {
                if let (Some(a), Some(b)) = (num(a), num(b)) {
                    return Level::LayerAboveGround(a, b);
                }
            }
        }
        match t {
            "surface" => Level::Surface,
            "mean sea level" => Level::MeanSeaLevel,
            "entire atmosphere" | "entire atmosphere (considered as a single layer)" => {
                Level::Column
            }
            _ => Level::Other(t.to_string()),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Level::Pressure(p) => format!("{p} hPa"),
            Level::AboveGround(h) => format!("{h} m AGL"),
            Level::LayerAboveGround(a, b) => format!("{}–{} m AGL", a.min(*b), a.max(*b)),
            Level::Surface => "Surface".into(),
            Level::MeanSeaLevel => "Mean sea level".into(),
            Level::Column => "Whole column".into(),
            Level::Other(t) => t.clone(),
        }
    }

    /// Browser order: surface and near-surface first, then pressure levels from the ground up,
    /// then layers and the rest.
    pub fn order(&self) -> (u8, i64) {
        match self {
            Level::Surface => (0, 0),
            Level::AboveGround(h) => (1, *h as i64),
            Level::MeanSeaLevel => (2, 0),
            Level::Pressure(p) => (3, -(*p as i64)),
            Level::LayerAboveGround(a, b) => (4, (a.max(*b)) as i64),
            Level::Column => (5, 0),
            Level::Other(_) => (6, 0),
        }
    }
}

/// What statistic over what interval, from the `.idx` timing text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Stat {
    Accumulation,
    Max,
    Min,
    Mean,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Timing {
    Analysis,
    /// An instant `minutes` after the run.
    Instant {
        minutes: u32,
    },
    /// A statistic over `from_h..to_h` hours after the run.
    Interval {
        stat: Stat,
        from_h: u32,
        to_h: u32,
    },
    Other(String),
}

impl Timing {
    pub fn parse(text: &str) -> Timing {
        let t = text.trim();
        if t == "anl" {
            return Timing::Analysis;
        }
        if let Some(h) = t
            .strip_suffix(" hour fcst")
            .and_then(|s| s.parse::<u32>().ok())
        {
            return Timing::Instant { minutes: h * 60 };
        }
        if let Some(m) = t
            .strip_suffix(" min fcst")
            .and_then(|s| s.parse::<u32>().ok())
        {
            return Timing::Instant { minutes: m };
        }
        for (suffix, stat) in [
            (" hour acc fcst", Stat::Accumulation),
            (" hour max fcst", Stat::Max),
            (" hour min fcst", Stat::Min),
            (" hour ave fcst", Stat::Mean),
        ] {
            if let Some((a, b)) = t.strip_suffix(suffix).and_then(|s| s.split_once('-')) {
                if let (Ok(from_h), Ok(to_h)) = (a.parse(), b.parse()) {
                    return Timing::Interval { stat, from_h, to_h };
                }
            }
        }
        Timing::Other(t.to_string())
    }

    /// The timing kind a selection keeps across leads: instant, or a statistic over a window of
    /// fixed length, or since the run started. `None` for text this module does not read.
    pub fn kind(&self) -> Option<TimingKind> {
        Some(match *self {
            Timing::Analysis | Timing::Instant { .. } => TimingKind::Instant,
            Timing::Interval {
                stat, from_h: 0, ..
            } => TimingKind::SinceRun(stat),
            Timing::Interval { stat, from_h, to_h } => TimingKind::Window {
                stat,
                hours: to_h.saturating_sub(from_h),
            },
            Timing::Other(_) => return None,
        })
    }
}

/// A timing that means the same thing at every lead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TimingKind {
    Instant,
    /// A statistic over the `hours` before the lead.
    Window {
        stat: Stat,
        hours: u32,
    },
    /// A statistic since the run began (the window grows with the lead).
    SinceRun(Stat),
}

impl TimingKind {
    pub fn label(self) -> String {
        let stat = |s: Stat| match s {
            Stat::Accumulation => "total",
            Stat::Max => "maximum",
            Stat::Min => "minimum",
            Stat::Mean => "mean",
        };
        match self {
            TimingKind::Instant => "instant".into(),
            TimingKind::Window { stat: s, hours } => format!("{hours} h {}", stat(s)),
            TimingKind::SinceRun(s) => format!("{} since the run began", stat(s)),
        }
    }

    /// Whether an `.idx` timing at lead `lead_h` is this kind. Instants match the lead itself;
    /// windows must end at it.
    pub fn matches(self, timing: &Timing, lead_h: u32) -> bool {
        match (self, timing) {
            (TimingKind::Instant, Timing::Analysis) => lead_h == 0,
            (TimingKind::Instant, Timing::Instant { minutes }) => *minutes == lead_h * 60,
            (
                TimingKind::Window { stat, hours },
                Timing::Interval {
                    stat: s,
                    from_h,
                    to_h,
                },
            ) => *s == stat && *to_h == lead_h && to_h.saturating_sub(*from_h) == hours,
            (
                TimingKind::SinceRun(stat),
                Timing::Interval {
                    stat: s,
                    from_h: 0,
                    to_h,
                },
            ) => *s == stat && *to_h == lead_h,
            _ => false,
        }
    }
}

/// One message line of an inventory.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Byte offset of the message in the GRIB2 file.
    pub offset: u64,
    /// Which field of that message this line is (`92.2` is the second, index 1): RAP packs u and
    /// v into one message, and decoding the message reads its first field.
    pub sub: usize,
    /// Where the next distinct message starts; `None` for the last one.
    pub end: Option<u64>,
    pub var: String,
    pub level_text: String,
    pub timing_text: String,
    pub level: Level,
    pub timing: Timing,
}

/// Every message line of an `.idx`, in file order.
pub fn parse_idx(idx: &str) -> Vec<Entry> {
    let rows: Vec<(u64, Vec<&str>)> = idx
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            let offset = f.get(1)?.parse::<u64>().ok()?;
            (f.len() >= 6).then_some((offset, f))
        })
        .collect();
    rows.iter()
        .enumerate()
        .map(|(i, (offset, f))| Entry {
            offset: *offset,
            sub: crate::grib_split::subfield_of(f[0]),
            // The next *distinct* offset: some files list several fields of one message.
            end: rows[i + 1..].iter().map(|(o, _)| *o).find(|o| o > offset),
            var: f[3].to_string(),
            level_text: f[4].to_string(),
            timing_text: f[5].to_string(),
            level: Level::parse(f[4]),
            timing: Timing::parse(f[5]),
        })
        .collect()
}

/// A field a model file offers, with its vetted quantity when there is one.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub entry: Entry,
    pub quantity: Option<&'static Quantity>,
}

impl Field {
    pub fn supported(&self) -> bool {
        self.quantity.is_some() && self.entry.timing.kind().is_some()
    }

    /// Why it cannot be shown, in words; `None` when it can.
    pub fn unsupported_reason(&self) -> Option<&'static str> {
        if self.quantity.is_none() {
            Some("no vetted units for this parameter")
        } else if self.entry.timing.kind().is_none() {
            Some("timing not understood")
        } else {
            None
        }
    }

    /// "Temperature · 500 hPa · instant (°C)", or the raw line for an unsupported one.
    pub fn label(&self) -> String {
        match (self.quantity, self.entry.timing.kind()) {
            (Some(q), Some(k)) => format!(
                "{} \u{b7} {} \u{b7} {} ({})",
                q.name,
                self.entry.level.label(),
                k.label(),
                q.unit
            ),
            _ => format!(
                "{} \u{b7} {} \u{b7} {}",
                self.entry.var, self.entry.level_text, self.entry.timing_text
            ),
        }
    }

    /// Whether `other` measures the same thing, so the two can be compared or differenced:
    /// same quantity, same level, same kind of interval of the same length.
    pub fn comparable(&self, other: &Field) -> Result<(), String> {
        let (Some(a), Some(b)) = (self.quantity, other.quantity) else {
            return Err("an unsupported field cannot be compared".into());
        };
        if a.var != b.var {
            return Err(format!("{} is not {}", a.name, b.name));
        }
        if self.entry.level != other.entry.level {
            return Err(format!(
                "{} is not {}",
                self.entry.level.label(),
                other.entry.level.label()
            ));
        }
        match (self.entry.timing.kind(), other.entry.timing.kind()) {
            (Some(x), Some(y)) if x == y => Ok(()),
            (Some(x), Some(y)) => Err(format!("{} is not {}", x.label(), y.label())),
            _ => Err("timing not understood".into()),
        }
    }
}

/// Every field in an inventory, supported ones first (by quantity, then level), unsupported
/// ones after in file order. One per `(parameter, level, timing)`.
pub fn discover(idx: &str) -> Vec<Field> {
    let mut seen = std::collections::HashSet::new();
    let mut fields: Vec<Field> = parse_idx(idx)
        .into_iter()
        .filter(|e| seen.insert((e.var.clone(), e.level_text.clone(), e.timing_text.clone())))
        .map(|entry| Field {
            quantity: vetted(&entry.var),
            entry,
        })
        .collect();
    fields.sort_by(|a, b| {
        let rank = |f: &Field| {
            f.quantity
                .and_then(|q| VETTED.iter().position(|v| v.var == q.var))
                .filter(|_| f.supported())
                .unwrap_or(usize::MAX)
        };
        rank(a)
            .cmp(&rank(b))
            .then(a.entry.level.order().cmp(&b.entry.level.order()))
    });
    fields
}

/// The levels at which both components of a vector are present with the same timing, as
/// `(u field, v field)` pairs: the input a wind barb or vector layer needs.
pub fn vector_pairs(fields: &[Field]) -> Vec<(&Field, &Field)> {
    fields
        .iter()
        .filter(|f| {
            matches!(
                f.quantity.and_then(|q| q.component),
                Some((Component::U, _))
            )
        })
        .filter_map(|u| {
            let partner = u.quantity?.component?.1;
            let v = fields.iter().find(|v| {
                v.entry.var == partner
                    && v.entry.level == u.entry.level
                    && v.entry.timing == u.entry.timing
            })?;
            Some((u, v))
        })
        .collect()
}

/// The message for `var` at `level_text` whose timing is `kind` at lead `lead_h`: its byte range
/// and which field of the message it is (see [`Entry::sub`]).
pub fn find(
    entries: &[Entry],
    var: &str,
    level_text: &str,
    kind: TimingKind,
    lead_h: u32,
) -> Option<(u64, Option<u64>, usize)> {
    entries
        .iter()
        .find(|e| e.var == var && e.level_text == level_text && kind.matches(&e.timing, lead_h))
        .map(|e| (e.offset, e.end, e.sub))
}

/// One field of a downloaded message: the message itself for the first, a rebuilt single-field
/// message for a later one (`crate::grib_split`).
pub fn field_bytes(message: Vec<u8>, sub: usize) -> anyhow::Result<Vec<u8>> {
    if sub == 0 {
        return Ok(message);
    }
    crate::grib_split::extract_field(&message, sub)
        .ok_or_else(|| anyhow::anyhow!("no field {} in that message", sub + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real lines from `noaa-hrrr-bdp-pds` `hrrr.t23z.wrfsfcf06.grib2.idx` (2026-10-06 23Z run),
    /// selected lines, each unedited (the ranges tested are between lines that are adjacent there).
    const HRRR: &str = "\
1:0:d=2026100623:REFC:entire atmosphere:6 hour fcst:
2:432252:d=2026100623:RETOP:cloud top:6 hour fcst:
3:634656:d=2026100623:var discipline=0 center=7 local_table=1 parmcat=16 parm=201:entire atmosphere:6 hour fcst:
4:1050116:d=2026100623:VIL:entire atmosphere:6 hour fcst:
5:1328000:d=2026100623:VIS:surface:6 hour fcst:
9:3382458:d=2026100623:GUST:surface:6 hour fcst:
10:4673429:d=2026100623:UGRD:250 mb:6 hour fcst:
11:5438389:d=2026100623:VGRD:250 mb:6 hour fcst:
14:7663922:d=2026100623:HGT:500 mb:6 hour fcst:
15:8363724:d=2026100623:TMP:500 mb:6 hour fcst:
16:8915960:d=2026100623:DPT:500 mb:6 hour fcst:
17:9871397:d=2026100623:UGRD:500 mb:6 hour fcst:
18:10463171:d=2026100623:VGRD:500 mb:6 hour fcst:
71:42985001:d=2026100623:TMP:2 m above ground:6 hour fcst:
72:44241964:d=2026100623:POT:2 m above ground:6 hour fcst:
73:45501454:d=2026100623:SPFH:2 m above ground:6 hour fcst:
74:47023412:d=2026100623:DPT:2 m above ground:6 hour fcst:
75:48197565:d=2026100623:RH:2 m above ground:6 hour fcst:
77:50622323:d=2026100623:UGRD:10 m above ground:6 hour fcst:
78:53003938:d=2026100623:VGRD:10 m above ground:6 hour fcst:
79:55385553:d=2026100623:WIND:10 m above ground:5-6 hour max fcst:
83:59582344:d=2026100623:PRATE:surface:6 hour fcst:
84:59639163:d=2026100623:APCP:surface:0-6 hour acc fcst:
85:60001030:d=2026100623:WEASD:surface:0-6 hour acc fcst:
90:60026981:d=2026100623:APCP:surface:5-6 hour acc fcst:
91:60221554:d=2026100623:WEASD:surface:5-6 hour acc fcst:
134:90727426:d=2026100623:HLCY:3000-0 m above ground:6 hour fcst:
";

    /// Real lines from `noaa-gfs-bdp-pds` `gfs.t12z.pgrb2.0p25.f000.idx` (2026-10-06).
    const GFS: &str = "\
1:0:d=2026100612:PRMSL:mean sea level:anl:
2:1000695:d=2026100612:CLMR:1 hybrid level:anl:
580:411843963:d=2026100612:TMP:2 m above ground:anl:
581:412356047:d=2026100612:SPFH:2 m above ground:anl:
";

    #[test]
    fn levels_and_timings_read_as_the_file_names_them() {
        let e = parse_idx(HRRR);
        let by = |var: &str, level: &str| {
            e.iter()
                .find(|x| x.var == var && x.level_text == level)
                .unwrap()
        };
        assert_eq!(by("TMP", "500 mb").level, Level::Pressure(500.0));
        assert_eq!(by("TMP", "2 m above ground").level, Level::AboveGround(2.0));
        assert_eq!(
            by("HLCY", "3000-0 m above ground").level,
            Level::LayerAboveGround(3000.0, 0.0)
        );
        assert_eq!(by("VIL", "entire atmosphere").level, Level::Column);
        assert_eq!(
            by("RETOP", "cloud top").level,
            Level::Other("cloud top".into())
        );
        assert_eq!(
            by("REFC", "entire atmosphere").timing,
            Timing::Instant { minutes: 360 }
        );
        assert_eq!(
            by("WIND", "10 m above ground").timing,
            Timing::Interval {
                stat: Stat::Max,
                from_h: 5,
                to_h: 6
            }
        );
        let apcp: Vec<_> = e.iter().filter(|x| x.var == "APCP").collect();
        assert_eq!(
            apcp[0].timing.kind(),
            Some(TimingKind::SinceRun(Stat::Accumulation))
        );
        assert_eq!(
            apcp[1].timing.kind(),
            Some(TimingKind::Window {
                stat: Stat::Accumulation,
                hours: 1
            })
        );
        let g = parse_idx(GFS);
        assert_eq!(g[0].timing, Timing::Analysis);
        assert_eq!(g[0].level, Level::MeanSeaLevel);
        // Byte ranges end at the next distinct offset; the last runs to the end of the file.
        assert_eq!((e[0].offset, e[0].end), (0, Some(432_252)));
        assert_eq!(e.last().unwrap().end, None);
    }

    #[test]
    fn only_vetted_parameters_get_units_and_local_codes_stay_unsupported() {
        let fields = discover(HRRR);
        let local = fields
            .iter()
            .find(|f| f.entry.var.starts_with("var "))
            .unwrap();
        assert!(!local.supported());
        assert_eq!(
            local.unsupported_reason(),
            Some("no vetted units for this parameter")
        );
        let pot = fields.iter().find(|f| f.entry.var == "POT").unwrap();
        assert!(
            pot.quantity.is_none(),
            "not vetted here, so no guessed unit"
        );
        // Supported ones come first, unsupported after.
        let first_unsupported = fields.iter().position(|f| !f.supported()).unwrap();
        assert!(fields[first_unsupported..].iter().all(|f| !f.supported()));
        let t500 = fields
            .iter()
            .find(|f| f.entry.var == "TMP" && f.entry.level == Level::Pressure(500.0))
            .unwrap();
        assert_eq!(
            t500.label(),
            "Temperature \u{b7} 500 hPa \u{b7} instant (°C)"
        );
        let q = t500.quantity.unwrap();
        assert!((q.to_display(273.15) - 0.0).abs() < 1e-4);
        assert!((vetted("HGT").unwrap().to_display(5640.0) - 564.0).abs() < 1e-3);
        assert!((vetted("PRMSL").unwrap().to_display(101_325.0) - 1013.25).abs() < 1e-3);
        // A lifted index is a difference: no offset.
        assert_eq!(vetted("LFTX").unwrap().offset, 0.0);
    }

    #[test]
    fn comparisons_need_the_same_quantity_level_and_interval() {
        let fields = discover(HRRR);
        let get = |var: &str, level: &str, timing: &str| {
            fields
                .iter()
                .find(|f| {
                    f.entry.var == var
                        && f.entry.level_text == level
                        && f.entry.timing_text == timing
                })
                .unwrap()
        };
        let total = get("APCP", "surface", "0-6 hour acc fcst");
        let hourly = get("APCP", "surface", "5-6 hour acc fcst");
        assert!(total.comparable(total).is_ok());
        let why = total.comparable(hourly).unwrap_err();
        assert!(why.contains("1 h total"), "{why}");
        let t2 = get("TMP", "2 m above ground", "6 hour fcst");
        let t500 = get("TMP", "500 mb", "6 hour fcst");
        assert!(t2.comparable(t500).unwrap_err().contains("500 hPa"));
        assert!(t2
            .comparable(get("DPT", "2 m above ground", "6 hour fcst"))
            .is_err());
    }

    #[test]
    fn wind_components_pair_at_their_shared_levels_and_a_message_is_found_by_kind() {
        let fields = discover(HRRR);
        let pairs = vector_pairs(&fields);
        let mut levels: Vec<String> = pairs.iter().map(|(u, _)| u.entry.level.label()).collect();
        levels.sort();
        assert_eq!(levels, ["10 m AGL", "250 hPa", "500 hPa"]);
        let e = parse_idx(HRRR);
        assert_eq!(
            find(
                &e,
                "APCP",
                "surface",
                TimingKind::SinceRun(Stat::Accumulation),
                6
            ),
            Some((59_639_163, Some(60_001_030), 0))
        );
        assert_eq!(
            find(
                &e,
                "APCP",
                "surface",
                TimingKind::Window {
                    stat: Stat::Accumulation,
                    hours: 1
                },
                6
            ),
            Some((60_026_981, Some(60_221_554), 0))
        );
        // The wrong lead finds nothing rather than another hour's field.
        assert_eq!(find(&e, "TMP", "500 mb", TimingKind::Instant, 5), None);
        assert_eq!(
            find(
                &parse_idx(GFS),
                "PRMSL",
                "mean sea level",
                TimingKind::Instant,
                0
            ),
            Some((0, Some(1_000_695), 0))
        );
    }
}
