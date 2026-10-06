//! KML and KMZ import (ROADMAP_NEW I1 steps 3 and 4), into the same [`GisFeature`] GeoJSON and
//! Shapefile import produce, so drawing, click popups, zoom-to-fit, remembering and export all
//! work on it unchanged.
//!
//! KML is XML, and this reads it the way the rest of the crate reads the XML it meets (S3
//! listings, TFR details): by tag, without a general XML parser. That is enough for the shapes
//! KML can carry — every `Placemark`'s `Point`, `LineString`, `LinearRing` and `Polygon` (with
//! holes), however deeply a `MultiGeometry` or `Folder` nests them — plus its `name`,
//! `description` and `ExtendedData` (`Data`/`value` pairs and schema `SimpleData`) as attributes.
//! Namespace prefixes (`kml:Placemark`), comments, CDATA sections and the five predefined
//! entities are handled. Styles, overlays, tours and `gx:` tracks are not read: they are display
//! hints, not shapes. Network links are never fetched; inside a KMZ, a link to another KML of the
//! same archive is read as part of it (see [`parse_kmz`]). KML coordinates are WGS 84 by
//! definition, so there is nothing to reproject.
//!
//! A KMZ is a zip archive holding a KML (conventionally `doc.kml`) and its icons. [`parse_kmz`]
//! finds that KML with the crate's small [`crate::zip`] reader, which checks every offset and
//! length against the archive and caps the inflated size so a hostile archive cannot drive an
//! unbounded allocation.

use crate::gis::{Geometry, GisFeature};

/// Every placemark shape in a KML document.
pub fn parse(kml: &str) -> anyhow::Result<Vec<GisFeature>> {
    let doc = clean(kml);
    anyhow::ensure!(
        doc.contains("<kml") || doc.contains("<Placemark") || doc.contains("<Document"),
        "not a KML document"
    );
    let mut out = Vec::new();
    for placemark in elements(&doc, "Placemark") {
        let mut properties = serde_json::Map::new();
        if let Some(name) = first_text(placemark, "name") {
            properties.insert("name".into(), name.into());
        }
        if let Some(d) = first_text(placemark, "description") {
            properties.insert("description".into(), d.into());
        }
        for (k, v) in extended_data(placemark) {
            properties.entry(k).or_insert(v.into());
        }
        for geometry in geometries(placemark) {
            out.push(GisFeature {
                geometry,
                properties: properties.clone(),
            });
        }
    }
    Ok(out)
}

