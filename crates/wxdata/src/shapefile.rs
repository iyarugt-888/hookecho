//! ESRI Shapefile import (ROADMAP_NEW I1 item 2): `.shp` geometry, `.dbf` attributes and the
//! `.prj` coordinate system, read into the same [`GisFeature`] the GeoJSON path produces — so
//! everything downstream (drawing, click popups, zoom-to-fit, GeoJSON export) works on a
//! Shapefile with no changes.
//!
//! Written from the published spec with no dependency: the format is small, and this parses
//! files a stranger emailed you, so every read is bounds-checked and a malformed or truncated
//! file is an error, never a panic and never a huge allocation driven by a length field.
//!
//! ## Coordinate systems (I2's rule, applied here)
//!
//! Geometry drawn in the wrong place is worse than geometry not drawn, so [`parse`] reads the
//! `.prj` and refuses what it cannot place correctly:
//!
//! - **WGS 84 and NAD 83** geographic: taken as lon/lat. The two differ by about a metre over the
//!   U.S., far below anything a weather map can show.
//! - **Web Mercator** (EPSG:3857): inverse-projected here.
//! - **Anything else**, including every State Plane and UTM zone: a named error saying what the
//!   file is in. Reprojection beyond Web Mercator is not built yet.
//! - **No `.prj`**: accepted only when every coordinate is a plausible lon/lat, otherwise an
//!   error — a file of metres would otherwise land in the Gulf of Guinea.
//!
//! ## What is not read
//!
//! `Z` and `M` values are skipped (the map is 2-D); `MultiPatch` (3-D surfaces) is an error; memo
//! (`M`) attribute columns come back null; `.shx` is not needed, since the `.shp` records can be
//! walked in order. Text attributes are decoded as UTF-8, falling back to Latin-1 for bytes that
//! are not valid UTF-8 — which is what most DBF files written by desktop GIS actually contain.

use crate::gis::{GisFeature, Geometry};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{Map, Value};

const SHP_FILE_CODE: i32 = 9994;
const SHP_HEADER_LEN: usize = 100;
const DBF_FIELD_DESC_LEN: usize = 32;
const EARTH_RADIUS_M: f64 = 6_378_137.0;

/// How a file's coordinates are to be read, decided from its `.prj`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crs {
    /// Already lon/lat in a datum indistinguishable from WGS 84 at map scale.
    LonLat,
    /// EPSG:3857 metres, to be inverse-projected.
    WebMercator,
}

/// Decide the coordinate system from a `.prj` (ESRI WKT), or say why it cannot be used.
pub fn crs_from_prj(prj: &str) -> Result<Crs> {
    let upper = prj.to_ascii_uppercase();
    if upper.trim_start().starts_with("PROJCS") {
        let is_web_mercator = upper.contains("WEB_MERCATOR")
            || upper.contains("MERCATOR_AUXILIARY_SPHERE")
            || upper.contains("POPULAR VISUALISATION")
            || upper.contains("\"EPSG\",\"3857\"")
            || upper.contains("\"EPSG\",\"900913\"");
        if is_web_mercator {
            return Ok(Crs::WebMercator);
        }
        bail!(
            "this shapefile is in a projected coordinate system ({}), which HookEcho cannot \
             place yet — re-export it as WGS 84 (EPSG:4326) or Web Mercator (EPSG:3857)",
            wkt_name(prj)
        );
    }
    if upper.trim_start().starts_with("GEOGCS") {
        let ok = ["WGS_1984", "WGS 84", "WGS84", "NORTH_AMERICAN_1983", "NAD83", "NAD_1983"]
            .iter()
            .any(|k| upper.contains(k));
        if ok {
            return Ok(Crs::LonLat);
        }
        bail!(
            "this shapefile uses the geographic datum {}, which is not WGS 84 or NAD 83 and can \
             sit tens of metres off — re-export it as WGS 84 (EPSG:4326)",
            wkt_name(prj)
        );
    }
    bail!("the .prj is not a coordinate system HookEcho recognises: {}", prj.trim().chars().take(60).collect::<String>())
}

