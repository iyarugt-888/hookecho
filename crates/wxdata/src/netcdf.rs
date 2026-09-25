//! NetCDF export (ROADMAP_NEW M5), in the classic format (CDF-1) — what xarray, Panoply, NCL,
//! GDAL, MATLAB and Py-ART all open. The classic format is a short, fully published big-endian
//! layout (a header of dimensions, attributes and variables, then each variable's data at the
//! offset the header names), so it is written here by hand rather than through the C library.
//!
//! [`NcFile`] is the general writer, shared with [`crate::cfradial`]; [`write`] is a lat/lon
//! scalar grid with CF-1.8 metadata:
//!
//! - a scalar `time` (seconds since the Unix epoch), the field's valid time;
//! - `lat(lat)` and `lon(lon)`, the cell *centres*, north to south and west to east — the CF
//!   coordinate variables every reader uses to place the grid;
//! - the field itself, `float32(lat, lon)`, NaN where there is no value.

use crate::mrms::MrmsField;

const NC_BYTE: u32 = 1;
const NC_CHAR: u32 = 2;
const NC_INT: u32 = 4;
const NC_FLOAT: u32 = 5;
const NC_DOUBLE: u32 = 6;
const NC_DIMENSION: u32 = 0x0A;
const NC_VARIABLE: u32 = 0x0B;
const NC_ATTRIBUTE: u32 = 0x0C;

/// An attribute value.
#[derive(Debug, Clone)]
pub(crate) enum Attr {
    Text(String),
    Byte(i8),
    Float(f32),
}

/// A variable's values, in the type it is stored as.
#[derive(Debug, Clone)]
pub(crate) enum Data {
    /// Characters, for a fixed-width string variable.
    Char(Vec<u8>),
    Byte(Vec<i8>),
    Int(Vec<i32>),
    Float(Vec<f32>),
    Double(Vec<f64>),
}

impl Data {
    fn kind(&self) -> u32 {
        match self {
            Data::Char(_) => NC_CHAR,
            Data::Byte(_) => NC_BYTE,
            Data::Int(_) => NC_INT,
            Data::Float(_) => NC_FLOAT,
            Data::Double(_) => NC_DOUBLE,
        }
    }

    fn byte_len(&self) -> usize {
        match self {
            Data::Char(v) => v.len(),
            Data::Byte(v) => v.len(),
            Data::Int(v) => v.len() * 4,
            Data::Float(v) => v.len() * 4,
            Data::Double(v) => v.len() * 8,
        }
    }

    fn put(&self, out: &mut Vec<u8>) {
        match self {
            Data::Char(v) => out.extend_from_slice(v),
            Data::Byte(v) => out.extend(v.iter().map(|b| *b as u8)),
            Data::Int(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_be_bytes())),
            Data::Float(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_be_bytes())),
            Data::Double(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_be_bytes())),
        }
    }
}

/// One variable: its name, the indices of its dimensions in [`NcFile::dims`], its attributes and
/// its data (row-major over those dimensions).
#[derive(Debug, Clone)]
pub(crate) struct Var {
    pub name: String,
    pub dims: Vec<usize>,
    pub attrs: Vec<(String, Attr)>,
    pub data: Data,
}

/// A whole classic-format file: dimensions, global attributes and variables.
#[derive(Debug, Clone, Default)]
pub(crate) struct NcFile {
    pub dims: Vec<(String, usize)>,
    pub globals: Vec<(String, Attr)>,
    pub vars: Vec<Var>,
}

/// Shorthand for a list of text attributes.
pub(crate) fn texts(pairs: &[(&str, &str)]) -> Vec<(String, Attr)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), Attr::Text(v.to_string())))
        .collect()
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn pad4(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    put_u32(out, name.len() as u32);
    out.extend_from_slice(name.as_bytes());
    pad4(out);
}

fn put_attrs(out: &mut Vec<u8>, attrs: &[(String, Attr)]) {
    if attrs.is_empty() {
        // ABSENT: a zero tag and a zero count.
        put_u32(out, 0);
        put_u32(out, 0);
        return;
    }
    put_u32(out, NC_ATTRIBUTE);
    put_u32(out, attrs.len() as u32);
    for (name, value) in attrs {
        put_name(out, name);
        let (kind, bytes): (u32, Vec<u8>) = match value {
            Attr::Text(s) => (NC_CHAR, s.as_bytes().to_vec()),
            Attr::Byte(b) => (NC_BYTE, vec![*b as u8]),
            Attr::Float(f) => (NC_FLOAT, f.to_be_bytes().to_vec()),
        };
        let count = if kind == NC_CHAR { bytes.len() } else { 1 };
        put_u32(out, kind);
        put_u32(out, count as u32);
        out.extend_from_slice(&bytes);
        pad4(out);
    }
}

