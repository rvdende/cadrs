//! T5: Use/project (S20), Pierce (S12.11), imprinting (S21) and sketch text (S16) through the
//! document's commands and regeneration.

use cadrs_core::commands::{
    AddExtrude, AddSketch, DeleteFeature, EditSketch, SetExtrude, SetSketchImprinting,
};
use cadrs_core::document::{Document, ExtrudeFeature, RegionRef};
use cadrs_core::parts::{cap_name, face_plane, parts};
use cadrs_core::{ElementId, FeatureId, History};
use cadrs_sketch::projection::Projected;
use cadrs_sketch::region::regions;
use cadrs_sketch::{
    ConstraintOf, CurveKind, CurveRef, DimensionKind, EdgeName, FaceOrigin, Link, PlaneRef,
    PointRef, Sketch, SketchOp, Vec2, solve,
};

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn new() -> Self {
        let d = Document::new("T5");
        let el = d.elements[0].id;
        Self {
            d,
            h: History::default(),
            el,
        }
    }

    fn sketch(&mut self, plane: PlaneRef) -> FeatureId {
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
        f
    }

    fn edit(&mut self, f: FeatureId, op: SketchOp) {
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

    fn g(&self, f: FeatureId) -> &Sketch {
        &self
            .d
            .element(self.el)
            .unwrap()
            .feature(f)
            .unwrap()
            .sketch()
            .unwrap()
            .geometry
    }

    fn features(&self) -> Vec<cadrs_core::Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    /// Extrudes every region of a sketch (or the ones `pick` chooses) by `depth`.
    fn extrude(
        &mut self,
        sketch: FeatureId,
        depth: f64,
        pick: impl Fn(&cadrs_sketch::Region) -> bool,
    ) -> FeatureId {
        let ex = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddExtrude {
                    element: self.el,
                    feature: ex,
                    extrude: ExtrudeFeature::default(),
                },
            )
            .unwrap();
        let rs: Vec<RegionRef> = regions(self.g(sketch))
            .iter()
            .filter(|r| pick(r))
            .map(|r| RegionRef::new(sketch, r))
            .collect();
        assert!(!rs.is_empty());
        self.h
            .execute(
                &mut self.d,
                &SetExtrude {
                    element: self.el,
                    feature: ex,
                    extrude: ExtrudeFeature {
                        regions: rs,
                        depth,
                        ..ExtrudeFeature::default()
                    },
                    label: "Select".into(),
                },
            )
            .unwrap();
        ex
    }
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> SketchOp {
    let c = [
        Vec2::new(x, y),
        Vec2::new(x + w, y),
        Vec2::new(x + w, y + h),
        Vec2::new(x, y + h),
    ];
    SketchOp::Batch(vec![
        SketchOp::AddPolyline {
            points: c.to_vec(),
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
        SketchOp::AddConstraints(cadrs_sketch::constraint::rectangle_constraints(c)),
    ])
}

fn circle(x: f64, y: f64, r: f64) -> SketchOp {
    SketchOp::AddCircle {
        center: Vec2::new(x, y),
        radius: r,
        construction: false,
    }
}

/// An edge on a start cap (the sketch plane).
fn starts(e: &EdgeName) -> bool {
    e.faces
        .iter()
        .any(|f| matches!(f.origin, FaceOrigin::Cap { end: false, .. }))
}

/// An edge on an end cap.
fn ends(e: &EdgeName) -> bool {
    e.faces
        .iter()
        .any(|f| matches!(f.origin, FaceOrigin::Cap { end: true, .. }))
}

fn uses(s: &Sketch) -> Vec<(cadrs_sketch::ConstraintId, cadrs_sketch::CurveId, Link)> {
    s.constraints
        .iter()
        .filter_map(|(k, c)| match *c {
            ConstraintOf::Use(CurveRef::Curve(x), l) => Some((k, x, l)),
            _ => None,
        })
        .collect()
}

/// A 50 × 30 box on Top, 25 high, and a sketch on its top face.
fn box_with_face_sketch() -> (Doc, FeatureId, FeatureId, FeatureId) {
    let mut d = Doc::new();
    let s1 = d.sketch(PlaneRef::Top);
    d.edit(s1, rect(0.0, 0.0, 50.0, 30.0));
    let ex = d.extrude(s1, 25.0, |_| true);
    let top = face_plane(
        &d.features(),
        ex,
        cap_name(&d.features(), ex, 0, true).unwrap(),
    )
    .unwrap();
    let s2 = d.sketch(top);
    (d, s1, ex, s2)
}

#[test]
fn use_projects_part_edges_and_follows_them() {
    let (mut d, s1, ex, s2) = box_with_face_sketch();
    // Use the top face: its four edges.
    let features = d.features();
    let solid = parts(&features).remove(0).solid;
    let frame =
        d.d.element(d.el)
            .unwrap()
            .feature(s2)
            .unwrap()
            .sketch()
            .unwrap()
            .plane
            .unwrap()
            .frame();
    let refs = vec![(ex, &*solid)];
    let ctx = cadrs_core::links::LinkContext {
        solids: refs,
        features: &features,
    };
    let items: Vec<(Projected, Link)> =
        cadrs_core::links::face_edges(&solid, &cap_name(&features, ex, 0, true).unwrap())
            .into_iter()
            .map(|edge| {
                let link = Link::Edge {
                    feature: ex.0,
                    edge,
                };
                (ctx.shape(link, &frame).unwrap(), link)
            })
            .collect();
    assert_eq!(items.len(), 4);
    d.edit(s2, SketchOp::Use { items });
    let g = d.g(s2);
    assert_eq!(uses(g).len(), 4);
    // Projected entities are fixed: fully defined.
    assert!(solve::analyze(g).fully_constrained());
    let len: f64 = g.curves.keys().filter_map(|c| g.line_length(c)).sum();
    assert!((len - 160.0).abs() < 1e-6, "{len}");
    // They make a region the size of the face (no double edges with the imprint).
    let r = regions(g);
    assert_eq!(r.len(), 1);
    assert!((r[0].area() - 1500.0).abs() < 1e-6);
    // The source changes (the box gets longer): the projection follows.
    let right = d
        .g(s1)
        .curves
        .iter()
        .find(|(_, c)| matches!(c.kind, CurveKind::Line { .. }))
        .map(|(k, _)| k)
        .unwrap();
    let _ = right;
    let (a, b) = {
        let g1 = d.g(s1);
        let p = |x: f64, y: f64| g1.point_at(Vec2::new(x, y), 1e-6).unwrap();
        (p(0.0, 0.0), p(50.0, 0.0))
    };
    d.edit(
        s1,
        SketchOp::SetDimension {
            dimension: cadrs_sketch::Dimension::new(DimensionKind::Horizontal { a, b }, 70.0, -5.0),
            moves: vec![],
            radii: vec![],
        },
    );
    let g = d.g(s2);
    let len: f64 = g.curves.keys().filter_map(|c| g.line_length(c)).sum();
    assert!((len - 2.0 * (70.0 + 30.0)).abs() < 1e-6, "{len}");
    assert!(g.broken.is_empty());
    // Deleting the link constraint leaves free geometry.
    let (k, c, _) = uses(d.g(s2))[0];
    d.edit(
        s2,
        SketchOp::Delete {
            curves: vec![],
            points: vec![],
            dimensions: vec![],
            constraints: vec![k],
        },
    );
    let g = d.g(s2);
    assert!(!g.is_projected(c) && g.curves.contains_key(c));
    assert!(!solve::analyze(g).fully_constrained());
    // Deleting the source: the remaining links break (the sketch is in error).
    d.h.execute(
        &mut d.d,
        &DeleteFeature {
            element: d.el,
            feature: ex,
            label: "Delete Extrude 1".into(),
        },
    )
    .unwrap();
    let g = d.g(s2);
    assert_eq!(g.broken.len(), 3);
    assert!(g.curve_is_orphaned(uses(g)[0].1));
    // Undo brings the source back and the error goes.
    d.h.undo(&mut d.d);
    assert!(d.g(s2).broken.is_empty());
}

#[test]
fn use_projects_other_sketches_arcs_circles_and_lines() {
    let mut d = Doc::new();
    // A circle and an arc on Top; a sketch on a parallel face-less plane: use the Front plane for
    // an edge-on projection and Top itself for a parallel one.
    let s1 = d.sketch(PlaneRef::Top);
    d.edit(s1, circle(10.0, 5.0, 4.0));
    d.edit(
        s1,
        SketchOp::AddArc {
            center: Vec2::new(-10.0, 0.0),
            start: Vec2::new(-5.0, 0.0),
            end: Vec2::new(-15.0, 0.0),
            construction: false,
        },
    );
    d.edit(
        s1,
        SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, -20.0), Vec2::new(30.0, -10.0)],
            closed: false,
            construction: false,
            label: "Add line",
        },
    );
    let s2 = d.sketch(PlaneRef::Top);
    let s3 = d.sketch(PlaneRef::Front);
    let features = d.features();
    let ctx = cadrs_core::links::LinkContext {
        solids: vec![],
        features: &features[..1],
    };
    let g1 = d.g(s1).clone();
    let top = PlaneRef::Top.frame();
    let front = PlaneRef::Front.frame();
    let mut items2 = Vec::new();
    let mut items3 = Vec::new();
    for (c, _) in &g1.curves {
        let link = Link::SketchCurve {
            feature: s1.0,
            curve: c,
        };
        items2.push((ctx.shape(link, &top).unwrap(), link));
        if let Some(s) = ctx.shape(link, &front) {
            items3.push((s, link));
        }
    }
    d.edit(s2, SketchOp::Use { items: items2 });
    let g2 = d.g(s2);
    let kinds: Vec<&str> = g2
        .curves
        .values()
        .map(|c| match c.kind {
            CurveKind::Line { .. } => "line",
            CurveKind::Circle { .. } => "circle",
            CurveKind::Arc { .. } => "arc",
            CurveKind::Ellipse { .. } => "ellipse",
            CurveKind::EllipseOffset { .. } => "offset ellipse",
            CurveKind::EllipseArc { .. } => "elliptical arc",
            CurveKind::Spline { .. } => "spline",
            CurveKind::Bezier { .. } => "bezier",
        })
        .collect();
    assert_eq!(kinds.len(), 3);
    assert!(kinds.contains(&"circle") && kinds.contains(&"arc") && kinds.contains(&"line"));
    let arc = g2
        .curves
        .keys()
        .find(|k| g2.arc_geom(*k).is_some())
        .unwrap();
    let a = g2.arc_geom(arc).unwrap();
    assert!((a.radius - 5.0).abs() < 1e-9 && a.center.distance(Vec2::new(-10.0, 0.0)) < 1e-9);
    // Seen edge-on from the Front plane every curve is a segment.
    assert_eq!(items3.len(), 3);
    assert!(items3.iter().all(|(s, _)| matches!(s, Projected::Line(..))));
    d.edit(s3, SketchOp::Use { items: items3 });
    // The circle's projection spans its diameter along X.
    let g3 = d.g(s3);
    assert!(
        g3.curves
            .keys()
            .any(|k| g3.line_length(k).is_some_and(|l| (l - 8.0).abs() < 1e-9))
    );
    // Moving the circle in sketch 1 moves both projections.
    let circle_id = g1
        .curves
        .iter()
        .find(|(_, c)| matches!(c.kind, CurveKind::Circle { .. }))
        .unwrap()
        .0;
    let center = g1.curve_points(circle_id)[0];
    d.edit(
        s1,
        SketchOp::MovePoints {
            moves: vec![(center, Vec2::new(20.0, 5.0))],
        },
    );
    let g2 = d.g(s2);
    let c2 = g2.curves.values().find_map(|c| match c.kind {
        CurveKind::Circle { center, .. } => Some(g2.pos(center)),
        _ => None,
    });
    assert!(c2.unwrap().distance(Vec2::new(20.0, 5.0)) < 1e-9);
    // Deleting sketch 1: both sketches are in error.
    d.h.execute(
        &mut d.d,
        &DeleteFeature {
            element: d.el,
            feature: s1,
            label: "Delete".into(),
        },
    )
    .unwrap();
    assert_eq!(d.g(s2).broken.len(), 3);
    assert_eq!(d.g(s3).broken.len(), 3);
}

