//! Direct edits: Delete face and Move face through the feature list. Expected values derived by
//! hand in each test's comments.
#![cfg(feature = "brep")]
#![allow(clippy::field_reassign_with_default)]

use std::f64::consts::PI;

use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude, SetFeature};
use cadrs_core::direct_edit::{DirectEditFeature, DirectEditKind, MoveType};
use cadrs_core::document::{
    AxisRef, BooleanOp, DirectionRef, Document, EdgeRef, ExtrudeFeature, FaceRef, FeatureKind,
};
use cadrs_core::rebuild::{self, Build};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, samples};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn new() -> Self {
        let d = Document::new("P3.8");
        let el = d.elements[0].id;
        Self {
            d,
            h: History::default(),
            el,
        }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn build(&self) -> std::sync::Arc<Build> {
        rebuild::build(&self.features())
    }

    fn parts(&self) -> Vec<Part> {
        let b = self.build();
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b.parts.clone()
    }

    fn volume(&self) -> f64 {
        self.parts().iter().map(|p| p.mass.unwrap().volume).sum()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddSketch {
                    element: self.el,
                    feature: f,
                    plane: Some(plane),
                },
            )
            .unwrap();
        for op in ops {
            self.h
                .execute(
                    &mut self.d,
                    &EditSketch {
                        element: self.el,
                        feature: f,
                        op,
                    },
                )
                .unwrap();
        }
        f
    }

    fn extrude(
        &mut self,
        s: FeatureId,
        seeds: &[Vec2],
        set: impl FnOnce(&mut ExtrudeFeature),
    ) -> FeatureId {
        let g = self
            .d
            .element(self.el)
            .unwrap()
            .feature(s)
            .unwrap()
            .sketch()
            .unwrap()
            .geometry
            .clone();
        let regions = samples::region_refs(s, &g, seeds);
        assert_eq!(regions.len(), seeds.len(), "a region is missing");
        let mut e = ExtrudeFeature {
            op: BooleanOp::New,
            ..samples::extrude_of(regions, 10.0)
        };
        set(&mut e);
        let f = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddExtrude {
                    element: self.el,
                    feature: f,
                    extrude: ExtrudeFeature::default(),
                },
            )
            .unwrap();
        self.h
            .execute(
                &mut self.d,
                &SetExtrude {
                    element: self.el,
                    feature: f,
                    extrude: e,
                    label: "Extrude".into(),
                },
            )
            .unwrap();
        f
    }

    /// A box x0..x1 × y0..y1 on Top, `h` high (New).
    fn block(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, h: f64) -> FeatureId {
        let s = self.sketch(PlaneRef::Top, vec![rect(x0, y0, x1, y1)]);
        self.extrude(s, &[Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0)], |e| {
            e.depth = h;
            e.depth_expr = format!("{h} mm");
        })
    }

    fn add(&mut self, base: &str, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddFeature {
                    element: self.el,
                    feature,
                    base_name: base.into(),
                    kind,
                },
            )
            .unwrap();
        feature
    }

    fn set(&mut self, feature: FeatureId, kind: FeatureKind) {
        self.h
            .execute(
                &mut self.d,
                &SetFeature {
                    element: self.el,
                    feature,
                    kind,
                    label: "Edit".into(),
                },
            )
            .unwrap();
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    let v = Vec2::new;
    SketchOp::AddPolyline {
        points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

fn circle(cx: f64, cy: f64, r: f64) -> SketchOp {
    SketchOp::AddCircle {
        center: Vec2::new(cx, cy),
        radius: r,
        construction: false,
    }
}

/// The faces of the part matching `pick` (its index and the face), as references.
fn faces_where(p: &Part, pick: impl Fn(&cadrs_core::solid::SolidFace) -> bool) -> Vec<FaceRef> {
    p.solid
        .faces
        .iter()
        .enumerate()
        .filter(|(_, f)| pick(f))
        .map(|(i, f)| FaceRef {
            part: p.id,
            face: f.name,
            seed: p.solid.face_point(i).unwrap(),
        })
        .collect()
}

/// A 40 × 20 × 10 block with a Ø10 hole through it.
fn block_with_hole() -> (Doc, f64) {
    let mut d = Doc::new();
    d.block(0.0, 0.0, 40.0, 20.0, 10.0);
    let s = d.sketch(PlaneRef::Top, vec![circle(20.0, 10.0, 5.0)]);
    d.extrude(s, &[Vec2::new(20.0, 10.0)], |e| {
        e.op = BooleanOp::Remove;
        e.depth = 10.0;
        e.depth_expr = "10 mm".into();
    });
    let v = 40.0 * 20.0 * 10.0 - PI * 25.0 * 10.0;
    close(d.volume(), v, 1e-6);
    (d, v)
}

fn is_cylinder(f: &cadrs_core::solid::SolidFace) -> bool {
    f.kind == Some(cadrs_kernel::SurfaceKind::Cylinder)
}

#[test]
fn delete_face_heals_a_hole() {
    // The hole's wall deleted: the top and bottom close over it, the block is whole (8000).
    let (mut d, _) = block_with_hole();
    let wall = faces_where(&d.parts()[0], is_cylinder);
    assert!(!wall.is_empty());
    d.add(
        "Delete face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::DeleteFace,
            faces: wall,
            ..Default::default()
        }),
    );
    close(d.volume(), 8000.0, 1e-6);
}

#[test]
fn move_face_moves_a_wall() {
    // The top (z = 10) moved up 5: the block is 15 high and the hole still goes through it.
    let (mut d, v) = block_with_hole();
    let top = faces_where(&d.parts()[0], |f| {
        f.plane
            .is_some_and(|p| p.normal()[2] > 0.5 && (p.origin[2] - 10.0).abs() < 1e-9)
    });
    assert_eq!(top.len(), 1);
    d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            faces: top,
            distance: 5.0,
            distance_expr: "5 mm".into(),
            ..Default::default()
        }),
    );
    close(d.volume(), v * 1.5, 1e-6);
}

#[test]
fn move_face_resizes_a_hole() {
    // The hole's wall moved 1 along its outward normal (into the hole): Ø10 becomes Ø8.
    let (mut d, _) = block_with_hole();
    let wall = faces_where(&d.parts()[0], is_cylinder);
    d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            faces: wall,
            distance: 1.0,
            distance_expr: "1 mm".into(),
            ..Default::default()
        }),
    );
    close(d.volume(), 8000.0 - PI * 16.0 * 10.0, 1e-6);
}

/// The block's top face (z = 10).
fn top_face(p: &Part) -> Vec<FaceRef> {
    faces_where(p, |f| {
        f.plane
            .is_some_and(|p| p.normal()[2] > 0.5 && (p.origin[2] - 10.0).abs() < 1e-9)
    })
}