impl NcFile {
    /// The header, with each variable's data placed at the offsets in `begins`.
    fn header(&self, begins: &[u32]) -> Vec<u8> {
        let mut out = b"CDF\x01".to_vec();
        put_u32(&mut out, 0); // numrecs: no record dimension
        put_u32(&mut out, NC_DIMENSION);
        put_u32(&mut out, self.dims.len() as u32);
        for (name, len) in &self.dims {
            put_name(&mut out, name);
            put_u32(&mut out, *len as u32);
        }
        put_attrs(&mut out, &self.globals);
        put_u32(&mut out, NC_VARIABLE);
        put_u32(&mut out, self.vars.len() as u32);
        for (v, begin) in self.vars.iter().zip(begins) {
            put_name(&mut out, &v.name);
            put_u32(&mut out, v.dims.len() as u32);
            for d in &v.dims {
                put_u32(&mut out, *d as u32);
            }
            put_attrs(&mut out, &v.attrs);
            put_u32(&mut out, v.data.kind());
            put_u32(&mut out, v.data.byte_len().next_multiple_of(4) as u32); // vsize
            put_u32(&mut out, *begin);
        }
        out
    }

    /// The file's bytes, or `None` when a variable's data does not match its dimensions or the
    /// whole would pass the classic format's 2 GiB of 32-bit offsets.
    pub(crate) fn to_bytes(&self) -> Option<Vec<u8>> {
        for v in &self.vars {
            let want: usize = v
                .dims
                .iter()
                .map(|d| self.dims.get(*d).map_or(0, |d| d.1))
                .product();
            let have = match &v.data {
                Data::Char(x) => x.len(),
                Data::Byte(x) => x.len(),
                Data::Int(x) => x.len(),
                Data::Float(x) => x.len(),
                Data::Double(x) => x.len(),
            };
            if want != have {
                return None;
            }
        }
        // The header's length depends on the offsets' count, not their values: measure once.
        let len = self.header(&vec![0; self.vars.len()]).len();
        let mut begins = Vec::with_capacity(self.vars.len());
        let mut at = len;
        for v in &self.vars {
            begins.push(u32::try_from(at).ok().filter(|b| *b < (1 << 31))?);
            at += v.data.byte_len().next_multiple_of(4);
        }
        if at >= 1 << 31 {
            return None;
        }
        let mut out = self.header(&begins);
        out.reserve(at - out.len());
        for v in &self.vars {
            v.data.put(&mut out);
            pad4(&mut out);
        }
        Some(out)
    }
}

