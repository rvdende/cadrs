//! P3.7 / PS21.2: the Funnel's Sketch 1 as `course_ps21_funnel` draws it through the UI (saved
//! from that scenario after its step 02 into `fixtures/funnel_sketch_1.ron`): the 6 × 4 ellipse,
//! its 0.125 offset, the handle, and the construction centreline from the origin to the end
//! line's midpoint (Coincident and Midpoint, as `ex4-step2.png`), with the slanted lines
//! Symmetric about it and the 1.5, 4.5 and 105° dimensions. It is fully defined.

use cadrs_sketch::{ConstraintOf, Sketch, solve};

fn sketch() -> Sketch {
    ron::from_str(include_str!("fixtures/funnel_sketch_1.ron")).expect("the fixture parses")
}

#[test]
fn funnel_sketch_1_is_fully_defined() {
    let s = sketch();
    let a = solve::analyze(&s);
    assert!(a.conflicting.is_empty() && a.conflicting_dimensions.is_empty(), "{a:?}");
    assert_eq!(a.dof, 0, "Sketch 1 is fully defined (black)");
    // Its driving dimensions: 6 and 4 (the axes), 0.125 (the offset), 1.5, 4.5 (mm) and 105°.
    let mut values: Vec<f64> = s.dimensions.values().filter(|d| !d.driven).map(|d| d.value).collect();
    values.sort_by(f64::total_cmp);
    let want = [3.175, 38.1, 101.6, 105.0, 114.3, 152.4];
    assert_eq!(values.len(), want.len(), "{values:?}");
    for (v, w) in values.iter().zip(want) {
        assert!((v - w).abs() < 1e-6, "{values:?}");
    }
    // The centreline is tied to the origin and to the end line's middle, and the slanted lines
    // are symmetric about it.
    assert!(s.constraints.values().any(|c| matches!(c, ConstraintOf::SymmetricCurves(..) | ConstraintOf::SymmetricPoints(..))));
    assert!(s.constraints.values().any(|c| matches!(c, ConstraintOf::Midpoint(..))));
}