#[test]
fn translate_without_a_direction_moves_along_the_first_faces_normal() {
    // The top translated 5 along its own normal (up): 15 high, as the offset above.
    let (mut d, v) = block_with_hole();
    let top = top_face(&d.parts()[0]);
    d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Translate,
            faces: top,
            distance: 5.0,
            distance_expr: "5 mm".into(),
            ..Default::default()
        }),
    );
    close(d.volume(), v * 1.5, 1e-6);
}

#[test]
fn translate_moves_a_hole_along_a_direction() {
    // The hole's wall translated 5 along X (the Right plane's normal): the hole is at x = 25,
    // the volume is the same and the centroid moves away from the hole, to
    // (8000·20 − 250π·25) / (8000 − 250π).
    let (mut d, v) = block_with_hole();
    let wall = faces_where(&d.parts()[0], is_cylinder);
    d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Translate,
            faces: wall,
            direction: Some(DirectionRef::PlaneNormal(PlaneRef::Right)),
            distance: 5.0,
            distance_expr: "5 mm".into(),
            ..Default::default()
        }),
    );
    let parts = d.parts();
    let m = parts[0].mass.unwrap();
    close(m.volume, v, 1e-6);
    let hole = 250.0 * PI;
    close(
        m.center_of_mass.x,
        (8000.0 * 20.0 - hole * 25.0) / (8000.0 - hole),
        1e-6,
    );
}

#[test]
fn rotate_tilts_a_face_about_an_axis() {
    // The top turned 10° about its edge along y = 0: its height is 10 + y·tan 10° (or
    // 10 − y·tan 10°, by which way the edge runs), so the block and the hole (whose mean height
    // is at y = 10) both grow or shrink by tan 10°; −10° does the opposite.
    let (mut d, v) = block_with_hole();
    let part = d.parts()[0].clone();
    let top = top_face(&part);
    let edge = part
        .solid
        .edges
        .iter()
        .find(|e| {
            e.points
                .iter()
                .all(|p| p[1].abs() < 1e-9 && (p[2] - 10.0).abs() < 1e-9)
        })
        .expect("the top edge along y = 0");
    let axis = AxisRef::Edge(EdgeRef {
        part: part.id,
        edge: edge.name,
        seed: edge.points[0],
    });
    let mut x = DirectEditFeature {
        kind: DirectEditKind::MoveFace,
        move_type: MoveType::Rotate,
        faces: top,
        axis: Some(axis),
        angle: 10.0,
        angle_expr: "10 deg".into(),
        ..Default::default()
    };
    let f = d.add("Move face", FeatureKind::DirectEdit(x.clone()));
    let turned = d.volume();
    let t = 10f64.to_radians().tan();
    close((turned - v).abs(), v * t, 1e-6);
    x.angle = -10.0;
    x.angle_expr = "-10 deg".into();
    d.set(f, FeatureKind::DirectEdit(x));
    close(d.volume() + turned, 2.0 * v, 1e-6);
}

#[test]
fn rotate_without_an_axis_says_what_to_pick() {
    let (mut d, _) = block_with_hole();
    let top = top_face(&d.parts()[0]);
    let f = d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Rotate,
            faces: top,
            ..Default::default()
        }),
    );
    let b = d.build();
    assert!(
        b.errors
            .iter()
            .any(|(id, e)| *id == f && e.contains("Select an axis of rotation"))
    );
}

#[test]
fn translate_without_a_direction_needs_a_flat_first_face() {
    // A hole's wall has no single normal: Translate asks for a direction instead of guessing.
    let (mut d, _) = block_with_hole();
    let wall = faces_where(&d.parts()[0], is_cylinder);
    let f = d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Translate,
            faces: wall,
            distance: 5.0,
            distance_expr: "5 mm".into(),
            ..Default::default()
        }),
    );
    let b = d.build();
    assert!(
        b.errors
            .iter()
            .any(|(id, e)| *id == f && e.contains("Select a direction")),
        "{:?}",
        b.errors
    );
}

#[test]
fn a_move_faces_parents_are_its_faces_and_the_reference_its_type_uses() {
    // The direction (a sketch line) is a parent of a Translate; switched to Rotate, the axis
    // (an edge of the block) is, and the sketch no longer holds it back.
    let (mut d, _) = block_with_hole();
    let part = d.parts()[0].clone();
    let line = d.sketch(
        PlaneRef::Top,
        vec![SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, -10.0), Vec2::new(10.0, -10.0)],
            closed: false,
            construction: false,
            label: "Add line",
        }],
    );
    let curve =
        d.d.element(d.el)
            .unwrap()
            .feature(line)
            .unwrap()
            .sketch()
            .unwrap()
            .geometry
            .curves
            .keys()
            .next()
            .unwrap();
    let edge = &part.solid.edges[0];
    let mut x = DirectEditFeature {
        kind: DirectEditKind::MoveFace,
        move_type: MoveType::Translate,
        faces: top_face(&part),
        direction: Some(DirectionRef::SketchLine {
            sketch: line,
            curve,
        }),
        axis: Some(AxisRef::Edge(EdgeRef {
            part: part.id,
            edge: edge.name,
            seed: edge.points[0],
        })),
        ..Default::default()
    };
    let parents = |x: &DirectEditFeature| {
        Feature {
            id: FeatureId::new(),
            name: "Move face 1".into(),
            kind: FeatureKind::DirectEdit(x.clone()),
            suppress_by: None,
        }
        .parents()
    };
    let block = part.feature;
    assert!(parents(&x).contains(&line));
    x.move_type = MoveType::Rotate;
    let p = parents(&x);
    assert!(!p.contains(&line), "the unused direction is not a parent");
    assert!(p.contains(&block));
    // The sketch line (drawn from x = 0 to x = 10) works as the direction too: the hole's
    // wall moved 5 along it goes to x = 25, as with the Right plane's normal in
    // `translate_moves_a_hole_along_a_direction` (the volume the same, the centroid away).
    x.move_type = MoveType::Translate;
    x.faces = faces_where(&part, is_cylinder);
    x.distance = 5.0;
    x.distance_expr = "5 mm".into();
    d.add("Move face", FeatureKind::DirectEdit(x));
    let m = d.parts()[0].mass.unwrap();
    let hole = 250.0 * PI;
    close(m.volume, 8000.0 - hole, 1e-6);
    close(
        m.center_of_mass.x,
        (8000.0 * 20.0 - hole * 25.0) / (8000.0 - hole),
        1e-6,
    );
}

