// Instanced observed Level II gates in geographic 3D. Each instance is one real gate; the six
// generated vertices make its azimuth/range footprint. Camera motion changes only group 0.

const PI: f32 = 3.14159265358979;
const EARTH_RADIUS_M: f32 = 6371000.0;
const EFFECTIVE_RADIUS_M: f32 = EARTH_RADIUS_M * 4.0 / 3.0;

struct Camera {
    center: vec2<f32>,
    scale: vec2<f32>,
    world_per_pixel: f32,
    road_scale: f32,
    mode_3d: f32,
    _pad: f32,
    view_proj: mat4x4<f32>,
};

struct Radar3d {
    radar_lat: f32,
    radar_lon: f32,
    antenna_altitude_m: f32,
    vertical_exaggeration: f32,
    opacity: f32,
    threshold_idx: f32,
    world_units_per_metre: f32,
    srv: f32,
    motion_e: f32,
    motion_n: f32,
    // Elevation angle of the volume's lowest tilt carrying this moment — see `beam_world`'s doc
    // comment for what it's used for.
    min_elevation_deg: f32,
    // The tilt the user clicked in the Layers list, or a large negative sentinel for "none" —
    // see `beam_world` and `fs_main` for what it does to that tilt's gates.
    highlight_elev: f32,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> radar: Radar3d;
@group(1) @binding(1) var lut_tex: texture_2d<f32>;

struct Gate {
    @location(0) polar: vec4<f32>, // azimuth, beam width, slant start, slant span (km)
    @location(1) data: vec4<f32>,  // elevation, palette index, original gate number, spare
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) value_idx: f32,
    @location(1) @interpolate(flat) azimuth: f32,
    @location(2) @interpolate(flat) elevation: f32,
};

// 4/3-earth beam height above the radar (m) at slant range `r` (m) and elevation `e` (rad).
fn beam_height_m(r: f32, e: f32) -> f32 {
    return sqrt(r * r + EFFECTIVE_RADIUS_M * EFFECTIVE_RADIUS_M
        + 2.0 * r * EFFECTIVE_RADIUS_M * sin(e)) - EFFECTIVE_RADIUS_M;
}

// Slant range (m) at which an `e`-radian beam passes over ground range `ground` (m) — the same
// closed form as `xsection::slant_from_ground_km`, in metres/radians instead of km/degrees.
fn slant_for_ground_m(ground: f32, e: f32) -> f32 {
    let theta = ground / EFFECTIVE_RADIUS_M;
    let denom = cos(e + theta);
    if (abs(denom) < 1e-6) {
        return ground;
    }
    return EFFECTIVE_RADIUS_M * sin(theta) / denom;
}

fn beam_world(azimuth_deg: f32, slant_km: f32, elevation_deg: f32) -> vec3<f32> {
    let r = max(slant_km, 0.0) * 1000.0;
    let e = elevation_deg * PI / 180.0;
    let height = beam_height_m(r, e);
    let central = atan2(r * cos(e), EFFECTIVE_RADIUS_M + r * sin(e));
    let ground = EFFECTIVE_RADIUS_M * central;

    // Every tilt's beam climbs with range from earth curvature alone — the lowest tilt is no
    // exception, so its own height at this same ground range is a floor the storm sits on, not
    // storm structure. Only how far above that floor this particular gate sits is genuine
    // structure (a higher tilt cutting through a taller core, a lofted layer), and only that part
    // scales with vertical exaggeration; the floor itself stays true to scale so a low-tilt base
    // scan never visibly lifts off the ground just because Vertical is turned up.
    let min_e = radar.min_elevation_deg * PI / 180.0;
    let base_height = beam_height_m(slant_for_ground_m(ground, min_e), min_e);
    let structure_height = max(height - base_height, 0.0);

    let phi1 = radar.radar_lat * PI / 180.0;
    let lambda1 = radar.radar_lon * PI / 180.0;
    let bearing = azimuth_deg * PI / 180.0;
    let delta = ground / EARTH_RADIUS_M;
    let phi2 = asin(clamp(sin(phi1) * cos(delta)
        + cos(phi1) * sin(delta) * cos(bearing), -1.0, 1.0));
    let lambda2 = lambda1 + atan2(
        sin(bearing) * sin(delta) * cos(phi1),
        cos(delta) - sin(phi1) * sin(phi2),
    );
    let world = vec2<f32>(
        lambda2 / (2.0 * PI) + 0.5,
        0.5 - log((1.0 + sin(phi2)) / (1.0 - sin(phi2))) / (4.0 * PI),
    );
    var d = world - camera.center;
    d.x = d.x - floor(d.x + 0.5);
    // Pull the selected layer clear of the stack so it reads as pulled out rather than merely
    // recolored — 600 m is well clear of the fill-gap midpoint copies and of the next real tilt
    // at any range the "Layers" list is likely to be used at.
    let selected = radar.highlight_elev > -900.0 && abs(elevation_deg - radar.highlight_elev) < 0.05;
    let pull_m = select(0.0, 600.0, selected);
    let altitude = radar.antenna_altitude_m + base_height
        + structure_height * radar.vertical_exaggeration + pull_m;
    return vec3<f32>(
        d.x / camera.world_per_pixel,
        -d.y / camera.world_per_pixel,
        altitude * radar.world_units_per_metre / camera.world_per_pixel,
    );
}

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, gate: Gate) -> VsOut {
    let corner = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, 0.0), vec2<f32>(0.5, 0.0), vec2<f32>(0.5, 1.0),
        vec2<f32>(-0.5, 0.0), vec2<f32>(0.5, 1.0), vec2<f32>(-0.5, 1.0),
    )[vertex];
    let az = gate.polar.x + corner.x * gate.polar.y;
    let range = gate.polar.z + corner.y * gate.polar.w;
    var out: VsOut;
    out.clip = camera.view_proj * vec4<f32>(beam_world(az, range, gate.data.x), 1.0);
    out.value_idx = gate.data.y;
    out.azimuth = gate.polar.x;
    out.elevation = gate.data.x;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var idx = in.value_idx;
    if (radar.srv > 0.5) {
        let az = in.azimuth * PI / 180.0;
        idx = clamp(idx - radar.motion_e * sin(az) - radar.motion_n * cos(az), 2.0, 255.0);
    }
    if (idx < radar.threshold_idx) { discard; }
    let color = textureLoad(lut_tex, vec2<i32>(i32(round(idx)), 0), 0);
    // Each tilt fades a little more than the one below it — from full strength at the lowest
    // (0.5°) scan down to about a third at the volume's highest (VCP tips top out at 19.5°) — so
    // the stack reads as receding layers instead of one flat wall of paint. Pure per-tilt fade,
    // not a function of range or height: two tilts really do share one transparency everywhere
    // along their sweep, only the sweep itself changes what shows through.
    let height_fade = mix(1.0, 0.35, clamp(in.elevation / 19.5, 0.0, 1.0));
    var alpha = color.a * radar.opacity * height_fade;
    // A layer is selected in the "Layers" list: fade every other tilt into the background so the
    // one pulled out in `beam_world` also reads as the one actually being looked at.
    if (radar.highlight_elev > -900.0 && abs(in.elevation - radar.highlight_elev) >= 0.05) {
        alpha = alpha * 0.12;
    }
    if (alpha <= 0.0) { discard; }
    return vec4<f32>(color.rgb, alpha);
}
