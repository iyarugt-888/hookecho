//! The soak runner (ROADMAP_2 §3.1): `hookecho --soak SITE [MINUTES] [--inject]` runs the app's
//! own Level II path — list the newest volume, download it, decode it, bin every tilt of
//! reflectivity and velocity — on a cycle, for as long as it is asked, and fails on what a long
//! severe-weather session must survive:
//!
//! - **a stalled feed**: no new volume for [`Rules::stall_after`] (longer than any VCP takes);
//! - **an unrecovered failure**: [`Rules::max_fail_streak`] cycles failing in a row, not followed
//!   by a success before the end;
//! - **accepted corruption**: with `--inject`, every fourth new volume is also handed over cut in
//!   half, as a dropped connection leaves it; decoding that must be an error, never a volume;
//! - **memory growth**: resident memory at the end more than [`Rules::max_growth`] times (and
//!   [`Rules::min_growth_mb`] above) what it was once warmed up.
//!
//! One line per cycle to stdout, a JSON summary at the end, and a non-zero exit on failure, so a
//! CI job or a shell loop can run it. Profiles are just lengths: 120 minutes for a developer smoke
//! soak, 720 for the 12-hour severe-weather soak, 1440 for the 24-hour one. Native only.
//!
//! With `--render` (ROADMAP_PARITY M7.1, 1008.md H2) each new volume also goes through the
//! production renderer on one long-lived GPU device, as the app's pane does: a scenario clock
//! rotates the product (REF, VEL, CC, ZDR) and the tilt (the lowest four) cycle by cycle, draws
//! that sweep with the map renderer, then builds the 3D smooth volume and uploads it. Each is
//! timed, and the GPU's allocated bytes (the backend's allocator report; unknown where it has
//! none) are tracked like resident memory: growth after warm-up fails the soak, and so does any
//! render that fails. `--jsonl PATH` writes one timestamped JSON line per cycle, after a first
//! line naming the run (site, profile, build, adapter). The window's own frame pacing and
//! presentation remain the Analyst log's telemetry in a real window.

use serde::Serialize;
use std::time::{Duration, Instant};

/// What makes a soak fail.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    pub stall_after: Duration,
    pub max_fail_streak: u32,
    pub max_growth: f64,
    pub min_growth_mb: f64,
    /// New volumes before the memory baseline is taken: caches fill first.
    pub warmup_volumes: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            // A clear-air VCP runs ten minutes; twice that is a feed that has stopped.
            stall_after: Duration::from_secs(20 * 60),
            max_fail_streak: 5,
            max_growth: 1.5,
            min_growth_mb: 200.0,
            warmup_volumes: 3,
        }
    }
}

/// What one cycle came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A new volume: decoded and binned.
    NewVolume {
        decode_ms: f64,
        bin_ms: f64,
        sweeps: usize,
    },
    /// The newest volume is the one already seen.
    NoChange,
    Failed(String),
}

/// Keeps the score and says whether the soak has failed.
#[derive(Debug, Default, Serialize)]
pub struct Judge {
    #[serde(skip)]
    rules: Option<Rules>,
    pub cycles: u32,
    pub volumes: u32,
    pub failures: u32,
    pub fail_streak: u32,
    pub worst_fail_streak: u32,
    pub injected: u32,
    pub injected_refused: u32,
    pub decode_ms_max: f64,
    pub decode_ms_total: f64,
    pub bin_ms_max: f64,
    /// Seconds since the start at which the last new volume arrived.
    pub last_volume_s: Option<f64>,
    pub rss_baseline_mb: Option<f64>,
    pub rss_last_mb: Option<f64>,
    pub rss_max_mb: Option<f64>,
    /// With `--render`: frames drawn, those that failed, and the slowest stages.
    pub renders: u32,
    pub render_failures: u32,
    pub render_ms_max: f64,
    pub build3d_ms_max: f64,
    pub upload3d_ms_max: f64,
    /// GPU memory the backend's allocator reports, MB; `None` where it reports none.
    pub gpu_baseline_mb: Option<f64>,
    pub gpu_last_mb: Option<f64>,
    pub gpu_max_mb: Option<f64>,
    /// Why it failed; empty while it has not.
    pub problems: Vec<String>,
}

