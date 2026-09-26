//! Tropical-cyclone model guidance ("spaghetti") and best tracks from NHC's ATCF files.
//!
//! Every storm and invest NHC tracks has an *a-deck* (`aid_public/a{id}.dat.gz`): one line per
//! model, cycle and forecast hour, from the global models (GFS, UKMET, Canadian, NAVGEM) through
//! the hurricane models (HAFS, COAMPS-TC), the consensus aids, the ensemble means and every GEFS
//! member, to the statistical tracks (BAM, CLIPER). Its *b-deck* (`btk/b{id}.dat`) is the best
//! track: where the system has been, six-hourly, with its wind and pressure.
//!
//! Which storms are active is read off the a-deck directory itself: a file NHC wrote in the last
//! day belongs to a system it is still running models on. That catches invests (AL90–AL99), which
//! the advisory feed (`CurrentStorms.json`) never lists but which is where spaghetti matters most.
//!
//! The format is fixed-order comma-separated fields: basin, number, cycle (`YYYYMMDDHH`), model
//! number, model id, forecast hour, latitude and longitude in tenths with a hemisphere letter,
//! wind (kt), pressure (mb), then wind radii and more, several lines per hour (one per radii
//! threshold). Missing values are zero.

use crate::alerts::USER_AGENT;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

const ADECK_DIR: &str = "https://ftp.nhc.noaa.gov/atcf/aid_public/";
const BDECK_DIR: &str = "https://ftp.nhc.noaa.gov/atcf/btk/";

/// An a-deck NHC wrote within this long is a system still being modeled.
pub const ACTIVE_HOURS: i64 = 24;
/// A model run older than the newest cycle by more than this is left out: its storm has moved
/// on. Late models (GFS, HAFS) arrive one cycle behind the early aids, so six hours is too tight.
pub const RUN_MAX_AGE_HOURS: i64 = 18;

/// What kind of guidance a model is. Groups are what the picker toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    /// NHC's own forecast.
    Official,
    /// Averages of several models (TVCN, HCCA): historically the best single track.
    Consensus,
    /// Global models: GFS, ECMWF, UKMET, Canadian, NAVGEM, JMA.
    Global,
    /// Regional hurricane models: HAFS-A/B, HWRF, HMON, COAMPS-TC.
    Hurricane,
    /// Ensemble means (GEFS, Canadian, ECMWF, UKMET ensembles).
    EnsembleMean,
    /// Individual ensemble members.
    EnsembleMember,
    /// Statistical and trajectory tracks: BAM, trajectory, CLIPER, extrapolation.
    Statistical,
    /// Intensity-only aids (SHIPS, LGEM) whose track just copies the official forecast; and
    /// anything not in the catalog. Never drawn as tracks.
    Other,
}

impl Group {
    pub const ALL: [Group; 8] = [
        Group::Official,
        Group::Consensus,
        Group::Global,
        Group::Hurricane,
        Group::EnsembleMean,
        Group::EnsembleMember,
        Group::Statistical,
        Group::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Official => "NHC official",
            Group::Consensus => "Consensus",
            Group::Global => "Global models",
            Group::Hurricane => "Hurricane models",
            Group::EnsembleMean => "Ensemble means",
            Group::EnsembleMember => "Ensemble members",
            Group::Statistical => "Statistical / trajectory",
            Group::Other => "Intensity aids & other",
        }
    }

    /// On until the user says otherwise: the groups a forecaster actually reads a spaghetti plot
    /// for. The official track is already on the map (the NHC track), members are clutter until
    /// asked for, and statistical tracks are baselines.
    pub fn default_on(self) -> bool {
        matches!(
            self,
            Group::Consensus | Group::Global | Group::Hurricane | Group::EnsembleMean
        )
    }
}

/// A model's name, group and line color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelInfo {
    pub name: &'static str,
    pub group: Group,
    pub rgb: [u8; 3],
    /// An "early" (interpolated) aid: the previous cycle of a late model, shifted to start at the
    /// current position. What NHC forecasters use at forecast time; shown only on request.
    pub interpolated: bool,
}

const fn info(name: &'static str, group: Group, rgb: [u8; 3], interpolated: bool) -> ModelInfo {
    ModelInfo {
        name,
        group,
        rgb,
        interpolated,
    }
}

