//! 3D reflectivity raymarch: a fullscreen-triangle MIP raymarch of a volume texture, rendered
//! into an egui rect via an `egui_wgpu` paint callback (mirrors the map callback pattern).

use glam::{Mat4, Vec3};

/// Raymarch uniform block (matches `shaders/raymarch.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    inv_view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    box_min: [f32; 4],
    box_max: [f32; 4],
    dims: [f32; 4], // nx, ny, nz, step_count
    ctl: [f32; 4],  // floor index, opacity, cell size, ceiling index (0 = none)
    clip_min: [f32; 4],
    clip_max: [f32; 4],
    /// See `shaders/raymarch.wgsl`'s own field of the same name: xy a world-space unit normal, z
    /// the signed distance along it, w whether the plane is active at all.
    plane: [f32; 4],
    /// Half-width of an optional slab straddling `plane`, in the same world units as
    /// `box_min`/`box_max` (Phase H4's "slab thickness") — `x = 0.0` keeps `plane`'s old
    /// half-space clip (cut away everything on the far side); `x > 0.0` keeps only a band of that
    /// half-width centered on the plane instead. y/z/w spare.
    plane_slab: [f32; 4],
    /// CC-anomaly opacity ramp: `[clear_idx, full_idx, faintest, enabled]`. See
    /// [`cc_anomaly_uniform`]; all-zero means off, which is what every non-CC volume passes.
    cc: [f32; 4],
    /// Horizontal CAPPI-altitude reference plane (Phase H4's last open item): `[z, half_width,
    /// spare, enabled]` in the same world units as `box_min`/`box_max`. See
    /// [`cappi_marker_uniform`]; all-zero means off, same convention as `plane`/`cc`.
    cappi_marker: [f32; 4],
    /// Opacity transfer function (Phase H2's curve): the first four stops' volume indices,
    /// ascending, and their opacities 0..1. `tf_a[0] < 0` means off. See [`tf_uniform`].
    tf_x: [f32; 4],
    tf_a: [f32; 4],
    /// Stops five to eight, and `[count, 0, 0, 0]` (M3.5 increment 2).
    tf_x2: [f32; 4],
    tf_a2: [f32; 4],
    tf_n: [f32; 4],
    /// Rendering mode: `[mode, km per world unit horizontally, km per world unit vertically,
    /// lit]`. See [`render_uniform`].
    render: [f32; 4],
}

/// How the raymarch turns a ray's samples into a pixel (ROADMAP_PARITY M3.5). A display choice
/// only: it never changes which voxels exist, their values, or anything sampled or exported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VolumeRender {
    /// Maximum-intensity projection: each pixel is the strongest kept value along its ray. The
    /// original mode, unchanged.
    #[default]
    Mip,
    /// Front-to-back alpha compositing: every kept sample adds its colour, weighted by its
    /// opacity per kilometre of path, so what lies in front partly hides what lies behind.
    Translucent,
    /// Translucent, shaded by the echo's own gradient so a core's shape reads.
    TranslucentLit,
}

impl VolumeRender {
    pub const ALL: [Self; 3] = [Self::Mip, Self::Translucent, Self::TranslucentLit];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mip => "Maximum",
            Self::Translucent => "Translucent",
            Self::TranslucentLit => "Lit",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::Mip => "Each pixel shows the strongest value along its line of sight (MIP)",
            Self::Translucent => {
                "Values blend front to back; the opacity is per kilometre of path, so the \
                 Quality setting does not change how solid the storm looks"
            }
            Self::TranslucentLit => {
                "Translucent, shaded by the echo's own gradient: display shading only, the \
                 values are unchanged"
            }
        }
    }

    pub fn translucent(self) -> bool {
        self != Self::Mip
    }
}

/// The `render` uniform: the mode, and the km one world unit spans horizontally and vertically
/// in this box (`half_km` across half its width, `top_km` up its height), so the shader measures
/// each step in real kilometres whatever the vertical exaggeration.
fn render_uniform(
    render: VolumeRender,
    box_min: Vec3,
    box_max: Vec3,
    half_km: f32,
    top_km: f32,
) -> [f32; 4] {
    let half_w = ((box_max.x - box_min.x) * 0.5).abs().max(1e-9);
    let height = (box_max.z - box_min.z).abs().max(1e-9);
    [
        if render.translucent() { 1.0 } else { 0.0 },
        half_km.max(0.0) / half_w,
        top_km.max(0.0) / height,
        if render == VolumeRender::TranslucentLit {
            1.0
        } else {
            0.0
        },
    ]
}

/// One step's opacity through a medium of opacity `a_km` per kilometre: `1 - (1 - a)^dt_km`.
/// The CPU mirror of `raymarch.wgsl`'s `step_alpha`.
pub fn step_alpha(a_km: f32, dt_km: f32) -> f32 {
    let clear = (1.0 - a_km).clamp(1e-4, 1.0);
    1.0 - clear.powf(dt_km)
}

/// Front-to-back composite of `(rgb, opacity per km)` samples spaced `dt_km` apart, with the
/// shader's early termination: premultiplied `(rgb, alpha)`. The CPU reference the translucent
/// mode's opacity tests run against.
pub fn composite_ray(samples: &[([f32; 3], f32)], dt_km: f32) -> ([f32; 3], f32) {
    let (mut rgb, mut a) = ([0.0f32; 3], 0.0f32);
    for (c, a_km) in samples {
        let s = step_alpha(*a_km, dt_km);
        for k in 0..3 {
            rgb[k] += (1.0 - a) * s * c[k];
        }
        a += (1.0 - a) * s;
        if a > 0.995 {
            break;
        }
    }
    (rgb, a)
}

/// Most stops an opacity curve can have.
pub const MAX_STOPS: usize = 8;

/// An opacity curve (Phase H2, M3.5 increment 2): two to eight `[value, opacity]` stops in a
/// product's own units (or, once mapped, in volume index space), ascending in value, opacity
/// 0..1, piecewise linear between them and flat past the ends. `Copy`, so a view stays cheap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TfStops {
    n: u8,
    pts: [[f32; 2]; MAX_STOPS],
}

impl TfStops {
    /// From any points: sorted by value, opacities clamped, at most [`MAX_STOPS`] kept (the
    /// first ones by value). `None` with fewer than two.
    pub fn new(points: &[[f32; 2]]) -> Option<Self> {
        let mut v: Vec<[f32; 2]> = points
            .iter()
            .filter(|p| p[0].is_finite() && p[1].is_finite())
            .map(|p| [p[0], p[1].clamp(0.0, 1.0)])
            .collect();
        v.sort_by(|a, b| a[0].total_cmp(&b[0]));
        v.truncate(MAX_STOPS);
        if v.len() < 2 {
            return None;
        }
        let mut pts = [[0.0; 2]; MAX_STOPS];
        pts[..v.len()].copy_from_slice(&v);
        Some(Self {
            n: v.len() as u8,
            pts,
        })
    }

    pub fn points(&self) -> &[[f32; 2]] {
        &self.pts[..self.n as usize]
    }

    pub fn points_mut(&mut self) -> &mut [[f32; 2]] {
        &mut self.pts[..self.n as usize]
    }

    pub fn len(&self) -> usize {
        self.n as usize
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The opacity at `v`: what the shader computes.
    pub fn alpha(&self, v: f32) -> f32 {
        let p = self.points();
        if v <= p[0][0] {
            return p[0][1];
        }
        for w in p.windows(2) {
            if v <= w[1][0] {
                let t = (v - w[0][0]) / (w[1][0] - w[0][0]).max(0.001);
                return w[0][1] + (w[1][1] - w[0][1]) * t;
            }
        }
        p[p.len() - 1][1]
    }

    /// A stop at `v` on the current curve (so adding one changes nothing until it is moved).
    /// `false` when the curve is full.
    pub fn insert(&mut self, v: f32) -> bool {
        if self.len() >= MAX_STOPS {
            return false;
        }
        let a = self.alpha(v);
        let mut pts = self.points().to_vec();
        pts.push([v, a]);
        *self = Self::new(&pts).expect("at least two");
        true
    }

    /// Remove stop `i`; `false` when only two are left.
    pub fn remove(&mut self, i: usize) -> bool {
        if self.len() <= 2 || i >= self.len() {
            return false;
        }
        let mut pts = self.points().to_vec();
        pts.remove(i);
        *self = Self::new(&pts).expect("at least two");
        true
    }

    /// The same curve with every value mapped (into a volume's index space, say).
    pub fn map_values(&self, f: impl Fn(f32) -> f32) -> Self {
        let pts: Vec<[f32; 2]> = self.points().iter().map(|p| [f(p[0]), p[1]]).collect();
        Self::new(&pts).unwrap_or(*self)
    }

    /// The legacy four-point form: these stops when there are four, otherwise four samples of
    /// the curve at its ends and thirds — what a build that only reads four points shows.
    pub fn as_four(&self) -> [[f32; 2]; 4] {
        let p = self.points();
        if p.len() == 4 {
            return [p[0], p[1], p[2], p[3]];
        }
        let (lo, hi) = (p[0][0], p[p.len() - 1][0]);
        let at = |t: f32| {
            let v = lo + (hi - lo) * t;
            [v, self.alpha(v)]
        };
        [at(0.0), at(1.0 / 3.0), at(2.0 / 3.0), at(1.0)]
    }
}

impl From<[[f32; 2]; 4]> for TfStops {
    fn from(p: [[f32; 2]; 4]) -> Self {
        Self::new(&p).expect("four points")
    }
}

/// Colour stops (M3.5, 1008.md C2): two to [`MAX_STOPS`] `(value, rgb)` pairs in a product's own
/// units, ascending, beside the opacity stops, replacing the palette's colours in a 3D volume.
/// Colours interpolate between stops and hold past the ends. Only the volume's colour table is
/// rebuilt from them ([`Self::lut`]): the sampled values, the probe and every export are unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorStops {
    pts: Vec<(f32, [u8; 3])>,
}

impl ColorStops {
    /// Sorted by value, non-finite values dropped, at most [`MAX_STOPS`]; `None` with fewer
    /// than two.
    pub fn new(points: &[(f32, [u8; 3])]) -> Option<Self> {
        let mut pts: Vec<(f32, [u8; 3])> =
            points.iter().copied().filter(|p| p.0.is_finite()).collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        pts.truncate(MAX_STOPS);
        (pts.len() >= 2).then_some(Self { pts })
    }

    /// Cool-to-warm across `(lo, hi)`: where an edit starts.
    pub fn default_for((lo, hi): (f32, f32)) -> Self {
        let at = |t: f32| lo + (hi - lo) * t;
        Self::new(&[
            (at(0.0), [40, 60, 170]),
            (at(0.5), [235, 225, 60]),
            (at(1.0), [215, 40, 40]),
        ])
        .expect("three stops")
    }

    pub fn points(&self) -> &[(f32, [u8; 3])] {
        &self.pts
    }

    pub fn points_mut(&mut self) -> &mut [(f32, [u8; 3])] {
        &mut self.pts
    }

    pub fn len(&self) -> usize {
        self.pts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    /// The colour at `v`.
    pub fn color(&self, v: f32) -> [u8; 3] {
        let p = &self.pts;
        if v <= p[0].0 {
            return p[0].1;
        }
        for w in p.windows(2) {
            if v <= w[1].0 {
                let t = ((v - w[0].0) / (w[1].0 - w[0].0).max(f32::EPSILON)).clamp(0.0, 1.0);
                let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
                return [
                    mix(w[0].1[0], w[1].1[0]),
                    mix(w[0].1[1], w[1].1[1]),
                    mix(w[0].1[2], w[1].1[2]),
                ];
            }
        }
        p[p.len() - 1].1
    }

    /// A stop at `v` with the colour already there; `false` when full.
    pub fn insert(&mut self, v: f32) -> bool {
        if self.len() >= MAX_STOPS {
            return false;
        }
        let c = self.color(v);
        let mut pts = self.pts.clone();
        pts.push((v, c));
        *self = Self::new(&pts).expect("at least two");
        true
    }

    /// Remove stop `i`; `false` when only two are left.
    pub fn remove(&mut self, i: usize) -> bool {
        if self.len() <= 2 || i >= self.len() {
            return false;
        }
        self.pts.remove(i);
        true
    }

    /// `base` (a volume's palette table, 256 RGBA entries over `range`) with every data entry's
    /// colour taken from these stops and its alpha kept, so what the palette left transparent
    /// stays transparent. Index 0 (empty) and 1 (range-folded) are unchanged.
    pub fn lut(&self, base: &[u8], (lo, hi): (f32, f32)) -> Vec<u8> {
        let mut out = base.to_vec();
        if out.len() < 1024 {
            return out;
        }
        let span = (hi - lo).max(f32::EPSILON);
        for raw in 2usize..=255 {
            let v = lo + (raw as f32 - 2.0) / 253.0 * span;
            let c = self.color(v);
            out[raw * 4..raw * 4 + 3].copy_from_slice(&c);
        }
        out
    }

    /// For saving: `[value, r, g, b]`.
    pub fn to_saved(&self) -> Vec<[f32; 4]> {
        self.pts
            .iter()
            .map(|(v, c)| [*v, c[0] as f32, c[1] as f32, c[2] as f32])
            .collect()
    }

    pub fn from_saved(saved: &[[f32; 4]]) -> Option<Self> {
        let byte = |x: f32| x.round().clamp(0.0, 255.0) as u8;
        let pts: Vec<(f32, [u8; 3])> = saved
            .iter()
            .map(|s| (s[0], [byte(s[1]), byte(s[2]), byte(s[3])]))
            .collect();
        Self::new(&pts)
    }
}

/// The transfer-function uniforms for `tf`: values and opacities of stops one to four and five
/// to eight, and the count; `None` gives the "off" form (`tf_a[0] < 0`).
pub fn tf_uniform(tf: Option<TfStops>) -> [[f32; 4]; 5] {
    let Some(tf) = tf else {
        return [[0.0; 4], [-1.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]];
    };
    let p = tf.points();
    let last = p[p.len() - 1];
    // Unused slots repeat the last stop, so the curve stays flat past it whatever the count.
    let get = |i: usize| p.get(i).copied().unwrap_or(last);
    let x = |r: std::ops::Range<usize>| {
        let v: Vec<f32> = r.map(|i| get(i)[0]).collect();
        [v[0], v[1], v[2], v[3]]
    };
    let a = |r: std::ops::Range<usize>| {
        let v: Vec<f32> = r.map(|i| get(i)[1]).collect();
        [v[0], v[1], v[2], v[3]]
    };
    [
        x(0..4),
        a(0..4),
        x(4..8),
        a(4..8),
        [p.len() as f32, 0.0, 0.0, 0.0],
    ]
}

/// A new volume grid to upload: `data` is `n×n×nz` interleaved (value index, valid) byte pairs —
/// see [`pack_rg8`] — and `lut` a 256-entry RGBA table.
#[derive(Clone)]
pub struct Volume3dUpload {
    pub data: Vec<u8>,
    pub n: u32,
    pub nz: u32,
    pub lut: Vec<u8>,
    pub half_km: f32,
    /// Where the box is centred, km east and north of the radar: zero for the whole-radar
    /// volume, a storm's position for a region of interest (ROADMAP_PARITY M3.6).
    pub center_km: [f32; 2],
    pub top_km: f32,
    /// Share (0..=1) of the scan's echo that lies beyond `half_km` and is therefore not in the
    /// volume. Shown in the UI so a cropped box is never mistaken for the whole scan.
    pub outside: f32,
    /// For a user-defined product, the value range its palette was drawn over; `None` for a
    /// moment, whose range is fixed.
    pub value_range: Option<(f32, f32)>,
    /// The value range `lut` indexes linearly (index 2 at the low end, 255 at the high), so
    /// colour stops can rebuild it; `None` where it does not (velocity, indexed by speed; the
    /// inverted CC table).
    pub lut_range: Option<(f32, f32)>,
}

impl Volume3dUpload {
    /// Horizontal cell size, km.
    pub fn cell_km(&self) -> f32 {
        2.0 * self.half_km / (self.n.max(2) - 1) as f32
    }
}

/// Expand a plain index grid (`0` empty, `2..=255` value) into the `Rg8Unorm` layout the raymarch
/// samples: R keeps the index, G is 255 where a real value exists. Filtering both together is what
/// lets the shader interpolate only across real voxels; see `shaders/raymarch.wgsl`.
pub fn pack_rg8(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2);
    for &v in data {
        out.push(v);
        out.push(if v >= 2 { 255 } else { 0 });
    }
    out
}