#[test]
fn rotate_follows_an_angle_variable() {
    // #a = 10 deg drives the angle; set to −10 deg the top tilts the other way: as in
    // `rotate_tilts_a_face_about_an_axis`, the two volumes add up to 2V.
    use cadrs_core::variables::{VariableFeature, VariableKind, VariableType};
    let (mut d, v) = block_with_hole();
    let part = d.parts()[0].clone();
    let angle = |expr: &str, value: f64| VariableFeature {
        name: "a".into(),
        var_type: VariableType::Angle,
        expr: expr.into(),
        value,
        kind: VariableKind::Angle,
        ..VariableFeature::default()
    };
    let var = d.add("Variable", FeatureKind::Variable(angle("10 deg", 10.0)));
    let edge = part
        .solid
        .edges
        .iter()
        .find(|e| {
            e.points
                .iter()
                .all(|p| p[1].abs() < 1e-9 && (p[2] - 10.0).abs() < 1e-9)
        })
        .expect("the top edge along y = 0");
    d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Rotate,
            faces: top_face(&part),
            axis: Some(AxisRef::Edge(EdgeRef {
                part: part.id,
                edge: edge.name,
                seed: edge.points[0],
            })),
            angle: 10.0,
            angle_expr: "#a".into(),
            ..Default::default()
        }),
    );
    let turned = d.volume();
    close((turned - v).abs(), v * 10f64.to_radians().tan(), 1e-6);
    d.set(var, FeatureKind::Variable(angle("-10 deg", -10.0)));
    close(d.volume() + turned, 2.0 * v, 1e-6);
}

#[test]
fn a_move_face_from_before_move_types_loads_as_an_offset() {
    // A document saved before Translate and Rotate: no move type, direction, axis or angle.
    let old = r#"(kind: MoveFace, faces: [], distance: 2.0, distance_expr: "2 mm")"#;
    let x: DirectEditFeature = ron::from_str(old).unwrap();
    assert_eq!(x.move_type, MoveType::Offset);
    assert_eq!((x.distance, x.direction, x.axis), (2.0, None, None));
    assert_eq!((x.angle, x.angle_expr.as_str()), (0.0, "0 deg"));
    // Delete fillet faces stays off for a Delete face saved before it (it rebuilds as it did);
    // a new one has it on.
    assert!(!x.delete_fillets);
    assert!(DirectEditFeature::default().delete_fillets);
}

#[test]
fn a_direct_edit_without_faces_says_what_to_pick() {
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 10.0, 10.0);
    let f = d.add(
        "Delete face",
        FeatureKind::DirectEdit(DirectEditFeature::default()),
    );
    let b = d.build();
    assert!(
        b.errors
            .iter()
            .any(|(id, e)| *id == f && e.contains("Select the faces to delete"))
    );
}

#[test]
fn a_plane_first_in_the_list_has_a_frame() {
    // An offset plane 2 above Top, before any part: its frame is at z = 2.
    let mut d = Doc::new();
    let p = d.add(
        "Plane",
        FeatureKind::Plane(cadrs_core::plane::PlaneFeature {
            kind: cadrs_core::plane::PlaneType::Offset,
            entities: vec![cadrs_core::plane::PlaneEntity::Plane(PlaneRef::Top)],
            offset: 2.0,
            offset_expr: "2 mm".into(),
            ..Default::default()
        }),
    );
    let f = d.features();
    let r = cadrs_core::parts::plane_feature_ref(&f, p).expect("the plane has a frame");
    close(r.frame().origin[2], 2.0, 1e-9);
}

#[test]
fn simplify_merges_coplanar_faces() {
    // Two blocks side by side joined: Simplify leaves one box with 6 faces, volume unchanged.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 10.0, 10.0);
    let s = d.sketch(PlaneRef::Top, vec![rect(10.0, 0.0, 20.0, 10.0)]);
    d.extrude(s, &[Vec2::new(15.0, 5.0)], |e| {
        e.op = BooleanOp::Add;
        e.depth = 10.0;
        e.depth_expr = "10 mm".into();
    });
    let any = faces_where(&d.parts()[0], |_| true)
        .into_iter()
        .take(1)
        .collect();
    d.add(
        "Simplify",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::Simplify,
            faces: any,
            ..Default::default()
        }),
    );
    let p = &d.parts()[0];
    close(p.mass.unwrap().volume, 2000.0, 1e-6);
    assert_eq!(p.solid.faces.len(), 6);
}

/// The faces of the first part that `pick` chooses, moved by each distance in turn: every move
/// either builds a valid body or fails with a reason, never a crash (OCCT's offset crashes when a
/// moved face reaches a face it doesn't border or shrinks away, so those are refused first).
/// Returns how many moves failed.
fn sweep_moves(
    make: impl Fn() -> Doc,
    pick: impl Fn(&cadrs_core::solid::SolidFace) -> bool,
    distances: impl Iterator<Item = f64>,
) -> usize {
    let mut failed = 0;
    for distance in distances {
        let mut d = make();
        let faces = faces_where(&d.parts()[0], &pick);
        assert!(!faces.is_empty());
        let f = d.add(
            "Move face",
            FeatureKind::DirectEdit(DirectEditFeature {
                kind: DirectEditKind::MoveFace,
                faces,
                distance,
                distance_expr: format!("{distance} mm"),
                ..Default::default()
            }),
        );
        let b = d.build();
        match b.errors.iter().find(|(id, _)| *id == f) {
            Some(_) => failed += 1,
            None => assert!(
                b.parts
                    .iter()
                    .all(|p| p.mass.is_some_and(|m| m.volume > 0.0)),
                "{distance}: an empty body"
            ),
        }
    }
    failed
}

fn steps(from: f64, to: f64, step: f64) -> impl Iterator<Item = f64> {
    (0..=((to - from) / step).round() as i32).map(move |k| from + k as f64 * step)
}