/// Look a model id up in the catalog. Unknown ids come back in [`Group::Other`].
pub fn model(tech: &str) -> ModelInfo {
    use Group::*;
    let t = tech.trim();
    // Ensemble members: two letters and a number.
    if t.len() == 4 && t[2..].chars().all(|c| c.is_ascii_digit()) {
        let fam = match &t[..2] {
            "AP" | "AC" => Some(("GEFS member", [120, 170, 255])),
            "CP" | "CC" => Some(("Canadian ens. member", [255, 150, 150])),
            "EP" | "EE" | "EN" => Some(("ECMWF ens. member", [120, 230, 160])),
            "UE" => Some(("UKMET ens. member", [230, 190, 120])),
            "NP" => Some(("NAVGEM ens. member", [200, 160, 230])),
            _ => None,
        };
        if let Some((name, rgb)) = fam {
            return info(name, EnsembleMember, rgb, false);
        }
    }
    match t {
        "OFCL" => info("NHC official", Official, [255, 255, 255], false),
        "OFCI" => info("NHC official (interp.)", Official, [255, 255, 255], true),
        "TVCN" => info("TVCN consensus", Consensus, [255, 105, 180], false),
        "TVCA" => info("TVCA consensus", Consensus, [255, 105, 180], false),
        "TVCE" => info("TVCE consensus", Consensus, [255, 105, 180], false),
        "TVCX" => info("TVCX consensus", Consensus, [255, 105, 180], false),
        "IVCN" => info("IVCN intensity consensus", Other, [200, 200, 200], false),
        "HCCA" => info("HCCA corrected consensus", Consensus, [255, 60, 130], false),
        "RVCN" => info("RVCN consensus", Consensus, [240, 130, 200], false),
        "GFEX" => info("GFS+ECMWF consensus", Consensus, [220, 90, 255], false),
        "AVNO" => info("GFS", Global, [60, 140, 255], false),
        "AVNI" | "AVN2" => info("GFS (interp.)", Global, [60, 140, 255], true),
        "EMX" | "ECMF" => info("ECMWF", Global, [40, 210, 110], false),
        "EMXI" | "EMX2" => info("ECMWF (interp.)", Global, [40, 210, 110], true),
        "EGRR" | "UKX" | "UKM" => info("UKMET", Global, [240, 200, 60], false),
        "UKXI" | "UKX2" | "EGRI" | "EGR2" | "UKMI" => {
            info("UKMET (interp.)", Global, [240, 200, 60], true)
        }
        "CMC" => info("Canadian (CMC)", Global, [240, 80, 70], false),
        "CMCI" | "CMC2" => info("Canadian (interp.)", Global, [240, 80, 70], true),
        "NVGM" | "NGX" => info("NAVGEM", Global, [180, 120, 240], false),
        "NVGI" | "NVG2" | "NGXI" | "NGX2" => {
            info("NAVGEM (interp.)", Global, [180, 120, 240], true)
        }
        "JGSM" => info("JMA", Global, [150, 110, 80], false),
        "JGSI" | "JGS2" => info("JMA (interp.)", Global, [150, 110, 80], true),
        "HFSA" => info("HAFS-A", Hurricane, [255, 150, 30], false),
        "HFAI" | "HFA2" => info("HAFS-A (interp.)", Hurricane, [255, 150, 30], true),
        "HFSB" => info("HAFS-B", Hurricane, [255, 215, 120], false),
        "HFBI" | "HFB2" => info("HAFS-B (interp.)", Hurricane, [255, 215, 120], true),
        "HWRF" => info("HWRF", Hurricane, [255, 120, 60], false),
        "HWFI" | "HWF2" => info("HWRF (interp.)", Hurricane, [255, 120, 60], true),
        "HMON" => info("HMON", Hurricane, [230, 170, 90], false),
        "HMNI" | "HMN2" => info("HMON (interp.)", Hurricane, [230, 170, 90], true),
        "CTCX" => info("COAMPS-TC", Hurricane, [110, 220, 230], false),
        "CTCI" | "CTC2" => info("COAMPS-TC (interp.)", Hurricane, [110, 220, 230], true),
        "AEMN" => info("GEFS mean", EnsembleMean, [120, 170, 255], false),
        "AEMI" | "AEM2" => info("GEFS mean (interp.)", EnsembleMean, [120, 170, 255], true),
        "CEMN" => info("Canadian ens. mean", EnsembleMean, [255, 150, 150], false),
        "CEMI" | "CEM2" => info(
            "Canadian ens. mean (interp.)",
            EnsembleMean,
            [255, 150, 150],
            true,
        ),
        "EEMN" => info("ECMWF ens. mean", EnsembleMean, [120, 230, 160], false),
        "EEMI" | "EEM2" => info(
            "ECMWF ens. mean (interp.)",
            EnsembleMean,
            [120, 230, 160],
            true,
        ),
        "UEMN" => info("UKMET ens. mean", EnsembleMean, [230, 190, 120], false),
        "UEMI" | "UEM2" => info(
            "UKMET ens. mean (interp.)",
            EnsembleMean,
            [230, 190, 120],
            true,
        ),
        "BAMS" => info("BAM shallow", Statistical, [170, 170, 170], false),
        "BAMM" => info("BAM medium", Statistical, [150, 150, 150], false),
        "BAMD" => info("BAM deep", Statistical, [130, 130, 130], false),
        "TABS" => info("Trajectory shallow", Statistical, [170, 170, 170], false),
        "TABM" => info("Trajectory medium", Statistical, [150, 150, 150], false),
        "TABD" => info("Trajectory deep", Statistical, [130, 130, 130], false),
        "LBAR" => info("LBAR", Statistical, [190, 190, 150], false),
        "XTRP" => info("Extrapolation", Statistical, [200, 200, 200], false),
        "CLP5" | "TCLP" => info("CLIPER", Statistical, [160, 160, 190], false),
        "OCD5" => info(
            "Climatology & persistence",
            Statistical,
            [160, 160, 190],
            false,
        ),
        "SHIP" => info("SHIPS", Other, [200, 200, 200], false),
        "DSHP" => info("Decay-SHIPS", Other, [200, 200, 200], false),
        "LGEM" => info("LGEM", Other, [200, 200, 200], false),
        _ => info("", Other, [180, 180, 180], false),
    }
}

