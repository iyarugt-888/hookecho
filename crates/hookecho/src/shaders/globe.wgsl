// Shared by the map shaders (prepended to each at load, see `render::map_shader`): where a
// normalized-mercator world point lands in clip space, for the flat 2D map, the pitched 3D map
// and the globe. Every including shader declares `camera` with the same fields.
//
// The globe: the point on a sphere whose scale at the view's centre matches the flat map's
// (`r = 1 / (2π · world_per_pixel · cos(lat0))`), in the same local pixel frame the pitched map
// uses (+X east, +Y north, +Z up, the centre at the origin), blended with the flat position by
// `camera.globe.x`. The far side of the sphere is pushed past the far plane: the map has no depth
// buffer, so without it the back of the planet would paint over the front. `Camera::ground_local`
// is the CPU mirror.

const GLOBE_PI: f32 = 3.14159265359;
const GLOBE_TAU: f32 = 6.28318530718;

fn globe_lat(y: f32) -> f32 {
    let n = GLOBE_PI * (1.0 - 2.0 * y);
    return atan(0.5 * (exp(n) - exp(-n)));
}

fn map_clip(world: vec2<f32>) -> vec4<f32> {
    if (camera.mode_3d < 0.5) {
        return vec4<f32>((world - camera.center) * camera.scale, 0.0, 1.0);
    }
    var delta = world - camera.center;
    delta.x = delta.x - floor(delta.x + 0.5);
    let flat_pos = vec3<f32>(delta.x, -delta.y, 0.0) / camera.world_per_pixel;
    let t = camera.globe.x;
    if (t <= 0.0) {
        return camera.view_proj * vec4<f32>(flat_pos, 1.0);
    }
    let lat0 = globe_lat(camera.center.y);
    let lat = globe_lat(world.y);
    let dl = delta.x * GLOBE_TAU;
    let n = vec3<f32>(
        cos(lat) * sin(dl),
        sin(lat) * cos(lat0) - cos(lat) * sin(lat0) * cos(dl),
        sin(lat) * sin(lat0) + cos(lat) * cos(lat0) * cos(dl),
    );
    let r = 1.0 / (GLOBE_TAU * camera.world_per_pixel * max(cos(lat0), 0.01));
    let on_sphere = n * r - vec3<f32>(0.0, 0.0, r);
    var clip = camera.view_proj * vec4<f32>(mix(flat_pos, on_sphere, t), 1.0);
    // Facing the eye: z inside the clip range; facing away: past the far plane, clipped per
    // fragment along the horizon.
    let facing = dot(n, normalize(camera.globe.yzw - on_sphere));
    clip.z = clip.w * (1.0 - 0.999 * facing);
    return clip;
}
