//! Inspection and Repair Tools (stage 3D): the feature error states of P3D.1. A lost input fails
//! only the features that use it, and each failed feature says which of its inputs is gone
//! (the dialog shows it as "Missing Face of Sketch 1"), until the loss is undone.
#![cfg(feature = "occt")]

use cadrs_core::applied::{EdgeOrFace, FilletFeature};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{BooleanOp, Document, EdgeRef, ExtrudeFeature};
use cadrs_core::rebuild::{self, Build, FeatureStatus};
use cadrs_core::samples;
use cadrs_core::{ElementId, Feature, FeatureId, History, Part};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn new() -> Self {
        let d = Document::new("P3D.1");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn build(&self) -> std::sync::Arc<Build> {
        rebuild::build(&self.features())
    }

    /// A box x0..x1 × 0..20 on Top, 10 high (New): its sketch and its extrude.
    fn block(&mut self, x0: f64, x1: f64) -> (FeatureId, FeatureId) {
        let s = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: s, plane: Some(PlaneRef::Top) }).unwrap();
        let v = Vec2::new;
        let op = SketchOp::AddPolyline {
            points: vec![v(x0, 0.0), v(x1, 0.0), v(x1, 20.0), v(x0, 20.0)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        };
        self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: s, op }).unwrap();
        let g = self.d.element(self.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
        let regions = samples::region_refs(s, &g, &[Vec2::new((x0 + x1) / 2.0, 10.0)]);
        let e = ExtrudeFeature { op: BooleanOp::New, ..samples::extrude_of(regions, 10.0) };
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude: e, label: "Extrude".into() })
            .unwrap();
        (s, f)
    }
}

fn edge_at(part: &Part, p: [f64; 3]) -> EdgeRef {
    let e = part.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap();
    assert!(e.distance(p) < 1e-3, "no edge at {p:?}");
    EdgeRef { part: part.id, edge: e.name, seed: p }
}

/// P3D.1 (X3, IR5.1–IR5.2): deleting Sketch 1's entities fails Extrude 1 (its region is gone)
/// and the Fillet on Extrude 1's edge (its edge is gone), and leaves the independent Sketch 2 /
/// Extrude 2 alone. Each failed feature names the input it lost (position 0 of its list), and
/// undoing the delete clears every error.
#[test]
fn error_propagation_marks_only_dependants() {
    let mut d = Doc::new();
    let (s1, e1) = d.block(0.0, 20.0);
    let (_s2, e2) = d.block(50.0, 70.0);
    let b = d.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    // The top edge of the first box along X at y = 0.
    let part = b.parts.iter().find(|p| p.feature == e1).unwrap().clone();
    let edge = edge_at(&part, [10.0, 0.0, 10.0]);
    let fillet = FeatureId::new();
    d.h.execute(
        &mut d.d,
        &AddFeature::fillet(
            d.el,
            fillet,
            FilletFeature {
                entities: vec![EdgeOrFace::Edge(edge)],
                size: 2.0,
                size_expr: "2 mm".into(),
                ..FilletFeature::default()
            },
        ),
    )
    .unwrap();
    let b = d.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.status(fillet), FeatureStatus::Ok);

    // Sketch 1: select all, delete (IR6.9's move, on the first box's sketch).
    let g = d.d.element(d.el).unwrap().feature(s1).unwrap().sketch().unwrap().geometry.clone();
    let op = SketchOp::Delete {
        curves: g.curves.keys().collect(),
        points: g.points.keys().collect(),
        dimensions: vec![],
        constraints: vec![],
    };
    d.h.execute(&mut d.d, &EditSketch { element: d.el, feature: s1, op }).unwrap();
    let b = d.build();
    assert!(matches!(b.status(e1), FeatureStatus::Error(_)), "{:?}", b.status(e1));
    assert!(matches!(b.status(fillet), FeatureStatus::Error(_)), "{:?}", b.status(fillet));
    assert_eq!(b.status(e2), FeatureStatus::Ok, "the independent extrude is untouched");
    assert_eq!(b.errors.len(), 2, "{:?}", b.errors);
    assert_eq!(b.missing_inputs(e1), &[0], "Extrude 1's region is the missing input");
    assert_eq!(b.missing_inputs(fillet), &[0], "Fillet 1's edge is the missing input");
    assert!(b.missing_inputs(e2).is_empty());
    assert_eq!(b.parts.len(), 1, "only Extrude 2's part is left");

    // Undo: every error clears.
    d.h.undo(&mut d.d).unwrap();
    let b = d.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert!(b.missing.is_empty());
    assert_eq!(b.status(fillet), FeatureStatus::Ok);
}

