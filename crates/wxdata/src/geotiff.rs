//! GeoTIFF export of a lat/lon scalar grid (ROADMAP_NEW M5): every gridded layer in the app — the
//! MRMS mosaics, the fields derived from a radar volume (VIL, echo tops, MEHS, POSH), model fields
//! — is an [`MrmsField`], a regular grid in geographic coordinates, which is exactly what a
//! GeoTIFF in EPSG:4326 describes. QGIS, ArcGIS, GDAL and rasterio open one directly.
//!
//! One band of 32-bit IEEE floats in the field's own units, uncompressed, rows north to south.
//! Cells with no value stay NaN and are declared as nodata (the GDAL_NODATA tag), which is what
//! the field already uses for "nothing here". Georeferencing is the three GeoTIFF tags every
//! reader supports: pixel scale, one tie point at the north-west corner, and a key directory
//! saying geographic WGS 84, pixel-is-area.

use crate::mrms::MrmsField;

/// TIFF field types used here.
const SHORT: u16 = 3;
const LONG: u16 = 4;
const ASCII: u16 = 2;
const DOUBLE: u16 = 12;

/// One IFD entry before layout: tag, type, count and its value bytes (little-endian).
struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    bytes: Vec<u8>,
}

fn shorts(tag: u16, v: &[u16]) -> Entry {
    Entry {
        tag,
        kind: SHORT,
        count: v.len() as u32,
        bytes: v.iter().flat_map(|x| x.to_le_bytes()).collect(),
    }
}

fn long(tag: u16, v: u32) -> Entry {
    Entry {
        tag,
        kind: LONG,
        count: 1,
        bytes: v.to_le_bytes().to_vec(),
    }
}

fn doubles(tag: u16, v: &[f64]) -> Entry {
    Entry {
        tag,
        kind: DOUBLE,
        count: v.len() as u32,
        bytes: v.iter().flat_map(|x| x.to_le_bytes()).collect(),
    }
}

fn ascii(tag: u16, s: &str) -> Entry {
    let mut bytes = s.as_bytes().to_vec();
    bytes.push(0);
    Entry {
        tag,
        kind: ASCII,
        count: bytes.len() as u32,
        bytes,
    }
}

