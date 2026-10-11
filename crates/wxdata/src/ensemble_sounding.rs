//! GEFS member soundings at a point, and their spread as an envelope (ROADMAP_NEW F8, 1010.md V10).
//!
//! The point sounding path reads one GRIB message per variable and level from a global file:
//! for 31 members on 19 levels that is over a thousand range requests and hundreds of megabytes
//! per click. NOMADS's GRIB filter cuts a file down to the variables, levels and a small box the
//! request names, so here each member costs two requests of a few kilobytes: the `pgrb2a` file
//! (1000, 925, 850, 700, 500, 250, 200 hPa) and the `pgrb2b` file (the twelve levels between),
//! temperature and relative humidity only. Dew point is derived from RH, as for the RAP.
//!
//! All members come from one cycle: the newest that has posted the control member at the lead
//! that lands on the asked-for valid time, snapped to the GEFS's 3-hour steps (the snap is
//! reported, never hidden). Requests are paced well under NOMADS's per-address limit, so a
//! whole ensemble takes the better part of a minute. NOMADS is not on the browser build's proxy,
//! so this is desktop and Android only.
//!
//! The envelope is the members' lowest and highest temperature and dew point at each level, at
//! the GEFS grid point nearest the click (half a degree). It describes the ensemble's spread,
//! not a probability, and a level fewer than [`MIN_MEMBERS`] members reached is left out.

use chrono::{DateTime, Datelike, Timelike, Utc};

/// Pressure levels in the `pgrb2a` and `pgrb2b` files that carry both TMP and RH, hPa.
pub const LEVELS_A: [u16; 7] = [1000, 925, 850, 700, 500, 250, 200];
pub const LEVELS_B: [u16; 12] = [975, 950, 900, 800, 750, 650, 600, 550, 450, 400, 350, 300];

/// Fewer members than this at a level, and the level is left out of the envelope; fewer in all,
/// and there is no envelope.
pub const MIN_MEMBERS: usize = 20;

/// The GEFS publishes the half-degree members every 3 hours to 240 h.
const LEAD_STEP_H: i64 = 3;
const MAX_LEAD_H: i64 = 240;

/// The space between request starts: two members' pair of files a little under every 1.3 s keeps
/// a full ensemble near 95 requests a minute, under NOMADS's 120.
const PACE: std::time::Duration = std::time::Duration::from_millis(650);

const NOMADS: &str = "https://nomads.ncep.noaa.gov/cgi-bin";

/// `(hPa, temperature °C, dew point °C)` levels, highest pressure first.
pub type Levels = Vec<(f64, f64, f64)>;

/// One member's profile.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberProfile {
    pub member: u8,
    pub levels: Levels,
}

/// The members' spread at one level.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvelopeLevel {
    pub pressure_hpa: f64,
    pub t_min_c: f64,
    pub t_max_c: f64,
    pub td_min_c: f64,
    pub td_max_c: f64,
    /// How many members have this level.
    pub members: usize,
}

/// The ensemble envelope at a point.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    pub run: DateTime<Utc>,
    pub fh: u16,
    /// The GEFS grid point the profiles are from (`[lon, lat]`, longitude −180..180).
    pub point: [f64; 2],
    /// Members that arrived.
    pub members: usize,
    /// Highest pressure first.
    pub levels: Vec<EnvelopeLevel>,
}

impl Envelope {
    pub fn valid(&self) -> DateTime<Utc> {
        self.run + chrono::Duration::hours(i64::from(self.fh))
    }
}