/// P3D.2 (IR2.7): the Constraint manager's "Delete all" removes every row of the filtered list
/// (here the whole list but the shared ends) as one command: one undo step brings them all
/// back.
#[test]
fn delete_all_is_one_undo_step() {
    use cadrs_sketch::diagnostics::{Filter, deletion, items};
    use cadrs_sketch::{ConstraintOf, CurveRef, Dimension, DimensionKind};
    let mut d = Doc::new();
    let s = FeatureId::new();
    d.h.execute(&mut d.d, &AddSketch { element: d.el, feature: s, plane: Some(PlaneRef::Top) }).unwrap();
    let edit = |d: &mut Doc, op: SketchOp| d.h.execute(&mut d.d, &EditSketch { element: d.el, feature: s, op }).unwrap();
    edit(&mut d, SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 15.0, construction: false });
    edit(&mut d, SketchOp::AddCircle { center: Vec2::new(40.0, 0.0), radius: 10.0, construction: false });
    let geometry = |d: &Doc| d.d.element(d.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
    let ids: Vec<_> = geometry(&d).curves.keys().collect();
    edit(
        &mut d,
        SketchOp::SetDimension {
            dimension: Dimension::new(DimensionKind::Diameter { curve: ids[0] }, 30.0, 0.0),
            moves: vec![],
            radii: vec![],
        },
    );
    edit(
        &mut d,
        SketchOp::AddConstraint {
            constraints: vec![ConstraintOf::Equal(CurveRef::Curve(ids[0]), CurveRef::Curve(ids[1]))],
            label: "Add equal",
        },
    );
    let g = geometry(&d);
    assert_eq!((g.constraints.len(), g.dimensions.len()), (1, 1));
    let all = items(&g, &cadrs_sketch::solve::conflict_set(&g, &cadrs_sketch::solve::conflicts(&g)));
    let shown: Vec<_> = all.iter().filter(|i| Filter::default().matches(i)).collect();
    let (constraints, dimensions) = deletion(&shown);
    let steps = d.h.undo_len();
    edit(&mut d, SketchOp::Delete { curves: vec![], points: vec![], dimensions, constraints });
    assert_eq!(d.h.undo_len(), steps + 1, "one undo step");
    let g = geometry(&d);
    assert!(g.constraints.is_empty() && g.dimensions.is_empty());
    d.h.undo(&mut d.d).unwrap();
    let g = geometry(&d);
    assert_eq!((g.constraints.len(), g.dimensions.len()), (1, 1));
}

