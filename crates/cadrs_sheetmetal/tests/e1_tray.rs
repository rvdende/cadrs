//! P3I.6: the exercise E1 tray stand-in lays flat into one part of seven walls and six bends.

use cadrs_sheetmetal::{Params, flatten, samples};

#[test]
fn the_e1_tray_lays_flat() {
    let p = Params { thickness: 1.0, bend_radius: 1.0, ..Params::default() };
    let m = samples::e1_tray(p).expect("builds");
    assert!(m.validate().is_empty(), "{:?}", m.validate());
    let flat = flatten(&m);
    assert!(flat.is_ok(), "{:?}", flat.errors);
    assert_eq!(flat.parts.len(), 1, "{:?}", m.joints.iter().map(|j| (j.a, j.b, &j.kind)).collect::<Vec<_>>());
    let part = &flat.parts[0];
    assert_eq!(part.walls.len(), 7);
    assert_eq!(part.bends.len(), 6);
    assert_eq!(part.outline.len(), 1, "{:?} {:?}", part.outline.iter().map(|o| (o.bounds(), o.area())).collect::<Vec<_>>(), part.pieces.iter().map(|p| (p.source, p.polygon.bounds())).collect::<Vec<_>>());
    // The fan hole, four mounting holes, three slots and the cut-out.
    assert_eq!(part.outline[0].holes.len(), 9);
}
