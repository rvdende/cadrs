use super::*;
use crate::constraint::{ConstraintKind, Fit, fit};
use crate::solve::{self, analyze};
use crate::{Dimension, DimensionKind, SketchEntity, SketchOp};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn line(s: &mut Sketch, a: Vec2, b: Vec2) -> CurveId {
    SketchOp::AddPolyline {
        points: vec![a, b],
        closed: false,
        construction: false,
        label: "Add line",
    }
    .apply(s)
    .unwrap();
    s.curves
        .iter()
        .find(|(k, _)| {
            s.curve_ends(*k)
                .is_some_and(|(p, q)| s.pos(p).distance(a) < 1e-9 && s.pos(q).distance(b) < 1e-9)
        })
        .map(|(k, _)| k)
        .unwrap()
}

fn circle(s: &mut Sketch, c: Vec2, r: f64) -> CurveId {
    let before: Vec<CurveId> = s.curves.keys().collect();
    SketchOp::AddCircle {
        center: c,
        radius: r,
        construction: false,
    }
    .apply(s)
    .unwrap();
    s.curves.keys().find(|k| !before.contains(k)).unwrap()
}

fn arc(s: &mut Sketch, c: Vec2, start: Vec2, end: Vec2) -> CurveId {
    let before: Vec<CurveId> = s.curves.keys().collect();
    SketchOp::AddArc {
        center: c,
        start,
        end,
        construction: false,
    }
    .apply(s)
    .unwrap();
    s.curves.keys().find(|k| !before.contains(k)).unwrap()
}

fn ends(s: &Sketch, id: CurveId) -> (Vec2, Vec2) {
    let (a, b) = s.curve_ends(id).unwrap();
    (s.pos(a), s.pos(b))
}

fn close(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 1e-6
}

fn trim_op(s: &mut Sketch, id: CurveId, at: Vec2) {
    SketchOp::Trim {
        picks: vec![(id, at)],
        points: vec![],
    }
    .apply(s)
    .unwrap();
}

/// A horizontal line from (0,0) to (100,0), crossed by vertical lines at x = 30 and x = 60.
fn crossed_line() -> (Sketch, CurveId, CurveId, CurveId) {
    let mut s = Sketch::new();
    let h = line(&mut s, v(0.0, 0.0), v(100.0, 0.0));
    let v1 = line(&mut s, v(30.0, -20.0), v(30.0, 20.0));
    let v2 = line(&mut s, v(60.0, -20.0), v(60.0, 20.0));
    (s, h, v1, v2)
}

#[test]
fn trim_line_between_two_crossings() {
    let (mut s, h, v1, v2) = crossed_line();
    s.add_constraint(ConstraintOf::Horizontal(Orient::Line(CurveRef::Curve(h))));
    trim_op(&mut s, h, v(45.0, 0.0));
    // Two pieces are left: 0..30 and 60..100, each still horizontal.
    let lines: Vec<CurveId> = s
        .curves
        .keys()
        .filter(|k| *k != v1 && *k != v2)
        .collect();
    assert_eq!(lines.len(), 2);
    let mut spans: Vec<(Vec2, Vec2)> = lines.iter().map(|k| ends(&s, *k)).collect();
    spans.sort_by(|a, b| a.0.x.min(a.1.x).total_cmp(&b.0.x.min(b.1.x)));
    assert!(close(spans[0].0, v(0.0, 0.0)) && close(spans[0].1, v(30.0, 0.0)));
    assert!(close(spans[1].0, v(60.0, 0.0)) && close(spans[1].1, v(100.0, 0.0)));
    let horizontals = s
        .constraints
        .values()
        .filter(|c| matches!(c, ConstraintOf::Horizontal(Orient::Line(_))))
        .count();
    assert_eq!(horizontals, 2);
    // The new ends lie on the cutting lines.
    let on = |cut: CurveId| {
        s.constraints
            .values()
            .filter(|c| matches!(c, ConstraintOf::PointOnCurve(_, CurveRef::Curve(k)) if *k == cut))
            .count()
    };
    assert_eq!((on(v1), on(v2)), (1, 1));
    // Nothing moved in the solve.
    assert!(solve::conflicts(&s).is_empty());
    assert!(close(ends(&s, v1).0, v(30.0, -20.0)));
}

