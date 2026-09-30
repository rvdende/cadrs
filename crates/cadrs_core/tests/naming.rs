//! P3.2 persistent naming, through the document's commands and regeneration: after a sketch
//! dimension change, an extrude depth change and a reorder of two independent extrudes, every
//! sketch on a face, every Use and Pierce link and every stored edge name still resolves to the
//! geometrically matching entity. A version 3 document still loads. P3.3: the same when
//! Extrude 2 is an Add, so the names cross a boolean (one part).
#![cfg(feature = "occt")]

use cadrs_core::commands::{AddExtrude, AddSketch, EditSketch, MoveFeature, SetExtrude};
use cadrs_core::document::{BooleanOp, Document, ExtrudeFeature};
use cadrs_core::links::{LinkContext, curve_samples, project, projected_distance};
use cadrs_core::parts::{cap_name, face_plane, parts, sketch_face_lost_in};
use cadrs_core::samples::{self, EXTRUDE_1_SEEDS, EXTRUDE_2_SEEDS, EYE_X};
use cadrs_core::solid::Solid;
use cadrs_core::{ElementId, Feature, FeatureId, History};
use cadrs_kernel::naming::Match;
use cadrs_sketch::projection::LinkTarget;
use cadrs_sketch::{
    ConstraintOf, CurveKind, EdgeName, FaceName, FaceOrigin, Link, PlaneRef, PointRef, Sketch,
    SketchOp, Vec2, Vec3, VertexName,
};

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn feature(&self, f: FeatureId) -> Feature {
        self.d.element(self.el).unwrap().feature(f).unwrap().clone()
    }

    fn g(&self, f: FeatureId) -> Sketch {
        self.feature(f).sketch().unwrap().geometry.clone()
    }

    fn sketch(&mut self, plane: PlaneRef) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) })
            .unwrap();
        f
    }

    fn edit(&mut self, f: FeatureId, op: SketchOp) {
        self.h
            .execute(&mut self.d, &EditSketch { element: self.el, feature: f, op })
            .unwrap();
    }

    fn set_extrude(&mut self, ex: FeatureId, extrude: ExtrudeFeature) {
        self.h
            .execute(
                &mut self.d,
                &SetExtrude { element: self.el, feature: ex, extrude, label: "Edit".into() },
            )
            .unwrap();
    }

    fn extrude(&mut self, sketch: FeatureId, seeds: &[Vec2], depth: f64) -> FeatureId {
        let ex = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddExtrude { element: self.el, feature: ex, extrude: ExtrudeFeature::default() },
            )
            .unwrap();
        let refs = samples::region_refs(sketch, &self.g(sketch), seeds);
        assert_eq!(refs.len(), seeds.len());
        self.set_extrude(ex, samples::extrude_of(refs, depth));
        ex
    }

    /// The solid of the part an extrude made or added to.
    fn solid(&self, extrude: FeatureId) -> Solid {
        (*parts(&self.features()).into_iter().find(|p| p.features.contains(&extrude)).unwrap().solid).clone()
    }

    fn depth(&self, extrude: FeatureId) -> f64 {
        self.feature(extrude).extrude().unwrap().depth
    }
}

/// The end cap of an extrude's `index`-th region.
fn cap(doc: &Doc, op: FeatureId, index: usize) -> FaceName {
    cap_name(&doc.features(), op, index, true).unwrap()
}

/// The Control Arm and what refers to it.
struct Arm {
    doc: Doc,
    s1: FeatureId,
    e1: FeatureId,
    e2: FeatureId,
    /// On Extrude 1's hub top (region 0), using the right eye ring's top edges.
    s3: FeatureId,
    /// On Extrude 2's left web top (its region 0), using its edges.
    s4: FeatureId,
    /// On Front: the hub's silhouette, and a line end pierced by the hub's top edge.
    s5: FeatureId,
    /// Edge names a feature stored ("fillet these edges"): the right web's edges and the right
    /// eye hole's top edge, each with where it was.
    stored: Vec<(EdgeName, Place)>,
    /// Vertex names stored the same way: the right web's corners.
    vertices: Vec<(VertexName, Place)>,
}

/// Where an edge or vertex is, in terms that survive the edits the tests make (the sketch's
/// lines don't move; depths and the eye hole's size change): at the top, the bottom or running
/// between them; the side of the arm (±y) and x to the millimetre, or on the eye hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Bottom,
    Top,
    Between,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Hole(Level),
    At { level: Level, y_sign: i8, x: i64 },
}

