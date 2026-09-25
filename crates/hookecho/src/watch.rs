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
//!   or for **every volume in a range** (`--from`/`--to`, one numbered file per volume);
//! - **at a fixed size**, whatever the screen (ROADMAP_NEW M2): `--size PX` for a square,
//!   `--frame WxH`, or `--preset 1080p|1440p|4k|portrait|social`;
//! - **as a still or a loop**, by the output's extension: `.png`, `.jpg` or `.webp` for stills,
//!   `.gif` or `.mp4` for one animated loop of a `--from`/`--to` range, each frame held for the
//!   real time to the next scan (`--interval real`, the default) or evenly (`--interval fixed`),
//!   at `--fps` frames a second on average. The loop's sidecar lists every frame's volume, valid
//!   time and hold.
//!
//! ```text
//! hookecho --watch --site KTLX --product REF --out radar.png [--every 60]
//! hookecho --watch --workspace "Home" --out home.png --once
//! hookecho --watch --site KTLX --time 2013-05-20T20:08 --out moore.png
//! hookecho --watch --site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:30 --out moore.png
//! hookecho --watch --site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:30 \
//!     --preset 1080p --out moore.mp4 --fps 6
//! ```
//!
//! Options: `--tilt N`, `--zoom Z`, `--center LON,LAT`, `--basemap SLUG` (`none` for a bare
//! sweep). NEXRAD only: the other networks publish no volume list to poll or scrub.

use crate::loopexport::{LoopFormat, StillFormat, Timing};
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
    /// Output width and height, px. Rendered as a square at the longer edge and cropped to the
    /// middle, so a wide frame shows more map at the same scale rather than a stretched one.
    pub frame: (u32, u32),
    pub output: Output,
    pub zoom: Option<f64>,
    pub center: Option<(f64, f64)>,
    pub basemap: crate::tiles::BasemapStyle,
    pub out: PathBuf,
    pub when: When,
}

/// What each render becomes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Output {
    /// One picture per volume.
    Still(StillFormat),
    /// One animated loop of every volume in the range.
    Loop(LoopFormat, Timing),
}

/// The frame sizes a broadcast or social post asks for, by name.
pub fn preset(name: &str) -> Option<(u32, u32)> {
    Some(match name.to_ascii_lowercase().as_str() {
        "1080p" | "hd" => (1920, 1080),
        "1440p" | "qhd" => (2560, 1440),
        "4k" | "2160p" | "uhd" => (3840, 2160),
        "portrait" | "vertical" | "story" => (1080, 1920),
        "social" | "square" => (1080, 1080),
        _ => return None,
    })
}

/// The largest edge the off-screen renderer draws.
const MAX_EDGE: u32 = 4096;

