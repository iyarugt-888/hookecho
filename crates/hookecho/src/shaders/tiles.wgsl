// Textured slippy-map tile quad. Vertices carry normalized-mercator world coords;
// the camera uniform maps them to clip space.

struct Camera {
    center: vec2<f32>,
    scale: vec2<f32>,
    world_per_pixel: f32,
    road_scale: f32,
    mode_3d: f32,
    _pad: f32,
    view_proj: mat4x4<f32>,
    // Globe: x the blend toward it (0 flat), yzw the eye in the local pixel frame.
    globe: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var tile_tex: texture_2d<f32>;
@group(1) @binding(1) var tile_smp: sampler;

struct VsIn {
    @location(0) world: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = map_clip(in.world);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(tile_tex, tile_smp, in.uv);
}