fn level(points: &[Vec3], depth: f64) -> Level {
    if points.iter().all(|p| close(p[2], 0.0)) {
        Level::Bottom
    } else if points.iter().all(|p| close(p[2], depth)) {
        Level::Top
    } else {
        Level::Between
    }
}

fn place(points: &[Vec3], mid: Vec3, depth: f64, hole_r: f64) -> Place {
    let lv = level(points, depth);
    if points.iter().all(|p| close((p[0] - EYE_X).hypot(p[1]), hole_r)) {
        return Place::Hole(lv);
    }
    Place::At {
        level: lv,
        y_sign: if mid[1] > 1e-6 { 1 } else if mid[1] < -1e-6 { -1 } else { 0 },
        x: mid[0].round() as i64,
    }
}

/// The current radius of the right eye's hole.
fn hole_radius(doc: &Doc, s1: FeatureId) -> f64 {
    let g = doc.g(s1);
    g.curves
        .values()
        .find_map(|c| match c.kind {
            CurveKind::Circle { center, radius } if g.pos(center).distance(Vec2::new(EYE_X, 0.0)) < 1e-9 && radius < 17.0 => {
                Some(radius)
            }
            _ => None,
        })
        .unwrap()
}

fn control_arm() -> Arm {
    control_arm_with(BooleanOp::New)
}

