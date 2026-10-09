//! The reference user-defined products in `docs/presets/` (1008.md C4): every one imports with
//! nothing refused or adjusted (its declared inputs, kind and altitude convention match what its
//! formula needs), and every one evaluates to a finite value on at least one of a few synthetic
//! storm gates and columns, so none is a formula that can never draw.

use wxdata::udp::{evaluate, evaluate_at_column, GateInputs};
use wxdata::udp_file::{import, Kind};

const PRESETS: &str = include_str!("../../../docs/presets/radar-analyst-custom-products.json");

fn gate(ref_dbz: f32, zdr: f32, cc: f32, kdp: f32, vel: f32, height_km: f32) -> GateInputs {
    GateInputs {
        reflectivity: Some(ref_dbz),
        velocity: Some(vel),
        spectrum_width: Some(4.0),
        differential_reflectivity: Some(zdr),
        specific_diff_phase: Some(kdp),
        correlation_coefficient: Some(cc),
        range_km: Some(60.0),
        azimuth_deg: Some(225.0),
        elevation_deg: Some(0.5 + height_km),
        beam_height_m: Some(height_km * 1000.0),
        beam_altitude_m: Some(height_km * 1000.0 + 380.0),
        freezing_level_m: Some(4000.0),
        minus20c_height_m: Some(6800.0),
        minus10c_height_m: Some(5500.0),
        minus30c_height_m: Some(8300.0),
        minus40c_height_m: Some(9800.0),
    }
}

/// A hail core, a debris ball, light rain, strong inbound flow, and the columns over them.
fn cases() -> Vec<(GateInputs, Vec<GateInputs>)> {
    let column =
        |f: &dyn Fn(f32) -> GateInputs| (0..12).map(|k| f(0.5 + k as f32)).collect::<Vec<_>>();
    vec![
        (
            gate(64.0, 0.3, 0.93, 2.5, 18.0, 1.0),
            column(&|h| gate(64.0 - 2.0 * h, 0.3, 0.93, 2.5, 18.0, h)),
        ),
        (
            gate(52.0, 0.2, 0.62, 0.4, -35.0, 0.6),
            column(&|h| gate(50.0 - 3.0 * h, 0.4, 0.7 + 0.02 * h, 0.4, -35.0, h)),
        ),
        (
            gate(28.0, 1.1, 0.99, 0.2, 6.0, 1.5),
            column(&|h| gate(28.0 - h, 1.1, 0.99, 0.2, 6.0, h)),
        ),
        (
            gate(45.0, 2.5, 0.97, 1.0, -48.0, 0.8),
            column(&|h| gate(55.0 - h, 3.0 - 0.3 * h, 0.97, 1.5, -48.0 + 2.0 * h, h)),
        ),
        // A heavy-rain core with broad spectrum width.
        (
            GateInputs {
                spectrum_width: Some(7.0),
                ..gate(50.0, 2.0, 0.985, 2.8, 10.0, 1.2)
            },
            column(&|h| gate(50.0 - 2.0 * h, 2.0, 0.985, 2.8, 10.0, h)),
        ),
        // A strong echo aloft, between the -10 and -20 C levels.
        (
            gate(52.0, 0.5, 0.97, 0.5, 15.0, 5.8),
            column(&|h| gate(56.0 - h, 0.5, 0.97, 0.5, 15.0, h)),
        ),
    ]
}

#[test]
fn every_reference_product_imports_cleanly_and_can_draw() {
    let imported = import(PRESETS, &|_| true).expect("the file parses");
    assert!(
        imported.diagnostics.is_empty(),
        "refused or adjusted:\n{}",
        imported.diagnostics.join("\n")
    );
    assert_eq!(imported.products.len(), 32);
    let file: serde_json::Value = serde_json::from_str(PRESETS).unwrap();
    let kinds: Vec<Kind> = file["products"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| serde_json::from_value(p["kind"].clone()).unwrap())
        .collect();
    let cases = cases();
    let mut never = Vec::new();
    for (def, kind) in imported.products.iter().zip(kinds) {
        let expr = def.compile().expect("imported products compile");
        let drew = cases.iter().any(|(g, column)| {
            let v = match kind {
                Kind::Gate => evaluate(&expr, g),
                Kind::Column => evaluate_at_column(&expr, g, column),
            };
            v.is_some_and(f32::is_finite)
        });
        if !drew {
            never.push(def.name.clone());
        }
    }
    assert!(never.is_empty(), "never finite on any case: {never:?}");
}

/// Unit and datum typing finds nothing to say about any reference product: none adds or compares
/// different quantities, or mixes heights above the antenna with heights above sea level.
#[test]
fn reference_products_are_unit_consistent() {
    let imported = import(PRESETS, &|_| true).expect("the file parses");
    let found: Vec<String> = imported
        .products
        .iter()
        .filter_map(|def| {
            let d = def.compile().unwrap().unit_diagnostics();
            (!d.is_empty()).then(|| format!("{}: {}", def.name, d.join("; ")))
        })
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}
