//! M8: every document mutation goes through the command layer, undoes to exactly the state
//! before it and redoes to exactly the state after it; and a document with every kind of
//! sketch state survives save → reload unchanged.

use cadrs_core::commands::{
    AddElement, AddSketch, DeleteElement, DeleteFeature, DuplicateElement, EditSketch,
    NewElementKind, RenameDocument, RenameElement, RenameFeature, ReplaceFeature, SetCurveAppearance, SetCustomColors,
    SetFaceAppearance, SetFeatureAppearance, SetPartAppearance, SetPartMaterial, SetSketchImprinting,
    SetSketchPlane,
};
use cadrs_core::{Command, Document, DocumentMeta, ElementId, FeatureId, History, Store};
use cadrs_sketch::constraint::{ConstraintOf, CurveSpec, Orient, rectangle_constraints};
use cadrs_sketch::{Dimension, DimensionKind, PlaneRef, Sketch, SketchOp, Vec2};

fn first_sketch(doc: &Document, element: ElementId, feature: FeatureId) -> &Sketch {
    &doc.element(element)
        .unwrap()
        .feature(feature)
        .unwrap()
        .sketch()
        .unwrap()
        .geometry
}

/// Runs `cmd`, then checks undo gives back the exact state before and redo the state after.
fn check(doc: &mut Document, history: &mut History, cmd: &dyn Command) {
    let before = doc.clone();
    history.execute(doc, cmd).unwrap_or_else(|e| panic!("{}: {e}", cmd.label()));
    let after = doc.clone();
    assert_ne!(before, after, "{} changed nothing", cmd.label());
    assert_eq!(history.undo_label(), Some(cmd.label().as_str()));
    history.undo(doc).unwrap();
    assert_eq!(*doc, before, "undo of {}", cmd.label());
    history.redo(doc).unwrap();
    assert_eq!(*doc, after, "redo of {}", cmd.label());
}