/// The first quoted name in a WKT string, for an error message.
fn wkt_name(wkt: &str) -> String {
    wkt.split('"')
        .nth(1)
        .map_or_else(|| "unnamed".to_string(), |s| format!("\"{s}\""))
}

/// Read a shapefile into features.
///
/// `dbf` supplies each feature's attributes when present; `prj` decides the coordinate system.
/// The `.dbf` must have exactly one row per `.shp` record — anything else means the files are not
/// a pair, and matching them up anyway would attach the wrong attributes to shapes.
pub fn parse(shp: &[u8], dbf: Option<&[u8]>, prj: Option<&str>) -> Result<Vec<GisFeature>> {
    let crs = prj.map(crs_from_prj).transpose()?;
    let shapes = read_shp(shp)?;
    let rows = dbf.map(read_dbf).transpose()?;
    if let Some(rows) = &rows {
        if rows.len() != shapes.len() {
            bail!(
                "the .dbf has {} rows but the .shp has {} records — these are not a matching pair",
                rows.len(),
                shapes.len()
            );
        }
    }

    let mut out = Vec::new();
    for (i, shape) in shapes.into_iter().enumerate() {
        let row = rows.as_ref().map(|r| &r[i]);
        // A record deleted in the .dbf is deleted: the shape stays in the .shp until the file is
        // packed, and drawing it would resurrect something the author removed.
        if row.is_some_and(|r| r.deleted) {
            continue;
        }
        let Some(geometry) = shape else { continue };
        out.push(GisFeature {
            geometry,
            properties: row.map(|r| r.properties.clone()).unwrap_or_default(),
        });
    }

    match crs {
        Some(Crs::LonLat) => {}
        Some(Crs::WebMercator) => {
            for f in &mut out {
                map_points(&mut f.geometry, mercator_to_lonlat);
            }
        }
        None => {
            let mut plausible = true;
            for f in &mut out {
                map_points(&mut f.geometry, |p| {
                    if p[0].abs() > 180.0 || p[1].abs() > 90.0 {
                        plausible = false;
                    }
                    p
                });
            }
            if !plausible {
                bail!(
                    "there is no .prj and the coordinates are outside longitude/latitude range, so \
                     this file is probably projected — add its .prj next to it, or re-export as WGS 84"
                );
            }
        }
    }
    Ok(out)
}

fn mercator_to_lonlat(p: [f64; 2]) -> [f64; 2] {
    let lon = (p[0] / EARTH_RADIUS_M).to_degrees();
    let lat = (2.0 * (p[1] / EARTH_RADIUS_M).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees();
    [lon, lat]
}

fn map_points(g: &mut Geometry, mut f: impl FnMut([f64; 2]) -> [f64; 2]) {
    let mut each = |p: &mut [f64; 2]| *p = f(*p);
    match g {
        Geometry::Point(p) => each(p),
        Geometry::MultiPoint(ps) | Geometry::LineString(ps) => ps.iter_mut().for_each(each),
        Geometry::MultiLineString(ls) | Geometry::Polygon(ls) => {
            ls.iter_mut().flatten().for_each(each)
        }
        Geometry::MultiPolygon(parts) => parts.iter_mut().flatten().flatten().for_each(each),
    }
}

// ---- byte reading ------------------------------------------------------------------------------

/// A cursor over untrusted bytes: every read says what it wanted if the data ran out.
struct Cur<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| anyhow!("the file ends in the middle of a record (wanted {n} bytes at offset {})", self.pos))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn i32_le(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
    fn f64_le(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }
    fn xy(&mut self) -> Result<[f64; 2]> {
        Ok([self.f64_le()?, self.f64_le()?])
    }
    fn remaining(&self) -> usize {
        self.b.len() - self.pos
    }
}

// ---- .shp --------------------------------------------------------------------------------------