/// Every placemark shape in a KMZ archive: its main KML, and the KML files inside the same
/// archive that it links to with a `NetworkLink` (how GDAL's LIBKML and other writers lay out a
/// multi-layer KMZ: a `doc.kml` that only links `layers/*.kml`). A link is followed only to an
/// entry of this archive by a relative path; one to anywhere else (`http:`, an absolute path,
/// `..`) is not fetched. Linked files are followed two levels deep, 64 files at most, each read
/// once.
pub fn parse_kmz(bytes: &[u8]) -> anyhow::Result<Vec<GisFeature>> {
    const MAX_DEPTH: usize = 2;
    const MAX_LINKED: usize = 64;
    let entries = crate::zip::entries(bytes)?;
    let root = main_kml(&entries)?;
    let mut out = Vec::new();
    let mut seen: Vec<String> = vec![root.name.clone()];
    let mut queue: Vec<(String, usize)> = vec![(root.name.clone(), 0)];
    while let Some((name, depth)) = queue.pop() {
        let Some(entry) = entries.iter().find(|e| e.name == name) else {
            continue;
        };
        let text = text_of(crate::zip::read(bytes, entry, MAX_KML_BYTES)?);
        let doc = clean(&text);
        // A linked file that is not KML (an image, say) is skipped, not an error.
        if depth == 0 || doc.contains("<kml") || doc.contains("<Placemark") {
            out.extend(parse(&text)?);
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        let dir = name.rsplit_once('/').map_or("", |(d, _)| d);
        for link in elements(&doc, "NetworkLink") {
            let Some(href) = first_text(link, "href") else {
                continue;
            };
            let Some(target) = archive_path(dir, href.trim()) else {
                continue;
            };
            if seen.contains(&target) || !entries.iter().any(|e| e.name == target) {
                continue;
            }
            anyhow::ensure!(
                seen.len() <= MAX_LINKED,
                "the KMZ links more than {MAX_LINKED} KML files"
            );
            seen.push(target.clone());
            queue.push((target, depth + 1));
        }
    }
    Ok(out)
}

/// A `NetworkLink` href as a path inside the archive, relative to the linking file's directory;
/// `None` for anything that would leave the archive.
fn archive_path(dir: &str, href: &str) -> Option<String> {
    if href.is_empty() || href.contains("://") || href.starts_with('/') || href.contains('\\') {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => return None,
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// Is this file a zip archive (a KMZ), judging by its first bytes rather than its name?
pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

// ---- KML ---------------------------------------------------------------------------------------

/// The document with comments removed, CDATA unwrapped (its text escaped, so the tag scan
/// cannot mistake markup inside it for elements) and namespace prefixes dropped from tag names.
fn clean(kml: &str) -> String {
    let mut s = String::with_capacity(kml.len());
    let mut rest = kml;
    while let Some(i) = rest.find('<') {
        s.push_str(&rest[..i]);
        rest = &rest[i..];
        if let Some(body) = rest.strip_prefix("<!--") {
            rest = body.find("-->").map_or("", |j| &body[j + 3..]);
        } else if let Some(body) = rest.strip_prefix("<![CDATA[") {
            let end = body.find("]]>").unwrap_or(body.len());
            s.push_str(
                &body[..end]
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;"),
            );
            rest = body.get(end + 3..).unwrap_or("");
        } else {
            // A tag: copy it, minus any `prefix:` on its name.
            let end = rest.find('>').map_or(rest.len(), |j| j + 1);
            let tag = &rest[..end];
            let (open, name_start) = if tag.starts_with("</") {
                ("</", 2)
            } else {
                ("<", 1)
            };
            let body = &tag[name_start..];
            let name_end = body
                .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .unwrap_or(body.len());
            match body[..name_end].rfind(':') {
                Some(colon) if !body.starts_with('?') && !body.starts_with('!') => {
                    s.push_str(open);
                    s.push_str(&body[colon + 1..]);
                }
                _ => s.push_str(tag),
            }
            rest = &rest[end..];
        }
    }
    s.push_str(rest);
    s
}

/// Where the next `<name …>` element starts at or after `from`: the index of its `<`, and of the
/// first byte after its opening tag. A self-closing `<name/>` has no content, so it is skipped.
fn open_tag(s: &str, name: &str, mut from: usize) -> Option<(usize, usize)> {
    let pat = format!("<{name}");
    loop {
        let i = from + s.get(from..)?.find(&pat)?;
        let after = i + pat.len();
        let next = s[after..].chars().next()?;
        if next == '>' || next.is_whitespace() || next == '/' {
            let close = after + s[after..].find('>')?;
            if s[..close].ends_with('/') {
                from = close;
                continue;
            }
            return Some((i, close + 1));
        }
        from = after;
    }
}

/// The content of every `<name>` element in `s`, outermost first, in document order. Elements
/// of the same name nested in each other (a `Folder` in a `Folder`) are matched by depth.
fn elements<'a>(s: &'a str, name: &str) -> Vec<&'a str> {
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some((_, content)) = open_tag(s, name, from) {
        let mut depth = 1;
        let mut at = content;
        let end = loop {
            let next_close = s[at..].find(&close).map(|j| at + j);
            let next_open = open_tag(s, name, at).map(|(i, _)| i);
            match (next_open, next_close) {
                (Some(o), Some(c)) if o < c => {
                    depth += 1;
                    at = o + 1;
                }
                (_, Some(c)) => {
                    depth -= 1;
                    if depth == 0 {
                        break Some(c);
                    }
                    at = c + close.len();
                }
                (_, None) => break None,
            }
        };
        let Some(end) = end else { break };
        out.push(&s[content..end]);
        from = end + close.len();
    }
    out
}

/// The text of the first `<name>` element, entities decoded and trimmed; `None` when absent or
/// empty.
fn first_text(s: &str, name: &str) -> Option<String> {
    let text = decode(elements(s, name).first()?.trim());
    (!text.is_empty()).then_some(text)
}

/// The value of an attribute in the opening tag that ends just before `content_start`.
fn attribute(s: &str, tag_start: usize, content_start: usize, attr: &str) -> Option<String> {
    let tag = &s[tag_start..content_start];
    let pat = format!("{attr}=");
    let i = tag.find(&pat)? + pat.len();
    let quote = tag[i..].chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let body = &tag[i + 1..];
    Some(decode(&body[..body.find(quote)?]))
}

/// A placemark's `ExtendedData`: `<Data name="k"><value>v</value></Data>` and schema
/// `<SimpleData name="k">v</SimpleData>` alike.
fn extended_data(placemark: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for ed in elements(placemark, "ExtendedData") {
        for (tag, value_in) in [("Data", true), ("SimpleData", false)] {
            let close = format!("</{tag}>");
            let mut from = 0;
            while let Some((start, content)) = open_tag(ed, tag, from) {
                let Some(end) = ed[content..].find(&close).map(|j| content + j) else {
                    break;
                };
                if let Some(key) = attribute(ed, start, content, "name") {
                    let body = &ed[content..end];
                    let value = if value_in {
                        first_text(body, "value").unwrap_or_default()
                    } else {
                        decode(body.trim())
                    };
                    out.push((key, value));
                }
                from = end + close.len();
            }
        }
    }
    out
}

/// The five predefined XML entities and numeric character references.
fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(semi) = rest.find(';').filter(|&j| j <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A `<coordinates>` list: whitespace-separated `lon,lat[,alt]` tuples. A tuple that does not
/// read as a plausible longitude/latitude is dropped rather than drawn somewhere wrong.
fn coordinates(s: &str) -> Vec<[f64; 2]> {
    let Some(text) = elements(s, "coordinates").into_iter().next() else {
        return Vec::new();
    };
    text.split_whitespace()
        .filter_map(|t| {
            let mut it = t.split(',').map(|v| v.trim().parse::<f64>());
            let lon = it.next()?.ok()?;
            let lat = it.next()?.ok()?;
            ((-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat)).then_some([lon, lat])
        })
        .collect()
}

/// Every shape in a placemark, in the order polygons, lines, points. A `Polygon`'s rings are
/// taken out of the text before lines are looked for, so a polygon boundary is not also read as
/// a line.
fn geometries(placemark: &str) -> Vec<Geometry> {
    let mut out = Vec::new();
    let mut rest = placemark.to_string();
    for poly in elements(placemark, "Polygon") {
        let mut rings = Vec::new();
        for outer in elements(poly, "outerBoundaryIs") {
            rings.push(coordinates(outer));
        }
        rings.truncate(1);
        for inner in elements(poly, "innerBoundaryIs") {
            for ring in elements(inner, "LinearRing") {
                rings.push(coordinates(ring));
            }
        }
        if rings.first().is_some_and(|r| r.len() >= 3) {
            rings.retain(|r| r.len() >= 3);
            out.push(Geometry::Polygon(rings));
        }
        rest = rest.replacen(poly, "", 1);
    }
    for tag in ["LineString", "LinearRing"] {
        for line in elements(&rest, tag) {
            let pts = coordinates(line);
            if pts.len() >= 2 {
                out.push(Geometry::LineString(pts));
            }
        }
    }
    for point in elements(&rest, "Point") {
        if let Some(&p) = coordinates(point).first() {
            out.push(Geometry::Point(p));
        }
    }
    out
}

// ---- KMZ ---------------------------------------------------------------------------------------

/// The largest KML a KMZ may inflate to.
const MAX_KML_BYTES: u64 = 256 * 1024 * 1024;

/// The KML inside a KMZ: `doc.kml` at the top level if there is one, else the first `.kml`
/// entry, as KMZ readers do.
pub fn kml_of_kmz(zip: &[u8]) -> anyhow::Result<String> {
    let entries = crate::zip::entries(zip)?;
    let kml = main_kml(&entries)?;
    Ok(text_of(crate::zip::read(zip, kml, MAX_KML_BYTES)?))
}

/// The archive's main KML: `doc.kml`, else the first `.kml` entry.
fn main_kml(entries: &[crate::zip::Entry]) -> anyhow::Result<&crate::zip::Entry> {
    entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case("doc.kml"))
        .or_else(|| {
            entries
                .iter()
                .find(|e| e.name.to_ascii_lowercase().ends_with(".kml"))
        })
        .ok_or_else(|| anyhow::anyhow!("the KMZ holds no .kml file"))
}

fn text_of(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>Chase plan</name>
  <!-- a comment with <Placemark> inside must not count -->
  <Folder><name>Targets</name>
    <Folder>
      <Placemark>
        <name>Target &amp; staging</name>
        <description><![CDATA[Meet at <b>the gas station</b>]]></description>
        <ExtendedData>
          <Data name="priority"><value>1</value></Data>
          <SchemaData schemaUrl="#s"><SimpleData name="county">Grady</SimpleData></SchemaData>
        </ExtendedData>
        <Point><coordinates>-97.94,35.05,0</coordinates></Point>
      </Placemark>
    </Folder>
  </Folder>
  <Placemark>
    <name>Route</name>
    <LineString><tessellate>1</tessellate>
      <coordinates>
        -97.5,35.2,0 -97.6,35.3,0
        -97.7,35.4,0
      </coordinates>
    </LineString>
  </Placemark>
  <Placemark>
    <name>Area</name>
    <MultiGeometry>
      <Polygon>
        <outerBoundaryIs><LinearRing><coordinates>0,0 4,0 4,4 0,4 0,0</coordinates></LinearRing></outerBoundaryIs>
        <innerBoundaryIs><LinearRing><coordinates>1,1 2,1 2,2 1,1</coordinates></LinearRing></innerBoundaryIs>
      </Polygon>
      <Point><coordinates>2,2</coordinates></Point>
    </MultiGeometry>
  </Placemark>
  <Placemark><name>No shape</name></Placemark>
</Document>
</kml>"##;

    #[test]
    fn reads_every_placemark_shape_with_its_attributes() {
        let f = parse(DOC).unwrap();
        assert_eq!(f.len(), 4, "{f:#?}");
        assert_eq!(f[0].geometry, Geometry::Point([-97.94, 35.05]));
        assert_eq!(f[0].properties["name"], "Target & staging");
        assert_eq!(
            f[0].properties["description"],
            "Meet at <b>the gas station</b>"
        );
        assert_eq!(f[0].properties["priority"], "1");
        assert_eq!(f[0].properties["county"], "Grady");
        assert_eq!(
            f[1].geometry,
            Geometry::LineString(vec![[-97.5, 35.2], [-97.6, 35.3], [-97.7, 35.4]])
        );
        let Geometry::Polygon(rings) = &f[2].geometry else {
            panic!("{:?}", f[2].geometry)
        };
        assert_eq!(rings.len(), 2, "outer ring and one hole");
        assert_eq!(rings[1][0], [1.0, 1.0]);
        assert_eq!(f[3].geometry, Geometry::Point([2.0, 2.0]));
        assert_eq!(
            f[3].properties["name"], "Area",
            "a MultiGeometry's parts share it"
        );
    }

    #[test]
    fn namespace_prefixes_and_entities_are_handled() {
        let doc = r#"<kml:kml xmlns:kml="http://www.opengis.net/kml/2.2"><kml:Placemark>
            <kml:name>A &lt;b&gt; &#233;t&#xE9;</kml:name>
            <kml:Point><kml:coordinates>10,20</kml:coordinates></kml:Point>
            </kml:Placemark></kml:kml>"#;
        let f = parse(doc).unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].properties["name"], "A <b> été");
        assert_eq!(f[0].geometry, Geometry::Point([10.0, 20.0]));
    }

    #[test]
    fn implausible_coordinates_and_non_kml_are_refused() {
        let doc = "<kml><Placemark><Point><coordinates>500000,4000000</coordinates></Point>\
                   </Placemark></kml>";
        assert!(parse(doc).unwrap().is_empty());
        assert!(parse("{\"type\": \"FeatureCollection\"}").is_err());
    }

    /// A one-entry zip archive, stored or deflated.
    fn zip_of(name: &str, body: &[u8], deflate: bool) -> Vec<u8> {
        use std::io::Write;
        let data = if deflate {
            let mut e =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(body).unwrap();
            e.finish().unwrap()
        } else {
            body.to_vec()
        };
        let method: u16 = if deflate { 8 } else { 0 };
        let mut z = Vec::new();
        z.extend_from_slice(b"PK\x03\x04");
        z.extend_from_slice(&[20, 0, 0, 0]);
        z.extend_from_slice(&method.to_le_bytes());
        z.extend_from_slice(&[0; 8]); // time, date, crc
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(body.len() as u32).to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&[0, 0]);
        z.extend_from_slice(name.as_bytes());
        z.extend_from_slice(&data);
        let cd = z.len();
        z.extend_from_slice(b"PK\x01\x02");
        z.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
        z.extend_from_slice(&method.to_le_bytes());
        z.extend_from_slice(&[0; 8]);
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(body.len() as u32).to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&[0; 12]); // extra, comment, disk, attrs
        z.extend_from_slice(&0u32.to_le_bytes()); // local header offset
        z.extend_from_slice(name.as_bytes());
        let cd_len = z.len() - cd;
        z.extend_from_slice(b"PK\x05\x06");
        z.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
        z.extend_from_slice(&(cd_len as u32).to_le_bytes());
        z.extend_from_slice(&(cd as u32).to_le_bytes());
        z.extend_from_slice(&[0, 0]);
        z
    }

    /// A stored zip of several entries.
    fn zip_many(files: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut z, mut cd) = (Vec::new(), Vec::new());
        for (name, body) in files {
            let offset = z.len() as u32;
            let head = |sig: &[u8], central: bool| {
                let mut h = sig.to_vec();
                h.extend_from_slice(if central {
                    &[20, 0, 20, 0, 0, 0]
                } else {
                    &[20, 0, 0, 0]
                });
                h.extend_from_slice(&[0, 0]); // stored
                h.extend_from_slice(&[0; 8]);
                h.extend_from_slice(&(body.len() as u32).to_le_bytes());
                h.extend_from_slice(&(body.len() as u32).to_le_bytes());
                h.extend_from_slice(&(name.len() as u16).to_le_bytes());
                h
            };
            z.extend(head(b"PK\x03\x04", false));
            z.extend_from_slice(&[0, 0]);
            z.extend_from_slice(name.as_bytes());
            z.extend_from_slice(body);
            cd.extend(head(b"PK\x01\x02", true));
            cd.extend_from_slice(&[0; 12]);
            cd.extend_from_slice(&offset.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let at = z.len();
        let n = files.len() as u16;
        z.extend_from_slice(&cd);
        z.extend_from_slice(b"PK\x05\x06");
        z.extend_from_slice(&[0, 0, 0, 0]);
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        z.extend_from_slice(&(at as u32).to_le_bytes());
        z.extend_from_slice(&[0, 0]);
        z
    }

    /// A multi-layer KMZ (GDAL's LIBKML layout): the main KML only links layer files inside the
    /// archive. Those are read; a link out of the archive, to the web or by an absolute path is
    /// not; a file linked twice is read once; links stop two levels down.
    #[test]
    fn a_kmz_reads_the_layers_its_main_kml_links_inside_the_archive_and_nothing_else() {
        let place = |n: &str, links: &[&str]| {
            let links: String = links
                .iter()
                .map(|h| format!("<NetworkLink><Link><href>{h}</href></Link></NetworkLink>"))
                .collect();
            format!(
                "<kml><Document>{links}<Placemark><name>{n}</name><Point><coordinates>-97,35</coordinates></Point></Placemark></Document></kml>"
            )
        };
        let doc = place(
            "root",
            &[
                "layers/a.kml",
                "layers/a.kml",
                "../evil.kml",
                "http://example.com/remote.kml",
                "/abs.kml",
            ],
        );
        let a = place("a", &["../doc.kml", "b.kml"]);
        let b = place("b", &["c.kml"]);
        let c = place("c", &[]);
        let evil = place("evil", &[]);
        let z = zip_many(&[
            ("doc.kml", doc.as_bytes()),
            ("layers/a.kml", a.as_bytes()),
            ("layers/b.kml", b.as_bytes()),
            ("layers/c.kml", c.as_bytes()),
            ("evil.kml", evil.as_bytes()),
            ("abs.kml", evil.as_bytes()),
        ]);
        let mut names: Vec<String> = parse_kmz(&z)
            .unwrap()
            .iter()
            .map(|f| f.properties["name"].as_str().unwrap().to_string())
            .collect();
        names.sort();
        assert_eq!(names, ["a", "b", "root"]);
        assert_eq!(archive_path("layers", "../x.kml"), None);
        assert_eq!(archive_path("", "https://x/y.kml"), None);
        assert_eq!(archive_path("", "/y.kml"), None);
        assert_eq!(
            archive_path("a/b", "./c/d.kml").as_deref(),
            Some("a/b/c/d.kml")
        );
    }

    #[test]
    fn a_kmz_is_unzipped_stored_or_deflated() {
        for deflate in [false, true] {
            let z = zip_of("doc.kml", DOC.as_bytes(), deflate);
            assert!(is_zip(&z));
            assert_eq!(parse_kmz(&z).unwrap().len(), 4, "deflate {deflate}");
        }
        let none = zip_of("icon.png", b"\x89PNG", false);
        assert!(parse_kmz(&none)
            .unwrap_err()
            .to_string()
            .contains("no .kml"));
    }

    #[test]
    fn a_damaged_kmz_is_an_error_never_a_panic() {
        let z = zip_of("doc.kml", DOC.as_bytes(), true);
        for cut in 0..z.len() {
            let _ = parse_kmz(&z[..cut]);
        }
        let mut flipped = z.clone();
        for i in 0..flipped.len() {
            flipped[i] ^= 0xFF;
            let _ = parse_kmz(&flipped);
            flipped[i] ^= 0xFF;
        }
    }
}
