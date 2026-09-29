//! 3D loop animation (ROADMAP_NEW H8): the built 3D frames a pane keeps so a loop plays its 3D
//! view frame for frame, and which frames playback reaches next so they can be built ahead.
//!
//! A Smooth volume or an isosurface takes from a fraction of a second to a few seconds to build,
//! far longer than a 4 fps frame lasts. Without a cache the 3D view lags the loop and shows one
//! volume's 3D under another volume's time; with one, every frame after the first pass is a
//! lookup. Frames are keyed by volume name (not the pane's live revision) so a frame built ahead
//! is found again when the playhead reaches it.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use lru::LruCache;
use wxdata::clock::Instant;
use wxdata::level2::{BinnedSweep, Moment, Scan};

use crate::render3d::Volume3dUpload;

/// What a Smooth volume was built from: volume name, live revision (0 for a complete archived
/// volume), resampled moment, full range, and loop quality (the smaller grid used for playback).
/// The last element is the storm motion taken off a storm-relative velocity volume (`f32` bits of
/// east and north m/s), `None` when it is ground-relative.
///
/// After it, a hash of the user-defined product drawn instead of the moment (formula and range),
/// `None` for a moment.
pub type SmoothKey = (
    String,
    u64,
    Moment,
    bool,
    bool,
    Option<(u32, u32)>,
    Option<u64>,
);

/// What an isosurface was built from: volume name, live revision (0 for a complete volume),
/// moment, threshold bits, smooth, and the nested-shell spacing bits (`None` = a single shell).
/// The last element is the storm motion, as in [`SmoothKey`].
pub type IsoKey = (
    String,
    u64,
    Moment,
    u32,
    bool,
    Option<u32>,
    Option<(u32, u32)>,
);

/// One isosurface shell: its value, how deep it is nested (0 = outermost), and its mesh.
pub type IsoShell = (f32, usize, wxdata::isosurface::IsoMesh);

/// An LRU bounded by the bytes its values hold rather than by their count: a loop-quality Smooth
/// volume is megabytes and an isosurface a few hundred kilobytes, so a count says nothing about
/// memory.
pub struct ByteLru<K: Hash + Eq, V> {
    map: LruCache<K, (V, usize)>,
    bytes: usize,
    cap_bytes: usize,
}

impl<K: Hash + Eq, V> ByteLru<K, V> {
    pub fn new(cap_bytes: usize) -> Self {
        Self {
            map: LruCache::unbounded(),
            bytes: 0,
            cap_bytes,
        }
    }

    /// Insert `v` (holding `size` bytes), evicting the least recently used until it fits. A value
    /// larger than the whole budget is not kept.
    pub fn insert(&mut self, k: K, v: V, size: usize) {
        if let Some((_, old)) = self.map.pop(&k) {
            self.bytes -= old;
        }
        if size > self.cap_bytes {
            return;
        }
        while self.bytes + size > self.cap_bytes {
            match self.map.pop_lru() {
                Some((_, (_, s))) => self.bytes -= s,
                None => break,
            }
        }
        self.bytes += size;
        self.map.put(k, (v, size));
    }

    /// Look up and mark as recently used.
    pub fn get(&mut self, k: &K) -> Option<&V> {
        self.map.get(k).map(|(v, _)| v)
    }

    /// Whether `k` is held, without touching its recency.
    pub fn contains(&self, k: &K) -> bool {
        self.map.contains(k)
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
    }
}

/// Voxel cap for a Smooth volume built for loop playback: small enough that a loop's worth of
/// frames fits in [`SMOOTH_CACHE_BYTES`] (two bytes a voxel).
pub const SMOOTH_LOOP_MAX_VOXELS: usize =
    if cfg!(any(target_os = "android", target_arch = "wasm32")) {
        2_000_000
    } else {
        6_000_000
    };

/// Memory budget per pane for built Smooth frames.
pub const SMOOTH_CACHE_BYTES: usize = if cfg!(target_os = "android") {
    96 << 20
} else if cfg!(target_arch = "wasm32") {
    40 << 20
} else {
    480 << 20
};

/// Memory budget per pane for built isosurfaces.
pub const ISO_CACHE_BYTES: usize = if cfg!(any(target_os = "android", target_arch = "wasm32")) {
    32 << 20
} else {
    256 << 20
};

/// How many frames ahead of the playhead are built in the background. The web build has no
/// threads, so every build there runs on the page's own thread; it looks only a few frames ahead.
pub const PREBUILD_AHEAD: usize = if cfg!(target_arch = "wasm32") { 4 } else { 24 };