/// One record's geometry, or `None` for a null shape (kept so row numbers still line up with the
/// `.dbf`).
fn read_shp(shp: &[u8]) -> Result<Vec<Option<Geometry>>> {
    if shp.len() < SHP_HEADER_LEN {
        bail!("this is too short to be a shapefile ({} bytes)", shp.len());
    }
    let code = i32::from_be_bytes(shp[0..4].try_into().expect("4 bytes"));
    if code != SHP_FILE_CODE {
        bail!("this is not a shapefile (file code {code}, expected {SHP_FILE_CODE})");
    }
    // The header's own length, in 16-bit words. Trailing bytes past it are ignored; a header that
    // claims more than the file holds is caught below when a record runs off the end.
    let declared = (i32::from_be_bytes(shp[24..28].try_into().expect("4 bytes")).max(0) as usize)
        .saturating_mul(2)
        .clamp(SHP_HEADER_LEN, usize::MAX);
    let end = declared.min(shp.len());

    let mut out = Vec::new();
    let mut pos = SHP_HEADER_LEN;
    while pos + 8 <= end {
        let content_words = i32::from_be_bytes(shp[pos + 4..pos + 8].try_into().expect("4 bytes"));
        let content_len = usize::try_from(content_words)
            .map_err(|_| anyhow!("record {} has a negative length", out.len() + 1))?
            .saturating_mul(2);
        let start = pos + 8;
        let stop = start
            .checked_add(content_len)
            .filter(|&s| s <= shp.len())
            .ok_or_else(|| anyhow!("record {} runs past the end of the file", out.len() + 1))?;
        let rec = &shp[start..stop];
        out.push(
            read_record(rec).with_context(|| format!("shapefile record {}", out.len() + 1))?,
        );
        pos = stop;
    }
    Ok(out)
}

fn read_record(rec: &[u8]) -> Result<Option<Geometry>> {
    let mut c = Cur { b: rec, pos: 0 };
    let kind = c.i32_le()?;
    Ok(match kind {
        0 => None,
        // Point, PointZ, PointM
        1 | 11 | 21 => Some(Geometry::Point(c.xy()?)),
        // MultiPoint, MultiPointZ, MultiPointM
        8 | 18 | 28 => {
            c.take(32)?; // bounding box
            let n = count(&mut c, 16)?;
            let pts = (0..n).map(|_| c.xy()).collect::<Result<Vec<_>>>()?;
            Some(Geometry::MultiPoint(pts))
        }
        // PolyLine / Polygon and their Z and M forms
        3 | 13 | 23 | 5 | 15 | 25 => {
            let polygon = matches!(kind, 5 | 15 | 25);
            c.take(32)?;
            let n_parts = count(&mut c, 4)?;
            let n_points = count(&mut c, 16)?;
            let parts = (0..n_parts)
                .map(|_| c.i32_le())
                .collect::<Result<Vec<_>>>()?;
            let pts = (0..n_points).map(|_| c.xy()).collect::<Result<Vec<_>>>()?;
            let rings = split_parts(&parts, &pts)?;
            Some(if polygon {
                assemble_polygons(rings)
            } else if rings.len() == 1 {
                Geometry::LineString(rings.into_iter().next().expect("one part"))
            } else {
                Geometry::MultiLineString(rings)
            })
        }
        31 => bail!("MultiPatch (3-D surface) shapes are not supported"),
        other => bail!("unknown shape type {other}"),
    })
}

/// Read a non-negative element count and check the record could actually hold that many
/// `each`-byte elements, so a hostile count cannot drive a huge allocation.
fn count(c: &mut Cur, each: usize) -> Result<usize> {
    let n = c.i32_le()?;
    let n = usize::try_from(n).map_err(|_| anyhow!("a negative element count"))?;
    if n.checked_mul(each).is_none_or(|bytes| bytes > c.remaining()) {
        bail!("a record claims {n} elements but is too short to hold them");
    }
    Ok(n)
}

/// Cut the point list into its parts at the given start indices.
fn split_parts(starts: &[i32], pts: &[[f64; 2]]) -> Result<Vec<Vec<[f64; 2]>>> {
    let mut bounds = Vec::with_capacity(starts.len() + 1);
    for &s in starts {
        let s = usize::try_from(s).map_err(|_| anyhow!("a part starts at a negative index"))?;
        if s > pts.len() {
            bail!("a part starts past the end of the points");
        }
        bounds.push(s);
    }
    bounds.push(pts.len());
    if bounds.windows(2).any(|w| w[0] > w[1]) {
        bail!("the parts are not in increasing order");
    }
    Ok(bounds.windows(2).map(|w| pts[w[0]..w[1]].to_vec()).collect())
}

