//! Deterministic failure injection (ROADMAP_2 §3.2): what a real feed hands the decoders when a
//! connection drops, a server errors, an object is half-written or a cache file is damaged.
//! Truncated responses, invalid gzip, invalid GRIB, malformed Level II volumes and chunks, and
//! missing framing. Every case must come back as an error (or an empty result where the API has
//! no error) — never a panic, and never a decoder grinding through a length it was lied to about.
//!
//! The fuzz targets (`fuzz/fuzz_targets`) search the same ground at random; these pin the cases
//! a live session actually meets, so they run on every `cargo test`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Cut points through a buffer: the edges, a few bytes in, and evenly through the rest.
fn cuts(len: usize) -> Vec<usize> {
    let mut at: Vec<usize> = vec![0, 1, 2, 3, 4, 8, 15, 16, 17, 23, 24, 25, 64, 512];
    at.extend((1..32).map(|i| len * i / 32));
    at.push(len.saturating_sub(1));
    at.retain(|c| *c < len);
    at.sort_unstable();
    at.dedup();
    at
}

/// Runs `f`, failing the test if it took longer than a decoder has any business taking.
fn quick<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    let took = start.elapsed();
    assert!(took < Duration::from_secs(5), "{what} took {took:?}");
    out
}

// --- HTTP responses that are not what was asked for ---

/// A server's error page or an empty body where data belongs.
const NOT_DATA: [&[u8]; 5] = [
    b"",
    b"<html><body>503 Service Unavailable</body></html>",
    b"<?xml version=\"1.0\"?><Error><Code>NoSuchKey</Code></Error>",
    b"{\"error\":\"rate limited\"}",
    &[0u8; 4096],
];

#[test]
fn an_error_page_is_not_a_radar_volume() {
    for body in NOT_DATA {
        assert!(wxdata::level2::decode_volume(body.to_vec()).is_err());
        assert!(wxdata::odim::decode(body.to_vec()).is_err());
    }
}

#[test]
fn an_error_page_is_not_a_grib_message() {
    for body in NOT_DATA {
        assert!(wxdata::mrms::decode_grib2(body).is_err());
        assert!(wxdata::grib_split::extract_field(body, 1).is_none());
    }
}

#[test]
fn an_error_page_is_not_a_satellite_image() {
    for body in NOT_DATA {
        assert!(wxdata::goes_abi::decode(body.to_vec(), 16, 16).is_err());
    }
}

#[test]
fn an_error_page_is_not_a_gis_layer() {
    for body in NOT_DATA {
        let text = String::from_utf8_lossy(body);
        assert!(wxdata::gis::parse_geojson(&text).is_err());
        assert!(wxdata::kml::parse_kmz(body).is_err());
        assert!(wxdata::shapefile::parse(body, None, None).is_err());
    }
}

// --- Truncated responses: a real file cut off at every point ---

#[test]
fn a_truncated_grib_message_is_refused_quickly() {
    let whole = fixture("regression/ndfd_snow_complex_packing.grib2");
    for cut in cuts(whole.len()) {
        let part = &whole[..cut];
        let got = quick(&format!("GRIB cut at {cut}"), || {
            wxdata::mrms::decode_grib2(part)
        });
        assert!(
            got.is_err(),
            "a GRIB message cut at {cut} of {} decoded",
            whole.len()
        );
    }
}

#[test]
fn a_grib_message_claiming_more_than_it_holds_is_refused() {
    // Section 0's total length, bytes 8..16, raised past the buffer: a forged or truncated
    // header must not send the decoder looking for sections that are not there.
    let mut forged = fixture("regression/ndfd_snow_complex_packing.grib2");
    let claimed = (forged.len() as u64 + 1_000_000).to_be_bytes();
    forged[8..16].copy_from_slice(&claimed);
    let got = quick("forged GRIB length", || wxdata::mrms::decode_grib2(&forged));
    assert!(got.is_err());
}