/// A pane's built 3D frames.
pub struct Loop3dCache {
    pub smooth: ByteLru<SmoothKey, Arc<Volume3dUpload>>,
    pub iso: ByteLru<IsoKey, Arc<Vec<IsoShell>>>,
}

impl Default for Loop3dCache {
    fn default() -> Self {
        Self {
            smooth: ByteLru::new(SMOOTH_CACHE_BYTES),
            iso: ByteLru::new(ISO_CACHE_BYTES),
        }
    }
}

/// Bytes an isosurface's shells hold.
pub fn iso_bytes(shells: &[IsoShell]) -> usize {
    shells
        .iter()
        .map(|(_, _, m)| m.verts.len() * 12 + m.tris.len() * 12 + 32)
        .sum()
}

/// Bytes a Smooth volume upload holds.
pub fn smooth_bytes(up: &Volume3dUpload) -> usize {
    up.data.len() + up.lut.len()
}

/// The frame playback moves to after `i`, mirroring [`crate::timeline::Timeline::tick`]: inside
/// a replay window it wraps within it; at the end of the listing a looping live window wraps to
/// its oldest frame and a looping archive to the first; without looping it stops.
pub fn next_frame(
    i: usize,
    len: usize,
    replay: Option<(usize, usize)>,
    following: bool,
    loop_enabled: bool,
    live_window: usize,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if let Some((from, to)) = replay {
        let to = to.min(len - 1);
        return Some(if i >= to { from.min(to) } else { i + 1 });
    }
    if i + 1 < len {
        Some(i + 1)
    } else if loop_enabled {
        Some(if following {
            len.saturating_sub(live_window.max(1))
        } else {
            0
        })
    } else {
        None
    }
}

/// Up to `count` frame indices playback reaches after the playhead, in order, each once.
pub fn upcoming(tl: &crate::timeline::Timeline, count: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = tl.playhead;
    while out.len() < count {
        match next_frame(
            i,
            tl.frames.len(),
            tl.replay,
            tl.following,
            tl.loop_enabled,
            tl.live_window,
        ) {
            Some(n) if n != tl.playhead && !out.contains(&n) => {
                out.push(n);
                i = n;
            }
            _ => break,
        }
    }
    out
}

/// How many 3D builds one pane runs ahead of the playhead at once. Each is a whole-volume
/// resample on a worker thread; the web build has no workers, so there one at a time keeps the
/// page responsive between frames.
pub const MAX_BUILDS_PER_PANE: usize = if cfg!(any(target_arch = "wasm32", target_os = "android")) {
    1
} else {
    2
};

/// How long playback waits for the next frame's 3D before moving on without it, so a build that
/// never lands cannot park the loop.
pub const HOLD_FOR_BUILD: std::time::Duration = std::time::Duration::from_secs(10);

/// The on-map Smooth volume aims for the radar's native 0.25 km gate spacing, then shrinks to fit
/// its voxel cap (two bytes each) and the device's 3D texture limit; see
/// `wxdata::volume3d::plan_grid`.
pub const SMOOTH_TARGET_CELL_KM: f32 = 0.25;
pub const SMOOTH_MAX_VOXELS: usize = if cfg!(target_os = "android") {
    10_000_000
} else if cfg!(target_arch = "wasm32") {
    12_000_000
} else {
    40_000_000
};

/// Isosurface grid: coarser than the smooth volume (a surface is read at a glance, and every
/// triangle is drawn by the painter each frame), capped in voxels and triangles.
pub const ISO_CELL_KM: f32 = 1.0;
pub const ISO_MAX_VOXELS: usize = 1_500_000;
pub const ISO_MAX_TRIS: usize = 60_000;

/// Reflectivity below which the 3D ZDR and KDP volumes are masked out: the polarimetric fields
/// are noise in weaker echo (ROADMAP_NEW H1/H2 quality mask).
pub const POLARIMETRIC_MASK_DBZ: f32 = 20.0;

/// Whether `moment`'s Smooth volume is masked by reflectivity.
pub fn masked_by_reflectivity(moment: Moment) -> bool {
    matches!(
        moment,
        Moment::DifferentialReflectivity | Moment::SpecificDifferentialPhase
    )
}