/// The Control Arm with Extrude 2 as `op2` (New: two parts; Add: one, as in the course).
fn control_arm_with(op2: BooleanOp) -> Arm {
    let d = Document::new("Control Arm");
    let el = d.elements[0].id;
    let mut doc = Doc { d, h: History::default(), el };
    let s1 = doc.sketch(PlaneRef::Top);
    doc.edit(s1, samples::control_arm_geometry());
    let dim = samples::eye_hole_dimension(&doc.g(s1)).unwrap();
    doc.edit(s1, dim);
    let e1 = doc.extrude(s1, &EXTRUDE_1_SEEDS, samples::EXTRUDE_1_DEPTH);
    let e2 = doc.extrude(s1, &EXTRUDE_2_SEEDS, samples::EXTRUDE_2_DEPTH);
    if op2 != BooleanOp::New {
        let mut e = doc.feature(e2).extrude().unwrap().clone();
        e.op = op2;
        doc.set_extrude(e2, e);
    }
    let solid1 = doc.solid(e1);
    let solid2 = doc.solid(e2);

    // Sketch 3 on the hub's top, using the right eye ring's top edges. (The hub's, the web's and
    // the eye's tops are one face, merged as Onshape merges them: its edges round the eye.)
    let s3 = doc.sketch(face_plane(&doc.features(), e1, cap(&doc, e1, 0)).unwrap());
    let use_face = |doc: &mut Doc, sketch: FeatureId, ex: FeatureId, solid: &Solid, face: FaceName, keep: &dyn Fn(&[Vec3]) -> bool| {
        let f = doc.features();
        let frame = doc.feature(sketch).sketch().unwrap().plane.unwrap().frame();
        let ctx = LinkContext { solids: vec![(ex, solid)], features: &f };
        let items: Vec<_> = solid
            .face_edges(&face)
            .into_iter()
            .filter(|edge| keep(&solid.edge(edge).unwrap().points))
            .map(|edge| {
                let link = Link::Edge { feature: ex.0, edge };
                (ctx.shape(link, &frame).unwrap(), link)
            })
            .collect();
        assert!(!items.is_empty());
        doc.edit(sketch, SketchOp::Use { items });
    };
    let eye = cap(&doc, e1, 2);
    let round_eye = |pts: &[Vec3]| pts.iter().all(|p| (p[0] - EYE_X).hypot(p[1]) < samples::EYE_R + 1e-6);
    use_face(&mut doc, s3, e1, &solid1, eye, &round_eye);
    // Sketch 4 on the left web's top.
    let s4 = doc.sketch(face_plane(&doc.features(), e2, cap(&doc, e2, 0)).unwrap());
    let web = cap(&doc, e2, 0);
    use_face(&mut doc, s4, e2, &solid2, web, &|_| true);
    // Sketch 5 on Front: the hub cylinder's silhouette and a pierced line end.
    let s5 = doc.sketch(PlaneRef::Front);
    let hub = solid1
        .faces
        .iter()
        .find(|f| {
            matches!(f.name.origin, FaceOrigin::Side { .. })
                && f.plane.is_none()
                && f.loops.iter().flatten().all(|p| (p[0].hypot(p[1]) - samples::HUB_R).abs() < 1e-3)
        })
        .unwrap()
        .name;
    let f = doc.features();
    let ctx = LinkContext { solids: vec![(e1, &solid1)], features: &f };
    let front = PlaneRef::Front.frame();
    let sil = Link::Silhouette { feature: e1.0, face: hub, index: 0 };
    doc.edit(s5, SketchOp::Use { items: vec![(ctx.shape(sil, &front).unwrap(), sil)] });
    doc.edit(
        s5,
        SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, 60.0), Vec2::new(-30.0, 45.0)],
            closed: false,
            construction: false,
            label: "Add line",
        },
    );
    let hub_top = EdgeName::new(hub, cap(&doc, e1, 0), 0);
    assert!(solid1.edge(&hub_top).is_some());
    let p = doc.g(s5).point_at(Vec2::new(-30.0, 45.0), 1e-6).unwrap();
    doc.edit(
        s5,
        SketchOp::AddConstraint {
            constraints: vec![ConstraintOf::Pierce(PointRef::Point(p), Link::Edge { feature: e1.0, edge: hub_top })],
            label: "Add pierce",
        },
    );
    // Stored edge names: every edge of the right web's sides (region 1) and the eye hole's top
    // edge. (Not by the web's caps: the hub's, the web's and the eye's caps are merged into one
    // face at each end, which may take the web cap's name.)
    let web_key = doc.feature(e1).extrude().unwrap().regions[1].key();
    let web = |n: &EdgeName| n.faces.iter().any(|f| matches!(f.origin, FaceOrigin::Side { region, .. } if region == web_key));
    let mut names: Vec<EdgeName> = solid1.edges.iter().map(|e| e.name).filter(web).collect();
    names.sort();
    names.dedup();
    let hole_top = solid1
        .edges
        .iter()
        .find(|e| {
            e.points.iter().all(|p| {
                ((p[0] - EYE_X).hypot(p[1]) - samples::EYE_HOLE_R).abs() < 1e-6
                    && (p[2] - samples::EXTRUDE_1_DEPTH).abs() < 1e-9
            })
        })
        .unwrap()
        .name;
    names.push(hole_top);
    let d1 = samples::EXTRUDE_1_DEPTH;
    let stored: Vec<(EdgeName, Place)> = names
        .iter()
        .map(|n| {
            let e = solid1.edge(n).unwrap();
            (*n, place(&e.points, e.midpoint(), d1, samples::EYE_HOLE_R))
        })
        .collect();
    assert!(stored.len() >= 7, "{}", stored.len());
    // Tops and bottoms, both sides: a swap would show.
    for want in [Level::Top, Level::Bottom] {
        for side in [1, -1] {
            assert!(
                stored.iter().any(|(_, p)| matches!(p, Place::At { level, y_sign, .. } if *level == want && *y_sign == side)),
                "no stored edge at {want:?} {side}"
            );
        }
    }
    // The web's corners (where a web line meets a hub or eye side), top and bottom.
    let t = ((samples::HUB_R - samples::EYE_R) / EYE_X).acos();
    let corners: Vec<Vec2> = [1.0, -1.0]
        .iter()
        .flat_map(|s: &f64| {
            [
                Vec2::new(samples::HUB_R * t.cos(), s * samples::HUB_R * t.sin()),
                Vec2::new(EYE_X + samples::EYE_R * t.cos(), s * samples::EYE_R * t.sin()),
            ]
        })
        .collect();
    let vertices: Vec<(VertexName, Place)> = solid1
        .vertices
        .iter()
        .filter(|v| corners.iter().any(|c| c.distance(Vec2::new(v.point[0], v.point[1])) < 1e-6))
        .map(|v| (v.name, place(&[v.point], v.point, d1, samples::EYE_HOLE_R)))
        .collect();
    assert!(vertices.len() >= 4, "{}", vertices.len());
    Arm { doc, s1, e1, e2, s3, s4, s5, stored, vertices }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Every reference resolves by its exact name to the entity that geometrically matches.
