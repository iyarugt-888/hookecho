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
    // How much of each tilt's climb above the antenna to actually draw, 0..=1. See `beam_world`.
    // Occupies the slot the volume's lowest-tilt elevation used to hold: that value stopped being
    // read once the floor built from it was replaced, and was kept only so this buffer's byte
    // layout (and the 16-byte alignment `_pad` below exists to satisfy) would not have to be
    // renegotiated. Reusing it keeps that layout untouched for a field that is read again.
    beam_rise: f32,
    // CC-anomaly opacity ramp: the palette index that draws faintest, the one that draws solid,
    // that faintest multiplier, and whether this is active at all. Off (all zero) for every
    // moment but correlation coefficient — see `fs_main`.
    cc_clear_idx: f32,
    cc_full_idx: f32,
    cc_faintest: f32,
    cc_on: f32,
    // Up to 8 tilts pulled out from the Layers list, or the sentinel (`NO_HIGHLIGHT`) in an
    // unused slot — see `beam_world` and `fs_main` for what it does to a gate on one of them.
    // Loose scalars rather than `array<f32,8>`: WGSL pads a real array to a 16-byte stride in the
    // uniform address space, which would needlessly balloon this buffer for no benefit here.
    highlight_0: f32,
    highlight_1: f32,
    highlight_2: f32,
    highlight_3: f32,
    highlight_4: f32,
    highlight_5: f32,
    highlight_6: f32,
    highlight_7: f32,
    // Pads the struct to 96 bytes. Some downlevel backends (mobile GLES via ANGLE, seen live on
    // Android Chrome) refuse a uniform buffer binding whose declared type isn't a multiple of 16
    // bytes — 23 scalar f32 fields is 92, one short. Desktop Vulkan/Metal/DX12 never enforced
    // this, which is how an unpadded version once shipped without the mismatch showing up here.
    _pad: f32,
};

const NO_HIGHLIGHT: f32 = -900.0;

fn highlight_slots() -> array<f32, 8> {
    return array<f32, 8>(
        radar.highlight_0, radar.highlight_1, radar.highlight_2, radar.highlight_3,
        radar.highlight_4, radar.highlight_5, radar.highlight_6, radar.highlight_7,
    );
}

// True when any tilt is currently pulled out from the Layers list at all.
fn any_highlighted() -> bool {
    let slots = highlight_slots();
    for (var i = 0; i < 8; i++) {
        if (slots[i] > NO_HIGHLIGHT) {
            return true;
        }
    }
    return false;
}

// True when `elevation_deg` is one of the tilts pulled out from the Layers list.
fn is_highlighted(elevation_deg: f32) -> bool {
    let slots = highlight_slots();
    for (var i = 0; i < 8; i++) {
        if (slots[i] > NO_HIGHLIGHT && abs(elevation_deg - slots[i]) < 0.05) {
            return true;
        }
    }
    return false;
}

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

fn beam_world(azimuth_deg: f32, slant_km: f32, elevation_deg: f32) -> vec3<f32> {
    let r = max(slant_km, 0.0) * 1000.0;
    let e = elevation_deg * PI / 180.0;
    let height = beam_height_m(r, e);
    let central = atan2(r * cos(e), EFFECTIVE_RADIUS_M + r * sin(e));
    let ground = EFFECTIVE_RADIUS_M * central;

    // Exaggerating the whole beam height (the original behaviour) multiplies the earth-curvature
    // climb every tilt shares, which visibly lifted the lowest tilt off the ground at long range
    // — that climb is the radar's coverage floor, not storm structure. A first fix subtracted the
    // *lowest* tilt's own height at this same ground range as a floor, but that went too far the
    // other way: near the radar a *steep* tilt's own elevation angle alone already puts it high
    // up (19.5 degrees is ~1.7 km up at just 5 km range, no curvature involved at all), and
    // subtracting a floor built from a much shallower tilt barely reduced that — exaggeration
    // multiplied nearly this gate's whole natural height, and ordinary nearby echo on a steep
    // tilt shot into the sky.
    //
    // Split this gate's own height instead: the flat-earth angle rise it would have even with no
    // curvature at all (`r * sin(e)` — true at any range for any tilt, and already large near the
    // radar for a steep one on its own), and the remainder, which is what earth curvature adds on
    // top. Only the remainder scales with vertical exaggeration; the angle term is this tilt's
    // honest, unexaggerated geometry and never does, at any range.
    let angle_height = r * sin(e);
    let curvature_height = height - angle_height;

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
    // Pull every selected layer clear of the stack so each reads as pulled out rather than merely
    // recolored — 600 m is well clear of the fill-gap midpoint copies and of the next real tilt
    // at any range the "Layers" list is likely to be used at.
    let pull_m = select(0.0, 600.0, is_highlighted(elevation_deg));
    // `beam_rise` scales the whole climb above the antenna, not just one half of it: a beam rises
    // with range by geometry, which at long range flares every tilt steeply upward and turns a
    // multi-tilt volume into a stack of cones. Pulling the rise down is proportional, so it takes
    // the most off exactly where the rise is largest — the far end — and 0 lays the sweeps flat.
    // The highlight pull is deliberately outside it: that offset exists to separate a selected
    // layer from its neighbours and has to keep working at any `beam_rise`.
    let rise = (angle_height + curvature_height * radar.vertical_exaggeration) * radar.beam_rise;
    let altitude = radar.antenna_altitude_m + rise + pull_m;
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
    // CC anomaly: for correlation coefficient the interesting gates are the *low* ones — lofted
    // debris, clutter, biological targets — and a floor that hides everything below a cut keeps
    // exactly the uniform high-CC rain nobody opened a CC volume to look at. Opacity instead
    // falls off as CC approaches ordinary meteorological values, so the background thins to a
    // trace and the anomaly is what's left standing. Multiplied here, unlike the raymarch's
    // version: `color.a` is the palette's own alpha and `height_fade` is the per-tilt depth cue,
    // and both remain meaningful independently of how anomalous this gate is.
    if (radar.cc_on > 0.5) {
        let t = clamp((idx - radar.cc_clear_idx) / (radar.cc_full_idx - radar.cc_clear_idx),
                      0.0, 1.0);
        alpha = alpha * (radar.cc_faintest + (1.0 - radar.cc_faintest) * t * t * (3.0 - 2.0 * t));
    }
    // One or more layers are selected in the "Layers" list: fade every other tilt into the
    // background so the ones pulled out in `beam_world` also read as what's actually being
    // looked at.
    if (any_highlighted() && !is_highlighted(in.elevation)) {
        alpha = alpha * 0.12;
    }
    if (alpha <= 0.0) { discard; }
    return vec4<f32>(color.rgb, alpha);
}
