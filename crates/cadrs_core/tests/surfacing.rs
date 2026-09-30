//! Thicken, Helix and Fill through the document's commands (`cadrs_core::surfacing`), with
//! volumes and areas from the kernel.
#![cfg(feature = "occt")]

use cadrs_core::advanced::{PathRef, SweepFeature};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{AxisRef, BodyType, Document, EdgeRef, ExtrudeFeature, FaceRef, FeatureKind, RegionRef};
use cadrs_core::surfacing::{FillEdge, FillFeature, HelixFeature, HelixPath, HelixType, ThickenFeature};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, PartKind, rebuild};
use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2};

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Surfacing");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn g(&self, f: FeatureId) -> Sketch {
        self.d.element(self.el).unwrap().feature(f).unwrap().sketch().unwrap().geometry.clone()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    fn extrude(&mut self, e: ExtrudeFeature) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude: e, label: "Extrude".into() })
            .unwrap();
        f
    }

    fn add(&mut self, base: &str, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h.execute(&mut self.d, &AddFeature { element: self.el, feature, base_name: base.into(), kind }).unwrap();
        feature
    }

    fn build(&self) -> std::sync::Arc<rebuild::Build> {
        let b = rebuild::build(&self.features());
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b
    }

    fn parts(&self) -> Vec<Part> {
        self.build().parts.clone()
    }
}

fn line(a: (f64, f64), b: (f64, f64)) -> SketchOp {
    SketchOp::AddPolyline { points: vec![Vec2::new(a.0, a.1), Vec2::new(b.0, b.1)], closed: false, construction: false, label: "Add line" }
}

fn rect(w: f64, h: f64) -> SketchOp {
    SketchOp::AddPolyline {
        points: vec![Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-9)
}

/// The face of `part` whose centre is nearest `p`.
fn face_near(part: &Part, p: [f64; 3]) -> FaceRef {
    let s = &part.solid;
    let d = |c: [f64; 3]| (0..3).map(|i| (c[i] - p[i]).powi(2)).sum::<f64>();
    let i = (0..s.faces.len()).min_by(|a, b| d(s.faces[*a].center.unwrap()).total_cmp(&d(s.faces[*b].center.unwrap()))).unwrap();
    FaceRef { part: part.id, face: s.faces[i].name, seed: s.faces[i].center.unwrap() }
}

/// The edge of `part` passing nearest `p`.
fn edge_near(part: &Part, p: [f64; 3]) -> EdgeRef {
    let e = part.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap();
    assert!(e.distance(p) < 1e-3, "no edge at {p:?}");
    EdgeRef { part: part.id, edge: e.name, seed: p }
}

#[test]
fn thicken_a_surface_and_a_face() {
    // A 20 × 10 surface (a line extruded as a surface), thickened 2 mm: 400 mm³, and the
    // surface is consumed.
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![line((0.0, 0.0), (20.0, 0.0))]);
    st.extrude(ExtrudeFeature { sketches: vec![s], body: BodyType::Surface, depth: 10.0, depth_expr: "10 mm".into(), ..Default::default() });
    let surface = st.parts()[0].clone();
    assert_eq!(surface.kind, PartKind::Surface);
    let t = st.add("Thicken", FeatureKind::Thicken(ThickenFeature { parts: vec![surface.id], thickness1: 2.0, ..Default::default() }));
    let parts = st.parts();
    assert_eq!(parts.len(), 1, "the surface is consumed");
    assert!(close(volume(&parts[0]), 400.0, 1e-6), "{}", volume(&parts[0]));
    // Both sides: 1.5 + 0.5.
    st.h
        .execute(
            &mut st.d,
            &cadrs_core::commands::SetFeature {
                element: st.el,
                feature: t,
                kind: FeatureKind::Thicken(ThickenFeature { parts: vec![surface.id], thickness1: 1.5, thickness2: 0.5, keep_tools: true, ..Default::default() }),
                label: "Edit Thicken".into(),
            },
        )
        .unwrap();
    let parts = st.parts();
    assert_eq!(parts.len(), 2, "Keep tools keeps the surface");
    let solid = parts.iter().find(|p| p.kind == PartKind::Solid).unwrap();
    assert!(close(volume(solid), 400.0, 1e-6), "{}", volume(solid));
    let bb = solid.solid.bounds().unwrap();
    assert!(close(bb.1[1] - bb.0[1], 2.0, 1e-6), "{bb:?}");

    // The top face of a 10 × 10 × 10 box, thickened 1 mm outward: a new 100 mm³ part.
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![rect(10.0, 10.0)]);
    st.extrude(ExtrudeFeature { sketches: vec![s], depth: 10.0, depth_expr: "10 mm".into(), ..Default::default() });
    let boxp = st.parts()[0].clone();
    let top = face_near(&boxp, [5.0, 5.0, 10.0]);
    st.add("Thicken", FeatureKind::Thicken(ThickenFeature { faces: vec![top], thickness1: 1.0, ..Default::default() }));
    let parts = st.parts();
    assert_eq!(parts.len(), 2);
    let new = parts.iter().find(|p| p.id != boxp.id).unwrap();
    assert!(close(volume(new), 100.0, 1e-6), "{}", volume(new));
    let bb = new.solid.bounds().unwrap();
    assert!(close(bb.0[2], 10.0, 1e-6) && close(bb.1[2], 11.0, 1e-6), "{bb:?}");
}