fn check(arm: &Arm, what: &str) {
    let doc = &arm.doc;
    let features = doc.features();
    let all = parts(&features);
    let (d1, d2) = (doc.depth(arm.e1), doc.depth(arm.e2));
    let solid1 = doc.solid(arm.e1);
    let solid2 = doc.solid(arm.e2);
    let hole_r = hole_radius(doc, arm.s1);

    // Sketches on faces: on the face they name (exactly), which is where it should be.
    for (s, ex, solid, depth, region) in [(arm.s3, arm.e1, &solid1, d1, 0), (arm.s4, arm.e2, &solid2, d2, 0)] {
        let i = features.iter().position(|f| f.id == s).unwrap();
        assert!(!sketch_face_lost_in(&features, i, &all), "{what}: sketch on a face lost");
        let PlaneRef::Face(fp) = doc.feature(s).sketch().unwrap().plane.unwrap() else { panic!() };
        assert_eq!(fp.face, cap(doc, ex, region), "{what}");
        let (fi, m) = solid.resolve_face(&fp.face, Some(&fp.frame()), fp.seed).unwrap();
        assert_eq!(m, Match::Exact, "{what}");
        let face = &solid.faces[fi];
        assert_eq!(face.plane, Some(fp.frame()), "{what}: the sketch follows its face");
        assert!(close(fp.origin[2], depth) && close(fp.frame().normal()[2], 1.0), "{what}: {fp:?}");
        assert!(face.loops.iter().flatten().all(|p| close(p[2], depth)), "{what}");
    }

    // Use and Pierce links: unbroken, by their exact names, lying on the named edge.
    for s in [arm.s3, arm.s4, arm.s5] {
        let g = doc.g(s);
        assert!(g.broken.is_empty(), "{what}: broken links in {s:?}");
        let frame = doc.feature(s).sketch().unwrap().plane.unwrap().frame();
        for (_, target, link) in g.links() {
            match (link, target) {
                (Link::Edge { feature, edge }, target) => {
                    let solid = if feature == arm.e1.0 { &solid1 } else { &solid2 };
                    let e = solid.edge(&edge).unwrap_or_else(|| panic!("{what}: {edge:?} gone"));
                    let curve = cadrs_core::links::curve_of_polyline(&e.points).unwrap();
                    match target {
                        LinkTarget::Curve(c) => {
                            let shape = project(curve, &frame).unwrap();
                            for p in curve_samples(&g, c) {
                                let d = projected_distance(&shape, p).unwrap();
                                assert!(d < 1e-6, "{what}: used curve off its edge by {d}");
                            }
                        }
                        LinkTarget::Point(p) => {
                            // The hub's top circle crosses Front at (−35, depth).
                            let at = g.pos(p);
                            assert!(close(at.x, -samples::HUB_R) && close(at.y, d1), "{what}: pierced at {at:?}");
                        }
                    }
                }
                (Link::Silhouette { face, .. }, LinkTarget::Curve(c)) => {
                    assert!(solid1.face(&face).is_some(), "{what}: silhouette face gone");
                    // Seen from Front, the hub's left silhouette is the line x = −35, 0 ≤ z ≤ d1;
                    // with the left web added to the hub (one part), only the step above the
                    // web is left of the hub's side there: d2 ≤ z ≤ d1.
                    let pts = curve_samples(&g, c);
                    // (From the mesh's rulings, 5° apart: within 0.05 mm.)
                    assert!(pts.iter().all(|p| (p.x + samples::HUB_R).abs() < 0.05), "{what}: {pts:?}");
                    let merged = solid1.faces.iter().any(|f| f.name.op == arm.e2.0);
                    let bottom = if merged { d2 } else { 0.0 };
                    let (lo, hi) = pts.iter().fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p.y), h.max(p.y)));
                    assert!(close(lo, bottom) && close(hi, d1), "{what}: {lo}..{hi}");
                }
                other => panic!("{what}: unexpected link {other:?}"),
            }
        }
    }
    // The eye ring's top edges in Sketch 3 include the hole, at its current size.
    let g3 = doc.g(arm.s3);
    assert!(
        g3.curves.values().any(|c| matches!(c.kind, CurveKind::Circle { radius, .. } if close(radius, hole_r))),
        "{what}: the used hole circle is not Ø{}",
        2.0 * hole_r
    );

    // Stored edge names: exact, on the right web's lines or the eye hole's top circle.
    let t = ((samples::HUB_R - samples::EYE_R) / EYE_X).acos();
    let on_web_line = |p: Vec3| {
        [1.0, -1.0].iter().any(|s: &f64| {
            let a = Vec2::new(samples::HUB_R * t.cos(), s * samples::HUB_R * t.sin());
            let b = Vec2::new(EYE_X + samples::EYE_R * t.cos(), s * samples::EYE_R * t.sin());
            cadrs_sketch::geom::dist_point_segment(Vec2::new(p[0], p[1]), a, b) < 1e-6
        })
    };
    for (name, was) in &arm.stored {
        let (i, m) = solid1.resolve_edge(name, |_| None).unwrap_or_else(|_| panic!("{what}: {name:?} lost"));
        assert_eq!(m, Match::Exact, "{what}");
        let e = &solid1.edges[i];
        let hole = e.points.iter().all(|p| close((p[0] - EYE_X).hypot(p[1]), hole_r) && close(p[2], d1));
        let web = e.points.iter().all(|p| on_web_line(*p) && (p[2] > -1e-9 && p[2] < d1 + 1e-9));
        assert!(hole || web, "{what}: {name:?} is at {:?}", e.points);
        // The same edge as before: same side, same end, same x.
        let now = place(&e.points, e.midpoint(), d1, hole_r);
        assert_eq!(now, *was, "{what}: {name:?} moved from {was:?} to {now:?}");
    }
    // Stored vertex names: exact, at their counterpart.
    for (name, was) in &arm.vertices {
        let v = solid1.vertex(name).unwrap_or_else(|| panic!("{what}: vertex {name:?} lost"));
        let now = place(&[v.point], v.point, d1, hole_r);
        assert_eq!(now, *was, "{what}: vertex {name:?} moved from {was:?} to {now:?}");
    }
    // Silhouettes lie on their faces (none across the gap between two pieces of a face, as a
    // stray line inside the eye hole once did): every one starts at a ruling's run of its face.
    for solid in [&solid1, &solid2] {
        for (fi, face) in solid.faces.iter().enumerate() {
            if face.plane.is_some() {
                continue;
            }
            for (a, _) in cadrs_core::links::silhouettes(solid, &face.name, [1.0, -1.0, 1.0]) {
                let on = solid
                    .rulings
                    .iter()
                    .filter(|r| r.face == fi)
                    .any(|r| ((a[0] - r.start[0]).hypot(a[1] - r.start[1])) < 1.0);
                assert!(on, "{what}: silhouette at {a:?} is off its face");
            }
        }
    }
}