#[test]
fn a_cylinder_projects_its_circle_and_silhouettes() {
    let mut d = Doc::new();
    // A cylinder along -Y from the Front plane: seen from Top its silhouettes are two lines.
    let s1 = d.sketch(PlaneRef::Front);
    d.edit(s1, circle(0.0, 10.0, 5.0));
    let ex = d.extrude(s1, 20.0, |_| true);
    let features = d.features();
    let solid = parts(&features).remove(0).solid;
    let side = solid.faces.iter().find(|f| f.plane.is_none()).unwrap().name;
    let top = PlaneRef::Top.frame();
    let sil = cadrs_core::links::silhouettes(&solid, &side, top.normal());
    assert_eq!(sil.len(), 2);
    let ctx = cadrs_core::links::LinkContext {
        solids: vec![(ex, &solid)],
        features: &features,
    };
    let lines: Vec<Projected> = (0..2)
        .map(|i| {
            ctx.shape(
                Link::Silhouette {
                    feature: ex.0,
                    face: side,
                    index: i,
                },
                &top,
            )
            .unwrap()
        })
        .collect();
    for l in &lines {
        let Projected::Line(a, b) = *l else { panic!() };
        assert!((a.distance(b) - 20.0).abs() < 1e-6);
        assert!((a.x.abs() - 5.0).abs() < 0.05, "{a:?}");
    }
    // The cap edge circle is a circle from Front, a segment from Top.
    let cap = solid.edges.iter().find(|e| starts(&e.name)).unwrap().name;
    let link = Link::Edge {
        feature: ex.0,
        edge: cap,
    };
    let Some(Projected::Circle(c, r)) = ctx.shape(link, &PlaneRef::Front.frame()) else {
        panic!()
    };
    assert!(c.distance(Vec2::new(0.0, 10.0)) < 1e-6 && (r - 5.0).abs() < 1e-6);
    assert!(matches!(ctx.shape(link, &top), Some(Projected::Line(..))));
    // Tilted planes see the circle as an ellipse.
    let tilted = cadrs_sketch::PlaneFrame {
        origin: [0.0; 3],
        u: [1.0, 0.0, 0.0],
        v: [0.0, -(0.5f64).sqrt(), (0.5f64).sqrt()],
    };
    let Some(Projected::Ellipse { minor, .. }) = ctx.shape(link, &tilted) else {
        panic!()
    };
    assert!((minor - 5.0 * (0.5f64).sqrt()).abs() < 1e-6);
}