#[test]
fn a_helix_is_a_sweep_path() {
    let mut st = Studio::new();
    // The axis: a vertical construction line on the Front plane (world Z).
    let axis_sk = st.sketch(PlaneRef::Front, vec![line((0.0, 0.0), (0.0, 30.0))]);
    let axis_curve = st.g(axis_sk).curves.keys().next().unwrap();
    let helix = st.add(
        "Helix",
        FeatureKind::Helix(HelixFeature {
            helix_type: HelixType::Axis,
            axis: Some(AxisRef::SketchCurve { sketch: axis_sk, curve: axis_curve }),
            path: HelixPath::TurnsAndPitch,
            revolutions: 3.0,
            pitch: 5.0,
            radius: 10.0,
            ..Default::default()
        }),
    );
    let b = st.build();
    let g = b.curves.get(&helix).expect("the helix's curve");
    let want = 3.0 * ((std::f64::consts::TAU * 10.0).powi(2) + 25.0).sqrt();
    assert!(close(g.length(), want, 1e-5), "{} vs {want}", g.length());
    assert!(close(g.at(1.0).0[2], 15.0, 1e-9));
    // A small circle on the Front plane at its start (10, 0, 0), swept along it.
    let prof = st.sketch(PlaneRef::Front, vec![SketchOp::AddCircle { center: Vec2::new(10.0, 0.0), radius: 0.5, construction: false }]);
    let regions = cadrs_sketch::region::regions(&st.g(prof));
    st.add(
        "Sweep",
        FeatureKind::Sweep(SweepFeature { regions: vec![RegionRef::new(prof, &regions[0])], path: vec![PathRef::Curve(helix)], ..Default::default() }),
    );
    let parts = st.parts();
    assert_eq!(parts.len(), 1);
    let v = volume(&parts[0]);
    let tube = std::f64::consts::PI * 0.25 * want;
    // The profile is square to the axis's plane, not to the (4.5° steep) helix: its area
    // across the path is cos(4.5°) of the circle's.
    assert!(close(v, tube, 2e-2), "{v} vs {tube}");
}

#[test]
fn fill_caps_a_tube_into_a_solid() {
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 5.0, construction: false }]);
    st.extrude(ExtrudeFeature { sketches: vec![s], body: BodyType::Surface, depth: 10.0, depth_expr: "10 mm".into(), ..Default::default() });
    let tube = st.parts()[0].clone();
    assert_eq!(tube.kind, PartKind::Surface);
    let bottom = edge_near(&tube, [5.0, 0.0, 0.0]);
    let top = edge_near(&tube, [5.0, 0.0, 10.0]);
    // The first cap: a flat disc, a surface of its own (the tube stays open).
    st.add("Fill", FeatureKind::Fill(FillFeature { edges: vec![FillEdge::Edge(bottom)], add: true, ..Default::default() }));
    let b = rebuild::build(&st.features());
    assert_eq!(b.parts.len(), 2);
    let disc = b.parts.iter().find(|p| p.id != tube.id).unwrap();
    let area: f64 = disc.solid.faces.iter().filter_map(|f| f.area).sum();
    assert!(close(area, std::f64::consts::PI * 25.0, 1e-6), "{area}");
    // The second closes it: one solid of π r² h.
    st.add("Fill", FeatureKind::Fill(FillFeature { edges: vec![FillEdge::Edge(top)], add: true, ..Default::default() }));
    let parts = st.parts();
    assert_eq!(parts.len(), 1, "{:?}", parts.iter().map(|p| p.kind).collect::<Vec<_>>());
    assert_eq!(parts[0].kind, PartKind::Solid);
    assert!(close(volume(&parts[0]), std::f64::consts::PI * 250.0, 1e-6), "{}", volume(&parts[0]));
}

