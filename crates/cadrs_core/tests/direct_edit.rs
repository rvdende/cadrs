//! Direct edits: Delete face and Move face through the feature list. Expected values derived by
//! hand in each test's comments.
#![cfg(feature = "occt")]
#![allow(clippy::field_reassign_with_default)]

use std::f64::consts::PI;

use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude, SetFeature};
use cadrs_core::direct_edit::{DirectEditFeature, DirectEditKind};
use cadrs_core::document::{BooleanOp, Document, ExtrudeFeature, FaceRef, FeatureKind};
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
        Self { d, h: History::default(), el }
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
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    fn extrude(&mut self, s: FeatureId, seeds: &[Vec2], set: impl FnOnce(&mut ExtrudeFeature)) -> FeatureId {
        let g = self.d.element(self.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
        let regions = samples::region_refs(s, &g, seeds);
        assert_eq!(regions.len(), seeds.len(), "a region is missing");
        let mut e = ExtrudeFeature { op: BooleanOp::New, ..samples::extrude_of(regions, 10.0) };
        set(&mut e);
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude: e, label: "Extrude".into() })
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
            .execute(&mut self.d, &AddFeature { element: self.el, feature, base_name: base.into(), kind })
            .unwrap();
        feature
    }

    fn set(&mut self, feature: FeatureId, kind: FeatureKind) {
        self.h.execute(&mut self.d, &SetFeature { element: self.el, feature, kind, label: "Edit".into() }).unwrap();
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
    SketchOp::AddCircle { center: Vec2::new(cx, cy), radius: r, construction: false }
}

/// The faces of the part matching `pick` (its index and the face), as references.
fn faces_where(p: &Part, pick: impl Fn(&cadrs_core::solid::SolidFace) -> bool) -> Vec<FaceRef> {
    p.solid
        .faces
        .iter()
        .enumerate()
        .filter(|(_, f)| pick(f))
        .map(|(i, f)| FaceRef { part: p.id, face: f.name, seed: p.solid.face_point(i).unwrap() })
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
    d.add("Delete face", FeatureKind::DirectEdit(DirectEditFeature { kind: DirectEditKind::DeleteFace, faces: wall, ..Default::default() }));
    close(d.volume(), 8000.0, 1e-6);
}

#[test]
fn move_face_moves_a_wall() {
    // The top (z = 10) moved up 5: the block is 15 high and the hole still goes through it.
    let (mut d, v) = block_with_hole();
    let top = faces_where(&d.parts()[0], |f| f.plane.is_some_and(|p| p.normal()[2] > 0.5 && (p.origin[2] - 10.0).abs() < 1e-9));
    assert_eq!(top.len(), 1);
    d.add("Move face", FeatureKind::DirectEdit(DirectEditFeature { kind: DirectEditKind::MoveFace, faces: top, distance: 5.0, distance_expr: "5 mm".into() }));
    close(d.volume(), v * 1.5, 1e-6);
}

#[test]
fn move_face_resizes_a_hole() {
    // The hole's wall moved 1 along its outward normal (into the hole): Ø10 becomes Ø8.
    let (mut d, _) = block_with_hole();
    let wall = faces_where(&d.parts()[0], is_cylinder);
    d.add("Move face", FeatureKind::DirectEdit(DirectEditFeature { kind: DirectEditKind::MoveFace, faces: wall, distance: 1.0, distance_expr: "1 mm".into() }));
    close(d.volume(), 8000.0 - PI * 16.0 * 10.0, 1e-6);
}

#[test]
fn a_direct_edit_without_faces_says_what_to_pick() {
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 10.0, 10.0);
    let f = d.add("Delete face", FeatureKind::DirectEdit(DirectEditFeature::default()));
    let b = d.build();
    assert!(b.errors.iter().any(|(id, e)| *id == f && e.contains("Select the faces to delete")));
}