impl Judge {
    pub fn new(rules: Rules) -> Self {
        Self {
            rules: Some(rules),
            ..Self::default()
        }
    }

    fn rules(&self) -> Rules {
        self.rules.unwrap_or_default()
    }

    /// Record a cycle that ended `at` into the run.
    pub fn cycle(&mut self, at: Duration, outcome: &Outcome) {
        self.cycles += 1;
        match outcome {
            Outcome::NewVolume {
                decode_ms, bin_ms, ..
            } => {
                self.volumes += 1;
                self.fail_streak = 0;
                self.decode_ms_total += decode_ms;
                self.decode_ms_max = self.decode_ms_max.max(*decode_ms);
                self.bin_ms_max = self.bin_ms_max.max(*bin_ms);
                self.last_volume_s = Some(at.as_secs_f64());
            }
            Outcome::NoChange => self.fail_streak = 0,
            Outcome::Failed(_) => {
                self.failures += 1;
                self.fail_streak += 1;
                self.worst_fail_streak = self.worst_fail_streak.max(self.fail_streak);
            }
        }
    }

    /// A deliberately cut volume was handed to the decoder; `refused` is whether it said no.
    pub fn injected(&mut self, refused: bool) {
        self.injected += 1;
        if refused {
            self.injected_refused += 1;
        } else if !self.problems.iter().any(|p| p.starts_with("A cut-off")) {
            self.problems
                .push("A cut-off volume decoded as if it were whole".to_string());
        }
    }

    /// Resident memory now, in MB.
    pub fn rss(&mut self, mb: f64) {
        if self.rss_baseline_mb.is_none() && self.volumes >= self.rules().warmup_volumes {
            self.rss_baseline_mb = Some(mb);
        }
        self.rss_last_mb = Some(mb);
        self.rss_max_mb = Some(self.rss_max_mb.map_or(mb, |m: f64| m.max(mb)));
    }

    /// A render-profile cycle: drawn (with its stage times) or failed.
    pub fn rendered(&mut self, stats: Result<&RenderStats, &str>) {
        match stats {
            Ok(s) => {
                self.renders += 1;
                self.render_ms_max = self.render_ms_max.max(s.render_ms);
                self.build3d_ms_max = self.build3d_ms_max.max(s.build3d_ms);
                self.upload3d_ms_max = self.upload3d_ms_max.max(s.upload3d_ms);
            }
            Err(_) => self.render_failures += 1,
        }
    }

    /// GPU memory allocated now, in MB. The baseline is taken when the resident one is.
    pub fn gpu(&mut self, mb: f64) {
        if self.gpu_baseline_mb.is_none() && self.volumes >= self.rules().warmup_volumes {
            self.gpu_baseline_mb = Some(mb);
        }
        self.gpu_last_mb = Some(mb);
        self.gpu_max_mb = Some(self.gpu_max_mb.map_or(mb, |m: f64| m.max(mb)));
    }

    /// Whether the run so far passes, as of `now` since the start; `ended` when it is over (an
    /// unrecovered streak only counts once there is no more time to recover in).
    pub fn verdict(&self, now: Duration, ended: bool) -> Vec<String> {
        let r = self.rules();
        let mut out = self.problems.clone();
        let since = now.as_secs_f64() - self.last_volume_s.unwrap_or(0.0);
        if since > r.stall_after.as_secs_f64() {
            out.push(format!(
                "No new volume for {:.0} min (stall at {:.0})",
                since / 60.0,
                r.stall_after.as_secs_f64() / 60.0
            ));
        }
        if ended && self.fail_streak >= r.max_fail_streak {
            out.push(format!(
                "Ended {} failed cycles in a row without recovering",
                self.fail_streak
            ));
        }
        if let (Some(base), Some(last)) = (self.rss_baseline_mb, self.rss_last_mb) {
            if last > base * r.max_growth && last - base > r.min_growth_mb {
                out.push(format!(
                    "Memory grew from {base:.0} MB to {last:.0} MB after warming up"
                ));
            }
        }
        if let (Some(base), Some(last)) = (self.gpu_baseline_mb, self.gpu_last_mb) {
            if last > base * r.max_growth && last - base > r.min_growth_mb {
                out.push(format!(
                    "GPU memory grew from {base:.0} MB to {last:.0} MB after warming up"
                ));
            }
        }
        if self.render_failures > 0 {
            out.push(format!(
                "Rendering failed {} time{}",
                self.render_failures,
                if self.render_failures == 1 { "" } else { "s" }
            ));
        }
        out
    }
}