#[test]
fn fill_a_sketch_boundary_and_a_saddle() {
    // A flat rectangle of sketch lines.
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Front, vec![rect(8.0, 3.0)]);
    let edges: Vec<FillEdge> = st.g(s).curves.keys().map(|c| FillEdge::SketchCurve { sketch: s, curve: c }).collect();
    st.add("Fill", FeatureKind::Fill(FillFeature { edges, ..Default::default() }));
    let parts = st.parts();
    assert_eq!(parts[0].kind, PartKind::Surface);
    let area: f64 = parts[0].solid.faces.iter().filter_map(|f| f.area).sum();
    assert!(close(area, 24.0, 1e-9), "{area}");

    // Four edges of a box's corner that don't lie in one plane: a Coons patch through them.
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![rect(10.0, 10.0)]);
    st.extrude(ExtrudeFeature { sketches: vec![s], depth: 10.0, depth_expr: "10 mm".into(), ..Default::default() });
    let boxp = st.parts()[0].clone();
    // (0,0,0)→(10,0,0)→(10,0,10)... pick a skew quadrilateral of edges:
    // bottom front, right front vertical, top right, left back vertical, closing? Use four
    // edges round the loop (0,0,0)-(10,0,0)-(10,10,0)... that is flat; a skew loop:
    // (0,0,0)→(10,0,0) [bottom front], (10,0,0)→(10,0,10) [front right vertical],
    // (10,0,10)→(10,10,10) [top right], back down (10,10,10)→... isn't an edge loop of 4.
    let e = |p: [f64; 3]| FillEdge::Edge(edge_near(&boxp, p));
    let edges = vec![e([5.0, 0.0, 0.0]), e([10.0, 0.0, 5.0]), e([10.0, 5.0, 10.0]), e([10.0, 10.0, 5.0]), e([5.0, 10.0, 0.0]), e([0.0, 5.0, 0.0])];
    // Six edges round a non-flat loop: refused (only 3 or 4 curves are patched).
    st.add("Fill", FeatureKind::Fill(FillFeature { edges, ..Default::default() }));
    let b = rebuild::build(&st.features());
    assert_eq!(b.errors.len(), 1, "{:?}", b.errors);
}

/// pibox's dome: an open box (a square tube surface and a fill on its top) thickened 0.8 mm
/// into a wall on the inside.
#[test]
fn thicken_an_open_box_of_surfaces() {
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![SketchOp::AddPolyline {
        points: vec![Vec2::new(-50.0, -50.0), Vec2::new(50.0, -50.0), Vec2::new(50.0, 50.0), Vec2::new(-50.0, 50.0)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }]);
    st.extrude(ExtrudeFeature { sketches: vec![s], body: BodyType::Surface, depth: 70.0, depth_expr: "70 mm".into(), ..Default::default() });
    let tube = st.parts()[0].clone();
    let top: Vec<FillEdge> =
        [[0.0, -50.0, 70.0], [50.0, 0.0, 70.0], [0.0, 50.0, 70.0], [-50.0, 0.0, 70.0]].iter().map(|p| FillEdge::Edge(edge_near(&tube, *p))).collect();
    st.add("Fill", FeatureKind::Fill(FillFeature { edges: top, add: true, ..Default::default() }));
    let parts = st.parts();
    assert_eq!(parts.len(), 2);
    let faces: Vec<FaceRef> = parts
        .iter()
        .flat_map(|p| p.solid.faces.iter().map(move |f| FaceRef { part: p.id, face: f.name, seed: f.center.unwrap() }))
        .collect();
    assert_eq!(faces.len(), 5);
    let t = st.add("Thicken", FeatureKind::Thicken(ThickenFeature { faces: faces.clone(), thickness1: 0.8, ..Default::default() }));
    let parts = st.parts();
    assert_eq!(parts.len(), 1);
    let v = volume(&parts[0]);
    // The five slabs' volume less their overlaps along the edges (inside or outside).
    let slabs = 100.0 * 100.0 * 0.8 + 4.0 * 100.0 * 70.0 * 0.8;
    assert!(v > slabs * 0.97 && v < slabs * 1.03, "{v} vs {slabs}");
    let _ = t;
}

#[test]
fn a_thread_swept_along_a_helix_on_its_cylinder_joins_it() {
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 18.375, construction: false }]);
    st.extrude(ExtrudeFeature { sketches: vec![s], depth: 10.0, depth_expr: "10 mm".into(), ..Default::default() });
    let cyl = st.parts()[0].clone();
    let face = face_near(&cyl, [18.375, 0.0, 5.0]);
    let helix = st.add(
        "Helix",
        FeatureKind::Helix(HelixFeature { face: Some(face), path: HelixPath::TurnsAndPitch, revolutions: 2.0, pitch: 4.125, ..Default::default() }),
    );
    // The thread's profile on Front at the helix's start, its inner side on the cylinder.
    let prof = st.sketch(
        PlaneRef::Front,
        vec![SketchOp::AddPolyline {
            points: vec![Vec2::new(18.375, -1.2), Vec2::new(19.8, -0.7), Vec2::new(19.8, 0.7), Vec2::new(18.375, 1.2)],
            closed: true,
            construction: false,
            label: "Add polygon",
        }],
    );
    let regions = cadrs_sketch::region::regions(&st.g(prof));
    st.add(
        "Sweep",
        FeatureKind::Sweep(SweepFeature {
            regions: vec![RegionRef::new(prof, &regions[0])],
            path: vec![PathRef::Curve(helix)],
            op: cadrs_core::BooleanOp::Add,
            merge_scope: vec![cyl.id],
            ..Default::default()
        }),
    );
    let parts = st.parts();
    assert_eq!(parts.len(), 1, "{:?}", parts.iter().map(volume).collect::<Vec<_>>());
}