#[test]
fn references_survive_a_sketch_dimension_change() {
    let mut arm = control_arm();
    check(&arm, "as built");
    // The right eye's hole: Ø20 → Ø24.
    let g = arm.doc.g(arm.s1);
    let (id, _) = g.dimensions.iter().next().unwrap();
    arm.doc.edit(arm.s1, SketchOp::SetDimensionValue { id, value: 24.0 });
    check(&arm, "after Ø24");
    // Undo puts it back.
    arm.doc.h.undo(&mut arm.doc.d).expect("undo");
    check(&arm, "after undo");
}

#[test]
fn references_survive_an_extrude_depth_change() {
    let mut arm = control_arm();
    let mut e = arm.doc.feature(arm.e1).extrude().unwrap().clone();
    e.depth = 55.0;
    e.depth_expr = "55 mm".into();
    arm.doc.set_extrude(arm.e1, e);
    check(&arm, "after depth 55");
    let mut e = arm.doc.feature(arm.e2).extrude().unwrap().clone();
    e.depth = 10.0;
    arm.doc.set_extrude(arm.e2, e);
    check(&arm, "after depth 10");
    // Undo both: back to 40 and 25.
    arm.doc.h.undo(&mut arm.doc.d).expect("undo");
    arm.doc.h.undo(&mut arm.doc.d).expect("undo");
    assert_eq!((arm.doc.depth(arm.e1), arm.doc.depth(arm.e2)), (40.0, 25.0));
    check(&arm, "after undoing both depths");
}