/// The sweeps a build reads: already binned (the displayed volume, whose binned sweeps the pane
/// caches), or a decoded scan from the download cache, binned on the worker (a frame ahead of the
/// playhead, which no pane has binned).
pub enum Sweeps {
    Binned {
        sweeps: Vec<BinnedSweep>,
        mask: Option<Vec<BinnedSweep>>,
    },
    Scan(Arc<Scan>),
}

impl Sweeps {
    /// The moment's tilts, lowest first (velocity dealiased when asked), and the reflectivity
    /// tilts that mask it when `masked`.
    fn resolve(
        self,
        moment: Moment,
        dealias: bool,
        masked: bool,
    ) -> (Vec<BinnedSweep>, Option<Vec<BinnedSweep>>) {
        match self {
            Sweeps::Binned { sweeps, mask } => (sweeps, mask),
            Sweeps::Scan(scan) => {
                let mut v = crate::view::Volume::new(scan, String::new(), chrono::Utc::now());
                let sweeps = if dealias && moment == Moment::Velocity {
                    v.velocity_tilts_dealiased()
                } else {
                    v.moment_tilts(moment)
                };
                let mask = masked.then(|| v.moment_tilts(Moment::Reflectivity));
                (sweeps, mask)
            }
        }
    }
}

/// Everything a Smooth build needs besides its sweeps.
pub struct SmoothSpec {
    pub moment: Moment,
    /// Inverted index (the CC Debris volume: low CC wins the maximum).
    pub invert: bool,
    /// Keep every gate rather than cropping the box to the echo.
    pub full_range: bool,
    pub table: crate::colormap::ColorTable,
    /// The device's 3D texture edge limit.
    pub max_dim: usize,
    pub max_voxels: usize,
    pub top_km: f32,
    /// Storm motion `(east, north)` m/s to take off a velocity volume (storm-relative).
    pub storm_uv: Option<(f32, f32)>,
    /// A user-defined product to build instead of `moment` (ROADMAP_NEW H1).
    pub product: Option<ProductSpec>,
}

/// A user-defined product (`wxdata::udp`) to resample in 3D: its formula, the range its palette
/// spans (`None` fits it to what the product produced) and the site facts the formula can read.
#[derive(Clone)]
pub struct ProductSpec {
    pub expr: wxdata::udp::Expr,
    pub range: Option<(f32, f32)>,
    pub env: wxdata::udp_volume::Env,
    /// The colour table it is drawn in; `None` is a plain ramp across its range.
    pub table: Option<crate::colormap::ColorTable>,
}

/// The product evaluated at every gate of every tilt, quantized over one range
/// (`wxdata::udp_volume`). Only a whole scan has every moment to read.
fn product_sweeps(sweeps: Sweeps, p: &ProductSpec) -> Option<Vec<BinnedSweep>> {
    let Sweeps::Scan(scan) = sweeps else {
        return None;
    };
    let mut v = crate::view::Volume::new(scan, String::new(), chrono::Utc::now());
    let n = v.elevations.len();
    let reads = p.expr.inputs();
    let moments = wxdata::udp_volume::MOMENTS;
    let input_of = |m: Moment| match m {
        Moment::Velocity => wxdata::udp::Input::Velocity,
        Moment::SpectrumWidth => wxdata::udp::Input::SpectrumWidth,
        Moment::DifferentialReflectivity => wxdata::udp::Input::DifferentialReflectivity,
        Moment::SpecificDifferentialPhase => wxdata::udp::Input::SpecificDifferentialPhase,
        Moment::CorrelationCoefficient => wxdata::udp::Input::CorrelationCoefficient,
        _ => wxdata::udp::Input::Reflectivity,
    };
    let reads_any = moments.iter().any(|m| reads.contains(&input_of(*m)));
    // Bin only the moments the formula reads (reflectivity for a geometry-only one); velocity
    // dealiased, so a fold is not a false value.
    let per: [Vec<Option<BinnedSweep>>; 6] = std::array::from_fn(|i| {
        let m = moments[i];
        let wanted = reads.contains(&input_of(m)) || (!reads_any && i == 0);
        if !wanted {
            return Vec::new();
        }
        (0..n)
            .map(|t| v.binned(m, t, m == Moment::Velocity).ok().cloned())
            .collect()
    });
    let evaluated: Vec<_> = wxdata::udp_volume::pair_tilts(&per)
        .iter()
        .filter_map(|tilt| wxdata::udp_volume::evaluate_tilt(&p.expr, tilt, p.env))
        .collect();
    wxdata::udp_volume::quantize(evaluated, p.range).map(|(s, _)| s)
}