/// One forecast position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AidPoint {
    /// Forecast hour.
    pub tau: u16,
    pub lat: f64,
    pub lon: f64,
    pub vmax_kt: Option<f32>,
    pub mslp_mb: Option<f32>,
}

/// One model run for one storm.
#[derive(Debug, Clone, PartialEq)]
pub struct Aid {
    pub tech: String,
    pub cycle: DateTime<Utc>,
    pub points: Vec<AidPoint>,
}

impl Aid {
    pub fn info(&self) -> ModelInfo {
        model(&self.tech)
    }

    /// The position at a forecast hour, interpolated between the hours the model reported.
    pub fn at_tau(&self, tau: f64) -> Option<(f64, f64)> {
        let i = self.points.iter().position(|p| f64::from(p.tau) >= tau)?;
        let b = self.points[i];
        if f64::from(b.tau) == tau || i == 0 {
            return (f64::from(b.tau) == tau).then_some((b.lat, b.lon));
        }
        let a = self.points[i - 1];
        let f = (tau - f64::from(a.tau)) / f64::from(b.tau - a.tau);
        Some((a.lat + (b.lat - a.lat) * f, a.lon + (b.lon - a.lon) * f))
    }
}

/// One best-track fix.
#[derive(Debug, Clone, PartialEq)]
pub struct BestPoint {
    pub time: DateTime<Utc>,
    pub lat: f64,
    pub lon: f64,
    pub vmax_kt: Option<f32>,
    pub mslp_mb: Option<f32>,
    /// Development stage: TD, TS, HU, EX (extratropical), LO (low), DB (disturbance), SS/SD
    /// (subtropical), WV (wave).
    pub stage: String,
}

/// A storm or invest with its guidance.
#[derive(Debug, Clone, PartialEq)]
pub struct Guidance {
    /// ATCF id, lower case (`al062026`).
    pub id: String,
    /// The storm's name from the best track (`FAY`), or `INVEST`.
    pub name: String,
    /// The latest run of each model, newest cycle first within no model.
    pub aids: Vec<Aid>,
    pub best: Vec<BestPoint>,
}

