//! Portable user-defined product files (ROADMAP_PARITY M3.2): a versioned JSON document carrying
//! product definitions from one installation to another, each with a stable ID and what it needs
//! spelled out — the inputs it reads, whether it reduces a column, the environmental heights it
//! needs, and the altitude convention of the heights it reads.
//!
//! What a product needs is derived from its formula, never trusted from the file: a declared list
//! that disagrees is reported and the formula wins. Import validates every product before it is
//! accepted — the formula parses, its nesting and size are bounded, a column formula reduces one
//! column at most, the palette is a colour table this build has — and returns a diagnostic for
//! each one refused or adjusted, so a malformed or hostile file costs a bounded parse and nothing
//! else. Formulas are the existing safe expression language; nothing in a file executes.
//!
//! A bare JSON list of definitions (what settings store, and what earlier builds wrote) imports
//! with its original meaning.

use crate::udp::{Input, ProductDef};
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "hookecho-product";
pub const VERSION: u32 = 1;

/// The longest formula a file may carry, in bytes.
pub const MAX_EXPRESSION_BYTES: usize = 4096;
/// The most operations a formula may hold; the per-gate cost of evaluating it is bounded by this.
pub const MAX_NODES: usize = 512;
/// The most products one file may carry.
pub const MAX_PRODUCTS: usize = 256;

/// The heights a formula reads, and what they are measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Altitude {
    /// Reads no height.
    None,
    /// `BEAM_HEIGHT_M`: above the radar antenna — not terrain AGL.
    AboveAntenna,
    /// `BEAM_ALTITUDE_M` and the isotherm heights: above mean sea level.
    Msl,
    /// Both conventions in one formula: comparing an antenna-relative height with an MSL one is
    /// off by the antenna's altitude, which the file says rather than hides.
    Mixed,
}

/// Whether a product is a value per gate or a reduction over a column of tilts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Gate,
    Column,
}

/// One product in a file: the definition, and what it needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(flatten)]
    pub def: ProductDef,
    /// Every input the formula reads, by name.
    #[serde(default)]
    pub requires: Vec<String>,
    /// The environmental heights among them, which need a matched sounding or model reading.
    #[serde(default)]
    pub environment: Vec<String>,
    #[serde(default = "default_kind")]
    pub kind: Kind,
    #[serde(default = "default_altitude")]
    pub altitude: Altitude,
}

fn default_kind() -> Kind {
    Kind::Gate
}

fn default_altitude() -> Altitude {
    Altitude::None
}

/// The document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductFile {
    pub format: String,
    pub version: u32,
    pub products: Vec<Entry>,
}

/// What importing found, product by product.
#[derive(Debug, Default, PartialEq)]
pub struct Import {
    /// Accepted definitions, in file order.
    pub products: Vec<ProductDef>,
    /// One line per product refused or adjusted, naming it.
    pub diagnostics: Vec<String>,
}

fn is_environment(i: Input) -> bool {
    matches!(
        i,
        Input::FreezingLevelM | Input::Minus10cHeightM | Input::Minus20cHeightM
    )
}

/// What a formula needs, derived from it.
fn describe(def: &ProductDef) -> Result<Entry, String> {
    let expr = def.compile().map_err(|e| e.to_string())?;
    let inputs = expr.inputs();
    let antenna = inputs.contains(&Input::BeamHeightM);
    let msl = inputs
        .iter()
        .any(|i| *i == Input::BeamAltitudeM || is_environment(*i));
    Ok(Entry {
        def: def.clone(),
        requires: inputs.iter().map(|i| i.name().to_string()).collect(),
        environment: inputs
            .iter()
            .filter(|i| is_environment(**i))
            .map(|i| i.name().to_string())
            .collect(),
        kind: if expr.uses_column() {
            Kind::Column
        } else {
            Kind::Gate
        },
        altitude: match (antenna, msl) {
            (false, false) => Altitude::None,
            (true, false) => Altitude::AboveAntenna,
            (false, true) => Altitude::Msl,
            (true, true) => Altitude::Mixed,
        },
    })
}

/// A stable ID for a definition that has none: derived from its name and formula, so the same
/// product gets the same ID wherever it is first exported.
pub fn derive_id(def: &ProductDef) -> String {
    // FNV-1a: stable across builds and platforms, unlike the standard library's hasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in def.name.bytes().chain([0]).chain(def.expression.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("udp-{h:016x}")
}

