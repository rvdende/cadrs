//! Modify joint and the Sheet metal table's edits through the document's commands (P3I.3,
//! SM6.4, SM13.3, SM13.4): a converted open box whose bends are changed, made rips and back,
//! reordered, and undone; the refolded parts keep their ids and their volumes follow the flat.
#![cfg(feature = "occt")]

use cadrs_core::applied::EdgeOrFace;
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{Document, EdgeRef, ExtrudeFeature, FeatureKind};
use cadrs_core::rebuild::Build;
use cadrs_core::sheetmetal::{SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};
use cadrs_core::sheetmetal_joint::{PutModifyJoint, SetTableOrder, TableEdit, modify_joint_of, table_edit};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, rebuild};
use cadrs_sheetmetal::{JointId, JointKind, Params, RipStyle};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Sheet metal");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn build(&self) -> std::sync::Arc<Build> {
        rebuild::build(&self.features())
    }

    fn ok(&self) -> std::sync::Arc<Build> {
        let b = self.build();
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b
    }

    /// A 100 × 60 × 40 block, converted with its bottom's four edges bent (T 2, R 3): an open
    /// box and the separate top.
    fn open_box(&mut self) -> FeatureId {
        let s = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: s, plane: Some(PlaneRef::Top) }).unwrap();
        let op = SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 60.0), Vec2::new(0.0, 60.0)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        };
        self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: s, op }).unwrap();
        let e = FeatureId::new();
        self.h.execute(&mut self.d, &AddExtrude { element: self.el, feature: e, extrude: ExtrudeFeature::default() }).unwrap();
        let ex = ExtrudeFeature { sketches: vec![s], depth: 40.0, depth_expr: "40 mm".into(), ..Default::default() };
        self.h.execute(&mut self.d, &SetExtrude { element: self.el, feature: e, extrude: ex, label: "Extrude".into() }).unwrap();
        let block = self.ok().parts[0].clone();
        let p = Params { thickness: 2.0, bend_radius: 3.0, k_factor: 0.45, minimal_gap: 0.2, ..SheetMetalModelFeature::default_params() };
        let mut x = SheetMetalModelFeature { operation: SheetMetalOp::Convert, params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
        x.parts = vec![block.id];
        x.bends = [[50.0, 0.0, 0.0], [100.0, 30.0, 0.0], [50.0, 60.0, 0.0], [0.0, 30.0, 0.0]].iter().map(|q| EdgeOrFace::Edge(edge_near(&block, *q))).collect();
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddFeature { element: self.el, feature: f, base_name: "Sheet metal model".into(), kind: FeatureKind::SheetMetalModel(x) }).unwrap();
        f
    }

    /// A table edit, as the panel makes it: the joint's Modify joint edited, or a new one.
    fn table(&mut self, model: FeatureId, joint: &str, edit: TableEdit) -> FeatureId {
        let b = self.build();
        let ctx = b.sheet_metal.iter().find(|c| c.feature == model).expect("context");
        let j = ctx.model.joints.iter().find(|j| j.name == joint).unwrap_or_else(|| panic!("no {joint}"));
        let features = self.features();
        let existing = modify_joint_of(&features, model, j.id);
        let prev = existing.and_then(|f| match &f.kind {
            FeatureKind::ModifyJoint(x) => Some(x.clone()),
            _ => None,
        });
        let feature = existing.map_or_else(FeatureId::new, |f| f.id);
        let x = table_edit(model, prev.as_ref(), j, &ctx.model.params, &edit);
        let label = edit.label(joint);
        let after = ctx.editors.clone();
        self.h.execute(&mut self.d, &PutModifyJoint { element: self.el, feature, joint: x, after, label }).unwrap();
        feature
    }

    fn undo(&mut self) {
        self.h.undo(&mut self.d).expect("something to undo");
    }
}

fn edge_near(part: &Part, p: [f64; 3]) -> EdgeRef {
    let e = part.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap();
    assert!(e.distance(p) < 1e-3, "no edge at {p:?}");
    EdgeRef { part: part.id, edge: e.name, seed: p }
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
}

fn ids(b: &Build) -> Vec<cadrs_core::PartId> {
    let mut v: Vec<_> = b.parts.iter().map(|p| p.id).collect();
    v.sort();
    v
}

fn joint<'a>(b: &'a Build, model: FeatureId, name: &str) -> &'a cadrs_sheetmetal::Joint {
    let ctx = b.sheet_metal.iter().find(|c| c.feature == model).unwrap();
    ctx.model.joints.iter().find(|j| j.name == name).unwrap_or_else(|| panic!("no {name}"))
}

