//! `--watch`: automated radar output (ROADMAP_NEW M3). Renders a site, product and tilt — or a
//! saved workspace's first pane — to a PNG with a JSON sidecar, through the same off-screen
//! renderer as `--serve`, and keeps it current:
//!
//! - **only when the source changes**: each poll lists the radar's volumes and renders only when
//!   a newer one has arrived, so a web page polling the file sees a new picture per volume, not
//!   one per poll;
//! - **atomically**: the PNG and the sidecar are written beside their targets and renamed over
//!   them, so a reader never sees half a file;
//! - **on a schedule** (`--every`), **once** (`--once`), for **one archived instant** (`--time`),
//!   or for **every volume in a range** (`--from`/`--to`, one numbered file per volume).
//!
//! ```text
//! hookecho --watch --site KTLX --product REF --out radar.png [--every 60]
//! hookecho --watch --workspace "Home" --out home.png --once
//! hookecho --watch --site KTLX --time 2013-05-20T20:08 --out moore.png
//! hookecho --watch --site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:30 --out moore.png
//! ```
//!
//! Options: `--tilt N`, `--size PX` (256..=2048), `--zoom Z`, `--center LON,LAT`,
//! `--basemap SLUG` (`none` for a bare sweep). NEXRAD only: the other networks publish no volume
//! list to poll or scrub.

use chrono::{DateTime, NaiveDateTime, Utc};
use std::path::{Path, PathBuf};
use std::time::Duration;
use wxdata::level2::{self, Moment};

/// What to render, how to frame it, and when.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub site: String,
    pub moment: Moment,
    pub tilt: usize,
    pub size: u32,
    pub zoom: Option<f64>,
    pub center: Option<(f64, f64)>,
    pub basemap: crate::tiles::BasemapStyle,
    pub out: PathBuf,
    pub when: When,
}

/// Which volumes to render.
#[derive(Debug, Clone, PartialEq)]
pub enum When {
    /// The newest volume, re-rendered as newer ones arrive, polling every `every`; `once` stops
    /// after the first.
    Live { every: Duration, once: bool },
    /// The archived volume nearest one instant.
    At(DateTime<Utc>),
    /// Every archived volume in a range, inclusive.
    Range(DateTime<Utc>, DateTime<Utc>),
}

fn parse_time(s: &str) -> anyhow::Result<DateTime<Utc>> {
    let s = s.trim_end_matches('Z');
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .map(|t| t.and_utc())
        .map_err(|_| anyhow::anyhow!("times look like 2013-05-20T20:08 (UTC), not '{s}'"))
}

/// Build a job from `--watch`'s arguments (everything after the flag). A `--workspace` fills in
/// the site, product, tilt, camera and basemap of that saved workspace's first pane; any of
/// those given explicitly as well win.
pub fn parse_args(
    args: &[String],
    workspaces: &[crate::workspace::Workspace],
) -> anyhow::Result<Job> {
    let mut flags = std::collections::HashMap::new();
    let mut once = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--once" => once = true,
            f if f.starts_with("--") => {
                let v = it
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("{f} needs a value"))?;
                flags.insert(f.trim_start_matches("--").to_string(), v.clone());
            }
            other => anyhow::bail!("unexpected argument '{other}'"),
        }
    }
    let get = |k: &str| flags.get(k).map(String::as_str);
    let pane = match get("workspace") {
        Some(name) => {
            let ws = workspaces
                .iter()
                .find(|w| w.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| anyhow::anyhow!("no saved workspace named '{name}'"))?;
            Some(
                ws.panes
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("workspace '{name}' has no panes"))?
                    .clone(),
            )
        }
        None => None,
    };
    let site = get("site")
        .map(str::to_ascii_uppercase)
        .or_else(|| pane.as_ref().and_then(|p| p.site.clone()))
        .ok_or_else(|| anyhow::anyhow!("--site (or a --workspace with one) is required"))?;
    anyhow::ensure!(
        wxdata::sites::is_nexrad(&site),
        "{site} is not a NEXRAD radar; only NEXRAD has a volume list to watch"
    );
    let moment = match get("product") {
        Some(p) => Moment::from_code(&p.to_ascii_uppercase())
            .ok_or_else(|| anyhow::anyhow!("unknown product '{p}'"))?,
        None => pane.as_ref().map_or(Moment::Reflectivity, |p| p.moment),
    };
    let tilt = match get("tilt") {
        Some(t) => t.parse()?,
        None => pane.as_ref().map_or(0, |p| p.tilt),
    };
    let zoom = match get("zoom") {
        Some(z) => Some(z.parse()?),
        None => pane.as_ref().map(|p| p.zoom),
    };
    let center = match get("center") {
        Some(c) => {
            let (lon, lat) = c
                .split_once(',')
                .ok_or_else(|| anyhow::anyhow!("--center is LON,LAT"))?;
            Some((lon.trim().parse()?, lat.trim().parse()?))
        }
        None => pane.as_ref().map(|p| (p.lon, p.lat)),
    };
    let basemap = crate::tiles::BasemapStyle::from_slug(
        get("basemap")
            .or_else(|| pane.as_ref().map(|p| p.basemap.as_str()))
            .unwrap_or("dark"),
    );
    let when = match (get("time"), get("from"), get("to")) {
        (Some(t), None, None) => When::At(parse_time(t)?),
        (None, Some(a), Some(b)) => {
            let (a, b) = (parse_time(a)?, parse_time(b)?);
            anyhow::ensure!(a <= b, "--from must be before --to");
            When::Range(a, b)
        }
        (None, None, None) => When::Live {
            every: Duration::from_secs(get("every").map_or(Ok(60), str::parse)?.max(15)),
            once,
        },
        _ => anyhow::bail!("give --time, or --from with --to, or neither for live"),
    };
    Ok(Job {
        site,
        moment,
        tilt,
        size: get("size").map_or(Ok(1000), str::parse)?,
        zoom,
        center,
        basemap,
        out: PathBuf::from(
            get("out").ok_or_else(|| anyhow::anyhow!("--out PATH.png is required"))?,
        ),
        when,
    })
}

