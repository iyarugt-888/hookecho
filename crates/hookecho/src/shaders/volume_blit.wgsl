// Draws a raymarched volume that was rendered offscreen (render3d's `Offscreen`) over the map:
// a full-viewport triangle sampling the stored image, which already holds premultiplied colour.
// The raymarch is redone only when the camera, settings or volume change, and at a reduced
// resolution where the GPU is a phone's; every other frame costs only this copy.

@group(0) @binding(0) var img: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var o: VsOut;
    o.pos = vec4<f32>(p[vid], 0.0, 1.0);
    // NDC y up; texture rows top down.
    o.uv = vec2<f32>(p[vid].x * 0.5 + 0.5, 0.5 - p[vid].y * 0.5);
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(img, samp, in.uv);
}