#[test]
fn trim_end_piece_and_undo_shape() {
    let (mut s, h, _, _) = crossed_line();
    // Past the last crossing: the piece from 60 to the end goes.
    trim_op(&mut s, h, v(80.0, 0.0));
    let (a, b) = ends(&s, h);
    assert!(close(a, v(0.0, 0.0)) && close(b, v(60.0, 0.0)));
    // The old end point is gone.
    assert!(s.point_at(v(100.0, 0.0), 1e-6).is_none());
}

#[test]
fn trim_without_crossings_deletes() {
    let (mut s, _, v1, _) = crossed_line();
    let lone = line(&mut s, v(0.0, 50.0), v(20.0, 50.0));
    trim_op(&mut s, lone, v(10.0, 50.0));
    assert!(!s.curves.contains_key(lone));
    assert!(s.curves.contains_key(v1));
    // A rectangle side between its corners (crossed only at its ends) is deleted too.
    let mut r = Sketch::new();
    SketchOp::AddPolyline {
        points: vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 5.0), v(0.0, 5.0)],
        closed: true,
        construction: false,
        label: "",
    }
    .apply(&mut r)
    .unwrap();
    let bottom = r
        .curves
        .keys()
        .find(|k| {
            let (a, b) = ends(&r, *k);
            a.y.abs() < 1e-9 && b.y.abs() < 1e-9
        })
        .unwrap();
    trim_op(&mut r, bottom, v(5.0, 0.0));
    assert_eq!(r.curves.len(), 3);
}

#[test]
fn trim_circle_becomes_arc() {
    let mut s = Sketch::new();
    let c = circle(&mut s, v(0.0, 0.0), 10.0);
    let l = line(&mut s, v(-20.0, 5.0), v(20.0, 5.0));
    s.dimensions.insert(Dimension::new(DimensionKind::Diameter { curve: c }, 20.0, 0.0));
    // Trim the top cap (above y = 5).
    trim_op(&mut s, c, v(0.0, 10.0));
    let CurveKind::Arc { .. } = s.curves[c].kind else {
        panic!("the circle is an arc now");
    };
    let g = s.arc_geom(c).unwrap();
    assert!((g.radius - 10.0).abs() < 1e-9);
    // The kept arc is the big lower part, from the right crossing round to the left one.
    assert!(g.sweep > std::f64::consts::PI);
    assert!(g.mid().y < 0.0);
    let x = 75f64.sqrt();
    assert!(close(g.start(), v(x, 5.0)) || close(g.start(), v(-x, 5.0)));
    // Its dimension stays and still holds.
    assert_eq!(s.dimensions.len(), 1);
    assert!(solve::conflicts(&s).is_empty());
    // Ends on the line.
    let on = s
        .constraints
        .values()
        .filter(|k| matches!(k, ConstraintOf::PointOnCurve(_, CurveRef::Curve(k)) if *k == l))
        .count();
    assert_eq!(on, 2);
    // A circle crossed once, or not at all, is deleted.
    let lone = circle(&mut s, v(100.0, 0.0), 5.0);
    trim_op(&mut s, lone, v(105.0, 0.0));
    assert!(!s.curves.contains_key(lone));
}