/// `path` with `_suffix` before its extension: `radar.png` + `201305` → `radar_201305.png`.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("radar");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("png");
    path.with_file_name(format!("{stem}_{suffix}.{ext}"))
}

/// The sidecar for an output file: the same name with `.json` for its extension.
fn sidecar(path: &Path) -> PathBuf {
    path.with_extension("json")
}

/// Replace `target` with `tmp` in one step. On one filesystem a rename is atomic, so a reader has
/// the old file or the new one, never a partial write.
fn replace(tmp: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(tmp, target)
}

/// Write `bytes` to `target` atomically: beside it first, then renamed over it.
fn write_atomic(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = target.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    replace(&tmp, target)
}

/// Render one volume (`id`) to `out` with its sidecar, both atomically.
fn render(job: &Job, id: &level2::Identifier, out: &Path) -> anyhow::Result<()> {
    let time = id
        .date_time()
        .ok_or_else(|| anyhow::anyhow!("volume {} has no time", id.name()))?;
    let tmp = out.with_extension("rendering.png");
    crate::headless::set_output(Some(job.size), job.zoom);
    crate::headless::set_center(job.center);
    crate::headless::set_extras(true);
    crate::headless::set_palette(None);
    crate::headless::run(
        tmp.to_string_lossy().as_ref(),
        &job.site,
        job.moment,
        job.tilt,
        true,
        None,
        None,
        Some(time.date_naive()),
        Some(&time.format("%H:%M").to_string()),
        job.basemap,
        job.moment == Moment::Velocity,
    )?;
    let meta = serde_json::json!({
        "site": job.site,
        "product": job.moment.short_name(),
        "tilt_index": job.tilt,
        "volume": id.name(),
        "valid_time_utc": time.to_rfc3339(),
        "rendered_utc": Utc::now().to_rfc3339(),
        "size_px": job.size,
        "zoom": job.zoom,
        "center": job.center.map(|(lon, lat)| serde_json::json!({ "lon": lon, "lat": lat })),
        "source": "NOAA NEXRAD Level II, rendered by HookEcho",
    });
    replace(&tmp, out)?;
    write_atomic(
        &sidecar(out),
        serde_json::to_string_pretty(&meta)?.as_bytes(),
    )?;
    println!("{} -> {}", id.name(), out.display());
    Ok(())
}