/// The NOMADS filter request for member `member` (0 the control, 1..=30 the perturbed members)
/// of the `run` cycle at lead `fh`, from the `b` file or the `a` file, cut to a one-degree box
/// around `[lon, lat]`.
pub fn member_url(run: DateTime<Utc>, fh: u16, member: u8, b: bool, lon: f64, lat: f64) -> String {
    let (script, dir, kind, levels): (&str, &str, &str, &[u16]) = if b {
        (
            "filter_gefs_atmos_0p50b.pl",
            "pgrb2bp5",
            "pgrb2b",
            &LEVELS_B,
        )
    } else {
        (
            "filter_gefs_atmos_0p50a.pl",
            "pgrb2ap5",
            "pgrb2a",
            &LEVELS_A,
        )
    };
    let name = if member == 0 {
        "gec00".to_string()
    } else {
        format!("gep{member:02}")
    };
    let hh = run.hour();
    let lev: String = levels.iter().map(|l| format!("&lev_{l}_mb=on")).collect();
    // The filter's box is in 0..360 longitude.
    let east = lon.rem_euclid(360.0);
    format!(
        "{NOMADS}/{script}?dir=%2Fgefs.{:04}{:02}{:02}%2F{hh:02}%2Fatmos%2F{dir}\
         &file={name}.t{hh:02}z.{kind}.0p50.f{fh:03}&var_TMP=on&var_RH=on{lev}\
         &subregion=&leftlon={:.2}&rightlon={:.2}&toplat={:.2}&bottomlat={:.2}",
        run.year(),
        run.month(),
        run.day(),
        (east - 0.5).floor(),
        (east + 0.5).ceil(),
        (lat + 0.5).ceil(),
        (lat - 0.5).floor(),
    )
}

/// The GEFS cycle and lead for a sounding valid at `valid`, from the newest of `runs` that is no
/// later than it: the lead snapped to the nearest 3-hour step, within the 240 h the members run.
/// Each candidate run gives its own lead.
pub fn leads_for(valid: DateTime<Utc>, runs: &[DateTime<Utc>]) -> Vec<(DateTime<Utc>, u16)> {
    runs.iter()
        .filter(|&&run| run <= valid)
        .filter_map(|&run| {
            let minutes = (valid - run).num_minutes();
            let fh = ((minutes as f64 / 60.0 / LEAD_STEP_H as f64).round() as i64) * LEAD_STEP_H;
            (0..=MAX_LEAD_H).contains(&fh).then_some((run, fh as u16))
        })
        .collect()
}

/// One member's profile from the two filter responses: TMP and RH per isobaric level at the grid
/// point nearest `[lon, lat]`, dew point from RH. Also the grid point used.
pub fn profile_from_grib(
    files: &[&[u8]],
    lon: f64,
    lat: f64,
) -> anyhow::Result<(Levels, [f64; 2])> {
    use gribberish::data_message::DataMessage;
    use gribberish::message::read_messages;
    use gribberish::templates::product::tables::FixedSurfaceType;
    let mut temp: std::collections::BTreeMap<i64, f64> = Default::default();
    let mut rh: std::collections::BTreeMap<i64, f64> = Default::default();
    let mut point = None;
    for bytes in files {
        for msg in read_messages(bytes) {
            let Ok(var) = msg.variable_abbrev() else {
                continue;
            };
            let Ok((FixedSurfaceType::IsobaricSurface, Some(pa))) = msg.first_fixed_surface()
            else {
                continue;
            };
            let target = match var.as_str() {
                "TMP" => &mut temp,
                "RH" => &mut rh,
                _ => continue,
            };
            let dm = DataMessage::try_from(&msg)
                .map_err(|e| anyhow::anyhow!("GEFS member decode: {e:?}"))?;
            let (lats, lons) = dm.metadata.latlng();
            // A regular lat/lon grid hands back its two axes, not a coordinate per point:
            // expand them row by row, as `global` does.
            let (lats, lons) = if lats.len() * lons.len() == dm.data.len() {
                lats.iter()
                    .flat_map(|&la| lons.iter().map(move |&lo| (la, lo)))
                    .unzip()
            } else {
                (lats, lons)
            };
            let nearest = (0..dm.data.len().min(lats.len()).min(lons.len()))
                .filter(|&i| dm.data[i].is_finite())
                .min_by(|&i, &j| {
                    let d = |k: usize| {
                        let dlon = (lons[k] - lon + 540.0).rem_euclid(360.0) - 180.0;
                        dlon * dlon + (lats[k] - lat).powi(2)
                    };
                    d(i).total_cmp(&d(j))
                })
                .ok_or_else(|| anyhow::anyhow!("no GEFS grid point near the sounding"))?;
            point.get_or_insert([
                (lons[nearest] + 540.0).rem_euclid(360.0) - 180.0,
                lats[nearest],
            ]);
            // Pa to tenths of hPa, as the key: 85000 Pa is 8500.
            target.insert((pa / 10.0).round() as i64, dm.data[nearest]);
        }
    }
    let mut levels: Levels = temp
        .iter()
        .filter_map(|(&key, &t_k)| {
            let rh = *rh.get(&key)?;
            let t_c = t_k - 273.15;
            Some((
                key as f64 / 10.0,
                t_c,
                crate::sounding::dewpoint_from_rh(t_c, rh),
            ))
        })
        .collect();
    levels.sort_by(|a, b| b.0.total_cmp(&a.0));
    anyhow::ensure!(
        !levels.is_empty(),
        "the GEFS member had no temperature and humidity levels"
    );
    Ok((levels, point.unwrap_or([lon, lat])))
}