/// The bytes of a single-band float32 GeoTIFF of `field`, with `description` in its
/// ImageDescription tag (what the grid is, its units, its time — readers show it as metadata).
/// `None` for an empty grid or one whose values do not match its dimensions.
pub fn write(field: &MrmsField, description: &str) -> Option<Vec<u8>> {
    let (nx, ny) = (field.nx, field.ny);
    if nx == 0 || ny == 0 || field.values.len() != nx * ny {
        return None;
    }
    let data: Vec<u8> = field.values.iter().flat_map(|v| v.to_le_bytes()).collect();
    // Header, then the pixel data, then the IFD and anything too big to sit inside it.
    const HEADER: u32 = 8;
    let data_offset = HEADER;
    let ifd_offset = {
        let end = data_offset as usize + data.len();
        (end + end % 2) as u32 // IFDs start on a word boundary
    };
    let dx = (field.lon_east - field.lon_west) / nx as f64;
    let dy = (field.lat_north - field.lat_south) / ny as f64;
    let mut entries = vec![
        long(256, nx as u32),           // ImageWidth
        long(257, ny as u32),           // ImageLength
        shorts(258, &[32]),             // BitsPerSample
        shorts(259, &[1]),              // Compression: none
        shorts(262, &[1]),              // Photometric: BlackIsZero
        ascii(270, description),        // ImageDescription
        long(273, data_offset),         // StripOffsets (one strip)
        shorts(277, &[1]),              // SamplesPerPixel
        long(278, ny as u32),           // RowsPerStrip
        long(279, data.len() as u32),   // StripByteCounts
        shorts(284, &[1]),              // PlanarConfiguration: chunky
        shorts(339, &[3]),              // SampleFormat: IEEE float
        doubles(33550, &[dx, dy, 0.0]), // ModelPixelScale
        doubles(
            33922, // ModelTiepoint: raster (0, 0) is the north-west corner
            &[0.0, 0.0, 0.0, field.lon_west, field.lat_north, 0.0],
        ),
        shorts(
            34735, // GeoKeyDirectory: version 1.1.0, three keys
            &[
                1, 1, 0, 3, //
                1024, 0, 1, 2, // GTModelType: geographic
                1025, 0, 1, 1, // GTRasterType: pixel is area
                2048, 0, 1, 4326, // GeographicType: WGS 84
            ],
        ),
        ascii(42113, "nan"), // GDAL_NODATA
    ];
    entries.sort_by_key(|e| e.tag);
    // Entry count, the entries, the next-IFD offset (none); then the overflow values.
    let ifd_len = 2 + entries.len() as u32 * 12 + 4;
    let mut overflow_at = ifd_offset + ifd_len;
    let mut ifd = Vec::new();
    let mut overflow = Vec::new();
    ifd.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for e in &entries {
        ifd.extend_from_slice(&e.tag.to_le_bytes());
        ifd.extend_from_slice(&e.kind.to_le_bytes());
        ifd.extend_from_slice(&e.count.to_le_bytes());
        if e.bytes.len() <= 4 {
            let mut inline = e.bytes.clone();
            inline.resize(4, 0);
            ifd.extend_from_slice(&inline);
        } else {
            ifd.extend_from_slice(&overflow_at.to_le_bytes());
            overflow.extend_from_slice(&e.bytes);
            if overflow.len() % 2 == 1 {
                overflow.push(0);
            }
            overflow_at = ifd_offset + ifd_len + overflow.len() as u32;
        }
    }
    ifd.extend_from_slice(&0u32.to_le_bytes());

    let mut out = Vec::with_capacity(ifd_offset as usize + ifd.len() + overflow.len());
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&ifd_offset.to_le_bytes());
    out.extend_from_slice(&data);
    out.resize(ifd_offset as usize, 0);
    out.extend_from_slice(&ifd);
    out.extend_from_slice(&overflow);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;

    fn field() -> MrmsField {
        // 4 wide, 3 tall, over central Oklahoma; one hole.
        let mut values: Vec<f32> = (0..12).map(|i| i as f32 * 1.5).collect();
        values[5] = f32::NAN;
        MrmsField {
            values,
            nx: 4,
            ny: 3,
            lon_west: -98.0,
            lon_east: -97.0,
            lat_north: 36.0,
            lat_south: 35.4,
            time: "2013-05-20T20:08:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn an_independent_reader_gets_the_grid_and_its_georeferencing_back() {
        let bytes = write(&field(), "MESH (mm) 2013-05-20T20:08Z").unwrap();
        let mut d = Decoder::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(d.dimensions().unwrap(), (4, 3));
        let DecodingResult::F32(v) = d.read_image().unwrap() else {
            panic!("expected float32 samples");
        };
        let want = field().values;
        assert_eq!(v.len(), want.len());
        for (a, b) in v.iter().zip(&want) {
            assert!(a == b || (a.is_nan() && b.is_nan()), "{a} vs {b}");
        }
        let scale = d.get_tag_f64_vec(Tag::Unknown(33550)).unwrap();
        assert!((scale[0] - 0.25).abs() < 1e-12 && (scale[1] - 0.2).abs() < 1e-12);
        let tie = d.get_tag_f64_vec(Tag::Unknown(33922)).unwrap();
        assert_eq!(&tie[3..5], &[-98.0, 36.0]);
        let keys = d.get_tag_u16_vec(Tag::Unknown(34735)).unwrap();
        assert_eq!(keys[3], 3, "three geokeys");
        assert!(keys.windows(4).any(|w| w == [2048, 0, 1, 4326]), "{keys:?}");
        assert_eq!(d.get_tag_ascii_string(Tag::Unknown(42113)).unwrap(), "nan");
        assert_eq!(
            d.get_tag_ascii_string(Tag::ImageDescription).unwrap(),
            "MESH (mm) 2013-05-20T20:08Z"
        );
    }

    #[test]
    fn an_empty_or_inconsistent_grid_is_not_written() {
        let mut f = field();
        f.values.pop();
        assert!(write(&f, "").is_none());
        let mut f = field();
        f.nx = 0;
        assert!(write(&f, "").is_none());
    }
}