impl Guidance {
    /// `AL06` / `EP94`: basin and number, the way forecasters say it.
    pub fn short_id(&self) -> String {
        self.id.get(..4).unwrap_or(&self.id).to_ascii_uppercase()
    }

    /// Invests are numbered 90–99.
    pub fn is_invest(&self) -> bool {
        self.id
            .get(2..4)
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (90..=99).contains(&n))
    }

    /// `Fay (AL06)`, `Invest 98L`.
    pub fn title(&self) -> String {
        let n = self.id.get(2..4).unwrap_or("");
        let basin = match self.id.get(..2) {
            Some("al") => "L",
            Some("ep") => "E",
            Some("cp") => "C",
            Some("wp") => "W",
            _ => "",
        };
        if self.is_invest() {
            format!("Invest {n}{basin}")
        } else {
            let mut name = self.name.to_ascii_lowercase();
            if let Some(c) = name.get_mut(..1) {
                c.make_ascii_uppercase();
            }
            if name.is_empty() || name == "invest" {
                self.short_id()
            } else {
                format!("{name} ({})", self.short_id())
            }
        }
    }

    /// The newest best-track fix.
    pub fn latest_fix(&self) -> Option<&BestPoint> {
        self.best.last()
    }

    /// The newest model cycle.
    pub fn newest_cycle(&self) -> Option<DateTime<Utc>> {
        self.aids.iter().map(|a| a.cycle).max()
    }
}

fn cycle_time(s: &str) -> Option<DateTime<Utc>> {
    let n = NaiveDateTime::parse_from_str(&format!("{}00", s.trim()), "%Y%m%d%H%M").ok()?;
    Some(Utc.from_utc_datetime(&n))
}

/// `298N` → 29.8, `426W` → −42.6. Longitudes past the dateline stay continuous (`1795E` is
/// 179.5, `1805W`... is not written; NHC wraps to `1795E`), so a track crossing it jumps; callers
/// that draw lines unwrap with [`unwrap_lon`].
fn coord(s: &str) -> Option<f64> {
    let s = s.trim();
    let (num, hemi) = s.split_at(s.len().checked_sub(1)?);
    let v = num.parse::<f64>().ok()? / 10.0;
    match hemi {
        "N" | "E" => Some(v),
        "S" | "W" => Some(-v),
        _ => None,
    }
}

/// Shift `lon` by whole turns to sit within 180° of `prev`, so a line crossing the dateline
/// does not wrap the globe.
pub fn unwrap_lon(prev: f64, lon: f64) -> f64 {
    let mut l = lon;
    while l - prev > 180.0 {
        l -= 360.0;
    }
    while prev - l > 180.0 {
        l += 360.0;
    }
    l
}

/// A physical value or `None`: the decks write 0 for missing.
fn positive(s: Option<&&str>) -> Option<f32> {
    s.and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| *v > 0.0)
}

/// Parse an a-deck, keeping cycles no older than `hours` before the newest one. Each model's
/// lines for a cycle become one [`Aid`] with one point per forecast hour.
pub fn parse_adeck(text: &str, hours: i64) -> Vec<Aid> {
    // Cycles ascend through the file, but not strictly (late models are appended later), so
    // find the newest first.
    let newest = text
        .lines()
        .filter_map(|l| cycle_time(l.split(',').nth(2)?))
        .max();
    let Some(newest) = newest else {
        return Vec::new();
    };
    let since = newest - chrono::Duration::hours(hours);
    let mut runs: std::collections::BTreeMap<(String, DateTime<Utc>), Vec<AidPoint>> =
        Default::default();
    for line in text.lines() {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() < 8 {
            continue;
        }
        let Some(cycle) = cycle_time(f[2]) else {
            continue;
        };
        if cycle < since {
            continue;
        }
        let tech = f[4].trim();
        let (Ok(tau), Some(lat), Some(lon)) =
            (f[5].trim().parse::<i32>(), coord(f[6]), coord(f[7]))
        else {
            continue;
        };
        // CARQ carries analysis positions at negative hours; nothing forecasts backwards.
        if tech.is_empty() || !(0..=240).contains(&tau) || (lat == 0.0 && lon == 0.0) {
            continue;
        }
        let pts = runs.entry((tech.to_string(), cycle)).or_default();
        // Several lines per hour (one per wind-radii threshold): the first carries it all.
        if pts.last().is_some_and(|p| p.tau == tau as u16) {
            continue;
        }
        pts.push(AidPoint {
            tau: tau as u16,
            lat,
            lon,
            vmax_kt: positive(f.get(8)),
            mslp_mb: positive(f.get(9)).filter(|p| (850.0..=1050.0).contains(p)),
        });
    }
    runs.into_iter()
        .map(|((tech, cycle), mut points)| {
            points.sort_by_key(|p| p.tau);
            points.dedup_by_key(|p| p.tau);
            Aid {
                tech,
                cycle,
                points,
            }
        })
        .collect()
}