#[test]
fn trim_arc_between_crossings() {
    let mut s = Sketch::new();
    // A half circle of radius 10 over the top, crossed by x = ±5.
    let a = arc(&mut s, v(0.0, 0.0), v(10.0, 0.0), v(-10.0, 0.0));
    line(&mut s, v(5.0, -5.0), v(5.0, 20.0));
    line(&mut s, v(-5.0, -5.0), v(-5.0, 20.0));
    trim_op(&mut s, a, v(0.0, 10.0));
    let arcs: Vec<CurveId> = s
        .curves
        .iter()
        .filter(|(_, c)| matches!(c.kind, CurveKind::Arc { .. }))
        .map(|(k, _)| k)
        .collect();
    assert_eq!(arcs.len(), 2);
    for k in &arcs {
        let g = s.arc_geom(*k).unwrap();
        assert!((g.radius - 10.0).abs() < 1e-9);
        assert!(g.sweep < std::f64::consts::FRAC_PI_2);
    }
    // One circle: the two pieces are Equal.
    assert!(s.constraints.values().any(|c| matches!(c, ConstraintOf::Equal(..))));
    assert!(solve::conflicts(&s).is_empty());
    // Trimming an arc's end piece.
    let mut s = Sketch::new();
    let a = arc(&mut s, v(0.0, 0.0), v(10.0, 0.0), v(-10.0, 0.0));
    line(&mut s, v(0.0, -5.0), v(0.0, 20.0));
    trim_op(&mut s, a, v(-8.0, 6.0));
    let g = s.arc_geom(a).unwrap();
    assert!(close(g.start(), v(10.0, 0.0)) && close(g.end(), v(0.0, 10.0)));
}

#[test]
fn trim_keeps_and_drops_constraints() {
    let (mut s, h, v1, _) = crossed_line();
    let right = s.point_at(v(100.0, 0.0), 1e-9).unwrap();
    let left = s.point_at(v(0.0, 0.0), 1e-9).unwrap();
    // A point constrained on the removed part, one on the kept part, a length dimension on
    // the whole line, and an Equal with another line.
    let q_gone = s.add_point(v(80.0, 0.0));
    let q_kept = s.add_point(v(10.0, 0.0));
    s.add_constraint(ConstraintOf::PointOnCurve(PointRef::Point(q_gone), CurveRef::Curve(h)));
    s.add_constraint(ConstraintOf::PointOnCurve(PointRef::Point(q_kept), CurveRef::Curve(h)));
    s.add_constraint(ConstraintOf::Equal(CurveRef::Curve(h), CurveRef::Curve(v1)));
    s.add_constraint(ConstraintOf::Horizontal(Orient::Line(CurveRef::Curve(h))));
    s.dimensions.insert(Dimension::new(DimensionKind::Aligned { a: left, b: right }, 100.0, 5.0));
    trim_op(&mut s, h, v(80.0, 0.0));
    let has = |c: ConstraintOf<PointRef, CurveRef>| s.constraints.values().any(|x| *x == c);
    assert!(!has(ConstraintOf::PointOnCurve(PointRef::Point(q_gone), CurveRef::Curve(h))));
    assert!(has(ConstraintOf::PointOnCurve(PointRef::Point(q_kept), CurveRef::Curve(h))));
    assert!(has(ConstraintOf::Horizontal(Orient::Line(CurveRef::Curve(h)))));
    assert!(!has(ConstraintOf::Equal(CurveRef::Curve(h), CurveRef::Curve(v1))));
    // The dimension to the removed end went with it.
    assert!(s.dimensions.is_empty());
    assert!(solve::conflicts(&s).is_empty());
}

#[test]
fn drag_trim_is_one_step() {
    let (mut s, h, v1, v2) = crossed_line();
    let base = s.clone();
    // A drag crossing the middle of h and the top halves of both verticals.
    let op = SketchOp::Trim {
        picks: vec![(v1, v(30.0, 10.0)), (h, v(45.0, 0.0)), (v2, v(60.0, 10.0))],
        points: vec![],
    };
    assert_eq!(op.label(), "Trim");
    op.apply(&mut s).unwrap();
    // The verticals lost their tops (above y = 0), h its middle.
    assert!(close(ends(&s, v1).1, v(30.0, 0.0)) || close(ends(&s, v1).0, v(30.0, 0.0)));
    assert!(close(ends(&s, v2).1, v(60.0, 0.0)) || close(ends(&s, v2).0, v(60.0, 0.0)));
    assert_eq!(s.curves.len(), 4);
    // A pick on a piece another pick already split is found by position.
    let mut t = base.clone();
    SketchOp::Trim {
        picks: vec![(h, v(45.0, 0.0)), (h, v(80.0, 0.0))],
        points: vec![],
    }
    .apply(&mut t)
    .unwrap();
    let hs: Vec<CurveId> = t.curves.keys().filter(|k| *k != v1 && *k != v2).collect();
    assert_eq!(hs.len(), 1);
    // Picks that trim nothing fail (so no empty undo step).
    let mut e = base.clone();
    assert!(
        SketchOp::Trim {
            picks: vec![(h, v(45.0, 30.0))],
            points: vec![],
        }
        .apply(&mut e)
        .is_err()
    );
    // A standalone point is deleted.
    let p = e.add_point(v(5.0, 5.0));
    SketchOp::Trim {
        picks: vec![],
        points: vec![p],
    }
    .apply(&mut e)
    .unwrap();
    assert!(!e.points.contains_key(p));
}