/// Take the storm motion off each velocity sweep, when there is one.
fn apply_storm_motion(sweeps: &mut [BinnedSweep], moment: Moment, uv: Option<(f32, f32)>) {
    if let (Moment::Velocity, Some((u, v))) = (moment, uv) {
        for s in sweeps {
            wxdata::level2::storm_relative(s, u, v);
        }
    }
}

/// Resample `sweeps` into the Smooth volume the map raymarches.
pub fn build_smooth(sweeps: Sweeps, spec: &SmoothSpec) -> Option<Volume3dUpload> {
    let masked = masked_by_reflectivity(spec.moment) && spec.product.is_none();
    let (mut sweeps, mask) = match &spec.product {
        Some(p) => (product_sweeps(sweeps, p)?, None),
        None => sweeps.resolve(spec.moment, true, masked),
    };
    if sweeps.is_empty() {
        return None;
    }
    if spec.product.is_none() {
        apply_storm_motion(&mut sweeps, spec.moment, spec.storm_uv);
    }
    // Derive the volume's horizontal extent from what this scan actually sampled instead of a
    // fixed radius that clipped far storms out of the volume.
    let full_km = wxdata::volume3d::max_sample_range_km(&sweeps).max(50.0);
    // Crop the box to where the echo is (and say how much that leaves out) unless the user asked
    // for every gate.
    let (half_km, outside) = if spec.full_range {
        (full_km, 0.0)
    } else {
        let e = wxdata::volume3d::echo_extent_km(&sweeps, full_km);
        (e.half_km, e.outside as f32)
    };
    let (n, nz) = wxdata::volume3d::plan_grid(
        half_km,
        spec.top_km,
        SMOOTH_TARGET_CELL_KM,
        spec.max_voxels,
        spec.max_dim,
    );
    let mut v3 = wxdata::volume3d::build(&sweeps, n, nz, half_km, spec.top_km)?;
    // ZDR and KDP are noise in weak echo, and a maximum-intensity raymarch finds the noise
    // first; they are masked by reflectivity on the same grid.
    if let Some(mask) = mask.as_deref() {
        if let Some(refl) = wxdata::volume3d::build(mask, n, nz, half_km, spec.top_km) {
            wxdata::volume3d::mask_by(&mut v3, &refl, POLARIMETRIC_MASK_DBZ);
        }
    }
    if spec.invert {
        wxdata::volume3d::invert_in_place(&mut v3);
    }
    let product_range = spec
        .product
        .is_some()
        .then_some((v3.value_min, v3.value_max));
    let lut = if let Some(range) = product_range {
        // A product's own colour table, else a ramp across the range it was drawn over.
        let ramp;
        let table = match spec.product.as_ref().and_then(|p| p.table.as_ref()) {
            Some(t) => t,
            None => {
                ramp = crate::colormap::ramp_table(range.0, range.1);
                &ramp
            }
        };
        crate::colormap::bake_lut(table, range, None)
    } else if spec.moment == Moment::Velocity {
        // Indexed by speed so the maximum along a ray finds the fastest wind either way.
        wxdata::volume3d::fold_by_speed(&mut v3);
        crate::colormap::speed_lut(&spec.table, v3.value_max)
    } else {
        let lut = crate::colormap::bake_lut(&spec.table, (v3.value_min, v3.value_max), None);
        if spec.invert {
            crate::colormap::invert_lut(lut)
        } else {
            lut
        }
    };
    Some(Volume3dUpload {
        data: crate::render3d::pack_rg8(&v3.data),
        n: v3.n as u32,
        nz: v3.nz as u32,
        lut: lut.to_vec(),
        half_km: v3.half_km,
        top_km: v3.top_km,
        outside,
        value_range: product_range,
    })
}

/// Everything an isosurface build needs besides its sweeps.
pub struct IsoSpec {
    pub moment: Moment,
    pub value: f32,
    /// Nested-shell spacing; `None` = a single shell.
    pub step: Option<f32>,
    pub smooth: bool,
    pub max_dim: usize,
    pub top_km: f32,
    /// Storm motion to take off a velocity volume, as in [`SmoothSpec`].
    pub storm_uv: Option<(f32, f32)>,
}

