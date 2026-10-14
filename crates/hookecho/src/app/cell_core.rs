//! A storm cell's core read on every tilt: the cell card's Core statistics, its By tilt table and
//! Core distribution, the compare view and the Storm Digest's core lines.
//!
//! A cell is a column, so one tilt's gates around its centre describe only the slice the pane
//! happens to show. Here every tilt of the volume is binned (velocity dealiased, as drawn), the
//! gates within [`CORE_KM`] of each wanted centre are cut out of it, and the sweep is dropped
//! before the next tilt, so a volume's twenty-odd tilts in seven moments are never held at once.
//! The result per centre is one [`RegionSamples`] whose rows keep their own tilt
//! (`regionstats::stack`), cached for the pane's volume.
//!
//! Binning every tilt takes a moment, so it runs off the UI thread; in the browser, where there is
//! no thread to run it on, one tilt is gathered per frame instead.

use super::*;
use wxdata::regionstats::{self, RegionSamples};

/// The core's half-width around a cell's centre, km.
pub(crate) const CORE_KM: f64 = 8.0;

/// The box of gates a core holds around `(lon, lat)`.
pub(crate) fn core_bbox(lon: f64, lat: f64) -> (f64, f64, f64, f64) {
    let dlat = CORE_KM / 110.57;
    let dlon = CORE_KM / (111.32 * lat.to_radians().cos().max(0.1));
    regionstats::bbox_of([lon - dlon, lat - dlat], [lon + dlon, lat + dlat])
}

/// A centre as a cache key: the exact position the storm table gives.
fn key_of(lon: f64, lat: f64) -> (u64, u64) {
    (lon.to_bits(), lat.to_bits())
}

/// Gathering the cores of some centres from one volume, a tilt at a time.
pub(crate) struct CoreJob {
    scan: Arc<Scan>,
    centres: Vec<(f64, f64)>,
    tilts: usize,
    next: usize,
    parts: Vec<Vec<RegionSamples>>,
}

impl CoreJob {
    pub(crate) fn new(scan: Arc<Scan>, centres: Vec<(f64, f64)>) -> Self {
        let tilts = level2::elevation_angles(&scan).len();
        let parts = vec![Vec::new(); centres.len()];
        Self {
            scan,
            centres,
            tilts,
            next: 0,
            parts,
        }
    }

    /// Gather the next tilt for every centre; true once every tilt is done.
    pub(crate) fn step(&mut self) -> bool {
        if self.next < self.tilts {
            let t = self.next;
            self.next += 1;
            let sweeps: Vec<Option<BinnedSweep>> = Moment::ALL
                .iter()
                .map(|&m| level2::bin_scan_opts(&self.scan, m, t, m == Moment::Velocity).ok())
                .collect();
            let refs: Vec<Option<&BinnedSweep>> = sweeps.iter().map(Option::as_ref).collect();
            for (part, &(lon, lat)) in self.parts.iter_mut().zip(&self.centres) {
                if let Some(s) = regionstats::gather_tilt(&Moment::ALL, &refs, core_bbox(lon, lat))
                {
                    part.push(s);
                }
            }
        }
        self.next >= self.tilts
    }

    /// Tilts gathered so far, of how many.
    pub(crate) fn progress(&self) -> (usize, usize) {
        (self.next, self.tilts)
    }

    /// Each centre's core over every tilt, `None` where no tilt has a gate there.
    pub(crate) fn finish(self) -> Gathered {
        self.centres
            .iter()
            .map(|&(lon, lat)| key_of(lon, lat))
            .zip(self.parts.into_iter().map(regionstats::stack))
            .collect()
    }
}

/// A gathering's answer: each centre's key and its core.
pub(crate) type Gathered = Vec<((u64, u64), Option<RegionSamples>)>;

/// Where a gathering runs.
enum Running {
    /// On a worker thread; its answer lands in the slot.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Thread(Arc<Mutex<Option<Gathered>>>),
    /// Stepped on the UI thread, a tilt per frame (the browser).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Local(Box<CoreJob>),
}