#[test]
fn trim_preview_path() {
    let (s, h, _, _) = crossed_line();
    let path = trim_removed_path(&s, h, v(45.0, 0.0)).unwrap();
    assert!(close(path[0], v(30.0, 0.0)) && close(*path.last().unwrap(), v(60.0, 0.0)));
}

#[test]
fn extend_line_to_line_arc_and_circle() {
    // A line from (0,0) to (10,0); a wall at x = 50.
    let mut s = Sketch::new();
    let l = line(&mut s, v(0.0, 0.0), v(10.0, 0.0));
    let wall = line(&mut s, v(50.0, -10.0), v(50.0, 10.0));
    let end = free_end_near(&s, l, v(9.0, 0.0)).unwrap();
    assert!(close(s.pos(end), v(10.0, 0.0)));
    let ext = extension(&s, l, end, None).unwrap();
    assert!(close(ext.to, v(50.0, 0.0)));
    assert_eq!(ext.by, Some(wall));
    // With the cursor short of the wall, it stops at the cursor; past it, at the wall.
    assert!(close(extension(&s, l, end, Some(v(30.0, 4.0))).unwrap().to, v(30.0, 0.0)));
    assert!(close(extension(&s, l, end, Some(v(90.0, 0.0))).unwrap().to, v(50.0, 0.0)));
    SketchOp::Extend {
        curve: l,
        end,
        to: ext.to,
        by: ext.by,
    }
    .apply(&mut s)
    .unwrap();
    assert!(close(ends(&s, l).1, v(50.0, 0.0)));
    assert!(s.constraints.values().any(|c| matches!(c, ConstraintOf::PointOnCurve(_, CurveRef::Curve(k)) if *k == wall)));
    assert!(solve::conflicts(&s).is_empty());

    // To a circle: the near side.
    let mut s = Sketch::new();
    let l = line(&mut s, v(0.0, 0.0), v(10.0, 0.0));
    circle(&mut s, v(40.0, 0.0), 5.0);
    let end = free_end_near(&s, l, v(10.0, 0.0)).unwrap();
    assert!(close(extension(&s, l, end, None).unwrap().to, v(35.0, 0.0)));

    // To an arc (only where the arc is).
    let mut s = Sketch::new();
    let l = line(&mut s, v(0.0, 0.0), v(10.0, 0.0));
    // A quarter arc round (40, 0) from (40,-5) to (45, 0): not on the near side.
    arc(&mut s, v(40.0, 0.0), v(40.0, -5.0), v(45.0, 0.0));
    let end = free_end_near(&s, l, v(10.0, 0.0)).unwrap();
    assert!(close(extension(&s, l, end, None).unwrap().to, v(45.0, 0.0)));
}

#[test]
fn extend_arc_round_its_circle() {
    // A quarter arc round the origin from (10,0) to (0,10); a line along y = -x... use the X
    // axis mirrored: a vertical wall at x = -5.
    let mut s = Sketch::new();
    let a = arc(&mut s, v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0));
    let wall = line(&mut s, v(-5.0, -20.0), v(-5.0, 20.0));
    let end = free_end_near(&s, a, v(0.0, 10.0)).unwrap();
    let ext = extension(&s, a, end, None).unwrap();
    // Counter-clockwise from (0,10) the circle meets x = -5 at y = +√75.
    assert!(close(ext.to, v(-5.0, 75f64.sqrt())));
    assert_eq!(ext.by, Some(wall));
    SketchOp::Extend {
        curve: a,
        end,
        to: ext.to,
        by: ext.by,
    }
    .apply(&mut s)
    .unwrap();
    let g = s.arc_geom(a).unwrap();
    assert!((g.radius - 10.0).abs() < 1e-9);
    assert!(close(g.end(), v(-5.0, 75f64.sqrt())));
    // From its start (10,0) clockwise it meets an arc too: a circle round (10,-10) of radius 5
    // is crossed by nothing on the way... use a line at y = -5.
    line(&mut s, v(0.0, -5.0), v(20.0, -5.0));
    let start = free_end_near(&s, a, v(10.0, 0.0)).unwrap();
    let ext = extension(&s, a, start, None).unwrap();
    assert!(close(ext.to, v(75f64.sqrt(), -5.0)));
}

