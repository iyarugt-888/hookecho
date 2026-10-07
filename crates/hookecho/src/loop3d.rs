//! 3D loop animation (ROADMAP_NEW H8): the built 3D frames a pane keeps so a loop plays its 3D
//! view frame for frame, and which frames playback reaches next so they can be built ahead.
//!
//! A Smooth volume or an isosurface takes from a fraction of a second to a few seconds to build,
//! far longer than a 4 fps frame lasts. Without a cache the 3D view lags the loop and shows one
//! volume's 3D under another volume's time; with one, every frame after the first pass is a
//! lookup. Every cached frame owns its actual decoded scan/revision and temporal policy.
//! Complete prefetch frames use continuous inputs and share the same decoded source at playback.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use lru::LruCache;
use wxdata::clock::Instant;
use wxdata::level2::temporal::{self, TemporalCoverage, TemporalPolicy};
use wxdata::level2::{BinnedSweep, Moment, Scan};

use crate::render3d::Volume3dUpload;

/// A selected decoded source, including independent same-name scans. Weak identity does not
/// retain gate buffers. This runtime key is never persisted as scientific provenance.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceKey {
    pub name: String,
    pub revision: u64,
    pub policy: TemporalPolicy,
    site: Option<String>,
    scan: crate::volume::ScanIdentity,
    acquisition: Option<crate::live_scan::AcquisitionSnapshot>,
}

impl SourceKey {
    pub(crate) fn new(
        site: Option<String>,
        name: String,
        revision: u64,
        scan: &Arc<Scan>,
        policy: TemporalPolicy,
    ) -> Self {
        Self {
            site,
            name,
            revision,
            scan: crate::volume::ScanIdentity::new(scan),
            acquisition: None,
            policy,
        }
    }

    pub(crate) fn for_view(view: &crate::view::MapView, policy: TemporalPolicy) -> Option<Self> {
        let volume = view.volume.as_ref()?;
        let mut key = Self::new(
            view.site.clone(),
            volume.name.clone(),
            volume.revision(),
            &volume.scan,
            policy,
        );
        key.acquisition = volume.acquisition_for(view.site.as_deref()).cloned();
        Some(key)
    }

    pub(crate) fn acquisition(&self) -> Option<&crate::live_scan::AcquisitionSnapshot> {
        self.acquisition.as_ref()
    }

    fn acquisition_bytes(&self) -> usize {
        self.acquisition
            .as_ref()
            .map_or(0, crate::live_scan::AcquisitionSnapshot::estimated_bytes)
    }
}

/// Every input affecting a resampled map volume. Camera/opacity/clipping are draw uniforms.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SmoothKey {
    pub source: SourceKey,
    pub moment: Moment,
    pub full_range: bool,
    pub loop_quality: bool,
    pub storm_uv: Option<(u32, u32)>,
    pub product: Option<u64>,
    pub palette: u64,
    pub high_contrast: bool,
    pub max_dim: usize,
    /// The region of interest, when one replaces the whole radar ([`Roi::key`]).
    pub roi: Option<[u32; 3]>,
}

/// Mesh inputs; palette, opacity and lighting are applied at paint time.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IsoKey {
    pub source: SourceKey,
    pub moment: Moment,
    pub value: u32,
    pub smooth: bool,
    pub step: Option<u32>,
    pub storm_uv: Option<(u32, u32)>,
    pub max_dim: usize,
}

/// Cached samples and the contributor metadata prepared for those samples.
pub struct SmoothFrame {
    pub upload: Arc<Volume3dUpload>,
    pub coverage: TemporalCoverage,
}

pub struct IsoFrame {
    pub shells: Vec<IsoShell>,
    pub coverage: TemporalCoverage,
}

fn coverage_bytes(coverage: &TemporalCoverage) -> usize {
    coverage.contributors.capacity() * std::mem::size_of::<temporal::SweepCoverage>()
        + coverage
            .contributors
            .iter()
            .filter_map(|sweep| sweep.native_passes.as_ref())
            .map(temporal::PassContributors::estimated_dynamic_bytes)
            .sum::<usize>()
}

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
    pub smooth: ByteLru<SmoothKey, Arc<SmoothFrame>>,
    pub iso: ByteLru<IsoKey, Arc<IsoFrame>>,
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
        .map(|(_, _, m)| {
            m.verts.capacity() * 12 + m.tris.capacity() * 12 + std::mem::size_of::<IsoShell>()
        })
        .sum()
}

/// Bytes a Smooth volume upload holds.
pub fn smooth_bytes(up: &Volume3dUpload) -> usize {
    up.data.capacity() + up.lut.capacity() + std::mem::size_of::<Volume3dUpload>()
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

/// How many current-frame and prefetch 3D builds one pane runs at once. Each is a whole-volume
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
    Captured {
        scan: Arc<Scan>,
        passes: Arc<wxdata::live_pass::PassAttributionIndex>,
    },
}

