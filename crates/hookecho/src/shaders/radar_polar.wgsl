// A map position's range and azimuth from a radar, in single precision that holds on every GPU
// (1010.md V19). The radar's own world position and the sine and cosine of its latitude come
// from the CPU, worked out in double precision. Everything that depends on the fragment is then a
// short series in small differences, plus a square root and one atan2. The earlier haversine,
// written with sin, cos and sinh of the fragment's own latitude and of small differences, was
// within 0.003 km on an RTX 2060 but off by up to 0.93 km and 0.91 degrees on Direct3D's
// software rasterizer (WARP), so the two drew different gates at nearly every pixel
// (`radar_precision_tests`).

const POLAR_PI: f32 = 3.14159265358979;
const POLAR_R_EARTH_KM: f32 = 6371.0;

// sin(x) and cos(x) for |x| up to about 0.2 rad, to f32 precision.
fn sin_small(x: f32) -> f32 {
    let x2 = x * x;
    return x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0)));
}
fn one_minus_cos_small(x: f32) -> f32 {
    let x2 = x * x;
    return x2 / 2.0 * (1.0 - x2 / 12.0 * (1.0 - x2 / 30.0));
}
// asin(x) for x up to about 0.1.
fn asin_small(x: f32) -> f32 {
    let x2 = x * x;
    return x * (1.0 + x2 * (1.0 / 6.0 + x2 * (3.0 / 40.0)));
}

// `(range km, azimuth degrees clockwise from north)` of `world` from a radar at `radar_world`,
// with `s0` and `c0` the sine and cosine of its latitude.
fn radar_range_az(world: vec2<f32>, radar_world: vec2<f32>, s0: f32, c0: f32) -> vec2<f32> {
    var d = world - radar_world;
    d.x = d.x - round(d.x);
    let dlon = d.x * 2.0 * POLAR_PI;
    // Mercator's n = pi (1 - 2y), so the change in n is -2 pi dy; latitude is the
    // Gudermannian of n. Its change from the radar's, by Taylor series in that change h:
    // gd' = c, gd'' = -c s, gd''' = c s^2 - c^3, gd'''' = 5 c^3 s - c s^3 at the radar.
    let h = -2.0 * POLAR_PI * d.y;
    let c2 = c0 * c0;
    let s2 = s0 * s0;
    let dlat = h * c0 * (1.0 + h * (-0.5 * s0 + h * ((s2 - c2) / 6.0
        + h * (5.0 * c2 * s0 - s2 * s0) / 24.0)));
    // cos of the fragment's latitude, from the radar's and the small difference.
    let cos_lat = c0 * (1.0 - one_minus_cos_small(dlat)) - s0 * sin_small(dlat);
    // Haversine.
    let sh = sin_small(dlat * 0.5);
    let sl = sin_small(dlon * 0.5);
    let a = sh * sh + c0 * cos_lat * sl * sl;
    let range_km = 2.0 * POLAR_R_EARTH_KM * asin_small(sqrt(max(a, 0.0)));
    // Initial bearing. Its x, cos(lat0) sin(lat) - sin(lat0) cos(lat) cos(dlon), is a small
    // difference of large terms; the same quantity, written without the cancellation.
    let y = sin_small(dlon) * cos_lat;
    let x = sin_small(dlat) + s0 * cos_lat * one_minus_cos_small(dlon);
    var az = atan2(y, x) * 180.0 / POLAR_PI;
    az = az - floor(az / 360.0) * 360.0;
    return vec2<f32>(range_km, az);
}