#[test]
fn pierce_holds_a_point_where_an_edge_crosses_the_plane() {
    let mut d = Doc::new();
    // A box on Top reaching across the Front plane (y from -10 to 20).
    let s1 = d.sketch(PlaneRef::Top);
    d.edit(s1, rect(0.0, -10.0, 40.0, 30.0));
    let ex = d.extrude(s1, 15.0, |_| true);
    // A sketch on Front with a line from the origin.
    let s2 = d.sketch(PlaneRef::Front);
    d.edit(
        s2,
        SketchOp::AddPolyline {
            points: vec![Vec2::new(-20.0, 30.0), Vec2::new(10.0, 5.0)],
            closed: false,
            construction: false,
            label: "Add line",
        },
    );
    // The right side's top edge (x = 40, z = 15) runs along Y and pierces Front at (40, 15).
    let features = d.features();
    let solid = parts(&features).remove(0).solid;
    let front = PlaneRef::Front.frame();
    let edge = solid
        .edges
        .iter()
        .find(|e| {
            e.points.len() == 2
                && e.points
                    .iter()
                    .all(|p| (p[0] - 40.0).abs() < 1e-9 && (p[2] - 15.0).abs() < 1e-9)
        })
        .unwrap()
        .name;
    let link = Link::Edge {
        feature: ex.0,
        edge,
    };
    let ctx = cadrs_core::links::LinkContext {
        solids: vec![(ex, &solid)],
        features: &features,
    };
    let at = ctx.pierce(link, &front, Vec2::ZERO).unwrap();
    assert!(at.distance(Vec2::new(40.0, 15.0)) < 1e-9, "{at:?}");
    // Pierce the line's end: it moves there and is fixed.
    let p = d.g(s2).point_at(Vec2::new(10.0, 5.0), 1e-6).unwrap();
    d.edit(
        s2,
        SketchOp::AddConstraint {
            constraints: vec![ConstraintOf::Pierce(PointRef::Point(p), link)],
            label: "Add pierce",
        },
    );
    assert!(d.g(s2).pos(p).distance(Vec2::new(40.0, 15.0)) < 1e-9);
    // The box grows taller: the point follows.
    let g1 = d.g(s1).clone();
    let features = d.features();
    let mut e = features
        .iter()
        .find(|f| f.id == ex)
        .unwrap()
        .extrude()
        .unwrap()
        .clone();
    e.depth = 22.0;
    d.h.execute(
        &mut d.d,
        &SetExtrude {
            element: d.el,
            feature: ex,
            extrude: e,
            label: "Depth".into(),
        },
    )
    .unwrap();
    let _ = g1;
    assert!(d.g(s2).pos(p).distance(Vec2::new(40.0, 22.0)) < 1e-9);
    // Circles pierce at the crossing nearest the point.
    let s3 = d.sketch(PlaneRef::Right);
    d.edit(s3, circle(0.0, 0.0, 10.0));
    let s4 = d.sketch(PlaneRef::Top);
    let features = d.features();
    let n = features.len() - 1;
    let ctx = cadrs_core::links::LinkContext {
        solids: vec![],
        features: &features[..n],
    };
    let c3 = d.g(s3).curves.keys().next().unwrap();
    let link = Link::SketchCurve {
        feature: s3.0,
        curve: c3,
    };
    let top = PlaneRef::Top.frame();
    let hit = ctx.pierce(link, &top, Vec2::new(0.0, 8.0)).unwrap();
    assert!(hit.distance(Vec2::new(0.0, 10.0)) < 1e-9, "{hit:?}");
    let hit = ctx.pierce(link, &top, Vec2::new(0.0, -8.0)).unwrap();
    assert!(hit.distance(Vec2::new(0.0, -10.0)) < 1e-9, "{hit:?}");
    let _ = s4;
}

#[test]
fn imprinting_splits_face_regions_unless_disabled() {
    let (mut d, _s1, _ex, s2) = box_with_face_sketch();
    // The top face's four edges are imprinted.
    assert_eq!(d.g(s2).imprint.len(), 4);
    // A circle over the right edge (x = 50): three regions, and the part of the circle on the
    // face can be extruded.
    d.edit(s2, circle(50.0, 15.0, 10.0));
    let r = regions(d.g(s2));
    assert_eq!(r.len(), 3);
    let half = std::f64::consts::PI * 50.0;
    let on_face = r
        .iter()
        .find(|r| r.contains(Vec2::new(45.0, 15.0)))
        .unwrap()
        .clone();
    assert!((on_face.area() - half).abs() < 1e-6);
    let ex2 = d.extrude(s2, 5.0, |r| r.contains(Vec2::new(45.0, 15.0)));
    let p = parts(&d.features());
    let part = p.iter().find(|p| p.feature == ex2).unwrap();
    assert!(
        (part.solid.volume() - half * 5.0).abs() / (half * 5.0) < 0.01,
        "{}",
        part.solid.volume()
    );
    // Disable imprinting: only the circle's region; undoable, and saved.
    d.h.execute(
        &mut d.d,
        &SetSketchImprinting {
            element: d.el,
            feature: s2,
            disable_imprinting: true,
        },
    )
    .unwrap();
    assert!(d.g(s2).imprint.is_empty());
    assert_eq!(regions(d.g(s2)).len(), 1);
    let text = ron::to_string(&d.d).unwrap();
    let back: Document = ron::from_str(&text).unwrap();
    assert_eq!(back, d.d);
    d.h.undo(&mut d.d);
    assert_eq!(d.g(s2).imprint.len(), 4);
    assert_eq!(regions(d.g(s2)).len(), 3);
}

