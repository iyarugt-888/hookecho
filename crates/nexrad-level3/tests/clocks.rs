//! PDB clock interpretation, independently checked against ICD 2620001 and Python struct/
//! datetime over the existing MetPy input files. No network or client clock participates.

use nexrad_level3::{decode, ProductTimes};

#[test]
fn real_products_retain_acquisition_and_output_clocks() {
    let cases: &[(&[u8], ProductTimes)] = &[
        (
            include_bytes!("data/nst_tlx.l3"),
            ProductTimes {
                data_start_unix: Some(1_585_612_974), // 2020-03-31 00:02:54 UTC
                generation_unix: Some(1_585_613_190), // 00:06:30 UTC
                volume_end_unix: None,
            },
        ),
        (
            include_bytes!("data/nmd_tlx.l3"),
            ProductTimes {
                data_start_unix: Some(1_585_612_974),
                generation_unix: Some(1_585_613_191),
                volume_end_unix: None,
            },
        ),
        (
            include_bytes!("data/dvl_tlx.l3"),
            ProductTimes {
                data_start_unix: Some(1_784_507_372), // 2026-07-20 00:29:32 UTC
                generation_unix: None,
                volume_end_unix: Some(1_784_507_580), // 00:33:00 UTC, not generation
            },
        ),
        (
            include_bytes!("data/eet_tlx.l3"),
            ProductTimes {
                data_start_unix: Some(1_784_507_372),
                generation_unix: None,
                volume_end_unix: Some(1_784_507_580),
            },
        ),
        (
            include_bytes!("data/n0g_tlx.l3"),
            ProductTimes {
                data_start_unix: Some(1_787_788_788), // 2026-08-26 23:59:48 UTC
                generation_unix: Some(1_787_788_829), // 2026-08-27 00:00:29 UTC
                volume_end_unix: None,
            },
        ),
        (
            include_bytes!("data/tz0_okc.l3"),
            ProductTimes {
                data_start_unix: Some(1_785_605_233),
                generation_unix: Some(1_785_605_302),
                volume_end_unix: None,
            },
        ),
    ];
    for (bytes, expected) in cases {
        let product = decode(bytes).expect("existing real product must decode");
        assert_eq!(product.times, *expected, "product {}", product.code);
        assert_eq!(
            decode(bytes).unwrap().times,
            *expected,
            "replay keeps clocks"
        );
    }
}

fn message(code: i16, start: (i16, u32), output: (i16, u32)) -> Vec<u8> {
    // Minimal uncompressed MHB + PDB. The offsets below are global bytes from the ICD;
    // no symbology block is needed to exercise the timestamp container boundary.
    let mut bytes = vec![0u8; 120];
    bytes[18..20].copy_from_slice(&(-1i16).to_be_bytes());
    bytes[30..32].copy_from_slice(&code.to_be_bytes());
    bytes[40..42].copy_from_slice(&start.0.to_be_bytes());
    bytes[42..46].copy_from_slice(&start.1.to_be_bytes());
    bytes[46..48].copy_from_slice(&output.0.to_be_bytes());
    bytes[48..52].copy_from_slice(&output.1.to_be_bytes());
    bytes
}

#[test]
fn one_based_epoch_and_midnight_do_not_shift_or_wrap() {
    let first = decode(&message(165, (1, 0), (1, 86_399))).unwrap();
    assert_eq!(first.times.data_start_unix, Some(0));
    assert_eq!(first.times.generation_unix, Some(86_399));
    let midnight = decode(&message(165, (1, 86_399), (2, 0))).unwrap();
    assert_eq!(midnight.times.data_start_unix, Some(86_399));
    assert_eq!(midnight.times.generation_unix, Some(86_400));
    let last = decode(&message(165, (i16::MAX, 86_399), (i16::MAX, 86_399))).unwrap();
    assert_eq!(last.times.data_start_unix, Some(2_831_068_799));
}

#[test]
fn invalid_clocks_remain_independently_unavailable() {
    for invalid in [(0, 0), (-1, 0), (i16::MIN, 0), (1, 86_400), (1, u32::MAX)] {
        let start = decode(&message(165, invalid, (1, 7))).unwrap();
        assert_eq!(start.times.data_start_unix, None, "{invalid:?}");
        assert_eq!(start.times.generation_unix, Some(7));
        let output = decode(&message(165, (1, 7), invalid)).unwrap();
        assert_eq!(output.times.data_start_unix, Some(7));
        assert_eq!(output.times.generation_unix, None, "{invalid:?}");
        let volume = decode(&message(134, (1, 7), invalid)).unwrap();
        assert_eq!(volume.times.volume_end_unix, None);
        assert_eq!(volume.times.generation_unix, None);
    }
}

#[test]
fn free_text_generation_does_not_become_an_acquisition() {
    let product = decode(&message(75, (1, 12), (1, 12))).unwrap();
    assert_eq!(
        product.times,
        ProductTimes {
            data_start_unix: None,
            generation_unix: Some(12),
            volume_end_unix: None,
        }
    );
}

#[cfg(feature = "serde")]
#[test]
fn serialized_source_clocks_preserve_roles_and_unknowns() {
    let product = decode(&message(134, (1, 7), (1, 12))).unwrap();
    let json = serde_json::to_value(product).unwrap();
    assert_eq!(
        json["times"],
        serde_json::json!({
            "data_start_unix": 7,
            "generation_unix": null,
            "volume_end_unix": 12,
        })
    );
    let product = decode(&message(165, (0, 7), (1, 12))).unwrap();
    let json = serde_json::to_value(product).unwrap();
    assert_eq!(json["times"]["data_start_unix"], serde_json::Value::Null);
    assert_eq!(json["times"]["generation_unix"], 12);
}