/// Give every definition without an ID one, keeping those it has. Returns whether any changed.
pub fn ensure_ids(products: &mut [ProductDef]) -> bool {
    let mut changed = false;
    for p in products.iter_mut().filter(|p| p.id.is_empty()) {
        p.id = derive_id(p);
        changed = true;
    }
    changed
}

/// Write `products` as a file. A definition that does not parse is written as it is, with no
/// derived needs, so exporting never loses a product someone is still editing.
pub fn export(products: &[ProductDef]) -> String {
    let products = products
        .iter()
        .map(|p| {
            let mut def = p.clone();
            if def.id.is_empty() {
                def.id = derive_id(&def);
            }
            describe(&def).unwrap_or(Entry {
                def,
                requires: Vec::new(),
                environment: Vec::new(),
                kind: Kind::Gate,
                altitude: Altitude::None,
            })
        })
        .collect();
    serde_json::to_string_pretty(&ProductFile {
        format: FORMAT.into(),
        version: VERSION,
        products,
    })
    .unwrap_or_default()
}

/// Validate one definition read from a file: `Err` refuses it, `Ok` carries notes on what was
/// adjusted.
fn validate(
    entry: &Entry,
    known_palette: &dyn Fn(&str) -> bool,
) -> Result<(ProductDef, Vec<String>), String> {
    let mut def = entry.def.clone();
    let mut notes = Vec::new();
    if def.name.trim().is_empty() {
        return Err("it has no name".into());
    }
    if def.expression.len() > MAX_EXPRESSION_BYTES {
        return Err(format!(
            "its formula is {} bytes, more than {MAX_EXPRESSION_BYTES}",
            def.expression.len()
        ));
    }
    let expr = def
        .compile()
        .map_err(|e| format!("its formula does not parse: {e}"))?;
    if expr.node_count() > MAX_NODES {
        return Err(format!(
            "its formula has {} operations, more than {MAX_NODES}",
            expr.node_count()
        ));
    }
    if expr.column_depth() > crate::udp_column::MAX_COLUMN_DEPTH {
        return Err("it reduces a column inside another column reduction".into());
    }
    if let Some((lo, hi)) = def.range {
        if !(lo.is_finite() && hi.is_finite() && lo < hi) {
            notes.push(format!(
                "its range {lo}..{hi} is not a range; fitted to its values instead"
            ));
            def.range = None;
        }
    }
    if let Some(p) = def.palette.clone() {
        if !known_palette(&p) {
            notes.push(format!(
                "colour table \u{201c}{p}\u{201d} is not one this build has; drawn on a plain ramp"
            ));
            def.palette = None;
        }
    }
    let derived = describe(&def)?;
    let declared = |v: &[String]| {
        let mut v: Vec<String> = v.iter().map(|s| s.to_ascii_uppercase()).collect();
        v.sort();
        v
    };
    if !entry.requires.is_empty() && declared(&entry.requires) != declared(&derived.requires) {
        notes.push(format!(
            "the file says it reads {}, its formula reads {}; the formula is used",
            entry.requires.join(", "),
            derived.requires.join(", ")
        ));
    }
    if entry.altitude != Altitude::None && entry.altitude != derived.altitude {
        notes.push(format!(
            "the file says its heights are {:?}, its formula's are {:?}; the formula is used",
            entry.altitude, derived.altitude
        ));
    }
    if derived.altitude == Altitude::Mixed {
        notes.push(
            "it compares BEAM_HEIGHT_M (above the antenna) with MSL heights; the difference is the \
             antenna's altitude"
                .into(),
        );
    }
    if def.id.is_empty() {
        def.id = derive_id(&def);
    }
    Ok((def, notes))
}