/// Resident memory of this process in MB, where the platform says.
pub fn rss_mb() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: f64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        Some(pages * 4096.0 / (1024.0 * 1024.0))
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // SAFETY: a zeroed counters struct with its size set is what the call fills; the
        // pseudo-handle from GetCurrentProcess needs no closing.
        unsafe {
            let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) == 0 {
                return None;
            }
            Some(c.WorkingSetSize as f64 / (1024.0 * 1024.0))
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

/// What a soak exercises beyond the data path.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Hand every fourth new volume over cut in half as well.
    pub inject: bool,
    /// The render profile: each new volume through the renderer on one GPU device.
    pub render: bool,
    /// Write one JSON line per cycle here.
    pub jsonl: Option<std::path::PathBuf>,
}

/// One render-profile cycle's stages, ms.
#[derive(Debug, Clone, Serialize)]
pub struct RenderStats {
    pub moment: &'static str,
    pub tilt: usize,
    pub render_ms: f64,
    pub build3d_ms: f64,
    pub upload3d_ms: f64,
}

/// The products the scenario clock rotates through.
const ROTATION: [wxdata::level2::Moment; 4] = [
    wxdata::level2::Moment::Reflectivity,
    wxdata::level2::Moment::Velocity,
    wxdata::level2::Moment::CorrelationCoefficient,
    wxdata::level2::Moment::DifferentialReflectivity,
];

/// The scenario clock: cycle `n` draws this product at this tilt (the lowest four).
pub fn scenario(n: u32, tilts: usize) -> (wxdata::level2::Moment, usize) {
    let moment = ROTATION[n as usize % ROTATION.len()];
    let tilt = (n as usize / ROTATION.len()) % tilts.clamp(1, 4);
    (moment, tilt)
}

/// One GPU device kept for the whole run, with the map's and the 3D view's renderers, as a pane
/// keeps them.
pub(crate) struct RenderProfile {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub adapter: String,
    resources: crate::render::RenderResources,
    res3d: crate::render3d::Volume3dResources,
    _target: wgpu::Texture,
    view: wgpu::TextureView,
}

impl RenderProfile {
    const SIZE: u32 = 1024;

    pub(crate) fn new(rt: &tokio::runtime::Runtime) -> anyhow::Result<Self> {
        let (device, queue, adapter) = crate::headless::init_gpu(rt)?;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let resources = crate::render::RenderResources::new(&device, format);
        let res3d = crate::render3d::Volume3dResources::new(&device, format);
        let target = crate::headless::new_target(&device, format, Self::SIZE);
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let info = adapter.get_info();
        Ok(Self {
            device,
            queue,
            adapter: format!("{} ({:?})", info.name, info.backend),
            resources,
            res3d,
            _target: target,
            view,
        })
    }

    /// GPU memory the backend's allocator holds for this device, MB; `None` where unreported.
    pub(crate) fn gpu_mb(&self) -> Option<f64> {
        self.device
            .generate_allocator_report()
            .map(|r| r.total_allocated_bytes as f64 / (1024.0 * 1024.0))
    }