#[test]
fn links_and_texts_are_saved() {
    let (mut d, _s1, ex, s2) = box_with_face_sketch();
    let features = d.features();
    let solid = parts(&features).remove(0).solid;
    let frame =
        d.d.element(d.el)
            .unwrap()
            .feature(s2)
            .unwrap()
            .sketch()
            .unwrap()
            .plane
            .unwrap()
            .frame();
    let ctx = cadrs_core::links::LinkContext {
        solids: vec![(ex, &solid)],
        features: &features,
    };
    let edge = solid.edges.iter().find(|e| ends(&e.name)).unwrap().name;
    let link = Link::Edge {
        feature: ex.0,
        edge,
    };
    d.edit(
        s2,
        SketchOp::Use {
            items: vec![(ctx.shape(link, &frame).unwrap(), link)],
        },
    );
    d.edit(
        s2,
        SketchOp::AddText {
            origin: Vec2::new(5.0, 5.0),
            dir: Vec2::new(1.0, 0.0),
            height: 8.0,
            style: cadrs_sketch::text::TextStyle {
                bold: true,
                ..cadrs_sketch::text::TextStyle::new("CAD")
            },
        },
    );
    let text = ron::to_string(&d.d).unwrap();
    let back: Document = ron::from_str(&text).unwrap();
    assert_eq!(back, d.d);
    let g = back.elements[0].features()[2]
        .sketch()
        .unwrap()
        .geometry
        .clone();
    assert_eq!(uses(&g).len(), 1);
    assert_eq!(g.texts.len(), 1);
}

#[test]
fn text_regions_extrude_into_raised_letters() {
    let mut d = Doc::new();
    let s1 = d.sketch(PlaneRef::Top);
    d.edit(
        s1,
        SketchOp::AddText {
            origin: Vec2::new(0.0, 0.0),
            dir: Vec2::new(1.0, 0.0),
            height: 10.0,
            style: cadrs_sketch::text::TextStyle::new("T"),
        },
    );
    let r = regions(d.g(s1));
    assert_eq!(r.len(), 1);
    let area = r[0].area();
    assert!(area > 10.0);
    let ex = d.extrude(s1, 2.0, |_| true);
    let p = parts(&d.features());
    let part = p.iter().find(|p| p.feature == ex).unwrap();
    assert!((part.solid.volume() - area * 2.0).abs() < 1e-6 * area);
    // "T" has 8 corners: 8 flat sides.
    let sides = part
        .solid
        .faces
        .iter()
        .filter(|f| matches!(f.name.origin, FaceOrigin::Side { .. }))
        .count();
    assert_eq!(sides, 8);
}

/// P3.7 (PS21.4, X13): Use of a rim's inner edge, made by extruding an ellipse's offset (the
/// kernel splits a whole offset ellipse into two half edges): the projection is the whole exact
/// offset ellipse, 0.125 in inside the 6 × 4 in ellipse.
#[test]
fn use_of_an_offset_ellipse_edge_projects_the_whole_offset_ellipse() {
    let mut d = Doc::new();
    let s1 = d.sketch(PlaneRef::Top);
    d.edit(
        s1,
        SketchOp::AddEllipse {
            center: Vec2::new(0.0, 0.0),
            major: Vec2::new(76.2, 0.0),
            minor: 50.8,
            construction: false,
        },
    );
    let e = d.g(s1).curves.keys().next().unwrap();
    d.edit(
        s1,
        SketchOp::Offset {
            chain: vec![(e, false)],
            distance: 3.175,
            left: true,
            label: (0.0, 0.0),
        },
    );
    // The band between the two curves.
    let ex = d.extrude(s1, 3.175, |r| !r.holes.is_empty());
    let features = d.features();
    let solid = parts(&features).remove(0).solid;
    // The inner top edge through (0, 47.625, 3.175).
    let at = [0.0, 47.625, 3.175];
    let edge = solid
        .edges
        .iter()
        .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)))
        .unwrap();
    // (The display polyline's sag from the curve.)
    assert!(edge.distance(at) < 0.5, "{}", edge.distance(at));
    let frame = PlaneRef::Top.frame();
    let ctx = cadrs_core::links::LinkContext {
        solids: vec![(ex, &*solid)],
        features: &features,
    };
    let link = Link::Edge {
        feature: ex.0,
        edge: edge.name,
    };
    let Some(Projected::EllipseOffset {
        center,
        major,
        minor,
        distance,
    }) = ctx.shape(link, &frame)
    else {
        panic!("{:?}", ctx.shape(link, &frame))
    };
    assert!(center.distance(Vec2::new(0.0, 0.0)) < 1e-9);
    assert!(major.distance(Vec2::new(76.2, 0.0)) < 1e-9);
    assert!((minor - 50.8).abs() < 1e-9 && (distance + 3.175).abs() < 1e-9);
    let s2 = d.sketch(PlaneRef::Top);
    d.edit(
        s2,
        SketchOp::Use {
            items: vec![(ctx.shape(link, &frame).unwrap(), link)],
        },
    );
    let g = d.g(s2);
    assert_eq!(uses(g).len(), 1);
    assert!(
        g.curves
            .values()
            .any(|c| matches!(c.kind, CurveKind::EllipseOffset { .. }))
    );
}

#[test]
fn a_plane_is_used_by_its_trace_and_a_line_made_normal_to_it() {
    // Final re-audit, S12.10: Front (y = 0) cuts a Top sketch along its x axis; the trace is
    // 100 mm long, centred on the sketch's origin. A plane parallel to the sketch has none.
    use cadrs_core::links::{Curve3, PLANE_TRACE_HALF, plane_trace};
    use cadrs_sketch::{Link, PlaneRef, SketchOp};
    let (top, front) = (PlaneRef::Top.frame(), PlaneRef::Front.frame());
    let Some(Curve3::Line(a, b)) = plane_trace(&front, &top) else {
        panic!("no trace")
    };
    let (pa, pb) = (top.to_sketch(a), top.to_sketch(b));
    assert!(pa.y.abs() < 1e-9 && pb.y.abs() < 1e-9 && (pa.x + pb.x).abs() < 1e-9);
    assert!((pa.distance(pb) - 2.0 * PLANE_TRACE_HALF).abs() < 1e-9);
    assert!(plane_trace(&top, &top).is_none());
    // A slanted line made normal to Front turns vertical; the trace is construction, fixed by
    // its Use link.
    let mut g = cadrs_sketch::Sketch::new();
    let line = g.add_line(Vec2::new(10.0, 10.0), Vec2::new(20.0, 30.0));
    SketchOp::NormalToPlane {
        line,
        trace: (pa, pb),
        link: Link::Plane(PlaneRef::Front),
    }
    .apply(&mut g)
    .unwrap();
    let (p, q) = g.curve_ends(line).unwrap();
    assert!(
        (g.pos(p).x - g.pos(q).x).abs() < 1e-9,
        "{:?} {:?}",
        g.pos(p),
        g.pos(q)
    );
    let trace = g.curves.keys().find(|k| *k != line).unwrap();
    assert!(g.curves[trace].construction);
    assert_eq!(g.links().len(), 1);
    assert!(cadrs_sketch::solve::conflicts(&g).is_empty());
    // Again: nothing new.
    assert!(
        SketchOp::NormalToPlane {
            line,
            trace: (pa, pb),
            link: Link::Plane(PlaneRef::Front)
        }
        .apply(&mut g)
        .is_err()
    );
}