/// The newest run of each model, dropping models whose newest run is more than
/// [`RUN_MAX_AGE_HOURS`] behind the newest cycle, and runs too short to draw.
pub fn latest_runs(aids: Vec<Aid>) -> Vec<Aid> {
    let Some(newest) = aids.iter().map(|a| a.cycle).max() else {
        return Vec::new();
    };
    let mut by_tech: std::collections::BTreeMap<String, Aid> = Default::default();
    for a in aids {
        if a.points.len() < 2 || newest - a.cycle > chrono::Duration::hours(RUN_MAX_AGE_HOURS) {
            continue;
        }
        match by_tech.get(&a.tech) {
            Some(have) if have.cycle >= a.cycle => {}
            _ => {
                by_tech.insert(a.tech.clone(), a);
            }
        }
    }
    by_tech.into_values().collect()
}

/// Parse a b-deck into its fixes, oldest first, and the storm's name.
pub fn parse_bdeck(text: &str) -> (Vec<BestPoint>, String) {
    let mut out: Vec<BestPoint> = Vec::new();
    let mut name = String::new();
    let mut named_at: Option<DateTime<Utc>> = None;
    for line in text.lines() {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() < 11 {
            continue;
        }
        let (Some(time), Some(lat), Some(lon)) = (cycle_time(f[2]), coord(f[6]), coord(f[7]))
        else {
            continue;
        };
        // The name of the newest fix: an invest that became a storm is `INVEST` early on.
        if let Some(n) = f.get(27).map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if named_at.is_none_or(|t| time >= t) {
                name = n.to_string();
                named_at = Some(time);
            }
        }
        // One line per radii threshold, as in the a-deck.
        if out.last().is_some_and(|p| p.time == time) {
            continue;
        }
        out.push(BestPoint {
            time,
            lat,
            lon,
            vmax_kt: positive(f.get(8)),
            mslp_mb: positive(f.get(9)).filter(|p| (850.0..=1050.0).contains(p)),
            stage: f[10].trim().to_string(),
        });
    }
    out.sort_by_key(|p| p.time);
    out.dedup_by_key(|p| p.time);
    (out, name)
}