#[test]
fn references_survive_reordering_two_independent_extrudes() {
    let mut arm = control_arm();
    let before = arm.doc.features().iter().position(|f| f.id == arm.e1).unwrap();
    arm.doc
        .h
        .execute(
            &mut arm.doc.d,
            &MoveFeature { element: arm.doc.el, feature: arm.e2, to: before, label: "Reorder".into() },
        )
        .unwrap();
    let order: Vec<FeatureId> = arm.doc.features().iter().map(|f| f.id).collect();
    assert!(order.iter().position(|f| *f == arm.e2) < order.iter().position(|f| *f == arm.e1));
    // Extrude 2 now makes Part 1.
    let ps = parts(&arm.doc.features());
    assert_eq!(ps[0].feature, arm.e2);
    check(&arm, "after reorder");
}

/// P3.3: Extrude 2 adds to Extrude 1 (one part, as in the course), so the faces and edges the
/// references name come through a boolean. Every reference still resolves exactly after a
/// sketch dimension change, depth changes and a reorder of the two extrudes (Extrude 2 then has
/// nothing to add to and makes its own part).
#[test]
fn references_survive_edits_when_extrude_2_adds() {
    let mut arm = control_arm_with(BooleanOp::Add);
    let ps = parts(&arm.doc.features());
    assert_eq!(ps.len(), 1, "Extrude 2 joins Part 1");
    assert_eq!(ps[0].features, vec![arm.e1, arm.e2]);
    check(&arm, "added: as built");
    // Ø20 → Ø24 and back.
    let g = arm.doc.g(arm.s1);
    let (id, _) = g.dimensions.iter().next().unwrap();
    arm.doc.edit(arm.s1, SketchOp::SetDimensionValue { id, value: 24.0 });
    check(&arm, "added: after Ø24");
    arm.doc.h.undo(&mut arm.doc.d).expect("undo");
    check(&arm, "added: after undo");
    // Depths 55 and 10.
    let mut e = arm.doc.feature(arm.e1).extrude().unwrap().clone();
    e.depth = 55.0;
    arm.doc.set_extrude(arm.e1, e);
    let mut e = arm.doc.feature(arm.e2).extrude().unwrap().clone();
    e.depth = 10.0;
    arm.doc.set_extrude(arm.e2, e);
    check(&arm, "added: after depths 55 and 10");
    arm.doc.h.undo(&mut arm.doc.d).expect("undo");
    arm.doc.h.undo(&mut arm.doc.d).expect("undo");
    // Reorder: Extrude 2 first.
    let before = arm.doc.features().iter().position(|f| f.id == arm.e1).unwrap();
    arm.doc
        .h
        .execute(
            &mut arm.doc.d,
            &MoveFeature { element: arm.doc.el, feature: arm.e2, to: before, label: "Reorder".into() },
        )
        .unwrap();
    let ps = parts(&arm.doc.features());
    assert_eq!(ps.len(), 2);
    assert_eq!(ps[0].feature, arm.e2);
    check(&arm, "added: after reorder");
}

/// A face or edge that is gone is a lost reference, not a guess; one redrawn in the same place
/// is found again by the geometric fallback, and the link is repaired to its new name.
#[test]
fn lost_references_and_the_geometric_fallback() {
    let mut arm = control_arm();
    // Redraw the right eye's hole: a new sketch curve, so a new name for its side face.
    let g = arm.doc.g(arm.s1);
    let hole = g
        .curves
        .iter()
        .find_map(|(id, c)| match c.kind {
            CurveKind::Circle { center, radius } if g.pos(center).distance(Vec2::new(EYE_X, 0.0)) < 1e-9 && radius < 17.0 => Some(id),
            _ => None,
        })
        .unwrap();
    arm.doc.edit(arm.s1, SketchOp::Delete { curves: vec![hole], points: vec![], dimensions: vec![], constraints: vec![] });
    arm.doc.edit(
        arm.s1,
        SketchOp::AddCircle { center: Vec2::new(EYE_X, 0.0), radius: samples::EYE_HOLE_R, construction: false },
    );
    let g3 = arm.doc.g(arm.s3);
    assert!(g3.broken.is_empty(), "the redrawn hole's edge is found by where it is");
    let renamed = g3.links().iter().any(|(_, _, l)| match l {
        Link::Edge { edge, .. } => !arm.stored.iter().any(|(n, _)| n == edge) && edge.faces.iter().any(|f| matches!(f.origin, FaceOrigin::Side { .. })),
        _ => false,
    });
    assert!(renamed, "the link names the new side face");

    // Extrude 1 without the eye ring: its edges are gone, so Sketch 3's links are lost.
    let mut e = arm.doc.feature(arm.e1).extrude().unwrap().clone();
    let all_regions = e.regions.clone();
    e.regions.truncate(2);
    arm.doc.set_extrude(arm.e1, e.clone());
    let g3 = arm.doc.g(arm.s3);
    assert_eq!(g3.broken.len(), g3.links().len());
    // The hub's top is still there.
    let fs = arm.doc.features();
    let i = fs.iter().position(|f| f.id == arm.s3).unwrap();
    assert!(!sketch_face_lost_in(&fs, i, &parts(&fs)));
    // Without the hub ring and the web (the eye ring alone), the face Sketch 3 is on is lost:
    // the sketch reports it and stays. (With the web still there, the hub's and web's tops are
    // one face, which the sketch stays on.)
    let frame = arm.doc.feature(arm.s3).sketch().unwrap().plane.unwrap().frame();
    e.regions = vec![all_regions[2].clone()];
    arm.doc.set_extrude(arm.e1, e);
    let fs = arm.doc.features();
    assert!(sketch_face_lost_in(&fs, i, &parts(&fs)));
    assert_eq!(arm.doc.feature(arm.s3).sketch().unwrap().plane.unwrap().frame(), frame);
    let _ = arm.s4;
}

