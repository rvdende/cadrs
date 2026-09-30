//! Neighbouring faces of a part on one surface are one face, as Onshape (Parasolid) leaves
//! them: the kernel merges them after every operation. Two touching regions extruded together
//! have one cap, a block added flush with another continues its faces, and references made
//! through any of the merged faces' names (a sketch on a face, an edge to fillet) still work.
//! The face counts are Onshape's for the same parts.
#![cfg(feature = "occt")]

use std::f64::consts::PI;

use cadrs_core::applied::{EdgeOrFace, FilletFeature};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, MoveFeature, SetExtrude};
use cadrs_core::document::{BooleanOp, Document, EdgeRef, ExtrudeFeature, Offset};
use cadrs_core::parts::{face_plane, refresh_face_planes, sketch_face_lost_in};
use cadrs_core::rebuild::{self, Build};
use cadrs_core::samples;
use cadrs_core::{ElementId, Feature, FeatureId, History, Part};
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
        let d = Document::new("Merged faces");
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

    fn part(&self) -> Part {
        let parts = self.parts();
        assert_eq!(parts.len(), 1);
        parts[0].clone()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    /// Extrude `id` of the regions of sketch `s` around `seeds`, with `f` applied to it.
    fn extrude(&mut self, id: FeatureId, s: FeatureId, seeds: &[Vec2], depth: f64, f: impl FnOnce(&mut ExtrudeFeature)) -> FeatureId {
        let g = self.d.element(self.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
        let regions = samples::region_refs(s, &g, seeds);
        assert_eq!(regions.len(), seeds.len());
        let mut e = ExtrudeFeature { op: BooleanOp::New, ..samples::extrude_of(regions, depth) };
        f(&mut e);
        self.h.execute(&mut self.d, &AddExtrude { element: self.el, feature: id, extrude: ExtrudeFeature::default() }).unwrap();
        self.h.execute(&mut self.d, &SetExtrude { element: self.el, feature: id, extrude: e, label: "Extrude".into() }).unwrap();
        id
    }

    /// A box x0..x1 × 0..10 on Top, 10 high: New, or added to the part.
    fn block(&mut self, id: FeatureId, x0: f64, x1: f64, op: BooleanOp) -> FeatureId {
        let s = self.sketch(PlaneRef::Top, vec![rect(x0, 0.0, x1, 10.0)]);
        self.extrude(id, s, &[Vec2::new((x0 + x1) / 2.0, 5.0)], 10.0, |e| e.op = op)
    }

    fn position(&self, f: FeatureId) -> usize {
        self.features().iter().position(|x| x.id == f).unwrap()
    }

    /// Moves extrude `e` and its sketch in before feature `before`.
    fn move_before(&mut self, e: FeatureId, before: FeatureId) {
        let sketch = self.features()[self.position(e)].extrude().unwrap().regions[0].sketch;
        for f in [sketch, e] {
            let to = self.position(before);
            self.h.execute(&mut self.d, &MoveFeature { element: self.el, feature: f, to, label: "Reorder".into() }).unwrap();
        }
        assert!(self.position(e) < self.position(before));
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    let v = Vec2::new;
    SketchOp::AddPolyline { points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)], closed: true, construction: false, label: "Add rectangle" }
}

fn id(n: u128) -> FeatureId {
    FeatureId(uuid::Uuid::from_u128(n))
}

/// The planar faces of a part facing along `n`.
fn facing(part: &Part, n: [f64; 3]) -> usize {
    part.solid
        .faces
        .iter()
        .filter(|f| {
            f.plane.is_some_and(|p| {
                let m = p.normal();
                let l = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
                (m[0] * n[0] + m[1] * n[1] + m[2] * n[2]) / l > 0.999
            })
        })
        .count()
}

fn edge_at(part: &Part, p: [f64; 3]) -> EdgeRef {
    let e = part.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap();
    assert!(e.distance(p) < 1e-3, "no edge at {p:?}");
    EdgeRef { part: part.id, edge: e.name, seed: p }
}

