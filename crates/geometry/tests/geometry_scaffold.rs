use sc_geometry::{Aspect, Ratio, SizePx};

#[test]
fn ratio_is_reduced() {
    assert_eq!(Ratio::new(1920, 1080), Ratio { num: 16, den: 9 });
    assert_eq!(Ratio::of(SizePx { w: 3024, h: 1964 }), Ratio::new(756, 491));
}

#[test]
fn aspect_round_trips_through_str() {
    for a in Aspect::ALL {
        assert_eq!(Aspect::parse(a.as_str()), Some(a));
    }
    assert_eq!(Aspect::Original.ratio(), None);
}
