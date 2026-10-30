//! A command's sketches on parts left for later ([`cadrs_core::parts::defer_regeneration`], as
//! the app has them) and regenerated after: the document and its undo step end as a command
//! regenerating them at once leaves them. (A test binary of its own: the switch is the
//! process's.)

use cadrs_core::commands::{AddExtrude, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{Document, ExtrudeFeature, FeatureKind, RegionRef};
use cadrs_core::parts::{cap_name, face_plane};
use cadrs_core::{ElementId, FeatureId, History};
use cadrs_sketch::region::regions;
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

/// A 50 × 30 box on Top, `depth` high, and a sketch on its top face: the document, its history,
/// the Part Studio, the extrude and the sketch.
fn box_with_face_sketch() -> (Document, History, ElementId, FeatureId, FeatureId) {
    let mut d = Document::new("Deferred");
    let mut h = History::default();
    let el = d.elements[0].id;
    let s1 = FeatureId::new();
    let add = AddSketch {
        element: el,
        feature: s1,
        plane: Some(PlaneRef::Top),
    };
    h.execute(&mut d, &add).unwrap();
    let points = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(50.0, 0.0),
        Vec2::new(50.0, 30.0),
        Vec2::new(0.0, 30.0),
    ];
    let edit = EditSketch {
        element: el,
        feature: s1,
        op: SketchOp::AddPolyline {
            points,
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
    };
    h.execute(&mut d, &edit).unwrap();
    let g = &d
        .element(el)
        .unwrap()
        .feature(s1)
        .unwrap()
        .sketch()
        .unwrap()
        .geometry;
    let rs: Vec<RegionRef> = regions(g).iter().map(|r| RegionRef::new(s1, r)).collect();
    let ex = FeatureId::new();
    let add = AddExtrude {
        element: el,
        feature: ex,
        extrude: ExtrudeFeature::default(),
    };
    h.execute(&mut d, &add).unwrap();
    let set = SetExtrude {
        element: el,
        feature: ex,
        extrude: ExtrudeFeature {
            regions: rs,
            depth: 25.0,
            ..ExtrudeFeature::default()
        },
        label: "Select".into(),
    };
    h.execute(&mut d, &set).unwrap();
    let features = d.element(el).unwrap().features().to_vec();
    let top = face_plane(&features, ex, cap_name(&features, ex, 0, true).unwrap()).unwrap();
    let s2 = FeatureId::new();
    let add = AddSketch {
        element: el,
        feature: s2,
        plane: Some(top),
    };
    h.execute(&mut d, &add).unwrap();
    (d, h, el, ex, s2)
}

/// The height of the sketch's plane.
fn height(d: &Document, el: ElementId, sketch: FeatureId) -> f64 {
    let sk = d
        .element(el)
        .unwrap()
        .feature(sketch)
        .unwrap()
        .sketch()
        .unwrap();
    sk.plane.unwrap().frame().origin[2]
}

/// The extrude 15 deeper.
fn deeper(d: &Document, el: ElementId, ex: FeatureId) -> SetExtrude {
    let FeatureKind::Extrude(e) = &d.element(el).unwrap().feature(ex).unwrap().kind else {
        panic!("an extrude");
    };
    SetExtrude {
        element: el,
        feature: ex,
        extrude: ExtrudeFeature {
            depth: 40.0,
            depth_expr: "40 mm".into(),
            ..e.clone()
        },
        label: "Depth".into(),
    }
}

#[test]
fn sketches_left_by_a_command_regenerated_after_as_at_once() {
    let (d0, h0, el, ex, s2) = box_with_face_sketch();
    assert!((height(&d0, el, s2) - 25.0).abs() < 1e-9);
    let cmd = deeper(&d0, el, ex);

    // Left, then regenerated as the app does it.
    let (mut d, mut h) = (d0.clone(), h0.clone());
    cadrs_core::parts::defer_regeneration(true);
    h.execute(&mut d, &cmd).unwrap();
    cadrs_core::parts::defer_regeneration(false);
    assert!(
        (height(&d, el, s2) - 25.0).abs() < 1e-9,
        "left where it was"
    );
    let left = cadrs_core::parts::take_deferred();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].0, el);
    let revision = h.revision();
    let before = d.element(el).unwrap().features().to_vec();
    let mut after = before.clone();
    cadrs_core::parts::regenerate_with(&mut after, &left[0].1);
    assert!(cadrs_core::parts::take_regenerated(
        &mut d, el, &before, &after
    ));
    assert!(h.amend_last(&d, revision));
    assert!((height(&d, el, s2) - 40.0).abs() < 1e-9);
    // Taken in once only.
    assert!(!h.amend_last(&d, revision));
    assert!(cadrs_core::parts::take_deferred().is_empty());
    let (mut later, mut kept) = (d, h);

    // At once.
    let (mut d, mut h) = (d0.clone(), h0.clone());
    h.execute(&mut d, &cmd).unwrap();
    assert!(cadrs_core::parts::take_deferred().is_empty());
    assert_eq!(later, d);

    // The undo step holds the sketch regenerated.
    kept.undo(&mut later);
    assert_eq!(later, d0);
    kept.redo(&mut later);
    assert_eq!(later, d);
}