#[test]
fn touching_regions_extrude_to_one_top_face() {
    // Two 10 × 10 squares side by side, both regions in one extrude: a 20 × 10 × 5 box, 6 faces
    // (one top, one bottom, one front and one back), 12 edges.
    let mut d = Doc::new();
    let s = d.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 10.0, 10.0), rect(10.0, 0.0, 20.0, 10.0)]);
    d.extrude(id(1), s, &[Vec2::new(5.0, 5.0), Vec2::new(15.0, 5.0)], 5.0, |_| {});
    let part = d.part();
    close(part.mass.unwrap().volume, 1000.0, 1e-6);
    assert_eq!(part.solid.faces.len(), 6);
    assert_eq!(part.solid.edges.len(), 12);
    assert_eq!(facing(&part, [0.0, 0.0, 1.0]), 1);
    assert_eq!(facing(&part, [0.0, -1.0, 0.0]), 1);
    // An L of three squares: 8 faces, as Onshape has.
    let mut d = Doc::new();
    let s = d.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 10.0, 10.0), rect(10.0, 0.0, 20.0, 10.0), rect(0.0, 10.0, 10.0, 20.0)]);
    d.extrude(id(1), s, &[Vec2::new(5.0, 5.0), Vec2::new(15.0, 5.0), Vec2::new(5.0, 15.0)], 5.0, |_| {});
    let part = d.part();
    assert_eq!(part.solid.faces.len(), 8);
    assert_eq!(facing(&part, [0.0, 0.0, 1.0]), 1);
}

#[test]
fn a_flush_add_continues_the_faces() {
    // A 10 × 10 × 10 block and a 5 × 10 × 10 one added against its right side, flush: one
    // 15 × 10 × 10 box of 6 faces.
    let mut d = Doc::new();
    d.block(id(0x200), 0.0, 10.0, BooleanOp::New);
    d.block(id(0x100), 10.0, 15.0, BooleanOp::Add);
    let part = d.part();
    close(part.mass.unwrap().volume, 1500.0, 1e-6);
    assert_eq!(part.solid.faces.len(), 6, "{:?}", part.solid.faces.iter().map(|f| f.name).collect::<Vec<_>>());
    assert_eq!(part.solid.edges.len(), 12);
    // A cylinder r 10, z 0..10, and one on top of it from the same circle (started 10 up),
    // added: one side face, 3 faces in all.
    let mut d = Doc::new();
    let s = d.sketch(PlaneRef::Top, vec![SketchOp::AddCircle { center: Vec2::ZERO, radius: 10.0, construction: false }]);
    d.extrude(id(1), s, &[Vec2::ZERO], 10.0, |_| {});
    d.extrude(id(2), s, &[Vec2::ZERO], 10.0, |e| {
        e.op = BooleanOp::Add;
        e.start_offset = Some(Offset { value: 10.0, expr: "10 mm".into(), flip: false });
    });
    let part = d.part();
    close(part.mass.unwrap().volume, PI * 100.0 * 20.0, 1e-6);
    assert_eq!(part.solid.faces.len(), 3, "{:?}", part.solid.faces.iter().map(|f| f.name).collect::<Vec<_>>());
}