/// The whole-cell cores of one pane's volume.
#[derive(Default)]
pub(crate) struct CellCores {
    /// The volume they are of (`HookEchoApp::volume_key`); another empties the cache.
    volume: Option<(usize, String, usize)>,
    done: std::collections::HashMap<(u64, u64), Option<Arc<RegionSamples>>>,
    /// The gathering out, and how far it has got (tilts done, of how many).
    running: Option<(Running, (usize, usize))>,
    /// The cell card's rows for the core it shows, worked out once per core (a whole cell is tens
    /// of thousands of gates), keyed by the core's allocation and the unit system.
    pub(crate) card: Option<CardRows>,
    /// The card's distribution chart cache, kept across frames for the same reason.
    pub(crate) card_cache: crate::ui::region_stats_window::RegionCache,
}

/// What the cell card shows of one whole-cell core.
pub(crate) struct CardRows {
    pub key: (usize, bool),
    /// Core statistics: label, value, notable.
    pub core: Vec<(&'static str, String, bool)>,
    /// By tilt: tilt and beam height, that tilt's values, notable.
    pub tilts: Vec<(String, String, bool)>,
}

/// One centre's whole-cell core.
#[derive(Clone)]
pub(crate) enum CellCore {
    /// Every tilt's gates around it.
    Ready(Arc<RegionSamples>),
    /// No tilt has a gate there.
    Empty,
    /// Still being gathered: tilts done, of how many (0 of 0 before it starts).
    Gathering(usize, usize),
}

impl HookEchoApp {
    /// The whole-cell cores at `centres` on pane `idx`'s volume, gathering the ones not yet read.
    /// `ctx` wakes the UI when a worker finishes.
    pub(crate) fn cell_cores(
        &mut self,
        idx: usize,
        centres: &[(f64, f64)],
        ctx: Option<&egui::Context>,
    ) -> Vec<CellCore> {
        let key = self.volume_key(idx);
        let cores = &mut self.whole_cores;
        if cores.volume.as_ref() != Some(&key) {
            *cores = CellCores {
                volume: Some(key),
                ..Default::default()
            };
        }
        // Collect a finished gathering, or step the browser's by one tilt.
        let mut finished = None;
        let mut local_done = false;
        match cores.running.as_mut() {
            Some((Running::Thread(slot), _)) => {
                finished = slot.lock().ok().and_then(|mut s| s.take());
            }
            Some((Running::Local(job), progress)) => {
                local_done = job.step();
                *progress = job.progress();
                if !local_done {
                    if let Some(ctx) = ctx {
                        ctx.request_repaint();
                    }
                }
            }
            None => {}
        }
        if local_done {
            if let Some((Running::Local(job), _)) = cores.running.take() {
                finished = Some(job.finish());
            }
        }
        if let Some(results) = finished {
            cores.running = None;
            for (k, s) in results {
                cores.done.insert(k, s.map(Arc::new));
            }
        }
        let missing: Vec<(f64, f64)> = centres
            .iter()
            .copied()
            .filter(|&(lon, lat)| !cores.done.contains_key(&key_of(lon, lat)))
            .collect();
        if cores.running.is_none() && !missing.is_empty() {
            if let Some(scan) = self.views[idx].volume.as_ref().map(|v| Arc::clone(&v.scan)) {
                let job = CoreJob::new(scan, missing);
                let progress = job.progress();
                let running = if cfg!(target_arch = "wasm32") {
                    Running::Local(Box::new(job))
                } else {
                    let slot = Arc::new(Mutex::new(None));
                    let out = Arc::clone(&slot);
                    let ctx = ctx.cloned();
                    let mut job = job;
                    self.spawner.spawn_blocking(move || {
                        while !job.step() {}
                        if let Ok(mut s) = out.lock() {
                            *s = Some(job.finish());
                        }
                        if let Some(ctx) = ctx {
                            ctx.request_repaint();
                        }
                    });
                    Running::Thread(slot)
                };
                self.whole_cores.running = Some((running, progress));
            }
        }
        let cores = &self.whole_cores;
        centres
            .iter()
            .map(|&(lon, lat)| match cores.done.get(&key_of(lon, lat)) {
                Some(Some(s)) => CellCore::Ready(Arc::clone(s)),
                Some(None) => CellCore::Empty,
                None => {
                    let (d, n) = cores.running.as_ref().map_or((0, 0), |r| r.1);
                    CellCore::Gathering(d, n)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gathering on a real (first-records) volume: the job steps once per tilt, every row of
    /// the core lies in its box with every moment's column, and a tilt's rows are exactly what a
    /// single-tilt gather of that tilt finds; a centre past the radar's reach has none.
    #[test]
    fn a_core_is_gathered_tilt_by_tilt() {
        let scan = Arc::new(
            level2::decode_volume(
                include_bytes!("../../../wxdata/tests/data/corpus/mayfield-2021-first-records.ar2")
                    .to_vec(),
            )
            .unwrap(),
        );
        let tilts = level2::elevation_angles(&scan).len();
        let centre = (-88.64, 36.74);
        let mut job = CoreJob::new(Arc::clone(&scan), vec![centre, (-80.0, 30.0)]);
        let mut steps = 1;
        while !job.step() {
            steps += 1;
        }
        assert_eq!(steps, tilts.max(1));
        assert_eq!(job.progress(), (tilts, tilts));
        let out = job.finish();
        assert_eq!(out[0].0, key_of(centre.0, centre.1));
        let core = out[0].1.as_ref().expect("gates around the storm");
        assert!(out[1].1.is_none(), "nothing out of range");
        assert_eq!(core.moments, Moment::ALL);
        let (w, s, e, n) = core_bbox(centre.0, centre.1);
        for r in &core.rows {
            assert!(r.lon >= w && r.lon <= e && r.lat >= s && r.lat <= n);
            assert_eq!(r.values.len(), Moment::ALL.len());
        }
        // The lowest tilt, gathered on its own the way the card's displayed-tilt core is.
        let sweeps: Vec<Option<BinnedSweep>> = Moment::ALL
            .iter()
            .map(|&m| level2::bin_scan_opts(&scan, m, 0, m == Moment::Velocity).ok())
            .collect();
        let refs: Vec<Option<&BinnedSweep>> = sweeps.iter().map(Option::as_ref).collect();
        let alone =
            regionstats::gather_tilt(&Moment::ALL, &refs, core_bbox(centre.0, centre.1)).unwrap();
        assert_eq!(core.tilt(core.tilts()[0]).unwrap().rows, alone.rows);
    }

    /// A full volume: the Moore storm's core (KTLX 2013-05-20 20:16Z) is read on many tilts, the
    /// highest well above the lowest, and its reflectivity peaks aloft as a supercell's does.
    /// Network on first run; the volume is cached in `target/trail-volumes`.
    #[test]
    #[ignore = "network"]
    fn a_full_volume_core_reads_the_storm_on_many_tilts() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let id = Identifier::new("KTLX20130520_201643_V06.gz".into());
        let scan = Arc::new(
            rt.block_on(level2::download_scan(
                id,
                Some(repo.join("target/trail-volumes")),
            ))
            .unwrap(),
        );
        let mut job = CoreJob::new(scan, vec![(-97.51, 35.32)]);
        while !job.step() {}
        let core = job.finish().remove(0).1.expect("the Moore storm");
        let tilts = core.tilts();
        assert!(tilts.len() >= 8, "{tilts:?}");
        assert!(tilts.last().unwrap() - tilts[0] > 5.0, "{tilts:?}");
        let z = core.index_of(Moment::Reflectivity).unwrap();
        let max = |s: &RegionSamples| s.summary(z).map(|v| v.max);
        let peak = max(&core).unwrap();
        assert!(peak >= 60.0, "{peak}");
        println!(
            "{} tilts, {} gates, max {peak:.0} dBZ",
            tilts.len(),
            core.rows.len()
        );
        for t in &tilts {
            let s = core.tilt(*t).unwrap();
            println!("{t:5.1} deg: {} gates, max {:?} dBZ", s.rows.len(), max(&s));
        }
    }
}