/// P3D.2 (IR6.1–IR6.8) on the inspection stand-in: Sketch 2 can't be solved (Equal 1 between
/// circles dimensioned Ø30 and Ø18) and has three loose ends (a spur's, and a 0.508 mm gap's
/// pair), so Extrude 2 has lost its region. Deleting Equal 1 leaves the sketch solvable;
/// Coincident on the gap's ends closes the tab and Extrude 2 builds again.
#[test]
fn inspection_stand_in_repairs_as_the_course_does() {
    use cadrs_core::samples::inspection::{self, EXTRUDE_2, SKETCH_2};
    use cadrs_sketch::diagnostics::{Filter, ItemId, Status, items, loose_ends};
    use cadrs_sketch::{ConstraintOf, PointRef};
    let mut d = Doc::new();
    let el = d.el;
    inspection::build_in(&mut cadrs_core::samples::gear_cover::DocHistory(&mut d.d, &mut d.h), el).unwrap();
    let geometry = |d: &Doc| d.d.element(d.el).unwrap().feature(SKETCH_2).unwrap().sketch().unwrap().geometry.clone();
    let b = d.build();
    assert!(matches!(b.status(EXTRUDE_2), FeatureStatus::Error(_)), "{:?}", b.status(EXTRUDE_2));
    assert_eq!(b.missing_inputs(EXTRUDE_2), &[0]);
    assert_eq!(b.errors.len(), 1, "only Extrude 2 fails: {:?}", b.errors);
    let g = geometry(&d);
    let groups = loose_ends(&g);
    let labels: Vec<String> = groups.iter().map(|g| g.label()).collect();
    assert_eq!(labels, ["Loose end", "Loose ends (2)"]);
    // The Errors filter: Equal 1 (and whatever else the solver left unsolved).
    let conflicting = cadrs_sketch::solve::conflicts(&g);
    let set = cadrs_sketch::solve::conflict_set(&g, &conflicting);
    let all = items(&g, &set);
    let errors = Filter { statuses: [Status::Error].into(), ..Filter::default() };
    let mut red: Vec<&str> = all.iter().filter(|i| errors.matches(i)).map(|i| i.name.as_str()).collect();
    red.sort();
    assert_eq!(red, ["Diameter 1", "Equal 1"], "the Errors filter lists the whole conflicting set");
    let equal = all.iter().find(|i| i.name == "Equal 1").expect("Equal 1 is listed");
    // On the ring's projected edges: external, from Extrude 1 (shown "Equal 1 [Extrude 1]").
    assert_eq!(equal.mode, cadrs_sketch::diagnostics::Mode::External);
    assert_eq!(equal.source, Some(inspection::EXTRUDE_1.0));
    let ItemId::Constraint(k) = equal.id else { panic!() };
    d.h.execute(&mut d.d, &EditSketch {
        element: el,
        feature: SKETCH_2,
        op: SketchOp::Delete { curves: vec![], points: vec![], dimensions: vec![], constraints: vec![k] },
    })
    .unwrap();
    assert!(cadrs_sketch::solve::conflicts(&geometry(&d)).is_empty(), "solvable without Equal 1");
    // Coincident on the gap's two ends (IR6.8).
    let g = geometry(&d);
    let pair = &loose_ends(&g)[1];
    let (a, b2) = (pair.ends[0].point, pair.ends[1].point);
    d.h.execute(&mut d.d, &EditSketch {
        element: el,
        feature: SKETCH_2,
        op: SketchOp::AddConstraint { constraints: vec![ConstraintOf::Coincident(PointRef::Point(a), PointRef::Point(b2))], label: "Add coincident" },
    })
    .unwrap();
    let g = geometry(&d);
    assert_eq!(loose_ends(&g).iter().map(|g| g.label()).collect::<Vec<_>>(), ["Loose end"]);
    let b = d.build();
    assert!(b.errors.is_empty(), "every error clears: {:?}", b.errors);
    assert_eq!(b.parts.len(), 2);
}

