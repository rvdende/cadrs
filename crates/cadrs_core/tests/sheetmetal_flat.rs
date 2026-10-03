//! Modelling in the flat (P3I.6, SM14) through the document's commands: a sketch on the flat
//! pattern plane, extruded Remove across a bend (the cut keeps its exact flat size and the folded
//! part loses exactly that material) and Add (a tab appears folded); an ordinary Extrude of a
//! flat-pattern sketch fails (SM14.3).
#![cfg(feature = "occt")]

use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{Document, ExtrudeFeature, FeatureKind};
use cadrs_core::rebuild::Build;
use cadrs_core::sheetmetal::{SheetMetalModelFeature, SheetMetalOp};
use cadrs_core::sheetmetal_flat::{FlatExtrudeFeature, MODEL_SPACE, flat_plane_id};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, rebuild};
use cadrs_sheetmetal::Params;
use cadrs_sheetmetal::flat::PieceSource;
use cadrs_sheetmetal::poly::{P2, perp};
use cadrs_sketch::{FeaturePlane, PlaneRef, SketchOp, Vec2};

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Flat");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    fn add(&mut self, base: &str, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h.execute(&mut self.d, &AddFeature { element: self.el, feature, base_name: base.into(), kind }).unwrap();
        feature
    }

    fn build(&self) -> std::sync::Arc<Build> {
        rebuild::build(&self.features())
    }

    fn ok(&self) -> std::sync::Arc<Build> {
        let b = self.build();
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b
    }

    /// A U channel: a sheet metal Extrude of an open U (30 up, 60 across, 30 up) on Front, 100
    /// deep, 2 thick, bend radius 3.
    fn channel(&mut self) -> FeatureId {
        let s = self.sketch(PlaneRef::Front, vec![poly(&[(0.0, 30.0), (0.0, 0.0), (60.0, 0.0), (60.0, 30.0)], false)]);
        let p = Params { thickness: 2.0, bend_radius: 3.0, ..SheetMetalModelFeature::default_params() };
        let x = SheetMetalModelFeature {
            operation: SheetMetalOp::Extrude,
            sketches: vec![s],
            depth: 100.0,
            depth_expr: "100 mm".into(),
            params: p,
            exprs: cadrs_core::sheetmetal::SheetMetalExprs::of(&p),
            ..Default::default()
        };
        self.add("Sheet metal model", FeatureKind::SheetMetalModel(x))
    }

    /// A sketch on the model's first flat part (its plane as the rebuild has it).
    fn flat_sketch(&mut self, model: FeatureId, ops: Vec<SketchOp>) -> FeatureId {
        let b = self.ok();
        let id = flat_plane_id(model, 0);
        let frame = *b.planes.get(&FeatureId(id)).expect("the flat pattern plane");
        self.sketch(PlaneRef::Feature(FeaturePlane::new(id, frame)), ops)
    }
}

fn poly(pts: &[(f64, f64)], closed: bool) -> SketchOp {
    SketchOp::AddPolyline { points: pts.iter().map(|(x, y)| Vec2::new(*x, *y)).collect(), closed, construction: false, label: "Add line" }
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
}

/// The folded volume the flat predicts: walls' flat area × T, bend regions scaled from the neutral
/// to the mid-thickness radius.
fn predicted(b: &Build) -> f64 {
    let ctx = &b.sheet_metal[0];
    let p = &ctx.model.params;
    let t = p.thickness;
    ctx.flat.parts[0]
        .pieces
        .iter()
        .map(|piece| {
            let area: f64 = piece.cut.iter().map(|c| c.area()).sum();
            match piece.source {
                PieceSource::Wall(_) => area * t,
                PieceSource::Bend(j) => {
                    let bend = ctx.model.joint(j).unwrap().bend().unwrap();
                    area * t * (bend.radius + t / 2.0) / (bend.radius + p.k_factor * t)
                }
            }
        })
        .sum()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-9)
}