/// Read a product file (or a bare list of definitions, as earlier builds wrote), validating every
/// product. `known_palette` says whether a colour-table code exists in this build.
pub fn import(text: &str, known_palette: &dyn Fn(&str) -> bool) -> Result<Import, String> {
    let entries: Vec<Entry> = match serde_json::from_str::<serde_json::Value>(text) {
        Err(e) => return Err(format!("not JSON: {e}")),
        Ok(serde_json::Value::Array(_)) => {
            let defs: Vec<ProductDef> = serde_json::from_str(text)
                .map_err(|e| format!("not a list of product definitions: {e}"))?;
            defs.into_iter()
                .map(|def| Entry {
                    def,
                    requires: Vec::new(),
                    environment: Vec::new(),
                    kind: Kind::Gate,
                    altitude: Altitude::None,
                })
                .collect()
        }
        Ok(value) => {
            let format = value.get("format").and_then(|f| f.as_str()).unwrap_or("");
            if format != FORMAT {
                return Err(format!(
                    "not a product file (format \u{201c}{format}\u{201d})"
                ));
            }
            let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
            if version > u64::from(VERSION) {
                return Err(format!(
                    "written by a newer version of this format ({version}; this build reads {VERSION})"
                ));
            }
            serde_json::from_value::<ProductFile>(value)
                .map_err(|e| format!("not a valid product file: {e}"))?
                .products
        }
    };
    if entries.len() > MAX_PRODUCTS {
        return Err(format!(
            "it holds {} products, more than {MAX_PRODUCTS}",
            entries.len()
        ));
    }
    let mut out = Import::default();
    for entry in &entries {
        let name = if entry.def.name.trim().is_empty() {
            "(unnamed)".to_string()
        } else {
            entry.def.name.clone()
        };
        match validate(entry, known_palette) {
            Ok((def, notes)) => {
                out.diagnostics
                    .extend(notes.into_iter().map(|n| format!("{name}: {n}")));
                out.products.push(def);
            }
            Err(why) => out
                .diagnostics
                .push(format!("{name}: not imported — {why}")),
        }
    }
    Ok(out)
}