/// P3D.4 (IR6.1–IR6.17): the Conrod stand-in, broken as shipped, repaired as the course does,
/// against values worked out independently of the kernel (inch; 1 in = 25.4 mm):
///
/// - **The offset region (IR6.10).** The web is a trapezoid, full width 1.0 at y = 1 and 0.6
///   at y = 5: its half width is w(y) = 0.5 − 0.05 (y − 1). Offsetting its outline 0.1 inward
///   moves the short sides to y = 1.1 and y = 4.9 and each slanted side 0.1 along its normal,
///   which is 0.1·√(1 + 0.05²) = 0.10012492 horizontally. The half widths become
///   w(1.1) − 0.10012492 = 0.39487508 and w(4.9) − 0.10012492 = 0.20487508, so the area is
///   (2·0.39487508 + 2·0.20487508) / 2 × 3.8 = **2.279051 in²** (the readout's "2.279 in²").
/// - **The pocket (Extrude 5, IR6.14).** That region removed 0.07 deep: 2.279051 × 0.07 =
///   **0.159534 in³**.
/// - **The fillet (IR6.16).** R0.03 round the pocket's floor adds (1 − π/4)·r² per unit
///   length along the floor's perimeter P = 0.78975 + 0.40975 + 2·√(3.8² + 0.19²) =
///   8.808994 in: **1.7014e−3 in³**; the corner patches make up the rest (within 3 %).
/// - **Repaired = healthy.** The repaired rod's volume equals that of the same model built
///   directly (`healthy_document`) to 1e−9, and its mass is V × 0.2836 lb/in³ (Steel).
#[test]
fn conrod_stand_in_repairs_as_the_course_does() {
    use cadrs_core::history_log::Origin;
    use cadrs_core::repair::{Reference, ReplaceReference, outline, references};
    use cadrs_core::samples::conrod::{self, *};
    use cadrs_sketch::diagnostics::{ItemId, items, loose_ends};
    use cadrs_sketch::{ConstraintOf, PointRef};
    const IN3: f64 = 25.4 * 25.4 * 25.4;
    const IN2: f64 = 25.4 * 25.4;
    let (doc, mut log) = conrod::document_and_history().unwrap();
    let mut d = Doc { d: doc, h: History::default(), el: STUDIO };
    let position = |d: &Doc, id: FeatureId| d.features().iter().position(|f| f.id == id).unwrap();
    let volume_up_to = |d: &Doc, id: FeatureId| {
        let f = d.features();
        let i = position(d, id);
        let b = rebuild::build(&f[..=i]);
        b.part(PART).unwrap().mass.unwrap().volume / IN3
    };
    let volume = |d: &Doc| d.build().part(PART).unwrap().mass.unwrap().volume / IN3;
    let geometry = |d: &Doc, s: FeatureId| d.d.element(d.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();

    // IR6.1: shipped broken. Sketch 2 can't be solved and is open, so Extrude 4 (its region
    // gone) and Circular pattern 1 (its seed failed) are red; nothing else is.
    let b = d.build();
    let mut failed: Vec<FeatureId> = b.errors.iter().map(|(f, _)| *f).collect();
    failed.sort();
    assert_eq!(failed, [EXTRUDE_4, PATTERN_1], "{:?} {:?}", b.errors, b.warnings);
    assert_eq!(b.missing_inputs(EXTRUDE_4), &[0]);
    assert_eq!(b.parts.len(), 1, "one part, Conrod");
    let g = geometry(&d, SKETCH_2);
    assert!(!cadrs_sketch::solve::conflicts(&g).is_empty(), "Sketch 2 can't be solved");
    // Its history holds the healthy states: the last entry is the break.
    assert_eq!(log.entries.last().unwrap().label, "Conrod :: Edit : Sketch 2");
    assert_eq!(log.entries[0].label, "Start");
    assert_eq!(log.state_at(log.head_index()).unwrap(), d.d);
    let broke = log.head_index();
    assert!(log.last_healthy(STUDIO, EXTRUDE_4).is_some_and(|e| e < broke), "Extrude 4 was healthy before the break");
    assert_eq!(log.last_healthy(STUDIO, EXTRUDE_5), Some(broke), "Extrude 5 is still healthy");

    // IR6.3–IR6.5: the Constraint manager's Errors hold Equal 1; delete it.
    let set = cadrs_sketch::solve::conflict_set(&g, &cadrs_sketch::solve::conflicts(&g));
    let all = items(&g, &set);
    let equal = all.iter().find(|i| i.name == "Equal 1").expect("Equal 1 is listed");
    let ItemId::Constraint(k) = equal.id else { panic!() };
    d.h.execute(&mut d.d, &EditSketch {
        element: STUDIO,
        feature: SKETCH_2,
        op: SketchOp::Delete { curves: vec![], points: vec![], dimensions: vec![], constraints: vec![k] },
    })
    .unwrap();
    // IR6.6–IR6.8: "Loose end" (the spur) and "Loose ends (2)" (the gap); Coincident closes it.
    let g = geometry(&d, SKETCH_2);
    assert!(cadrs_sketch::solve::conflicts(&g).is_empty());
    let groups = loose_ends(&g);
    assert_eq!(groups.iter().map(|g| g.label()).collect::<Vec<_>>(), ["Loose end", "Loose ends (2)"]);
    let (a, b2) = (groups[1].ends[0].point, groups[1].ends[1].point);
    d.h.execute(&mut d.d, &EditSketch {
        element: STUDIO,
        feature: SKETCH_2,
        op: SketchOp::AddConstraint { constraints: vec![ConstraintOf::Coincident(PointRef::Point(a), PointRef::Point(b2))], label: "Add coincident" },
    })
    .unwrap();
    let b = d.build();
    assert!(b.errors.is_empty(), "every error clears: {:?}", b.errors);
    let fixed = log.record(&d.d, Origin::Command("Edit Sketch 2".into()), CREATED + 86_400, "me").unwrap();
    assert_eq!(log.entries[fixed].label, "Conrod :: Edit : Sketch 2");
    conrod::note_healthy(&mut log, &d.d);
    let v_healthy_slot = volume(&d);

    // IR6.9: Sketch 3's entities deleted; IR6.10: the web face's outer loop offset 0.1 in
    // inward. Its region is 2.279051 in².
    let g = geometry(&d, SKETCH_3);
    d.h.execute(&mut d.d, &EditSketch {
        element: STUDIO,
        feature: SKETCH_3,
        op: SketchOp::Delete {
            curves: g.curves.keys().collect(),
            points: g.points.keys().collect(),
            dimensions: g.dimensions.keys().collect(),
            constraints: g.constraints.keys().collect(),
        },
    })
    .unwrap();
    let op = {
        let mut dh = cadrs_core::samples::gear_cover::DocHistory(&mut d.d, &mut d.h);
        let op = conrod::web_offset(&dh, OFFSET * 25.4).unwrap();
        use cadrs_core::samples::gear_cover::Studio;
        dh.run(&EditSketch { element: STUDIO, feature: SKETCH_3, op: op.clone() }).unwrap();
        op
    };
    assert!(matches!(op, SketchOp::OffsetLoop { .. }));
    let g = geometry(&d, SKETCH_3);
    let inner = cadrs_sketch::region::regions(&g)
        .into_iter()
        .filter(|r| r.contains(Vec2::new(0.0, 3.0 * 25.4)))
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("the offset bounds a region");
    let area = inner.area() / IN2;
    assert!((area - 2.279_051).abs() < 1e-6, "offset region {area} in²");
    assert_eq!(format!("{area:.3}"), "2.279", "the Area readout");
    let edited = log.record(&d.d, Origin::Command("Edit Sketch 3".into()), CREATED + 86_460, "me").unwrap();
    assert_eq!(log.entries[edited].label, "Conrod :: Edit : Sketch 3");
    conrod::note_healthy(&mut log, &d.d);
    // Extrude 5 lost its region, the fillet its edge: both keep them as Missing (IR5.2).
    let b = d.build();
    let mut failed: Vec<FeatureId> = b.errors.iter().map(|(f, _)| *f).collect();
    failed.sort();
    assert_eq!(failed, [EXTRUDE_5, FILLET], "{:?}", b.errors);
    assert_eq!(b.missing_inputs(EXTRUDE_5), &[0]);
    assert_eq!(b.missing_inputs(FILLET), &[0]);
    // IR3.3: their last healthy regeneration is the Sketch 2 fix ("Edit healthy moment").
    assert_eq!(log.last_healthy(STUDIO, EXTRUDE_5), Some(fixed));
    assert_eq!(log.last_healthy(STUDIO, FILLET), Some(fixed));
    // IR3.6: in that state the missing region is the slot (its outline, round (0, 3) in), and
    // the missing edge's tangent chain is the slot's floor (4 edges).
    let then = log.state_at(fixed).unwrap();
    let then_features = then.element(STUDIO).unwrap().features().to_vec();
    let then_build = rebuild::build(&then_features);
    let ext5 = d.features()[position(&d, EXTRUDE_5)].clone();
    let missing_region = references(&ext5)[0].clone();
    let lines = outline(&then_features, &then_build.parts, &missing_region, false);
    assert_eq!(lines.len(), 1);
    let ys: Vec<f64> = lines[0].iter().map(|p| p[1] / 25.4).collect();
    let (lo, hi) = ys.iter().fold((f64::MAX, f64::MIN), |(l, h), y| (l.min(*y), h.max(*y)));
    assert!((lo - (SLOT_Y.0 - SLOT_R)).abs() < 1e-3 && (hi - (SLOT_Y.1 + SLOT_R)).abs() < 1e-3, "the slot: {lo}..{hi}");
    let fil = d.features()[position(&d, FILLET)].clone();
    let missing_edge = references(&fil)[0].clone();
    // (Where an edge was is on the parts before the fillet that took it.)
    let fi = then_features.iter().position(|f| f.id == FILLET).unwrap();
    let then_before = rebuild::build(&then_features[..fi]);
    assert_eq!(outline(&then_features, &then_before.parts, &missing_edge, true).len(), 4, "the slot floor's tangent chain");
    assert_eq!(cadrs_core::repair::entity_edges(&then_before.parts, &missing_edge), 4);

    // IR6.13–IR6.14: Replace reference: Extrude 5's missing region by the new one (Propagate
    // on). The pocket comes back: 2.279051 × 0.07 = 0.159534 in³ removed.
    let g = geometry(&d, SKETCH_3);
    let region = cadrs_core::RegionRef::new(SKETCH_3, &inner);
    let _ = g;
    d.h.execute(&mut d.d, &ReplaceReference { element: STUDIO, feature: EXTRUDE_5, index: 0, with: Reference::Region(region), propagate: true })
        .unwrap();
    let b = d.build();
    assert_eq!(b.errors.iter().map(|(f, _)| *f).collect::<Vec<_>>(), [FILLET], "{:?}", b.errors);
    let before_pocket = {
        let f = d.features();
        let i = position(&d, EXTRUDE_5);
        rebuild::build(&f[..i]).part(PART).unwrap().mass.unwrap().volume / IN3
    };
    let pocket = before_pocket - volume_up_to(&d, EXTRUDE_5);
    assert!((pocket - 0.159_534).abs() < 1e-6, "pocket {pocket} in³");
    // The rod before the pocket, in closed form (inch):
    // - the web: the trapezoid (1.0 + 0.6) / 2 × 4 = 3.2 in², 0.3 thick: 0.96;
    // - each end, 0.5 thick: its ring π(R² − r²) plus the part of its neck (x ±w, from the web's
    //   short side to y_n) outside the ring's outer circle, which is the neck's rectangle less
    //   the circle's cap over it: cap(R, w, h) = ∫₋w^w (√(R² − x²) − h) dx
    //   = w√(R² − w²) + R² asin(w / R) − 2wh, h the rectangle's near side's distance from the
    //   centre. Big end: R 0.75, r 0.45, w 0.5, rectangle y 0.5–1.0 (h = 0.5):
    //   0.5 − cap = 0.3100197; small end: R 0.5, r 0.3, w 0.3, y 5.0–5.45 about 5.8 (h = 0.35):
    //   0.27 − cap = 0.1991247;
    // - three notches through the small end (0.5): each the triangle (apex 0.38 from the centre,
    //   sides at ±15°) inside the outer circle R = 0.5: the sides meet it at t = −a cos15° +
    //   √(a² cos²15° − a² + R²) from the apex (a = 0.38), a triangle ½t² sin30° plus the circle's
    //   segment ½R²(θ − sin θ) over the chord (θ the chord's angle at the centre): 0.0038366.
    // V = 0.96 + (π·0.36 + 0.3100197)·0.5 + (π·0.16 + 0.1991247)·0.5 − 3·0.5·0.0038366
    //   = 2.025631 in³.
    let cap = |r: f64, w: f64, h: f64| w * (r * r - w * w).sqrt() + r * r * (w / r).asin() - 2.0 * w * h;
    let big = 0.5 - cap(0.75, 0.5, 0.5);
    let small = 0.6 * 0.45 - cap(0.5, 0.3, 0.35);
    let notch = {
        let (a, r, c) = (0.38f64, 0.5f64, 15f64.to_radians().cos());
        let t = -a * c + (a * a * c * c - a * a + r * r).sqrt();
        let (sx, sy) = (t * 15f64.to_radians().sin(), a + t * c);
        let theta = 2.0 * sx.atan2(sy);
        0.5 * t * t * 30f64.to_radians().sin() + 0.5 * r * r * (theta - theta.sin())
    };
    assert!((big - 0.310_019_7).abs() < 1e-7 && (small - 0.199_124_7).abs() < 1e-7 && (notch - 0.003_836_6).abs() < 1e-7, "{big} {small} {notch}");
    let closed = 0.96 + (std::f64::consts::PI * 0.36 + big) * 0.5 + (std::f64::consts::PI * 0.16 + small) * 0.5 - 3.0 * 0.5 * notch;
    assert!((closed - 2.025_631).abs() < 1e-6, "{closed}");
    assert!((before_pocket - closed).abs() < 1e-6, "before the pocket {before_pocket} in³ vs {closed}");

    // IR6.15–IR6.16: the fillet's missing edge replaced by the pocket's floor face. The new
    // floor's corners aren't tangent, so one of its edges would stand for 1 of the 4 edges the
    // missing chain had (IR4.7): the face takes all of them.
    let floor = conrod::floor_face(&d.features()).expect("the pocket's floor");
    let now = d.build();
    let one_edge = now
        .part(PART)
        .unwrap()
        .solid
        .face_edges(&floor.face)
        .first()
        .copied()
        .map(|e| Reference::Edge(cadrs_core::document::EdgeRef { part: PART, edge: e, seed: floor.seed }))
        .unwrap();
    let old = cadrs_core::repair::entity_edges(&then_before.parts, &missing_edge);
    assert!(cadrs_core::repair::chain_check(old, cadrs_core::repair::entity_edges(&now.parts, &one_edge), true).is_some());
    assert_eq!(cadrs_core::repair::chain_check(old, cadrs_core::repair::entity_edges(&now.parts, &Reference::Face(floor)), false), None);
    d.h.execute(&mut d.d, &ReplaceReference { element: STUDIO, feature: FILLET, index: 0, with: Reference::Face(floor), propagate: true })
        .unwrap();
    let b = d.build();
    assert!(b.errors.is_empty(), "no errors remain: {:?}", b.errors);
    let dv = volume(&d) - volume_up_to(&d, EXTRUDE_5);
    let expected = (1.0 - std::f64::consts::FRAC_PI_4) * FILLET_R * FILLET_R * 8.808_994;
    assert!((expected - 1.7014e-3).abs() < 1e-7);
    assert!((dv - expected).abs() / expected < 0.03, "fillet ΔV {dv:e} vs {expected:e}");

    // Repaired == built directly, and the mass (IR6.17) is V × 0.2836 lb/in³.
    let healthy = conrod::healthy_document().unwrap();
    let hb = rebuild::build(healthy.element(STUDIO).unwrap().features());
    assert!(hb.errors.is_empty(), "{:?}", hb.errors);
    let (v, vh) = (volume(&d), hb.part(PART).unwrap().mass.unwrap().volume / IN3);
    assert!(((v - vh) / vh).abs() < 1e-9, "repaired {v} vs healthy {vh}");
    assert!(v < v_healthy_slot, "the trapezoid pocket is larger than the slot was");
    let steel = d.d.element(STUDIO).unwrap().part_prop(PART).and_then(|p| p.material.clone()).expect("Steel");
    assert_eq!(steel.name, "Steel");
    let mass_lb = v * IN3 * steel.density_kg_mm3() / 0.453_592_37;
    assert!((mass_lb - v * STEEL_LB_IN3).abs() / mass_lb < 1e-4, "{mass_lb} lb");
    // The whole rod: the closed form less the pocket plus the fillet (its term within 3 % of
    // (1 − π/4)·r²·P), and the stand-in's values.
    let fillet_bound = 0.03 * expected;
    assert!((v - (closed - 2.279_051 * 0.07 + expected)).abs() < fillet_bound + 1e-6, "{v} in³");
    assert!((v - 1.867_79).abs() < 1e-5, "{v} in³");
    assert!((mass_lb - 0.529_70).abs() < 1e-5, "{mass_lb} lb");
    println!("Conrod: V {v:.6} in³, mass {mass_lb:.6} lb, pocket {pocket:.6} in³, fillet ΔV {dv:.4e} in³, offset area {area:.6} in²");
}

/// The Conrod stand-in's fixture files (broken as shipped, its history, and the healthy model)
/// are what the sample builds (regenerate with `CADRS_REGENERATE_FIXTURES=1`).
#[test]
fn conrod_fixture_is_current() {
    use cadrs_core::samples::conrod;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let (doc, log) = conrod::document_and_history().unwrap();
    let file = conrod::file(doc);
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
    let doc_path = root.join("conrod_standin.cadrs");
    let log_path = root.join("conrod_standin.history.ron");
    let healthy = conrod::file(conrod::healthy_document().unwrap());
    let healthy_text = ron::ser::to_string_pretty(&healthy, ron::ser::PrettyConfig::default()).unwrap();
    let healthy_path = root.join("conrod_standin_healthy.cadrs");
    if std::env::var_os("CADRS_REGENERATE_FIXTURES").is_some() {
        std::fs::write(&doc_path, &text).unwrap();
        log.save_path(&log_path).unwrap();
        std::fs::write(&healthy_path, &healthy_text).unwrap();
    }
    let on_disk = cadrs_core::Store::load_path(&healthy_path).expect("fixtures/conrod_standin_healthy.cadrs");
    assert_eq!(on_disk, healthy, "fixtures/conrod_standin_healthy.cadrs is stale");
    let on_disk = cadrs_core::Store::load_path(&doc_path).expect("fixtures/conrod_standin.cadrs");
    assert_eq!(on_disk, file, "fixtures/conrod_standin.cadrs is stale");
    let back = cadrs_core::history_log::HistoryLog::load_path(&log_path).expect("fixtures/conrod_standin.history.ron");
    assert_eq!(back.entries, log.entries, "fixtures/conrod_standin.history.ron is stale");
    assert_eq!(back.state_at(back.head_index()).unwrap(), file.document);
}