#[test]
#[ignore = "slow: a few hundred rebuilds"]
fn moving_faces_too_far_fails_without_crashing() {
    let top = |z: f64| {
        move |f: &cadrs_core::solid::SolidFace| {
            f.plane
                .is_some_and(|p| p.normal()[2] > 0.5 && (p.origin[2] - z).abs() < 1e-9)
        }
    };
    // The hole's wall: shrunk away (5) or grown through the long sides (−5).
    let n = sweep_moves(|| block_with_hole().0, is_cylinder, steps(-12.0, 6.0, 0.25));
    assert!(n > 0, "the far moves fail");
    // The top: down through the bottom (−10).
    sweep_moves(|| block_with_hole().0, top(10.0), steps(-25.0, 10.0, 0.5));
    // Two Ø10 holes 6 apart: one grown into the other (−3), then through the sides.
    let two_holes = || {
        let mut d = Doc::new();
        d.block(0.0, 0.0, 40.0, 20.0, 10.0);
        let s = d.sketch(
            PlaneRef::Top,
            vec![circle(12.0, 10.0, 5.0), circle(28.0, 10.0, 5.0)],
        );
        d.extrude(s, &[Vec2::new(12.0, 10.0), Vec2::new(28.0, 10.0)], |e| {
            e.op = BooleanOp::Remove;
            e.depth = 10.0;
        });
        d
    };
    let left_hole = |f: &cadrs_core::solid::SolidFace| {
        is_cylinder(f) && f.axis.is_some_and(|(o, _)| (o[0] - 12.0).abs() < 1e-6)
    };
    sweep_moves(two_holes, left_hole, steps(-6.0, 1.0, 0.25));
    // A pocket 4 deep from the bottom: its floor down through the opening (4) or up through
    // the top (−6).
    let pocket = || {
        let mut d = Doc::new();
        d.block(0.0, 0.0, 40.0, 20.0, 10.0);
        let s = d.sketch(PlaneRef::Top, vec![rect(10.0, 5.0, 30.0, 15.0)]);
        d.extrude(s, &[Vec2::new(20.0, 10.0)], |e| {
            e.op = BooleanOp::Remove;
            e.depth = 4.0;
        });
        d
    };
    let floor = |f: &cadrs_core::solid::SolidFace| {
        f.plane
            .is_some_and(|p| p.normal()[2] < -0.5 && (p.origin[2] - 4.0).abs() < 1e-9)
    };
    sweep_moves(pocket, floor, steps(-8.0, 6.0, 0.25));
    // A Ø10 boss 15 high on the block: its top down into the block (−5), its wall grown past
    // the block's sides.
    let boss = || {
        let mut d = Doc::new();
        d.block(0.0, 0.0, 40.0, 20.0, 10.0);
        let s = d.sketch(PlaneRef::Top, vec![circle(20.0, 10.0, 5.0)]);
        d.extrude(s, &[Vec2::new(20.0, 10.0)], |e| {
            e.op = BooleanOp::Add;
            e.depth = 15.0;
        });
        d
    };
    sweep_moves(boss, top(15.0), steps(-12.0, 5.0, 0.5));
    sweep_moves(boss, is_cylinder, steps(-6.0, 8.0, 0.5));
}

#[test]
fn delete_face_caps_a_hole() {
    // Cap: the hole's wall deleted and its two openings capped: the whole block again, the caps
    // faces of their own (8).
    let (mut d, _) = block_with_hole();
    let wall = faces_where(&d.parts()[0], is_cylinder);
    d.add(
        "Delete face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::DeleteFace,
            faces: wall,
            void: cadrs_core::direct_edit::VoidFill::Cap,
            ..Default::default()
        }),
    );
    let part = d.parts()[0].clone();
    close(part.mass.unwrap().volume, 8000.0, 1e-6);
    assert_eq!(part.solid.faces.len(), 8);
}

#[test]
fn delete_face_leaves_the_top_open() {
    // Leave open: the top deleted, nothing put in its place: a surface of the other 6 faces
    // (the bottom, 4 sides and the hole's wall).
    let (mut d, _) = block_with_hole();
    let top = top_face(&d.parts()[0]);
    d.add(
        "Delete face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::DeleteFace,
            faces: top,
            void: cadrs_core::direct_edit::VoidFill::Open,
            ..Default::default()
        }),
    );
    let part = d.parts()[0].clone();
    assert_eq!(part.kind, cadrs_core::parts::PartKind::Surface);
    assert_eq!(part.solid.faces.len(), 6);
}

#[test]
fn delete_face_takes_the_fillets_next_to_it() {
    // A Ø10 boss 5 high on a 40 × 20 × 10 block, its base edge filleted R1 (a torus). Deleting
    // the boss's wall and top with Delete fillet faces (the default) takes the fillet too: the
    // plain block, 8000, 6 faces. With it off, the fillet is left with nothing to heal to.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 40.0, 20.0, 10.0);
    let s = d.sketch(PlaneRef::Top, vec![circle(20.0, 10.0, 5.0)]);
    d.extrude(s, &[Vec2::new(20.0, 10.0)], |e| {
        e.op = BooleanOp::Add;
        e.depth = 15.0;
        e.depth_expr = "15 mm".into();
    });
    let part = d.parts()[0].clone();
    let base = part
        .solid
        .edges
        .iter()
        .min_by(|a, b| {
            a.distance([25.0, 10.0, 10.0])
                .total_cmp(&b.distance([25.0, 10.0, 10.0]))
        })
        .unwrap();
    d.add(
        "Fillet",
        FeatureKind::Fillet(cadrs_core::applied::FilletFeature {
            entities: vec![cadrs_core::applied::EdgeOrFace::Edge(EdgeRef {
                part: part.id,
                edge: base.name,
                seed: [25.0, 10.0, 10.0],
            })],
            size: 1.0,
            size_expr: "1 mm".into(),
            ..Default::default()
        }),
    );
    let part = d.parts()[0].clone();
    let boss = faces_where(&part, |f| {
        is_cylinder(f) || f.plane.is_some_and(|p| (p.origin[2] - 15.0).abs() < 1e-9)
    });
    assert_eq!(boss.len(), 2, "the boss's wall and top");
    let mut x = DirectEditFeature {
        kind: DirectEditKind::DeleteFace,
        faces: boss,
        delete_fillets: false,
        ..Default::default()
    };
    let f = d.add("Delete face", FeatureKind::DirectEdit(x.clone()));
    assert!(d.build().errors.iter().any(|(id, _)| *id == f));
    x.delete_fillets = true;
    d.set(f, FeatureKind::DirectEdit(x));
    let part = d.parts()[0].clone();
    close(part.mass.unwrap().volume, 8000.0, 1e-6);
    assert_eq!(part.solid.faces.len(), 6);
}

/// The edge of `part` nearest `p`, as a reference.
fn edge_near(part: &Part, p: [f64; 3]) -> EdgeRef {
    let e = part
        .solid
        .edges
        .iter()
        .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
        .unwrap();
    assert!(e.distance(p) < 1e-3, "no edge at {p:?}");
    EdgeRef {
        part: part.id,
        edge: e.name,
        seed: p,
    }
}

/// The block with the hole and a 15 high block beside it (x 50..60): its top corner and top.
fn block_with_a_taller_one() -> (Doc, f64, cadrs_core::document::VertexRef, FaceRef) {
    let (mut d, v) = block_with_hole();
    d.block(50.0, 0.0, 60.0, 10.0, 15.0);
    let parts = d.parts();
    let tall = parts.iter().find(|p| p.solid.faces.len() == 6).unwrap();
    let corner = tall
        .solid
        .vertices
        .iter()
        .find(|x| {
            let p = x.point;
            (p[0] - 50.0).abs() < 1e-9 && p[1].abs() < 1e-9 && (p[2] - 15.0).abs() < 1e-9
        })
        .unwrap();
    let corner = cadrs_core::document::VertexRef {
        part: tall.id,
        vertex: corner.name,
        point: corner.point,
    };
    let top = faces_where(tall, |f| {
        f.plane
            .is_some_and(|p| p.normal()[2] > 0.5 && (p.origin[2] - 15.0).abs() < 1e-9)
    })[0];
    (d, v, corner, top)
}