#[test]
fn a_fillet_on_a_merged_edge() {
    // The 15 × 10 × 10 box of two blocks: its top front edge is one edge, 15 long; an R2 fillet
    // on it takes (1 − π/4)·2²·15. Picked before the second block was added (its name is the
    // first block's faces', merged away since), the fillet rounds the whole merged edge.
    let mut d = Doc::new();
    d.block(id(0x200), 0.0, 10.0, BooleanOp::New);
    let before = d.part();
    let edge = edge_at(&before, [5.0, 0.0, 10.0]);
    let fillet = FilletFeature { entities: vec![EdgeOrFace::Edge(edge)], size: 2.0, size_expr: "2 mm".into(), ..FilletFeature::default() };
    let f = FeatureId::new();
    d.h.execute(&mut d.d, &AddFeature::fillet(d.el, f, fillet)).unwrap();
    close(d.part().mass.unwrap().volume, 1000.0 - (1.0 - PI / 4.0) * 4.0 * 10.0, 1e-6);
    // The second block, moved in before the fillet.
    let b = d.block(id(0x100), 10.0, 15.0, BooleanOp::Add);
    d.move_before(b, f);
    let part = d.part();
    close(part.mass.unwrap().volume, 1500.0 - (1.0 - PI / 4.0) * 4.0 * 15.0, 1e-6);
    // Picked on the merged box: the same.
    let mut d = Doc::new();
    d.block(id(0x200), 0.0, 10.0, BooleanOp::New);
    d.block(id(0x100), 10.0, 15.0, BooleanOp::Add);
    let edge = edge_at(&d.part(), [12.0, 0.0, 10.0]);
    let fillet = FilletFeature { entities: vec![EdgeOrFace::Edge(edge)], size: 2.0, size_expr: "2 mm".into(), ..FilletFeature::default() };
    d.h.execute(&mut d.d, &AddFeature::fillet(d.el, FeatureId::new(), fillet)).unwrap();
    close(d.part().mass.unwrap().volume, 1500.0 - (1.0 - PI / 4.0) * 4.0 * 15.0, 1e-6);
}

#[test]
fn a_sketch_on_a_merged_face_regenerates() {
    // A sketch on the first block's front face (y = 0) with a Ø2 hole cut 3 deep. The second
    // block is then moved in before the sketch: the front face is merged with the second
    // block's (and named after it, the smaller name), and the sketch stays on it, by its own
    // name, where it was. (Both ways round:
    // the first block's front face named after the second's, or the second's after the first's.)
    for (first, second) in [(0x200, 0x100), (0x100, 0x200)] {
        let mut d = Doc::new();
        let a = d.block(id(first), 0.0, 10.0, BooleanOp::New);
        let part = d.part();
        let front = part
            .solid
            .faces
            .iter()
            .position(|f| f.plane.is_some_and(|p| p.normal()[1] < -0.999))
            .unwrap();
        let name = part.solid.faces[front].name;
        let plane = face_plane(&d.features(), a, name).unwrap();
        let frame = plane.frame();
        let c = frame.to_sketch([3.0, 0.0, 5.0]);
        let s = d.sketch(plane, vec![SketchOp::AddCircle { center: c, radius: 1.0, construction: false }]);
        d.extrude(FeatureId::new(), s, &[c], 3.0, |e| {
            e.op = BooleanOp::Remove;
            e.flip = true;
        });
        close(d.part().mass.unwrap().volume, 1000.0 - PI * 3.0, 1e-6);
        let b = d.block(id(second), 10.0, 15.0, BooleanOp::Add);
        d.move_before(b, s);
        let mut features = d.features();
        refresh_face_planes(&mut features);
        let i = features.iter().position(|f| f.id == s).unwrap();
        let sk = features[i].sketch().unwrap();
        let Some(PlaneRef::Face(fp)) = sk.plane else { panic!() };
        assert_eq!(fp.face, name, "the sketch keeps its face's name");
        assert_eq!(fp.frame(), frame, "the sketch stays where it was");
        let build = rebuild::build(&features);
        assert!(build.errors.is_empty(), "{:?}", build.errors);
        assert!(!sketch_face_lost_in(&features, i, &build.parts));
        let part = &build.parts[0];
        // One front face (and the hole's floor, 3 in, facing the same way).
        assert_eq!(facing(part, [0.0, -1.0, 0.0]), 2, "one front face");
        close(part.mass.unwrap().volume, 1500.0 - PI * 3.0, 1e-6);
    }
}