#[test]
fn extend_without_boundary_is_a_no_op() {
    let mut s = Sketch::new();
    let l = line(&mut s, v(0.0, 0.0), v(10.0, 0.0));
    // A line off to the side, not in the way.
    line(&mut s, v(20.0, 5.0), v(30.0, 5.0));
    let end = free_end_near(&s, l, v(10.0, 0.0)).unwrap();
    assert!(extension(&s, l, end, None).is_none());
    // Behind the end: nothing.
    assert!(extension(&s, l, end, Some(v(5.0, 0.0))).is_none());
    // A joined end cannot extend.
    let mut r = Sketch::new();
    let l1 = line(&mut r, v(0.0, 0.0), v(10.0, 0.0));
    line(&mut r, v(10.0, 0.0), v(10.0, 10.0));
    assert!(free_end_near(&r, l1, v(10.0, 0.0)).is_some_and(|p| close(r.pos(p), v(0.0, 0.0))));
}

#[test]
fn split_line_keeps_shape_and_point_dimensions() {
    let mut s = Sketch::new();
    let l = line(&mut s, v(0.0, 0.0), v(100.0, 0.0));
    s.add_constraint(ConstraintOf::Horizontal(Orient::Line(CurveRef::Curve(l))));
    let dof = analyze(&s).dof;
    SketchOp::Split {
        curve: l,
        at: vec![v(40.0, 0.0)],
    }
    .apply(&mut s)
    .unwrap();
    assert_eq!(s.curves.len(), 2);
    let p = s.point_at(v(40.0, 0.0), 1e-9).unwrap();
    assert_eq!(s.curves_at(p).count(), 2);
    // The only new freedom is the split point sliding along the line.
    assert_eq!(analyze(&s).dof, dof + 1, "{:?}", s.constraints.values().collect::<Vec<_>>());
    // The split point takes a dimension from the line's start, and slides along the line.
    let a = s.point_at(v(0.0, 0.0), 1e-9).unwrap();
    let far = s.point_at(v(100.0, 0.0), 1e-9).unwrap();
    // Hold the line's ends (so only the split point moves).
    s.add_constraint(ConstraintOf::FixPoint(PointRef::Point(a)));
    s.add_constraint(ConstraintOf::FixPoint(PointRef::Point(far)));
    let dof = analyze(&s).dof;
    SketchOp::SetDimension {
        dimension: Dimension::new(DimensionKind::Aligned { a, b: p }, 25.0, 5.0),
        moves: vec![],
        radii: vec![],
    }
    .apply(&mut s)
    .unwrap();
    assert!(close(s.pos(p), v(25.0, 0.0)));
    assert!(close(s.pos(far), v(100.0, 0.0)));
    assert_eq!(analyze(&s).dof, dof - 1);
    assert!(solve::conflicts(&s).is_empty());
    // A split at an end is refused.
    assert!(
        SketchOp::Split {
            curve: l,
            at: vec![v(0.0, 0.0)],
        }
        .apply(&mut s.clone())
        .is_err()
    );
}