/// The volumes a site has on the UTC days `from..=to` touch, oldest first.
async fn volumes_between(
    site: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> anyhow::Result<Vec<level2::Identifier>> {
    let mut day = from.date_naive();
    let mut out = Vec::new();
    while day <= to.date_naive() {
        out.extend(level2::list_volumes(site, day).await?);
        day = day
            .succ_opt()
            .ok_or_else(|| anyhow::anyhow!("date overflow"))?;
    }
    out.sort_by_key(|id| id.date_time());
    out.dedup_by_key(|id| id.name().to_string());
    Ok(out)
}

/// Run a job to completion (or forever, for a live one without `--once`).
pub fn run(job: &Job) -> anyhow::Result<()> {
    // Multi-threaded, as `--serve`'s is: the shared HTTP client pools its connections on the
    // runtime that opened them, and only a multi-threaded runtime keeps driving them between
    // `block_on` calls. On a current-thread one, the volume listing's pooled connection sat
    // undriven while `headless::run` downloaded on its own runtime, and every download failed.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    match &job.when {
        When::At(t) => {
            let ids = rt.block_on(volumes_between(
                &job.site,
                *t - chrono::Duration::hours(1),
                *t + chrono::Duration::hours(1),
            ))?;
            let id = ids
                .into_iter()
                .filter(|id| id.date_time().is_some())
                .min_by_key(|id| (id.date_time().unwrap() - *t).num_seconds().abs())
                .ok_or_else(|| anyhow::anyhow!("no {} volume near {t}", job.site))?;
            render(job, &id, &job.out)
        }
        When::Range(a, b) => {
            let ids: Vec<_> = rt
                .block_on(volumes_between(&job.site, *a, *b))?
                .into_iter()
                .filter(|id| id.date_time().is_some_and(|t| t >= *a && t <= *b))
                .collect();
            anyhow::ensure!(
                !ids.is_empty(),
                "no {} volumes between {a} and {b}",
                job.site
            );
            for id in &ids {
                let t = id.date_time().unwrap_or(*a);
                render(
                    job,
                    id,
                    &with_suffix(&job.out, &t.format("%Y%m%d_%H%M%S").to_string()),
                )?;
            }
            println!("{} volume(s) rendered", ids.len());
            Ok(())
        }
        When::Live { every, once } => {
            let mut last: Option<String> = None;
            loop {
                let now = Utc::now();
                let newest = rt
                    .block_on(level2::list_volumes(&job.site, now.date_naive()))
                    .map(|ids| ids.into_iter().rfind(|id| id.date_time().is_some()));
                match newest {
                    Ok(Some(id)) if last.as_deref() != Some(id.name()) => {
                        match render(job, &id, &job.out) {
                            Ok(()) => last = Some(id.name().to_string()),
                            Err(e) => eprintln!("render failed, will retry: {e}"),
                        }
                        if *once && last.is_some() {
                            return Ok(());
                        }
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("volume list failed, will retry: {e}"),
                }
                std::thread::sleep(*every);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn a_site_job_defaults_to_live_reflectivity_every_minute() {
        let j = parse_args(&args("--site ktlx --out r.png"), &[]).unwrap();
        assert_eq!(j.site, "KTLX");
        assert_eq!(j.moment, Moment::Reflectivity);
        assert_eq!(
            j.when,
            When::Live {
                every: Duration::from_secs(60),
                once: false
            }
        );
        assert_eq!((j.tilt, j.size, j.zoom, j.center), (0, 1000, None, None));
        let j = parse_args(&args("--site KTLX --out r.png --every 5 --once"), &[]).unwrap();
        assert_eq!(
            j.when,
            When::Live {
                every: Duration::from_secs(15),
                once: true
            },
            "polling is floored at 15 s"
        );
    }

    #[test]
    fn times_ranges_and_their_mistakes() {
        let j = parse_args(
            &args("--site KTLX --product VEL --tilt 1 --time 2013-05-20T20:08 --out m.png"),
            &[],
        )
        .unwrap();
        assert_eq!(j.moment, Moment::Velocity);
        assert_eq!(j.when, When::At("2013-05-20T20:08:00Z".parse().unwrap()));
        let j = parse_args(
            &args("--site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:30:00Z --out m.png"),
            &[],
        )
        .unwrap();
        assert!(matches!(j.when, When::Range(..)));
        for bad in [
            "--site KTLX --out m.png --time yesterday",
            "--site KTLX --out m.png --from 2013-05-20T20:30 --to 2013-05-20T19:50",
            "--site KTLX --out m.png --from 2013-05-20T20:30",
            "--site KTLX --out m.png --product XYZ",
            "--site TOKC --out m.png",
            "--out m.png",
            "--site KTLX",
            "--site KTLX --out",
        ] {
            assert!(parse_args(&args(bad), &[]).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_workspace_supplies_its_first_panes_view_and_flags_still_win() {
        let ws: crate::workspace::Workspace = serde_json::from_value(serde_json::json!({
            "name": "Home",
            "panes": [{
                "site": "KFWS", "moment": "Velocity", "tilt": 2, "srv": false,
                "basemap": "none", "lon": -97.1, "lat": 32.8, "zoom": 8.5,
            }],
        }))
        .unwrap();
        let j = parse_args(
            &args("--workspace home --out h.png --once"),
            std::slice::from_ref(&ws),
        )
        .unwrap();
        assert_eq!(
            (j.site.as_str(), j.moment, j.tilt),
            ("KFWS", Moment::Velocity, 2)
        );
        assert_eq!((j.zoom, j.center), (Some(8.5), Some((-97.1, 32.8))));
        let j = parse_args(
            &args("--workspace Home --product REF --out h.png"),
            std::slice::from_ref(&ws),
        )
        .unwrap();
        assert_eq!(j.moment, Moment::Reflectivity, "an explicit flag wins");
        assert!(parse_args(&args("--workspace Away --out h.png"), &[ws]).is_err());
    }

    #[test]
    fn outputs_are_named_and_replaced_whole() {
        assert_eq!(
            with_suffix(Path::new("out/radar.png"), "20130520_200811"),
            Path::new("out/radar_20130520_200811.png")
        );
        assert_eq!(
            sidecar(Path::new("out/radar.png")),
            Path::new("out/radar.json")
        );
        let dir = std::env::temp_dir().join(format!("hookecho-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("meta.json");
        write_atomic(&target, b"old").unwrap();
        write_atomic(&target, b"new").unwrap();
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"new",
            "an existing file is replaced"
        );
        assert!(
            !target.with_extension("tmp").exists(),
            "no temp file left behind"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