// ---- polygons ----------------------------------------------------------------------------------

/// Twice the signed area: negative for clockwise (a shapefile outer ring), positive for
/// counter-clockwise (a hole).
fn signed_area2(ring: &[[f64; 2]]) -> f64 {
    ring.windows(2)
        .map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1])
        .sum()
}

fn point_in_ring(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut inside = false;
    for w in ring.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}

/// Group a polygon record's rings into polygons. The spec orients outer rings clockwise and holes
/// counter-clockwise, and one record can hold several separate polygons; a hole belongs to the
/// outer ring that contains it. A file written with the opposite orientation would turn every hole
/// into a filled island, so a ring that is counter-clockwise but sits inside no outer ring is
/// treated as an outer ring rather than dropped.
fn assemble_polygons(rings: Vec<Vec<[f64; 2]>>) -> Geometry {
    let rings: Vec<_> = rings.into_iter().filter(|r| r.len() >= 4).collect();
    let mut polys: Vec<Vec<Vec<[f64; 2]>>> = Vec::new();
    let mut holes = Vec::new();
    for ring in rings {
        if signed_area2(&ring) <= 0.0 {
            polys.push(vec![ring]);
        } else {
            holes.push(ring);
        }
    }
    for hole in holes {
        let owner = polys
            .iter()
            .position(|p| point_in_ring(hole[0], &p[0]));
        match owner {
            Some(i) => polys[i].push(hole),
            None => polys.push(vec![hole]),
        }
    }
    if polys.len() == 1 {
        Geometry::Polygon(polys.pop().expect("one polygon"))
    } else {
        Geometry::MultiPolygon(polys)
    }
}

// ---- .dbf --------------------------------------------------------------------------------------

struct Row {
    deleted: bool,
    properties: Map<String, Value>,
}

struct Field {
    name: String,
    kind: u8,
    len: usize,
    decimals: u8,
}

fn read_dbf(dbf: &[u8]) -> Result<Vec<Row>> {
    if dbf.len() < DBF_FIELD_DESC_LEN {
        bail!("the .dbf is too short to hold a header");
    }
    let n_records = u32::from_le_bytes(dbf[4..8].try_into().expect("4 bytes")) as usize;
    let header_len = u16::from_le_bytes(dbf[8..10].try_into().expect("2 bytes")) as usize;
    let record_len = u16::from_le_bytes(dbf[10..12].try_into().expect("2 bytes")) as usize;
    if header_len < DBF_FIELD_DESC_LEN || header_len > dbf.len() || record_len == 0 {
        bail!("the .dbf header is inconsistent");
    }

    let mut fields = Vec::new();
    let mut at = DBF_FIELD_DESC_LEN;
    while at + DBF_FIELD_DESC_LEN <= header_len && dbf[at] != 0x0D {
        let d = &dbf[at..at + DBF_FIELD_DESC_LEN];
        let name_end = d[..11].iter().position(|&b| b == 0).unwrap_or(11);
        fields.push(Field {
            name: decode_text(&d[..name_end]).trim().to_string(),
            kind: d[11],
            len: d[16] as usize,
            decimals: d[17],
        });
        at += DBF_FIELD_DESC_LEN;
    }
    // 1 for the deletion flag at the start of every record.
    let widths: usize = fields.iter().map(|f| f.len).sum::<usize>() + 1;
    if widths > record_len {
        bail!("the .dbf fields are wider than its own record length");
    }

    let needed = header_len
        .checked_add(n_records.checked_mul(record_len).ok_or_else(|| anyhow!("the .dbf record count is absurd"))?)
        .ok_or_else(|| anyhow!("the .dbf size overflows"))?;
    if needed > dbf.len() {
        bail!(
            "the .dbf claims {n_records} rows but is too short to hold them ({} of {needed} bytes)",
            dbf.len()
        );
    }

    let mut rows = Vec::with_capacity(n_records);
    for i in 0..n_records {
        let rec = &dbf[header_len + i * record_len..header_len + (i + 1) * record_len];
        let mut props = Map::new();
        let mut off = 1;
        for f in &fields {
            let raw = &rec[off..off + f.len];
            off += f.len;
            props.insert(f.name.clone(), field_value(f, raw));
        }
        rows.push(Row {
            deleted: rec[0] == 0x2A,
            properties: props,
        });
    }
    Ok(rows)
}