/// Build the isosurface shells of `sweeps`. Velocity is dealiased first, so a folded couplet does
/// not grow false inbound/outbound shells.
pub fn build_iso(sweeps: Sweeps, spec: &IsoSpec) -> Option<Vec<IsoShell>> {
    let (mut sweeps, _) = sweeps.resolve(spec.moment, true, false);
    if sweeps.is_empty() {
        return None;
    }
    apply_storm_motion(&mut sweeps, spec.moment, spec.storm_uv);
    let full = wxdata::volume3d::max_sample_range_km(&sweeps).max(50.0);
    let half = wxdata::volume3d::echo_extent_km(&sweeps, full).half_km;
    let (n, nz) =
        wxdata::volume3d::plan_grid(half, spec.top_km, ISO_CELL_KM, ISO_MAX_VOXELS, spec.max_dim);
    let v3 = wxdata::volume3d::build(&sweeps, n, nz, half, spec.top_km)?;
    let shells = iso_shells(spec.moment, spec.value, spec.step);
    // One triangle budget shared by every shell: the painter draws them all each frame.
    let budget = ISO_MAX_TRIS / shells.len().max(1);
    Some(
        shells
            .into_iter()
            .map(|(value, depth, high_inside)| {
                let mut mesh = wxdata::isosurface::isosurface(&v3, value, high_inside, budget);
                if spec.smooth {
                    wxdata::isosurface::smooth(&mut mesh, 2);
                }
                (value, depth, mesh)
            })
            .filter(|(_, _, m)| !m.tris.is_empty())
            .collect(),
    )
}

/// The shells to draw for `moment` at `value`: `(value, nesting depth, high inside)`, outermost
/// first. Velocity is always a pair — outbound at `+value` and inbound at `-value` — since a
/// single velocity threshold would show only one side of a couplet. With `step`, two more shells
/// sit inside each one, `step` and `2·step` further in (lower for CC, whose low side is inside).
pub fn iso_shells(moment: Moment, value: f32, step: Option<f32>) -> Vec<(f32, usize, bool)> {
    let depths = if step.is_some() { 3 } else { 1 };
    let step = step.unwrap_or(0.0).abs();
    let mut out = Vec::new();
    for d in 0..depths {
        let off = step * d as f32;
        match moment {
            Moment::Velocity => {
                let v = value.abs() + off;
                out.push((v, d, true));
                out.push((-v, d, false));
            }
            // Low CC is the interesting side (debris); every other moment is high-inside.
            Moment::CorrelationCoefficient => out.push((value - off, d, false)),
            _ => out.push((value + off, d, true)),
        }
    }
    out
}

/// One 3D build: which pane it is for and what it builds.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum JobKey {
    Smooth(usize, SmoothKey),
    Iso(usize, IsoKey),
}

impl JobKey {
    fn pane(&self) -> usize {
        match self {
            JobKey::Smooth(p, _) | JobKey::Iso(p, _) => *p,
        }
    }
}

/// A finished build (`None`: nothing to show — no sweeps, or nothing crossed the threshold).
pub enum Built {
    Smooth(Option<Volume3dUpload>),
    Iso(Option<Vec<IsoShell>>),
}

/// Every pane's 3D builds in flight. The displayed frame's build and those ahead of the playhead
/// all run through here and land in the pane's [`Loop3dCache`], so a frame built ahead is a
/// lookup when playback reaches it, and a displayed frame's build is never thrown away when the
/// playhead moves on before it finishes.
pub struct Loop3dJobs {
    tx: Sender<(JobKey, Built)>,
    rx: Receiver<(JobKey, Built)>,
    inflight: HashMap<JobKey, Instant>,
    /// Builds that produced nothing, so they are not started again every frame.
    empty: HashSet<JobKey>,
}

impl Default for Loop3dJobs {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            tx,
            rx,
            inflight: HashMap::new(),
            empty: HashSet::new(),
        }
    }
}

impl Loop3dJobs {
    /// Whether `key` needs starting: not running, and not already known to be empty.
    pub fn wants(&self, key: &JobKey) -> bool {
        !self.inflight.contains_key(key) && !self.empty.contains(key)
    }

    /// Whether `key` was built and produced nothing to show.
    pub fn came_up_empty(&self, key: &JobKey) -> bool {
        self.empty.contains(key)
    }

    /// When `key`'s build started, if it is running.
    pub fn started(&self, key: &JobKey) -> Option<Instant> {
        self.inflight.get(key).copied()
    }

    /// Builds running for `pane`.
    pub fn running(&self, pane: usize) -> usize {
        self.inflight.keys().filter(|k| k.pane() == pane).count()
    }