/// The bytes of a CF-1.8 NetCDF classic file of `field`: the grid as variable `name`
/// (letters, digits and underscores), described by `long_name` and, when known, `units`.
/// `None` for an empty or inconsistent grid, or one too big for the classic format's 2 GiB.
pub fn write(
    field: &MrmsField,
    name: &str,
    long_name: &str,
    units: Option<&str>,
    source: &str,
) -> Option<Vec<u8>> {
    let (nx, ny) = (field.nx, field.ny);
    if nx == 0 || ny == 0 || field.values.len() != nx * ny {
        return None;
    }
    let dx = (field.lon_east - field.lon_west) / nx as f64;
    let dy = (field.lat_north - field.lat_south) / ny as f64;
    let valid = field.time.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut field_attrs = texts(&[("long_name", long_name)]);
    if let Some(u) = units {
        field_attrs.extend(texts(&[("units", u)]));
    }
    field_attrs.extend(texts(&[
        (
            "comment",
            "NaN where there is no value; lat/lon are cell centres",
        ),
        // The scalar valid time, attached the CF way.
        ("coordinates", "time"),
    ]));
    NcFile {
        dims: vec![("lat".into(), ny), ("lon".into(), nx)],
        globals: texts(&[
            ("Conventions", "CF-1.8"),
            ("title", long_name),
            ("source", source),
            (
                "history",
                &format!("{valid} valid time; written by HookEcho"),
            ),
        ]),
        vars: vec![
            Var {
                name: "time".into(),
                dims: Vec::new(),
                attrs: texts(&[
                    ("standard_name", "time"),
                    ("units", "seconds since 1970-01-01 00:00:00 UTC"),
                    ("calendar", "standard"),
                ]),
                data: Data::Double(vec![field.time.timestamp() as f64]),
            },
            Var {
                name: "lat".into(),
                dims: vec![0],
                attrs: texts(&[
                    ("standard_name", "latitude"),
                    ("units", "degrees_north"),
                    ("axis", "Y"),
                ]),
                data: Data::Double(
                    (0..ny)
                        .map(|j| field.lat_north - (j as f64 + 0.5) * dy)
                        .collect(),
                ),
            },
            Var {
                name: "lon".into(),
                dims: vec![1],
                attrs: texts(&[
                    ("standard_name", "longitude"),
                    ("units", "degrees_east"),
                    ("axis", "X"),
                ]),
                data: Data::Double(
                    (0..nx)
                        .map(|i| field.lon_west + (i as f64 + 0.5) * dx)
                        .collect(),
                ),
            },
            Var {
                name: name.to_string(),
                dims: vec![0, 1],
                attrs: field_attrs,
                data: Data::Float(field.values.clone()),
            },
        ],
    }
    .to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader for exactly what [`write`] produces, written from the format specification: the
    /// header walk, then each variable's data at its own offset.
    struct Read<'a> {
        b: &'a [u8],
        at: usize,
    }
    impl Read<'_> {
        fn u32(&mut self) -> u32 {
            let v = u32::from_be_bytes(self.b[self.at..self.at + 4].try_into().unwrap());
            self.at += 4;
            v
        }
        fn name(&mut self) -> String {
            let n = self.u32() as usize;
            let s = String::from_utf8(self.b[self.at..self.at + n].to_vec()).unwrap();
            self.at += n.next_multiple_of(4);
            s
        }
        fn attrs(&mut self) -> Vec<(String, String)> {
            let (tag, n) = (self.u32(), self.u32());
            assert!(tag == NC_ATTRIBUTE || (tag == 0 && n == 0));
            (0..n)
                .map(|_| {
                    let name = self.name();
                    let (kind, count) = (self.u32(), self.u32() as usize);
                    let text = match kind {
                        NC_CHAR => {
                            let s = String::from_utf8(self.b[self.at..self.at + count].to_vec())
                                .unwrap();
                            self.at += count.next_multiple_of(4);
                            s
                        }
                        NC_DOUBLE => {
                            let d = f64::from_be_bytes(
                                self.b[self.at..self.at + 8].try_into().unwrap(),
                            );
                            self.at += 8;
                            d.to_string()
                        }
                        k => panic!("attribute type {k}"),
                    };
                    (name, text)
                })
                .collect()
        }
    }

    fn field() -> MrmsField {
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
    fn the_file_walks_by_the_spec_and_holds_the_grid_and_its_coordinates() {
        let bytes = write(
            &field(),
            "mesh",
            "Maximum expected hail size",
            Some("mm"),
            "t",
        )
        .unwrap();
        let mut r = Read { b: &bytes, at: 0 };
        assert_eq!(&bytes[..4], b"CDF\x01");
        r.at = 4;
        assert_eq!(r.u32(), 0, "no records");
        assert_eq!((r.u32(), r.u32()), (NC_DIMENSION, 2));
        assert_eq!((r.name(), r.u32()), ("lat".into(), 3));
        assert_eq!((r.name(), r.u32()), ("lon".into(), 4));
        let globals = r.attrs();
        assert!(globals.contains(&("Conventions".into(), "CF-1.8".into())));
        assert_eq!((r.u32(), r.u32()), (NC_VARIABLE, 4));
        let mut vars = Vec::new();
        for _ in 0..4 {
            let name = r.name();
            let dims: Vec<u32> = (0..r.u32()).map(|_| r.u32()).collect();
            let attrs = r.attrs();
            let (kind, vsize, begin) = (r.u32(), r.u32() as usize, r.u32() as usize);
            vars.push((name, dims, attrs, kind, vsize, begin));
        }
        let names: Vec<&str> = vars.iter().map(|v| v.0.as_str()).collect();
        assert_eq!(names, ["time", "lat", "lon", "mesh"]);
        let doubles = |begin: usize, n: usize| -> Vec<f64> {
            (0..n)
                .map(|i| {
                    f64::from_be_bytes(bytes[begin + i * 8..begin + i * 8 + 8].try_into().unwrap())
                })
                .collect()
        };
        assert_eq!(doubles(vars[0].5, 1)[0], 1_369_080_480.0, "valid time");
        let lat = doubles(vars[1].5, 3);
        assert!(
            (lat[0] - 35.9).abs() < 1e-9 && (lat[2] - 35.5).abs() < 1e-9,
            "{lat:?}"
        );
        let lon = doubles(vars[2].5, 4);
        assert!(
            (lon[0] + 97.875).abs() < 1e-9 && (lon[3] + 97.125).abs() < 1e-9,
            "{lon:?}"
        );
        let mesh = &vars[3];
        assert_eq!(mesh.1, vec![0, 1], "(lat, lon)");
        assert!(mesh.2.contains(&("units".into(), "mm".into())));
        assert_eq!(mesh.3, NC_FLOAT);
        let v: Vec<f32> = (0..12)
            .map(|i| {
                f32::from_be_bytes(
                    bytes[mesh.5 + i * 4..mesh.5 + i * 4 + 4]
                        .try_into()
                        .unwrap(),
                )
            })
            .collect();
        assert!(v[5].is_nan() && v[11] == 16.5 && v[0] == 0.0);
        // The data runs to the end of the file, and nothing overlaps.
        assert_eq!(mesh.5 + mesh.4, bytes.len());
        assert!(vars.windows(2).all(|w| w[0].5 + w[0].4 <= w[1].5));
    }

    #[test]
    fn an_empty_or_inconsistent_grid_is_not_written() {
        let mut f = field();
        f.values.pop();
        assert!(write(&f, "x", "x", None, "").is_none());
    }
}