#[test]
fn a_hole_through_the_face_is_no_region() {
    // A 50 × 30 plate with a hole of radius 5, and a sketch on its top face with a line across
    // it: the hole's edge is imprinted, but the hole is no region (its fill hid the hole).
    let mut d = Doc::new();
    let s1 = d.sketch(PlaneRef::Top);
    d.edit(s1, rect(0.0, 0.0, 50.0, 30.0));
    d.edit(s1, circle(25.0, 15.0, 5.0));
    let ex = d.extrude(s1, 25.0, |r| !r.contains(Vec2::new(25.0, 15.0)));
    let top = face_plane(
        &d.features(),
        ex,
        cap_name(&d.features(), ex, 0, true).unwrap(),
    )
    .unwrap();
    let s2 = d.sketch(top);
    let line = vec![Vec2::new(-10.0, 5.0), Vec2::new(60.0, 5.0)];
    d.edit(
        s2,
        SketchOp::AddPolyline {
            points: line,
            closed: false,
            construction: false,
            label: "Add line",
        },
    );
    assert_eq!(d.g(s2).imprint.len(), 5);
    let r = regions(d.g(s2));
    assert_eq!(r.len(), 2);
    assert!(!r.iter().any(|r| r.contains(Vec2::new(25.0, 15.0))));
}

#[test]
fn a_line_snapped_to_a_hole_centre_follows_the_hole() {
    // A 50 × 30 plate with a hole of radius 5 at (25, 15), and a sketch on its top face.
    let mut d = Doc::new();
    let s1 = d.sketch(PlaneRef::Top);
    d.edit(s1, rect(0.0, 0.0, 50.0, 30.0));
    d.edit(s1, circle(25.0, 15.0, 5.0));
    let ex = d.extrude(s1, 25.0, |r| !r.contains(Vec2::new(25.0, 15.0)));
    let top = face_plane(
        &d.features(),
        ex,
        cap_name(&d.features(), ex, 0, true).unwrap(),
    )
    .unwrap();
    let s2 = d.sketch(top);
    // The imprints are linked to their edges.
    assert!(d.g(s2).imprint.iter().all(|i| i.link.is_some()));
    // A line from the hole's centre to the plate's corner, drawn against the snapping copy.
    let ext = cadrs_sketch::external::External::of(d.g(s2)).unwrap();
    let op = SketchOp::AddPolyline {
        points: vec![Vec2::new(25.0, 15.0), Vec2::new(50.0, 30.0)],
        closed: false,
        construction: false,
        label: "Add line",
    };
    let op = ext.commit(d.g(s2), op);
    d.edit(s2, op);
    assert_eq!(uses(d.g(s2)).len(), 2);
    let line_start = |g: &Sketch| {
        g.curves
            .values()
            .find_map(|c| match c.kind {
                CurveKind::Line { a, b } if !c.construction => Some([g.pos(a), g.pos(b)]),
                _ => None,
            })
            .unwrap()
    };
    // The hole moves 10 mm along X: the line's end follows it, the corner end stays.
    let centre = d.g(s1).point_at(Vec2::new(25.0, 15.0), 1e-6).unwrap();
    d.edit(
        s1,
        SketchOp::MovePoints {
            moves: vec![(centre, Vec2::new(35.0, 15.0))],
        },
    );
    let ends = line_start(d.g(s2));
    assert!(
        ends.iter()
            .any(|p| p.distance(Vec2::new(35.0, 15.0)) < 1e-6),
        "{ends:?}"
    );
    assert!(
        ends.iter()
            .any(|p| p.distance(Vec2::new(50.0, 30.0)) < 1e-6),
        "{ends:?}"
    );
    assert!(d.g(s2).broken.is_empty());
}

/// Use of a face whose arc edge is seen at an angle: the arc projects to an elliptical arc,
/// meeting the projected line at its ends, so the face's outline is a region (the half disc's
/// area foreshortened by the tilt).
#[test]
fn an_arc_edge_seen_at_an_angle_projects_to_an_elliptical_arc() {
    let mut d = Doc::new();
    // A half disc of radius 10 on Top (the arc over y > 0), extruded 20 up.
    let s1 = d.sketch(PlaneRef::Top);
    d.edit(
        s1,
        SketchOp::Batch(vec![
            SketchOp::AddArc {
                center: Vec2::new(0.0, 0.0),
                start: Vec2::new(10.0, 0.0),
                end: Vec2::new(-10.0, 0.0),
                construction: false,
            },
            SketchOp::AddPolyline {
                points: vec![Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0)],
                closed: false,
                construction: false,
                label: "Add line",
            },
        ]),
    );
    let ex = d.extrude(s1, 20.0, |_| true);
    let features = d.features();
    let solid = parts(&features).remove(0).solid;
    let top = cap_name(&features, ex, 0, true).unwrap();
    let ctx = cadrs_core::links::LinkContext {
        solids: vec![(ex, &*solid)],
        features: &features,
    };
    // A plane tilted 45° about X (both ways round: the arc runs counter-clockwise either way).
    let h = (0.5f64).sqrt();
    for v in [[0.0, h, h], [0.0, h, -h]] {
        let tilted = cadrs_sketch::PlaneFrame {
            origin: [0.0, 0.0, 5.0],
            u: [1.0, 0.0, 0.0],
            v,
        };
        let items: Vec<(Projected, Link)> = cadrs_core::links::face_edges(&solid, &top)
            .into_iter()
            .map(|edge| {
                let link = Link::Edge {
                    feature: ex.0,
                    edge,
                };
                (ctx.shape(link, &tilted).expect("every edge projects"), link)
            })
            .collect();
        assert_eq!(items.len(), 2, "{items:?}");
        let arc = items
            .iter()
            .find_map(|(p, _)| match *p {
                Projected::EllipseArc {
                    center,
                    major,
                    minor,
                    start,
                    end,
                } => Some((center, major, minor, start, end)),
                _ => None,
            })
            .expect("an elliptical arc");
        let (center, major, minor, start, end) = arc;
        assert!(
            (center.distance(major) - 10.0).abs() < 1e-9 && (minor - 10.0 * h).abs() < 1e-9,
            "{arc:?}"
        );
        // Its ends are the line's.
        let Some(Projected::Line(a, b)) = items
            .iter()
            .map(|(p, _)| p.clone())
            .find(|p| matches!(p, Projected::Line(..)))
        else {
            panic!("{items:?}")
        };
        assert!(
            (start.distance(a) < 1e-9 && end.distance(b) < 1e-9)
                || (start.distance(b) < 1e-9 && end.distance(a) < 1e-9),
            "{arc:?} {a:?} {b:?}"
        );
        let mut g = Sketch::new();
        for (shape, link) in &items {
            g.add_projected(shape.clone(), *link).unwrap();
        }
        assert!(
            g.curves
                .values()
                .any(|c| matches!(c.kind, CurveKind::EllipseArc { .. }))
        );
        // Fully defined (projected), and one region: the half ellipse.
        assert!(solve::analyze(&g).fully_constrained());
        let rs = regions(&g);
        assert_eq!(rs.len(), 1, "{rs:?}");
        let want = std::f64::consts::PI * 100.0 / 2.0 * h;
        assert!(
            (rs[0].area() - want).abs() < 1e-6,
            "{} != {want}",
            rs[0].area()
        );
        // The source moves (taller): the projection follows.
        let shape = items[0].0.clone();
        let c = g.curves.keys().next().unwrap();
        assert!(g.set_projected(c, shape));
    }
}

