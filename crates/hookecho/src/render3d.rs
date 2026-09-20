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
    ctl: [f32; 4],  // minimum reflectivity index to draw; rest spare
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
    pub top_km: f32,
    /// Share (0..=1) of the scan's echo that lies beyond `half_km` and is therefore not in the
    /// volume. Shown in the UI so a cropped box is never mistaken for the whole scan.
    pub outside: f32,
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
}

impl Default for View3d {
    fn default() -> Self {
        Self {
            threshold_idx: 2.0,
            clip: [0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            plane: None,
            cc: [0.0; 4],
            cappi_km: None,
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
/// `top_km` is the volume's own vertical span (`wxdata::volume3d::Volume3d::top_km`), needed only
/// to place `v3.cappi_km`'s reference plane at the right fraction of the fixed orbit box.
#[allow(clippy::too_many_arguments)]
pub fn orbit_uniform(
    az_deg: f32,
    el_deg: f32,
    dist: f32,
    aspect: f32,
    n: u32,
    nz: u32,
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
        ctl: [v3.threshold_idx, 1.0, 0.0, 0.0],
        clip_min: [v3.clip[0], v3.clip[2], v3.clip[4], 0.0],
        clip_max: [v3.clip[1], v3.clip[3], v3.clip[5], 0.0],
        plane: plane_uniform(v3.plane, BOX_MIN, BOX_MAX),
        plane_slab: plane_slab_uniform(v3.plane, BOX_MIN, BOX_MAX),
        cc: v3.cc,
        cappi_marker: cappi_marker_uniform(v3.cappi_km, top_km, BOX_MIN, BOX_MAX),
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
    let z0 = antenna_altitude_m as f64 * metres_to_px * vertical_exaggeration as f64;
    let z1 = (antenna_altitude_m as f64 + upload.top_km as f64 * 1_000.0)
        * metres_to_px
        * vertical_exaggeration as f64;
    let box_min = Vec3::new(
        (dx / wpp - half_px) as f32,
        (dy / wpp - half_px) as f32,
        z0 as f32,
    );
    let box_max = Vec3::new(
        (dx / wpp + half_px) as f32,
        (dy / wpp + half_px) as f32,
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
            0.0,
        ],
        clip_min: [view.clip[0], view.clip[2], view.clip[4], 0.0],
        clip_max: [view.clip[1], view.clip[3], view.clip[5], 0.0],
        plane: plane_uniform(view.plane, box_min, box_max),
        plane_slab: plane_slab_uniform(view.plane, box_min, box_max),
        cc: view.cc,
        cappi_marker: cappi_marker_uniform(view.cappi_km, upload.top_km, box_min, box_max),
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
    _tex: wgpu::Texture,
    _lut: wgpu::Texture,
    bind_group: wgpu::BindGroup,
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

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, up: &Volume3dUpload) {
        // Every route to a 3D draw goes through here (the egui callback and `render_once`), so
        // this is the one place the pipeline has to exist by.
        self.ensure_pipeline(device);
        let size = wgpu::Extent3d {
            width: up.n,
            height: up.n,
            depth_or_array_layers: up.nz,
        };
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("volume3d_tex"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rg8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
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
        let lut_size = wgpu::Extent3d {
            width: 256,
            height: 1,
            depth_or_array_layers: 1,
        };
        let lut = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("volume3d_lut"),
            size: lut_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &lut,
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
            lut_size,
        );
        let tex_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let lut_view = lut.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = volume_sampler(device);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("raymarch_bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buf.as_entire_binding(),
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
        self.gpu = Some(Gpu {
            _tex: tex,
            _lut: lut,
            bind_group,
        });
    }

    fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let (Some(gpu), Some(pipeline)) = (&self.gpu, &self.pipeline) {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.draw(0..3, 0..1);
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
}

impl egui_wgpu::CallbackTrait for Volume3dCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
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
            queue.write_buffer(&res.uniform_buf, 0, bytemuck::bytes_of(&self.uniform));
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
            res.record(pass);
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
        let size = wgpu::Extent3d {
            width: up.n,
            height: up.n,
            depth_or_array_layers: up.nz,
        };
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("map_volume3d_tex"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rg8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
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
        let lut_size = wgpu::Extent3d {
            width: 256,
            height: 1,
            depth_or_array_layers: 1,
        };
        let lut = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("map_volume3d_lut"),
            size: lut_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &lut,
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
            lut_size,
        );
        let tex_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let lut_view = lut.create_view(&wgpu::TextureViewDescriptor::default());
        let uniform_buf = self.uniform_bufs[pane]
            .as_ref()
            .expect("just created above");
        let sampler = volume_sampler(device);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map_raymarch_bg"),
            layout: &self.bgl,
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
        self.panes[pane] = Some(Gpu {
            _tex: tex,
            _lut: lut,
            bind_group,
        });
    }

    fn set_uniform_for_pane(&mut self, pane: usize, queue: &wgpu::Queue, uniform: Uniforms) {
        if let Some(buf) = &self.uniform_bufs[pane] {
            queue.write_buffer(buf, 0, bytemuck::bytes_of(&uniform));
        }
    }

    fn record_for_pane(&self, pane: usize, pass: &mut wgpu::RenderPass<'_>) {
        if let (Some(gpu), Some(pipeline)) = (&self.panes[pane], &self.pipeline) {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

/// Per-frame draw for one pane's map-pitch smooth volume: which pane (indexes
/// [`MapVolume3dResources`]'s fixed-size pane array), an optional new volume upload, and this
/// frame's camera/radar uniforms (built by [`map_uniform`]).
pub struct MapVolume3dCallback {
    pub pane: u32,
    pub upload: Option<Volume3dUpload>,
    pub uniform: Uniforms,
}

impl egui_wgpu::CallbackTrait for MapVolume3dCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if resources.get::<MapVolume3dResources>().is_none() {
            if let Some(Volume3dFormat(fmt)) = resources.get::<Volume3dFormat>().copied() {
                resources.insert(MapVolume3dResources::new(device, fmt));
            }
        }
        if let Some(res) = resources.get_mut::<MapVolume3dResources>() {
            let pane = self.pane as usize;
            if let Some(up) = &self.upload {
                res.upload_for_pane(pane, device, queue, up);
            }
            res.set_uniform_for_pane(pane, queue, self.uniform);
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