fn volume_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("volume3d_sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

/// Box extents of the rendered volume (z exaggerated for legibility).
const BOX_MIN: Vec3 = Vec3::new(-1.0, -1.0, 0.0);
const BOX_MAX: Vec3 = Vec3::new(1.0, 1.0, 0.5);

/// An additional vertical clip plane at any bearing (Phase H4) — the axis-aligned `clip` slab can
/// only ever cut along the box's own east-west/north-south faces, so cutting into a storm at the
/// angle it actually leans or approaches from needs a plane that isn't locked to those axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VerticalPlane {
    /// Which way the plane's normal points, degrees clockwise from north (the volume is kept on
    /// the side the normal points toward) — the same bearing convention as everywhere else in
    /// this app (storm motion, azimuth).
    pub bearing_deg: f32,
    /// Signed offset of the plane from the box center along its normal, as a fraction of the
    /// box's half-width (`-1.0..=1.0`) — matches `View3d::clip`'s own fraction-of-box convention,
    /// so it reads the same regardless of which box (the orbit window's fixed one, or the
    /// main map's dynamically-sized one) it is applied to.
    pub offset: f32,
    /// Half-width of an optional slab straddling the plane, as a fraction of the box's half-width
    /// (same convention as `offset`) — `None` keeps the plane's old behavior: clip away
    /// everything on the far side. `Some(t)` keeps only a band of half-width `t` centered on the
    /// plane instead ("slab thickness": two parallel planes with a gap, rather than one plane
    /// cutting the volume in half).
    pub thickness: Option<f32>,
}

/// What the viewer is looking at, beyond the camera: the reflectivity floor and the slab the
/// raymarch is confined to. Defaults draw the whole volume, which is the old behaviour.
#[derive(Clone, Copy, Debug)]
pub struct View3d {
    /// Minimum volume index (2..=255) a voxel must reach to be drawn. 2 = everything.
    pub threshold_idx: f32,
    /// Maximum volume index a voxel may have and still be drawn (Phase H2's value window): with
    /// the floor, isolates a band such as 45-55 dBZ. 0 = no ceiling.
    pub ceiling_idx: f32,
    /// Slab bounds as fractions of the box, `[x0, x1, y0, y1, z0, z1]`.
    pub clip: [f32; 6],
    /// `None` disables the plane clip entirely (the common case).
    pub plane: Option<VerticalPlane>,
    /// CC-anomaly ramp from [`cc_anomaly_uniform`], or all-zero for the volumes that aren't CC.
    pub cc: [f32; 4],
    /// Altitude (km, same "beam height above the radar" convention as `wxdata::volume3d::build`'s
    /// z-grid and `wxdata::volume3d::cappi`'s `alt_km`) of a horizontal reference plane to draw
    /// inside the 3D view — Phase H4's last open item: the CAPPI window already slices the volume
    /// at this height as its own separate 2D tool, but nothing showed *where* that height sits
    /// relative to the storm until now. `None` disables it (the common case).
    pub cappi_km: Option<f32>,
    /// Opacity transfer function: four `[volume index, opacity]` points, opacity drawn piecewise
    /// linear between them (and flat past the ends), replacing the fixed ramp. `None` keeps the
    /// ramp.
    pub tf: Option<TfStops>,
    /// MIP or translucent compositing (M3.5). With translucent rendering the opacity from the
    /// ramp, CC ramp or `tf` is read as opacity per kilometre of path.
    pub render: VolumeRender,
}