/// Use of a face with a spline edge: the edge projects to a spline through points on it (or a
/// line when seen edge-on), closing the face's outline into a region of the right size.
#[test]
fn a_spline_edge_projects_to_a_spline() {
    let mut d = Doc::new();
    // A profile on Top: a spline from (0, 0) to (40, 0) over the x axis, closed by a line.
    let s1 = d.sketch(PlaneRef::Top);
    d.edit(
        s1,
        SketchOp::Batch(vec![
            SketchOp::AddSpline {
                points: vec![
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 8.0),
                    Vec2::new(25.0, 12.0),
                    Vec2::new(40.0, 0.0),
                ],
                periodic: false,
                start_tangent: None,
                end_tangent: None,
                construction: false,
            },
            SketchOp::AddPolyline {
                points: vec![Vec2::new(40.0, 0.0), Vec2::new(0.0, 0.0)],
                closed: false,
                construction: false,
                label: "Add line",
            },
        ]),
    );
    let area = regions(d.g(s1))[0].area();
    let ex = d.extrude(s1, 10.0, |_| true);
    let features = d.features();
    let solid = parts(&features).remove(0).solid;
    let top = cap_name(&features, ex, 0, true).unwrap();
    let ctx = cadrs_core::links::LinkContext {
        solids: vec![(ex, &*solid)],
        features: &features,
    };
    let project = |frame: &cadrs_sketch::PlaneFrame| -> Vec<(Projected, Link)> {
        cadrs_core::links::face_edges(&solid, &top)
            .into_iter()
            .map(|edge| {
                let link = Link::Edge {
                    feature: ex.0,
                    edge,
                };
                (ctx.shape(link, frame).expect("every edge projects"), link)
            })
            .collect()
    };
    let h = (0.5f64).sqrt();
    let flat = cadrs_sketch::PlaneFrame {
        origin: [0.0, 0.0, 30.0],
        u: [1.0, 0.0, 0.0],
        v: [0.0, 1.0, 0.0],
    };
    let tilted = cadrs_sketch::PlaneFrame {
        origin: [0.0, 0.0, 5.0],
        u: [1.0, 0.0, 0.0],
        v: [0.0, h, h],
    };
    for (frame, scale) in [(flat, 1.0), (tilted, h)] {
        let items = project(&frame);
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(
            items
                .iter()
                .any(|(p, _)| matches!(p, Projected::Spline { closed: false, .. })),
            "{items:?}"
        );
        let mut g = Sketch::new();
        for (shape, link) in &items {
            g.add_projected(shape.clone(), *link).unwrap();
        }
        assert!(
            g.curves
                .values()
                .any(|c| matches!(c.kind, CurveKind::Spline { .. }))
        );
        assert!(solve::analyze(&g).fully_constrained());
        let rs = regions(&g);
        assert_eq!(rs.len(), 1, "{rs:?}");
        let want = area * scale;
        assert!(
            (rs[0].area() - want).abs() < 1e-3 * want,
            "{} != {want}",
            rs[0].area()
        );
        // Following a source with fewer points keeps the curve, its ends and the region.
        let k = g
            .curves
            .iter()
            .find(|(_, c)| matches!(c.kind, CurveKind::Spline { .. }))
            .unwrap()
            .0;
        let Some((Projected::Spline { points, .. }, _)) = items
            .iter()
            .find(|(p, _)| matches!(p, Projected::Spline { .. }))
        else {
            unreachable!()
        };
        let fewer: Vec<Vec2> = points
            .iter()
            .copied()
            .step_by(2)
            .chain(points.last().copied())
            .collect();
        let fewer: Vec<Vec2> = fewer
            .iter()
            .enumerate()
            .filter(|(i, p)| *i == 0 || fewer[i - 1].distance(**p) > 0.0)
            .map(|(_, p)| *p)
            .collect();
        assert!(g.set_projected(
            k,
            Projected::Spline {
                points: fewer.clone(),
                closed: false
            }
        ));
        assert_eq!(g.splines[k].points.len(), fewer.len());
        assert_eq!(regions(&g).len(), 1);
    }
    // Seen edge-on (from Front, the top face's edges lie along z = 10): lines.
    for (p, _) in project(&PlaneRef::Front.frame()) {
        assert!(matches!(p, Projected::Line(..)), "{p:?}");
    }
}

/// Sets an extrude's whole feature (the helpers' extrudes are New with every field defaulted).
fn add_extrude(d: &mut Doc, extrude: ExtrudeFeature) -> FeatureId {
    let ex = FeatureId::new();
    d.h.execute(
        &mut d.d,
        &AddExtrude {
            element: d.el,
            feature: ex,
            extrude: ExtrudeFeature::default(),
        },
    )
    .unwrap();
    d.h.execute(
        &mut d.d,
        &SetExtrude {
            element: d.el,
            feature: ex,
            extrude,
            label: "Select".into(),
        },
    )
    .unwrap();
    ex
}

