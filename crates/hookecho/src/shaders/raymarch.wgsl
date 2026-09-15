// Maximum-intensity-projection raymarch of a 3D reflectivity volume.
//
// A fullscreen triangle casts one ray per pixel from an orbit camera, intersects the volume's
// axis-aligned box, marches it taking the max reflectivity index, and colors that via the
// reflectivity LUT. Empty rays are transparent so the egui window background shows through.
//
// Phase H4: an optional vertical plane at any angle (not just the box's own axes) further clips
// what the march can see, per-sample rather than by tightening the box intersection above — one
// extra comparison per step, and it composes with the axis slab for free.

struct Uniforms {
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    box_min: vec4<f32>,
    box_max: vec4<f32>,
    dims: vec4<f32>, // nx, ny, nz, step_count
    // x: minimum reflectivity index to draw; y: layer opacity.
    ctl: vec4<f32>,
    // Slab bounds as fractions of the full box, so slicing narrows what is marched without
    // changing how a world position maps to a voxel.
    clip_min: vec4<f32>,
    clip_max: vec4<f32>,
    // An additional vertical clip at any angle rather than only the box's own axes: xy is a
    // world-space unit normal, z the signed distance from the origin along it, w whether this is
    // active at all (1.0) or ignored (0.0). With no slab (below), a sample is kept only on the
    // side the normal points toward — see `fs_main`.
    plane: vec4<f32>,
    // x: half-width (world units) of an optional slab straddling `plane`. 0.0 means no slab —
    // `plane` cuts the volume in half as before. > 0.0 keeps only a band of that half-width
    // centered on the plane instead of clipping one whole side away.
    plane_slab: vec4<f32>,
    // CC-anomaly opacity ramp: x the index that draws faintest, y the index that draws solid,
    // z that faintest multiplier, w whether this is active at all. x and y are already in this
    // volume's own index space — for the inverted debris volume x > y — so one signed ratio
    // works without the shader knowing which volume it has.
    cc: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var vol: texture_3d<u32>;
@group(0) @binding(2) var lut: texture_2d<f32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var o: VsOut;
    o.pos = vec4<f32>(p[vid], 0.0, 1.0);
    o.ndc = p[vid];
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Reconstruct the world-space ray from the inverse view-projection.
    let far4 = u.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let far = far4.xyz / far4.w;
    let ro = u.cam_pos.xyz;
    let rd = normalize(far - ro);

    // Slab intersection with the (possibly sliced) volume box.
    let full_span = u.box_max.xyz - u.box_min.xyz;
    let cmin = u.box_min.xyz + u.clip_min.xyz * full_span;
    let cmax = u.box_min.xyz + u.clip_max.xyz * full_span;
    let inv = 1.0 / rd;
    let t0s = (cmin - ro) * inv;
    let t1s = (cmax - ro) * inv;
    let tsmall = min(t0s, t1s);
    let tbig = max(t0s, t1s);
    let tmin = max(max(tsmall.x, tsmall.y), max(tsmall.z, 0.0));
    let tmax = min(min(tbig.x, tbig.y), tbig.z);
    if (tmax <= tmin) {
        discard;
    }

    let steps = i32(u.dims.w);
    // Voxel lookup always uses the full box: slicing must not restretch the texture.
    let span = full_span;
    let dims = vec3<f32>(u.dims.x, u.dims.y, u.dims.z);
    // Below the threshold a voxel is treated as empty, so raising it carves the weak echo away
    // and leaves the cores standing on their own.
    let floor_idx = u32(max(u.ctl.x, 2.0));
    var max_idx: u32 = 0u;
    for (var s = 0; s < steps; s = s + 1) {
        let t = tmin + (tmax - tmin) * (f32(s) + 0.5) / f32(steps);
        let pos = ro + rd * t;
        // A sample outside the plane's kept region is treated as empty for this ray, same as a
        // sample outside the axis-aligned slab above. With no slab thickness that region is a
        // half-space (the side the normal points toward); with one, it's a band of that
        // half-width straddling the plane instead.
        let n_dot = pos.x * u.plane.x + pos.y * u.plane.y;
        // Parenthesized: WGSL's parser treats a bare `<`/`>` in call-argument position as the
        // start of a template-argument list (as in `vec3<f32>`) unless it's wrapped.
        let clipped_by_plane = u.plane.w > 0.5 && select(
            (n_dot < u.plane.z),
            (abs(n_dot - u.plane.z) > u.plane_slab.x),
            u.plane_slab.x > 0.0,
        );
        if (!clipped_by_plane) {
            let uvw = (pos - u.box_min.xyz) / span;
            let voxel = vec3<i32>(clamp(uvw * dims, vec3<f32>(0.0), dims - 1.0));
            let idx = textureLoad(vol, voxel, 0).r;
            if (idx >= floor_idx && idx > max_idx) {
                max_idx = idx;
            }
        }
    }

    if (max_idx < floor_idx) {
        discard;
    }
    let color = textureLoad(lut, vec2<i32>(i32(max_idx), 0), 0);
    var alpha: f32;
    if (u.cc.w > 0.5) {
        // CC anomaly: opacity is a function of how far this voxel's correlation coefficient sits
        // below ordinary meteorological scatter, so the background storm fades out and the
        // low-CC pocket inside it is what stays solid. This *replaces* the generic ramp below
        // rather than scaling it — the two are competing opacity models, and multiplying them
        // drove background CC to invisible instead of to the faint trace that keeps a debris
        // ball legibly embedded in the storm around it.
        let t = clamp((f32(max_idx) - u.cc.x) / (u.cc.y - u.cc.x), 0.0, 1.0);
        // Smoothstep, not a linear ramp: it flattens at both ends, which is what makes the
        // progression read as the intended tiers without banding the volume into hard shells.
        let shaped = t * t * (3.0 - 2.0 * t);
        alpha = (u.cc.z + (1.0 - u.cc.z) * shaped) * u.ctl.y;
    } else {
        // Opacity ramps from the threshold, not from zero: with a 45 dBZ floor the surviving
        // cores read solid instead of uniformly hazy.
        let head = max(255.0 - f32(floor_idx), 1.0);
        alpha = clamp((f32(max_idx) - f32(floor_idx)) / head * 1.6 + 0.15, 0.0, 1.0) * u.ctl.y;
    }
    if (alpha <= 0.0) { discard; }
    return vec4<f32>(color.rgb * alpha, alpha);
}