/// The members' lowest and highest temperature and dew point at each level reached by at least
/// [`MIN_MEMBERS`] of them, highest pressure first.
pub fn envelope(profiles: &[MemberProfile]) -> Vec<EnvelopeLevel> {
    let mut by_level: std::collections::BTreeMap<i64, Vec<(f64, f64)>> = Default::default();
    for p in profiles {
        for &(hpa, t, td) in &p.levels {
            if t.is_finite() && td.is_finite() {
                by_level
                    .entry((hpa * 10.0).round() as i64)
                    .or_default()
                    .push((t, td));
            }
        }
    }
    let mut out: Vec<EnvelopeLevel> = by_level
        .into_iter()
        .filter(|(_, v)| v.len() >= MIN_MEMBERS)
        .map(|(key, v)| {
            let fold = |f: fn(&(f64, f64)) -> f64, pick: fn(f64, f64) -> f64, start: f64| {
                v.iter().map(f).fold(start, pick)
            };
            EnvelopeLevel {
                pressure_hpa: key as f64 / 10.0,
                t_min_c: fold(|x| x.0, f64::min, f64::MAX),
                t_max_c: fold(|x| x.0, f64::max, f64::MIN),
                td_min_c: fold(|x| x.1, f64::min, f64::MAX),
                td_max_c: fold(|x| x.1, f64::max, f64::MIN),
                members: v.len(),
            }
        })
        .collect();
    out.sort_by(|a, b| b.pressure_hpa.total_cmp(&a.pressure_hpa));
    out
}

async fn get(http: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let bytes = http
        .get(url)
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", crate::alerts::USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    anyhow::ensure!(bytes.starts_with(b"GRIB"), "NOMADS sent no GRIB for {url}");
    Ok(bytes.to_vec())
}

async fn member(
    http: &reqwest::Client,
    run: DateTime<Utc>,
    fh: u16,
    m: u8,
    lon: f64,
    lat: f64,
) -> anyhow::Result<(MemberProfile, [f64; 2])> {
    let a = get(http, &member_url(run, fh, m, false, lon, lat)).await?;
    crate::task::sleep(PACE).await;
    let b = get(http, &member_url(run, fh, m, true, lon, lat)).await?;
    let (levels, point) = profile_from_grib(&[&a, &b], lon, lat)?;
    Ok((MemberProfile { member: m, levels }, point))
}