/// The a-decks in a directory listing written within [`ACTIVE_HOURS`] of `now`, as ATCF ids
/// (`al062026`). The listing is Apache's: `<a href="aal062026.dat.gz">…</a>  2026-09-25 19:47`.
pub fn active_ids(listing: &str, now: DateTime<Utc>) -> Vec<String> {
    let mut out = Vec::new();
    for line in listing.lines() {
        let Some(rest) = line.split("href=\"a").nth(1) else {
            continue;
        };
        let Some(file) = rest.split('"').next() else {
            continue;
        };
        let Some(id) = file.strip_suffix(".dat.gz") else {
            continue;
        };
        // Basin + number + year: `al062026`.
        if id.len() != 8 || !id[..2].chars().all(|c| c.is_ascii_lowercase()) {
            continue;
        }
        // The modified time follows the link text.
        let after = line.rsplit("</a>").next().unwrap_or("").trim();
        let stamp: String = after.chars().take(16).collect();
        let Ok(t) = NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%d %H:%M") else {
            continue;
        };
        let t = Utc.from_utc_datetime(&t);
        if now - t <= chrono::Duration::hours(ACTIVE_HOURS) {
            out.push(id.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

async fn get_bytes(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let r = client
        .get(crate::net::fetch_url(url))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?;
    Ok(r.bytes().await?.to_vec())
}

/// Gunzip when the bytes are gzip (a browser may already have undone the encoding).
fn maybe_gunzip(bytes: Vec<u8>) -> anyhow::Result<String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        let mut s = String::new();
        flate2::read::MultiGzDecoder::new(&bytes[..]).read_to_string(&mut s)?;
        Ok(s)
    } else {
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// Fetch one storm's guidance: the latest run of every model and the best track.
pub async fn fetch_storm(client: &reqwest::Client, id: &str) -> anyhow::Result<Guidance> {
    anyhow::ensure!(
        id.len() == 8 && id.chars().all(|c| c.is_ascii_alphanumeric()),
        "not an ATCF id: {id:?}"
    );
    let id = id.to_ascii_lowercase();
    let adeck = maybe_gunzip(get_bytes(client, &format!("{ADECK_DIR}a{id}.dat.gz")).await?)?;
    let aids = latest_runs(parse_adeck(&adeck, RUN_MAX_AGE_HOURS + 6));
    // A brand-new invest can have models before its first best-track fix.
    let (best, name) = match get_bytes(client, &format!("{BDECK_DIR}b{id}.dat")).await {
        Ok(b) => parse_bdeck(&String::from_utf8_lossy(&b)),
        Err(e) => {
            log::debug!("best track for {id}: {e}");
            (Vec::new(), String::new())
        }
    };
    Ok(Guidance {
        id,
        name,
        aids,
        best,
    })
}

/// Every active storm and invest with its guidance, newest-numbered last. A storm whose files
/// fail to load is skipped rather than failing the rest.
pub async fn fetch_active(client: &reqwest::Client) -> anyhow::Result<Vec<Guidance>> {
    let listing = String::from_utf8_lossy(&get_bytes(client, ADECK_DIR).await?).into_owned();
    let ids = active_ids(&listing, Utc::now());
    let mut out = Vec::new();
    for id in ids {
        match fetch_storm(client, &id).await {
            Ok(g) if !g.aids.is_empty() || !g.best.is_empty() => out.push(g),
            Ok(_) => {}
            Err(e) => log::warn!("tropical guidance for {id}: {e}"),
        }
    }
    Ok(out)
}

/// Great-circle distance in nautical miles.
pub fn distance_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let (dp, dl) = ((lat2 - lat1).to_radians(), (lon2 - lon1).to_radians());
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    3440.065 * 2.0 * a.sqrt().asin()
}

/// How far apart the given runs are at a forecast hour: the mean distance (nm) of each run's
/// position from their centroid. `None` with fewer than three runs reaching that hour, which is
/// too few to call a spread.
pub fn spread_nm(aids: &[&Aid], tau: f64) -> Option<(f64, usize)> {
    let pos: Vec<(f64, f64)> = aids.iter().filter_map(|a| a.at_tau(tau)).collect();
    if pos.len() < 3 {
        return None;
    }
    let n = pos.len() as f64;
    let lat = pos.iter().map(|p| p.0).sum::<f64>() / n;
    let lon0 = pos[0].1;
    let lon = pos.iter().map(|p| unwrap_lon(lon0, p.1)).sum::<f64>() / n;
    let mean = pos
        .iter()
        .map(|p| distance_nm(p.0, unwrap_lon(lon0, p.1), lat, lon))
        .sum::<f64>()
        / n;
    Some((mean, pos.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed a-deck: two GFS cycles (one line per radii threshold), an early aid, a GEFS
    /// member, CARQ's analysis lines, and a line with a missing position.
    const ADECK: &str = "\
AL, 06, 2026092500, 03, AVNO,   0, 290N,  410W,  60,  990, XX,  34, NEQ,   47,   42,   36,   37,
AL, 06, 2026092500, 03, AVNO,  12, 292N,  415W,  60,  990, XX,  34, NEQ,   47,   42,   36,   37,
AL, 06, 2026092512, 03, AVNO,   0, 298N,  426W,  63,  986, XX,  34, NEQ,   47,   42,   36,   37,
AL, 06, 2026092512, 03, AVNO,   0, 298N,  426W,  63,  986, XX,  50, NEQ,   26,   26,   25,   24,
AL, 06, 2026092512, 03, AVNO,   6, 299N,  429W,  61,  994, XX,  34, NEQ,   46,   36,   35,   44,
AL, 06, 2026092512, 03, AVNO,  12, 300N,  433W,   0,    0, XX,  34, NEQ,    0,    0,    0,    0,
AL, 06, 2026092518, 03, AVNI,   0, 298N,  428W,  45, 1004,
AL, 06, 2026092518, 03, AVNI,  12, 296N,  436W,  46, 1003,
AL, 06, 2026092512, 03, AP07,   0, 298N,  426W,  40, 1005,
AL, 06, 2026092512, 03, AP07,  24, 294N,  445W,  38, 1006,
AL, 06, 2026092518, 01, CARQ, -12, 301N,  423W,  50, 1002,
AL, 06, 2026092518, 01, CARQ,   0, 298N,  428W,  45, 1004,
AL, 06, 2026092518, 03, XTRP,   0,    ,     ,   0,    0,
AL, 06, 2026092518, 03, XTRP,  12, 296N,  436W,   0,    0,
AL, 06, 2026092518, 03, XTRP,  24, 294N,  445W,   0,    0,
";

    #[test]
    fn adeck_runs_are_grouped_by_model_and_cycle() {
        let aids = parse_adeck(ADECK, 24);
        let gfs: Vec<&Aid> = aids.iter().filter(|a| a.tech == "AVNO").collect();
        assert_eq!(gfs.len(), 2, "two cycles");
        let g12 = gfs
            .iter()
            .find(|a| a.cycle.format("%H").to_string() == "12")
            .unwrap();
        let taus: Vec<u16> = g12.points.iter().map(|p| p.tau).collect();
        assert_eq!(
            taus,
            [0, 6, 12],
            "one point per hour despite the radii lines"
        );
        assert_eq!(g12.points[0].lat, 29.8);
        assert_eq!(g12.points[0].lon, -42.6);
        assert_eq!(g12.points[0].vmax_kt, Some(63.0));
        assert_eq!(g12.points[2].vmax_kt, None, "zero is missing");
        assert_eq!(g12.points[2].mslp_mb, None);
        // CARQ's negative hour is dropped; the position-less XTRP line too.
        let carq = aids.iter().find(|a| a.tech == "CARQ").unwrap();
        assert_eq!(carq.points.len(), 1);
        assert_eq!(
            aids.iter().find(|a| a.tech == "XTRP").unwrap().points.len(),
            2
        );
        // A 12-hour window drops the 00Z cycle.
        assert!(parse_adeck(ADECK, 12)
            .iter()
            .all(|a| a.cycle.format("%H").to_string() != "00"));
        assert!(parse_adeck("", 24).is_empty());
    }

    #[test]
    fn latest_runs_keep_each_models_newest_drawable_run() {
        let runs = latest_runs(parse_adeck(ADECK, 48));
        let techs: Vec<&str> = runs.iter().map(|a| a.tech.as_str()).collect();
        assert_eq!(
            techs,
            ["AP07", "AVNI", "AVNO", "XTRP"],
            "CARQ has one point"
        );
        let gfs = runs.iter().find(|a| a.tech == "AVNO").unwrap();
        assert_eq!(gfs.cycle.format("%d%H").to_string(), "2512");
        // Interpolated between reported hours.
        let (lat, lon) = gfs.at_tau(9.0).unwrap();
        assert!(
            (lat - 29.95).abs() < 1e-9 && (lon - -43.1).abs() < 1e-9,
            "{lat} {lon}"
        );
        assert_eq!(gfs.at_tau(12.0), Some((30.0, -43.3)));
        assert_eq!(gfs.at_tau(48.0), None);
    }

    #[test]
    fn the_catalog_names_and_groups_models() {
        assert_eq!(model("AVNO").name, "GFS");
        assert_eq!(model("AVNO").group, Group::Global);
        assert!(model("AVNI").interpolated && !model("AVNO").interpolated);
        assert_eq!(model("HFSA").group, Group::Hurricane);
        assert_eq!(model("TVCN").group, Group::Consensus);
        assert_eq!(model("AP17").group, Group::EnsembleMember);
        assert_eq!(model("AC00").name, "GEFS member");
        assert_eq!(model("SHIP").group, Group::Other);
        assert_eq!(model("ZZZZ").group, Group::Other);
        assert_eq!(model("AEMN").group, Group::EnsembleMean);
        assert!(Group::Global.default_on() && !Group::EnsembleMember.default_on());
    }

    #[test]
    fn best_track_reads_fixes_and_name() {
        let b = "\
AL, 06, 2026092512,   , BEST,   0, 301N,  423W,  50, 1002, TS,  34, NEQ,   70,   20,   20,   60, 1018,  130,  20,  60,   0,   L,   0,    ,   0,   0,        FAY, M,
AL, 06, 2026092512,   , BEST,   0, 301N,  423W,  50, 1002, TS,  50, NEQ,   20,    0,    0,   20, 1018,  130,  20,  60,   0,   L,   0,    ,   0,   0,        FAY, M,
AL, 06, 2026092506,   , BEST,   0, 303N,  418W,  55, 1000, TS,  34, NEQ,   70,   20,   20,   60, 1018,  130,  20,  60,   0,   L,   0,    ,   0,   0,     INVEST, M,
AL, 06, 2026092518,   , BEST,   0, 298N,  428W,  45, 1004, TS,  34, NEQ,   70,   20,   20,   60,
";
        let (pts, name) = parse_bdeck(b);
        assert_eq!(name, "FAY");
        assert_eq!(pts.len(), 3);
        assert_eq!(
            pts[0].time.format("%d%H").to_string(),
            "2506",
            "oldest first"
        );
        assert_eq!(pts[2].lat, 29.8);
        assert_eq!(pts[1].stage, "TS");
        assert_eq!(pts[1].vmax_kt, Some(50.0));
    }

    #[test]
    fn active_ids_come_from_recent_files() {
        let listing = r#"<a href="aal062026.dat.gz">aal062026.dat.gz</a>        2026-09-25 19:47  443K
<a href="aal982026.dat.gz">aal982026.dat.gz</a>        2026-09-18 06:54   34K
<a href="aep952026.dat.gz">aep952026.dat.gz</a>        2026-09-25 01:10   20K
<a href="storm_list.txt">storm_list.txt</a>        2026-09-25 19:47   1K
<a href="aal062026.dat">aal062026.dat</a>        2026-09-25 19:47   4M"#;
        let now = "2026-09-26T00:00:00Z".parse().unwrap();
        assert_eq!(active_ids(listing, now), ["al062026", "ep952026"]);
    }

    #[test]
    fn titles_read_like_nhc() {
        let g = |id: &str, name: &str| Guidance {
            id: id.into(),
            name: name.into(),
            aids: vec![],
            best: vec![],
        };
        assert_eq!(g("al062026", "FAY").title(), "Fay (AL06)");
        assert_eq!(g("al982026", "INVEST").title(), "Invest 98L");
        assert!(g("ep952026", "").is_invest());
        assert_eq!(g("ep152026", "").title(), "EP15");
    }

    #[test]
    fn coordinates_and_spread() {
        assert_eq!(coord("298N"), Some(29.8));
        assert_eq!(coord(" 1795E"), Some(179.5));
        assert_eq!(coord("12S"), Some(-1.2));
        assert_eq!(coord(""), None);
        assert_eq!(unwrap_lon(179.0, -179.5), 180.5);
        assert!((distance_nm(0.0, 0.0, 1.0, 0.0) - 60.04).abs() < 0.1);
        let run = |lat: f64| Aid {
            tech: "X".into(),
            cycle: Utc::now(),
            points: vec![
                AidPoint {
                    tau: 0,
                    lat: 20.0,
                    lon: -60.0,
                    vmax_kt: None,
                    mslp_mb: None,
                },
                AidPoint {
                    tau: 72,
                    lat,
                    lon: -70.0,
                    vmax_kt: None,
                    mslp_mb: None,
                },
            ],
        };
        let (a, b, c) = (run(25.0), run(26.0), run(27.0));
        let (nm, n) = spread_nm(&[&a, &b, &c], 72.0).unwrap();
        assert_eq!(n, 3);
        assert!((nm - 40.0).abs() < 1.0, "{nm}");
        assert!(spread_nm(&[&a, &b], 72.0).is_none());
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_live_guidance() {
        let client = reqwest::Client::new();
        let all = fetch_active(&client).await.unwrap();
        for g in &all {
            eprintln!(
                "{}: {} runs, {} fixes, newest {:?}",
                g.title(),
                g.aids.len(),
                g.best.len(),
                g.newest_cycle()
            );
        }
    }
}