impl Sweeps {
    pub(crate) fn captured(
        scan: Arc<Scan>,
        receipt: Option<&crate::live_scan::AcquisitionSnapshot>,
    ) -> Self {
        if let Some(passes) =
            receipt.and_then(|receipt| receipt.inventory().source_attribution.clone())
        {
            Self::Captured { scan, passes }
        } else {
            Self::Scan(scan)
        }
    }

    fn passes(&self) -> Option<Arc<wxdata::live_pass::PassAttributionIndex>> {
        match self {
            Self::Captured { passes, .. } => Some(passes.clone()),
            _ => None,
        }
    }

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
            Sweeps::Scan(scan) | Sweeps::Captured { scan, .. } => {
                let mut v =
                    crate::view::Volume::new(scan, String::new(), chrono::DateTime::UNIX_EPOCH);
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
    /// A region of interest instead of the whole radar (ROADMAP_PARITY M3.6).
    pub roi: Option<Roi>,
}

/// A box around one storm or place: its centre, km east and north of the radar, and its
/// half-width, km. Spending the voxel budget on it gives finer cells than the whole radar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roi {
    pub center_km: [f32; 2],
    pub half_km: f32,
}

impl Roi {
    /// Cache identity: the floats' bits.
    pub fn key(self) -> [u32; 3] {
        [
            self.center_km[0].to_bits(),
            self.center_km[1].to_bits(),
            self.half_km.to_bits(),
        ]
    }
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
fn product_sweeps(
    sweeps: Sweeps,
    p: &ProductSpec,
    policy: TemporalPolicy,
    passes: Option<&wxdata::live_pass::PassAttributionIndex>,
) -> Option<(Vec<BinnedSweep>, TemporalCoverage)> {
    let scan = match sweeps {
        Sweeps::Scan(scan) | Sweeps::Captured { scan, .. } => scan,
        _ => return None,
    };
    let mut v = crate::view::Volume::new(scan, String::new(), chrono::DateTime::UNIX_EPOCH);
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
    let mut per: [Vec<Option<BinnedSweep>>; 6] = std::array::from_fn(|i| {
        let m = moments[i];
        let wanted = reads.contains(&input_of(m)) || (!reads_any && i == 0);
        if !wanted {
            return Vec::new();
        }
        (0..n)
            .map(|t| v.binned(m, t, m == Moment::Velocity).ok().cloned())
            .collect()
    });
    let mut coverage = TemporalCoverage {
        policy,
        contributors: Vec::new(),
    };
    for moment in &mut per {
        for sweep in moment.iter_mut().flatten() {
            coverage.contributors.extend(
                temporal::prepare_with_passes(std::slice::from_mut(sweep), policy, passes)
                    .ok()?
                    .contributors,
            );
        }
    }
    let evaluated: Vec<_> = wxdata::udp_volume::pair_tilts(&per)
        .iter()
        .filter_map(|tilt| {
            let (base, mut values) = wxdata::udp_volume::evaluate_tilt(&p.expr, tilt, p.env)?;
            // Geometry-only formulas can produce values without an echo. They must not
            // refill rows excluded by the source policy, even before auto-range fitting.
            if !reads_any && policy == TemporalPolicy::StrictCurrent {
                let mut eligible = vec![2; base.data.len()];
                wxdata::level2::mask_previous_pass_rows(&base, &mut eligible);
                for (value, eligible) in values.iter_mut().zip(eligible) {
                    if eligible == 0 {
                        *value = None;
                    }
                }
            }
            Some((base, values))
        })
        .collect();
    let (sweeps, _) = wxdata::udp_volume::quantize(evaluated, p.range)?;
    Some((sweeps, coverage))
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
    let frame = build_smooth_covered(sweeps, spec, TemporalPolicy::Continuous)?;
    Arc::try_unwrap(frame.upload).ok()
}

/// Prepare all contributing inputs before product evaluation, masking or interpolation.
pub fn build_smooth_covered(
    sweeps: Sweeps,
    spec: &SmoothSpec,
    policy: TemporalPolicy,
) -> Option<SmoothFrame> {
    let passes = sweeps.passes();
    let masked = masked_by_reflectivity(spec.moment) && spec.product.is_none();
    let (mut sweeps, mask, coverage) = match &spec.product {
        Some(p) => {
            let (sweeps, coverage) = product_sweeps(sweeps, p, policy, passes.as_deref())?;
            (sweeps, None, coverage)
        }
        None => {
            let (mut sweeps, mut mask) = sweeps.resolve(spec.moment, true, masked);
            let mut coverage =
                temporal::prepare_with_passes(&mut sweeps, policy, passes.as_deref()).ok()?;
            if let Some(mask) = &mut mask {
                coverage.contributors.extend(
                    temporal::prepare_with_passes(mask, policy, passes.as_deref())
                        .ok()?
                        .contributors,
                );
            }
            (sweeps, mask, coverage)
        }
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
    // A region of interest sets the box itself: nothing outside it is claimed to be in view, and
    // its smaller box is what buys the finer cells.
    let (half_km, outside) = if let Some(roi) = spec.roi {
        (roi.half_km.max(1.0), 0.0)
    } else if spec.full_range {
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
    let center = spec.roi.map_or([0.0, 0.0], |r| r.center_km);
    let mut v3 = wxdata::volume3d::build_at(&sweeps, n, nz, center, half_km, spec.top_km)?;
    // ZDR and KDP are noise in weak echo, and a maximum-intensity raymarch finds the noise
    // first; they are masked by reflectivity on the same grid.
    if let Some(mask) = mask.as_deref() {
        if let Some(refl) = wxdata::volume3d::build_at(mask, n, nz, center, half_km, spec.top_km) {
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
    Some(SmoothFrame {
        coverage,
        upload: Arc::new(Volume3dUpload {
            data: crate::render3d::pack_rg8(&v3.data),
            n: v3.n as u32,
            nz: v3.nz as u32,
            lut: lut.to_vec(),
            half_km: v3.half_km,
            center_km: center,
            top_km: v3.top_km,
            outside,
            value_range: product_range,
        }),
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
    build_iso_covered(sweeps, spec, TemporalPolicy::Continuous).map(|frame| frame.shells)
}

pub fn build_iso_covered(
    sweeps: Sweeps,
    spec: &IsoSpec,
    policy: TemporalPolicy,
) -> Option<IsoFrame> {
    let passes = sweeps.passes();
    let (mut sweeps, _) = sweeps.resolve(spec.moment, true, false);
    let coverage = temporal::prepare_with_passes(&mut sweeps, policy, passes.as_deref()).ok()?;
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
    Some(IsoFrame {
        coverage,
        shells: shells
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
    })
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

/// A finished build (`None`: unavailable inputs or a failed worker). Valid empty meshes retain coverage.
pub enum Built {
    Smooth(Option<SmoothFrame>),
    Iso(Option<IsoFrame>),
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
    /// Whether `key` needs starting and shares available capacity with all jobs in its pane.
    pub fn wants(&self, key: &JobKey) -> bool {
        !self.inflight.contains_key(key)
            && !self.empty.contains(key)
            && self.running(key.pane()) < MAX_BUILDS_PER_PANE
    }

    /// Whether `key` was built and produced nothing to show.
    pub fn came_up_empty(&self, key: &JobKey) -> bool {
        self.empty.contains(key)
    }

    /// Explicit retry of an empty/failed selection; does not disturb in-flight work.
    pub fn retry(&mut self, key: &JobKey) {
        self.empty.remove(key);
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
        if !self.wants(&key) {
            return;
        }
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
                (JobKey::Smooth(_, k), Built::Smooth(Some(up)))
                    if k.source.policy == up.coverage.policy =>
                {
                    let size = smooth_bytes(&up.upload)
                        + coverage_bytes(&up.coverage)
                        + k.source.acquisition_bytes()
                        + std::mem::size_of::<SmoothFrame>();
                    cache.smooth.insert(k, Arc::new(up), size);
                }
                (JobKey::Iso(_, k), Built::Iso(Some(shells)))
                    if k.source.policy == shells.coverage.policy =>
                {
                    let size = iso_bytes(&shells.shells)
                        + coverage_bytes(&shells.coverage)
                        + k.source.acquisition_bytes()
                        + std::mem::size_of::<IsoFrame>()
                        + (shells.shells.capacity() - shells.shells.len())
                            * std::mem::size_of::<IsoShell>();
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

    fn mixed_inputs(moment: Moment) -> Vec<BinnedSweep> {
        [0.5, 1.5]
            .into_iter()
            .map(|elevation_deg| {
                let (value_min, value_max) = moment.value_range();
                BinnedSweep {
                    moment,
                    az_bins: 8,
                    gate_count: 8,
                    data: vec![180; 64],
                    first_gate_km: 5.0,
                    gate_interval_km: 5.0,
                    elevation_deg,
                    value_min,
                    value_max,
                    bin_time_ms: [vec![1_700_000_120_000; 4], vec![1_700_000_000_000; 4]].concat(),
                    ..Default::default()
                }
            })
            .collect()
    }

    fn smooth_spec(moment: Moment) -> SmoothSpec {
        SmoothSpec {
            roi: None,
            moment,
            invert: false,
            full_range: true,
            table: crate::colormap::default_table(moment).clone(),
            max_dim: 32,
            max_voxels: 32 * 32 * 16,
            top_km: 6.0,
            storm_uv: None,
            product: None,
        }
    }

    #[test]
    fn a_region_of_interest_builds_finer_cells_where_it_is_put() {
        let inputs = mixed_inputs(Moment::Reflectivity);
        let whole_spec = smooth_spec(Moment::Reflectivity);
        let whole = build_smooth_covered(
            Sweeps::Binned {
                sweeps: inputs.clone(),
                mask: None,
            },
            &whole_spec,
            TemporalPolicy::Continuous,
        )
        .unwrap();
        let roi = Roi {
            center_km: [10.0, 5.0],
            half_km: 8.0,
        };
        let spec = SmoothSpec {
            roi: Some(roi),
            ..smooth_spec(Moment::Reflectivity)
        };
        let region = build_smooth_covered(
            Sweeps::Binned {
                sweeps: inputs.clone(),
                mask: None,
            },
            &spec,
            TemporalPolicy::Continuous,
        )
        .unwrap();
        let (w, r) = (&whole.upload, &region.upload);
        assert_eq!(r.center_km, [10.0, 5.0]);
        assert_eq!((w.center_km, r.half_km, r.outside), ([0.0, 0.0], 8.0, 0.0));
        assert!(
            r.cell_km() < w.cell_km() / 2.0,
            "{} km cells vs {} km",
            r.cell_km(),
            w.cell_km()
        );
        // The grid is exactly the centred wxdata build.
        let reference = wxdata::volume3d::build_at(
            &inputs,
            r.n as usize,
            r.nz as usize,
            roi.center_km,
            roi.half_km,
            spec.top_km,
        )
        .unwrap();
        assert_eq!(r.data, crate::render3d::pack_rg8(&reference.data));
        // Region and whole radar are different cache entries.
        assert_ne!(Some(roi.key()), None::<[u32; 3]>);
        assert_ne!(
            Roi {
                half_km: 9.0,
                ..roi
            }
            .key(),
            roi.key(),
            "a resized region rebuilds"
        );
    }

    #[test]
    fn smooth_strict_prepares_inputs_before_interpolation_and_preserves_continuous_samples() {
        let inputs = mixed_inputs(Moment::Reflectivity);
        let spec = smooth_spec(Moment::Reflectivity);
        let continuous = build_smooth_covered(
            Sweeps::Binned {
                sweeps: inputs.clone(),
                mask: None,
            },
            &spec,
            TemporalPolicy::Continuous,
        )
        .unwrap();
        let reference = wxdata::volume3d::build(
            &inputs,
            continuous.upload.n as usize,
            continuous.upload.nz as usize,
            continuous.upload.half_km,
            spec.top_km,
        )
        .unwrap();
        assert_eq!(
            continuous.upload.data,
            crate::render3d::pack_rg8(&reference.data)
        );
        assert_eq!(continuous.coverage.retained_older_rows(), 8);
        let strict = build_smooth_covered(
            Sweeps::Binned {
                sweeps: inputs.clone(),
                mask: None,
            },
            &spec,
            TemporalPolicy::StrictCurrent,
        )
        .unwrap();
        assert_eq!(strict.coverage.excluded_rows(), 8);
        assert_eq!(
            strict.coverage.acquisition_range_ms(),
            Some((1_700_000_120_000, 1_700_000_120_000))
        );
        let mut removed = 0;
        let mut kept = 0;
        let n = continuous.upload.n as usize;
        for (voxel, (old, new)) in continuous
            .upload
            .data
            .as_chunks::<2>()
            .0
            .iter()
            .zip(strict.upload.data.as_chunks::<2>().0.iter())
            .enumerate()
        {
            if old[0] >= 2 {
                if voxel % n < n / 2 {
                    assert_eq!(*new, [0, 0]);
                    removed += 1;
                } else {
                    assert_eq!(new, old);
                    kept += 1;
                }
            }
        }
        assert!(
            removed > 100 && kept > 100,
            "exercise both sectors: {removed}/{kept}"
        );
        assert!(
            inputs.iter().all(|s| s.data.iter().all(|v| *v == 180)),
            "cached inputs are immutable"
        );
    }

    #[test]
    fn reflectivity_quality_mask_contributes_its_own_policy_and_clocks() {
        let mut primary = mixed_inputs(Moment::DifferentialReflectivity);
        for s in &mut primary {
            s.bin_time_ms.fill(1_700_000_240_000);
        }
        let mask = mixed_inputs(Moment::Reflectivity);
        let spec = smooth_spec(Moment::DifferentialReflectivity);
        let frame = build_smooth_covered(
            Sweeps::Binned {
                sweeps: primary,
                mask: Some(mask),
            },
            &spec,
            TemporalPolicy::StrictCurrent,
        )
        .unwrap();
        assert_eq!(frame.coverage.contributors.len(), 4);
        assert_eq!(frame.coverage.excluded_rows(), 8);
        assert_eq!(
            frame.coverage.acquisition_range_ms(),
            Some((1_700_000_120_000, 1_700_000_240_000))
        );
        let n = frame.upload.n as usize;
        assert!(frame
            .upload
            .data
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
            .filter(|(v, _)| v % n < n / 2)
            .all(|(_, v)| *v == [0, 0]));
        assert!(frame
            .upload
            .data
            .as_chunks::<2>()
            .0
            .iter()
            .any(|v| v[0] >= 2));
    }

    #[test]
    fn isosurfaces_mesh_only_the_prepared_inputs_and_retain_empty_frame_coverage() {
        let inputs = mixed_inputs(Moment::Reflectivity);
        let spec = IsoSpec {
            moment: Moment::Reflectivity,
            value: 30.0,
            step: Some(5.0),
            smooth: true,
            max_dim: 32,
            top_km: 6.0,
            storm_uv: None,
        };
        let strict = build_iso_covered(
            Sweeps::Binned {
                sweeps: inputs.clone(),
                mask: None,
            },
            &spec,
            TemporalPolicy::StrictCurrent,
        )
        .unwrap();
        let mut prepared = inputs.clone();
        temporal::prepare(&mut prepared, TemporalPolicy::StrictCurrent).unwrap();
        let reference = build_iso(
            Sweeps::Binned {
                sweeps: prepared,
                mask: None,
            },
            &spec,
        )
        .unwrap();
        assert!(!strict.shells.is_empty());
        assert_eq!(strict.shells.len(), reference.len());
        for ((_, _, actual), (_, _, expected)) in strict.shells.iter().zip(&reference) {
            assert_eq!(actual.verts, expected.verts);
            assert_eq!(actual.tris, expected.tris);
        }
        assert_eq!(strict.coverage.excluded_rows(), 8);
        let empty = build_iso_covered(
            Sweeps::Binned {
                sweeps: inputs,
                mask: None,
            },
            &IsoSpec {
                value: 100.0,
                ..spec
            },
            TemporalPolicy::StrictCurrent,
        )
        .unwrap();
        assert!(empty.shells.is_empty());
        assert_eq!(
            empty.coverage.excluded_rows(),
            8,
            "valid empty surfaces keep source coverage"
        );
    }

    fn fixture_scan() -> Arc<Scan> {
        Arc::new(
            wxdata::level2::decode_volume(
                include_bytes!("../../wxdata/tests/data/corpus/mayfield-2021-first-records.ar2")
                    .to_vec(),
            )
            .unwrap(),
        )
    }

    fn source(scan: &Arc<Scan>, policy: TemporalPolicy) -> SourceKey {
        SourceKey::new(Some("KPAH".into()), "same-frame".into(), 0, scan, policy)
    }

    fn smooth_key(scan: &Arc<Scan>, policy: TemporalPolicy) -> SmoothKey {
        SmoothKey {
            roi: None,
            source: source(scan, policy),
            moment: Moment::Reflectivity,
            full_range: true,
            loop_quality: false,
            storm_uv: None,
            product: None,
            palette: 0,
            high_contrast: false,
            max_dim: 32,
        }
    }

    #[test]
    fn cached_map_payloads_keep_their_original_receipt_and_charge_its_capacity() {
        let scan = fixture_scan();
        let weak = Arc::downgrade(&scan);
        let (_, receipt) = crate::live_scan::acquisition_fixture("KPAH");
        let mut view = crate::view::MapView::new(
            Some("KPAH".into()),
            crate::render::mercator::Camera::at_lonlat(-88.0, 37.0, 8.0),
        );
        view.volume = Some(crate::view::Volume::from_live_captured(
            scan.clone(),
            "same-frame".into(),
            chrono::DateTime::UNIX_EPOCH,
            Some(receipt.clone()),
        ));
        let accepted = SourceKey::for_view(&view, TemporalPolicy::Continuous).unwrap();
        let prefetch = source(&scan, TemporalPolicy::Continuous);
        assert_ne!(
            accepted, prefetch,
            "prefetch without evidence cannot satisfy a raw accepted source"
        );
        let mut key = smooth_key(&scan, TemporalPolicy::Continuous);
        key.source = accepted.clone();
        let frame = build_smooth_covered(
            Sweeps::Binned {
                sweeps: mixed_inputs(Moment::Reflectivity),
                mask: None,
            },
            &smooth_spec(Moment::Reflectivity),
            TemporalPolicy::Continuous,
        )
        .unwrap();
        let smooth_charge = smooth_bytes(&frame.upload)
            + coverage_bytes(&frame.coverage)
            + std::mem::size_of::<SmoothFrame>()
            + receipt.estimated_bytes();
        let iso = IsoKey {
            source: accepted.clone(),
            moment: Moment::Reflectivity,
            value: 100.0f32.to_bits(),
            smooth: true,
            step: None,
            storm_uv: None,
            max_dim: 32,
        };
        let iso_frame = IsoFrame {
            shells: Vec::new(),
            coverage: frame.coverage.clone(),
        };
        let iso_charge = coverage_bytes(&iso_frame.coverage)
            + std::mem::size_of::<IsoFrame>()
            + receipt.estimated_bytes();
        let mut jobs = Loop3dJobs::default();
        jobs.tx
            .send((JobKey::Smooth(0, key.clone()), Built::Smooth(Some(frame))))
            .unwrap();
        jobs.tx
            .send((JobKey::Iso(0, iso.clone()), Built::Iso(Some(iso_frame))))
            .unwrap();
        // Delivery occurs after the source advances; it must retain the worker's original receipt.
        view.volume.as_mut().unwrap().apply_live(
            scan.clone(),
            "same-frame".into(),
            chrono::DateTime::UNIX_EPOCH,
            &[0.5],
        );
        let current = SourceKey::for_view(&view, TemporalPolicy::Continuous).unwrap();
        assert_ne!(accepted, current);
        assert_eq!(current.acquisition(), None);
        let mut caches = [Loop3dCache::default()];
        jobs.drain(&mut caches);
        assert!(caches[0].smooth.contains(&key));
        assert!(caches[0].iso.contains(&iso));
        assert_eq!(caches[0].smooth.bytes(), smooth_charge);
        assert_eq!(caches[0].iso.bytes(), iso_charge);
        let mut new_key = key.clone();
        new_key.source = current;
        assert!(!caches[0].smooth.contains(&new_key));
        drop(view);
        drop(scan);
        assert!(weak.upgrade().is_none());
        assert_eq!(key.source.acquisition(), Some(&receipt));
        assert_eq!(iso.source.acquisition(), Some(&receipt));
    }

    #[test]
    fn source_identity_distinguishes_independent_decodes_and_live_arrivals_without_retaining_scans()
    {
        let scan = fixture_scan();
        let key = source(&scan, TemporalPolicy::Continuous);
        assert_eq!(Arc::strong_count(&scan), 1);
        assert_eq!(key, source(&scan, TemporalPolicy::Continuous));
        assert_ne!(key, source(&fixture_scan(), TemporalPolicy::Continuous));
        assert_ne!(key, source(&scan, TemporalPolicy::StrictCurrent));
        let mut view = crate::view::MapView::new(
            Some("KPAH".into()),
            crate::render::mercator::Camera::at_lonlat(-88.0, 37.0, 8.0),
        );
        view.volume = Some(crate::view::Volume::from_live(
            Arc::clone(&scan),
            "same-frame".into(),
            chrono::DateTime::UNIX_EPOCH,
        ));
        let before = SourceKey::for_view(&view, TemporalPolicy::Continuous).unwrap();
        view.live_scan_revision += 1;
        assert_eq!(
            before,
            SourceKey::for_view(&view, TemporalPolicy::Continuous).unwrap(),
            "pane counter is not accepted source revision"
        );
        let volume = view.volume.as_mut().unwrap();
        volume.apply_live(Arc::clone(&scan), "same-frame".into(), volume.time, &[0.5]);
        assert_ne!(
            before,
            SourceKey::for_view(&view, TemporalPolicy::Continuous).unwrap()
        );
        view.site = Some("KTLX".into());
        assert_ne!(
            key,
            SourceKey::for_view(&view, TemporalPolicy::Continuous).unwrap()
        );
        let weak = Arc::downgrade(&scan);
        drop(view);
        drop(scan);
        assert!(
            weak.upgrade().is_none(),
            "cache keys must not keep radar buffers alive"
        );
    }

    #[test]
    fn current_and_prefetch_jobs_share_admission_and_late_results_keep_their_original_context() {
        let scan = fixture_scan();
        let old = smooth_key(&scan, TemporalPolicy::Continuous);
        let current = smooth_key(&scan, TemporalPolicy::StrictCurrent);
        let mut jobs = Loop3dJobs::default();
        for pane_job in 0..MAX_BUILDS_PER_PANE {
            let mut k = old.clone();
            k.source.revision = pane_job as u64;
            jobs.inflight.insert(JobKey::Smooth(0, k), Instant::now());
        }
        assert!(!jobs.wants(&JobKey::Smooth(0, current.clone())));
        let iso = IsoKey {
            source: current.source.clone(),
            moment: Moment::Reflectivity,
            value: 30.0f32.to_bits(),
            smooth: false,
            step: None,
            storm_uv: None,
            max_dim: 32,
        };
        assert!(!jobs.wants(&JobKey::Iso(0, iso.clone())));
        assert!(jobs.wants(&JobKey::Iso(1, iso)));
        let frame = build_smooth_covered(
            Sweeps::Binned {
                sweeps: mixed_inputs(Moment::Reflectivity),
                mask: None,
            },
            &smooth_spec(Moment::Reflectivity),
            TemporalPolicy::Continuous,
        )
        .unwrap();
        jobs.tx
            .send((JobKey::Smooth(0, old.clone()), Built::Smooth(Some(frame))))
            .unwrap();
        let mut caches = [Loop3dCache::default()];
        jobs.drain(&mut caches);
        assert!(caches[0].smooth.contains(&old));
        assert!(
            !caches[0].smooth.contains(&current),
            "late success cannot satisfy a new policy"
        );
        let old_job = JobKey::Smooth(0, old);
        jobs.tx
            .send((old_job.clone(), Built::Smooth(None)))
            .unwrap();
        jobs.drain(&mut caches);
        assert!(jobs.came_up_empty(&old_job));
        let new_job = JobKey::Smooth(0, current);
        assert!(
            !jobs.came_up_empty(&new_job),
            "late failure cannot poison new selection"
        );
        jobs.retry(&old_job);
        assert!(!jobs.came_up_empty(&old_job));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn direct_start_cannot_bypass_the_shared_worker_limit() {
        let scan = fixture_scan();
        let mut jobs = Loop3dJobs::default();
        for revision in 0..MAX_BUILDS_PER_PANE {
            let mut key = smooth_key(&scan, TemporalPolicy::Continuous);
            key.source.revision = revision as u64;
            jobs.inflight.insert(JobKey::Smooth(0, key), Instant::now());
        }
        let runtime = tokio::runtime::Runtime::new().unwrap();
        jobs.start(
            JobKey::Smooth(0, smooth_key(&scan, TemporalPolicy::StrictCurrent)),
            &crate::rt::Spawner::new(runtime.handle().clone()),
            &egui::Context::default(),
            || Built::Smooth(None),
        );
        assert_eq!(
            jobs.running(0),
            MAX_BUILDS_PER_PANE,
            "direct starts must obey admission too"
        );
    }

    #[test]
    fn cache_rejects_policy_mismatched_delivery_and_accounts_for_allocated_payloads() {
        let scan = fixture_scan();
        let key = smooth_key(&scan, TemporalPolicy::StrictCurrent);
        let frame = build_smooth_covered(
            Sweeps::Binned {
                sweeps: mixed_inputs(Moment::Reflectivity),
                mask: None,
            },
            &smooth_spec(Moment::Reflectivity),
            TemporalPolicy::Continuous,
        )
        .unwrap();
        assert!(smooth_bytes(&frame.upload) > frame.upload.data.len());
        assert!(
            coverage_bytes(&frame.coverage) >= 2 * std::mem::size_of::<temporal::SweepCoverage>()
        );
        let mut jobs = Loop3dJobs::default();
        let mut caches = [Loop3dCache::default()];
        jobs.tx
            .send((JobKey::Smooth(0, key.clone()), Built::Smooth(Some(frame))))
            .unwrap();
        jobs.drain(&mut caches);
        assert!(!caches[0].smooth.contains(&key));
        assert!(jobs.came_up_empty(&JobKey::Smooth(0, key)));
    }

    #[test]
    fn product_input_moments_and_geometry_formulas_obey_policy_before_quantization() {
        use nexrad_model::data::{MomentData, Radial, RadialStatus, Sweep};
        let base = fixture_scan();
        let radials = (0..8)
            .map(|i| {
                let time = if i < 4 {
                    1_700_000_120_000
                } else {
                    1_700_000_000_000
                };
                let data =
                    || MomentData::from_fixed_point(8, 5000, 5000, 8, 2.0, 66.0, vec![180; 8]);
                Radial::new(
                    time,
                    1,
                    i as f32 * 45.0,
                    45.0,
                    RadialStatus::IntermediateRadialData,
                    1,
                    0.5,
                    Some(data()),
                    None,
                    None,
                    Some(data()),
                    None,
                    None,
                    None,
                )
            })
            .collect();
        let scan = Arc::new(Scan::with_site(
            base.site().unwrap().clone(),
            base.coverage_pattern().clone(),
            vec![Sweep::new(1, radials)],
        ));
        for expression in ["REF + ZDR", "RANGE_KM"] {
            let product = ProductSpec {
                expr: wxdata::udp::parse(expression).unwrap(),
                range: None,
                env: wxdata::udp_volume::Env::default(),
                table: None,
            };
            let (continuous, coverage) = product_sweeps(
                Sweeps::Scan(Arc::clone(&scan)),
                &product,
                TemporalPolicy::Continuous,
                None,
            )
            .unwrap();
            let (strict, current) = product_sweeps(
                Sweeps::Scan(Arc::clone(&scan)),
                &product,
                TemporalPolicy::StrictCurrent,
                None,
            )
            .unwrap();
            assert!(coverage.retained_older_rows() > 0);
            assert!(current.excluded_rows() > 0);
            assert_eq!(
                current.contributors.len(),
                if expression == "REF + ZDR" { 2 } else { 1 }
            );
            let sweep = &strict[0];
            let source =
                wxdata::level2::bin_scan_opts(&scan, Moment::Reflectivity, 0, false).unwrap();
            let mut removed = 0;
            let mut kept = 0;
            // Wide radials wrap and reach into preceding angular bins. Assert against their
            // recorded clocks, rather than assuming a geometric half-plane boundary.
            for (row, time) in source.bin_time_ms.iter().enumerate() {
                let gates = &sweep.data[row * sweep.gate_count..(row + 1) * sweep.gate_count];
                if *time == 1_700_000_000_000 {
                    assert!(
                        gates.iter().all(|v| *v == 0),
                        "{expression} must not refill excluded rows"
                    );
                    assert!(continuous[0].data
                        [row * sweep.gate_count..(row + 1) * sweep.gate_count]
                        .iter()
                        .any(|v| *v >= 2));
                    removed += 1;
                } else if *time == 1_700_000_120_000 && gates.iter().any(|v| *v >= 2) {
                    kept += 1;
                }
            }
            assert!(removed > 100 && kept > 100);
        }
    }

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
    #[test]
    fn captured_native_inputs_reach_smooth_iso_and_formula_receipts_without_becoming_output_lineage(
    ) {
        let scan = fixture_scan();
        let mut tracker = wxdata::live_pass::PassTracker::default();
        tracker.observe(&scan, false);
        let passes = Arc::new(tracker.attribution_for_scan(&scan));
        let (mut receiver, _) = crate::live_scan::acquisition_fixture("KPAH");
        let receipt = receiver
            .capture_acquisition(
                wxdata::live::RadialCoverage {
                    progress: receiver.progress.unwrap(),
                    source_passes: Some(tracker.inventory()),
                    source_sequences: None,
                    source_attribution: Some(passes.clone()),
                    source_scope: None,
                    radials: vec![],
                },
                chrono::Utc::now(),
            )
            .unwrap();
        let smooth = build_smooth_covered(
            Sweeps::captured(scan.clone(), Some(&receipt)),
            &smooth_spec(Moment::Reflectivity),
            TemporalPolicy::Continuous,
        )
        .unwrap();
        let iso = build_iso_covered(
            Sweeps::captured(scan.clone(), Some(&receipt)),
            &IsoSpec {
                moment: Moment::Reflectivity,
                value: 30.0,
                step: None,
                smooth: false,
                max_dim: 32,
                top_km: 6.0,
                storm_uv: None,
            },
            TemporalPolicy::Continuous,
        )
        .unwrap();
        assert_eq!(smooth.coverage, iso.coverage);
        assert!(smooth.coverage.contributors.iter().any(|c| c
            .native_passes
            .as_ref()
            .is_some_and(|p| !p.passes.is_empty())));
        let legacy = build_smooth_covered(
            Sweeps::Scan(scan.clone()),
            &smooth_spec(Moment::Reflectivity),
            TemporalPolicy::Continuous,
        )
        .unwrap();
        assert_eq!(smooth.upload.data, legacy.upload.data);
        assert!(legacy
            .coverage
            .contributors
            .iter()
            .all(|c| c.native_passes.is_none()));
        assert!(coverage_bytes(&smooth.coverage) > coverage_bytes(&legacy.coverage));
        let product = ProductSpec {
            expr: wxdata::udp::parse("REF + 1").unwrap(),
            range: None,
            env: wxdata::udp_volume::Env::default(),
            table: None,
        };
        let (generated, inputs) = product_sweeps(
            Sweeps::captured(scan, Some(&receipt)),
            &product,
            TemporalPolicy::Continuous,
            Some(&passes),
        )
        .unwrap();
        assert!(inputs.contributors.iter().any(|c| c
            .native_passes
            .as_ref()
            .is_some_and(|p| !p.passes.is_empty())));
        assert!(
            generated.iter().all(|s| s.source_radials.is_none()),
            "formulas retain input coverage, never a sole output writer"
        );
    }
}