#[test]
fn a_slanted_cut_cylinders_edge_is_used_as_an_ellipse_and_a_plate_round_it_joins() {
    use cadrs_core::PartId;
    use cadrs_core::document::{BooleanOp, FaceRef};
    let mut d = Doc::new();
    // A 10 mm radius cylinder cut at a slant (1 in 6), as the presso foot's leg is cut level.
    let s1 = d.sketch(PlaneRef::Top);
    d.edit(s1, circle(0.0, 0.0, 10.0));
    let leg = d.extrude(s1, 60.0, |_| true);
    let s2 = d.sketch(PlaneRef::Front);
    d.edit(
        s2,
        SketchOp::AddPolyline {
            points: vec![
                Vec2::new(-30.0, 25.0),
                Vec2::new(30.0, 35.0),
                Vec2::new(30.0, 80.0),
                Vec2::new(-30.0, 80.0),
            ],
            closed: true,
            construction: false,
            label: "Add polygon",
        },
    );
    let rs = regions(d.g(s2))
        .iter()
        .map(|r| RegionRef::new(s2, r))
        .collect();
    let cut = add_extrude(
        &mut d,
        ExtrudeFeature {
            regions: rs,
            depth: 100.0,
            symmetric: true,
            op: BooleanOp::Remove,
            merge_all: true,
            ..ExtrudeFeature::default()
        },
    );
    // The slanted face, extruded 5 mm along its normal (the foot's Extrude 4).
    let solid = parts(&d.features()).remove(0).solid;
    let i = solid.faces.iter().position(|f| f.name.op == cut.0).unwrap();
    let stub = add_extrude(
        &mut d,
        ExtrudeFeature {
            faces: vec![FaceRef {
                part: PartId::new(leg, 0),
                face: solid.faces[i].name,
                seed: solid.face_point(i).unwrap(),
            }],
            depth: 5.0,
            op: BooleanOp::Add,
            ..ExtrudeFeature::default()
        },
    );
    assert_eq!(parts(&d.features()).len(), 1);
    // A sketch on the stub's end uses its edge: one exact ellipse, the cylinder's radius across
    // and 10/cos θ along the slope (tan θ = 1/6), not a spline through points along it.
    let features = d.features();
    let solid = parts(&features).remove(0).solid;
    let cap = solid
        .faces
        .iter()
        .find(|f| f.name.op == stub.0 && matches!(f.name.origin, FaceOrigin::Cap { end: true, .. }))
        .unwrap()
        .name;
    let plane = face_plane(&features, stub, cap).unwrap();
    let s3 = d.sketch(plane);
    let frame = plane.frame();
    let ctx = cadrs_core::links::LinkContext {
        solids: vec![(leg, &*solid)],
        features: &features,
    };
    let edges = cadrs_core::links::face_edges(&solid, &cap);
    assert_eq!(edges.len(), 1);
    let link = Link::Edge {
        feature: stub.0,
        edge: edges[0],
    };
    let shape = ctx.shape(link, &frame).unwrap();
    let Projected::Ellipse {
        center,
        major,
        minor,
    } = shape
    else {
        panic!("{shape:?}")
    };
    assert!((minor - 10.0).abs() < 1e-6, "{minor}");
    let a = 10.0 * 37f64.sqrt() / 6.0;
    assert!(
        (major.distance(center) - a).abs() < 1e-6,
        "{}",
        major.distance(center)
    );
    d.edit(
        s3,
        SketchOp::Use {
            items: vec![(shape, link)],
        },
    );
    let g = d.g(s3);
    assert!(
        g.curves
            .values()
            .all(|c| matches!(c.kind, CurveKind::Ellipse { .. }))
    );
    assert_eq!(g.points.len(), 2, "the ellipse's centre and major point");
    // A plate round it, back along the stub (its inner face on the stub's side): one part.
    d.edit(s3, circle(center.x, center.y, 30.0));
    let ring = std::f64::consts::PI * (900.0 - a * 10.0);
    let rs: Vec<RegionRef> = regions(d.g(s3))
        .iter()
        .filter(|r| (r.area() - ring).abs() < 1e-3 * ring)
        .map(|r| RegionRef::new(s3, r))
        .collect();
    assert_eq!(rs.len(), 1);
    add_extrude(
        &mut d,
        ExtrudeFeature {
            regions: rs,
            depth: 5.0,
            flip: true,
            op: BooleanOp::Add,
            merge_all: true,
            ..ExtrudeFeature::default()
        },
    );
    let build = cadrs_core::rebuild::build(&d.features());
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    assert_eq!(build.parts.len(), 1, "the plate joins the leg");
}

#[test]
fn an_ellipse_in_space_projects_onto_any_plane_and_crosses_one() {
    use cadrs_core::links::{Curve3, crossings, project, projected_distance};
    let n = |v: [f64; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / l, v[1] / l, v[2] / l]
    };
    // An ellipse (semi-axes 12 and 5) in a plane tipped about X and Y.
    let center = [1.0, 2.0, 3.0];
    let a = n([0.0, 3.0, 1.0]).map(|x| x * 12.0);
    let normal = n([1.0, -0.2, 0.6]);
    let normal = {
        // At right angles to the major axis.
        let k: f64 = (0..3).map(|i| normal[i] * a[i]).sum::<f64>() / 144.0;
        n([0, 1, 2].map(|i| normal[i] - k * a[i]))
    };
    let b = {
        let c = [
            normal[1] * a[2] - normal[2] * a[1],
            normal[2] * a[0] - normal[0] * a[2],
            normal[0] * a[1] - normal[1] * a[0],
        ];
        n(c).map(|x| x * 5.0)
    };
    let at = |t: f64| [0, 1, 2].map(|i| center[i] + a[i] * t.cos() + b[i] * t.sin());
    let curve = Curve3::Ellipse {
        center,
        major: [0, 1, 2].map(|i| center[i] + a[i]),
        minor: 5.0,
        normal,
        offset: 0.0,
    };
    // Seen from Top, Front and a tilted plane: an ellipse through every point's projection.
    let tilted = cadrs_sketch::PlaneFrame {
        origin: [0.0, 0.0, 4.0],
        u: [1.0, 0.0, 0.0],
        v: [0.0, (0.5f64).sqrt(), (0.5f64).sqrt()],
    };
    for frame in [PlaneRef::Top.frame(), PlaneRef::Front.frame(), tilted] {
        let shape = project(curve.clone(), &frame).unwrap();
        assert!(matches!(shape, Projected::Ellipse { .. }), "{shape:?}");
        for i in 0..36 {
            let p = frame.to_sketch(at(i as f64 * 10f64.to_radians()));
            let d = projected_distance(&shape, p).unwrap();
            assert!(d < 1e-6, "{d}");
        }
        // Where it crosses the plane: two points, on the ellipse.
        let hits = crossings(curve.clone(), &frame);
        assert_eq!(hits.len(), 2, "{hits:?}");
        for h in hits {
            let w = frame.to_world(h);
            let d: [f64; 3] = [0, 1, 2].map(|i| w[i] - center[i]);
            let along = |v: [f64; 3], k: f64| (0..3).map(|i| d[i] * v[i]).sum::<f64>() / (k * k);
            let (x, y) = (along(a, 12.0), along(b, 5.0));
            assert!((x * x + y * y - 1.0).abs() < 1e-9, "{x} {y}");
            assert!(along(normal, 1.0).abs() < 1e-9);
        }
    }
    // Edge-on (the plane's normal in the ellipse's plane, along its minor axis): a segment
    // across the major axis.
    let side = cadrs_sketch::PlaneFrame {
        origin: [0.0; 3],
        u: n(a),
        v: normal,
    };
    let Some(Projected::Line(p, q)) = project(curve, &side) else {
        panic!()
    };
    assert!((p.distance(q) - 24.0).abs() < 1e-6);
}

