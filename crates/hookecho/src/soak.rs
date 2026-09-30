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
//! This soaks the data path, headless. The renderer's own long-run behaviour (GPU memory, frame
//! pacing) is what the Analyst log's telemetry watches in a real window.

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

/// Run the soak. Returns whether it passed.
pub fn run(site: &str, minutes: u64, inject: bool) -> anyhow::Result<bool> {
    use wxdata::level2::{self, Moment};
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
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
            Outcome::NewVolume {
                decode_ms,
                bin_ms,
                sweeps: binned,
            }
        });
        let at = start.elapsed();
        judge.cycle(at, &outcome);
        if let Some(mb) = rss_mb() {
            judge.rss(mb);
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
                "{:>6.1} min  new {}  decode {decode_ms:.0} ms  bin {sweeps} sweeps {bin_ms:.0} ms{mem}",
                at.as_secs_f64() / 60.0,
                last_name.as_deref().unwrap_or("?")
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