fn parse_frame(s: &str) -> anyhow::Result<(u32, u32)> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| anyhow::anyhow!("--frame is WIDTHxHEIGHT, like 1920x1080"))?;
    let (w, h): (u32, u32) = (w.trim().parse()?, h.trim().parse()?);
    anyhow::ensure!(
        (64..=MAX_EDGE).contains(&w) && (64..=MAX_EDGE).contains(&h),
        "--frame edges are 64..={MAX_EDGE} px"
    );
    Ok((w, h))
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
    let frame = match (get("frame"), get("preset"), get("size")) {
        (Some(f), None, None) => parse_frame(f)?,
        (None, Some(p), None) => preset(p).ok_or_else(|| {
            anyhow::anyhow!("unknown preset '{p}': 1080p, 1440p, 4k, portrait or social")
        })?,
        (None, None, size) => {
            let px: u32 = size.map_or(Ok(1000), str::parse)?;
            let px = px.clamp(256, MAX_EDGE);
            (px, px)
        }
        _ => anyhow::bail!("give one of --size, --frame or --preset"),
    };
    let out =
        PathBuf::from(get("out").ok_or_else(|| anyhow::anyhow!("--out PATH.png is required"))?);
    let fps: f32 = get("fps").map_or(Ok(6.0), str::parse)?;
    anyhow::ensure!((0.5..=30.0).contains(&fps), "--fps is 0.5..=30");
    let timing = match get("interval").unwrap_or("real") {
        "real" => Timing::Real { fps },
        "fixed" => Timing::Fixed { fps },
        other => anyhow::bail!("--interval is real or fixed, not '{other}'"),
    };
    let ext = out
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let output = match (StillFormat::from_path(&out), ext.as_str()) {
        (Some(f), _) => Output::Still(f),
        (None, "gif") => Output::Loop(LoopFormat::Gif, timing),
        (None, "mp4") => Output::Loop(LoopFormat::Mp4, timing),
        _ => anyhow::bail!("--out ends in .png, .jpg, .webp, .gif or .mp4"),
    };
    if matches!(output, Output::Loop(..)) {
        anyhow::ensure!(
            matches!(when, When::Range(..)),
            "a .gif or .mp4 loop needs a --from/--to range"
        );
    }
    Ok(Job {
        site,
        moment,
        tilt,
        frame,
        output,
        zoom,
        center,
        basemap,
        out,
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

/// Render one volume (`id`) at the job's frame size: a square at the longer edge, cropped to the
/// middle by the renderer before it stamps the caption and colour bar. Returns the picture and the
/// volume's valid time.
fn render_frame(
    job: &Job,
    id: &level2::Identifier,
    scratch: &Path,
) -> anyhow::Result<(image::RgbaImage, DateTime<Utc>)> {
    let time = id
        .date_time()
        .ok_or_else(|| anyhow::anyhow!("volume {} has no time", id.name()))?;
    let (w, h) = job.frame;
    crate::headless::set_output(Some(w.max(h)), job.zoom);
    crate::headless::set_crop(Some((w, h)));
    crate::headless::set_center(job.center);
    crate::headless::set_extras(true);
    crate::headless::set_palette(None);
    crate::headless::run(
        scratch.to_string_lossy().as_ref(),
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
    let img = image::open(scratch)?.to_rgba8();
    let _ = std::fs::remove_file(scratch);
    Ok((img, time))
}

/// What every sidecar says about the job, whichever kind of output it describes.
fn job_meta(job: &Job) -> serde_json::Value {
    serde_json::json!({
        "site": job.site,
        "product": job.moment.short_name(),
        "tilt_index": job.tilt,
        "frame_px": [job.frame.0, job.frame.1],
        "zoom": job.zoom,
        "center": job.center.map(|(lon, lat)| serde_json::json!({ "lon": lon, "lat": lat })),
        "rendered_utc": Utc::now().to_rfc3339(),
        "source": "NOAA NEXRAD Level II, rendered by HookEcho",
    })
}

/// Render one volume (`id`) to the still `out` with its sidecar, both atomically.
fn render(
    job: &Job,
    id: &level2::Identifier,
    out: &Path,
    format: StillFormat,
) -> anyhow::Result<()> {
    let (img, time) = render_frame(job, id, &out.with_extension("rendering.png"))?;
    let bytes = crate::loopexport::encode_still(&img, format)?;
    let mut meta = job_meta(job);
    meta["volume"] = id.name().into();
    meta["valid_time_utc"] = time.to_rfc3339().into();
    write_atomic(out, &bytes)?;
    write_atomic(
        &sidecar(out),
        serde_json::to_string_pretty(&meta)?.as_bytes(),
    )?;
    println!("{} -> {}", id.name(), out.display());
    Ok(())
}

/// Render every volume in `ids` and encode them into one loop at `job.out`, with a sidecar that
/// lists each frame. Frames are staged as PNGs beside the output, so a long loop never has to
/// fit in memory, and the finished file is renamed into place.
fn render_loop(
    job: &Job,
    ids: &[level2::Identifier],
    format: LoopFormat,
    timing: Timing,
) -> anyhow::Result<()> {
    let stage = job.out.with_extension("frames");
    std::fs::create_dir_all(&stage)?;
    let result = (|| {
        let mut files = Vec::with_capacity(ids.len());
        let mut volumes = Vec::with_capacity(ids.len());
        for (i, id) in ids.iter().enumerate() {
            let (img, time) = render_frame(job, id, &stage.join("square.png"))?;
            let file = stage.join(format!("f{i:05}.png"));
            img.save(&file)?;
            files.push(file);
            volumes.push((id.name().to_string(), time));
            println!("frame {}/{}: {}", i + 1, ids.len(), id.name());
        }
        let frames = crate::loopexport::frame_list(&volumes, timing);
        let delays: Vec<u32> = frames.iter().map(|f| f.delay_ms).collect();
        let tmp = job.out.with_extension(match format {
            LoopFormat::Gif => "tmp.gif",
            LoopFormat::Mp4 => "tmp.mp4",
        });
        match format {
            #[cfg(not(target_arch = "wasm32"))]
            LoopFormat::Gif => crate::loopexport::encode_gif_timed(
                files.iter().map(|f| Ok(image::open(f)?.to_rgba8())),
                &delays,
                &tmp,
            )?,
            #[cfg(target_arch = "wasm32")]
            LoopFormat::Gif => anyhow::bail!("GIF export needs a filesystem"),
            LoopFormat::Mp4 => crate::loopexport::encode_mp4_files(&files, &delays, &tmp)?,
        }
        replace(&tmp, &job.out)?;
        let mut meta = job_meta(job);
        meta["format"] = match format {
            LoopFormat::Gif => "gif",
            LoopFormat::Mp4 => "mp4",
        }
        .into();
        let (interval, fps) = crate::loopexport::timing_words(timing);
        meta["interval"] = interval.into();
        meta["fps"] = fps.into();
        meta["duration_ms"] = frames
            .iter()
            .map(|f| u64::from(f.delay_ms))
            .sum::<u64>()
            .into();
        meta["frames"] = serde_json::to_value(&frames)?;
        write_atomic(
            &sidecar(&job.out),
            serde_json::to_string_pretty(&meta)?.as_bytes(),
        )?;
        println!("{} frame loop -> {}", frames.len(), job.out.display());
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&stage);
    result
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

/// The still format a job writes; only a range job can be a loop, which `parse_args` enforces.
fn still(job: &Job) -> anyhow::Result<StillFormat> {
    match job.output {
        Output::Still(f) => Ok(f),
        Output::Loop(..) => anyhow::bail!("a loop needs a --from/--to range"),
    }
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
            render(job, &id, &job.out, still(job)?)
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
            if let Output::Loop(format, timing) = job.output {
                return render_loop(job, &ids, format, timing);
            }
            let format = still(job)?;
            for id in &ids {
                let t = id.date_time().unwrap_or(*a);
                render(
                    job,
                    id,
                    &with_suffix(&job.out, &t.format("%Y%m%d_%H%M%S").to_string()),
                    format,
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
                        match render(job, &id, &job.out, still(job)?) {
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
        assert_eq!(
            (j.tilt, j.frame, j.zoom, j.center),
            (0, (1000, 1000), None, None)
        );
        assert_eq!(j.output, Output::Still(StillFormat::Png));
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
    fn frames_presets_formats_and_loops() {
        let j = parse_args(&args("--site KTLX --out r.jpg --preset 1080p"), &[]).unwrap();
        assert_eq!(
            (j.frame, j.output),
            ((1920, 1080), Output::Still(StillFormat::Jpeg))
        );
        let j = parse_args(&args("--site KTLX --out r.webp --frame 1080x1920"), &[]).unwrap();
        assert_eq!(
            (j.frame, j.output),
            ((1080, 1920), Output::Still(StillFormat::Webp))
        );
        assert_eq!(preset("4K"), Some((3840, 2160)));
        assert_eq!(preset("social"), Some((1080, 1080)));
        let j = parse_args(
            &args("--site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:30 --out m.mp4 --fps 8"),
            &[],
        )
        .unwrap();
        assert_eq!(
            j.output,
            Output::Loop(LoopFormat::Mp4, Timing::Real { fps: 8.0 })
        );
        let j = parse_args(
            &args(
                "--site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:30 --out m.gif \
                 --interval fixed",
            ),
            &[],
        )
        .unwrap();
        assert_eq!(
            j.output,
            Output::Loop(LoopFormat::Gif, Timing::Fixed { fps: 6.0 })
        );
        for bad in [
            "--site KTLX --out m.gif",
            "--site KTLX --out m.mp4 --time 2013-05-20T20:08",
            "--site KTLX --out m.bmp",
            "--site KTLX --out m.png --preset cinema",
            "--site KTLX --out m.png --frame 1920",
            "--site KTLX --out m.png --frame 9000x100",
            "--site KTLX --out m.png --size 800 --preset 4k",
            "--site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:30 --out m.gif --fps 90",
            "--site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:30 --out m.gif --interval odd",
        ] {
            assert!(parse_args(&args(bad), &[]).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_loops_sidecar_times_every_frame() {
        use chrono::TimeZone;
        let t = |m| Utc.with_ymd_and_hms(2013, 5, 20, 20, m, 0).unwrap();
        let vols = vec![
            ("KTLX20130520_200000_V06".to_string(), t(0)),
            ("KTLX20130520_200400_V06".to_string(), t(4)),
            ("KTLX20130520_200500_V06".to_string(), t(5)),
        ];
        let frames = crate::loopexport::frame_list(&vols, Timing::Fixed { fps: 4.0 });
        assert_eq!(
            frames
                .iter()
                .map(|f| (f.delay_ms, f.start_ms))
                .collect::<Vec<_>>(),
            [(250, 0), (250, 250), (750, 500)]
        );
        assert_eq!(frames[1].volume, "KTLX20130520_200400_V06");
        assert_eq!(frames[2].valid_time_utc, "2013-05-20T20:05:00+00:00");
        let real = crate::loopexport::frame_list(&vols, Timing::Real { fps: 4.0 });
        assert!(
            real[0].delay_ms > real[1].delay_ms,
            "the 4-minute gap outlasts the 1-minute one"
        );
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