/// A document exercising every command, built through `history`.
fn build(doc: &mut Document, history: &mut History) -> (ElementId, FeatureId) {
    let ps = doc.elements[0].id;
    check(doc, history, &RenameDocument { name: "Bracket".into() });
    let extra = ElementId::new();
    check(
        doc,
        history,
        &AddElement {
            id: extra,
            kind: NewElementKind::PartStudio,
            name: None,
            after: Some(ps),
        },
    );
    check(
        doc,
        history,
        &RenameElement {
            id: extra,
            name: "Plates".into(),
        },
    );
    let copy = ElementId::new();
    check(doc, history, &DuplicateElement { source: extra, id: copy });
    check(doc, history, &DeleteElement { id: copy });

    let f = FeatureId::new();
    check(
        doc,
        history,
        &AddSketch {
            element: ps,
            feature: f,
            plane: None,
        },
    );
    check(
        doc,
        history,
        &SetSketchPlane {
            element: ps,
            feature: f,
            plane: Some(PlaneRef::Front),
        },
    );
    check(
        doc,
        history,
        &SetSketchImprinting {
            element: ps,
            feature: f,
            disable_imprinting: true,
        },
    );
    let edit = |op: SketchOp| EditSketch {
        element: ps,
        feature: f,
        op,
    };
    // Geometry with its automatic constraints.
    let corners = [
        Vec2::new(0.0, 0.0),
        Vec2::new(40.0, 0.0),
        Vec2::new(40.0, 25.0),
        Vec2::new(0.0, 25.0),
    ];
    check(
        doc,
        history,
        &edit(SketchOp::Batch(vec![
            SketchOp::AddPolyline {
                points: corners.to_vec(),
                closed: true,
                construction: false,
                label: "Add rectangle",
            },
            SketchOp::AddConstraints(rectangle_constraints(corners)),
        ])),
    );
    check(
        doc,
        history,
        &edit(SketchOp::AddCircle {
            center: Vec2::new(70.0, 10.0),
            radius: 8.0,
            construction: false,
        }),
    );
    check(
        doc,
        history,
        &edit(SketchOp::AddArc {
            center: Vec2::new(-30.0, 0.0),
            start: Vec2::new(-20.0, 0.0),
            end: Vec2::new(-30.0, 10.0),
            construction: true,
        }),
    );
    check(
        doc,
        history,
        &edit(SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, 60.0), Vec2::new(30.0, 70.0)],
            closed: false,
            construction: false,
            label: "Add line",
        }),
    );
    let sk = first_sketch(doc, ps, f).clone();
    let line = sk
        .curves
        .iter()
        .find(|(_, c)| {
            matches!(c.kind, cadrs_sketch::CurveKind::Line { a, .. } if sk.pos(a) == Vec2::new(0.0, 60.0))
        })
        .map(|(k, _)| k)
        .unwrap();
    let circle = sk
        .curves
        .iter()
        .find(|(_, c)| matches!(c.kind, cadrs_sketch::CurveKind::Circle { .. }))
        .map(|(k, _)| k)
        .unwrap();
    // A constraint (the constraint tools).
    check(
        doc,
        history,
        &edit(SketchOp::AddConstraints(vec![ConstraintOf::Horizontal(
            Orient::Line(CurveSpec::Id(line)),
        )])),
    );
    check(
        doc,
        history,
        &edit(SketchOp::SetConstruction {
            curves: vec![line],
            construction: true,
        }),
    );
    // Dimensions: driving, then its value, its label and driven.
    let a = sk.point_at(corners[0], 1e-6).unwrap();
    let b = sk.point_at(corners[1], 1e-6).unwrap();
    check(
        doc,
        history,
        &edit(SketchOp::SetDimension {
            dimension: Dimension {
                kind: DimensionKind::Horizontal { a, b },
                value: 50.0,
                offset: -6.0,
                along: 0.0,
                driven: false,
            },
            moves: vec![],
            radii: vec![],
        }),
    );
    check(
        doc,
        history,
        &edit(SketchOp::SetDimension {
            dimension: Dimension {
                kind: DimensionKind::Diameter { curve: circle },
                value: 20.0,
                offset: 0.7,
                along: 0.0,
                driven: false,
            },
            moves: vec![],
            radii: vec![(circle, 10.0)],
        }),
    );
    let sk = first_sketch(doc, ps, f).clone();
    let (width, _) = sk
        .dimensions
        .iter()
        .find(|(_, d)| matches!(d.kind, DimensionKind::Horizontal { .. }))
        .unwrap();
    check(doc, history, &edit(SketchOp::SetDimensionValue { id: width, value: 45.0 }));
    check(
        doc,
        history,
        &edit(SketchOp::MoveDimensionLabel {
            id: width,
            offset: -9.5,
            along: 3.25,
        }),
    );
    check(doc, history, &edit(SketchOp::SetDimensionDriven { id: width, driven: true }));
    // A drag's end.
    let cadrs_sketch::CurveKind::Line { b: p, .. } = sk.curves[line].kind else {
        panic!("not a line");
    };
    check(
        doc,
        history,
        &edit(SketchOp::SetGeometry {
            points: vec![(p, sk.pos(p) + Vec2::new(3.0, 0.0))],
            radii: vec![],
        }),
    );
    // T3: the entity tools' geometry (points, ellipses, polygons, slots, fillets, chamfers).
    check(doc, history, &edit(SketchOp::AddPoint { pos: Vec2::new(70.0, 70.0) }));
    check(
        doc,
        history,
        &edit(SketchOp::AddEllipse {
            center: Vec2::new(100.0, 0.0),
            major: Vec2::new(130.0, 0.0),
            minor: 10.0,
            construction: false,
        }),
    );
    check(
        doc,
        history,
        &edit(SketchOp::AddPolygon {
            center: Vec2::new(-80.0, 0.0),
            radius: 15.0,
            angle: 0.2,
            sides: 6,
            inscribed: true,
            construction: false,
        }),
    );
    let sides = first_sketch(doc, ps, f)
        .dimensions
        .iter()
        .find(|(_, d)| matches!(d.kind, DimensionKind::Sides { .. }))
        .map(|(k, _)| k)
        .unwrap();
    check(doc, history, &edit(SketchOp::SetDimensionValue { id: sides, value: 7.0 }));
    for (pts, construction) in [
        (vec![Vec2::new(0.0, -80.0), Vec2::new(40.0, -80.0)], true),
        (
            vec![Vec2::new(-60.0, -60.0), Vec2::new(-20.0, -60.0), Vec2::new(-20.0, -30.0)],
            false,
        ),
        (
            vec![Vec2::new(60.0, -60.0), Vec2::new(100.0, -60.0), Vec2::new(100.0, -30.0)],
            false,
        ),
    ] {
        check(
            doc,
            history,
            &edit(SketchOp::AddPolyline {
                points: pts,
                closed: false,
                construction,
                label: "Add line",
            }),
        );
    }
    let sk = first_sketch(doc, ps, f).clone();
    let spine = sk
        .curves
        .iter()
        .find(|(_, c)| c.construction && matches!(c.kind, cadrs_sketch::CurveKind::Line { .. }))
        .map(|(k, _)| k)
        .unwrap();
    check(
        doc,
        history,
        &edit(SketchOp::Slot {
            source: spine,
            width: 8.0,
            equal_to: None,
            construction: false,
        }),
    );
    let fillet_corner = sk.point_at(Vec2::new(-20.0, -60.0), 1e-9).unwrap();
    check(
        doc,
        history,
        &edit(SketchOp::Fillet {
            corner: fillet_corner,
            radius: 5.0,
            equal_to: None,
        }),
    );
    let chamfer_corner = sk.point_at(Vec2::new(100.0, -60.0), 1e-9).unwrap();
    check(
        doc,
        history,
        &edit(SketchOp::Chamfer {
            corner: chamfer_corner,
            d1: 6.0,
            d2: 4.0,
            equal_to: None,
        }),
    );
    // P3.5: appearances (part, face, feature/sketch), materials (library and custom) and the
    // document's custom colours are document state: undoable and saved.
    let part = cadrs_core::PartId::new(f, 0);
    let blue = cadrs_core::Appearance::rgb(41, 128, 185);
    check(doc, history, &SetPartAppearance { element: ps, parts: vec![part], appearance: Some(blue.with_alpha(128)) });
    check(
        doc,
        history,
        &SetFaceAppearance {
            element: ps,
            part,
            faces: vec![cadrs_core::solid::cap_name(f.0, 7, true)],
            appearance: Some(cadrs_core::Appearance::rgb(214, 48, 49)),
        },
    );
    check(
        doc,
        history,
        &SetFeatureAppearance {
            element: ps,
            feature: f,
            appearance: Some(cadrs_core::Appearance::rgb(39, 174, 96)),
            label: "Edit sketch appearance".into(),
        },
    );
    check(
        doc,
        history,
        &SetPartMaterial { element: ps, parts: vec![part], material: cadrs_core::material::library("Aluminum - 6061") },
    );
    check(
        doc,
        history,
        &SetPartMaterial {
            element: ps,
            parts: vec![cadrs_core::PartId::new(f, 1)],
            material: Some(cadrs_core::Material::custom("Oak", 750.0)),
        },
    );
    let curve = first_sketch(doc, ps, f).curves.keys().next().unwrap();
    check(
        doc,
        history,
        &SetCurveAppearance { element: ps, sketch: f, curve, appearance: Some(cadrs_core::Appearance::rgb(241, 196, 15)) },
    );
    check(doc, history, &SetCustomColors { colors: vec![blue, cadrs_core::Appearance::rgb(1, 2, 3)] });
    // A material needs a positive density and a name.
    for bad in [cadrs_core::Material::custom("Air", 0.0), cadrs_core::Material::custom(" ", 1.0)] {
        assert!(history.execute(doc, &SetPartMaterial { element: ps, parts: vec![part], material: Some(bad) }).is_err());
    }
    check(
        doc,
        history,
        &RenameFeature {
            element: ps,
            feature: f,
            name: "Profile".into(),
        },
    );
    (ps, f)
}