#[test]
fn translate_up_to_a_vertex_with_an_offset() {
    // The block's top translated up to the taller block's corner (z = 15): 15 high (1.5 V);
    // with an offset distance of 2 it stops 2 short: 13 high (1.3 V).
    let (mut d, v, corner, _) = block_with_a_taller_one();
    let top = top_face(&d.parts()[0]);
    let mut x = DirectEditFeature {
        kind: DirectEditKind::MoveFace,
        move_type: MoveType::Translate,
        end: cadrs_core::direct_edit::MoveEnd::UpTo,
        up_to: Some(cadrs_core::document::UpTo::Vertex(corner)),
        faces: top,
        ..Default::default()
    };
    let f = d.add("Move face", FeatureKind::DirectEdit(x.clone()));
    let first = |d: &Doc| d.parts()[0].mass.unwrap().volume;
    close(first(&d), v * 1.5, 1e-6);
    x.offset = 2.0;
    x.offset_expr = "2 mm".into();
    d.set(f, FeatureKind::DirectEdit(x));
    close(first(&d), v * 1.3, 1e-6);
}

#[test]
fn offset_up_to_a_parallel_face_and_not_a_slanted_one() {
    // Offset up to the taller block's top: 15 high. Translate up to one of its sides (not
    // parallel to the top) is refused with a reason.
    let (mut d, v, _, tall_top) = block_with_a_taller_one();
    let top = top_face(&d.parts()[0]);
    let mut x = DirectEditFeature {
        kind: DirectEditKind::MoveFace,
        move_type: MoveType::Offset,
        end: cadrs_core::direct_edit::MoveEnd::UpTo,
        up_to: Some(cadrs_core::document::UpTo::Face(tall_top)),
        faces: top,
        ..Default::default()
    };
    let f = d.add("Move face", FeatureKind::DirectEdit(x.clone()));
    close(d.parts()[0].mass.unwrap().volume, v * 1.5, 1e-6);
    let parts = d.parts();
    let tall = parts.iter().find(|p| p.solid.faces.len() == 6).unwrap();
    let side = faces_where(tall, |f| f.plane.is_some_and(|p| p.normal()[0] > 0.5))[0];
    x.move_type = MoveType::Translate;
    x.up_to = Some(cadrs_core::document::UpTo::Face(side));
    d.set(f, FeatureKind::DirectEdit(x));
    assert!(
        d.build()
            .errors
            .iter()
            .any(|(id, e)| *id == f && e.contains("isn't parallel")),
        "{:?}",
        d.build().errors
    );
}

#[test]
fn rotate_reapplies_the_fillet() {
    // A 40 × 20 × 10 block, the edge between its front (y = 0) and top filleted R2; the front
    // turned 10° about its bottom edge with Reapply fillet: the same as turning the plain front
    // and filleting that edge R2 after (volume), with an R2 fillet there still.
    let fillet = |d: &mut Doc| {
        // The edge between the front (facing −y, turned or not) and the top.
        let part = d.parts()[0].clone();
        let face = |pick: &dyn Fn([f64; 3]) -> bool| {
            part.solid
                .faces
                .iter()
                .find(|f| f.plane.is_some_and(|p| pick(p.normal())))
                .unwrap()
                .name
        };
        let (front, top) = (face(&|n| n[1] < -0.5), face(&|n| n[2] > 0.9));
        let e = part
            .solid
            .edges
            .iter()
            .find(|e| e.name.touches(&front) && e.name.touches(&top))
            .unwrap();
        let edge = EdgeRef {
            part: part.id,
            edge: e.name,
            seed: e.midpoint(),
        };
        d.add(
            "Fillet",
            FeatureKind::Fillet(cadrs_core::applied::FilletFeature {
                entities: vec![cadrs_core::applied::EdgeOrFace::Edge(edge)],
                size: 2.0,
                size_expr: "2 mm".into(),
                ..Default::default()
            }),
        );
    };
    let turn = |d: &mut Doc| {
        let part = d.parts()[0].clone();
        let front = faces_where(&part, |f| f.plane.is_some_and(|p| p.normal()[1] < -0.5));
        d.add(
            "Move face",
            FeatureKind::DirectEdit(DirectEditFeature {
                kind: DirectEditKind::MoveFace,
                move_type: MoveType::Rotate,
                faces: front,
                axis: Some(AxisRef::Edge(edge_near(&part, [20.0, 0.0, 0.0]))),
                angle: 10.0,
                angle_expr: "10 deg".into(),
                ..Default::default()
            }),
        );
    };
    let mut a = Doc::new();
    a.block(0.0, 0.0, 40.0, 20.0, 10.0);
    fillet(&mut a);
    turn(&mut a);
    let mut b = Doc::new();
    b.block(0.0, 0.0, 40.0, 20.0, 10.0);
    turn(&mut b);
    fillet(&mut b);
    let (pa, pb) = (a.parts()[0].clone(), b.parts()[0].clone());
    close(pa.mass.unwrap().volume, pb.mass.unwrap().volume, 1e-6);
    assert_eq!(pa.solid.faces.len(), 7);
    assert!(
        (0..pa.solid.faces.len()).any(|i| pa
            .solid
            .fillet_radius(i)
            .is_some_and(|r| (r - 2.0).abs() < 1e-9)),
        "an R2 fillet"
    );
}

