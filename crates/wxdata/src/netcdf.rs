//! NetCDF export of a lat/lon scalar grid (ROADMAP_NEW M5), in the classic format (CDF-1) with
//! CF-1.8 metadata — what xarray, Panoply, NCL, GDAL and MATLAB all open, and the format the
//! atmospheric-science tools around this app expect a gridded field in.
//!
//! The classic format is a short, fully published big-endian layout (a header of dimensions,
//! attributes and variables, then each variable's data at the offset the header names), so it is
//! written here by hand rather than through the C library. One file holds:
//!
//! - a scalar `time` (seconds since the Unix epoch), the field's valid time;
//! - `lat(lat)` and `lon(lon)`, the cell *centres*, north to south and west to east — the CF
//!   coordinate variables every reader uses to place the grid;
//! - the field itself, `float32(lat, lon)`, NaN where there is no value.

use crate::mrms::MrmsField;

const NC_CHAR: u32 = 2;
const NC_FLOAT: u32 = 5;
const NC_DOUBLE: u32 = 6;
const NC_DIMENSION: u32 = 0x0A;
const NC_VARIABLE: u32 = 0x0B;
const NC_ATTRIBUTE: u32 = 0x0C;

/// An attribute value. Only text is needed: every number in the file is a variable.
enum Value<'a> {
    Text(&'a str),
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

fn put_attrs(out: &mut Vec<u8>, attrs: &[(&str, Value)]) {
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
        match value {
            Value::Text(s) => {
                put_u32(out, NC_CHAR);
                put_u32(out, s.len() as u32);
                out.extend_from_slice(s.as_bytes());
                pad4(out);
            }
        }
    }
}

/// One variable: its name, dimension ids, attributes, type and data (already big-endian).
struct Var<'a> {
    name: &'a str,
    dims: Vec<u32>,
    attrs: Vec<(&'a str, Value<'a>)>,
    kind: u32,
    data: Vec<u8>,
}

/// The header for `vars`, with each variable's data placed at the offsets in `begins`.
fn header(
    dims: &[(&str, usize)],
    globals: &[(&str, Value)],
    vars: &[Var],
    begins: &[u32],
) -> Vec<u8> {
    let mut out = b"CDF\x01".to_vec();
    put_u32(&mut out, 0); // numrecs: no record dimension
    put_u32(&mut out, NC_DIMENSION);
    put_u32(&mut out, dims.len() as u32);
    for (name, len) in dims {
        put_name(&mut out, name);
        put_u32(&mut out, *len as u32);
    }
    put_attrs(&mut out, globals);
    put_u32(&mut out, NC_VARIABLE);
    put_u32(&mut out, vars.len() as u32);
    for (v, begin) in vars.iter().zip(begins) {
        put_name(&mut out, v.name);
        put_u32(&mut out, v.dims.len() as u32);
        for d in &v.dims {
            put_u32(&mut out, *d);
        }
        put_attrs(&mut out, &v.attrs);
        put_u32(&mut out, v.kind);
        put_u32(&mut out, v.data.len().next_multiple_of(4) as u32); // vsize
        put_u32(&mut out, *begin);
    }
    out
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
    if nx == 0 || ny == 0 || field.values.len() != nx * ny || nx * ny * 4 >= (1 << 31) {
        return None;
    }
    let dx = (field.lon_east - field.lon_west) / nx as f64;
    let dy = (field.lat_north - field.lat_south) / ny as f64;
    let lats: Vec<u8> = (0..ny)
        .flat_map(|j| (field.lat_north - (j as f64 + 0.5) * dy).to_be_bytes())
        .collect();
    let lons: Vec<u8> = (0..nx)
        .flat_map(|i| (field.lon_west + (i as f64 + 0.5) * dx).to_be_bytes())
        .collect();
    let values: Vec<u8> = field.values.iter().flat_map(|v| v.to_be_bytes()).collect();
    let time = field.time.timestamp() as f64;
    let valid = field.time.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let history = format!("{valid} valid time; written by HookEcho");

    let dims = [("lat", ny), ("lon", nx)];
    let globals = [
        ("Conventions", Value::Text("CF-1.8")),
        ("title", Value::Text(long_name)),
        ("source", Value::Text(source)),
        ("history", Value::Text(&history)),
    ];
    let mut field_attrs = vec![("long_name", Value::Text(long_name))];
    if let Some(u) = units {
        field_attrs.push(("units", Value::Text(u)));
    }
    field_attrs.push((
        "comment",
        Value::Text("NaN where there is no value; lat/lon are cell centres"),
    ));
    // The scalar valid time, attached the CF way.
    field_attrs.push(("coordinates", Value::Text("time")));
    let vars = [
        Var {
            name: "time",
            dims: Vec::new(),
            attrs: vec![
                ("standard_name", Value::Text("time")),
                (
                    "units",
                    Value::Text("seconds since 1970-01-01 00:00:00 UTC"),
                ),
                ("calendar", Value::Text("standard")),
            ],
            kind: NC_DOUBLE,
            data: time.to_be_bytes().to_vec(),
        },
        Var {
            name: "lat",
            dims: vec![0],
            attrs: vec![
                ("standard_name", Value::Text("latitude")),
                ("units", Value::Text("degrees_north")),
                ("axis", Value::Text("Y")),
            ],
            kind: NC_DOUBLE,
            data: lats,
        },
        Var {
            name: "lon",
            dims: vec![1],
            attrs: vec![
                ("standard_name", Value::Text("longitude")),
                ("units", Value::Text("degrees_east")),
                ("axis", Value::Text("X")),
            ],
            kind: NC_DOUBLE,
            data: lons,
        },
        Var {
            name,
            dims: vec![0, 1],
            attrs: field_attrs,
            kind: NC_FLOAT,
            data: values,
        },
    ];
    // The header's length does not depend on the offsets' values, only their count, so lay it out
    // once to measure it, then again with the real offsets.
    let len = header(&dims, &globals, &vars, &[0; 4]).len();
    let mut begins = [0u32; 4];
    let mut at = len;
    for (b, v) in begins.iter_mut().zip(&vars) {
        *b = at as u32;
        at += v.data.len().next_multiple_of(4);
    }
    let mut out = header(&dims, &globals, &vars, &begins);
    for v in &vars {
        out.extend_from_slice(&v.data);
        pad4(&mut out);
    }
    Some(out)
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
