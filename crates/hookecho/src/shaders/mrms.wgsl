// MRMS national mosaic layer. A world-space quad covers the CONUS grid's mercator bbox;
// each fragment inverts web-mercator to lon/lat, maps to the regular lat/lon grid (plate-carrée
// → mercator warp), samples the R8 index texture, and colors it through the shared LUT.

const PI: f32 = 3.14159265358979;

struct Camera {
    center: vec2<f32>,
    scale: vec2<f32>,
    world_per_pixel: f32,
    road_scale: f32,
    mode_3d: f32,
    _pad: f32,
    view_proj: mat4x4<f32>,
};

// Grid bounds + dimensions (48 bytes, matching mrms_bgl's min_binding_size).
struct Mrms {
    lon_west: f32,
    lat_north: f32,
    lon_east: f32,
    lat_south: f32,
    nx: f32,
    ny: f32,
    /// Layer opacity 0..1, rewritten per frame from the Layer Manager slider.
    opacity: f32,
    /// 1 = interpolate between grid cells (continuous ramps only; the CPU never sets it for a
    /// categorical layer, where a blend of two class codes is a third, wrong class).
    smoothing: f32,
    _pad2: f32,
    _pad3: f32,
    _pad4: f32,
    _pad5: f32,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> mrms: Mrms;
@group(1) @binding(1) var grid_tex: texture_2d<u32>;
@group(1) @binding(2) var lut_tex: texture_2d<f32>;

struct VsIn { @location(0) world: vec2<f32> };
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec2<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    let p = (in.world - camera.center) * camera.scale;
    if (camera.mode_3d > 0.5) {
        var delta = in.world - camera.center;
        delta.x = delta.x - floor(delta.x + 0.5);
        let local = vec3<f32>(delta.x, -delta.y, 0.0) / camera.world_per_pixel;
        out.clip = camera.view_proj * vec4<f32>(local, 1.0);
    } else {
        out.clip = vec4<f32>(p, 0.0, 1.0);
    }
    out.world = in.world;
    return out;
}

fn world_to_lonlat(w: vec2<f32>) -> vec2<f32> {
    let lon = w.x * 360.0 - 180.0;
    let n = PI * (1.0 - 2.0 * w.y);
    let lat = atan(sinh(n)) * 180.0 / PI;
    return vec2<f32>(lon, lat);
}

// Bilinear blend of the four cells around (fu, fv), in index space (the ramps' indices are linear
// in the ramp position), over the cells that hold a value. Where most of the neighbourhood is
// empty the nearest cell's own index stands, so an echo's edge stays where the data puts it.
fn smoothed_index(fu: f32, fv: f32, nearest: u32) -> u32 {
    let n = vec2<i32>(i32(mrms.nx), i32(mrms.ny));
    let g = vec2<f32>(fu * mrms.nx - 0.5, fv * mrms.ny - 0.5);
    let base = vec2<i32>(floor(g));
    let t = g - floor(g);
    var acc = 0.0;
    var wsum = 0.0;
    for (var dy = 0; dy < 2; dy = dy + 1) {
        for (var dx = 0; dx < 2; dx = dx + 1) {
            let p = clamp(base + vec2<i32>(dx, dy), vec2<i32>(0, 0), n - vec2<i32>(1, 1));
            let r = textureLoad(grid_tex, p, 0).r;
            let w = select(1.0 - t.x, t.x, dx == 1) * select(1.0 - t.y, t.y, dy == 1);
            if (r >= 2u) {
                acc = acc + w * f32(r);
                wsum = wsum + w;
            }
        }
    }
    if (wsum < 0.5) {
        return nearest;
    }
    return u32(round(acc / wsum));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let ll = world_to_lonlat(in.world);
    let fu = (ll.x - mrms.lon_west) / (mrms.lon_east - mrms.lon_west);
    let fv = (mrms.lat_north - ll.y) / (mrms.lat_north - mrms.lat_south);
    if (fu < 0.0 || fu >= 1.0 || fv < 0.0 || fv >= 1.0) { discard; }

    let gx = i32(fu * mrms.nx);
    let gy = i32(fv * mrms.ny);
    var raw = textureLoad(grid_tex, vec2<i32>(gx, gy), 0).r;
    if (mrms.smoothing > 0.5) {
        raw = smoothed_index(fu, fv, raw);
    }

    let color = textureLoad(lut_tex, vec2<i32>(i32(raw), 0), 0);
    if (color.a == 0.0) { discard; }
    return vec4<f32>(color.rgb, color.a * mrms.opacity);
}