    /// Cycle `n`'s scenario on `scan`: the sweep drawn, then the 3D volume built and uploaded.
    pub(crate) fn exercise(
        &mut self,
        scan: &wxdata::level2::Scan,
        n: u32,
    ) -> anyhow::Result<RenderStats> {
        use wxdata::level2::{self, Moment};
        let tilts = level2::elevation_angles(scan).len();
        let (moment, tilt) = scenario(n, tilts);
        let t = Instant::now();
        let sweep = level2::bin_scan(scan, moment, tilt)?;
        let camera = crate::render::mercator::Camera::at_lonlat(
            f64::from(sweep.radar_lon),
            f64::from(sweep.radar_lat),
            8.5,
        );
        let cb = crate::headless::sweep_callback(
            &sweep,
            &camera,
            Self::SIZE,
            crate::colormap::default_table(moment),
        );
        let clear = wgpu::Color::BLACK;
        self.resources
            .render_once(&self.device, &self.queue, &self.view, &cb, clear);
        let mut next = cb;
        next.radar_upload = None;
        self.resources
            .render_once(&self.device, &self.queue, &self.view, &next, clear);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| anyhow::anyhow!("GPU poll after the sweep: {e}"))?;
        let render_ms = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let sweeps: Vec<_> = (0..tilts)
            .filter_map(|k| level2::bin_scan_opts(scan, Moment::Reflectivity, k, false).ok())
            .collect();
        let half_km = wxdata::volume3d::max_sample_range_km(&sweeps).max(50.0);
        let v3 = wxdata::volume3d::build(&sweeps, 192, 48, half_km, 18.0)
            .ok_or_else(|| anyhow::anyhow!("no 3D volume from {} tilts", sweeps.len()))?;
        let build3d_ms = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let lut = crate::colormap::bake_lut(
            crate::colormap::default_table(Moment::Reflectivity),
            (v3.value_min, v3.value_max),
            None,
        )
        .to_vec();
        self.res3d.upload(
            &self.device,
            &self.queue,
            &crate::render3d::Volume3dUpload {
                data: crate::render3d::pack_rg8(&v3.data),
                n: v3.n as u32,
                nz: v3.nz as u32,
                lut,
                half_km: v3.half_km,
                center_km: [0.0, 0.0],
                top_km: v3.top_km,
                outside: 0.0,
                value_range: None,
                lut_range: None,
            },
        );
        self.queue.submit(std::iter::empty());
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| anyhow::anyhow!("GPU poll after the 3D upload: {e}"))?;
        Ok(RenderStats {
            moment: moment.short_name(),
            tilt,
            render_ms,
            build3d_ms,
            upload3d_ms: t.elapsed().as_secs_f64() * 1000.0,
        })
    }
}