/// A version 3 document (sketches on faces and links stored as FaceTag/EdgeTag, written by the
/// P3.1 code: `tests/fixtures/v3_control_arm.ron`) loads, and its references resolve to the
/// same geometry: regenerating it changes nothing.
#[test]
fn a_v3_document_loads_and_resolves() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/v3_control_arm.ron");
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("version: 3") && text.contains("face: End("), "not a v3 file");
    let file = cadrs_core::Store::load_path(std::path::Path::new(path)).unwrap();
    assert_eq!(file.version, cadrs_core::store::SCHEMA_VERSION);
    let features = file.document.elements[0].features().to_vec();
    let e1 = FeatureId(uuid::Uuid::from_u128(2));
    // The face plane's name: Extrude 1's end cap of region 0.
    let sk2 = features.iter().find(|f| f.id.0 == uuid::Uuid::from_u128(4)).unwrap();
    let PlaneRef::Face(fp) = sk2.sketch().unwrap().plane.unwrap() else { panic!() };
    assert_eq!(fp.face, cap_name(&features, e1, 0, true).unwrap());
    // Every link names Extrude 1's faces.
    let mut links = 0;
    for f in &features {
        let Some(sk) = f.sketch() else { continue };
        for (_, _, l) in sk.geometry.links() {
            links += 1;
            match l {
                Link::Edge { edge, .. } => assert!(edge.faces.iter().all(|f| f.op == e1.0)),
                Link::Silhouette { face, .. } => assert_eq!(face.op, e1.0),
                Link::SketchCurve { .. } | Link::Plane(_) => {}
            }
        }
    }
    assert_eq!(links, 4);
    // Regenerating moves nothing and breaks nothing.
    let mut regenerated = features.clone();
    cadrs_core::parts::refresh_face_planes(&mut regenerated);
    for (a, b) in features.iter().zip(&regenerated) {
        let (Some(sa), Some(sb)) = (a.sketch(), b.sketch()) else { continue };
        assert!(sb.geometry.broken.is_empty(), "{}: broken links", b.name);
        // The same plane and face (regenerating adds the face's seed point).
        assert_eq!(sa.plane.map(|p| p.frame()), sb.plane.map(|p| p.frame()), "{}", b.name);
        assert_eq!(sa.plane.and_then(|p| p.face()).map(|f| f.face), sb.plane.and_then(|p| p.face()).map(|f| f.face));
        // Sketch 3's first line is the hub's silhouette seen from Front, from the mesh's rulings:
        // the hub's side is one face now (merged from the pieces of its circle), meshed a little
        // differently, and the line is within 0.01 mm of where it was (x = −35 exactly now).
        let tol = if b.name == "Sketch 3" { 0.01 } else { 1e-6 };
        for (k, p) in &sa.geometry.points {
            let q = sb.geometry.points[k].pos;
            assert!(p.pos.distance(q) < tol, "{}: a point moved from {:?} to {q:?}", b.name, p.pos);
        }
    }
    // Saved again, it is a current (version 5, P3G.1) file that loads back the same.
    let text = ron::ser::to_string(&file).unwrap();
    assert!(text.contains("version:5") || text.contains("version: 5"));
}