#[test]
fn offset_up_to_a_part() {
    // A 10 × 10 block floating over the top (z 15…20): Offset up to it stops the top at its
    // nearest point over the face, 15 high (1.5 V). Up to the part being edited, or to the
    // taller block beside it (nothing over the face), is refused.
    let (mut d, v, corner, _) = block_with_a_taller_one();
    let s = d.sketch(PlaneRef::Top, vec![rect(10.0, 2.0, 20.0, 12.0)]);
    d.extrude(s, &[Vec2::new(15.0, 7.0)], |e| {
        e.depth = 5.0;
        e.depth_expr = "5 mm".into();
        e.start_offset = Some(cadrs_core::Offset {
            value: 15.0,
            expr: "15 mm".into(),
            flip: false,
        });
    });
    let parts = d.parts();
    let first = parts[0].clone();
    let floating = parts
        .iter()
        .find(|p| (p.mass.unwrap().volume - 500.0).abs() < 1e-6)
        .expect("the floating block")
        .id;
    let mut x = DirectEditFeature {
        kind: DirectEditKind::MoveFace,
        move_type: MoveType::Offset,
        end: cadrs_core::direct_edit::MoveEnd::UpTo,
        up_to: Some(cadrs_core::document::UpTo::Part(floating)),
        faces: top_face(&first),
        ..Default::default()
    };
    let f = d.add("Move face", FeatureKind::DirectEdit(x.clone()));
    close(d.parts()[0].mass.unwrap().volume, v * 1.5, 1e-6);
    for (target, why) in [
        (first.id, "not the one being edited"),
        (corner.part, "Nothing of the part"),
    ] {
        x.up_to = Some(cadrs_core::document::UpTo::Part(target));
        d.set(f, FeatureKind::DirectEdit(x.clone()));
        let b = d.build();
        assert!(
            b.errors.iter().any(|(id, e)| *id == f && e.contains(why)),
            "{:?}",
            b.errors
        );
    }
}

#[test]
fn offset_up_to_a_part_larger_than_the_face() {
    // A 100 × 100 plate floating over the 40 × 20 top (z 15…17), none of its corners over the
    // face: the top still stops at it, 15 high (1.5 V).
    let (mut d, v) = block_with_hole();
    let s = d.sketch(PlaneRef::Top, vec![rect(-30.0, -40.0, 70.0, 60.0)]);
    d.extrude(s, &[Vec2::new(20.0, 10.0)], |e| {
        e.depth = 2.0;
        e.depth_expr = "2 mm".into();
        e.start_offset = Some(cadrs_core::Offset {
            value: 15.0,
            expr: "15 mm".into(),
            flip: false,
        });
    });
    let parts = d.parts();
    let block = parts
        .iter()
        .find(|p| (p.mass.unwrap().volume - v).abs() < 1e-6)
        .unwrap()
        .clone();
    let plate = parts.iter().find(|p| p.id != block.id).unwrap().id;
    d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Offset,
            end: cadrs_core::direct_edit::MoveEnd::UpTo,
            up_to: Some(cadrs_core::document::UpTo::Part(plate)),
            faces: top_face(&block),
            ..Default::default()
        }),
    );
    let parts = d.parts();
    let grown = parts.iter().find(|p| p.id == block.id).unwrap();
    close(grown.mass.unwrap().volume, v * 1.5, 1e-6);
}

#[test]
fn delete_face_caps_two_openings() {
    // The block's top and bottom deleted with Cap: two openings, each capped (the caps faces of
    // their own): the block with its hole again.
    let (mut d, v) = block_with_hole();
    let part = d.parts()[0].clone();
    let ends = faces_where(&part, |f| {
        f.plane.is_some_and(|p| p.normal()[2].abs() > 0.5)
    });
    assert_eq!(ends.len(), 2);
    d.add(
        "Delete face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::DeleteFace,
            faces: ends,
            void: cadrs_core::direct_edit::VoidFill::Cap,
            ..Default::default()
        }),
    );
    let part = d.parts()[0].clone();
    close(part.mass.unwrap().volume, v, 1e-6);
    assert_eq!(part.solid.faces.len(), 7);
}

#[test]
fn delete_face_caps_openings_nested_three_deep() {
    // A 60 × 60 × 20 block with a ring groove 5 deep round an island (radii 10 to 20): its
    // top's opening holds the groove's (r 20), which holds the island's (r 10). Deleting the
    // top and the island's top with Cap puts back a top round the groove and the island's own
    // cap: the same part (volume and faces).
    let mut d = Doc::new();
    d.block(0.0, 0.0, 60.0, 60.0, 20.0);
    let s = d.sketch(
        PlaneRef::Top,
        vec![circle(30.0, 30.0, 20.0), circle(30.0, 30.0, 10.0)],
    );
    d.extrude(s, &[Vec2::new(45.0, 30.0)], |e| {
        e.op = BooleanOp::Remove;
        e.depth = 5.0;
        e.depth_expr = "5 mm".into();
        e.start_offset = Some(cadrs_core::Offset {
            value: 15.0,
            expr: "15 mm".into(),
            flip: false,
        });
    });
    let before = d.parts()[0].clone();
    let groove = PI * (400.0 - 100.0) * 5.0;
    close(before.mass.unwrap().volume, 72_000.0 - groove, 1e-6);
    let tops = faces_where(&before, |f| {
        f.plane
            .is_some_and(|p| p.normal()[2] > 0.5 && (p.origin[2] - 20.0).abs() < 1e-9)
    });
    assert_eq!(tops.len(), 2, "the block's top and the island's");
    d.add(
        "Delete face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::DeleteFace,
            faces: tops,
            void: cadrs_core::direct_edit::VoidFill::Cap,
            ..Default::default()
        }),
    );
    let after = d.parts()[0].clone();
    close(after.mass.unwrap().volume, 72_000.0 - groove, 1e-6);
    assert_eq!(after.solid.faces.len(), before.solid.faces.len());
}

#[test]
fn offset_up_to_a_bar_across_the_face() {
    // A plain 40 × 20 × 10 block (its top meshed by its 4 corners) and a 4 × 4 bar floating
    // over the top across its middle (x −10…50, y 8…12, z 15…19), its ends off to the sides:
    // none of its corners over the face and none of the face's under it, only its edges cross
    // the face's outline. The top still stops under it, 15 high (12 000).
    let mut d = Doc::new();
    d.block(0.0, 0.0, 40.0, 20.0, 10.0);
    let v = 8000.0;
    let s = d.sketch(PlaneRef::Top, vec![rect(-10.0, 8.0, 50.0, 12.0)]);
    d.extrude(s, &[Vec2::new(20.0, 10.0)], |e| {
        e.depth = 4.0;
        e.depth_expr = "4 mm".into();
        e.start_offset = Some(cadrs_core::Offset {
            value: 15.0,
            expr: "15 mm".into(),
            flip: false,
        });
    });
    let parts = d.parts();
    let block = parts
        .iter()
        .find(|p| (p.mass.unwrap().volume - v).abs() < 1e-6)
        .unwrap()
        .clone();
    let bar = parts.iter().find(|p| p.id != block.id).unwrap().id;
    d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Offset,
            end: cadrs_core::direct_edit::MoveEnd::UpTo,
            up_to: Some(cadrs_core::document::UpTo::Part(bar)),
            faces: top_face(&block),
            ..Default::default()
        }),
    );
    let parts = d.parts();
    let grown = parts.iter().find(|p| p.id == block.id).unwrap();
    close(grown.mass.unwrap().volume, v * 1.5, 1e-6);
}

