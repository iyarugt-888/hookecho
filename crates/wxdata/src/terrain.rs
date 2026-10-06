//! Ground elevation from the public AWS Terrain Tiles (ROADMAP_PARITY M3.6's "terrain AGL when
//! available"): 256×256 Web Mercator PNG tiles in the Terrarium encoding, metres above mean sea
//! level as `R·256 + G + B/256 − 32768`, compiled by Mapzen from SRTM, the USGS NED/3DEP and other
//! public sources (registry.opendata.aws/terrain-tiles). No key is needed.
//!
//! Tiles are read at [`ZOOM`] — about 76 m per pixel at the equator, 60 m at 38° — and sampled
//! bilinearly. That is ground height on a grid, not a survey: a radar on a mountaintop or a tower
//! sits above the smoothed cell, and the stated resolution goes with every answer. Height above
//! ground is a beam's MSL height minus this; the beam model itself remains an approximation.

/// The zoom ground is read at.
pub const ZOOM: u8 = 11;
/// Where the tiles are.
pub const TILE_URL: &str = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium";
/// What a reader is told the ground comes from.
pub const SOURCE: &str = "AWS Terrain Tiles (Terrarium; SRTM, USGS 3DEP and others)";
const SIZE: usize = 256;

/// A tile's address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileId {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

/// The tile holding `(lon, lat)` at `z`, and where in it the point falls, in pixels.
pub fn locate(lon: f64, lat: f64, z: u8) -> Option<(TileId, f64, f64)> {
    if !(lon.is_finite() && lat.is_finite()) || lat.abs() > 85.05 {
        return None;
    }
    let n = f64::from(1u32 << z);
    let fx = (lon + 180.0) / 360.0 * n;
    let lat_r = lat.to_radians();
    let fy = (1.0 - (lat_r.tan() + 1.0 / lat_r.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    let (x, y) = (fx.floor(), fy.floor());
    let max = n - 1.0;
    let id = TileId {
        z,
        x: x.clamp(0.0, max) as u32,
        y: y.clamp(0.0, max) as u32,
    };
    Some((id, (fx - x) * SIZE as f64, (fy - y) * SIZE as f64))
}

/// Ground metres per pixel at `lat` and zoom `z`.
pub fn resolution_m(lat: f64, z: u8) -> f64 {
    40_075_016.686 * lat.to_radians().cos() / (SIZE as f64 * f64::from(1u32 << z))
}

/// One decoded tile: 256×256 heights, metres MSL, rows north to south.
#[derive(Debug, Clone)]
pub struct Tile {
    pub id: TileId,
    pub heights: Vec<f32>,
}

/// Decode a Terrarium PNG (RGB or RGBA, 8-bit, 256×256).
pub fn decode(id: TileId, png_bytes: &[u8]) -> anyhow::Result<Tile> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    anyhow::ensure!(
        info.width as usize == SIZE && info.height as usize == SIZE,
        "a terrain tile is 256×256, this one {}×{}",
        info.width,
        info.height
    );
    anyhow::ensure!(
        info.bit_depth == png::BitDepth::Eight,
        "a terrain tile is 8-bit"
    );
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => anyhow::bail!("a terrain tile is RGB, this one {other:?}"),
    };
    let heights = buf[..SIZE * SIZE * channels]
        .chunks_exact(channels)
        .map(|p| (f32::from(p[0]) * 256.0 + f32::from(p[1]) + f32::from(p[2]) / 256.0) - 32768.0)
        .collect();
    Ok(Tile { id, heights })
}

impl Tile {
    /// Ground height at a pixel position (as [`locate`] gives it), bilinear between the four
    /// nearest pixel centres, clamped at the tile's edge.
    pub fn sample(&self, px: f64, py: f64) -> f32 {
        let at = |x: isize, y: isize| {
            let x = x.clamp(0, SIZE as isize - 1) as usize;
            let y = y.clamp(0, SIZE as isize - 1) as usize;
            self.heights[y * SIZE + x]
        };
        let (fx, fy) = (px - 0.5, py - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
        let (x0, y0) = (x0 as isize, y0 as isize);
        let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1, y0) * tx;
        let bottom = at(x0, y0 + 1) * (1.0 - tx) + at(x0 + 1, y0 + 1) * tx;
        top * (1.0 - ty) + bottom * ty
    }

    /// Ground height at `(lon, lat)`, when this tile holds it.
    pub fn height_at(&self, lon: f64, lat: f64) -> Option<f32> {
        let (id, px, py) = locate(lon, lat, self.id.z)?;
        (id == self.id).then(|| self.sample(px, py))
    }
}

/// Fetch and decode one tile.
pub async fn fetch(client: &reqwest::Client, id: TileId) -> anyhow::Result<Tile> {
    let url = format!("{TILE_URL}/{}/{}/{}.png", id.z, id.x, id.y);
    let bytes = client
        .get(&url)
        .timeout(crate::net::FEED_TIMEOUT)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    decode(id, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_math_matches_the_slippy_map_convention() {
        let (id, px, py) = locate(-97.2778, 35.3331, 11).unwrap();
        assert_eq!((id.z, id.x, id.y), (11, 470, 808));
        assert!((0.0..256.0).contains(&px) && (0.0..256.0).contains(&py));
        assert!((resolution_m(0.0, 11) - 76.437).abs() < 0.01);
        assert!(locate(0.0, 89.0, 11).is_none());
        assert!(locate(f64::NAN, 0.0, 11).is_none());
    }

    #[test]
    fn a_bad_tile_is_an_error_never_a_panic() {
        let id = TileId { z: 11, x: 0, y: 0 };
        assert!(decode(id, b"not a png").is_err());
        assert!(decode(id, &[]).is_err());
    }
}