/// Run the soak. Returns whether it passed.
pub fn run(site: &str, minutes: u64, opts: &Options) -> anyhow::Result<bool> {
    use std::io::Write as _;
    use wxdata::level2::{self, Moment};
    let inject = opts.inject;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let mut profile = if opts.render {
        Some(RenderProfile::new(&rt)?)
    } else {
        None
    };
    let mut jsonl = match &opts.jsonl {
        Some(path) => Some(std::io::BufWriter::new(std::fs::File::create(path)?)),
        None => None,
    };
    if let Some(out) = jsonl.as_mut() {
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "run": "soak",
                "site": site.to_ascii_uppercase(),
                "minutes": minutes,
                "inject": inject,
                "profile": if opts.render { "render" } else { "data" },
                "build": env!("CARGO_PKG_VERSION"),
                "adapter": profile.as_ref().map(|p| p.adapter.clone()),
                "started_utc": chrono::Utc::now().to_rfc3339(),
            })
        )?;
        out.flush()?;
    }
    let mut scenario_n = 0u32;
    let rules = Rules::default();
    let mut judge = Judge::new(rules);
    let start = Instant::now();
    let deadline = Duration::from_secs(minutes * 60);
    let cycle_every = Duration::from_secs(30);
    let mut last_name: Option<String> = None;
    let site = site.to_ascii_uppercase();
    println!(
        "soak {site}: {minutes} min, a cycle every 30 s{}",
        if inject {
            ", injecting cut-off volumes"
        } else {
            ""
        }
    );
    while start.elapsed() < deadline {
        let began = Instant::now();
        let mut new_scan: Option<wxdata::level2::Scan> = None;
        let outcome = rt.block_on(async {
            let id = match level2::latest_identifier(&site).await {
                Ok(id) => id,
                Err(e) => return Outcome::Failed(format!("list: {e:#}")),
            };
            let name = id.name().to_string();
            if last_name.as_deref() == Some(name.as_str()) {
                return Outcome::NoChange;
            }
            let bytes = match level2::volume_bytes(id).await {
                Ok(b) => b,
                Err(e) => return Outcome::Failed(format!("download {name}: {e:#}")),
            };
            // Every fourth new volume, also hand over half of it, as a dropped connection would.
            if inject && judge.volumes.is_multiple_of(4) {
                let cut = bytes[..bytes.len() / 2].to_vec();
                // Refused, or decoded but known to be short of the end: either way it cannot pass
                // for the whole volume.
                let refused = wxdata::task::blocking(move || {
                    level2::decode_volume(cut).map_or(true, |s| !level2::scan_complete(&s))
                })
                .await
                .unwrap_or(true);
                judge.injected(refused);
            }
            let t = Instant::now();
            let scan = match wxdata::task::blocking(move || level2::decode_volume(bytes)).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => return Outcome::Failed(format!("decode {name}: {e:#}")),
                Err(e) => return Outcome::Failed(format!("decode {name}: {e:#}")),
            };
            let decode_ms = t.elapsed().as_secs_f64() * 1000.0;
            if !level2::scan_complete(&scan) {
                return Outcome::Failed(format!(
                    "{name} is not a whole volume (no end-of-volume radial)"
                ));
            }
            let t = Instant::now();
            let tilts = level2::elevation_angles(&scan).len();
            let mut binned = 0;
            for tilt in 0..tilts {
                for m in [Moment::Reflectivity, Moment::Velocity] {
                    if level2::bin_scan(&scan, m, tilt).is_ok() {
                        binned += 1;
                    }
                }
            }
            let bin_ms = t.elapsed().as_secs_f64() * 1000.0;
            last_name = Some(name);
            new_scan = Some(scan);
            Outcome::NewVolume {
                decode_ms,
                bin_ms,
                sweeps: binned,
            }
        });
        // The render profile: the new volume through the renderer on the long-lived device.
        let rendered = match (profile.as_mut(), new_scan.take()) {
            (Some(p), Some(scan)) => {
                let r = p.exercise(&scan, scenario_n).map_err(|e| format!("{e:#}"));
                scenario_n += 1;
                judge.rendered(r.as_ref().map_err(String::as_str));
                Some(r)
            }
            _ => None,
        };
        let gpu_mb = profile.as_ref().and_then(RenderProfile::gpu_mb);
        let at = start.elapsed();
        judge.cycle(at, &outcome);
        if let Some(mb) = rss_mb() {
            judge.rss(mb);
        }
        if let Some(mb) = gpu_mb {
            judge.gpu(mb);
        }
        if let Some(out) = jsonl.as_mut() {
            let (kind, detail) = match &outcome {
                Outcome::NewVolume { .. } => ("new", None),
                Outcome::NoChange => ("no change", None),
                Outcome::Failed(e) => ("failed", Some(e.clone())),
            };
            let (decode_ms, bin_ms) = match &outcome {
                Outcome::NewVolume {
                    decode_ms, bin_ms, ..
                } => (Some(*decode_ms), Some(*bin_ms)),
                _ => (None, None),
            };
            writeln!(
                out,
                "{}",
                serde_json::json!({
                    "t_s": at.as_secs_f64(),
                    "utc": chrono::Utc::now().to_rfc3339(),
                    "outcome": kind,
                    "error": detail,
                    "volume": matches!(outcome, Outcome::NewVolume { .. })
                        .then(|| last_name.clone())
                        .flatten(),
                    "decode_ms": decode_ms,
                    "bin_ms": bin_ms,
                    "render": rendered.as_ref().map(|r| match r {
                        Ok(s) => serde_json::to_value(s).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "failed": e }),
                    }),
                    "rss_mb": judge.rss_last_mb,
                    "gpu_mb": gpu_mb,
                })
            )?;
            out.flush()?;
        }
        let mem = judge
            .rss_last_mb
            .map_or(String::new(), |m| format!(" · {m:.0} MB"));
        match &outcome {
            Outcome::NewVolume {
                decode_ms,
                bin_ms,
                sweeps,
            } => println!(
                "{:>6.1} min  new {}  decode {decode_ms:.0} ms  bin {sweeps} sweeps {bin_ms:.0} ms{mem}{}",
                at.as_secs_f64() / 60.0,
                last_name.as_deref().unwrap_or("?"),
                match &rendered {
                    Some(Ok(s)) => format!(
                        " · drew {} tilt {} {:.0} ms, 3D {:.0}+{:.0} ms{}",
                        s.moment,
                        s.tilt + 1,
                        s.render_ms,
                        s.build3d_ms,
                        s.upload3d_ms,
                        gpu_mb.map_or(String::new(), |g| format!(", GPU {g:.0} MB"))
                    ),
                    Some(Err(e)) => format!(" · RENDER FAILED: {e}"),
                    None => String::new(),
                }
            ),
            Outcome::NoChange => println!("{:>6.1} min  no change{mem}", at.as_secs_f64() / 60.0),
            Outcome::Failed(e) => println!(
                "{:>6.1} min  FAILED ({} in a row): {e}{mem}",
                at.as_secs_f64() / 60.0,
                judge.fail_streak
            ),
        }
        // A stall or accepted corruption fails now; there is no point soaking on.
        let problems = judge.verdict(at, false);
        if !problems.is_empty() {
            judge.problems = problems;
            break;
        }
        let spent = began.elapsed();
        if spent < cycle_every {
            std::thread::sleep(cycle_every - spent);
        }
    }
    let problems = judge.verdict(start.elapsed(), true);
    judge.problems = problems;
    println!("{}", serde_json::to_string_pretty(&judge)?);
    for p in &judge.problems {
        eprintln!("soak FAILED: {p}");
    }
    Ok(judge.problems.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol() -> Outcome {
        Outcome::NewVolume {
            decode_ms: 400.0,
            bin_ms: 100.0,
            sweeps: 30,
        }
    }

    #[test]
    fn the_scenario_clock_rotates_products_then_tilts() {
        use wxdata::level2::Moment;
        let seen: Vec<_> = (0..10).map(|n| scenario(n, 14)).collect();
        assert_eq!(seen[0], (Moment::Reflectivity, 0));
        assert_eq!(seen[3], (Moment::DifferentialReflectivity, 0));
        assert_eq!(seen[4], (Moment::Reflectivity, 1));
        assert_eq!(
            scenario(16, 14),
            (Moment::Reflectivity, 0),
            "the lowest four tilts"
        );
        assert_eq!(scenario(5, 1).1, 0, "a one-tilt scan stays on it");
        assert_eq!(scenario(5, 0).1, 0);
    }

    #[test]
    fn gpu_growth_and_any_failed_render_fail_the_soak() {
        let stats = RenderStats {
            moment: "REF",
            tilt: 0,
            render_ms: 5.0,
            build3d_ms: 20.0,
            upload3d_ms: 7.0,
        };
        let mut j = Judge::new(Rules::default());
        for s in 0..3 {
            j.cycle(Duration::from_secs(s), &vol());
            j.rendered(Ok(&stats));
        }
        j.gpu(100.0);
        j.gpu(120.0);
        assert!(j.verdict(Duration::from_secs(4), true).is_empty());
        assert_eq!((j.renders, j.render_ms_max), (3, 5.0));
        j.gpu(500.0);
        let p = j.verdict(Duration::from_secs(5), true);
        assert!(
            p.iter()
                .any(|p| p.contains("GPU memory grew from 100 MB to 500 MB")),
            "{p:?}"
        );
        let mut j = Judge::new(Rules::default());
        j.rendered(Err("device lost"));
        let p = j.verdict(Duration::ZERO, false);
        assert_eq!(p, ["Rendering failed 1 time"]);
    }

    /// The render profile on the Moore 2013 volume for 40 scenario cycles on one device: every
    /// cycle draws, and the GPU's allocated memory stops growing once every product has been
    /// drawn. Writes `target/parity-review/m7.1/render-profile.txt`.
    #[test]
    #[ignore = "gpu: the render profile on a real volume"]
    fn gpu_render_profile_holds_its_memory() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = repo.join("target/scientific-corpus/KTLX20130520_201229_V06.gz");
        let Ok(bytes) = std::fs::read(&path) else {
            println!("SKIP: {} not provisioned", path.display());
            return;
        };
        let scan = wxdata::level2::decode_volume(bytes).expect("Moore 2013 decodes");
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut p = RenderProfile::new(&rt).expect("GPU adapter");
        let mut lines = vec![format!("adapter: {}", p.adapter)];
        let mut gpu = Vec::new();
        for n in 0..40 {
            let s = p.exercise(&scan, n).expect("each cycle draws");
            let mb = p.gpu_mb();
            gpu.push(mb);
            lines.push(format!(
                "cycle {n:>2}: {} tilt {} drawn {:.1} ms, 3D built {:.1} ms, uploaded {:.1} ms, GPU {}",
                s.moment,
                s.tilt + 1,
                s.render_ms,
                s.build3d_ms,
                s.upload3d_ms,
                mb.map_or("unreported".to_string(), |m| format!("{m:.1} MB"))
            ));
        }
        let report = lines.join("\n") + "\n";
        print!("{report}");
        let dir = repo.join("target/parity-review/m7.1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("render-profile.txt"), report).unwrap();
        // After the first full rotation (all four products, all four tilts: 16 cycles) the
        // allocation must not keep rising.
        if let (Some(Some(warm)), Some(Some(last))) = (gpu.get(16), gpu.last()) {
            assert!(
                *last <= warm * 1.05 + 1.0,
                "GPU memory kept growing: {warm:.1} MB at cycle 16, {last:.1} MB at the end"
            );
        }
    }

    #[test]
    fn a_healthy_run_passes() {
        let mut j = Judge::new(Rules::default());
        for m in 0..60 {
            j.cycle(
                Duration::from_secs(m * 60),
                &if m % 5 == 0 { vol() } else { Outcome::NoChange },
            );
            j.rss(300.0);
        }
        assert!(j.verdict(Duration::from_secs(3600), true).is_empty());
        assert_eq!(j.volumes, 12);
    }

    #[test]
    fn a_feed_that_stops_is_a_stall() {
        let mut j = Judge::new(Rules::default());
        j.cycle(Duration::from_secs(60), &vol());
        let p = j.verdict(Duration::from_secs(60 + 21 * 60), false);
        assert!(
            p.iter().any(|p| p.contains("No new volume for 21 min")),
            "{p:?}"
        );
    }

    #[test]
    fn failures_only_fail_the_run_if_they_are_not_recovered_from() {
        let mut j = Judge::new(Rules::default());
        for s in 0..6 {
            j.cycle(Duration::from_secs(s), &Outcome::Failed("x".into()));
        }
        j.cycle(Duration::from_secs(7), &vol());
        assert!(
            j.verdict(Duration::from_secs(8), true).is_empty(),
            "recovered"
        );
        assert_eq!(j.worst_fail_streak, 6);
        for s in 10..16 {
            j.cycle(Duration::from_secs(s), &Outcome::Failed("x".into()));
        }
        let p = j.verdict(Duration::from_secs(16), true);
        assert!(p.iter().any(|p| p.contains("6 failed cycles")), "{p:?}");
    }

    #[test]
    fn corruption_accepted_or_memory_that_keeps_growing_fails() {
        let mut j = Judge::new(Rules::default());
        j.injected(true);
        assert!(j.verdict(Duration::ZERO, false).is_empty());
        j.injected(false);
        assert!(!j.verdict(Duration::ZERO, false).is_empty());

        let mut j = Judge::new(Rules::default());
        for s in 0..3 {
            j.cycle(Duration::from_secs(s), &vol());
        }
        j.rss(300.0);
        j.rss(700.0);
        let p = j.verdict(Duration::from_secs(4), true);
        assert!(p.iter().any(|p| p.contains("300 MB to 700 MB")), "{p:?}");
        // Growth small in absolute terms is not a leak.
        let mut j = Judge::new(Rules::default());
        for s in 0..3 {
            j.cycle(Duration::from_secs(s), &vol());
        }
        j.rss(100.0);
        j.rss(180.0);
        assert!(j.verdict(Duration::from_secs(4), true).is_empty());
    }
}