impl Default for View3d {
    fn default() -> Self {
        Self {
            threshold_idx: 2.0,
            ceiling_idx: 0.0,
            clip: [0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            plane: None,
            cc: [0.0; 4],
            cappi_km: None,
            tf: None,
            render: VolumeRender::Mip,
        }
    }
}

/// Half of the box's larger horizontal span — the same "fraction of the box" scale both `offset`
/// and `thickness` are expressed in, shared so the two agree on what "1.0" means.
fn half_extent(box_min: Vec3, box_max: Vec3) -> f32 {
    ((box_max.x - box_min.x).abs()).max((box_max.y - box_min.y).abs()) * 0.5
}

/// The `plane` uniform field for a box spanning `box_min..box_max`: world-space unit normal, the
/// signed distance along it, and whether it's active — `[0,0,0,0]` (inert; `pos.x*0+pos.y*0 < 0`
/// is never true) when `plane` is `None`, so callers don't need their own separate enable check.
fn plane_uniform(plane: Option<VerticalPlane>, box_min: Vec3, box_max: Vec3) -> [f32; 4] {
    let Some(p) = plane else {
        return [0.0, 0.0, 0.0, 0.0];
    };
    let theta = p.bearing_deg.to_radians();
    // Compass bearing (0 = north, 90 = east) onto the box's own x = east, y = north axes.
    let (nx, ny) = (theta.sin(), theta.cos());
    let center = (box_min + box_max) * 0.5;
    let d = nx * center.x + ny * center.y + p.offset * half_extent(box_min, box_max);
    [nx, ny, d, 1.0]
}

/// The `plane_slab` uniform field: `x` is the optional slab's half-width in world units, `0.0`
/// when there is no plane at all or its `thickness` is `None` (the shader then falls back to
/// `plane`'s old half-space clip). Scaled by the same `half_extent` as `plane`'s own `offset`, so
/// a `thickness` of `0.1` means the same real width regardless of which box it applies to.
fn plane_slab_uniform(plane: Option<VerticalPlane>, box_min: Vec3, box_max: Vec3) -> [f32; 4] {
    let half_thickness = plane
        .and_then(|p| p.thickness)
        .map(|t| t * half_extent(box_min, box_max))
        .unwrap_or(0.0);
    [half_thickness, 0.0, 0.0, 0.0]
}

/// The `cappi_marker` uniform field for a box spanning `box_min..box_max`: world-space z of the
/// reference plane, its half-width band, and whether it's active. `cappi_km` is in the same
/// "beam height above the radar" unit as `top_km` (both from `wxdata::volume3d`), so the box's own
/// z=0..z=(box_max.z-box_min.z) span is treated as 0..`top_km` km. Inert (`[0,0,0,0]`) when
/// `cappi_km` is `None`, `top_km` isn't positive, or the altitude falls outside the box — pinning
/// an out-of-range altitude to the nearest edge would show a plane at the wrong height, which is
/// worse than not showing one.
fn cappi_marker_uniform(
    cappi_km: Option<f32>,
    top_km: f32,
    box_min: Vec3,
    box_max: Vec3,
) -> [f32; 4] {
    let Some(km) = cappi_km else {
        return [0.0, 0.0, 0.0, 0.0];
    };
    if top_km <= 0.0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let frac = km / top_km;
    if !(0.0..=1.0).contains(&frac) {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let z = box_min.z + frac * (box_max.z - box_min.z);
    // A thin band rather than an infinitely-thin plane, so it survives float precision and reads
    // as a visible line rather than flickering in and out as the camera moves.
    let half_width = ((box_max.z - box_min.z) * 0.006).max(1e-4);
    [z, half_width, 0.0, 1.0]
}

/// Ground-track endpoints of `plane`'s line on the map (Phase H4's "cross-section line visible in
/// map pane"), for drawing it in the 2D pane the same way the cross-section tool draws its own
/// two-point line — ties the 3D plane clip back to geographic context instead of leaving it
/// visible only from inside the 3D view. `radar` is `[lon, lat]`; `half_km` is the same box
/// half-extent `pane_smooth_volume`'s raymarch box uses (the main map's box is centered on the
/// radar site, unlike the standalone orbit window's fixed abstract one), so the returned line
/// lands at the same real-world offset the raymarch actually cuts at.
///
/// Mirrors [`plane_uniform`]'s geometry: the plane's own line runs perpendicular to its normal
/// (`bearing_deg`), offset from the site by `offset * half_km` along the normal. `half_km * sqrt(2)`
/// on each side of that foot point covers the box's diagonal regardless of where the offset put it.
pub fn plane_ground_track(
    plane: VerticalPlane,
    radar: [f64; 2],
    half_km: f32,
) -> ([f64; 2], [f64; 2]) {
    let foot = crate::geo::destination_point(
        radar,
        plane.bearing_deg as f64,
        (plane.offset * half_km) as f64,
    );
    let half_len = half_km as f64 * std::f64::consts::SQRT_2;
    let a = crate::geo::destination_point(foot, plane.bearing_deg as f64 + 90.0, half_len);
    let b = crate::geo::destination_point(foot, plane.bearing_deg as f64 - 90.0, half_len);
    (a, b)
}

/// Orbit-camera uniforms: azimuth/elevation in degrees, `dist` from the box center, view `aspect`.
/// `half_km` and `top_km` are the volume's own half-width and vertical span
/// (`wxdata::volume3d::Volume3d`), which place `v3.cappi_km`'s reference plane at the right
/// fraction of the fixed orbit box and give translucent rendering its kilometre scale.
#[allow(clippy::too_many_arguments)]
pub fn orbit_uniform(
    az_deg: f32,
    el_deg: f32,
    dist: f32,
    aspect: f32,
    n: u32,
    nz: u32,
    half_km: f32,
    top_km: f32,
    steps: u32,
    v3: View3d,
) -> Uniforms {
    let center = (BOX_MIN + BOX_MAX) * 0.5;
    let (az, el) = (az_deg.to_radians(), el_deg.to_radians());
    let dir = Vec3::new(el.cos() * az.sin(), el.cos() * az.cos(), el.sin());
    let eye = center + dir * dist;
    let view = Mat4::look_at_rh(eye, center, Vec3::Z);
    let proj = Mat4::perspective_rh(45f32.to_radians(), aspect.max(0.1), 0.01, 100.0);
    let inv = (proj * view).inverse();
    Uniforms {
        inv_view_proj: inv.to_cols_array_2d(),
        cam_pos: [eye.x, eye.y, eye.z, 1.0],
        box_min: [BOX_MIN.x, BOX_MIN.y, BOX_MIN.z, 0.0],
        box_max: [BOX_MAX.x, BOX_MAX.y, BOX_MAX.z, 0.0],
        dims: [n as f32, n as f32, nz as f32, steps as f32],
        ctl: [v3.threshold_idx, 1.0, 0.0, v3.ceiling_idx],
        clip_min: [v3.clip[0], v3.clip[2], v3.clip[4], 0.0],
        clip_max: [v3.clip[1], v3.clip[3], v3.clip[5], 0.0],
        plane: plane_uniform(v3.plane, BOX_MIN, BOX_MAX),
        plane_slab: plane_slab_uniform(v3.plane, BOX_MIN, BOX_MAX),
        cc: v3.cc,
        cappi_marker: cappi_marker_uniform(v3.cappi_km, top_km, BOX_MIN, BOX_MAX),
        tf_x: tf_uniform(v3.tf)[0],
        tf_a: tf_uniform(v3.tf)[1],
        tf_x2: tf_uniform(v3.tf)[2],
        tf_a2: tf_uniform(v3.tf)[3],
        tf_n: tf_uniform(v3.tf)[4],
        render: render_uniform(v3.render, BOX_MIN, BOX_MAX, half_km, top_km),
    }
}

/// Main-map raymarch uniforms. The Cartesian texture stays radar-relative, while the box is
/// expressed in the same local pixel coordinates as the pitched geographic camera.
#[allow(clippy::too_many_arguments)]
pub fn map_uniform(
    camera: &crate::render::mercator::Camera,
    viewport: (f32, f32),
    radar_lon: f64,
    radar_lat: f64,
    antenna_altitude_m: f32,
    upload: &Volume3dUpload,
    steps: u32,
    view: View3d,
    vertical_exaggeration: f32,
    opacity: f32,
) -> Uniforms {
    let radar_world = crate::render::mercator::lonlat_to_world(radar_lon, radar_lat);
    let wpp = camera.world_per_pixel();
    let dx = (radar_world.0 - camera.center.0 + 0.5).rem_euclid(1.0) - 0.5;
    let dy = camera.center.1 - radar_world.1;
    let metres_to_px = crate::render::mercator::Camera::world_units_per_metre(radar_lat) / wpp;
    let half_px = upload.half_km as f64 * 1_000.0 * metres_to_px;
    // A region of interest is centred off the radar: east is +x, north is +y in this box.
    let (east_px, north_px) = (
        upload.center_km[0] as f64 * 1_000.0 * metres_to_px,
        upload.center_km[1] as f64 * 1_000.0 * metres_to_px,
    );
    let z0 = antenna_altitude_m as f64 * metres_to_px * vertical_exaggeration as f64;
    let z1 = (antenna_altitude_m as f64 + upload.top_km as f64 * 1_000.0)
        * metres_to_px
        * vertical_exaggeration as f64;
    let box_min = Vec3::new(
        (dx / wpp + east_px - half_px) as f32,
        (dy / wpp + north_px - half_px) as f32,
        z0 as f32,
    );
    let box_max = Vec3::new(
        (dx / wpp + east_px + half_px) as f32,
        (dy / wpp + north_px + half_px) as f32,
        z1 as f32,
    );
    let eye = camera.eye_position(viewport);
    Uniforms {
        inv_view_proj: camera
            .view_projection(viewport)
            .inverse()
            .to_cols_array_2d(),
        cam_pos: [eye.x, eye.y, eye.z, 1.0],
        box_min: [box_min.x, box_min.y, box_min.z, 0.0],
        box_max: [box_max.x, box_max.y, box_max.z, 0.0],
        dims: [
            upload.n as f32,
            upload.n as f32,
            upload.nz as f32,
            steps as f32,
        ],
        ctl: [
            view.threshold_idx,
            opacity.clamp(0.0, 1.0),
            // One sample per cell: the smaller of the horizontal and (exaggerated) vertical cell.
            (2.0 * half_px / upload.n.max(1) as f64)
                .min(
                    upload.top_km as f64 * 1_000.0 * metres_to_px * vertical_exaggeration as f64
                        / upload.nz.max(1) as f64,
                )
                .max(1e-6) as f32,
            view.ceiling_idx,
        ],
        clip_min: [view.clip[0], view.clip[2], view.clip[4], 0.0],
        clip_max: [view.clip[1], view.clip[3], view.clip[5], 0.0],
        plane: plane_uniform(view.plane, box_min, box_max),
        plane_slab: plane_slab_uniform(view.plane, box_min, box_max),
        cc: view.cc,
        cappi_marker: cappi_marker_uniform(view.cappi_km, upload.top_km, box_min, box_max),
        tf_x: tf_uniform(view.tf)[0],
        tf_a: tf_uniform(view.tf)[1],
        tf_x2: tf_uniform(view.tf)[2],
        tf_a2: tf_uniform(view.tf)[3],
        tf_n: tf_uniform(view.tf)[4],
        render: render_uniform(view.render, box_min, box_max, upload.half_km, upload.top_km),
    }
}

/// Convert a dBZ threshold into the volume's 2..=255 index space. `range` is the volume's
/// `(value_min, value_max)`; the mapping mirrors `volume3d::build`.
pub fn threshold_index(dbz: f32, range: (f32, f32)) -> f32 {
    let (lo, hi) = range;
    let span = (hi - lo).max(f32::EPSILON);
    (2.0 + ((dbz - lo) / span) * 253.0).clamp(2.0, 255.0)
}

/// The `cc` uniform both 3D shaders read: `[clear_idx, full_idx, faintest, enabled]`.
///
/// The ramp is expressed as two palette indices *in whatever index space the shader samples*,
/// rather than as CC values plus a direction flag. That matters because `SmoothDebris` raymarches
/// an inverted volume ([`wxdata::volume3d::invert_in_place`]), where low CC is a *high* index —
/// so its ramp runs the opposite way round from the observed path's. Handing the shader the two
/// endpoints lets one `(idx - clear) / (full - clear)` serve both: the division is signed, so the
/// direction is carried by the endpoints themselves and neither shader needs to know which volume
/// it is looking at.
pub fn cc_anomaly_uniform(
    anomaly: crate::view::CcAnomaly,
    range: (f32, f32),
    inverted: bool,
) -> [f32; 4] {
    if !anomaly.enabled {
        return [0.0, 0.0, 0.0, 0.0];
    }
    // The sliders are independent, so nothing stops the clear edge being dragged below the opaque
    // one. Ordering them here (rather than constraining the widgets against each other, which
    // makes both feel sticky) keeps the ramp pointing the right way whatever the user does.
    let opaque = anomaly.opaque_cc.min(anomaly.clear_cc);
    let clear = anomaly.clear_cc.max(opaque + crate::view::MIN_CC_SPAN);
    let flip = |i: f32| if inverted { 257.0 - i } else { i };
    [
        flip(threshold_index(clear, range)),
        flip(threshold_index(opaque, range)),
        anomaly.faintest.clamp(0.0, 1.0),
        1.0,
    ]
}

/// This tilt's beam altitude (m, antenna-relative before `antenna_altitude_m` is added) at
/// ground range `ground_km` — the CPU mirror of `radar_observed.wgsl`'s `beam_world`, which is
/// also the function [`pick_observed_tilt`] inverts. Kept in lock-step with the shader on purpose:
/// see that function's own comment for why only the earth-curvature half of the rise is
/// exaggerated and the flat-earth angle half never is.
fn tilt_altitude_m(
    ground_km: f64,
    elevation_deg: f32,
    vertical_exaggeration: f64,
    beam_rise: f64,
) -> f64 {
    let slant_km = wxdata::xsection::slant_from_ground_km(ground_km, elevation_deg as f64);
    let angle_height_m = slant_km * 1_000.0 * (elevation_deg as f64).to_radians().sin();
    let true_height_m = wxdata::xsection::beam_height_km(slant_km, elevation_deg as f64) * 1_000.0;
    let curvature_height_m = true_height_m - angle_height_m;
    (angle_height_m + curvature_height_m * vertical_exaggeration) * beam_rise
}

/// A NEXRAD WSR-88D's half-power beamwidth (degrees). The beam guides draw its edges at half this
/// either side of each tilt's centreline.
pub const BEAMWIDTH_DEG: f32 = 0.95;

/// Where a beam at `bearing_deg`, `ground_km` out, on a tilt of `elevation_deg`, sits in the map
/// camera's local 3D frame — the CPU mirror of `radar_observed.wgsl`'s `beam_world`, so a guide
/// drawn from it lines up with the observed sweeps drawn by the shader.
#[allow(clippy::too_many_arguments)]
pub fn beam_local(
    camera: &crate::render::mercator::Camera,
    radar_lon: f64,
    radar_lat: f64,
    antenna_altitude_m: f64,
    vertical_exaggeration: f64,
    beam_rise: f64,
    bearing_deg: f64,
    ground_km: f64,
    elevation_deg: f32,
) -> Vec3 {
    let [lon, lat] = crate::geo::destination_point([radar_lon, radar_lat], bearing_deg, ground_km);
    let world = crate::render::mercator::lonlat_to_world(lon, lat);
    let wpp = camera.world_per_pixel();
    let mut dx = world.0 - camera.center.0;
    dx -= (dx + 0.5).floor(); // wrap to (-0.5, 0.5], matching `beam_world`'s own wrap
    let dy = world.1 - camera.center.1;
    let metres_to_px = crate::render::mercator::Camera::world_units_per_metre(radar_lat) / wpp;
    let altitude_m = antenna_altitude_m
        + tilt_altitude_m(ground_km, elevation_deg, vertical_exaggeration, beam_rise);
    Vec3::new(
        (dx / wpp) as f32,
        (-dy / wpp) as f32,
        (altitude_m * metres_to_px) as f32,
    )
}

/// A point in the camera's local 3D frame on screen, or `None` behind the camera.
pub fn project_local(
    camera: &crate::render::mercator::Camera,
    viewport_px: (f32, f32),
    point: Vec3,
) -> Option<(f32, f32)> {
    let clip =
        camera.view_projection(viewport_px) * glam::Vec4::new(point.x, point.y, point.z, 1.0);
    if clip.w <= f32::EPSILON {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some((
        (ndc.x + 1.0) * viewport_px.0 * 0.5,
        (1.0 - ndc.y) * viewport_px.1 * 0.5,
    ))
}

/// The screen position of `(lon, lat)` at `km` MSL in the 3D map, or `None` behind the camera.
pub fn lonlat_alt_screen(
    camera: &crate::render::mercator::Camera,
    viewport_px: (f32, f32),
    lon: f64,
    lat: f64,
    km: f64,
    vertical_exaggeration: f64,
) -> Option<(f32, f32)> {
    let world = crate::render::mercator::lonlat_to_world(lon, lat);
    let wpp = camera.world_per_pixel();
    let mut dx = world.0 - camera.center.0;
    dx -= (dx + 0.5).floor();
    let dy = world.1 - camera.center.1;
    let mpp = crate::render::mercator::Camera::world_units_per_metre(lat) / wpp;
    let p = Vec3::new(
        (dx / wpp) as f32,
        (-dy / wpp) as f32,
        (km * 1_000.0 * mpp * vertical_exaggeration) as f32,
    );
    project_local(camera, viewport_px, p)
}

/// A height ruler standing on the ground at `(lon, lat)`: the screen position of each tick from
/// sea level up to `top_km` every `step_km`, as `(km MSL, point)`, lowest first. Ticks behind
/// the camera are left out. Heights are MSL, the same frame as the 3D map's height surfaces.
pub fn height_ruler(
    camera: &crate::render::mercator::Camera,
    viewport_px: (f32, f32),
    lon: f64,
    lat: f64,
    vertical_exaggeration: f64,
    top_km: f64,
    step_km: f64,
) -> Vec<(f64, (f32, f32))> {
    let step = step_km.max(0.1);
    let ticks = (top_km / step).floor() as usize;
    (0..=ticks)
        .filter_map(|i| {
            let km = i as f64 * step;
            lonlat_alt_screen(camera, viewport_px, lon, lat, km, vertical_exaggeration)
                .map(|s| (km, s))
        })
        .collect()
}

/// Phase H5's beam guides, as screen polylines: each tilt's cone drawn as rings at a few ranges,
/// the lowest and highest tilts' centrelines with their half-beamwidth edges along `bearing_deg`,
/// and the antenna mast. A polyline breaks where a point falls behind the camera.
pub struct BeamGuides {
    /// `(tilt index, ring)` — lowest tilt first.
    pub rings: Vec<(usize, Vec<(f32, f32)>)>,
    /// `(is_edge, line)`: centrelines, then the edges either side of them.
    pub beams: Vec<(bool, Vec<(f32, f32)>)>,
    /// Ground point under the radar, and the antenna above it.
    pub mast: Option<[(f32, f32); 2]>,
}

/// The ranges (km) the cone rings are drawn at.
pub const BEAM_RING_KM: [f64; 4] = [50.0, 100.0, 150.0, 200.0];

/// Build [`BeamGuides`] for a radar and its distinct tilt angles (repeats such as SAILS
/// re-scans of 0.5° drawn once).
#[allow(clippy::too_many_arguments)]
pub fn beam_guides(
    camera: &crate::render::mercator::Camera,
    viewport_px: (f32, f32),
    radar_lon: f64,
    radar_lat: f64,
    ground_m: f64,
    antenna_altitude_m: f64,
    vertical_exaggeration: f64,
    beam_rise: f64,
    elevations_deg: &[f32],
    bearing_deg: f64,
    max_km: f64,
) -> BeamGuides {
    let at = |bearing: f64, km: f64, elev: f32| {
        project_local(
            camera,
            viewport_px,
            beam_local(
                camera,
                radar_lon,
                radar_lat,
                antenna_altitude_m,
                vertical_exaggeration,
                beam_rise,
                bearing,
                km,
                elev,
            ),
        )
    };
    let mut tilts: Vec<(usize, f32)> = Vec::new();
    for (i, &e) in elevations_deg.iter().enumerate() {
        if !tilts.iter().any(|&(_, t)| (t - e).abs() < 0.05) {
            tilts.push((i, e));
        }
    }
    tilts.sort_by(|a, b| a.1.total_cmp(&b.1));
    // Split a run of projected points wherever one is behind the camera.
    let lines = |pts: Vec<Option<(f32, f32)>>| -> Vec<Vec<(f32, f32)>> {
        let mut out = vec![Vec::new()];
        for p in pts {
            match p {
                Some(p) => out.last_mut().unwrap().push(p),
                None if !out.last().unwrap().is_empty() => out.push(Vec::new()),
                None => {}
            }
        }
        out.retain(|l| l.len() >= 2);
        out
    };
    let mut rings = Vec::new();
    for &(i, e) in &tilts {
        for km in BEAM_RING_KM.iter().copied().filter(|&km| km <= max_km) {
            let pts = (0..=72).map(|k| at(k as f64 * 5.0, km, e)).collect();
            rings.extend(lines(pts).into_iter().map(|l| (i, l)));
        }
    }
    let mut beams = Vec::new();
    let ends: Vec<f32> = match (tilts.first(), tilts.last()) {
        (Some(lo), Some(hi)) if hi.0 != lo.0 => vec![lo.1, hi.1],
        (Some(lo), _) => vec![lo.1],
        _ => Vec::new(),
    };
    for e in ends {
        for (edge, off) in [
            (false, 0.0),
            (true, -BEAMWIDTH_DEG / 2.0),
            (true, BEAMWIDTH_DEG / 2.0),
        ] {
            let pts = (0..=60)
                .map(|k| at(bearing_deg, max_km * k as f64 / 60.0, e + off))
                .collect();
            beams.extend(lines(pts).into_iter().map(|l| (edge, l)));
        }
    }
    let ground = beam_local(
        camera,
        radar_lon,
        radar_lat,
        ground_m,
        vertical_exaggeration,
        0.0,
        0.0,
        0.0,
        0.0,
    );
    let antenna = beam_local(
        camera,
        radar_lon,
        radar_lat,
        antenna_altitude_m,
        vertical_exaggeration,
        0.0,
        0.0,
        0.0,
        0.0,
    );
    let mast = match (
        project_local(camera, viewport_px, ground),
        project_local(camera, viewport_px, antenna),
    ) {
        (Some(a), Some(b)) => Some([a, b]),
        _ => None,
    };
    BeamGuides { rings, beams, mast }
}

/// An MRMS layer-height field (echo tops, km MSL) as a surface over the 3D map (Phase H6), for
/// the lon/lat box `bounds` (`[west, south, east, north]`), sampled on at most `max_side` cells a
/// side. Each cell's colour comes from `color_of(value)` (`None` = nothing there). Returns the
/// translucent surface and, every `grid_every` cells, the analysis grid lines drawn over it — the
/// look that keeps an analysed MRMS surface from reading as observed radar.
#[allow(clippy::too_many_arguments)]
pub fn height_surface_screen(
    camera: &crate::render::mercator::Camera,
    viewport_px: (f32, f32),
    origin: egui::Pos2,
    grid: &wxdata::mrms::MrmsField,
    bounds: [f64; 4],
    max_side: usize,
    vertical_exaggeration: f64,
    opacity: f32,
    color_of: impl Fn(f32) -> Option<[u8; 3]>,
) -> (egui::Mesh, Vec<Vec<egui::Pos2>>) {
    let w = bounds[0].max(grid.lon_west);
    let e = bounds[2].min(grid.lon_east);
    let s = bounds[1].max(grid.lat_south);
    let n = bounds[3].min(grid.lat_north);
    let mut mesh = egui::Mesh::default();
    if w >= e || s >= n {
        return (mesh, Vec::new());
    }
    let side = max_side.max(2);
    let (nx, ny) = (side, side);
    let wpp = camera.world_per_pixel();
    let mpp = crate::render::mercator::Camera::world_units_per_metre((s + n) * 0.5) / wpp;
    let vp = camera.view_projection(viewport_px);
    // Each sample: its value, and its screen position with clip depth.
    let mut pts: Vec<Option<(f32, egui::Pos2, f32)>> = Vec::with_capacity(nx * ny);
    for j in 0..ny {
        let lat = n - (n - s) * j as f64 / (ny - 1) as f64;
        for i in 0..nx {
            let lon = w + (e - w) * i as f64 / (nx - 1) as f64;
            let v = block_max(
                grid,
                lon,
                lat,
                (e - w) / (nx - 1) as f64 * 0.5,
                (n - s) / (ny - 1) as f64 * 0.5,
            );
            pts.push(v.and_then(|km| {
                let world = crate::render::mercator::lonlat_to_world(lon, lat);
                let mut dx = world.0 - camera.center.0;
                dx -= (dx + 0.5).floor();
                let dy = world.1 - camera.center.1;
                let p = Vec3::new(
                    (dx / wpp) as f32,
                    (-dy / wpp) as f32,
                    (km as f64 * 1_000.0 * mpp * vertical_exaggeration) as f32,
                );
                let clip = vp * glam::Vec4::new(p.x, p.y, p.z, 1.0);
                (clip.w > f32::EPSILON).then(|| {
                    let ndc = clip.truncate() / clip.w;
                    (
                        km,
                        origin
                            + egui::vec2(
                                (ndc.x + 1.0) * viewport_px.0 * 0.5,
                                (1.0 - ndc.y) * viewport_px.1 * 0.5,
                            ),
                        clip.w,
                    )
                })
            }));
        }
    }
    let at = |i: usize, j: usize| pts[j * nx + i];
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
    // (summed depth, corners as (value, screen position, depth)).
    type Quad = (f32, [(f32, egui::Pos2, f32); 4]);
    let mut quads: Vec<Quad> = Vec::new();
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            if let (Some(a), Some(b), Some(c), Some(d)) =
                (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1))
            {
                quads.push((a.2 + b.2 + c.2 + d.2, [a, b, c, d]));
            }
        }
    }
    quads.sort_by(|x, y| y.0.total_cmp(&x.0));
    for (_, q) in quads {
        let mean = (q[0].0 + q[1].0 + q[2].0 + q[3].0) / 4.0;
        let Some([r, g, b]) = color_of(mean) else {
            continue;
        };
        let col = egui::Color32::from_rgba_unmultiplied(r, g, b, alpha);
        let base = mesh.vertices.len() as u32;
        for v in &q {
            mesh.colored_vertex(v.1, col);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    // Analysis grid lines along every `grid_every`-th row and column, broken where there is no
    // surface.
    let grid_every = (side / 12).max(4);
    let mut lines = Vec::new();
    let mut run = |line: &mut Vec<egui::Pos2>, p: Option<(f32, egui::Pos2, f32)>| match p {
        Some(p) => line.push(p.1),
        None => {
            if line.len() >= 2 {
                lines.push(std::mem::take(line));
            } else {
                line.clear();
            }
        }
    };
    for j in (0..ny).step_by(grid_every) {
        let mut line = Vec::new();
        for i in 0..nx {
            run(&mut line, at(i, j));
        }
        run(&mut line, None);
    }
    for i in (0..nx).step_by(grid_every) {
        let mut line = Vec::new();
        for j in 0..ny {
            run(&mut line, at(i, j));
        }
        run(&mut line, None);
    }
    (mesh, lines)
}

/// The largest positive value of `grid` within `half_lon` x `half_lat` degrees of `(lon, lat)`:
/// what a coarse surface sample should show of the finer grid under it (a storm's top, not
/// whichever cell the sample point happens to land on).
fn block_max(
    grid: &wxdata::mrms::MrmsField,
    lon: f64,
    lat: f64,
    half_lon: f64,
    half_lat: f64,
) -> Option<f32> {
    if grid.nx == 0 || grid.ny == 0 || grid.values.len() != grid.nx * grid.ny {
        return None;
    }
    let col =
        |x: f64| ((x - grid.lon_west) / (grid.lon_east - grid.lon_west) * grid.nx as f64).floor();
    let row = |y: f64| {
        ((grid.lat_north - y) / (grid.lat_north - grid.lat_south) * grid.ny as f64).floor()
    };
    let (c0, c1) = (
        col(lon - half_lon).max(0.0),
        col(lon + half_lon).min(grid.nx as f64 - 1.0),
    );
    let (r0, r1) = (
        row(lat + half_lat).max(0.0),
        row(lat - half_lat).min(grid.ny as f64 - 1.0),
    );
    if c0 > c1 || r0 > r1 {
        return None;
    }
    let mut best: Option<f32> = None;
    for r in r0 as usize..=r1 as usize {
        for c in c0 as usize..=c1 as usize {
            let v = grid.values[r * grid.nx + c];
            if v.is_finite() && v > 0.0 && best.is_none_or(|b| v > b) {
                best = Some(v);
            }
        }
    }
    best
}

/// An isosurface (Phase H3, `wxdata::isosurface`) on screen as an `egui` mesh: each vertex moved
/// from the volume's radar-relative km into the camera's local frame exactly as `map_uniform`
/// places the smooth volume's box, triangles sorted far to near (the painter has no depth
/// buffer, and the surface is translucent), and optionally shaded by a fixed light from the
/// northwest and above so the surface's shape reads.
#[allow(clippy::too_many_arguments)]
pub fn iso_mesh_screen(
    camera: &crate::render::mercator::Camera,
    viewport_px: (f32, f32),
    origin: egui::Pos2,
    radar_lon: f64,
    radar_lat: f64,
    antenna_altitude_m: f64,
    vertical_exaggeration: f64,
    mesh: &wxdata::isosurface::IsoMesh,
    color: [u8; 3],
    opacity: f32,
    lit: bool,
) -> egui::Mesh {
    let radar_world = crate::render::mercator::lonlat_to_world(radar_lon, radar_lat);
    let wpp = camera.world_per_pixel();
    let dx = (radar_world.0 - camera.center.0 + 0.5).rem_euclid(1.0) - 0.5;
    let dy = camera.center.1 - radar_world.1;
    let mpp = crate::render::mercator::Camera::world_units_per_metre(radar_lat) / wpp;
    let local: Vec<Vec3> = mesh
        .verts
        .iter()
        .map(|p| {
            Vec3::new(
                (dx / wpp + p[0] as f64 * 1_000.0 * mpp) as f32,
                (dy / wpp + p[1] as f64 * 1_000.0 * mpp) as f32,
                ((antenna_altitude_m + p[2] as f64 * 1_000.0) * mpp * vertical_exaggeration) as f32,
            )
        })
        .collect();
    let vp = camera.view_projection(viewport_px);
    let screen: Vec<Option<(egui::Pos2, f32)>> = local
        .iter()
        .map(|p| {
            let clip = vp * glam::Vec4::new(p.x, p.y, p.z, 1.0);
            (clip.w > f32::EPSILON).then(|| {
                let ndc = clip.truncate() / clip.w;
                (
                    origin
                        + egui::vec2(
                            (ndc.x + 1.0) * viewport_px.0 * 0.5,
                            (1.0 - ndc.y) * viewport_px.1 * 0.5,
                        ),
                    clip.w,
                )
            })
        })
        .collect();
    let light = Vec3::new(-0.4, 0.5, 0.8).normalize();
    let mut order: Vec<(f32, usize)> = mesh
        .tris
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let w: f32 = t
                .iter()
                .map(|&v| screen[v as usize].map(|s| s.1))
                .sum::<Option<f32>>()?;
            Some((w, i))
        })
        .collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out = egui::Mesh::default();
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
    for (_, i) in order {
        let t = mesh.tris[i];
        let shade = if lit {
            let [a, b, c] = t.map(|v| local[v as usize]);
            let n = (b - a).cross(c - a).normalize_or_zero();
            0.35 + 0.65 * n.dot(light).abs()
        } else {
            1.0
        };
        let col = egui::Color32::from_rgba_unmultiplied(
            (color[0] as f32 * shade) as u8,
            (color[1] as f32 * shade) as u8,
            (color[2] as f32 * shade) as u8,
            alpha,
        );
        let base = out.vertices.len() as u32;
        for &v in &t {
            let (pos, _) = screen[v as usize].expect("filtered above");
            out.colored_vertex(pos, col);
        }
        out.add_triangle(base, base + 1, base + 2);
    }
    out
}