/// Merge imported definitions into `existing`: one with an ID already there replaces it (the
/// same product, updated); a new one whose name is taken is renamed rather than shadowing it.
/// Returns how many were added and replaced.
pub fn merge(existing: &mut Vec<ProductDef>, imported: Vec<ProductDef>) -> (usize, usize) {
    let (mut added, mut replaced) = (0, 0);
    for mut def in imported {
        if let Some(slot) = existing
            .iter_mut()
            .find(|p| !p.id.is_empty() && p.id == def.id)
        {
            *slot = def;
            replaced += 1;
            continue;
        }
        let base = def.name.clone();
        let mut n = 2;
        while existing.iter().any(|p| p.name == def.name) {
            def.name = format!("{base} ({n})");
            n += 1;
        }
        existing.push(def);
        added += 1;
    }
    (added, replaced)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, expression: &str) -> ProductDef {
        ProductDef {
            id: String::new(),
            name: name.into(),
            units: "dBZ".into(),
            expression: expression.into(),
            range: Some((0.0, 80.0)),
            palette: Some("REF".into()),
        }
    }

    fn palettes(code: &str) -> bool {
        crate::level2::Moment::from_code(code).is_some()
    }

    #[test]
    fn a_file_round_trips_with_ids_and_what_each_product_needs() {
        let products = vec![
            def("Hail column", "max_vertical(REF) - 0 * MINUS10C_HEIGHT_M"),
            def("Low ZDR", "ZDR * BEAM_HEIGHT_M / 1000"),
        ];
        let text = export(&products);
        let file: ProductFile = serde_json::from_str(&text).unwrap();
        assert_eq!((file.format.as_str(), file.version), (FORMAT, VERSION));
        let hail = &file.products[0];
        assert_eq!(hail.kind, Kind::Column);
        assert_eq!(hail.altitude, Altitude::Msl);
        assert_eq!(hail.environment, ["MINUS10C_HEIGHT_M"]);
        assert!(hail.requires.contains(&"REF".to_string()));
        assert_eq!(file.products[1].altitude, Altitude::AboveAntenna);
        assert_eq!(file.products[1].kind, Kind::Gate);
        let back = import(&text, &palettes).unwrap();
        assert!(back.diagnostics.is_empty(), "{:?}", back.diagnostics);
        assert_eq!(back.products.len(), 2);
        for (a, b) in back.products.iter().zip(&products) {
            assert_eq!(
                a.id,
                derive_id(b),
                "a stable ID, the same wherever exported"
            );
            assert_eq!(
                (&a.name, &a.expression, a.range, &a.palette),
                (&b.name, &b.expression, b.range, &b.palette)
            );
        }
    }

    #[test]
    fn a_bare_list_from_an_earlier_build_imports_with_its_meaning() {
        let old = r#"[{"name":"Z","units":"dBZ","expression":"REF + 1"}]"#;
        let got = import(old, &palettes).unwrap();
        assert_eq!(got.products.len(), 1);
        assert_eq!(got.products[0].expression, "REF + 1");
        assert!(!got.products[0].id.is_empty());
        assert!(got.diagnostics.is_empty());
    }

    #[test]
    fn bad_products_are_refused_by_name_and_the_rest_imported() {
        let deep = format!("{}REF{}", "(".repeat(100), ")".repeat(100));
        let big = (0..600).map(|_| "REF").collect::<Vec<_>>().join(" + ");
        let mut entries = vec![
            def("Good", "REF * 2"),
            def("Broken", "REF +"),
            def("Deep", &deep),
            def("Big", &big),
            def("Nested", "max_vertical(max_vertical(REF))"),
            def("", "REF"),
            def("Huge", &"1".repeat(MAX_EXPRESSION_BYTES + 1)),
        ];
        entries[0].palette = Some("NOPE".into());
        entries[0].range = Some((5.0, 5.0));
        let text = export(&entries);
        let got = import(&text, &palettes).unwrap();
        assert_eq!(got.products.len(), 1, "{:?}", got.diagnostics);
        assert_eq!(got.products[0].palette, None);
        assert_eq!(got.products[0].range, None);
        let says = |who: &str, what: &str| {
            assert!(
                got.diagnostics
                    .iter()
                    .any(|d| d.starts_with(who) && d.contains(what)),
                "{who}: {what} in {:?}",
                got.diagnostics
            )
        };
        says("Good", "not one this build has");
        says("Good", "not a range");
        says("Broken", "does not parse");
        says("Deep", "does not parse");
        says("Big", "operations");
        says("Nested", "inside another column");
        says("(unnamed)", "no name");
        says("Huge", "bytes");
    }

    #[test]
    fn declared_needs_that_disagree_are_said_and_the_formula_wins() {
        let text = r#"{"format":"hookecho-product","version":1,"products":[
            {"name":"Z","units":"","expression":"REF + BEAM_ALTITUDE_M * 0",
             "requires":["ZDR"],"altitude":"above_antenna"}]}"#;
        let got = import(text, &palettes).unwrap();
        assert_eq!(got.products.len(), 1);
        assert!(
            got.diagnostics
                .iter()
                .any(|d| d.contains("its formula reads")),
            "{:?}",
            got.diagnostics
        );
        assert!(
            got.diagnostics.iter().any(|d| d.contains("heights")),
            "{:?}",
            got.diagnostics
        );
        let mixed = def("M", "BEAM_HEIGHT_M - FREEZING_LEVEL_M");
        let got = import(&export(&[mixed]), &palettes).unwrap();
        assert!(got
            .diagnostics
            .iter()
            .any(|d| d.contains("antenna's altitude")));
    }

    #[test]
    fn other_documents_and_newer_versions_are_refused_whole() {
        assert!(import("nope", &palettes)
            .unwrap_err()
            .starts_with("not JSON"));
        assert!(import(r#"{"format":"x"}"#, &palettes)
            .unwrap_err()
            .contains("not a product file"));
        assert!(import(
            r#"{"format":"hookecho-product","version":9,"products":[]}"#,
            &palettes
        )
        .unwrap_err()
        .contains("newer version"));
        let many: Vec<ProductDef> = (0..=MAX_PRODUCTS)
            .map(|i| def(&format!("P{i}"), "REF"))
            .collect();
        assert!(import(&export(&many), &palettes)
            .unwrap_err()
            .contains("products"));
    }

    #[test]
    fn merging_replaces_by_id_and_never_shadows_a_name() {
        let mut mine = vec![def("A", "REF"), def("B", "ZDR")];
        ensure_ids(&mut mine);
        let mut updated = mine[0].clone();
        updated.expression = "REF + 1".into();
        let stranger = ProductDef {
            id: "udp-other".into(),
            ..def("B", "KDP")
        };
        let (added, replaced) = merge(&mut mine, vec![updated, stranger]);
        assert_eq!((added, replaced), (1, 1));
        assert_eq!(mine[0].expression, "REF + 1");
        assert_eq!(mine[2].name, "B (2)");
        assert!(!ensure_ids(&mut mine), "IDs are kept once given");
    }
}