#[test]
fn split_arc_and_circle() {
    let mut s = Sketch::new();
    let a = arc(&mut s, v(0.0, 0.0), v(10.0, 0.0), v(-10.0, 0.0));
    let dof = analyze(&s).dof;
    SketchOp::Split {
        curve: a,
        at: vec![v(0.0, 10.0)],
    }
    .apply(&mut s)
    .unwrap();
    assert_eq!(s.curves.len(), 2);
    for k in s.curves.keys() {
        let g = s.arc_geom(k).unwrap();
        assert!((g.radius - 10.0).abs() < 1e-9);
        assert!((g.sweep - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }
    assert_eq!(analyze(&s).dof, dof + 1);

    let mut s = Sketch::new();
    let c = circle(&mut s, v(0.0, 0.0), 10.0);
    let dof = analyze(&s).dof;
    // One point is not enough for a circle.
    assert!(
        SketchOp::Split {
            curve: c,
            at: vec![v(10.0, 0.0)],
        }
        .apply(&mut s.clone())
        .is_err()
    );
    SketchOp::Split {
        curve: c,
        at: vec![v(10.0, 0.0), v(-10.0, 0.0)],
    }
    .apply(&mut s)
    .unwrap();
    assert_eq!(s.curves.len(), 2);
    assert!(s.curves.values().all(|c| matches!(c.kind, CurveKind::Arc { .. })));
    // Two split points, each sliding round the circle.
    assert_eq!(analyze(&s).dof, dof + 2);
    assert!(analyze(&s).conflicting.is_empty());
}

#[test]
fn normal_line_to_circle_and_ellipse() {
    let mut s = Sketch::new();
    let c = circle(&mut s, v(0.0, 0.0), 10.0);
    let l = line(&mut s, v(12.0, 3.0), v(30.0, 8.0));
    let sel = [SketchEntity::Curve(l), SketchEntity::Curve(c)];
    let Fit::Complete(cs) = fit(ConstraintKind::Normal, &s, &sel) else {
        panic!("a line and a circle take Normal");
    };
    assert_eq!(cs, vec![ConstraintOf::Normal(CurveRef::Curve(l), CurveRef::Curve(c))]);
    // Either order; two lines do not.
    assert!(matches!(fit(ConstraintKind::Normal, &s, &[sel[1], sel[0]]), Fit::Complete(_)));
    let l2 = line(&mut s, v(0.0, 40.0), v(10.0, 40.0));
    assert_eq!(
        fit(ConstraintKind::Normal, &s, &[SketchEntity::Curve(l), SketchEntity::Curve(l2)]),
        Fit::Invalid
    );
    SketchOp::AddConstraint {
        constraints: cs,
        label: "Add normal",
    }
    .apply(&mut s)
    .unwrap();
    // The line's direction passes through the center.
    let (a, b) = ends(&s, l);
    let center = s.pos(match s.curves[c].kind {
        CurveKind::Circle { center, .. } => center,
        _ => unreachable!(),
    });
    assert!((b - a).normalize().cross(center - a).abs() < 1e-6);
    assert!(analyze(&s).conflicting.is_empty());

    // An ellipse: the line meets it square.
    let mut s = Sketch::new();
    SketchOp::AddEllipse {
        center: v(0.0, 0.0),
        major: v(20.0, 0.0),
        minor: 10.0,
        construction: false,
    }
    .apply(&mut s)
    .unwrap();
    let e = s.curves.keys().next().unwrap();
    let p = v(20.0 * 0.6f64.cos(), 10.0 * 0.6f64.sin());
    let l = line(&mut s, p, p + v(10.0, 3.0));
    let pe = s.point_at(p, 1e-9).unwrap();
    s.add_constraint(ConstraintOf::PointOnCurve(PointRef::Point(pe), CurveRef::Curve(e)));
    SketchOp::AddConstraint {
        constraints: vec![ConstraintOf::Normal(CurveRef::Curve(l), CurveRef::Curve(e))],
        label: "Add normal",
    }
    .apply(&mut s)
    .unwrap();
    let (a, b) = ends(&s, l);
    let g = s.ellipse_geom(e).unwrap();
    let t = g.nearest_t(a);
    assert!(g.point_at(t).distance(a) < 1e-6);
    // The line is square to the ellipse's tangent there.
    assert!(g.tangent_at(t).normalize().dot((b - a).normalize()).abs() < 1e-6);
    assert!(analyze(&s).conflicting.is_empty());
}