/// How many `t` samples to scan for a sign change before bisecting.
const PICK_SAMPLES: usize = 400;
/// Bisection refinements once a crossing is bracketed — 24 halvings of even the widest bracket
/// narrows it to sub-metre `t` resolution.
const PICK_BISECT_STEPS: u32 = 24;
/// `t = 1` is the far clip plane by construction (`Camera::screen_ray` unprojects clip-space
/// `z = -1`/`+1` to get the near/far points `screen_ray` returns), and `view_projection` sets that
/// plane at 40x the camera's own distance from the ground — deliberately generous headroom for the
/// raymarched volume, not a distance real radar coverage (≤ ~460 km) ever approaches. Scanning is
/// bounded at exactly 1 rather than beyond it: nothing past the far plane is ever rendered, so
/// nothing past it can be what a click was aiming at.
const PICK_T_MAX: f32 = 1.0;
/// Sample spacing is `t = PICK_T_MAX * (i / PICK_SAMPLES) ^ PICK_SAMPLE_POWER` rather than linear:
/// real content sits within a small fraction of `t`'s full 0..1 range (the far plane's 40x
/// headroom means a target at even the maximum real radar range can correspond to `t` under 0.05),
/// so linear sampling spent the vast majority of its budget scanning empty space beyond any tilt's
/// actual surface. A power curve concentrates samples near the camera, where the content is,
/// without needing to know the camera's exact distance to pick a tighter bound up front.
const PICK_SAMPLE_POWER: f32 = 4.0;

/// Which tilt (and where) a 3D click on the Observed radar volume actually lands on, by casting
/// the click ray against each tilt's own beam-height surface — the same geometry
/// `radar_observed.wgsl`'s `beam_world` places every gate on — instead of the map's flat ground
/// plane. A ground-plane click resolves to the same point no matter how high up the
/// visually-clicked echo actually sits, which is why a 3D gate-inspector click used to report
/// whatever tilt was already selected for the flat 2D view, regardless of what the user actually
/// clicked on.
///
/// Returns the index into `elevations_deg` (lowest-scanned-first, matching `MapView::elevations`)
/// of the tilt closest to the camera along the ray, and the ground `(lon, lat)` under that hit —
/// `None` when the ray doesn't cross any tilt's surface at all (a click past the radar's coverage,
/// or on empty sky above every tilt).
#[allow(clippy::too_many_arguments)]
pub fn pick_observed_tilt(
    camera: &crate::render::mercator::Camera,
    px: (f32, f32),
    viewport_px: (f32, f32),
    radar_lon: f64,
    radar_lat: f64,
    antenna_altitude_m: f64,
    vertical_exaggeration: f64,
    beam_rise: f64,
    elevations_deg: &[f32],
) -> Option<(usize, f64, f64)> {
    let (near, dir) = camera.screen_ray(px, viewport_px)?;
    pick_along_ray(
        near,
        dir,
        camera.center,
        camera.world_per_pixel(),
        radar_lon,
        radar_lat,
        antenna_altitude_m,
        vertical_exaggeration,
        beam_rise,
        elevations_deg,
    )
}

/// [`pick_observed_tilt`]'s geometry, taking the click ray directly rather than unprojecting it
/// from a screen pixel and camera — split out so the root-finding itself is testable against a
/// hand-built ray, without also having to construct a perspective camera whose particular pitch,
/// bearing and zoom don't happen to put the ray somewhere geometrically adversarial (grazing the
/// radar at a shallow angle, or genuinely passing behind a nearer tilt that legitimately occludes
/// the one under test — both real, both irrelevant to whether this function's own arithmetic is
/// right).
#[allow(clippy::too_many_arguments)]
fn pick_along_ray(
    near: Vec3,
    dir: Vec3,
    camera_center: (f64, f64),
    wpp: f64,
    radar_lon: f64,
    radar_lat: f64,
    antenna_altitude_m: f64,
    vertical_exaggeration: f64,
    beam_rise: f64,
    elevations_deg: &[f32],
) -> Option<(usize, f64, f64)> {
    let metres_to_px = crate::render::mercator::Camera::world_units_per_metre(radar_lat) / wpp;

    // Ground range (km) from the radar and altitude (m) above it, at ray parameter `t`, plus the
    // (lon, lat) under that point — computed together since every candidate needs at least two of
    // the three and the lon/lat is only needed once, for whichever `t` wins.
    let at = |t: f32| -> (f64, f64, f64, f64) {
        let p = near + dir * t;
        let world = (
            (camera_center.0 + p.x as f64 * wpp).rem_euclid(1.0),
            camera_center.1 - p.y as f64 * wpp,
        );
        let (lon, lat) = crate::render::mercator::world_to_lonlat(world.0, world.1);
        let (ground_km, _) = crate::geo::great_circle([radar_lon, radar_lat], [lon, lat]);
        let altitude_m = p.z as f64 / metres_to_px;
        (ground_km, altitude_m, lon, lat)
    };
    // `f(t)` for one candidate tilt: positive above its beam surface, negative below.
    let f = |t: f32, elevation_deg: f32| -> f64 {
        let (ground_km, altitude_m, ..) = at(t);
        altitude_m
            - antenna_altitude_m
            - tilt_altitude_m(ground_km, elevation_deg, vertical_exaggeration, beam_rise)
    };

    let mut best: Option<(f32, usize)> = None;
    for (i, &elevation_deg) in elevations_deg.iter().enumerate() {
        let mut prev_t = 0.0f32;
        let mut prev_f = f(prev_t, elevation_deg);
        for step in 1..=PICK_SAMPLES {
            let t = PICK_T_MAX * (step as f32 / PICK_SAMPLES as f32).powf(PICK_SAMPLE_POWER);
            let cur_f = f(t, elevation_deg);
            if prev_f.is_finite() && cur_f.is_finite() && prev_f.signum() != cur_f.signum() {
                let (mut lo, mut hi, mut flo) = (prev_t, t, prev_f);
                for _ in 0..PICK_BISECT_STEPS {
                    let mid = (lo + hi) * 0.5;
                    let fmid = f(mid, elevation_deg);
                    if fmid.signum() == flo.signum() {
                        lo = mid;
                        flo = fmid;
                    } else {
                        hi = mid;
                    }
                }
                let hit_t = (lo + hi) * 0.5;
                if best.is_none_or(|(best_t, _)| hit_t < best_t) {
                    best = Some((hit_t, i));
                }
                break; // this tilt's nearest crossing only — a farther one is occluded by this one
            }
            prev_t = t;
            prev_f = cur_f;
        }
    }

    let (hit_t, tilt) = best?;
    let (_, _, lon, lat) = at(hit_t);
    Some((tilt, lon, lat))
}

struct Gpu {
    tex: wgpu::Texture,
    lut: wgpu::Texture,
    /// `(n, nz)` of `tex`, so a volume of the same shape is written into it rather than into a
    /// new texture.
    dims: (u32, u32),
    bind_group: wgpu::BindGroup,
}

/// Write `up`'s voxels and colour table into `tex`/`lut` (which must have its shape).
fn write_volume(
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    lut: &wgpu::Texture,
    up: &Volume3dUpload,
) {
    let size = wgpu::Extent3d {
        width: up.n,
        height: up.n,
        depth_or_array_layers: up.nz,
    };
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &up.data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(up.n * 2),
            rows_per_image: Some(up.n),
        },
        size,
    );
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: lut,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &up.lut,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(256 * 4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 256,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
}