#[test]
fn offset_up_to_a_round_bar_exactly() {
    // A wide R100 cylinder along x floating over the plain 40 × 20 top (its axis at y = 10,
    // z = 117, its lowest line z = 17, between its mesh's vertices): Offset up to it stops the
    // top exactly there (the kernel's distance, not its mesh's), 17 high: 40·20·17.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 40.0, 20.0, 10.0);
    let s = d.sketch(PlaneRef::Right, vec![circle(10.0, 117.0, 100.0)]);
    d.extrude(s, &[Vec2::new(10.0, 117.0)], |e| {
        e.depth = 60.0;
        e.depth_expr = "60 mm".into();
    });
    let parts = d.parts();
    let block = parts
        .iter()
        .find(|p| (p.mass.unwrap().volume - 8000.0).abs() < 1e-6)
        .unwrap()
        .clone();
    let rod_part = parts.iter().find(|p| p.id != block.id).unwrap().clone();
    let rod = rod_part.id;
    // Turned 7° about its own axis (its end circle's): the same cylinder, its mesh's vertices
    // no longer on its lowest line.
    d.add(
        "Transform",
        FeatureKind::Transform(cadrs_core::transform::TransformFeature {
            transform_type: cadrs_core::transform::TransformType::Rotate,
            parts: vec![rod],
            axis: Some(AxisRef::Edge(edge_near(&rod_part, [0.0, 110.0, 117.0]))),
            angle: 7.0,
            angle_expr: "7 deg".into(),
            ..Default::default()
        }),
    );
    d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Offset,
            end: cadrs_core::direct_edit::MoveEnd::UpTo,
            up_to: Some(cadrs_core::document::UpTo::Part(rod)),
            faces: top_face(&block),
            ..Default::default()
        }),
    );
    let parts = d.parts();
    let grown = parts.iter().find(|p| p.id == block.id).unwrap();
    close(grown.mass.unwrap().volume, 40.0 * 20.0 * 17.0, 1e-6);
}

#[test]
fn offset_up_to_a_part_over_one_piece_of_a_split_top() {
    // A plain 40 × 20 × 10 block, its top split along x = 19 (a Split, Face type, by a plane 19
    // off Right) into two pieces of one face; a 10 × 10 block floating over the second piece
    // only (x 25…35, z 15…20). Offset up to it moves the top's pieces up to z = 15 (12 000).
    let mut d = Doc::new();
    d.block(0.0, 0.0, 40.0, 20.0, 10.0);
    let plane = d.add(
        "Plane",
        FeatureKind::Plane(cadrs_core::plane::PlaneFeature {
            kind: cadrs_core::plane::PlaneType::Offset,
            entities: vec![cadrs_core::plane::PlaneEntity::Plane(PlaneRef::Right)],
            offset: 19.0,
            offset_expr: "19 mm".into(),
            ..Default::default()
        }),
    );
    let part = d.parts()[0].clone();
    let top = top_face(&part);
    let tool = cadrs_core::parts::plane_feature_ref(&d.features(), plane).unwrap();
    d.add(
        "Split",
        FeatureKind::Split(cadrs_core::advanced::SplitFeature {
            split_type: cadrs_core::advanced::SplitType::Face,
            parts: vec![part.id],
            faces: top.clone(),
            tool: Some(cadrs_core::advanced::SplitToolRef::Plane(tool)),
            ..Default::default()
        }),
    );
    let s = d.sketch(PlaneRef::Top, vec![rect(25.0, 5.0, 35.0, 15.0)]);
    d.extrude(s, &[Vec2::new(30.0, 10.0)], |e| {
        e.depth = 5.0;
        e.depth_expr = "5 mm".into();
        e.start_offset = Some(cadrs_core::Offset {
            value: 15.0,
            expr: "15 mm".into(),
            flip: false,
        });
    });
    let parts = d.parts();
    let block = parts.iter().find(|p| p.id == part.id).unwrap().clone();
    let pieces = faces_where(&block, |f| {
        f.plane
            .is_some_and(|p| p.normal()[2] > 0.5 && (p.origin[2] - 10.0).abs() < 1e-9)
    });
    assert_eq!(pieces.len(), 2, "the top in two pieces");
    let target = parts.iter().find(|p| p.id != part.id).unwrap().id;
    d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Offset,
            end: cadrs_core::direct_edit::MoveEnd::UpTo,
            up_to: Some(cadrs_core::document::UpTo::Part(target)),
            faces: pieces,
            ..Default::default()
        }),
    );
    let parts = d.parts();
    let grown = parts.iter().find(|p| p.id == part.id).unwrap();
    close(grown.mass.unwrap().volume, 12_000.0, 1e-6);
}

#[test]
fn offset_up_to_a_part_moves_each_face_along_its_own_normal() {
    // A plain 40 × 20 × 10 block, its right side (x = 40, picked first) and left side (x = 0)
    // offset up to a block beyond the left side only (x −20…−10): the left side reaches it 10
    // out along its own normal (−x), so both sides go 10 out: 60 wide, 12 000. (Measured along
    // the first face's normal, the left side would have been taken 10 in.)
    let mut d = Doc::new();
    d.block(0.0, 0.0, 40.0, 20.0, 10.0);
    d.block(-20.0, 0.0, -10.0, 20.0, 10.0);
    let parts = d.parts();
    let block = parts
        .iter()
        .find(|p| (p.mass.unwrap().volume - 8000.0).abs() < 1e-6)
        .unwrap()
        .clone();
    let target = parts.iter().find(|p| p.id != block.id).unwrap().id;
    let side = |sign: f64| {
        faces_where(&block, move |f| {
            f.plane.is_some_and(|p| p.normal()[0] * sign > 0.5)
        })
    };
    let mut faces = side(1.0);
    faces.extend(side(-1.0));
    d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Offset,
            end: cadrs_core::direct_edit::MoveEnd::UpTo,
            up_to: Some(cadrs_core::document::UpTo::Part(target)),
            faces,
            ..Default::default()
        }),
    );
    let parts = d.parts();
    let grown = parts.iter().find(|p| p.id == block.id).unwrap();
    close(grown.mass.unwrap().volume, 12_000.0, 1e-6);
}