/// A reader for classic-format files written from the format specification rather than from the
/// writer above, for tests: the header walk, then each variable's data at its own offset.
#[cfg(test)]
pub(crate) mod spec_read {
    use std::collections::HashMap;

    /// One variable as the header describes it.
    #[derive(Debug, Clone)]
    pub struct VarInfo {
        pub dims: Vec<usize>,
        /// Attributes rendered as text (numbers with `to_string`).
        pub attrs: HashMap<String, String>,
        pub kind: u32,
        pub vsize: usize,
        pub begin: usize,
    }

    /// A parsed file: dimensions (name, length), global attributes and variables, in order.
    #[derive(Debug, Clone)]
    pub struct Parsed {
        pub dims: Vec<(String, usize)>,
        pub globals: HashMap<String, String>,
        pub vars: Vec<(String, VarInfo)>,
    }

    impl Parsed {
        pub fn var(&self, name: &str) -> &VarInfo {
            &self.vars.iter().find(|(n, _)| n == name).expect(name).1
        }
    }

    struct Cur<'a> {
        b: &'a [u8],
        at: usize,
    }

    impl Cur<'_> {
        fn u32(&mut self) -> u32 {
            let v = u32::from_be_bytes(self.b[self.at..self.at + 4].try_into().unwrap());
            self.at += 4;
            v
        }
        fn name(&mut self) -> String {
            let n = self.u32() as usize;
            let s = String::from_utf8(self.b[self.at..self.at + n].to_vec()).unwrap();
            self.at += n.next_multiple_of(4);
            s
        }
        fn attrs(&mut self) -> HashMap<String, String> {
            let (tag, n) = (self.u32(), self.u32());
            assert!(
                tag == 0x0C || (tag == 0 && n == 0),
                "attribute list tag {tag}"
            );
            (0..n)
                .map(|_| {
                    let name = self.name();
                    let (kind, count) = (self.u32(), self.u32() as usize);
                    let size = [0, 1, 1, 2, 4, 4, 8][kind as usize] * count;
                    let raw = &self.b[self.at..self.at + size];
                    self.at += size.next_multiple_of(4);
                    let text = match kind {
                        1 => (raw[0] as i8).to_string(),
                        2 => String::from_utf8(raw.to_vec()).unwrap(),
                        4 => i32::from_be_bytes(raw.try_into().unwrap()).to_string(),
                        5 => f32::from_be_bytes(raw.try_into().unwrap()).to_string(),
                        6 => f64::from_be_bytes(raw.try_into().unwrap()).to_string(),
                        k => panic!("attribute type {k}"),
                    };
                    (name, text)
                })
                .collect()
        }
    }

    pub fn parse(b: &[u8]) -> Parsed {
        assert_eq!(&b[..4], b"CDF\x01", "classic format magic");
        let mut c = Cur { b, at: 4 };
        assert_eq!(c.u32(), 0, "no records");
        let (tag, nd) = (c.u32(), c.u32());
        assert_eq!(tag, 0x0A);
        let dims = (0..nd).map(|_| (c.name(), c.u32() as usize)).collect();
        let globals = c.attrs();
        let (tag, nv) = (c.u32(), c.u32());
        assert_eq!(tag, 0x0B);
        let vars = (0..nv)
            .map(|_| {
                let name = c.name();
                let n = c.u32();
                let dims = (0..n).map(|_| c.u32() as usize).collect();
                let attrs = c.attrs();
                let (kind, vsize, begin) = (c.u32(), c.u32() as usize, c.u32() as usize);
                (
                    name,
                    VarInfo {
                        dims,
                        attrs,
                        kind,
                        vsize,
                        begin,
                    },
                )
            })
            .collect();
        Parsed {
            dims,
            globals,
            vars,
        }
    }
}