#[test]
fn a_flat_cut_across_a_bend_keeps_its_flat_size_folded() {
    let mut st = Studio::new();
    let model = st.channel();
    let b = st.ok();
    let v0 = volume(&b.parts[0]);
    let area0 = b.sheet_metal[0].flat.parts[0].area();
    // The lesson's slot: 0.5 × 6.0 in (12.7 × 152.4 mm would be longer than the part: 12.7 × 50
    // here), across the first bend, square to it, centred on its centreline.
    let bend = b.sheet_metal[0].flat.parts[0].bends[0].clone();
    eprintln!("channel flat: bounds {:?} bends {:?} plane {:?}", b.sheet_metal[0].flat.parts[0].bounds(), b.sheet_metal[0].flat.parts[0].bends.iter().map(|b| b.center).collect::<Vec<_>>(), b.planes.get(&FeatureId(flat_plane_id(model, 0))));
    let c = P2::from((bend.center.a.coords + bend.center.b.coords) / 2.0);
    let (d, n) = (bend.center.dir(), perp(bend.center.dir()));
    let (w, l) = (12.7, 50.0);
    let corners = [c - d * (w / 2.0) - n * (l / 2.0), c + d * (w / 2.0) - n * (l / 2.0), c + d * (w / 2.0) + n * (l / 2.0), c - d * (w / 2.0) + n * (l / 2.0)];
    let s = st.flat_sketch(model, vec![poly(&corners.map(|q| (q.x, q.y)), true)]);
    st.add("Extrude", FeatureKind::FlatExtrude(FlatExtrudeFeature { remove: true, sketches: vec![s], ..Default::default() }));
    let b = st.ok();
    assert_eq!(b.parts.len(), 1);
    let flat = &b.sheet_metal[0].flat.parts[0];
    // The flat loses exactly the slot, as one hole w × l.
    assert!((area0 - flat.area() - w * l).abs() < 1e-6, "{}", area0 - flat.area());
    assert_eq!(flat.outline.len(), 1);
    let holes: Vec<&Vec<P2>> = flat.outline.iter().flat_map(|o| o.holes.iter()).collect();
    assert_eq!(holes.len(), 1);
    let along: Vec<f64> = holes[0].iter().map(|q| (q - c).dot(&d)).collect();
    let across: Vec<f64> = holes[0].iter().map(|q| (q - c).dot(&n)).collect();
    let span = |v: &[f64]| v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min);
    assert!((span(&along) - w).abs() < 1e-6 && (span(&across) - l).abs() < 1e-6, "{} × {}", span(&along), span(&across));
    // Folded: the part loses that material, wrapped round the bend (walls flat, the bend region
    // at its mid-thickness radius).
    let v1 = volume(&b.parts[0]);
    assert!(close(v1, predicted(&b), 1e-6), "{v1} vs {}", predicted(&b));
    assert!(v0 - v1 > w * l * 2.0 * 0.9, "{v0} → {v1}");
    // The part keeps its id and name.
    assert_eq!(b.parts[0].name, "Part 1");
}

#[test]
fn a_tab_added_in_the_flat_appears_folded() {
    let mut st = Studio::new();
    let model = st.channel();
    let b = st.ok();
    let v0 = volume(&b.parts[0]);
    let flat = &b.sheet_metal[0].flat.parts[0];
    let (lo, hi) = flat.bounds().unwrap();
    // A 20 × 8 tab on the flat's edge at its lowest y, in the middle of x.
    let mx = (lo.x + hi.x) / 2.0;
    let s = st.flat_sketch(model, vec![poly(&[(mx - 10.0, lo.y - 8.0), (mx + 10.0, lo.y - 8.0), (mx + 10.0, lo.y), (mx - 10.0, lo.y)], true)]);
    st.add("Extrude", FeatureKind::FlatExtrude(FlatExtrudeFeature { remove: false, sketches: vec![s], ..Default::default() }));
    let b = st.ok();
    let v1 = volume(&b.parts[0]);
    assert!(close(v1 - v0, 20.0 * 8.0 * 2.0, 1e-6), "{}", v1 - v0);
    assert!(close(v1, predicted(&b), 1e-6));
}

#[test]
fn an_ordinary_extrude_of_a_flat_sketch_fails() {
    let mut st = Studio::new();
    let model = st.channel();
    let (lo, hi) = st.ok().sheet_metal[0].flat.parts[0].bounds().unwrap();
    let (cx, cy) = ((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
    let s = st.flat_sketch(model, vec![poly(&[(cx - 5.0, cy - 5.0), (cx + 5.0, cy - 5.0), (cx + 5.0, cy + 5.0), (cx - 5.0, cy + 5.0)], true)]);
    let f = FeatureId::new();
    st.h.execute(&mut st.d, &AddExtrude { element: st.el, feature: f, extrude: ExtrudeFeature::default() }).unwrap();
    let e = ExtrudeFeature { sketches: vec![s], depth: 5.0, depth_expr: "5 mm".into(), ..Default::default() };
    st.h.execute(&mut st.d, &SetExtrude { element: st.el, feature: f, extrude: e, label: "Extrude".into() }).unwrap();
    let b = st.build();
    assert_eq!(b.errors.iter().find(|(id, _)| *id == f).map(|(_, e)| e.as_str()), Some(MODEL_SPACE), "{:?}", b.errors);
    // A flat pattern extrude of the same sketch rebuilds.
    let g = st.add("Extrude", FeatureKind::FlatExtrude(FlatExtrudeFeature { remove: true, sketches: vec![s], ..Default::default() }));
    let b = st.build();
    assert!(!b.errors.iter().any(|(id, _)| *id == g), "{:?}", b.errors);
}
