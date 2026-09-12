//! Regression test for a real GOES ABI-L2-CMIP granule (`tests/data/regression/`, not part of
//! `golden.rs`'s h5py-comparison suite — there is no h5py in this environment to generate a
//! golden file against, so this checks physical plausibility instead of reference agreement).
//!
//! This file's `y` coordinate variable carries a `DIMENSION_LIST` attribute — an array of HDF5
//! object references, a datatype this reader doesn't interpret — whose dataspace this parser
//! misread as a handful of enormous dimensions, panicking on `usize` multiplication overflow
//! computing the attribute's element count. Any ABI CMIP file has this attribute on its
//! coordinate variables, so every one of them hit this before the fix.

use std::path::PathBuf;

fn sample() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/regression/abi_cmip_m6c13.nc");
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn opens_without_panicking_on_the_dimension_list_attribute() {
    let f = hdf5lite::File::open(sample()).expect("open");
    // The specific attribute that used to overflow: reading it must not panic, whether or not
    // this reader can make sense of its value.
    let _ = f.attributes("y");
}

#[test]
fn brightness_temperatures_decode_to_a_physically_plausible_range() {
    let f = hdf5lite::File::open(sample()).expect("open");
    let cmi = f.read_f64("CMI").expect("CMI dataset");
    assert_eq!(cmi.len(), 500 * 500, "500x500 mesoscale sector");
    let finite: Vec<f64> = cmi.iter().copied().filter(|v| v.is_finite()).collect();
    assert_eq!(finite.len(), cmi.len(), "band 13 has no fill in this granule");
    let (min, max) = finite
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    // Band 13 (clean IR window) brightness temperature, Kelvin. Cold cloud tops to a warm
    // surface, comfortably inside any real scene's range.
    assert!((150.0..350.0).contains(&min), "min {min} K implausible");
    assert!((150.0..350.0).contains(&max), "max {max} K implausible");
    assert!(max - min > 5.0, "no dynamic range at all: {min}..{max} K");
}

#[test]
fn the_fixed_grid_projection_attributes_are_present() {
    let f = hdf5lite::File::open(sample()).expect("open");
    let proj = f
        .attributes("goes_imager_projection")
        .expect("projection attrs");
    for key in [
        "semi_major_axis",
        "semi_minor_axis",
        "perspective_point_height",
        "longitude_of_projection_origin",
        "latitude_of_projection_origin",
    ] {
        assert!(
            proj.get(key).and_then(|v| v.as_f64()).is_some(),
            "missing or non-numeric {key}"
        );
    }
}