/// The GPU side of one raymarched volume. A volume of the same shape as `existing` is written
/// into its textures and keeps its bind group: a loop playing in 3D replaces the volume every
/// frame, and allocating a fresh multi-megabyte 3D texture, sampler and bind group each time
/// left the driver churning through memory until frames slowed and the device was lost.
fn volume_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bgl: &wgpu::BindGroupLayout,
    uniform_buf: &wgpu::Buffer,
    existing: Option<Gpu>,
    up: &Volume3dUpload,
) -> Gpu {
    if let Some(gpu) = existing.filter(|g| g.dims == (up.n, up.nz)) {
        write_volume(queue, &gpu.tex, &gpu.lut, up);
        return gpu;
    }
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("volume3d_tex"),
        size: wgpu::Extent3d {
            width: up.n,
            height: up.n,
            depth_or_array_layers: up.nz,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rg8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let lut = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("volume3d_lut"),
        size: wgpu::Extent3d {
            width: 256,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    write_volume(queue, &tex, &lut, up);
    let tex_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let lut_view = lut.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = volume_sampler(device);
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("raymarch_bg"),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&tex_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&lut_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    Gpu {
        tex,
        lut,
        dims: (up.n, up.nz),
        bind_group,
    }
}

/// The share of a pane's resolution the raymarch is drawn at. A phone or tablet GPU marching a
/// few hundred samples through a 3D texture for every pixel of a 2560-pixel screen heats until it
/// throttles: frames slow minute by minute and a frame that takes too long gets the device lost.
/// Half resolution is a quarter of that work and, the volume being smooth at that scale, looks
/// the same; the visual-quality guard trims it further while frames are slow.
pub fn raymarch_scale(android: bool, degraded: bool) -> f32 {
    let base = if android { 0.5 } else { 1.0 };
    if degraded {
        base * 0.7
    } else {
        base
    }
}

/// The offscreen image size for a `rect_px` (physical pixels) callback at `scale`, within the
/// device's texture limit.
pub fn offscreen_px(rect_px: [f32; 2], scale: f32, max_dim: u32) -> [u32; 2] {
    rect_px.map(|v| ((v * scale).round() as u32).clamp(1, max_dim.max(1)))
}

/// Draws an [`Offscreen`] image over the pane (`volume_blit.wgsl`).
struct Blit {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl Blit {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("volume_blit_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/volume_blit.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("volume_blit_layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("volume_blit_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // The image holds premultiplied colour, as the raymarch writes it.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("volume_blit_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            bgl,
            sampler,
        }
    }
}

/// A raymarched volume kept as an image: raymarched again only when what it was drawn from
/// changes (the camera and every other uniform, or a new volume), so a still 3D view costs a copy
/// per frame instead of a full march, and the march itself runs at [`raymarch_scale`].
struct Offscreen {
    _tex: wgpu::Texture,
    view: wgpu::TextureView,
    size: [u32; 2],
    bind_group: wgpu::BindGroup,
    /// The uniforms and volume generation the image holds, `None` before the first march.
    drawn: Option<(Vec<u8>, u64)>,
}

impl Offscreen {
    fn new(
        device: &wgpu::Device,
        blit: &Blit,
        format: wgpu::TextureFormat,
        size: [u32; 2],
    ) -> Self {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("volume_offscreen"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("volume_blit_bg"),
            layout: &blit.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&blit.sampler),
                },
            ],
        });
        Self {
            _tex: tex,
            view,
            size,
            bind_group,
            drawn: None,
        }
    }
}

/// Bring `slot` to `size` and, unless it already holds exactly this, raymarch `gpu` into it with
/// `uniform` (already written to the volume's uniform buffer) on `encoder`.
#[allow(clippy::too_many_arguments)]
fn march_offscreen(
    slot: &mut Option<Offscreen>,
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    blit: &Blit,
    format: wgpu::TextureFormat,
    pipeline: &wgpu::RenderPipeline,
    gpu: &Gpu,
    uniform: &Uniforms,
    generation: u64,
    size: [u32; 2],
) -> bool {
    if slot.as_ref().is_none_or(|o| o.size != size) {
        *slot = Some(Offscreen::new(device, blit, format, size));
    }
    let Some(off) = slot.as_mut() else {
        return false;
    };
    let key = (bytemuck::bytes_of(uniform).to_vec(), generation);
    if off.drawn.as_ref() == Some(&key) {
        return false;
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("raymarch_offscreen"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &off.view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &gpu.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    off.drawn = Some(key);
    true
}

fn draw_offscreen(blit: &Blit, off: &Offscreen, pass: &mut wgpu::RenderPass<'_>) {
    pass.set_pipeline(&blit.pipeline);
    pass.set_bind_group(0, &off.bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// Long-lived raymarch resources (pipeline + latest volume), stored in egui's callback map.
///
/// The pipeline is built the first time a 3D volume is uploaded, not at startup: compiling the
/// raymarch shader costs real time on a phone's driver, and most sessions never open the 3D view.
/// The target format the 3D pipeline must be compiled against, parked in the callback resources
/// so the first 3D frame can build [`Volume3dResources`] itself.
#[derive(Clone, Copy)]
pub struct Volume3dFormat(pub wgpu::TextureFormat);

pub struct Volume3dResources {
    format: wgpu::TextureFormat,
    pipeline: Option<wgpu::RenderPipeline>,
    bgl: wgpu::BindGroupLayout,
    uniform_buf: wgpu::Buffer,
    gpu: Option<Gpu>,
    blit: Option<Blit>,
    offscreen: Option<Offscreen>,
    /// Bumped with every volume upload, so the offscreen image is remarched for new data.
    generation: u64,
    /// How many times the volume has been raymarched (a still view should not add to it).
    pub(crate) marches: u64,
}

impl Volume3dResources {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("raymarch_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("raymarch_uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            format,
            pipeline: None,
            bgl,
            uniform_buf,
            gpu: None,
            blit: None,
            offscreen: None,
            generation: 0,
            marches: 0,
        }
    }

    /// Compile the raymarch pipeline on first use.
    fn ensure_pipeline(&mut self, device: &wgpu::Device) {
        if self.pipeline.is_some() {
            return;
        }
        let format = self.format;
        let bgl = &self.bgl;
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/raymarch.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("raymarch_layout"),
            bind_group_layouts: &[Some(bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("raymarch_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Premultiplied alpha (shader outputs rgb*a, a).
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        self.pipeline = Some(pipeline);
    }

    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        up: &Volume3dUpload,
    ) {
        // Every route to a 3D draw goes through here (the egui callback and `render_once`), so
        // this is the one place the pipeline has to exist by.
        self.ensure_pipeline(device);
        self.generation += 1;
        let existing = self.gpu.take();
        self.gpu = Some(volume_gpu(
            device,
            queue,
            &self.bgl,
            &self.uniform_buf,
            existing,
            up,
        ));
    }

    fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let (Some(gpu), Some(pipeline)) = (&self.gpu, &self.pipeline) {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    pub(crate) fn write_uniform(&self, queue: &wgpu::Queue, uniform: &Uniforms) {
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(uniform));
    }

    /// Raymarch into the offscreen image if what it holds is out of date.
    pub(crate) fn march(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniform: &Uniforms,
        size: [u32; 2],
    ) {
        let (Some(gpu), Some(pipeline)) = (&self.gpu, &self.pipeline) else {
            return;
        };
        let blit = self
            .blit
            .get_or_insert_with(|| Blit::new(device, self.format));
        self.marches += u64::from(march_offscreen(
            &mut self.offscreen,
            device,
            encoder,
            blit,
            self.format,
            pipeline,
            gpu,
            uniform,
            self.generation,
            size,
        ));
    }

    pub(crate) fn record_offscreen(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let (Some(_), Some(blit), Some(off)) = (&self.gpu, &self.blit, &self.offscreen) {
            draw_offscreen(blit, off, pass);
        }
    }

    /// Upload a volume + camera and raymarch it once into `view` (headless verify harness).
    pub fn render_once(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        upload: &Volume3dUpload,
        uniform: Uniforms,
        clear: wgpu::Color,
    ) {
        self.upload(device, queue, upload);
        self.render_uploaded(device, queue, view, uniform, clear);
    }

    /// [`Self::render_once`] on the volume already uploaded: the raymarch alone, as a frame
    /// that does not change the volume draws it (a performance trace times this).
    pub fn render_uploaded(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        uniform: Uniforms,
        clear: wgpu::Color,
    ) {
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniform));
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("raymarch_headless"),
        });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("raymarch_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.record(&mut pass);
        }
        queue.submit(Some(enc.finish()));
    }
}

/// Per-frame raymarch draw: an optional new volume upload + the current camera uniforms.
pub struct Volume3dCallback {
    pub upload: Option<Volume3dUpload>,
    pub uniform: Uniforms,
    /// The raymarch's image size, physical pixels ([`offscreen_px`]).
    pub target_px: [u32; 2],
}

impl egui_wgpu::CallbackTrait for Volume3dCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        // Built on first use rather than at startup: the raymarch pipeline is a few hundred ms
        // of shader compilation in front of the first frame, for a window most sessions never
        // open. `Volume3dFormat` is inserted at startup so we know what to compile against.
        if resources.get::<Volume3dResources>().is_none() {
            if let Some(Volume3dFormat(fmt)) = resources.get::<Volume3dFormat>().copied() {
                resources.insert(Volume3dResources::new(device, fmt));
            }
        }
        if let Some(res) = resources.get_mut::<Volume3dResources>() {
            if let Some(up) = &self.upload {
                res.upload(device, queue, up);
            }
            res.write_uniform(queue, &self.uniform);
            res.march(device, encoder, &self.uniform, self.target_px);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(res) = resources.get::<Volume3dResources>() {
            res.record_offscreen(pass);
        }
    }
}

/// The map-pitch "Smooth" representation: the same raymarch shader as the standalone 3D window,
/// but resident per pane rather than as the one volume the window shows. A deliberately separate
/// type from [`Volume3dResources`] — both live in the same `egui_wgpu::CallbackResources`
/// type-map, keyed by type, so sharing one would mean the window and an on-map pane fight over a
/// single GPU texture the moment they show different volumes at once.
///
/// Bounded to the app's platform pane ceiling rather than a `HashMap`, since the index is always
/// small and known ahead of time.
pub struct MapVolume3dResources {
    format: wgpu::TextureFormat,
    pipeline: Option<wgpu::RenderPipeline>,
    bgl: wgpu::BindGroupLayout,
    panes: [Option<Gpu>; crate::view::MAX_PANES],
    uniform_bufs: [Option<wgpu::Buffer>; crate::view::MAX_PANES],
    blit: Option<Blit>,
    offscreen: [Option<Offscreen>; crate::view::MAX_PANES],
    /// Per pane, bumped with every volume upload (see [`Offscreen`]).
    generations: [u64; crate::view::MAX_PANES],
}