fn field_value(f: &Field, raw: &[u8]) -> Value {
    let text = decode_text(raw);
    let t = text.trim();
    match f.kind {
        b'C' => Value::String(text.trim_end().to_string()),
        b'N' | b'F' => {
            if t.is_empty() {
                return Value::Null;
            }
            match t.parse::<f64>() {
                Ok(v) if f.decimals == 0 && v.fract() == 0.0 && v.abs() < 9.0e15 => {
                    Value::from(v as i64)
                }
                Ok(v) => serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number),
                Err(_) => Value::Null,
            }
        }
        b'D' if t.len() == 8 && t.bytes().all(|b| b.is_ascii_digit()) => {
            Value::String(format!("{}-{}-{}", &t[0..4], &t[4..6], &t[6..8]))
        }
        b'L' => match t.chars().next() {
            Some('T' | 't' | 'Y' | 'y') => Value::Bool(true),
            Some('F' | 'f' | 'N' | 'n') => Value::Bool(false),
            _ => Value::Null,
        },
        // Memo, binary and anything else: nothing honest to show.
        _ => Value::Null,
    }
}

/// UTF-8 where the bytes are valid UTF-8, otherwise Latin-1 (every byte a code point).
fn decode_text(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| c as char).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- a small shapefile writer, so the tests state their fixtures rather than embed blobs ----

    fn shp_file(records: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::new();
        for (i, rec) in records.iter().enumerate() {
            body.extend_from_slice(&(i as i32 + 1).to_be_bytes());
            body.extend_from_slice(&((rec.len() / 2) as i32).to_be_bytes());
            body.extend_from_slice(rec);
        }
        let mut out = vec![0u8; SHP_HEADER_LEN];
        out[0..4].copy_from_slice(&SHP_FILE_CODE.to_be_bytes());
        out[24..28].copy_from_slice(&(((SHP_HEADER_LEN + body.len()) / 2) as i32).to_be_bytes());
        out[28..32].copy_from_slice(&1000i32.to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn point(x: f64, y: f64) -> Vec<u8> {
        let mut r = 1i32.to_le_bytes().to_vec();
        r.extend_from_slice(&x.to_le_bytes());
        r.extend_from_slice(&y.to_le_bytes());
        r
    }

    fn null_shape() -> Vec<u8> {
        0i32.to_le_bytes().to_vec()
    }

    /// PolyLine (kind 3) or Polygon (kind 5) from parts of `[x, y]` points.
    fn poly(kind: i32, parts: &[Vec<[f64; 2]>]) -> Vec<u8> {
        let mut r = kind.to_le_bytes().to_vec();
        r.extend_from_slice(&[0u8; 32]);
        r.extend_from_slice(&(parts.len() as i32).to_le_bytes());
        let total: usize = parts.iter().map(Vec::len).sum();
        r.extend_from_slice(&(total as i32).to_le_bytes());
        let mut start = 0;
        for p in parts {
            r.extend_from_slice(&(start as i32).to_le_bytes());
            start += p.len();
        }
        for p in parts.iter().flatten() {
            r.extend_from_slice(&p[0].to_le_bytes());
            r.extend_from_slice(&p[1].to_le_bytes());
        }
        r
    }

    /// Clockwise square: what the spec calls an outer ring.
    fn cw(x0: f64, y0: f64, s: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x0, y0 + s], [x0 + s, y0 + s], [x0 + s, y0], [x0, y0]]
    }

    /// Counter-clockwise square: a hole.
    fn ccw(x0: f64, y0: f64, s: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x0 + s, y0], [x0 + s, y0 + s], [x0, y0 + s], [x0, y0]]
    }

    /// `fields` are `(name, type, length, decimals)`; `rows` are the raw cell text per field.
    fn dbf_file(fields: &[(&str, u8, u8, u8)], rows: &[(bool, Vec<&str>)]) -> Vec<u8> {
        let record_len = 1 + fields.iter().map(|f| f.2 as usize).sum::<usize>();
        let header_len = 32 + fields.len() * 32 + 1;
        let mut out = vec![0u8; 32];
        out[0] = 3;
        out[4..8].copy_from_slice(&(rows.len() as u32).to_le_bytes());
        out[8..10].copy_from_slice(&(header_len as u16).to_le_bytes());
        out[10..12].copy_from_slice(&(record_len as u16).to_le_bytes());
        for (name, kind, len, dec) in fields {
            let mut d = [0u8; 32];
            d[..name.len()].copy_from_slice(name.as_bytes());
            d[11] = *kind;
            d[16] = *len;
            d[17] = *dec;
            out.extend_from_slice(&d);
        }
        out.push(0x0D);
        for (deleted, cells) in rows {
            out.push(if *deleted { 0x2A } else { 0x20 });
            for ((_, _, len, _), cell) in fields.iter().zip(cells) {
                let mut c = cell.as_bytes().to_vec();
                c.resize(*len as usize, b' ');
                out.extend_from_slice(&c);
            }
        }
        out
    }

    const WGS84: &str = r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]]]"#;

    // ---- geometry -----------------------------------------------------------------------------

    #[test]
    fn a_point_is_read_as_lon_lat() {
        let shp = shp_file(&[point(-97.5, 35.25)]);
        let f = parse(&shp, None, Some(WGS84)).expect("parses");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].geometry, Geometry::Point([-97.5, 35.25]));
    }

    #[test]
    fn one_part_is_a_line_and_several_are_a_multi_line() {
        let a = vec![[0.0, 0.0], [1.0, 1.0]];
        let b = vec![[2.0, 2.0], [3.0, 3.0]];
        let shp = shp_file(&[poly(3, std::slice::from_ref(&a)), poly(3, &[a.clone(), b.clone()])]);
        let f = parse(&shp, None, Some(WGS84)).expect("parses");
        assert_eq!(f[0].geometry, Geometry::LineString(a.clone()));
        assert_eq!(f[1].geometry, Geometry::MultiLineString(vec![a, b]));
    }

    #[test]
    fn a_hole_is_attached_to_the_outer_ring_that_contains_it() {
        let outer = cw(0.0, 0.0, 10.0);
        let hole = ccw(2.0, 2.0, 2.0);
        let shp = shp_file(&[poly(5, &[outer.clone(), hole.clone()])]);
        let f = parse(&shp, None, Some(WGS84)).expect("parses");
        assert_eq!(f[0].geometry, Geometry::Polygon(vec![outer, hole]));
    }

    #[test]
    fn two_separate_outer_rings_are_two_polygons_and_each_keeps_its_own_hole() {
        let (a, b) = (cw(0.0, 0.0, 10.0), cw(20.0, 0.0, 10.0));
        let hole_in_b = ccw(22.0, 2.0, 2.0);
        let shp = shp_file(&[poly(5, &[a.clone(), b.clone(), hole_in_b.clone()])]);
        let f = parse(&shp, None, Some(WGS84)).expect("parses");
        assert_eq!(
            f[0].geometry,
            Geometry::MultiPolygon(vec![vec![a], vec![b, hole_in_b]])
        );
    }

    #[test]
    fn a_counter_clockwise_ring_inside_nothing_is_an_outer_ring_not_a_dropped_one() {
        // A file written with the opposite orientation must still draw.
        let ring = ccw(0.0, 0.0, 5.0);
        let shp = shp_file(&[poly(5, std::slice::from_ref(&ring))]);
        let f = parse(&shp, None, Some(WGS84)).expect("parses");
        assert_eq!(f[0].geometry, Geometry::Polygon(vec![ring]));
    }

    #[test]
    fn a_null_shape_is_skipped_but_still_counts_for_attribute_alignment() {
        let shp = shp_file(&[point(1.0, 2.0), null_shape(), point(3.0, 4.0)]);
        let dbf = dbf_file(
            &[("NAME", b'C', 6, 0)],
            &[(false, vec!["first"]), (false, vec!["nulled"]), (false, vec!["third"])],
        );
        let f = parse(&shp, Some(&dbf), Some(WGS84)).expect("parses");
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].properties["NAME"], "first");
        assert_eq!(f[1].properties["NAME"], "third", "the null row must not shift the next one");
    }

    // ---- attributes ---------------------------------------------------------------------------

    #[test]
    fn attribute_types_come_back_as_their_json_types() {
        let shp = shp_file(&[point(0.0, 0.0)]);
        let dbf = dbf_file(
            &[
                ("NAME", b'C', 8, 0),
                ("POP", b'N', 8, 0),
                ("RATIO", b'N', 8, 3),
                ("WHEN", b'D', 8, 0),
                ("OK", b'L', 1, 0),
                ("BLANK", b'N', 4, 0),
            ],
            &[(false, vec!["Norman", "120000", "0.125", "20130520", "T", ""])],
        );
        let p = &parse(&shp, Some(&dbf), Some(WGS84)).expect("parses")[0].properties;
        assert_eq!(p["NAME"], "Norman");
        assert_eq!(p["POP"], 120000, "an integer column is an integer, not 120000.0");
        assert_eq!(p["RATIO"], 0.125);
        assert_eq!(p["WHEN"], "2013-05-20");
        assert_eq!(p["OK"], true);
        assert_eq!(p["BLANK"], Value::Null, "an empty number is null, not zero");
    }

    #[test]
    fn a_deleted_row_is_dropped_even_though_its_shape_is_still_in_the_shp() {
        let shp = shp_file(&[point(1.0, 1.0), point(2.0, 2.0)]);
        let dbf = dbf_file(
            &[("N", b'N', 2, 0)],
            &[(true, vec!["1"]), (false, vec!["2"])],
        );
        let f = parse(&shp, Some(&dbf), Some(WGS84)).expect("parses");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].geometry, Geometry::Point([2.0, 2.0]));
    }

    #[test]
    fn latin_1_text_survives_instead_of_becoming_an_error() {
        let shp = shp_file(&[point(0.0, 0.0)]);
        let mut dbf = dbf_file(&[("N", b'C', 5, 0)], &[(false, vec!["Zz"])]);
        // "Ré" in Latin-1: 0x52 0xE9, which is not valid UTF-8.
        let at = dbf.len() - 5;
        dbf[at] = 0x52;
        dbf[at + 1] = 0xE9;
        let p = &parse(&shp, Some(&dbf), Some(WGS84)).expect("parses")[0].properties;
        assert_eq!(p["N"], "R\u{e9}");
    }

    #[test]
    fn a_dbf_that_is_not_this_shapes_pair_is_refused_rather_than_misattributed() {
        let shp = shp_file(&[point(1.0, 1.0), point(2.0, 2.0)]);
        let dbf = dbf_file(&[("N", b'N', 2, 0)], &[(false, vec!["1"])]);
        let err = parse(&shp, Some(&dbf), Some(WGS84)).unwrap_err().to_string();
        assert!(err.contains("1 rows") && err.contains("2 records"), "{err}");
    }

    // ---- coordinate systems -------------------------------------------------------------------

    #[test]
    fn web_mercator_is_inverse_projected() {
        // 10 degrees east / 45 degrees north in EPSG:3857 metres.
        let x = 10f64.to_radians() * EARTH_RADIUS_M;
        let y = (std::f64::consts::FRAC_PI_4 + 45f64.to_radians() / 2.0).tan().ln() * EARTH_RADIUS_M;
        let shp = shp_file(&[point(x, y)]);
        let prj = r#"PROJCS["WGS_1984_Web_Mercator_Auxiliary_Sphere",GEOGCS["GCS_WGS_1984"]]"#;
        let f = parse(&shp, None, Some(prj)).expect("parses");
        let Geometry::Point(p) = f[0].geometry else { panic!("point") };
        assert!((p[0] - 10.0).abs() < 1e-9 && (p[1] - 45.0).abs() < 1e-9, "{p:?}");
    }

    #[test]
    fn nad83_is_accepted_as_lon_lat() {
        let prj = r#"GEOGCS["GCS_North_American_1983",DATUM["D_North_American_1983"]]"#;
        assert_eq!(crs_from_prj(prj).unwrap(), Crs::LonLat);
    }

    #[test]
    fn a_state_plane_file_is_refused_and_named() {
        let prj = r#"PROJCS["NAD_1983_StatePlane_Oklahoma_South_FIPS_3502_Feet",GEOGCS["GCS_North_American_1983"]]"#;
        let err = crs_from_prj(prj).unwrap_err().to_string();
        assert!(err.contains("StatePlane_Oklahoma_South"), "must say what it is in: {err}");
    }

    #[test]
    fn an_older_datum_is_refused_because_it_can_sit_tens_of_metres_off() {
        let prj = r#"GEOGCS["GCS_North_American_1927",DATUM["D_North_American_1927"]]"#;
        assert!(crs_from_prj(prj).unwrap_err().to_string().contains("1927"));
    }

    #[test]
    fn no_prj_is_fine_for_lon_lat_and_an_error_for_metres() {
        let ok = shp_file(&[point(-97.0, 35.0)]);
        assert!(parse(&ok, None, None).is_ok());
        let metres = shp_file(&[point(500_000.0, 4_000_000.0)]);
        let err = parse(&metres, None, None).unwrap_err().to_string();
        assert!(err.contains("no .prj"), "{err}");
    }

    // ---- hostile and broken input -------------------------------------------------------------

    #[test]
    fn something_that_is_not_a_shapefile_says_so() {
        assert!(parse(&[0u8; 200], None, None).unwrap_err().to_string().contains("not a shapefile"));
        assert!(parse(b"tiny", None, None).unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn a_huge_point_count_in_a_tiny_record_is_refused_not_allocated() {
        let mut rec = 3i32.to_le_bytes().to_vec();
        rec.extend_from_slice(&[0u8; 32]);
        rec.extend_from_slice(&1i32.to_le_bytes()); // one part
        rec.extend_from_slice(&i32::MAX.to_le_bytes()); // two billion points
        rec.extend_from_slice(&0i32.to_le_bytes());
        let shp = shp_file(&[rec]);
        let err = parse(&shp, None, Some(WGS84)).unwrap_err();
        assert!(format!("{err:#}").contains("too short to hold"), "{err:#}");
    }

    #[test]
    fn a_part_index_outside_the_points_is_an_error() {
        let mut rec = poly(3, &[vec![[0.0, 0.0], [1.0, 1.0]]]);
        // Overwrite the single part start (offset 4 kind + 32 box + 4 parts + 4 points = 44).
        rec[44..48].copy_from_slice(&99i32.to_le_bytes());
        let shp = shp_file(&[rec]);
        assert!(parse(&shp, None, Some(WGS84)).is_err());
    }

    #[test]
    fn multipatch_is_named_as_unsupported() {
        let shp = shp_file(&[31i32.to_le_bytes().to_vec()]);
        let err = parse(&shp, None, Some(WGS84)).unwrap_err();
        assert!(format!("{err:#}").contains("MultiPatch"), "{err:#}");
    }

    #[test]
    fn no_truncation_of_a_valid_file_can_panic() {
        // The property that matters for a parser of untrusted files: every prefix of a real file
        // is either parsed or refused, never a panic.
        let shp = shp_file(&[
            point(1.0, 2.0),
            poly(5, &[cw(0.0, 0.0, 10.0), ccw(2.0, 2.0, 2.0)]),
            poly(3, &[vec![[0.0, 0.0], [1.0, 1.0]]]),
        ]);
        let dbf = dbf_file(
            &[("NAME", b'C', 6, 0), ("N", b'N', 4, 0)],
            &[
                (false, vec!["a", "1"]),
                (false, vec!["b", "2"]),
                (false, vec!["c", "3"]),
            ],
        );
        for cut in 0..shp.len() {
            let _ = parse(&shp[..cut], Some(&dbf), Some(WGS84));
        }
        for cut in 0..dbf.len() {
            let _ = parse(&shp, Some(&dbf[..cut]), Some(WGS84));
        }
        // ...and the untruncated pair still parses.
        assert_eq!(parse(&shp, Some(&dbf), Some(WGS84)).unwrap().len(), 3);
    }
}