#[test]
fn every_command_undoes_and_redoes_exactly() {
    let mut doc = Document::new("Doc");
    let mut history = History::default();
    let (ps, f) = build(&mut doc, &mut history);
    // Deleting and replacing features (cancel, feature-list delete).
    let before = doc.element(ps).unwrap().feature(f).unwrap().clone();
    let two: Vec<_> = first_sketch(&doc, ps, f).curves.keys().take(2).collect();
    check(
        &mut doc,
        &mut history,
        &EditSketch {
            element: ps,
            feature: f,
            op: SketchOp::Delete {
                curves: two,
                points: vec![],
                dimensions: vec![],
                constraints: vec![],
            },
        },
    );
    check(
        &mut doc,
        &mut history,
        &ReplaceFeature {
            element: ps,
            feature: before,
            label: "Cancel Profile".into(),
        },
    );
    check(
        &mut doc,
        &mut history,
        &DeleteFeature {
            element: ps,
            feature: f,
            label: "Delete Profile".into(),
        },
    );
    // Undoing everything gives back the new document; redoing everything the final one.
    let end = doc.clone();
    let n = history.undo_len();
    while history.undo(&mut doc).is_some() {}
    assert_eq!(doc.name, "Doc");
    assert_eq!(doc.elements.len(), 2);
    assert!(doc.elements[0].features().is_empty());
    for _ in 0..n {
        history.redo(&mut doc).unwrap();
    }
    assert_eq!(doc, end);
}