#[test]
fn up_to_a_part_refuses_several_faces_when_one_is_curved() {
    // The top and the hole's wall translated up to a block floating over the top: the kernel
    // can't sweep the wall exactly, and the mesh measures from the first face only (the wall
    // could meet the part first), so it's refused rather than guessed.
    let (mut d, _) = block_with_hole();
    let s = d.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 40.0, 20.0)]);
    d.extrude(s, &[Vec2::new(2.0, 2.0)], |e| {
        e.op = BooleanOp::New;
        e.depth = 5.0;
        e.depth_expr = "5 mm".into();
        e.start_offset = Some(cadrs_core::Offset {
            value: 15.0,
            expr: "15 mm".into(),
            flip: false,
        });
    });
    let parts = d.parts();
    let first = parts[0].clone();
    let floating = parts.iter().find(|p| p.id != first.id).unwrap().id;
    let mut faces = top_face(&first);
    faces.extend(faces_where(&first, is_cylinder));
    let f = d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Translate,
            end: cadrs_core::direct_edit::MoveEnd::UpTo,
            up_to: Some(cadrs_core::document::UpTo::Part(floating)),
            faces,
            ..Default::default()
        }),
    );
    let b = d.build();
    assert!(
        b.errors
            .iter()
            .any(|(id, e)| *id == f && e.contains("several faces")),
        "{:?}",
        b.errors
    );
}

/// A plain 40 × 20 × 10 block with its top split along x = 19 (a Split, Face type, by a plane
/// 19 off Right) into two coplanar pieces: the block, and the piece over x 19…40.
fn block_with_a_split_top() -> (Doc, Part, FaceRef) {
    let mut d = Doc::new();
    d.block(0.0, 0.0, 40.0, 20.0, 10.0);
    let plane = d.add(
        "Plane",
        FeatureKind::Plane(cadrs_core::plane::PlaneFeature {
            kind: cadrs_core::plane::PlaneType::Offset,
            entities: vec![cadrs_core::plane::PlaneEntity::Plane(PlaneRef::Right)],
            offset: 19.0,
            offset_expr: "19 mm".into(),
            ..Default::default()
        }),
    );
    let part = d.parts()[0].clone();
    let tool = cadrs_core::parts::plane_feature_ref(&d.features(), plane).unwrap();
    d.add(
        "Split",
        FeatureKind::Split(cadrs_core::advanced::SplitFeature {
            split_type: cadrs_core::advanced::SplitType::Face,
            parts: vec![part.id],
            faces: top_face(&part),
            tool: Some(cadrs_core::advanced::SplitToolRef::Plane(tool)),
            ..Default::default()
        }),
    );
    let part = d.parts()[0].clone();
    let piece = faces_where(&part, |f| {
        f.plane.is_some_and(|p| p.normal()[2] > 0.5) && f.center.is_some_and(|c| c[0] > 19.0)
    });
    assert_eq!(piece.len(), 1, "the top's piece over x 19…40");
    (d, part, piece[0])
}

#[test]
fn moving_one_piece_of_a_split_top_leaves_a_step() {
    // The piece over x 19…40 offset 5 out of the block: the other piece can't follow it (it is
    // coplanar), so a step 5 high joins them at x = 19, facing −x (#22: Onshape's Move face
    // fills the gap with the swept face's side). 8000 + 21 × 20 × 5; 7 faces. Offset 3 in, a
    // step 3 deep facing +x: 8000 − 21 × 20 × 3. Translated along its normal, the same.
    for (move_type, distance, volume, facing) in [
        (MoveType::Offset, 5.0, 8000.0 + 2100.0, -1.0),
        (MoveType::Offset, -3.0, 8000.0 - 1260.0, 1.0),
        (MoveType::Translate, 5.0, 8000.0 + 2100.0, -1.0),
        (MoveType::Translate, -3.0, 8000.0 - 1260.0, 1.0),
    ] {
        let (mut d, _, piece) = block_with_a_split_top();
        d.add(
            "Move face",
            FeatureKind::DirectEdit(DirectEditFeature {
                kind: DirectEditKind::MoveFace,
                move_type,
                faces: vec![piece],
                distance,
                distance_expr: format!("{distance} mm"),
                ..Default::default()
            }),
        );
        let part = d.parts()[0].clone();
        close(part.mass.unwrap().volume, volume, 1e-6);
        let step = faces_where(&part, |f| {
            f.plane
                .is_some_and(|p| p.normal()[0] * facing > 0.5 && (p.origin[0] - 19.0).abs() < 1e-6)
        });
        assert_eq!(
            step.len(),
            1,
            "the step at x = 19 ({move_type:?} {distance})"
        );
        let area = part
            .solid
            .faces
            .iter()
            .find(|f| f.name == step[0].face)
            .and_then(|f| f.area)
            .unwrap();
        close(area, 20.0 * distance.abs(), 1e-6);
    }
}

#[test]
fn offset_a_filleted_end_with_its_fillets_reapplied() {
    // A 40 × 20 × 10 block with R4 fillets on its right end's top and front edges and on the
    // top front edge running along it, meeting in a corner (a sphere). The right end offset 5
    // with Reapply fillet: its two fillets and the corner go along with it (the faces they join
    // the end to slide along the move), the fillet along x grows 5 longer (#22, as Onshape: a
    // wall end moved with the rounds on it). Its volume grows by 5 × the block's filleted cross
    // section, 200 − (4² − π 4²/4).
    let mut d = Doc::new();
    d.block(0.0, 0.0, 40.0, 20.0, 10.0);
    let part = d.parts()[0].clone();
    d.add(
        "Fillet",
        FeatureKind::Fillet(cadrs_core::applied::FilletFeature {
            entities: [[40.0, 10.0, 10.0], [40.0, 0.0, 5.0], [20.0, 0.0, 10.0]]
                .into_iter()
                .map(|p| cadrs_core::applied::EdgeOrFace::Edge(edge_near(&part, p)))
                .collect(),
            size: 4.0,
            size_expr: "4 mm".into(),
            ..Default::default()
        }),
    );
    let part = d.parts()[0].clone();
    let before = part.mass.unwrap().volume;
    let end = faces_where(&part, |f| {
        f.plane
            .is_some_and(|p| p.normal()[0] > 0.5 && (p.origin[0] - 40.0).abs() < 1e-9)
    });
    assert_eq!(end.len(), 1);
    d.add(
        "Move face",
        FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Offset,
            faces: end,
            distance: 5.0,
            distance_expr: "5 mm".into(),
            ..Default::default()
        }),
    );
    let part = d.parts()[0].clone();
    let section = 200.0 - (16.0 - PI * 16.0 / 4.0);
    close(part.mass.unwrap().volume, before + 5.0 * section, 1e-6);
    // The end's two fillets are R4 again, their axes 5 further along x (41: 40 − 4 + 5).
    let rounds: Vec<_> = part
        .solid
        .faces
        .iter()
        .filter(|f| is_cylinder(f) && f.radius.is_some_and(|r| (r - 4.0).abs() < 1e-9))
        .filter_map(|f| f.axis)
        .collect();
    assert!(
        rounds
            .iter()
            .filter(|(o, z)| z[0].abs() < 1e-9 && (o[0] - 41.0).abs() < 1e-6)
            .count()
            == 2,
        "{rounds:?}"
    );
}