#[test]
fn a_radius_and_a_k_factor_from_the_table_refold_the_part_in_place() {
    let mut st = Studio::new();
    let m = st.open_box();
    let before = st.ok();
    let box_before = before.parts.iter().max_by(|a, b| volume(a).total_cmp(&volume(b))).unwrap().clone();
    // Double-click Bend B's radius: 6 mm. A Modify joint appears right after the model.
    let f = st.table(m, "Bend B", TableEdit::Radius(6.0, "6 mm".into()));
    let names: Vec<String> = st.features().iter().map(|f| f.name.clone()).collect();
    assert_eq!(names.last().map(String::as_str), Some("Modify joint 1"), "{names:?}");
    let after = st.ok();
    assert_eq!(ids(&after), ids(&before), "the parts keep their ids");
    let bend = joint(&after, m, "Bend B").bend().unwrap();
    assert_eq!(bend.radius, 6.0);
    let box_after = after.parts.iter().find(|p| p.id == box_before.id).unwrap();
    assert!((volume(box_after) - volume(&box_before)).abs() > 1.0, "the box changed");
    // The K factor cell: the same Modify joint, edited.
    let g = st.table(m, "Bend B", TableEdit::Value(0.3, "0.3".into()));
    assert_eq!(g, f);
    assert_eq!(st.features().iter().filter(|f| matches!(f.kind, FeatureKind::ModifyJoint(_))).count(), 1);
    let after = st.ok();
    let bend = joint(&after, m, "Bend B").bend().unwrap();
    assert_eq!((bend.radius, bend.value), (6.0, Some(cadrs_sheetmetal::BendValue::KFactor(0.3))));
    // Out of range: the feature fails with the range, the model stays as it was.
    st.table(m, "Bend B", TableEdit::Value(1.2, "1.2".into()));
    let bad = st.build();
    assert_eq!(bad.errors.iter().find(|(id, _)| *id == f).map(|(_, e)| e.as_str()), Some("K Factor must be between -1.5 and 1"));
    assert_eq!(joint(&bad, m, "Bend B").bend().unwrap().radius, 3.0);
    // Undo, twice: back to radius 6, K 0.3, then K from the model.
    st.undo();
    assert_eq!(joint(&st.ok(), m, "Bend B").bend().unwrap().value, Some(cadrs_sheetmetal::BendValue::KFactor(0.3)));
    st.undo();
    assert_eq!(joint(&st.ok(), m, "Bend B").bend().unwrap().value, None);
    st.undo();
    assert!(st.features().iter().all(|f| !matches!(f.kind, FeatureKind::ModifyJoint(_))));
    assert_eq!(joint(&st.ok(), m, "Bend B").bend().unwrap().radius, 3.0);
}

#[test]
fn a_bend_converted_to_a_rip_and_back_and_a_rip_style() {
    let mut st = Studio::new();
    let m = st.open_box();
    let before = st.ok();
    assert_eq!(before.parts.len(), 2);
    // Convert Bend A to rip: it moves to Other joints as "Joint A"; the south wall comes off.
    st.table(m, "Bend A", TableEdit::ConvertToRip);
    let after = st.ok();
    let a = joint(&after, m, "Joint A");
    assert!(matches!(a.kind, JointKind::Rip { style: RipStyle::EdgeJoint, .. }));
    assert_eq!(after.parts.len(), 3);
    for p in &before.parts {
        assert!(after.parts.iter().any(|q| q.id == p.id), "{} kept", p.name);
    }
    // Its Style: Butt joint – Direction 1 (a 90° joint).
    st.table(m, "Joint A", TableEdit::RipStyle(RipStyle::ButtDirection1));
    let styled = st.ok();
    assert!(matches!(joint(&styled, m, "Joint A").kind, JointKind::Rip { style: RipStyle::ButtDirection1, .. }));
    // Its Type back to Bend: one Modify joint all along, and two parts again.
    st.table(m, "Joint A", TableEdit::ConvertToBend);
    let back = st.ok();
    assert!(joint(&back, m, "Bend A").bend().is_some());
    assert_eq!(back.parts.len(), 2);
    assert_eq!(st.features().iter().filter(|f| matches!(f.kind, FeatureKind::ModifyJoint(_))).count(), 1);
    // A rip of the model made a bend: the top joins the box (one part).
    let rip = before.sheet_metal[0].model.joints.iter().find(|j| matches!(j.kind, JointKind::Rip { .. }) && {
        let ctx = &before.sheet_metal[0];
        let top = ctx.parts.iter().find(|(_, w)| w.len() == 1).unwrap().1[0];
        j.a == top || j.b == top
    });
    let name = rip.unwrap().name.clone();
    st.table(m, &name, TableEdit::ConvertToBend);
    let joined = st.ok();
    assert_eq!(joined.parts.len(), 1, "{:?}", joined.parts.iter().map(|p| &p.name).collect::<Vec<_>>());
}

#[test]
fn move_up_and_down_keep_the_table_order_with_the_model() {
    let mut st = Studio::new();
    let m = st.open_box();
    let b = st.ok();
    let ctx = b.sheet_metal.iter().find(|c| c.feature == m).unwrap();
    let c = joint(&b, m, "Bend C").id;
    let order = cadrs_sheetmetal::joint_edit::moved(&ctx.model, c, -1).unwrap();
    st.h.execute(&mut st.d, &SetTableOrder { element: st.el, model: m, order, label: "Move up Bend C".into() }).unwrap();
    let names = |b: &Build| -> Vec<String> {
        b.sheet_metal[0].model.joints.iter().filter(|j| j.bend().is_some()).map(|j| j.name.clone()).collect()
    };
    let moved = st.ok();
    assert_eq!(names(&moved), ["Bend A", "Bend C", "Bend B", "Bend D"]);
    // A Modify joint after it keeps the order.
    st.table(m, "Bend D", TableEdit::Radius(4.0, "4 mm".into()));
    assert_eq!(names(&st.ok()), ["Bend A", "Bend C", "Bend B", "Bend D"]);
    st.undo();
    st.undo();
    assert_eq!(names(&st.ok()), ["Bend A", "Bend B", "Bend C", "Bend D"]);
    let _ = JointId(0);
}