#[test]
fn documents_survive_save_and_reload_exactly() {
    let mut doc = Document::new("Doc");
    let mut history = History::default();
    let (ps, f) = build(&mut doc, &mut history);
    let sk = first_sketch(&doc, ps, f);
    assert!(!sk.dimensions.is_empty() && !sk.constraints.is_empty());
    assert!(sk.dimensions.values().any(|d| d.driven && d.along != 0.0));
    // The T3 kinds are in it: an ellipse, a polygon's side count, a hidden constraint.
    assert!(sk.curves.values().any(|c| matches!(c.kind, cadrs_sketch::CurveKind::Ellipse { .. })));
    assert!(sk.dimensions.values().any(|d| matches!(d.kind, DimensionKind::Sides { .. }) && d.value == 7.0));
    assert!(sk.constraints.values().any(|c| matches!(c, ConstraintOf::EqualDistance(..))));
    let dir = std::env::temp_dir().join(format!("cadrs-roundtrip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::new(&dir);
    let meta = DocumentMeta::new("me", 1_000);
    store.create(&doc, &meta).unwrap();
    let back = store.load(doc.id).unwrap();
    assert_eq!(back.document, doc);
    assert_eq!(back.meta, meta);
    // Point ids survive too (dimensions and constraints refer to them).
    let a = first_sketch(&back.document, ps, f);
    assert!(a.points.keys().eq(sk.points.keys()));
    assert!(a.curves.keys().eq(sk.curves.keys()));
    // Editing after a reload keeps working (slot maps keep their free lists).
    let mut d2 = back.document.clone();
    let mut h2 = History::default();
    check(
        &mut d2,
        &mut h2,
        &EditSketch {
            element: ps,
            feature: f,
            op: SketchOp::AddCircle {
                center: Vec2::new(0.0, -40.0),
                radius: 5.0,
                construction: false,
            },
        },
    );
    // Saving the reloaded document again writes the same file.
    let text = std::fs::read_to_string(store.document_path(doc.id)).unwrap();
    store.save(&back.document, &back.meta).unwrap();
    assert_eq!(std::fs::read_to_string(store.document_path(doc.id)).unwrap(), text);
    let _ = std::fs::remove_dir_all(dir);
}