/// An HDF5 file's tail can be free space and metadata the decoder never reads, so a cut there
/// may still decode — but only to exactly what the whole file gives, never to a partial sweep.
#[test]
fn a_truncated_odim_volume_is_an_error_or_the_whole_volume() {
    for name in ["dwd-boo-tilt00.h5", "dwd-boo-tilt05.h5"] {
        let whole = fixture(name);
        let (_, full) = wxdata::odim::decode(whole.clone()).expect("the whole file decodes");
        let full = format!("{full:?}");
        for cut in cuts(whole.len()) {
            let part = whole[..cut].to_vec();
            let got = quick(&format!("{name} cut at {cut}"), || {
                wxdata::odim::decode(part)
            });
            if let Ok((_, scan)) = got {
                assert!(
                    format!("{scan:?}") == full,
                    "{name} cut at {cut} of {} decoded to less than the whole",
                    whole.len()
                );
            }
        }
    }
}

// --- Invalid gzip: an MRMS or cache file cut short, or not gzip at all ---

#[test]
fn a_damaged_gzip_stream_is_never_whole() {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&fixture("regression/ndfd_snow_complex_packing.grib2"))
        .unwrap();
    let gz = enc.finish().unwrap();
    assert!(wxdata::objcache::is_whole_gzip(&gz));
    for cut in cuts(gz.len()) {
        assert!(!wxdata::objcache::is_whole_gzip(&gz[..cut]), "cut at {cut}");
    }
    // A flipped byte in the middle of the deflate stream, or in the CRC trailer.
    for at in [gz.len() / 2, gz.len() - 6] {
        let mut bad = gz.clone();
        bad[at] ^= 0x5a;
        assert!(!wxdata::objcache::is_whole_gzip(&bad), "flip at {at}");
    }
    for body in NOT_DATA {
        assert!(!wxdata::objcache::is_whole_gzip(body));
    }
}

// --- Malformed Level II ---

#[test]
fn a_malformed_level2_volume_is_an_error() {
    // An Archive II header ("AR2V0006.") followed by nothing, by garbage, and by a bzip2 block
    // marker with garbage behind it (what a half-written object from the live bucket looks like).
    let mut header = b"AR2V0006.001".to_vec();
    header.extend_from_slice(&[0u8; 12]);
    let mut garbage = header.clone();
    garbage.extend((0..4096u32).map(|i| (i * 31 % 251) as u8));
    let mut bzip = header.clone();
    bzip.extend_from_slice(&[0, 0, 0x10, 0]);
    bzip.extend_from_slice(b"BZh91AY&SY");
    bzip.extend((0..2048u32).map(|i| (i * 17 % 253) as u8));
    for (what, bytes) in [
        ("short", b"AR2V".to_vec()),
        ("header only", header),
        ("garbage body", garbage),
        ("broken bzip2", bzip),
    ] {
        let got = quick(what, || wxdata::level2::decode_volume(bytes));
        assert!(got.is_err(), "{what} decoded");
    }
}

// --- Live chunk framing: a missing or cut-off chunk between worker and app ---

#[test]
fn live_chunk_framing_refuses_a_cut_off_buffer() {
    let framed = wxdata::live::frame([&b"first chunk"[..], &b"second"[..]].into_iter());
    assert_eq!(
        wxdata::live::split_framed(&framed).unwrap(),
        vec![&b"first chunk"[..], &b"second"[..]]
    );
    assert!(wxdata::live::split_framed(&[]).unwrap().is_empty());
    for cut in 1..framed.len() {
        // Every cut that does not land on a chunk boundary is a framing error.
        let boundary = cut == 4 + 11;
        let got = wxdata::live::split_framed(&framed[..cut]);
        assert_eq!(got.is_ok(), boundary, "cut at {cut}: {got:?}");
    }
}

#[test]
fn a_live_volume_with_no_blocks_is_an_error() {
    assert!(wxdata::live_block::assemble_scan(&[]).is_err());
}