impl MapVolume3dResources {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        // Identical layout to `Volume3dResources`'s — both compile the same `raymarch.wgsl`
        // against the same three bindings, just into separate pipeline objects so an on-map pane
        // and the popup window never share a bind group.
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map_raymarch_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        Self {
            format,
            pipeline: None,
            bgl,
            // `[None; MAX_PANES]` needs `Option<Gpu>: Copy`, which a
            // `wgpu::Texture`/`BindGroup` inside it is not; `from_fn` avoids that requirement.
            panes: std::array::from_fn(|_| None),
            uniform_bufs: std::array::from_fn(|_| None),
            blit: None,
            offscreen: std::array::from_fn(|_| None),
            generations: [0; crate::view::MAX_PANES],
        }
    }

    fn ensure_pipeline(&mut self, device: &wgpu::Device) {
        if self.pipeline.is_some() {
            return;
        }
        let format = self.format;
        let bgl = &self.bgl;
        // The same shader file as the window's raymarch — a second, independent pipeline object
        // compiled from it, not a shared one; see the type's own doc comment for why.
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/raymarch.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map_raymarch_layout"),
            bind_group_layouts: &[Some(bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("map_raymarch_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        self.pipeline = Some(pipeline);
    }

    fn upload_for_pane(
        &mut self,
        pane: usize,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        up: &Volume3dUpload,
    ) {
        self.ensure_pipeline(device);
        if self.uniform_bufs[pane].is_none() {
            self.uniform_bufs[pane] = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("map_raymarch_uniform"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let uniform_buf = self.uniform_bufs[pane]
            .as_ref()
            .expect("just created above");
        self.generations[pane] += 1;
        let existing = self.panes[pane].take();
        self.panes[pane] = Some(volume_gpu(
            device,
            queue,
            &self.bgl,
            uniform_buf,
            existing,
            up,
        ));
    }

    /// Free pane `pane`'s volume (its 3D was turned off).
    fn release_pane(&mut self, pane: usize) {
        self.panes[pane] = None;
        self.uniform_bufs[pane] = None;
        self.offscreen[pane] = None;
    }

    /// Raymarch pane `pane` into its offscreen image if what that holds is out of date.
    fn march_pane(
        &mut self,
        pane: usize,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniform: &Uniforms,
        size: [u32; 2],
    ) {
        let (Some(gpu), Some(pipeline)) = (&self.panes[pane], &self.pipeline) else {
            return;
        };
        let blit = self
            .blit
            .get_or_insert_with(|| Blit::new(device, self.format));
        let _ = march_offscreen(
            &mut self.offscreen[pane],
            device,
            encoder,
            blit,
            self.format,
            pipeline,
            gpu,
            uniform,
            self.generations[pane],
            size,
        );
    }

    fn set_uniform_for_pane(&mut self, pane: usize, queue: &wgpu::Queue, uniform: Uniforms) {
        if let Some(buf) = &self.uniform_bufs[pane] {
            queue.write_buffer(buf, 0, bytemuck::bytes_of(&uniform));
        }
    }

    fn record_for_pane(&self, pane: usize, pass: &mut wgpu::RenderPass<'_>) {
        if let (Some(_), Some(blit), Some(off)) =
            (&self.panes[pane], &self.blit, &self.offscreen[pane])
        {
            draw_offscreen(blit, off, pass);
        }
    }
}

/// Per-frame draw for one pane's map-pitch smooth volume: which pane (indexes
/// [`MapVolume3dResources`]'s fixed-size pane array), an optional new volume upload, and this
/// frame's camera/radar uniforms (built by [`map_uniform`]).
pub struct MapVolume3dCallback {
    pub pane: u32,
    /// Free the pane's volume instead of drawing it.
    pub release: bool,
    /// Shared with the pane's loop cache, so replaying a built frame costs no copy.
    pub upload: Option<std::sync::Arc<Volume3dUpload>>,
    pub uniform: Uniforms,
    /// The raymarch's image size, physical pixels ([`offscreen_px`]).
    pub target_px: [u32; 2],
}

impl egui_wgpu::CallbackTrait for MapVolume3dCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if resources.get::<MapVolume3dResources>().is_none() {
            if let Some(Volume3dFormat(fmt)) = resources.get::<Volume3dFormat>().copied() {
                resources.insert(MapVolume3dResources::new(device, fmt));
            }
        }
        if let Some(res) = resources.get_mut::<MapVolume3dResources>() {
            let pane = self.pane as usize;
            if self.release {
                res.release_pane(pane);
                return Vec::new();
            }
            if let Some(up) = &self.upload {
                res.upload_for_pane(pane, device, queue, up);
            }
            res.set_uniform_for_pane(pane, queue, self.uniform);
            res.march_pane(pane, device, encoder, &self.uniform, self.target_px);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(res) = resources.get::<MapVolume3dResources>() {
            res.record_for_pane(self.pane as usize, pass);
        }
    }
}

#[cfg(test)]
mod cc_anomaly_tests {
    use super::cc_anomaly_uniform;
    use crate::view::CcAnomaly;

    /// Correlation coefficient's palette range (`Moment::CorrelationCoefficient::value_range`),
    /// which is what both 3D paths bin CC against.
    const CC: (f32, f32) = (0.0, 1.05);

    /// Mirror of the smoothstep both shaders apply, so the table in `CcAnomaly`'s doc comment can
    /// be checked against the real numbers rather than asserted by eye.
    fn alpha_at(cc_value: f32, a: CcAnomaly, inverted: bool) -> f32 {
        let [clear, full, faintest, on] = cc_anomaly_uniform(a, CC, inverted);
        if on < 0.5 {
            return 1.0;
        }
        let mut idx = super::threshold_index(cc_value, CC);
        if inverted {
            idx = 257.0 - idx;
        }
        let t = ((idx - clear) / (full - clear)).clamp(0.0, 1.0);
        faintest + (1.0 - faintest) * t * t * (3.0 - 2.0 * t)
    }

    /// The whole point of the mode: opacity must fall as CC rises. A regression that flipped this
    /// would restore precisely the behaviour it replaced — background rain solid, debris hidden.
    #[test]
    fn opacity_decreases_as_correlation_rises() {
        let a = CcAnomaly::default();
        let mut previous = f32::INFINITY;
        for step in 0..=40 {
            let cc = 0.60 + step as f32 * 0.01;
            let alpha = alpha_at(cc, a, false);
            assert!(
                alpha <= previous + 1e-6,
                "alpha rose at CC {cc:.2}: {alpha} after {previous}"
            );
            previous = alpha;
        }
    }

    /// The tiers `CcAnomaly` documents, as numbers. Not meteorological claims — just the promise
    /// that the defaults land where the doc comment says they do.
    #[test]
    fn the_default_ramp_matches_its_documented_tiers() {
        let a = CcAnomaly::default();
        let at = |cc| alpha_at(cc, a, false);
        assert!(at(0.99) <= 0.06, "background should be nearly transparent");
        assert!(
            (0.03..0.15).contains(&at(0.96)),
            "0.95-0.97 should be faint"
        );
        assert!(
            (0.15..0.45).contains(&at(0.92)),
            "0.90-0.95 should be visible"
        );
        assert!(
            (0.55..0.95).contains(&at(0.85)),
            "0.80-0.90 should be strong"
        );
        assert!(
            at(0.70) >= 0.99,
            "below the solid edge should be full strength"
        );
    }

    /// The debris volume is raymarched with its indices flipped, so its ramp has to run the other
    /// way round to still mean the same thing. Both paths must agree on the alpha for a given CC,
    /// or the same storm reads differently depending on which 3D mode is open.
    #[test]
    fn the_inverted_debris_volume_gets_the_same_alpha_for_the_same_cc() {
        let a = CcAnomaly::default();
        for step in 0..=17 {
            let cc = 0.80 + step as f32 * 0.01;
            let (plain, debris) = (alpha_at(cc, a, false), alpha_at(cc, a, true));
            assert!(
                (plain - debris).abs() < 0.02,
                "CC {cc:.2}: observed {plain} vs debris {debris}"
            );
        }
        // And the endpoints really are swapped in index space, which is what makes that work.
        let [clear_p, full_p, ..] = cc_anomaly_uniform(a, CC, false);
        let [clear_i, full_i, ..] = cc_anomaly_uniform(a, CC, true);
        assert!(clear_p > full_p, "plain: high CC is the high index");
        assert!(clear_i < full_i, "inverted: high CC is the low index");
    }

    /// Dragging both edges together would divide by zero in the shader. A hard step is a fine
    /// thing to ask for; a NaN alpha is not.
    #[test]
    fn collapsing_the_two_edges_still_gives_a_usable_ramp() {
        let a = CcAnomaly {
            clear_cc: 0.90,
            opaque_cc: 0.90,
            ..CcAnomaly::default()
        };
        let [clear, full, ..] = cc_anomaly_uniform(a, CC, false);
        assert!(
            (clear - full).abs() > f32::EPSILON,
            "span collapsed to zero"
        );
        assert!(alpha_at(0.95, a, false).is_finite());
        assert!(alpha_at(0.85, a, false) > alpha_at(0.95, a, false));
    }

    /// A crossed pair (solid edge dragged above the clear edge) must not invert the ramp — that
    /// would make high CC solid, which is the bug this whole mode exists to fix.
    #[test]
    fn a_crossed_pair_does_not_flip_the_ramp() {
        let a = CcAnomaly {
            clear_cc: 0.85,
            opaque_cc: 0.95,
            ..CcAnomaly::default()
        };
        assert!(alpha_at(0.70, a, false) > alpha_at(0.99, a, false));
    }

    /// Disabled must be inert, since that same all-zero value is what every non-CC volume passes.
    #[test]
    fn disabled_is_all_zero() {
        let a = CcAnomaly {
            enabled: false,
            ..CcAnomaly::default()
        };
        assert_eq!(cc_anomaly_uniform(a, CC, false), [0.0; 4]);
    }
}

#[cfg(test)]
mod plane_tests {
    use super::{plane_uniform, VerticalPlane};
    use glam::Vec3;

    // A 2x2x1 box centered on the origin, same shape `orbit_uniform` and `map_uniform` both hand
    // in (they differ only in where that box sits and how big it is).
    const BOX_MIN: Vec3 = Vec3::new(-1.0, -1.0, 0.0);
    const BOX_MAX: Vec3 = Vec3::new(1.0, 1.0, 1.0);

    #[test]
    fn disabled_plane_is_inert() {
        assert_eq!(plane_uniform(None, BOX_MIN, BOX_MAX), [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn north_bearing_is_a_unit_normal_pointing_north() {
        let p = VerticalPlane {
            bearing_deg: 0.0,
            offset: 0.0,
            thickness: None,
        };
        let [nx, ny, _, on] = plane_uniform(Some(p), BOX_MIN, BOX_MAX);
        assert_eq!(on, 1.0);
        assert!(nx.abs() < 1e-5, "north has no east component: {nx}");
        assert!((ny - 1.0).abs() < 1e-5, "north is +y: {ny}");
    }

    #[test]
    fn east_bearing_is_a_unit_normal_pointing_east() {
        let p = VerticalPlane {
            bearing_deg: 90.0,
            offset: 0.0,
            thickness: None,
        };
        let [nx, ny, _, _] = plane_uniform(Some(p), BOX_MIN, BOX_MAX);
        assert!((nx - 1.0).abs() < 1e-5, "east is +x: {nx}");
        assert!(ny.abs() < 1e-5, "east has no north component: {ny}");
    }

    #[test]
    fn zero_offset_passes_through_the_box_center() {
        // Center is (0,0) here, so the plane's distance along any normal is 0.
        let p = VerticalPlane {
            bearing_deg: 37.0,
            offset: 0.0,
            thickness: None,
        };
        let [.., d, _] = plane_uniform(Some(p), BOX_MIN, BOX_MAX);
        assert!(
            d.abs() < 1e-5,
            "plane through a centered box's own center: {d}"
        );
    }

    #[test]
    fn offset_scales_with_the_box_half_width_not_a_fixed_distance() {
        // This box's half-width is 1.0 (spans -1..1); offset 0.5 should land the plane at
        // world distance 0.5 along its normal from the box center.
        let p = VerticalPlane {
            bearing_deg: 0.0,
            offset: 0.5,
            thickness: None,
        };
        let [_, ny, d, _] = plane_uniform(Some(p), BOX_MIN, BOX_MAX);
        assert!((ny - 1.0).abs() < 1e-5);
        assert!((d - 0.5).abs() < 1e-5, "d: {d}");

        // A box twice as wide (still centered on the origin) scales the same fractional offset
        // to twice the world distance.
        let wide_min = Vec3::new(-2.0, -2.0, 0.0);
        let wide_max = Vec3::new(2.0, 2.0, 1.0);
        let [_, _, d_wide, _] = plane_uniform(Some(p), wide_min, wide_max);
        assert!((d_wide - 1.0).abs() < 1e-5, "d_wide: {d_wide}");
    }

    #[test]
    fn a_box_not_centered_on_the_origin_offsets_the_plane_with_it() {
        // Same shape as BOX_MIN..BOX_MAX but shifted +5 in x and +3 in y — as map_uniform's box
        // is, sitting wherever the radar is on screen rather than at a fixed origin.
        let shifted_min = BOX_MIN + Vec3::new(5.0, 3.0, 0.0);
        let shifted_max = BOX_MAX + Vec3::new(5.0, 3.0, 0.0);
        let p = VerticalPlane {
            bearing_deg: 0.0,
            offset: 0.0,
            thickness: None,
        };
        let [_, ny, d, _] = plane_uniform(Some(p), shifted_min, shifted_max);
        assert!((ny - 1.0).abs() < 1e-5);
        // The plane through the (shifted) center: d = normal . center = 1*3 = 3.
        assert!((d - 3.0).abs() < 1e-5, "d: {d}");
    }

    #[test]
    fn no_thickness_means_no_slab() {
        let p = VerticalPlane {
            bearing_deg: 0.0,
            offset: 0.0,
            thickness: None,
        };
        assert_eq!(
            super::plane_slab_uniform(Some(p), BOX_MIN, BOX_MAX),
            [0.0; 4]
        );
        // A disabled plane has no slab either, same as it has no normal.
        assert_eq!(super::plane_slab_uniform(None, BOX_MIN, BOX_MAX), [0.0; 4]);
    }

    #[test]
    fn thickness_scales_with_the_box_half_width_like_offset_does() {
        // Same box/fraction relationship `offset_scales_with_the_box_half_width_not_a_fixed_
        // distance` proves for `d` — `thickness` uses the same `half_extent` helper, so it should
        // agree exactly on a box with half-width 1.0.
        let p = VerticalPlane {
            bearing_deg: 0.0,
            offset: 0.0,
            thickness: Some(0.25),
        };
        let [half_thickness, ..] = super::plane_slab_uniform(Some(p), BOX_MIN, BOX_MAX);
        assert!(
            (half_thickness - 0.25).abs() < 1e-5,
            "half_thickness: {half_thickness}"
        );

        let wide_min = Vec3::new(-2.0, -2.0, 0.0);
        let wide_max = Vec3::new(2.0, 2.0, 1.0);
        let [half_thickness_wide, ..] = super::plane_slab_uniform(Some(p), wide_min, wide_max);
        assert!(
            (half_thickness_wide - 0.5).abs() < 1e-5,
            "half_thickness_wide: {half_thickness_wide}"
        );
    }
}

#[cfg(test)]
mod tf_stops_tests {
    use super::{tf_uniform, ColorStops, TfStops, MAX_STOPS};

    /// Colour stops (1008.md C2): sorted and bounded like the opacity stops, interpolated and
    /// held past the ends, inserting keeps the colours, and the rebuilt table changes only data
    /// entries' colour, never their alpha or the empty and range-folded entries.
    #[test]
    fn colour_stops_recolour_only_the_data_entries() {
        let c = ColorStops::new(&[(60.0, [255, 0, 0]), (0.0, [0, 0, 255]), (f32::NAN, [9; 3])])
            .unwrap();
        assert_eq!(c.points()[0].0, 0.0, "sorted, non-finite dropped");
        assert_eq!(c.len(), 2);
        assert!(ColorStops::new(&[(1.0, [0; 3])]).is_none());
        assert_eq!(c.color(-10.0), [0, 0, 255]);
        assert_eq!(c.color(90.0), [255, 0, 0]);
        assert_eq!(c.color(30.0), [128, 0, 128]);
        let mut d = c.clone();
        assert!(d.insert(30.0));
        // A new stop changes nothing until moved, but for its colour's rounding to a byte.
        let (x, y) = (d.color(15.0), c.color(15.0));
        assert!(
            x.iter().zip(y).all(|(a, b)| a.abs_diff(b) <= 1),
            "{x:?} {y:?}"
        );
        assert!(!d.remove(5) && d.remove(1) && !d.remove(0));
        let many: Vec<(f32, [u8; 3])> = (0..12).map(|i| (i as f32, [0; 3])).collect();
        assert_eq!(ColorStops::new(&many).unwrap().len(), MAX_STOPS);

        let mut base = vec![0u8; 1024];
        base[4..8].copy_from_slice(&[1, 2, 3, 4]); // range-folded
        for raw in 2..256 {
            base[raw * 4..raw * 4 + 4].copy_from_slice(&[7, 7, 7, (raw % 200) as u8]);
        }
        let lut = c.lut(&base, (0.0, 60.0));
        assert_eq!(
            &lut[..8],
            &base[..8],
            "empty and range-folded entries unchanged"
        );
        assert_eq!(
            &lut[8..12],
            &[0, 0, 255, 2],
            "index 2 is the low end, alpha kept"
        );
        assert_eq!(
            &lut[255 * 4..256 * 4],
            &[255, 0, 0, 55],
            "index 255 the high end"
        );
        assert!(lut.chunks(4).zip(base.chunks(4)).all(|(a, b)| a[3] == b[3]));
        assert_eq!(
            c.lut(&[1, 2], (0.0, 1.0)),
            vec![1, 2],
            "a short table is left alone"
        );

        let saved = c.to_saved();
        assert_eq!(ColorStops::from_saved(&saved), Some(c.clone()));
        assert_eq!(
            ColorStops::from_saved(&[[0.0, -5.0, 300.0, 12.4], [1.0, 0.0, 0.0, 0.0]])
                .unwrap()
                .points()[0]
                .1,
            [0, 255, 12],
            "saved channels clamp to bytes"
        );
    }

    #[test]
    fn stops_sort_clamp_and_keep_between_two_and_eight() {
        let t = TfStops::new(&[[50.0, 1.4], [10.0, 0.0], [30.0, 0.5]]).unwrap();
        assert_eq!(t.points(), [[10.0, 0.0], [30.0, 0.5], [50.0, 1.0]]);
        assert!(
            TfStops::new(&[[1.0, 0.0]]).is_none(),
            "one stop is not a curve"
        );
        let many: Vec<[f32; 2]> = (0..12).map(|i| [i as f32, 0.5]).collect();
        assert_eq!(TfStops::new(&many).unwrap().len(), MAX_STOPS);
    }

    #[test]
    fn the_curve_is_piecewise_linear_and_flat_past_its_ends() {
        let t = TfStops::new(&[[10.0, 0.0], [20.0, 1.0], [30.0, 0.2]]).unwrap();
        assert_eq!(t.alpha(0.0), 0.0);
        assert!((t.alpha(15.0) - 0.5).abs() < 1e-6);
        assert!((t.alpha(25.0) - 0.6).abs() < 1e-6);
        assert_eq!(t.alpha(99.0), 0.2);
    }

    #[test]
    fn adding_a_stop_changes_nothing_until_it_moves_and_two_always_remain() {
        let mut t = TfStops::from([[0.0, 0.0], [10.0, 0.2], [20.0, 0.8], [30.0, 1.0]]);
        let before: Vec<f32> = (0..=30).map(|v| t.alpha(v as f32)).collect();
        assert!(t.insert(15.0));
        assert_eq!(t.len(), 5);
        let after: Vec<f32> = (0..=30).map(|v| t.alpha(v as f32)).collect();
        for (a, b) in before.iter().zip(&after) {
            assert!((a - b).abs() < 1e-5);
        }
        while t.len() > 2 {
            assert!(t.remove(1));
        }
        assert!(!t.remove(0), "a curve keeps two stops");
        for _ in 0..10 {
            t.insert(5.0);
        }
        assert_eq!(t.len(), MAX_STOPS, "full at eight");
    }

    #[test]
    fn the_uniform_carries_every_stop_and_pads_with_the_last() {
        let six = TfStops::new(&[
            [2.0, 0.0],
            [20.0, 0.1],
            [40.0, 0.2],
            [60.0, 0.3],
            [80.0, 0.4],
            [100.0, 0.9],
        ])
        .unwrap();
        let [x, a, x2, a2, n] = tf_uniform(Some(six));
        assert_eq!((x, a), ([2.0, 20.0, 40.0, 60.0], [0.0, 0.1, 0.2, 0.3]));
        assert_eq!(
            (x2, a2),
            ([80.0, 100.0, 100.0, 100.0], [0.4, 0.9, 0.9, 0.9])
        );
        assert_eq!(n[0], 6.0);
        // Off is still `tf_a[0] < 0`.
        assert!(tf_uniform(None)[1][0] < 0.0);
        // A four-point curve is the legacy four-point uniform.
        let four = TfStops::from([[2.0, 0.0], [100.0, 0.2], [200.0, 0.6], [255.0, 1.0]]);
        let [x, a, ..] = tf_uniform(Some(four));
        assert_eq!((x, a), ([2.0, 100.0, 200.0, 255.0], [0.0, 0.2, 0.6, 1.0]));
        assert_eq!(
            four.as_four(),
            [[2.0, 0.0], [100.0, 0.2], [200.0, 0.6], [255.0, 1.0]]
        );
        // Six stops sample to four at the ends and thirds, on the curve.
        let s = six.as_four();
        assert_eq!((s[0][0], s[3][0]), (2.0, 100.0));
        assert!((s[1][1] - six.alpha(s[1][0])).abs() < 1e-6);
    }
}

#[cfg(test)]
mod roi_box_tests {
    #[test]
    fn a_region_box_sits_east_and_north_of_the_radar_by_its_offset() {
        let cam = crate::render::mercator::Camera::at_lonlat(-97.3, 35.3, 8.0);
        let upload = |center_km: [f32; 2]| super::Volume3dUpload {
            data: Vec::new(),
            n: 64,
            nz: 16,
            lut: Vec::new(),
            half_km: 20.0,
            center_km,
            top_km: 12.0,
            outside: 0.0,
            value_range: None,
            lut_range: None,
        };
        let u = |c| {
            super::map_uniform(
                &cam,
                (800.0, 600.0),
                -97.3,
                35.3,
                370.0,
                &upload(c),
                128,
                super::View3d::default(),
                1.0,
                1.0,
            )
        };
        let (at_radar, offset) = (u([0.0, 0.0]), u([10.0, -5.0]));
        let width = at_radar.box_max[0] - at_radar.box_min[0];
        let px_per_km = width / 40.0;
        let dx = offset.box_min[0] - at_radar.box_min[0];
        let dy = offset.box_min[1] - at_radar.box_min[1];
        assert!(
            (dx - 10.0 * px_per_km).abs() < 1e-2 * px_per_km,
            "{dx} vs {px_per_km}"
        );
        assert!((dy + 5.0 * px_per_km).abs() < 1e-2 * px_per_km, "{dy}");
        // Same size, same vertical extent: only moved.
        assert_eq!(offset.box_max[0] - offset.box_min[0], width);
        assert_eq!(offset.box_min[2], at_radar.box_min[2]);
    }
}

#[cfg(test)]
mod translucent_tests {
    use super::{composite_ray, render_uniform, step_alpha, VolumeRender, BOX_MAX, BOX_MIN};

    /// A uniform medium of opacity `a` per km, `len_km` deep, marched in `steps` steps.
    fn uniform(a: f32, len_km: f32, steps: usize) -> f32 {
        let dt = len_km / steps as f32;
        composite_ray(&vec![([1.0, 1.0, 1.0], a); steps], dt).1
    }

    #[test]
    fn opacity_does_not_depend_on_the_step_count() {
        // The analytic answer is 1 - (1 - a)^L; 0.1 per km through 12 km is 0.7176.
        let want = 1.0 - 0.9f32.powf(12.0);
        for steps in [8, 32, 96, 160, 256, 1024] {
            let got = uniform(0.1, 12.0, steps);
            assert!((got - want).abs() < 1e-4, "{steps} steps: {got} vs {want}");
        }
        // One km of the medium is exactly its per-km opacity, whatever it is split into.
        for steps in [1, 4, 64] {
            assert!((uniform(0.35, 1.0, steps) - 0.35).abs() < 1e-5);
        }
    }

    #[test]
    fn a_fully_opaque_or_empty_value_composes_as_expected() {
        assert_eq!(step_alpha(0.0, 3.0), 0.0);
        assert!(step_alpha(1.0, 1.0) > 0.999);
        // Front to back: an opaque red sample hides the blue one behind it.
        let (rgb, a) = composite_ray(&[([1.0, 0.0, 0.0], 1.0), ([0.0, 0.0, 1.0], 1.0)], 1.0);
        assert!(a > 0.99 && rgb[0] > 0.99 && rgb[2] < 0.01, "{rgb:?} {a}");
    }

    #[test]
    fn compositing_differs_from_mip_where_a_weak_shell_hides_a_core() {
        // A ray through 10 km of weak echo (0.05/km) before 2 km of a core (0.8/km): MIP would
        // show the core's colour at full strength; compositing shows the core seen through the
        // haze in front of it — the defining difference between the modes.
        let haze = ([0.2, 0.2, 0.2], 0.05);
        let core = ([1.0, 0.0, 0.0], 0.8);
        let mut ray = vec![haze; 40];
        ray.extend(vec![core; 8]);
        let (rgb, a) = composite_ray(&ray, 0.25);
        let haze_a = 1.0 - 0.95f32.powf(10.0);
        let core_a = 1.0 - 0.2f32.powf(2.0);
        assert!((a - (haze_a + (1.0 - haze_a) * core_a)).abs() < 1e-4);
        assert!((rgb[0] - (haze_a * 0.2 + (1.0 - haze_a) * core_a)).abs() < 1e-4);
        assert!(
            rgb[0] < 0.9,
            "the core is dimmed by the haze in front of it"
        );
    }

    #[test]
    fn the_render_uniform_measures_both_axes_in_real_km() {
        // The orbit box is 2 units wide and 0.5 tall: a 150 km half-width volume 18 km deep.
        let [mode, h, v, lit] =
            render_uniform(VolumeRender::TranslucentLit, BOX_MIN, BOX_MAX, 150.0, 18.0);
        assert_eq!((mode, lit), (1.0, 1.0));
        assert!((h - 150.0).abs() < 1e-4 && (v - 36.0).abs() < 1e-4);
        let [mode, _, _, lit] = render_uniform(VolumeRender::Mip, BOX_MIN, BOX_MAX, 150.0, 18.0);
        assert_eq!((mode, lit), (0.0, 0.0));
    }
}

#[cfg(test)]
mod cappi_marker_tests {
    use super::cappi_marker_uniform;
    use glam::Vec3;

    const BOX_MIN: Vec3 = Vec3::new(-1.0, -1.0, 0.0);
    const BOX_MAX: Vec3 = Vec3::new(1.0, 1.0, 18.0);

    #[test]
    fn disabled_marker_is_inert() {
        assert_eq!(cappi_marker_uniform(None, 18.0, BOX_MIN, BOX_MAX), [0.0; 4]);
    }

    #[test]
    fn zero_altitude_lands_on_the_box_floor() {
        let [z, _, _, on] = cappi_marker_uniform(Some(0.0), 18.0, BOX_MIN, BOX_MAX);
        assert_eq!(on, 1.0);
        assert!((z - BOX_MIN.z).abs() < 1e-5, "z: {z}");
    }

    #[test]
    fn top_km_altitude_lands_on_the_box_ceiling() {
        let [z, _, _, on] = cappi_marker_uniform(Some(18.0), 18.0, BOX_MIN, BOX_MAX);
        assert_eq!(on, 1.0);
        assert!((z - BOX_MAX.z).abs() < 1e-5, "z: {z}");
    }

    #[test]
    fn half_altitude_lands_at_the_box_midpoint() {
        let [z, ..] = cappi_marker_uniform(Some(9.0), 18.0, BOX_MIN, BOX_MAX);
        let mid = (BOX_MIN.z + BOX_MAX.z) * 0.5;
        assert!((z - mid).abs() < 1e-5, "z: {z}, mid: {mid}");
    }

    #[test]
    fn an_altitude_outside_the_volumes_own_top_is_inert_rather_than_pinned() {
        // Pinning to the nearest edge would show a plane at the wrong height, which is worse than
        // not showing one at all.
        assert_eq!(
            cappi_marker_uniform(Some(25.0), 18.0, BOX_MIN, BOX_MAX),
            [0.0; 4]
        );
        assert_eq!(
            cappi_marker_uniform(Some(-1.0), 18.0, BOX_MIN, BOX_MAX),
            [0.0; 4]
        );
    }

    #[test]
    fn a_non_positive_top_km_is_inert() {
        assert_eq!(
            cappi_marker_uniform(Some(3.0), 0.0, BOX_MIN, BOX_MAX),
            [0.0; 4]
        );
    }

    #[test]
    fn the_band_is_a_small_fraction_of_the_box_height_not_a_fixed_world_distance() {
        let [_, half_width, ..] = cappi_marker_uniform(Some(9.0), 18.0, BOX_MIN, BOX_MAX);
        let wide_min = Vec3::new(-1.0, -1.0, 0.0);
        let wide_max = Vec3::new(1.0, 1.0, 36.0);
        let [_, half_width_wide, ..] = cappi_marker_uniform(Some(18.0), 18.0, wide_min, wide_max);
        assert!(
            (half_width_wide - half_width * 2.0).abs() < 1e-4,
            "half_width: {half_width}, half_width_wide: {half_width_wide}"
        );
    }
}

#[cfg(test)]
mod ground_track_tests {
    use super::{plane_ground_track, VerticalPlane};
    use crate::geo::great_circle;

    const RADAR: [f64; 2] = [-97.5, 35.3];

    #[test]
    fn zero_offset_runs_through_the_radar_site() {
        // Same fact `zero_offset_passes_through_the_box_center` proves for the shader uniform:
        // an unoffset plane passes through the box center, which on the main map *is* the site.
        let p = VerticalPlane {
            bearing_deg: 37.0,
            offset: 0.0,
            thickness: None,
        };
        let (a, b) = plane_ground_track(p, RADAR, 150.0);
        // The site sits on the segment `a..b`, i.e. equidistant-ish from both ends and each end
        // is `half_km * sqrt(2)` from the site — check the endpoint distances directly rather
        // than midpoint arithmetic on lon/lat, which isn't linear.
        let half_len = 150.0 * std::f64::consts::SQRT_2;
        let (km_a, _) = great_circle(RADAR, a);
        let (km_b, _) = great_circle(RADAR, b);
        assert!((km_a - half_len).abs() < 0.5, "km_a: {km_a}");
        assert!((km_b - half_len).abs() < 0.5, "km_b: {km_b}");
    }

    #[test]
    fn the_line_runs_perpendicular_to_the_planes_bearing() {
        // A north-pointing plane (bearing 0) cuts an east-west line: both endpoints should bear
        // due east/west (90/270) from the offset foot point, not north/south.
        let p = VerticalPlane {
            bearing_deg: 0.0,
            offset: 0.0,
            thickness: None,
        };
        let (a, b) = plane_ground_track(p, RADAR, 150.0);
        let (_, brg_a) = great_circle(RADAR, a);
        let (_, brg_b) = great_circle(RADAR, b);
        assert!((brg_a - 90.0).abs() < 0.5, "brg_a: {brg_a}");
        assert!((brg_b - 270.0).abs() < 0.5, "brg_b: {brg_b}");
    }

    #[test]
    fn offset_moves_the_line_along_the_bearing_not_the_line_itself() {
        // A north-pointing plane offset 0.5 (half the box) should have its line's foot point
        // 75 km (half of 150) due north of the site — same distance/bearing math `offset_scales_
        // with_the_box_half_width_not_a_fixed_distance` proves for the shader's own `d`.
        let p = VerticalPlane {
            bearing_deg: 0.0,
            offset: 0.5,
            thickness: None,
        };
        let (a, b) = plane_ground_track(p, RADAR, 150.0);
        let midpoint_bearing_from_radar = {
            let (km_a, brg_a) = great_circle(RADAR, a);
            let (km_b, brg_b) = great_circle(RADAR, b);
            // Both endpoints are still ~equidistant from the site's due-north line, confirming
            // the line shifted north as a whole rather than tilting.
            assert!((km_a - km_b).abs() < 0.5, "km_a: {km_a}, km_b: {km_b}");
            (brg_a, brg_b)
        };
        // Endpoints bear roughly NE/NW from the site now, not due E/W, since the line itself
        // moved north of the radar.
        assert!(
            midpoint_bearing_from_radar.0 < 90.0,
            "brg_a: {:?}",
            midpoint_bearing_from_radar
        );
        assert!(
            midpoint_bearing_from_radar.1 > 270.0,
            "brg_b: {:?}",
            midpoint_bearing_from_radar
        );
    }

    #[test]
    fn negative_offset_moves_the_line_the_opposite_way() {
        let north = plane_ground_track(
            VerticalPlane {
                bearing_deg: 0.0,
                offset: 0.5,
                thickness: None,
            },
            RADAR,
            150.0,
        );
        let south = plane_ground_track(
            VerticalPlane {
                bearing_deg: 0.0,
                offset: -0.5,
                thickness: None,
            },
            RADAR,
            150.0,
        );
        // The two feet are symmetric about the site: the north-offset line's near endpoint is
        // north of the site, the negative-offset line's near endpoint is south.
        let (_, brg_north_a) = great_circle(RADAR, north.0);
        let (_, brg_south_a) = great_circle(RADAR, south.0);
        assert!(brg_north_a < 90.0, "north.0 bearing: {brg_north_a}");
        assert!(
            brg_south_a > 90.0 && brg_south_a < 180.0,
            "south.0 bearing: {brg_south_a}"
        );
    }
}

#[cfg(test)]
mod pick_tests {
    use super::*;
    use crate::render::mercator::Camera;
    use glam::Vec4;

    /// Forward transform mirroring `radar_observed.wgsl`'s `beam_world`, in the same local
    /// pixel/metre-preserving coordinate system [`pick_observed_tilt`] works in. Used only to
    /// build a known point for the round-trip tests below — [`pick_observed_tilt`] must recover
    /// exactly what this places.
    #[allow(clippy::too_many_arguments)]
    fn beam_world_local(
        camera: &Camera,
        radar_lon: f64,
        radar_lat: f64,
        antenna_altitude_m: f64,
        vertical_exaggeration: f64,
        beam_rise: f64,
        bearing_deg: f64,
        ground_km: f64,
        elevation_deg: f32,
    ) -> Vec3 {
        let [lon, lat] =
            crate::geo::destination_point([radar_lon, radar_lat], bearing_deg, ground_km);
        let world = crate::render::mercator::lonlat_to_world(lon, lat);
        let wpp = camera.world_per_pixel();
        let mut dx = world.0 - camera.center.0;
        dx -= (dx + 0.5).floor(); // wrap to (-0.5, 0.5], matching `beam_world`'s own wrap
        let dy = world.1 - camera.center.1;
        let metres_to_px = Camera::world_units_per_metre(radar_lat) / wpp;
        let altitude_m = antenna_altitude_m
            + tilt_altitude_m(ground_km, elevation_deg, vertical_exaggeration, beam_rise);
        Vec3::new(
            (dx / wpp) as f32,
            (-dy / wpp) as f32,
            (altitude_m * metres_to_px) as f32,
        )
    }

    /// Where `beam_world_local`'s point actually lands on screen — the same clip-space ->
    /// viewport-pixel conversion `Camera::world_to_screen`'s 3D branch does, generalized to a
    /// point that isn't pinned to the ground (`z = 0`) the way a map label always is.
    fn project(camera: &Camera, viewport_px: (f32, f32), point: Vec3) -> Option<(f32, f32)> {
        let clip = camera.view_projection(viewport_px) * Vec4::new(point.x, point.y, point.z, 1.0);
        if clip.w <= f32::EPSILON {
            return None; // behind the camera
        }
        let ndc = clip.truncate() / clip.w;
        Some((
            (ndc.x + 1.0) * viewport_px.0 * 0.5,
            (1.0 - ndc.y) * viewport_px.1 * 0.5,
        ))
    }

    fn test_camera() -> Camera {
        // Zoom 8 (a typical *2D* metro-area zoom) puts the 3D camera's own altitude at ~360 km
        // real-world — the tilts under test differ by a few km at most, so that's most of a
        // scene's depth spent on a needle nobody asked to find. Zoom 12 is closer to what a
        // pitched storm-scale view actually uses, bringing the camera down to ~20 km: still well
        // above any tilt, but by a margin the geometry can resolve cleanly.
        let mut cam = Camera::at_lonlat(-97.5, 35.3, 12.0);
        cam.pitch = 45.0;
        cam.bearing = 0.0;
        cam
    }

    #[test]
    fn an_echo_top_surface_covers_only_the_tops_and_rises_with_them() {
        let mut cam = Camera::at_lonlat(-97.5, 35.3, 8.0);
        cam.pitch = 50.0;
        let vp = (1000.0, 700.0);
        // 10 x 10 cells over a degree: tops of 12 km in the west half, none in the east half.
        let grid = wxdata::mrms::MrmsField {
            values: (0..100)
                .map(|i| if i % 10 < 5 { 12.0 } else { 0.0 })
                .collect(),
            nx: 10,
            ny: 10,
            lon_west: -98.0,
            lon_east: -97.0,
            lat_north: 35.8,
            lat_south: 34.8,
            time: chrono::DateTime::UNIX_EPOCH,
        };
        let draw = |grid: &wxdata::mrms::MrmsField| {
            super::height_surface_screen(
                &cam,
                vp,
                egui::Pos2::ZERO,
                grid,
                [-98.0, 34.8, -97.0, 35.8],
                20,
                1.0,
                0.5,
                |_| Some([200, 200, 200]),
            )
        };
        let (mesh, lines) = draw(&grid);
        assert!(!mesh.vertices.is_empty() && !lines.is_empty());
        // Every vertex sits in the west half.
        let (east_x, _) =
            cam.world_to_screen(crate::render::mercator::lonlat_to_world(-97.45, 35.3), vp);
        assert!(mesh.vertices.iter().all(|v| v.pos.x < east_x + 5.0));
        // Twice the height sits higher on screen.
        let tall = wxdata::mrms::MrmsField {
            values: grid.values.iter().map(|v| v * 2.0).collect(),
            ..grid.clone()
        };
        let (taller, _) = draw(&tall);
        let mean_y = |m: &egui::Mesh| {
            m.vertices.iter().map(|v| v.pos.y).sum::<f32>() / m.vertices.len() as f32
        };
        assert!(mean_y(&taller) < mean_y(&mesh));
    }

    #[test]
    fn height_ruler_rises_up_the_screen_and_grows_with_exaggeration() {
        let mut cam = Camera::at_lonlat(-97.5, 35.3, 8.0);
        cam.pitch = 50.0;
        let vp = (1200.0, 800.0);
        let r1 = super::height_ruler(&cam, vp, -97.5, 35.3, 1.0, 16.0, 2.0);
        assert_eq!(r1.len(), 9);
        assert_eq!(r1[0].0, 0.0);
        assert!(r1.windows(2).all(|w| w[1].1 .1 < w[0].1 .1), "{r1:?}");
        let r3 = super::height_ruler(&cam, vp, -97.5, 35.3, 3.0, 16.0, 2.0);
        let len = |r: &[(f64, (f32, f32))]| r[0].1 .1 - r.last().unwrap().1 .1;
        assert!(len(&r3) > len(&r1) * 2.0);
    }

    #[test]
    fn beam_guides_draw_each_distinct_tilt_once_and_the_beam_between_its_edges() {
        let mut cam = Camera::at_lonlat(-97.5, 35.3, 7.0);
        cam.pitch = 45.0;
        let vp = (1200.0, 800.0);
        // A SAILS re-scan repeats 0.5°: three angles, two distinct.
        let g = super::beam_guides(
            &cam,
            vp,
            -97.28,
            35.33,
            370.0,
            400.0,
            1.0,
            1.0,
            &[0.5, 1.5, 0.48],
            0.0,
            230.0,
        );
        let tilts: std::collections::BTreeSet<usize> = g.rings.iter().map(|(t, _)| *t).collect();
        assert_eq!(tilts.len(), 2, "{tilts:?}");
        assert!(g.mast.is_some());
        // Lowest and highest: a centreline and two edges each.
        assert_eq!(g.beams.iter().filter(|(edge, _)| !edge).count(), 2);
        assert_eq!(g.beams.iter().filter(|(edge, _)| *edge).count(), 4);
        // At the far end, a beam that rises sits higher on screen (smaller y) than the one below.
        let tip = |i: usize| *g.beams[i].1.last().unwrap();
        let (lower_edge, upper_edge) = (tip(1), tip(2));
        assert!(
            upper_edge.1 < lower_edge.1,
            "{upper_edge:?} above {lower_edge:?}"
        );
    }

    /// `beam_rise` exists to stop distant gates flaring upward, so what it has to do is take more
    /// height off at long range than at short — proportionally on every gate, which in absolute
    /// metres is exactly "more, further out". Checked as a ratio at two ranges rather than as two
    /// magic numbers, so the assertion stays true if the beam model itself is ever refined.
    #[test]
    fn lowering_beam_rise_pulls_distant_gates_down_further_than_near_ones() {
        let (elev, ve) = (0.5f32, 1.0);
        let drop_at = |ground_km: f64| {
            tilt_altitude_m(ground_km, elev, ve, 1.0) - tilt_altitude_m(ground_km, elev, ve, 0.5)
        };
        let near = drop_at(20.0);
        let far = drop_at(200.0);
        assert!(near > 0.0, "halving the rise must lower a near gate at all");
        assert!(
            far > near * 5.0,
            "a distant gate has to come down far more than a near one: {far} vs {near}"
        );
    }

    /// The two ends of the control are the part a user actually reaches for: 100% has to be the
    /// untouched geometry every other test here already pins, and 0% has to be genuinely flat —
    /// every tilt at every range sitting on the antenna's own altitude, which is what "lay it out
    /// like the 2D view" means.
    #[test]
    fn full_beam_rise_is_true_geometry_and_zero_is_flat() {
        for elevation in [0.5f32, 4.0, 19.5] {
            for ground_km in [5.0, 60.0, 200.0] {
                let truth = wxdata::xsection::beam_height_km(
                    wxdata::xsection::slant_from_ground_km(ground_km, elevation as f64),
                    elevation as f64,
                ) * 1_000.0;
                let full = tilt_altitude_m(ground_km, elevation, 1.0, 1.0);
                assert!(
                    (full - truth).abs() < 1e-6,
                    "100% must be the real beam height at {elevation}° / {ground_km} km: \
                     {full} vs {truth}"
                );
                assert_eq!(
                    tilt_altitude_m(ground_km, elevation, 1.0, 0.0),
                    0.0,
                    "0% must be flat at {elevation}° / {ground_km} km"
                );
            }
        }
    }

    /// Clicking has to keep landing on the tilt the user can see, which only holds if the pick
    /// geometry is driven by the same `beam_rise` the shader drew with — the CPU/GPU lock-step
    /// `tilt_altitude_m`'s own doc comment calls for. A pick that still assumed true geometry
    /// would miss by kilometres at a reduced rise, which is exactly the regression this catches.
    #[test]
    fn picking_follows_a_reduced_beam_rise_rather_than_true_geometry() {
        let camera = test_camera();
        let viewport = (800.0, 600.0);
        let (radar_lon, radar_lat) = (-97.5, 35.3);
        let (antenna_altitude_m, vertical_exaggeration) = (400.0, 3.0);
        let beam_rise = 0.35;
        let elevations = [1.8f32];
        let (bearing_deg, ground_km) = (0.0, 40.0);

        let point = beam_world_local(
            &camera,
            radar_lon,
            radar_lat,
            antenna_altitude_m,
            vertical_exaggeration,
            beam_rise,
            bearing_deg,
            ground_km,
            elevations[0],
        );
        let px = project(&camera, viewport, point).expect("point should be in front of the camera");

        let (picked_tilt, lon, lat) = pick_observed_tilt(
            &camera,
            px,
            viewport,
            radar_lon,
            radar_lat,
            antenna_altitude_m,
            vertical_exaggeration,
            beam_rise,
            &elevations,
        )
        .expect("the ray should cross the flattened tilt's beam surface");

        assert_eq!(picked_tilt, 0);
        let [expect_lon, expect_lat] =
            crate::geo::destination_point([radar_lon, radar_lat], bearing_deg, ground_km);
        let (drift_km, _) = crate::geo::great_circle([expect_lon, expect_lat], [lon, lat]);
        assert!(drift_km < 1.0, "picked {drift_km} km from the placed point");
    }

    /// The whole point of `pick_observed_tilt`: given the exact screen pixel a known point on one
    /// specific tilt's beam surface projects to, it has to recover *that* tilt (not whichever one
    /// happened to be selected for the flat 2D view) and a ground position close to the real one.
    #[test]
    fn recovers_the_tilt_and_ground_position_a_known_point_was_placed_at() {
        let camera = test_camera();
        let viewport = (800.0, 600.0);
        let (radar_lon, radar_lat) = (-97.5, 35.3);
        let (antenna_altitude_m, vertical_exaggeration) = (400.0, 3.0);
        let elevations = [1.8f32];
        let (bearing_deg, ground_km, tilt_idx) = (0.0, 40.0, 0);

        let point = beam_world_local(
            &camera,
            radar_lon,
            radar_lat,
            antenna_altitude_m,
            vertical_exaggeration,
            1.0,
            bearing_deg,
            ground_km,
            elevations[tilt_idx],
        );
        let px = project(&camera, viewport, point).expect("point should be in front of the camera");

        let (picked_tilt, lon, lat) = pick_observed_tilt(
            &camera,
            px,
            viewport,
            radar_lon,
            radar_lat,
            antenna_altitude_m,
            vertical_exaggeration,
            1.0,
            &elevations,
        )
        .expect("the ray should cross the known tilt's beam surface");

        assert_eq!(picked_tilt, tilt_idx);
        let [expect_lon, expect_lat] =
            crate::geo::destination_point([radar_lon, radar_lat], bearing_deg, ground_km);
        let (drift_km, _) = crate::geo::great_circle([expect_lon, expect_lat], [lon, lat]);
        assert!(drift_km < 0.5, "picked ({lon}, {lat}), expected near ({expect_lon}, {expect_lat}), drift {drift_km} km");
    }

    /// A steeper tilt's beam is *always* higher than a shallower one's at the same ground range —
    /// its height grows faster with range from the same antenna, and the two curves only meet at
    /// range zero — so a ray dropping straight down onto a point on the shallow tilt's cone
    /// necessarily passes through the steep tilt's cone first. That is exactly what a z-buffered
    /// render would show (the steep tilt's gates in front, hiding the shallow ones behind them,
    /// unless one layer is pulled clear of the stack — `radar_observed.wgsl`'s `pull_m`, not
    /// modeled here) — so the crossing nearer the camera has to win regardless of which tilt a
    /// caller was aiming for. This is the property the "nearest crossing wins" logic exists for;
    /// pinning it down here means a future change that broke it (e.g. always preferring the first
    /// tilt in the list) would fail loudly rather than merely stop matching the real render.
    #[test]
    fn when_two_tilts_cross_the_same_ray_the_one_nearer_the_camera_wins() {
        let (radar_lon, radar_lat) = (-97.5, 35.3);
        let (antenna_altitude_m, vertical_exaggeration) = (400.0, 3.0);
        let elevations = [0.5f32, 4.5]; // steep (index 1) must win every time below
        let wpp = 1.0e-5; // an arbitrary, realistic-scale world-per-pixel
        let metres_to_px = Camera::world_units_per_metre(radar_lat) / wpp;
        let camera_center = (0.5, 0.5); // arbitrary; only the offset from it matters here

        for ground_km in [3.0, 10.0, 60.0] {
            let shallow_altitude_m = antenna_altitude_m
                + tilt_altitude_m(ground_km, elevations[0], vertical_exaggeration, 1.0);
            let [lon, lat] = crate::geo::destination_point([radar_lon, radar_lat], 0.0, ground_km);
            let world = crate::render::mercator::lonlat_to_world(lon, lat);
            let mut dx = world.0 - camera_center.0;
            dx -= (dx + 0.5).floor();
            let dy = world.1 - camera_center.1;
            let xy = Vec3::new((dx / wpp) as f32, (-dy / wpp) as f32, 0.0);
            // A vertical ray straight down through this ground column, starting 5 km above the
            // shallow tilt's own point — comfortably above both cones at every range tested.
            let near = Vec3::new(
                xy.x,
                xy.y,
                ((shallow_altitude_m + 5_000.0) * metres_to_px) as f32,
            );
            // `t` only ranges 0..=1 (`PICK_T_MAX`), so the direction vector — not just its sign —
            // has to carry the whole descent: a unit vector would move the ray a single local
            // unit over that whole range, nowhere near the ~15 unit drop needed to clear both
            // cones and reach the ground at this `metres_to_px` scale.
            let dir = Vec3::new(0.0, 0.0, -near.z * 2.0);

            let (tilt, ..) = pick_along_ray(
                near,
                dir,
                camera_center,
                wpp,
                radar_lon,
                radar_lat,
                antenna_altitude_m,
                vertical_exaggeration,
                1.0,
                &elevations,
            )
            .expect("a straight drop from above should cross the steep tilt's cone");
            assert_eq!(tilt, 1, "at ground_km={ground_km}");
        }
    }

    /// A click on empty sky above every tilt's beam surface finds nothing — it must not silently
    /// fall back to some default tilt, which would be exactly the bug this function replaces.
    #[test]
    fn a_click_above_every_tilt_finds_nothing() {
        let (radar_lon, radar_lat) = (-97.5, 35.3);
        let (antenna_altitude_m, vertical_exaggeration) = (400.0, 3.0);
        let elevations = [0.5f32, 0.9, 1.8];
        let wpp = 1.0e-5;
        let metres_to_px = Camera::world_units_per_metre(radar_lat) / wpp;
        // 10 km out, where the steepest tilt tested (1.8°) is still only a few hundred metres up
        // — nowhere near this ray, which stays between 20 and 50 km altitude the whole way and
        // never approaches the ground at all.
        let [lon, lat] = crate::geo::destination_point([radar_lon, radar_lat], 0.0, 10.0);
        let camera_center = crate::render::mercator::lonlat_to_world(radar_lon, radar_lat);
        let world = crate::render::mercator::lonlat_to_world(lon, lat);
        let mut dx = world.0 - camera_center.0;
        dx -= (dx + 0.5).floor();
        let dy = world.1 - camera_center.1;
        let near = Vec3::new(
            (dx / wpp) as f32,
            (-dy / wpp) as f32,
            (50_000.0 * metres_to_px) as f32,
        );
        let dir = Vec3::new(0.0, 0.0, (-30_000.0 * metres_to_px) as f32);
        assert!(pick_along_ray(
            near,
            dir,
            camera_center,
            wpp,
            radar_lon,
            radar_lat,
            antenna_altitude_m,
            vertical_exaggeration,
            1.0,
            &elevations,
        )
        .is_none());
    }
}