    /// Run `job` off the UI thread and deliver what it builds under `key`.
    pub fn start(
        &mut self,
        key: JobKey,
        spawner: &crate::rt::Spawner,
        ctx: &egui::Context,
        job: impl FnOnce() -> Built + Send + 'static,
    ) {
        self.inflight.insert(key.clone(), Instant::now());
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let smooth = matches!(key, JobKey::Smooth(..));
        spawner.spawn(async move {
            // A panicked build still answers, so its key never stays booked.
            let built = wxdata::task::blocking(job).await.unwrap_or(if smooth {
                Built::Smooth(None)
            } else {
                Built::Iso(None)
            });
            let _ = tx.send((key, built));
            ctx.request_repaint();
        });
    }

    /// Move finished builds into their panes' caches.
    pub fn drain(&mut self, caches: &mut [Loop3dCache]) {
        while let Ok((key, built)) = self.rx.try_recv() {
            let took = self.inflight.remove(&key).map(|t| t.elapsed());
            log::debug!(
                "3d build {key:?}: {} in {took:?}",
                match &built {
                    Built::Smooth(Some(_)) | Built::Iso(Some(_)) => "built",
                    _ => "empty",
                }
            );
            if self.empty.len() > 512 {
                self.empty.clear();
            }
            let Some(cache) = caches.get_mut(key.pane()) else {
                continue;
            };
            match (key, built) {
                (JobKey::Smooth(_, k), Built::Smooth(Some(up))) => {
                    let size = smooth_bytes(&up);
                    cache.smooth.insert(k, Arc::new(up), size);
                }
                (JobKey::Iso(_, k), Built::Iso(Some(shells))) => {
                    let size = iso_bytes(&shells);
                    cache.iso.insert(k, Arc::new(shells), size);
                }
                (key, _) => {
                    self.empty.insert(key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_lru_evicts_oldest_until_the_new_value_fits() {
        let mut c: ByteLru<u32, &str> = ByteLru::new(100);
        c.insert(1, "a", 40);
        c.insert(2, "b", 40);
        assert!(c.get(&1).is_some()); // 1 is now the most recent
        c.insert(3, "c", 40);
        assert!(!c.contains(&2), "least recently used goes first");
        assert!(c.contains(&1) && c.contains(&3));
        assert_eq!(c.bytes(), 80);
        // Too big for the whole budget: not kept, nothing evicted for it.
        c.insert(4, "d", 200);
        assert!(!c.contains(&4));
        assert_eq!(c.bytes(), 80);
        // Replacing a key frees its old size.
        c.insert(1, "a2", 10);
        assert_eq!(c.bytes(), 50);
    }

    #[test]
    fn iso_shells_pair_velocity_and_nest_inward() {
        assert_eq!(
            iso_shells(Moment::Reflectivity, 50.0, None),
            vec![(50.0, 0, true)]
        );
        assert_eq!(
            iso_shells(Moment::Reflectivity, 40.0, Some(10.0)),
            vec![(40.0, 0, true), (50.0, 1, true), (60.0, 2, true)]
        );
        // Velocity: outbound and inbound, whichever sign the threshold was given.
        assert_eq!(
            iso_shells(Moment::Velocity, -20.0, None),
            vec![(20.0, 0, true), (-20.0, 0, false)]
        );
        // CC nests toward lower values, low inside.
        let cc = iso_shells(Moment::CorrelationCoefficient, 0.8, Some(0.1));
        assert_eq!(cc.len(), 3);
        assert!(cc.iter().all(|s| !s.2));
        assert!((cc[2].0 - 0.6).abs() < 1e-6);
    }

    #[test]
    fn next_frame_follows_the_loop_rules() {
        // Archive loop wraps to the start; live loop to the oldest frame of its window.
        assert_eq!(next_frame(9, 10, None, false, true, 4), Some(0));
        assert_eq!(next_frame(9, 10, None, true, true, 4), Some(6));
        assert_eq!(next_frame(9, 10, None, false, false, 4), None);
        assert_eq!(next_frame(3, 10, None, false, true, 4), Some(4));
        // Replay window wraps inside itself.
        assert_eq!(next_frame(5, 10, Some((2, 5)), false, true, 4), Some(2));
        assert_eq!(next_frame(2, 10, Some((2, 5)), false, false, 4), Some(3));
        assert_eq!(next_frame(0, 0, None, false, true, 4), None);
    }

    #[test]
    fn upcoming_lists_each_frame_of_a_live_loop_once() {
        let mut tl = crate::timeline::Timeline::default();
        tl.frames = Vec::new();
        assert!(upcoming(&tl, 5).is_empty());
    }
}
