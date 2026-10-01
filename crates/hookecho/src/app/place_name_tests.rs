use super::short_place_name;

#[test]
fn keeps_the_place_and_drops_the_administrative_tail() {
    // What Nominatim actually returns for a town query.
    assert_eq!(
        short_place_name("Norman, Cleveland County, Oklahoma, United States"),
        "Norman"
    );
    // Already short, or oddly shaped: pass it through rather than blanking the name.
    assert_eq!(short_place_name("Dallas"), "Dallas");
    assert_eq!(short_place_name(""), "");
    assert_eq!(short_place_name("  Tulsa , Oklahoma"), "Tulsa");
}