#[test]
fn a_sketch_on_a_slanted_cut_face_imprints_its_ellipse_and_its_regions_join_the_part() {
    use cadrs_core::document::BooleanOp;
    use cadrs_sketch::ImprintShape;
    let mut d = Doc::new();
    // A 10 mm radius cylinder cut at a slant (1 in 6), as the presso foot's leg.
    let s1 = d.sketch(PlaneRef::Top);
    d.edit(s1, circle(0.0, 0.0, 10.0));
    d.extrude(s1, 60.0, |_| true);
    let s2 = d.sketch(PlaneRef::Front);
    d.edit(
        s2,
        SketchOp::AddPolyline {
            points: vec![
                Vec2::new(-30.0, 25.0),
                Vec2::new(30.0, 35.0),
                Vec2::new(30.0, 80.0),
                Vec2::new(-30.0, 80.0),
            ],
            closed: true,
            construction: false,
            label: "Add polygon",
        },
    );
    let rs = regions(d.g(s2))
        .iter()
        .map(|r| RegionRef::new(s2, r))
        .collect();
    let cut = add_extrude(
        &mut d,
        ExtrudeFeature {
            regions: rs,
            depth: 100.0,
            symmetric: true,
            op: BooleanOp::Remove,
            merge_all: true,
            ..ExtrudeFeature::default()
        },
    );
    // A sketch on the cut face: its edge is imprinted as the exact ellipse (10 across, 10/cos θ
    // along the slope), not left out.
    let features = d.features();
    let solid = parts(&features).remove(0).solid;
    let face = solid
        .faces
        .iter()
        .find(|f| f.name.op == cut.0)
        .unwrap()
        .name;
    let s3 = d.sketch(face_plane(&features, cut, face).unwrap());
    let g = d.g(s3);
    let [im] = g.imprint.as_slice() else {
        panic!("{:?}", g.imprint)
    };
    let ImprintShape::Ellipse {
        center,
        major,
        minor,
    } = im.shape
    else {
        panic!("{:?}", im.shape)
    };
    let a = 10.0 * 37f64.sqrt() / 6.0;
    assert!((minor - 10.0).abs() < 1e-6 && (major.distance(center) - a).abs() < 1e-6);
    // Lines drawn from its minor axis's ends out to a circle beside it (the foot's outline)
    // end on the imprint: no loose ends, and regions either side of it.
    let across = {
        let m = (major - center) * (1.0 / a);
        Vec2::new(-m.y, m.x) * 10.0
    };
    let out = (major - center) * (40.0 / a);
    let (p, q) = (center + across, center - across);
    d.edit(
        s3,
        SketchOp::AddPolyline {
            points: vec![p, p + out],
            closed: false,
            construction: false,
            label: "Add line",
        },
    );
    d.edit(
        s3,
        SketchOp::AddPolyline {
            points: vec![q, q + out],
            closed: false,
            construction: false,
            label: "Add line",
        },
    );
    d.edit(s3, circle(center.x + out.x, center.y + out.y, 10.0));
    let g = d.g(s3);
    assert!(cadrs_sketch::diagnostics::loose_ends(g).is_empty());
    let rs = regions(g);
    let ellipse = std::f64::consts::PI * a * 10.0;
    assert!(
        rs.iter()
            .any(|r| (r.area() - ellipse).abs() < 1e-3 * ellipse)
    );
    assert_eq!(rs.len(), 3, "the ellipse, the strip and the circle");
    // Its regions extruded out from the face, joined: one part.
    let picked = rs.iter().map(|r| RegionRef::new(s3, r)).collect();
    add_extrude(
        &mut d,
        ExtrudeFeature {
            regions: picked,
            depth: 5.0,
            op: BooleanOp::Add,
            merge_all: true,
            ..ExtrudeFeature::default()
        },
    );
    let build = cadrs_core::rebuild::build(&d.features());
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    assert_eq!(build.parts.len(), 1);
}

#[test]
fn an_elliptical_arc_edge_is_exact_seen_edge_on_and_from_above() {
    use cadrs_core::links::{Curve3, crossings, edge_curve, project};
    use cadrs_core::solid::{EdgeEllipse, SolidEdge};
    // Half of a slanted cut's ellipse (20 along Y, 10 across, in z = 0), as the edge a later
    // feature splits it into: a coarse polyline whose points miss the ellipse's end at +Y.
    let at = |t: f64| [10.0 * t.sin(), 20.0 * t.cos(), 0.0];
    let face = cadrs_sketch::FaceName::new(
        FeatureId::new().0,
        FaceOrigin::Cap {
            region: 0,
            end: true,
        },
    );
    let edge = SolidEdge {
        name: EdgeName::new(face, face, 0),
        points: (0..7).map(|i| at(-1.3 + 2.2 * i as f64 / 6.0)).collect(),
        circle: None,
        ellipse: Some(EdgeEllipse {
            center: [0.0; 3],
            major: [0.0, 20.0, 0.0],
            minor: [10.0, 0.0, 0.0],
        }),
        tangent_group: None,
        smooth: false,
    };
    let curve = edge_curve(&edge).unwrap();
    assert!(matches!(curve, Curve3::EllipseArc { .. }), "{curve:?}");
    // Edge-on from Right: the segment reaches the ellipse's end (y = 20), not just the polyline's
    // nearest point to it (the outline a silhouette closes, presso df64's Sketch 5).
    let right = PlaneRef::Right.frame();
    let Some(Projected::Line(p, q)) = project(curve.clone(), &right) else {
        panic!()
    };
    let far = |v: Vec2| right.to_world(v)[1];
    let ys = [far(p), far(q)];
    assert!(ys.iter().any(|y| (y - 20.0).abs() < 1e-9), "{ys:?}");
    assert!(
        ys.iter().any(|y| (y - 20.0 * 1.3f64.cos()).abs() < 1e-9),
        "{ys:?}"
    );
    // From above: the elliptical arc itself, its ends the edge's.
    let Some(Projected::EllipseArc {
        center,
        major,
        minor,
        start,
        end,
    }) = project(curve.clone(), &PlaneRef::Top.frame())
    else {
        panic!()
    };
    assert!(center.distance(Vec2::ZERO) < 1e-9 && (major.distance(center) - 20.0).abs() < 1e-9);
    assert!((minor - 10.0).abs() < 1e-9);
    let ends = [at(-1.3), at(0.9)].map(|w| Vec2::new(w[0], w[1]));
    assert!(
        ends.iter()
            .all(|e| e.distance(start) < 1e-9 || e.distance(end) < 1e-9)
    );
    // It crosses y = 10 once (at t = -π/3; t = π/3 is past its end).
    let level = cadrs_sketch::PlaneFrame {
        origin: [0.0, 10.0, 0.0],
        u: [1.0, 0.0, 0.0],
        v: [0.0, 0.0, 1.0],
    };
    assert_eq!(crossings(curve, &level).len(), 1);
}