/// The GEFS envelope at `[lon, lat]` for a sounding valid at `valid`: every member from one cycle
/// (see the module doc), `progress` told how many members have arrived as they do.
pub async fn fetch_envelope(
    http: &reqwest::Client,
    lon: f64,
    lat: f64,
    valid: DateTime<Utc>,
    progress: impl Fn(usize),
) -> anyhow::Result<Envelope> {
    anyhow::ensure!(
        !cfg!(target_arch = "wasm32"),
        "GEFS member soundings come from NOMADS, which the browser version cannot reach: use the \
         desktop or Android app"
    );
    let runs = crate::global::gefs_cycles(Utc::now(), 4);
    let mut last_err = None;
    for (run, fh) in leads_for(valid, &runs) {
        let (control, point) = match member(http, run, fh, 0, lon, lat).await {
            Ok(c) => c,
            Err(e) => {
                last_err = Some(e);
                continue;
            }
        };
        let mut profiles = vec![control];
        progress(profiles.len());
        for m in 1..crate::ensemble::GEFS_MEMBERS {
            crate::task::sleep(PACE).await;
            if let Ok((p, _)) = member(http, run, fh, m, lon, lat).await {
                profiles.push(p);
                progress(profiles.len());
            }
        }
        anyhow::ensure!(
            profiles.len() >= MIN_MEMBERS,
            "only {} of {} GEFS members arrived",
            profiles.len(),
            crate::ensemble::GEFS_MEMBERS
        );
        return Ok(Envelope {
            run,
            fh,
            point,
            members: profiles.len(),
            levels: envelope(&profiles),
        });
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no GEFS cycle reaches that valid time")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// The filter request names the member's file, its own levels, and a box in 0..360 longitude
    /// around the point.
    #[test]
    fn a_member_request_names_its_file_levels_and_box() {
        let run = Utc.with_ymd_and_hms(2026, 10, 11, 12, 0, 0).unwrap();
        let a = member_url(run, 24, 0, false, -97.5, 35.4);
        assert!(
            a.contains("filter_gefs_atmos_0p50a.pl?dir=%2Fgefs.20261011%2F12%2Fatmos%2Fpgrb2ap5"),
            "{a}"
        );
        assert!(a.contains("&file=gec00.t12z.pgrb2a.0p50.f024"), "{a}");
        assert!(
            a.contains("&lev_850_mb=on") && !a.contains("&lev_800_mb=on"),
            "{a}"
        );
        assert!(
            a.contains("&leftlon=262.00&rightlon=263.00&toplat=36.00&bottomlat=34.00"),
            "{a}"
        );
        let b = member_url(run, 24, 7, true, -97.5, 35.4);
        assert!(
            b.contains("filter_gefs_atmos_0p50b.pl")
                && b.contains("&file=gep07.t12z.pgrb2b.0p50.f024"),
            "{b}"
        );
        assert!(
            b.contains("&lev_800_mb=on") && !b.contains("&lev_850_mb=on"),
            "{b}"
        );
    }

    /// A valid time picks each run's lead, snapped to 3 hours, and drops runs that are after it or
    /// too far before it.
    #[test]
    fn a_valid_time_snaps_to_each_runs_three_hour_leads() {
        let t = |d: u32, h: u32, m: u32| Utc.with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap();
        let runs = [t(11, 18, 0), t(11, 12, 0), t(11, 6, 0), t(1, 0, 0)];
        assert_eq!(
            leads_for(t(11, 21, 40), &runs),
            vec![(t(11, 18, 0), 3), (t(11, 12, 0), 9), (t(11, 6, 0), 15)]
        );
        // Before a run: not that run.
        assert_eq!(leads_for(t(11, 17, 0), &runs[..1]), vec![]);
    }

    /// The envelope is each level's extremes over the members that have it; a level too few reach
    /// is left out; missing values are skipped.
    #[test]
    fn the_envelope_is_each_levels_extremes_over_enough_members() {
        let profiles: Vec<MemberProfile> = (0..25u8)
            .map(|m| MemberProfile {
                member: m,
                levels: {
                    let k = f64::from(m);
                    let mut l = vec![
                        (850.0, 10.0 + k * 0.2, 5.0 - k * 0.1),
                        (500.0, -12.0 + k * 0.1, -30.0),
                    ];
                    if m < 5 {
                        // Only five members reach 300 hPa.
                        l.push((300.0, -40.0, -55.0));
                    }
                    if m == 3 {
                        l[0].1 = f64::NAN;
                    }
                    l
                },
            })
            .collect();
        let env = envelope(&profiles);
        assert_eq!(env.len(), 2, "{env:?}");
        assert_eq!(env[0].pressure_hpa, 850.0);
        assert_eq!(env[0].members, 24);
        assert!((env[0].t_min_c - 10.0).abs() < 1e-9 && (env[0].t_max_c - 14.8).abs() < 1e-9);
        assert!((env[0].td_min_c - 2.6).abs() < 1e-9 && (env[0].td_max_c - 5.0).abs() < 1e-9);
        assert_eq!(env[1].pressure_hpa, 500.0);
        assert!(envelope(&profiles[..10]).is_empty());
    }

    /// The live NOMADS filter: one member's two files decode to 19 levels at a grid point near the
    /// click, cold and dry aloft, warm below.
    #[tokio::test]
    #[ignore = "network: NOMADS"]
    async fn live_one_member_profile_from_nomads() {
        let http = reqwest::Client::new();
        let runs = crate::global::gefs_cycles(Utc::now(), 4);
        let (lon, lat) = (-97.5, 35.4);
        let mut found = None;
        for &run in &runs {
            if let Ok(p) = member(&http, run, 24, 0, lon, lat).await {
                found = Some((run, p));
                break;
            }
        }
        let (run, (profile, point)) = found.expect("a GEFS cycle on NOMADS");
        eprintln!(
            "GEFS {run} f024 control at {point:?}: {} levels; {:?}",
            profile.levels.len(),
            profile.levels
        );
        assert_eq!(profile.levels.len(), 19);
        assert!(
            (point[0] - lon).abs() <= 0.25 && (point[1] - lat).abs() <= 0.25,
            "{point:?}"
        );
        let (_, t_sfc, td_sfc) = profile.levels[0];
        let (_, t_top, _) = *profile.levels.last().unwrap();
        assert!(t_sfc > t_top + 30.0 && td_sfc <= t_sfc + 0.01);
    }

    /// The live envelope end to end: every member from one cycle, paced, for a point near
    /// Oklahoma City a day ahead. Spreads are ordered, positive and plausible.
    #[tokio::test]
    #[ignore = "network: NOMADS, about a minute"]
    async fn live_envelope_from_nomads() {
        let http = reqwest::Client::new();
        let valid = Utc::now() + chrono::Duration::hours(24);
        let started = std::time::Instant::now();
        let env = fetch_envelope(&http, -97.5, 35.4, valid, |_| {})
            .await
            .expect("an envelope");
        eprintln!(
            "GEFS {} f{:03} at {:?}: {} members, {} levels, {:.0} s",
            env.run,
            env.fh,
            env.point,
            env.members,
            env.levels.len(),
            started.elapsed().as_secs_f64()
        );
        for l in &env.levels {
            eprintln!(
                "  {:>4.0} hPa  T {:6.1} .. {:6.1}  Td {:6.1} .. {:6.1}  ({} members)",
                l.pressure_hpa, l.t_min_c, l.t_max_c, l.td_min_c, l.td_max_c, l.members
            );
        }
        assert!(env.members >= MIN_MEMBERS);
        assert!(env.levels.len() >= 15);
        for l in &env.levels {
            assert!(l.t_min_c <= l.t_max_c && l.td_min_c <= l.td_max_c);
            assert!(l.t_max_c - l.t_min_c < 20.0, "{l:?}");
            assert!(l.td_max_c <= l.t_max_c + 0.01);
        }
    }
}
