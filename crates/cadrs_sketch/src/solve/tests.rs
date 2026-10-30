use super::*;
use crate::constraint::{ConstraintKind, Fit, fit, rectangle_constraints};
use crate::{Curve, Dimension, SketchEntity, SketchOp};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn line(s: &mut Sketch, a: Vec2, b: Vec2) -> (CurveId, PointId, PointId) {
    let pa = s.ensure_point(a);
    let pb = s.ensure_point(b);
    let c = s.curves.insert(Curve {
        kind: CurveKind::Line { a: pa, b: pb },
        construction: false,
    });
    (c, pa, pb)
}

fn circle(s: &mut Sketch, c: Vec2, r: f64) -> CurveId {
    let center = s.add_point(c);
    s.curves.insert(Curve {
        kind: CurveKind::Circle { center, radius: r },
        construction: false,
    })
}

fn arc(s: &mut Sketch, c: Vec2, a: Vec2, b: Vec2) -> CurveId {
    let center = s.ensure_point(c);
    let start = s.ensure_point(a);
    let end = s.ensure_point(b);
    s.curves.insert(Curve {
        kind: CurveKind::Arc { center, start, end },
        construction: false,
    })
}

/// A rectangle from the rectangle tool: its sides (bottom, right, top, left) and corners.
fn rect(s: &mut Sketch, a: Vec2, c: Vec2) -> ([CurveId; 4], [PointId; 4]) {
    let corners = [a, v(c.x, a.y), c, v(a.x, c.y)];
    SketchOp::Batch(vec![
        SketchOp::AddPolyline {
            points: corners.to_vec(),
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
        SketchOp::AddConstraints(rectangle_constraints(corners)),
    ])
    .apply(s)
    .unwrap();
    let ids = corners.map(|p| s.point_at(p, 1e-9).unwrap());
    let side = |i: usize| {
        let (p, q) = (ids[i], ids[(i + 1) % 4]);
        s.curves
            .keys()
            .find(|k| {
                s.curve_ends(*k)
                    .is_some_and(|(x, y)| (x, y) == (p, q) || (x, y) == (q, p))
            })
            .unwrap()
    };
    ([side(0), side(1), side(2), side(3)], ids)
}

fn cref(c: CurveId) -> CurveRef {
    CurveRef::Curve(c)
}

fn pref(p: PointId) -> PointRef {
    PointRef::Point(p)
}

fn add(s: &mut Sketch, c: Constraint) {
    SketchOp::AddConstraint {
        constraints: vec![c],
        label: "test",
    }
    .apply(s)
    .unwrap();
}

fn max_residual(s: &Sketch) -> f64 {
    let sys = System::new(s, &HashSet::new());
    let all: Vec<usize> = (0..sys.eqs.len()).collect();
    sys.max_residual(&all)
}

// ---------------------------------------------------------------------------------------------
// Residuals and Jacobians

/// Every equation's Jacobian matches central finite differences.
fn check_jacobian(s: &Sketch) {
    let sys = System::new(s, &HashSet::new());
    assert!(!sys.eqs.is_empty());
    let vars: Vec<usize> = (0..sys.x.len()).collect();
    let all: Vec<usize> = (0..sys.eqs.len()).collect();
    let j = sys.jacobian(&vars, &all);
    let h = 1e-6;
    for (row, eq) in sys.eqs.iter().enumerate() {
        for &c in &vars {
            let mut xp = sys.x.clone();
            let mut xm = sys.x.clone();
            xp[c] += h;
            xm[c] -= h;
            let fd = (eq.residual(&xp) - eq.residual(&xm)) / (2.0 * h);
            assert!(
                (fd - j[(row, c)]).abs() < 1e-5 * (1.0 + fd.abs()),
                "{:?}: d/dx{c} analytic {} vs finite difference {fd}",
                eq.kind,
                j[(row, c)]
            );
        }
    }
}

#[test]
fn jacobians_match_finite_differences() {
    // A deliberately unsolved sketch with every kind of equation.
    let mut s = Sketch::new();
    let (l1, a1, b1) = line(&mut s, v(0.3, 0.1), v(10.0, 2.0));
    let (l2, a2, _) = line(&mut s, v(-3.0, 5.0), v(4.0, 11.0));
    let c1 = circle(&mut s, v(20.0, 3.0), 4.0);
    let c2 = circle(&mut s, v(31.0, 5.0), 6.5);
    let ar = arc(&mut s, v(-10.0, -10.0), v(-4.0, -9.0), v(-12.0, -3.5));
    let ar2 = arc(&mut s, v(5.0, -20.0), v(9.0, -19.0), v(3.0, -15.0));
    let lone = s.add_point(v(7.0, 7.5));
    use ConstraintOf as C;
    let cs = [
        C::Coincident(pref(lone), PointRef::Origin),
        C::Coincident(pref(lone), pref(a2)),
        C::PointOnCurve(pref(lone), cref(l1)),
        C::PointOnCurve(pref(lone), CurveRef::XAxis),
        C::PointOnCurve(pref(lone), cref(c1)),
        C::PointOnCurve(pref(lone), cref(ar)),
        C::Midpoint(pref(lone), cref(l2)),
        C::Midpoint(pref(lone), cref(ar2)),
        C::Horizontal(Orient::Line(cref(l1))),
        C::Vertical(Orient::Points(pref(a1), pref(b1))),
        C::Parallel(cref(l1), cref(l2)),
        C::Parallel(cref(l1), CurveRef::YAxis),
        C::Perpendicular(cref(l1), cref(l2)),
        C::Tangent(cref(l1), cref(c1)),
        C::Tangent(cref(ar), cref(l2)),
        C::Tangent(cref(c1), cref(c2)),
        C::Tangent(cref(ar), cref(ar2)),
        C::Tangent(cref(c2), cref(ar2)),
        C::Equal(cref(l1), cref(l2)),
        C::Equal(cref(c1), cref(ar)),
        C::Equal(cref(ar2), cref(ar)),
        C::Concentric(cref(c1), cref(ar)),
        C::SymmetricPoints(pref(lone), pref(b1), cref(l2)),
        C::SymmetricCurves(cref(c1), cref(c2), cref(l2)),
        C::SymmetricCurves(cref(ar), cref(ar2), cref(l1)),
        C::SymmetricCurves(cref(l1), cref(l2), CurveRef::YAxis),
        C::EqualOffset(cref(l1), cref(l2), cref(c1), cref(ar)),
        C::EqualOffset(cref(ar2), cref(ar), cref(l2), cref(l1)),
    ];
    for c in cs {
        s.constraints.insert(c);
    }
    for kind in [
        DimensionKind::Horizontal { a: a1, b: b1 },
        DimensionKind::Vertical { a: a1, b: b1 },
        DimensionKind::Aligned { a: a1, b: a2 },
        DimensionKind::Diameter { curve: c2 },
        DimensionKind::Radius { curve: ar },
        DimensionKind::PointLine {
            p: pref(lone),
            line: cref(l1),
        },
        DimensionKind::PointLine {
            p: PointRef::Origin,
            line: cref(l2),
        },
        DimensionKind::PointLine {
            p: pref(a2),
            line: CurveRef::XAxis,
        },
        DimensionKind::Angle {
            a: cref(l1),
            b: cref(l2),
            flip_a: false,
            flip_b: false,
            ccw: None,
        },
        DimensionKind::Angle {
            a: cref(l1),
            b: cref(l2),
            flip_a: true,
            flip_b: false,
            ccw: None,
        },
        DimensionKind::Angle {
            a: cref(l2),
            b: CurveRef::YAxis,
            flip_a: false,
            flip_b: true,
            ccw: None,
        },
        DimensionKind::PointCircle {
            p: pref(lone),
            circle: c1,
            far: false,
        },
        DimensionKind::PointCircle {
            p: pref(a1),
            circle: ar,
            far: true,
        },
        DimensionKind::LineCircle {
            line: cref(l1),
            circle: c2,
            far: false,
        },
        DimensionKind::LineCircle {
            line: cref(l2),
            circle: ar2,
            far: true,
        },
        DimensionKind::CircleCircle {
            a: c1,
            b: ar2,
            far_a: false,
            far_b: true,
            axis: None,
        },
        DimensionKind::CircleCircle {
            a: ar,
            b: ar2,
            far_a: true,
            far_b: false,
            axis: None,
        },
        DimensionKind::Offset {
            source: l1,
            target: l2,
        },
        DimensionKind::Offset {
            source: ar,
            target: c2,
        },
    ] {
        s.dimensions.insert(Dimension {
            kind,
            value: 3.0,
            offset: 0.0,
            along: 0.0,
            driven: false,
        });
    }
    check_jacobian(&s);
    // Every constraint above made at least one equation.
    let sys = System::new(&s, &HashSet::new());
    for (k, _) in &s.constraints {
        assert!(sys.eqs.iter().any(|e| e.source == Source::Constraint(k)));
    }
    assert_eq!(
        sys.eqs.iter().filter(|e| e.kind == Kind::ArcEnd).count(),
        2,
        "one implicit equation per arc"
    );
}

#[test]
fn residuals_are_zero_on_satisfied_geometry() {
    let mut s = Sketch::new();
    let (l, ..) = line(&mut s, v(0.0, 3.0), v(10.0, 3.0));
    // Tangent to y = 3 and through the origin.
    let c = circle(&mut s, v(21f64.sqrt(), -2.0), 5.0);
    let a = arc(&mut s, v(0.0, 0.0), v(2.0, 0.0), v(0.0, 2.0));
    use ConstraintOf as C;
    s.constraints.insert(C::Horizontal(Orient::Line(cref(l))));
    s.constraints.insert(C::Tangent(cref(l), cref(c)));
    s.constraints.insert(C::Parallel(cref(l), CurveRef::XAxis));
    s.constraints
        .insert(C::Perpendicular(cref(l), CurveRef::YAxis));
    s.constraints
        .insert(C::PointOnCurve(PointRef::Origin, cref(c)));
    assert!(max_residual(&s) < 1e-12);
    let _ = a;
}

// ---------------------------------------------------------------------------------------------
// Solving

#[test]
fn constrained_rectangle_converges_to_its_dimensions() {
    let mut s = Sketch::new();
    let (_, p) = rect(&mut s, v(3.1, 2.2), v(49.1, 33.2));
    add(
        &mut s,
        ConstraintOf::Coincident(pref(p[0]), PointRef::Origin),
    );
    for (kind, value) in [
        (DimensionKind::Horizontal { a: p[0], b: p[1] }, 50.0),
        (DimensionKind::Vertical { a: p[1], b: p[2] }, 30.0),
    ] {
        SketchOp::SetDimension {
            dimension: Dimension {
                kind,
                value,
                offset: 5.0,
                along: 0.0,
                driven: false,
            },
            moves: vec![],
            radii: vec![],
        }
        .apply(&mut s)
        .unwrap();
    }
    assert!(s.pos(p[0]).distance(v(0.0, 0.0)) < 1e-7);
    assert!(
        s.pos(p[2]).distance(v(50.0, 30.0)) < 1e-7,
        "{:?}",
        s.pos(p[2])
    );
    let a = analyze(&s);
    assert_eq!(a.dof, 0);
    assert!(a.fully_constrained());
    assert!(s.curves.keys().all(|k| a.curve(k) == Status::Full));
    assert!(s.points.keys().all(|k| a.point(k) == Status::Full));
}

#[test]
fn slot_converges() {
    // Two parallel lines joined by two tangent arcs, lines equal, one arc's radius set.
    let mut s = Sketch::new();
    let (top, t0, t1) = line(&mut s, v(0.0, 5.3), v(20.4, 4.6));
    let (bottom, b0, b1) = line(&mut s, v(0.2, -4.8), v(19.5, -5.1));
    let (pb1, pt1, pt0, pb0) = (s.pos(b1), s.pos(t1), s.pos(t0), s.pos(b0));
    let right = arc(&mut s, v(20.0, 0.0), pb1, pt1);
    let left = arc(&mut s, v(0.0, 0.0), pt0, pb0);
    use ConstraintOf as C;
    for c in [
        C::Horizontal(Orient::Line(cref(top))),
        C::Tangent(cref(top), cref(right)),
        C::Tangent(cref(bottom), cref(right)),
        C::Tangent(cref(top), cref(left)),
        C::Tangent(cref(bottom), cref(left)),
        C::Equal(cref(left), cref(right)),
    ] {
        s.constraints.insert(c);
    }
    s.dimensions.insert(Dimension {
        kind: DimensionKind::Radius { curve: right },
        value: 5.0,
        offset: 0.0,
        along: 0.0,
        driven: false,
    });
    let report = solve(&mut s);
    assert!(report.conflicting.is_empty(), "{report:?}");
    assert!(max_residual(&s) < 1e-8);
    let (ga, gb) = (s.arc_geom(left).unwrap(), s.arc_geom(right).unwrap());
    assert!((ga.radius - 5.0).abs() < 1e-7 && (gb.radius - 5.0).abs() < 1e-7);
    // The bottom line ended up parallel to the top.
    let d = s.pos(b1) - s.pos(b0);
    assert!(d.y.abs() < 1e-6 * d.length());
}

#[test]
fn tangent_chain_converges() {
    // Line, tangent arc, line: a hairpin drawn roughly, then solved.
    let mut s = Sketch::new();
    let (l1, _, e1) = line(&mut s, v(0.0, 0.0), v(10.0, 0.3));
    let pe1 = s.pos(e1);
    let a = arc(&mut s, v(10.2, 4.8), pe1, v(10.0, 10.1));
    let end = s.pos(s.curve_ends(a).unwrap().1);
    let (l2, ..) = line(&mut s, end, v(-0.5, 9.7));
    use ConstraintOf as C;
    s.constraints.insert(C::Tangent(cref(l1), cref(a)));
    s.constraints.insert(C::Tangent(cref(l2), cref(a)));
    s.constraints.insert(C::Horizontal(Orient::Line(cref(l1))));
    let report = solve(&mut s);
    assert!(report.conflicting.is_empty());
    assert!(max_residual(&s) < 1e-8);
    let g = s.arc_geom(a).unwrap();
    // The first line is horizontal and tangent: the center is straight above its end (a
    // tangency residual of 1e-10 mm still lets the touching point slide by ~1e-4 mm).
    let e = s.pos(e1);
    assert!((g.center.x - e.x).abs() < 1e-3, "{:?} {:?}", g.center, e);
    let _ = l2;
}

// ---------------------------------------------------------------------------------------------
// Conflicts

#[test]
fn horizontal_and_vertical_on_one_line_conflict() {
    let mut s = Sketch::new();
    let (l, ..) = line(&mut s, v(0.0, 0.0), v(10.0, 1.0));
    add(&mut s, ConstraintOf::Horizontal(Orient::Line(cref(l))));
    let before = s.clone();
    add(&mut s, ConstraintOf::Vertical(Orient::Line(cref(l))));
    let a = analyze(&s);
    assert_eq!(a.conflicting.len(), 1);
    let vert = s
        .constraints
        .iter()
        .find(|(_, c)| matches!(c, ConstraintOf::Vertical(_)))
        .unwrap()
        .0;
    assert_eq!(
        a.conflicting,
        vec![vert],
        "the newer constraint is the one not solved"
    );
    assert_eq!(a.curve(l), Status::Over);
    // The geometry still satisfies the horizontal.
    for p in s.points.keys() {
        assert!(s.pos(p).distance(before.pos(p)) < 1e-9);
    }
    // Deleting the offending constraint: back to normal.
    SketchOp::Delete {
        curves: vec![],
        points: vec![],
        dimensions: vec![],
        constraints: vec![vert],
    }
    .apply(&mut s)
    .unwrap();
    let a = analyze(&s);
    assert!(!a.has_conflicts());
    assert_eq!(a.curve(l), Status::Under);
}

#[test]
fn fix_and_coincident_conflict() {
    let mut s = Sketch::new();
    let (_, a, _) = line(&mut s, v(5.0, 5.0), v(10.0, 7.0));
    add(&mut s, ConstraintOf::FixPoint(pref(a)));
    add(&mut s, ConstraintOf::Coincident(pref(a), PointRef::Origin));
    assert_eq!(s.pos(a), v(5.0, 5.0), "a fixed point does not move");
    let r = analyze(&s);
    assert!(r.has_conflicts());
    assert_eq!(r.point(a), Status::Over);
}

#[test]
fn redundant_constraints_are_consistent_not_conflicting() {
    let mut s = Sketch::new();
    let ([bottom, _, top, _], _) = rect(&mut s, v(0.0, 0.0), v(10.0, 5.0));
    // Bottom is already horizontal (parallel to the horizontal top).
    add(&mut s, ConstraintOf::Horizontal(Orient::Line(cref(bottom))));
    let a = analyze(&s);
    assert!(!a.has_conflicts());
    assert_eq!(a.redundant.len(), 1);
    assert_eq!(a.dof, 4);
    let _ = top;
}

// ---------------------------------------------------------------------------------------------
// Degrees of freedom and status

#[test]
fn dof_counts() {
    let mut s = Sketch::new();
    line(&mut s, v(0.0, 0.0), v(10.0, 3.0));
    assert_eq!(analyze(&s).dof, 4, "free line");
    let mut s = Sketch::new();
    circle(&mut s, v(0.0, 0.0), 3.0);
    assert_eq!(analyze(&s).dof, 3, "free circle");
    let mut s = Sketch::new();
    arc(&mut s, v(0.0, 0.0), v(3.0, 0.0), v(0.0, 3.0));
    assert_eq!(analyze(&s).dof, 5, "free arc");
    let mut s = Sketch::new();
    let (_, p) = rect(&mut s, v(2.0, 1.0), v(10.0, 5.0));
    assert_eq!(
        analyze(&s).dof,
        4,
        "rectangle with its automatic constraints"
    );
    add(
        &mut s,
        ConstraintOf::Coincident(pref(p[0]), PointRef::Origin),
    );
    assert_eq!(analyze(&s).dof, 2);
    let mut s = Sketch::new();
    let (l, ..) = line(&mut s, v(1.0, 1.0), v(10.0, 3.0));
    add(&mut s, ConstraintOf::FixCurve(cref(l)));
    assert_eq!(analyze(&s).dof, 0, "a fixed line");
}

/// A rectangle placed on the origin: the edges on the axes are black, the others blue
/// (`screens/12a`).
#[test]
fn status_matches_onshape_after_placing_a_rectangle() {
    let mut s = Sketch::new();
    let ([bottom, right, top, left], p) = rect(&mut s, v(0.0, 0.0), v(49.1, 33.2));
    add(
        &mut s,
        ConstraintOf::Coincident(pref(p[0]), PointRef::Origin),
    );
    let a = analyze(&s);
    assert_eq!(a.curve(bottom), Status::Full);
    assert_eq!(a.curve(left), Status::Full);
    assert_eq!(a.curve(top), Status::Under);
    assert_eq!(a.curve(right), Status::Under);
    assert_eq!(a.point(p[0]), Status::Full);
    assert_eq!(a.point(p[1]), Status::Under);
    assert_eq!(a.point(p[2]), Status::Under);
}

// ---------------------------------------------------------------------------------------------
// Dragging

#[test]
fn dragging_a_corner_moves_as_little_as_possible() {
    let mut s = Sketch::new();
    let ([_, _, top, _], p) = rect(&mut s, v(0.0, 0.0), v(20.0, 10.0));
    let target = v(25.0, 14.0);
    assert!(drag(
        &mut s,
        &Drag::Points(vec![(p[2], target)]),
        &HashSet::new()
    ));
    assert!(s.pos(p[2]).distance(target) < 1e-8);
    // The opposite corner stays; the neighbours only slide.
    assert!(
        s.pos(p[0]).distance(v(0.0, 0.0)) < 1e-8,
        "{:?}",
        s.pos(p[0])
    );
    assert!(
        s.pos(p[1]).distance(v(25.0, 0.0)) < 1e-6,
        "{:?}",
        s.pos(p[1])
    );
    assert!(
        s.pos(p[3]).distance(v(0.0, 14.0)) < 1e-6,
        "{:?}",
        s.pos(p[3])
    );
    // The top stays horizontal.
    let (a, b) = s.curve_ends(top).unwrap();
    assert!((s.pos(a).y - s.pos(b).y).abs() < 1e-9);
}

/// M8 regression (perf_500/02): dragging a rectangle's corner out past the opposite side and
/// back must not leave the rectangle flipped or stretched. Each frame solves from the sketch as
/// it was when the drag began, so the result depends only on where the cursor is.
#[test]
fn dragging_out_and_back_restores_the_rectangle() {
    let mut s = Sketch::new();
    let (_, p) = rect(&mut s, v(120.0, 100.0), v(134.0, 110.0));
    let base = s.clone();
    let mut live = s.clone();
    let skip = HashSet::new();
    // The path of perf_500's last drags: out past the bottom edge, around, and back.
    let path: Vec<Vec2> = (0..=20)
        .map(|i| {
            let t = i as f64 / 20.0;
            v(134.0 + 26.0 * t, 110.0 - 20.0 * t)
        })
        .chain((0..=20).map(|i| {
            let t = i as f64 / 20.0;
            v(160.0 - 26.0 * t, 90.0 + 20.0 * t)
        }))
        .collect();
    for target in &path {
        drag_from(
            &base,
            &mut live,
            &Drag::Points(vec![(p[2], *target)]),
            &skip,
        );
    }
    for (i, q) in p.iter().enumerate() {
        assert!(
            live.pos(*q).distance(base.pos(*q)) < 1e-6,
            "corner {i} moved: {:?} -> {:?}",
            base.pos(*q),
            live.pos(*q)
        );
    }
    // A drag that stays on the right side follows the cursor exactly.
    let mut live = base.clone();
    assert!(drag_from(
        &base,
        &mut live,
        &Drag::Points(vec![(p[2], v(150.0, 120.0))]),
        &skip
    ));
    assert!(live.pos(p[2]).distance(v(150.0, 120.0)) < 1e-8);
    assert!(live.pos(p[0]).distance(v(120.0, 100.0)) < 1e-8);
}

/// #38: a drag is not stopped where a segment's length would pass through zero. Pulling a
/// rectangle's corner past the opposite corner turns the rectangle inside out (its width and
/// height change sign), still a rectangle, with the corner at the cursor.
#[test]
fn dragging_a_corner_past_the_opposite_corner_flips_the_rectangle() {
    let mut s = Sketch::new();
    let ([bottom, right, top, left], p) = rect(&mut s, v(120.0, 100.0), v(134.0, 110.0));
    let base = s.clone();
    let skip = HashSet::new();
    // Frame by frame, as the pointer moves: through the opposite side and on past the corner.
    let mut live = base.clone();
    for i in 0..=30 {
        let t = i as f64 / 30.0;
        let target = v(134.0 - 26.0 * t, 110.0 - 20.0 * t);
        drag_from(&base, &mut live, &Drag::Points(vec![(p[2], target)]), &skip);
    }
    let target = v(108.0, 90.0);
    assert!(
        live.pos(p[2]).distance(target) < 1e-6,
        "{:?}",
        live.pos(p[2])
    );
    assert!(
        live.pos(p[0]).distance(v(120.0, 100.0)) < 1e-6,
        "{:?}",
        live.pos(p[0])
    );
    assert!(
        live.pos(p[1]).distance(v(108.0, 100.0)) < 1e-6,
        "{:?}",
        live.pos(p[1])
    );
    assert!(
        live.pos(p[3]).distance(v(120.0, 90.0)) < 1e-6,
        "{:?}",
        live.pos(p[3])
    );
    for l in [bottom, top] {
        let (a, b) = live.curve_ends(l).unwrap();
        assert!((live.pos(a).y - live.pos(b).y).abs() < 1e-9);
    }
    for l in [left, right] {
        let (a, b) = live.curve_ends(l).unwrap();
        assert!((live.pos(a).x - live.pos(b).x).abs() < 1e-9);
    }
    // The same target in one jump, straight from the start.
    let mut live = base.clone();
    assert!(drag_from(
        &base,
        &mut live,
        &Drag::Points(vec![(p[2], target)]),
        &skip
    ));
    assert!(
        live.pos(p[2]).distance(target) < 1e-6,
        "{:?}",
        live.pos(p[2])
    );
    assert!(
        live.pos(p[1]).distance(v(108.0, 100.0)) < 1e-6,
        "{:?}",
        live.pos(p[1])
    );
    assert!(
        live.pos(p[3]).distance(v(120.0, 90.0)) < 1e-6,
        "{:?}",
        live.pos(p[3])
    );
    // Across one side only: the width changes sign, the height stays.
    let mut live = base.clone();
    assert!(drag_from(
        &base,
        &mut live,
        &Drag::Points(vec![(p[2], v(110.0, 112.0))]),
        &skip
    ));
    assert!(
        live.pos(p[1]).distance(v(110.0, 100.0)) < 1e-6,
        "{:?}",
        live.pos(p[1])
    );
    assert!(
        live.pos(p[3]).distance(v(120.0, 112.0)) < 1e-6,
        "{:?}",
        live.pos(p[3])
    );
}

#[test]
fn dragging_a_constrained_point_projects() {
    // A point on a fixed horizontal line can only slide along it.
    let mut s = Sketch::new();
    let (l, a, b) = line(&mut s, v(0.0, 0.0), v(10.0, 0.0));
    add(&mut s, ConstraintOf::FixCurve(cref(l)));
    let (_, q, _) = line(&mut s, v(5.0, 0.0), v(5.0, 8.0));
    add(&mut s, ConstraintOf::PointOnCurve(pref(q), cref(l)));
    assert!(drag(
        &mut s,
        &Drag::Points(vec![(q, v(7.0, 3.0))]),
        &HashSet::new()
    ));
    assert!(s.pos(q).distance(v(7.0, 0.0)) < 1e-6, "{:?}", s.pos(q));
    // A fixed point does not move at all.
    let before = s.pos(a);
    drag(
        &mut s,
        &Drag::Points(vec![(a, v(-5.0, 4.0))]),
        &HashSet::new(),
    );
    assert_eq!(s.pos(a), before);
    let _ = b;
}

#[test]
fn dragging_a_circle_rim_changes_its_radius() {
    let mut s = Sketch::new();
    let c = circle(&mut s, v(0.0, 0.0), 5.0);
    assert!(drag(&mut s, &Drag::Rim(c, v(8.0, 0.0)), &HashSet::new()));
    let CurveKind::Circle { center, radius } = s.curves[c].kind else {
        unreachable!()
    };
    assert!(
        s.pos(center).distance(v(0.0, 0.0)) < 0.1,
        "{:?}",
        s.pos(center)
    );
    assert!((s.pos(center).distance(v(8.0, 0.0)) - radius).abs() < 1e-8);
    assert!(radius > 7.8);
}

#[test]
fn dragging_a_horizontal_line_keeps_it_horizontal() {
    let mut s = Sketch::new();
    let (l, a, b) = line(&mut s, v(0.0, 0.0), v(10.0, 0.0));
    add(&mut s, ConstraintOf::Horizontal(Orient::Line(cref(l))));
    assert!(drag(
        &mut s,
        &Drag::Points(vec![(b, v(12.0, 4.0))]),
        &HashSet::new()
    ));
    assert!((s.pos(a).y - s.pos(b).y).abs() < 1e-9);
    assert!(s.pos(b).distance(v(12.0, 4.0)) < 1e-8);
}

// ---------------------------------------------------------------------------------------------
// Constraint tools

#[test]
fn tools_fit_selections() {
    let mut s = Sketch::new();
    let ([bottom, right, top, left], p) = rect(&mut s, v(0.0, 0.0), v(20.0, 10.0));
    let c = circle(&mut s, v(40.0, 5.0), 3.0);
    let e = |x| SketchEntity::Curve(x);
    assert_eq!(
        fit(ConstraintKind::Horizontal, &s, &[e(bottom)]),
        Fit::Complete(vec![ConstraintOf::Horizontal(Orient::Line(cref(bottom)))])
    );
    assert_eq!(
        fit(ConstraintKind::Horizontal, &s, &[SketchEntity::Point(p[0])]),
        Fit::Partial
    );
    assert_eq!(fit(ConstraintKind::Parallel, &s, &[e(top)]), Fit::Partial);
    // Already there.
    assert_eq!(
        fit(ConstraintKind::Parallel, &s, &[e(top), e(bottom)]),
        Fit::Invalid
    );
    assert_eq!(
        fit(ConstraintKind::Perpendicular, &s, &[e(c), e(left)]),
        Fit::Invalid
    );
    assert!(matches!(
        fit(ConstraintKind::Tangent, &s, &[e(c), e(right)]),
        Fit::Complete(_)
    ));
    assert!(matches!(
        fit(
            ConstraintKind::Coincident,
            &s,
            &[SketchEntity::Point(p[0]), SketchEntity::Origin]
        ),
        Fit::Complete(_)
    ));
    assert!(matches!(
        fit(
            ConstraintKind::Midpoint,
            &s,
            &[e(bottom), SketchEntity::Origin]
        ),
        Fit::Complete(_)
    ));
    assert_eq!(
        fit(ConstraintKind::Fix, &s, &[SketchEntity::Origin]),
        Fit::Invalid
    );
}

#[test]
fn coincident_points_merge_once_solved() {
    let mut s = Sketch::new();
    let (_, _, b) = line(&mut s, v(0.0, 0.0), v(10.0, 0.0));
    let (_, c, _) = line(&mut s, v(12.0, 1.0), v(20.0, 5.0));
    add(&mut s, ConstraintOf::Coincident(pref(b), pref(c)));
    assert_eq!(s.points.len(), 3);
    assert_eq!(s.shared_points().len(), 1);
    assert!(s.constraints.is_empty());
    // Minimum movement: both ends met in the middle.
    let m = s.pos(s.shared_points()[0]);
    assert!(m.distance(v(11.0, 0.5)) < 1e-6, "{m:?}");
}

// ---------------------------------------------------------------------------------------------
// Performance

#[test]
fn five_hundred_entities_solve() {
    let mut s = Sketch::new();
    let mut corners = Vec::new();
    for i in 0..125 {
        let x = (i % 25) as f64 * 30.0;
        let y = (i / 25) as f64 * 30.0;
        let (_, p) = rect(&mut s, v(x, y), v(x + 20.0, y + 10.0));
        corners.push(p);
    }
    assert_eq!(s.curves.len(), 500);
    // Knock every rectangle out of shape, then solve.
    for p in &corners {
        let q = s.pos(p[2]);
        s.points[p[2]].pos = q + v(0.7, -0.4);
    }
    let t = std::time::Instant::now();
    let report = solve(&mut s);
    let solve_time = t.elapsed();
    assert!(report.conflicting.is_empty());
    assert!(max_residual(&s) < 1e-8);
    let t = std::time::Instant::now();
    let a = analyze(&s);
    let analyze_time = t.elapsed();
    assert_eq!(a.dof, 500);
    eprintln!("500 lines: solve {solve_time:?}, analyze {analyze_time:?}");
    // Generous, so debug builds pass too; release runs in a few milliseconds.
    assert!(solve_time.as_secs_f64() < 5.0);
}

// ---------------------------------------------------------------------------------------------
// Dimensions (M7)

/// A rectangle from the origin, like the rectangle tool draws it.
fn origin_rect(s: &mut Sketch) -> ([CurveId; 4], [PointId; 4]) {
    let (sides, p) = rect(s, v(0.0, 0.0), v(49.1, 33.2));
    add(s, ConstraintOf::Coincident(pref(p[0]), PointRef::Origin));
    (sides, p)
}

fn dim(s: &mut Sketch, kind: DimensionKind, value: f64) {
    SketchOp::SetDimension {
        dimension: Dimension::new(kind, value, 5.0),
        moves: vec![],
        radii: vec![],
    }
    .apply(s)
    .unwrap();
}

fn measured(s: &Sketch, kind: DimensionKind) -> f64 {
    crate::dimension::measure(s, kind).unwrap()
}

#[test]
fn new_dimension_residuals_hold_on_solved_geometry() {
    let mut s = Sketch::new();
    let (l1, ..) = line(&mut s, v(0.0, 0.0), v(10.0, 0.0));
    let (l2, ..) = line(&mut s, v(0.0, 0.0), v(5.0, 5.0));
    let p = s.add_point(v(3.0, 7.0));
    let pl = DimensionKind::PointLine {
        p: pref(p),
        line: cref(l1),
    };
    assert!((measured(&s, pl) - 7.0).abs() < 1e-12);
    let py = DimensionKind::PointLine {
        p: pref(p),
        line: CurveRef::YAxis,
    };
    assert!((measured(&s, py) - 3.0).abs() < 1e-12);
    let ang = |fa, fb| DimensionKind::Angle {
        a: cref(l1),
        b: cref(l2),
        flip_a: fa,
        flip_b: fb,
        ccw: None,
    };
    assert!((measured(&s, ang(false, false)) - 45.0).abs() < 1e-9);
    assert!((measured(&s, ang(true, false)) - 135.0).abs() < 1e-9);
    assert!((measured(&s, ang(true, true)) - 45.0).abs() < 1e-9);
    // The solver's residuals agree with the measured values.
    s.dimensions.insert(Dimension::new(pl, 7.0, 1.0));
    s.dimensions
        .insert(Dimension::new(ang(true, false), 135.0, 1.0));
    assert!(max_residual(&s) < 1e-9);
}

#[test]
fn angle_and_point_line_dimensions_drive_geometry() {
    let mut s = Sketch::new();
    let (l1, ..) = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    let (l2, ..) = line(&mut s, v(0.0, 0.0), v(10.0, 12.0));
    add(&mut s, ConstraintOf::FixCurve(cref(l1)));
    let ang = DimensionKind::Angle {
        a: cref(l1),
        b: cref(l2),
        flip_a: false,
        flip_b: false,
        ccw: None,
    };
    dim(&mut s, ang, 30.0);
    assert!((measured(&s, ang) - 30.0).abs() < 1e-6);
    // A point held 8 mm from the line.
    let p = s.add_point(v(5.0, 3.0));
    dim(
        &mut s,
        DimensionKind::PointLine {
            p: pref(p),
            line: cref(l1),
        },
        8.0,
    );
    assert!((s.pos(p).y.abs() - 8.0).abs() < 1e-6, "{:?}", s.pos(p));
    assert!(conflicts(&s).is_empty());
}

#[test]
fn width_and_height_fully_constrain_a_rectangle_from_the_origin() {
    let mut s = Sketch::new();
    let (sides, p) = origin_rect(&mut s);
    assert_eq!(analyze(&s).dof, 2);
    // Width (the bottom line's length) and height (the right line's length), as the Dimension
    // tool makes them.
    let (b0, b1) = s.curve_ends(sides[0]).unwrap();
    dim(&mut s, DimensionKind::Aligned { a: b0, b: b1 }, 50.0);
    let (r0, r1) = s.curve_ends(sides[1]).unwrap();
    dim(&mut s, DimensionKind::Aligned { a: r0, b: r1 }, 30.0);
    assert!(
        s.pos(p[2]).distance(v(50.0, 30.0)) < 1e-6,
        "{:?}",
        s.pos(p[2])
    );
    let a = analyze(&s);
    assert_eq!(a.dof, 0);
    assert!(a.fully_constrained());
    assert!(s.curves.keys().all(|k| a.curve(k) == Status::Full));
    assert!(s.points.keys().all(|k| a.point(k) == Status::Full));
}

#[test]
fn an_extra_dimension_conflicts() {
    let mut s = Sketch::new();
    let (_, p) = origin_rect(&mut s);
    dim(&mut s, DimensionKind::Horizontal { a: p[0], b: p[1] }, 50.0);
    dim(&mut s, DimensionKind::Vertical { a: p[1], b: p[2] }, 30.0);
    // The diagonal at its current length over-defines the sketch: shown as conflicting.
    let diag = 50f64.hypot(30.0);
    dim(&mut s, DimensionKind::Aligned { a: p[0], b: p[2] }, diag);
    let id = s
        .dimensions
        .iter()
        .find(|(_, d)| matches!(d.kind, DimensionKind::Aligned { .. }))
        .unwrap()
        .0;
    let a = analyze(&s);
    assert_eq!(a.conflicting_dimensions, vec![id]);
    assert!(a.has_conflicts());
    assert_eq!(a.point(p[2]), Status::Over);
    // With another value it cannot hold: it is left unsolved and the rectangle keeps its size.
    SketchOp::SetDimensionValue { id, value: 70.0 }
        .apply(&mut s)
        .unwrap();
    assert_eq!(conflicts(&s), vec![Source::Dimension(id)]);
    assert!(s.pos(p[2]).distance(v(50.0, 30.0)) < 1e-6);
    // Deleting it recovers.
    SketchOp::Delete {
        curves: vec![],
        points: vec![],
        dimensions: vec![id],
        constraints: vec![],
    }
    .apply(&mut s)
    .unwrap();
    let a = analyze(&s);
    assert!(!a.has_conflicts());
    assert!(a.fully_constrained());
}

#[test]
fn editing_a_dimension_moves_the_geometry() {
    let mut s = Sketch::new();
    let (_, p) = origin_rect(&mut s);
    dim(&mut s, DimensionKind::Horizontal { a: p[0], b: p[1] }, 50.0);
    let id = s.dimensions.keys().next().unwrap();
    SketchOp::SetDimensionValue { id, value: 40.0 }
        .apply(&mut s)
        .unwrap();
    assert!((s.pos(p[1]).x - 40.0).abs() < 1e-6);
    // Invalid values are refused (a negative distance is the other side, #96, but 0 is none).
    assert!(
        SketchOp::SetDimensionValue { id, value: 0.0 }
            .apply(&mut s)
            .is_err()
    );
    // Moving the label changes nothing else.
    let before = s.clone();
    SketchOp::MoveDimensionLabel {
        id,
        offset: -12.0,
        along: 7.0,
    }
    .apply(&mut s)
    .unwrap();
    assert_eq!(s.dimensions[id].offset, -12.0);
    assert_eq!(s.dimensions[id].along, 7.0);
    assert!(
        s.points
            .iter()
            .all(|(k, pt)| pt.pos == before.points[k].pos)
    );
}

#[test]
fn over_defining_dimensions_are_created_driven() {
    let mut s = Sketch::new();
    let (_, p) = origin_rect(&mut s);
    let diag = |s: &Sketch| {
        Dimension::new(
            DimensionKind::Aligned { a: p[0], b: p[2] },
            s.pos(p[0]).distance(s.pos(p[2])),
            5.0,
        )
    };
    // Under-defined: the diagonal is fine as a driving dimension.
    assert!(!crate::dimension::over_defines(&s, &diag(&s)));
    dim(&mut s, DimensionKind::Horizontal { a: p[0], b: p[1] }, 50.0);
    dim(&mut s, DimensionKind::Vertical { a: p[1], b: p[2] }, 30.0);
    let d = diag(&s);
    assert!(crate::dimension::over_defines(&s, &d));
    // Driven: measured, not solved, no conflict, not editable.
    let id = s.dimensions.insert(Dimension {
        driven: true,
        value: 1.0,
        ..d
    });
    assert!(!analyze(&s).has_conflicts());
    assert!(analyze(&s).fully_constrained());
    assert!(
        SketchOp::SetDimensionValue { id, value: 70.0 }
            .apply(&mut s)
            .is_err()
    );
    // Made driving, it over-defines the sketch: conflicting (red), with its measured value.
    SketchOp::SetDimensionDriven { id, driven: false }
        .apply(&mut s)
        .unwrap();
    assert!((s.dimensions[id].value - 50f64.hypot(30.0)).abs() < 1e-9);
    let a = analyze(&s);
    assert_eq!(a.conflicting_dimensions, vec![id]);
    // So are the width and height it conflicts with (the whole conflicting set).
    assert_eq!(
        a.involved_dimensions.len(),
        2,
        "{:?}",
        a.involved_dimensions
    );
    // The edges at its points are red too.
    let red = s
        .curves
        .keys()
        .filter(|k| a.curve(*k) == Status::Over)
        .count();
    assert_eq!(red, 4);
}

// ---------------------------------------------------------------------------------------------
// Bézier curves and Curvature (Final re-audit, S12.14)

fn bezier(s: &mut Sketch, p: [Vec2; 4]) -> CurveId {
    SketchOp::AddBezier {
        points: p,
        construction: false,
    }
    .apply(s)
    .unwrap();
    s.curves
        .iter()
        .find(|(_, c)| matches!(c.kind, CurveKind::Bezier { a, b, .. } if s.pos(a).distance(p[0]) < 1e-9 && s.pos(b).distance(p[3]) < 1e-9))
        .map(|(k, _)| k)
        .unwrap()
}

#[test]
fn curvature_joins_two_beziers_g2() {
    let mut s = Sketch::new();
    let a = bezier(
        &mut s,
        [v(0.0, 0.0), v(10.0, 0.0), v(20.0, 5.0), v(30.0, 10.0)],
    );
    // Drawn from the first's end, bending the other way and not tangent.
    let b = bezier(
        &mut s,
        [v(30.0, 10.0), v(40.0, 20.0), v(50.0, 18.0), v(60.0, 10.0)],
    );
    let sel = [SketchEntity::Curve(a), SketchEntity::Curve(b)];
    let Fit::Complete(cs) = fit(ConstraintKind::Curvature, &s, &sel) else {
        panic!("Curvature fits two joined Béziers")
    };
    SketchOp::AddConstraint {
        constraints: cs,
        label: "Add curvature",
    }
    .apply(&mut s)
    .unwrap();
    assert!(conflicts(&s).is_empty());
    let (ga, gb) = (s.bezier_geom(a).unwrap(), s.bezier_geom(b).unwrap());
    // One point, one tangent, one curvature where they meet.
    assert!(ga.point_at(1.0).distance(gb.point_at(0.0)) < 1e-9);
    let (ta, tb) = (
        ga.tangent_at(1.0).normalize(),
        gb.tangent_at(0.0).normalize(),
    );
    assert!(
        ta.cross(tb).abs() < 1e-7 && ta.dot(tb) > 0.0,
        "{ta:?} {tb:?}"
    );
    assert!(
        (ga.curvature_at(1.0) - gb.curvature_at(0.0)).abs() < 1e-7,
        "{} {}",
        ga.curvature_at(1.0),
        gb.curvature_at(0.0)
    );
    // The analysis counts it: two equations (G1, G2) off the 16 point coordinates (the shared
    // end counted once: 14).
    assert_eq!(analyze(&s).dof, 14 - 2);
}

#[test]
fn curvature_of_a_bezier_and_an_arc_is_one_over_r() {
    let mut s = Sketch::new();
    // A quarter arc of radius 10 counter-clockwise from (10, 0) to (0, 10); a Bézier leaves
    // its start going down, roughly tangent.
    let r = arc(&mut s, v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0));
    let b = bezier(
        &mut s,
        [v(10.0, 0.0), v(10.5, -6.0), v(14.0, -12.0), v(20.0, -15.0)],
    );
    let sel = [SketchEntity::Curve(b), SketchEntity::Curve(r)];
    let Fit::Complete(cs) = fit(ConstraintKind::Curvature, &s, &sel) else {
        panic!()
    };
    SketchOp::AddConstraint {
        constraints: cs,
        label: "Add curvature",
    }
    .apply(&mut s)
    .unwrap();
    assert!(conflicts(&s).is_empty());
    let g = s.bezier_geom(b).unwrap();
    let arc = s.arc_geom(r).unwrap();
    // Run through the joint: the arc backwards (clockwise) into its start, then the Bézier.
    let t = g.tangent_at(0.0).normalize();
    assert!(t.cross(-arc.start_tangent()).abs() < 1e-7);
    assert!(
        (g.curvature_at(0.0) - (-1.0 / arc.radius)).abs() < 1e-7,
        "{} vs {}",
        g.curvature_at(0.0),
        -1.0 / arc.radius
    );
}

#[test]
fn tangent_and_curvature_need_a_shared_end() {
    let mut s = Sketch::new();
    let a = bezier(
        &mut s,
        [v(0.0, 0.0), v(10.0, 0.0), v(20.0, 5.0), v(30.0, 10.0)],
    );
    let (l, ..) = line(&mut s, v(40.0, 0.0), v(50.0, 0.0));
    let sel = [SketchEntity::Curve(a), SketchEntity::Curve(l)];
    assert_eq!(fit(ConstraintKind::Curvature, &s, &sel), Fit::Invalid);
    assert_eq!(fit(ConstraintKind::Tangent, &s, &sel), Fit::Invalid);
    // Joined to a line: G2 to a line flattens the Bézier's end (curvature 0).
    let (m, ..) = line(&mut s, v(30.0, 10.0), v(40.0, 30.0));
    let sel = [SketchEntity::Curve(a), SketchEntity::Curve(m)];
    let Fit::Complete(cs) = fit(ConstraintKind::Curvature, &s, &sel) else {
        panic!()
    };
    SketchOp::AddConstraint {
        constraints: cs,
        label: "Add curvature",
    }
    .apply(&mut s)
    .unwrap();
    assert!(conflicts(&s).is_empty());
    let g = s.bezier_geom(a).unwrap();
    assert!(g.curvature_at(1.0).abs() < 1e-7);
    // Two lines never take Curvature.
    let sel = [SketchEntity::Curve(l), SketchEntity::Curve(m)];
    assert_eq!(fit(ConstraintKind::Curvature, &s, &sel), Fit::Invalid);
}

#[test]
fn a_bezier_closes_a_region_with_its_exact_area() {
    // A Bézier from (0,0) up over to (20,0), closed by a line. With control points (0,0),
    // (0,10), (20,10), (20,0): y = 30 t(1 − t), x = 60t² − 40t³, so the area ∫ y dx =
    // 3600 ∫ t²(1 − t)² dt = 3600 / 30 = 120 exactly.
    let mut s = Sketch::new();
    let b = bezier(
        &mut s,
        [v(0.0, 0.0), v(0.0, 10.0), v(20.0, 10.0), v(20.0, 0.0)],
    );
    line(&mut s, v(20.0, 0.0), v(0.0, 0.0));
    let regions = crate::region::regions(&s);
    assert_eq!(regions.len(), 1);
    let g = s.bezier_geom(b).unwrap();
    let n = 20000;
    let pts: Vec<Vec2> = (0..=n).map(|i| g.point_at(i as f64 / n as f64)).collect();
    let poly = crate::geom::polygon_area(&pts).abs();
    assert!(
        (regions[0].area() - poly).abs() < 1e-5,
        "{} vs {}",
        regions[0].area(),
        poly
    );
    assert!(
        (regions[0].area() - 120.0).abs() < 1e-9,
        "{}",
        regions[0].area()
    );
}

#[test]
fn a_bezier_crossing_a_line_splits_regions() {
    let mut s = Sketch::new();
    rect(&mut s, v(0.0, 0.0), v(40.0, 20.0));
    // Across the rectangle, from its bottom side to its top side.
    bezier(
        &mut s,
        [v(10.0, -5.0), v(15.0, 10.0), v(25.0, 10.0), v(30.0, 25.0)],
    );
    let regions = crate::region::regions(&s);
    assert_eq!(regions.len(), 2, "{}", regions.len());
    let total: f64 = regions.iter().map(|r| r.area()).sum();
    assert!((total - 800.0).abs() < 1e-6, "{total}");
}

// ---------------------------------------------------------------------------------------------
// Ellipses (Final re-audit, S8): tangent lines and crossings

#[test]
fn a_line_tangent_to_an_ellipse() {
    let mut s = Sketch::new();
    SketchOp::AddEllipse {
        center: v(0.0, 0.0),
        major: v(30.0, 0.0),
        minor: 10.0,
        construction: false,
    }
    .apply(&mut s)
    .unwrap();
    let e = s.curves.keys().next().unwrap();
    // A line above it, a little tilted, not touching.
    let (l, ..) = line(&mut s, v(-40.0, 14.0), v(40.0, 18.0));
    let sel = [SketchEntity::Curve(l), SketchEntity::Curve(e)];
    let Fit::Complete(cs) = fit(ConstraintKind::Tangent, &s, &sel) else {
        panic!("a line and an ellipse take Tangent")
    };
    SketchOp::AddConstraint {
        constraints: cs,
        label: "Add tangent",
    }
    .apply(&mut s)
    .unwrap();
    assert!(conflicts(&s).is_empty());
    let g = s.ellipse_geom(e).unwrap();
    let (a, b) = s.curve_ends(l).unwrap();
    let (pa, pb) = (s.pos(a), s.pos(b));
    // Tangent: the line's nearest approach to the ellipse is 0, and it doesn't cross it (the
    // implicit value is ≥ 0 along the line).
    let n = 4000;
    let mut min_d = f64::INFINITY;
    let mut min_imp = f64::INFINITY;
    for i in 0..=n {
        let p = pa.lerp(pb, i as f64 / n as f64);
        min_d = min_d.min(g.distance(p));
        min_imp = min_imp.min(g.implicit(p));
    }
    assert!(min_d < 1e-4, "{min_d}");
    assert!(min_imp > -1e-6, "{min_imp}");
}

#[test]
fn inference_finds_where_an_ellipse_crosses_a_line() {
    use crate::infer::{Shape, intersect};
    let g = crate::geom::EllipseGeom::new(v(0.0, 0.0), v(30.0, 0.0), 10.0);
    let seg = Shape::Segment(v(-50.0, 5.0), v(50.0, 5.0));
    let mut pts = intersect(&Shape::Ellipse(g), &seg);
    pts.sort_by(|p, q| p.x.total_cmp(&q.x));
    // x = ±30·√(1 − 0.25).
    let x = 30.0 * 0.75f64.sqrt();
    assert_eq!(pts.len(), 2);
    assert!(
        pts[0].distance(v(-x, 5.0)) < 1e-9 && pts[1].distance(v(x, 5.0)) < 1e-9,
        "{pts:?}"
    );
    // Only the existing part of a segment counts; and a circle crossing it.
    assert_eq!(intersect(&seg, &Shape::Ellipse(g)).len(), 2);
    assert_eq!(
        intersect(
            &Shape::Segment(v(0.0, 5.0), v(50.0, 5.0)),
            &Shape::Ellipse(g)
        )
        .len(),
        1
    );
    assert_eq!(
        intersect(
            &Shape::Ellipse(g),
            &Shape::Circle {
                center: v(30.0, 0.0),
                radius: 5.0
            }
        )
        .len(),
        2
    );
}

/// Ex2 step 2 (`intro-to-sketching/ex2-step2.png`): lines from the origin with H or V are black
/// (they can't leave their own line), even alone with a free end; the side line joined to the
/// base's free end stays blue (it can still slide along x).
#[test]
fn lines_from_the_origin_with_h_or_v_are_black() {
    for (dx, dy) in [(60.0, 0.0), (-60.0, 0.0), (0.0, 180.0), (0.0, -180.0)] {
        let mut u = Sketch::new();
        let (l, p0, _) = line(&mut u, v(0.0, 0.0), v(dx, dy));
        add(&mut u, ConstraintOf::Coincident(pref(p0), PointRef::Origin));
        let o = Orient::Line(cref(l));
        add(
            &mut u,
            if dx == 0.0 {
                ConstraintOf::Vertical(o)
            } else {
                ConstraintOf::Horizontal(o)
            },
        );
        assert_eq!(analyze(&u).curve(l), Status::Full, "({dx}, {dy})");
    }
    let mut s = Sketch::new();
    let (c, c0, _) = line(&mut s, v(0.0, 0.0), v(0.0, 180.0));
    add(&mut s, ConstraintOf::Coincident(pref(c0), PointRef::Origin));
    add(&mut s, ConstraintOf::Vertical(Orient::Line(cref(c))));
    // The base starts on the centre line's (shared) start point.
    let (b, ..) = line(&mut s, v(0.0, 0.0), v(-60.0, 0.0));
    add(&mut s, ConstraintOf::Horizontal(Orient::Line(cref(b))));
    let (side, ..) = line(&mut s, v(-60.0, 0.0), v(-60.0, 45.0));
    add(&mut s, ConstraintOf::Vertical(Orient::Line(cref(side))));
    let a = analyze(&s);
    assert_eq!(
        (a.curve(c), a.curve(b), a.curve(side)),
        (Status::Full, Status::Full, Status::Under)
    );
}

/// #96: a negative distance puts its point on the other side, and a positive one back; the
/// value keeps its sign while the solver holds the size.
#[test]
fn a_negative_distance_flips_its_point() {
    for kind in ["horizontal", "vertical", "aligned"] {
        let mut s = Sketch::new();
        let (_, a, b) = line(&mut s, v(5.0, 5.0), v(25.0, 15.0));
        add(&mut s, ConstraintOf::FixPoint(pref(a)));
        let k = match kind {
            "horizontal" => DimensionKind::Horizontal { a, b },
            "vertical" => DimensionKind::Vertical { a, b },
            _ => DimensionKind::Aligned { a, b },
        };
        dim(&mut s, k, 20.0);
        let id = s.dimensions.keys().next().unwrap();
        let side = |s: &Sketch| {
            let d = s.pos(b) - s.pos(a);
            match kind {
                "horizontal" => d.x,
                "vertical" => d.y,
                _ => d.x.signum() * d.length(),
            }
        };
        assert!(side(&s) > 0.0, "{kind}");
        SketchOp::SetDimensionValue { id, value: -12.0 }
            .apply(&mut s)
            .unwrap();
        assert_eq!(s.dimensions[id].value, -12.0);
        assert!((side(&s) + 12.0).abs() < 1e-6, "{kind}: {}", side(&s));
        assert!((measured(&s, k) - 12.0).abs() < 1e-6, "{kind}");
        assert!(!analyze(&s).has_conflicts(), "{kind}");
        // Larger, still on that side (no flip back), then positive again: back across.
        SketchOp::SetDimensionValue { id, value: -18.0 }
            .apply(&mut s)
            .unwrap();
        assert!((side(&s) + 18.0).abs() < 1e-6, "{kind}: {}", side(&s));
        SketchOp::SetDimensionValue { id, value: 7.0 }
            .apply(&mut s)
            .unwrap();
        assert!((side(&s) - 7.0).abs() < 1e-6, "{kind}: {}", side(&s));
    }
}

/// #96: a point-to-line distance turning negative puts the point across the line.
#[test]
fn a_negative_point_line_distance_crosses_the_line() {
    let mut s = Sketch::new();
    let (l, a, b) = line(&mut s, v(0.0, 0.0), v(40.0, 0.0));
    add(&mut s, ConstraintOf::FixPoint(pref(a)));
    add(&mut s, ConstraintOf::FixPoint(pref(b)));
    let p = s.add_point(v(10.0, 8.0));
    let k = DimensionKind::PointLine {
        p: pref(p),
        line: cref(l),
    };
    dim(&mut s, k, 8.0);
    let id = s.dimensions.keys().next().unwrap();
    SketchOp::SetDimensionValue { id, value: -5.0 }
        .apply(&mut s)
        .unwrap();
    assert!((s.pos(p).y + 5.0).abs() < 1e-6, "{:?}", s.pos(p));
}

/// #96: an angle dimension turns its line like a hinge: on round the way it turns, smoothly
/// through 180° and 360° and back below 0° (any value goes), past 180° with its arc and value
/// round the outside; its label dragged back between the rays measures the same rays the other
/// way round.
#[test]
fn an_angle_turns_its_line_like_a_hinge() {
    let mut s = Sketch::new();
    let (la, a0, _) = line(&mut s, v(0.0, 0.0), v(30.0, 0.0));
    let (lb, b0, _) = line(&mut s, v(0.0, 0.0), v(20.0, 20.0));
    assert_eq!(a0, b0, "the lines share their corner");
    add(&mut s, ConstraintOf::FixPoint(pref(a0)));
    add(&mut s, ConstraintOf::Horizontal(Orient::Line(cref(la))));
    let k = DimensionKind::Angle {
        a: cref(la),
        b: cref(lb),
        flip_a: false,
        flip_b: false,
        ccw: None,
    };
    dim(&mut s, k, 45.0);
    let id = s.dimensions.keys().next().unwrap();
    // Which way the second line points, and where `deg` (anticlockwise from +X) points.
    let dir = |s: &Sketch| (s.pos(lb_end(s, lb)) - v(0.0, 0.0)).normalize();
    let at = |deg: f64| v(deg.to_radians().cos(), deg.to_radians().sin());
    let set = |s: &mut Sketch, value: f64| {
        SketchOp::SetDimensionValue { id, value }.apply(s).unwrap();
        assert!(
            (dir(s) - at(value)).length() < 1e-6,
            "{value}: {:?}",
            dir(s)
        );
        assert!(!analyze(s).has_conflicts(), "{value}");
        let m = crate::dimension::measured_value(s, &s.dimensions[id]).unwrap();
        assert!((m - value).abs() < 1e-6, "{value}: measured {m}");
        // Swung round, not stretched.
        let l = s.line_length(lb).unwrap();
        assert!((l - 20.0 * 2f64.sqrt()).abs() < 1e-6, "{value}: length {l}");
    };
    // Two turns round in 10° steps, as a variable driving it would, then back past 0°.
    for i in 0..=72 {
        set(&mut s, 45.0 + i as f64 * 10.0);
    }
    for value in [180.0, 0.0, -30.0, -200.0, 270.0, 90.0, 400.0] {
        set(&mut s, value);
    }
    // 270°: a quarter turn the other side of the first line, the arc and value outside.
    set(&mut s, 270.0);
    assert!((dir(&s) - v(0.0, -1.0)).length() < 1e-6);
    let st = crate::dimension::LayoutStyle::new(10.0, (12.0, 6.0));
    let lay = crate::dimension::layout(&s, &s.dimensions[id], st).unwrap();
    assert!(!lay.arcs.is_empty());
    for g in &lay.arcs {
        let mid = g.point_at(g.start_angle + g.sweep / 2.0);
        assert_eq!(
            crate::dimension::angle_outside(&s, k, mid),
            Some(true),
            "{g:?}"
        );
    }
    assert_eq!(
        crate::dimension::angle_outside(&s, k, lay.label),
        Some(true)
    );
    // Measured between the rays again: the geometry stays, 90° the other way round (clockwise),
    // which it now turns.
    let before = s.clone();
    SketchOp::SetAngleOutside { id, outside: false }
        .apply(&mut s)
        .unwrap();
    assert!((s.dimensions[id].value - 90.0).abs() < 1e-9);
    assert!(
        s.pos(lb_end(&s, lb))
            .distance(before.pos(lb_end(&before, lb)))
            < 1e-6
    );
    assert_eq!(
        crate::dimension::angle_ccw(&s, s.dimensions[id].kind),
        Some(false)
    );
    SketchOp::SetDimensionValue { id, value: 180.0 }
        .apply(&mut s)
        .unwrap();
    assert!((dir(&s) - v(-1.0, 0.0)).length() < 1e-6, "{:?}", dir(&s));
    // A label outside the rays asks for the outside, one between them for the inside.
    SketchOp::SetDimensionValue { id, value: 60.0 }
        .apply(&mut s)
        .unwrap();
    let x = v(0.0, 0.0);
    let inside = x + (v(1.0, 0.0) + dir(&s)) * 5.0;
    assert_eq!(crate::dimension::angle_outside(&s, k, inside), Some(false));
    assert_eq!(
        crate::dimension::angle_outside(&s, k, x - (inside - x)),
        Some(true)
    );
}

fn lb_end(s: &Sketch, l: CurveId) -> PointId {
    s.curve_ends(l).unwrap().1
}

/// #96: making a dimension driving again keeps the side it measured: a negative distance
/// stays negative (an angle outside: `driven_and_expression_angles_keep_their_side`).
#[test]
fn driving_again_keeps_the_side() {
    let mut s = Sketch::new();
    let (_, a, b) = line(&mut s, v(0.0, 0.0), v(10.0, 0.0));
    add(&mut s, ConstraintOf::FixPoint(pref(a)));
    let k = DimensionKind::Horizontal { a, b };
    dim(&mut s, k, 10.0);
    let id = s.dimensions.keys().next().unwrap();
    SketchOp::SetDimensionValue { id, value: -10.0 }
        .apply(&mut s)
        .unwrap();
    for driven in [true, false] {
        SketchOp::SetDimensionDriven { id, driven }
            .apply(&mut s)
            .unwrap();
    }
    assert!((s.dimensions[id].value + 10.0).abs() < 1e-6);
    assert!((s.pos(b).x + 10.0).abs() < 1e-6);
}

/// #96: a negative distance to a plane axis puts the point across the axis; a negative first
/// dimension (which scales the sketch) flips and scales by its size.
#[test]
fn negative_to_an_axis_and_a_negative_first_dimension() {
    let mut s = Sketch::new();
    let p = s.add_point(v(10.0, 8.0));
    let k = DimensionKind::PointLine {
        p: pref(p),
        line: CurveRef::XAxis,
    };
    dim(&mut s, k, 8.0);
    let id = s.dimensions.keys().next().unwrap();
    SketchOp::SetDimensionValue { id, value: -5.0 }
        .apply(&mut s)
        .unwrap();
    assert!((s.pos(p).y + 5.0).abs() < 1e-6, "{:?}", s.pos(p));
    // A free line's first dimension typed -40: scaled to 40 long, its end on the other side.
    let mut s = Sketch::new();
    let (_, a, b) = line(&mut s, v(5.0, 0.0), v(25.0, 0.0));
    let k = DimensionKind::Horizontal { a, b };
    dim(&mut s, k, 20.0);
    let id = s.dimensions.keys().next().unwrap();
    crate::edit::set_value_op(&s, id, -40.0, true)
        .apply(&mut s)
        .unwrap();
    assert_eq!(s.dimensions[id].value, -40.0);
    assert!(
        ((s.pos(b) - s.pos(a)).x + 40.0).abs() < 1e-6,
        "{:?}",
        s.pos(b)
    );
    // A dimension made with a negative value puts its point on the other side at once.
    let mut s = Sketch::new();
    let (_, a, b) = line(&mut s, v(0.0, 0.0), v(30.0, 0.0));
    add(&mut s, ConstraintOf::FixPoint(pref(a)));
    dim(&mut s, DimensionKind::Aligned { a, b }, -30.0);
    assert!((s.pos(b).x + 30.0).abs() < 1e-6, "{:?}", s.pos(b));
}

/// #96: a driven dimension shows its value on its side, and an angle a variable sets keeps its
/// side (its expression would set it back).
#[test]
fn driven_and_expression_angles_keep_their_side() {
    let mut s = Sketch::new();
    let (la, _, _) = line(&mut s, v(0.0, 0.0), v(30.0, 0.0));
    let (lb, _, _) = line(&mut s, v(0.0, 0.0), v(0.0, 20.0));
    let k = DimensionKind::Angle {
        a: cref(la),
        b: cref(lb),
        flip_a: false,
        flip_b: false,
        ccw: None,
    };
    dim(&mut s, k, 270.0);
    let id = s.dimensions.keys().next().unwrap();
    SketchOp::SetDimensionDriven { id, driven: true }
        .apply(&mut s)
        .unwrap();
    let d = s.dimensions[id];
    assert!((crate::dimension::measured_value(&s, &d).unwrap() - 270.0).abs() < 1e-6);
    SketchOp::SetDimensionDriven { id, driven: false }
        .apply(&mut s)
        .unwrap();
    assert!((s.dimensions[id].value - 270.0).abs() < 1e-6);
    s.expressions.push((id, "#a".into()));
    assert!(
        SketchOp::SetAngleOutside { id, outside: false }
            .apply(&mut s)
            .is_err()
    );
}

/// #96: a negative angle turns its line back past the first, its arc the short way back, and
/// its label dragged outside measures the same lines the long way on; a negative length that
/// turns the first line round takes the line hinged on it round too, and one that turns the
/// second line round keeps the angle (taken along the line's other way).
#[test]
fn a_negative_angle_and_hinges_on_a_line_turned_round() {
    let mut s = Sketch::new();
    let (la, a0, a1) = line(&mut s, v(0.0, 0.0), v(30.0, 0.0));
    let (lb, b0, b1) = line(&mut s, v(0.0, 0.0), v(20.0, 20.0));
    assert_eq!(a0, b0, "the lines share their corner");
    add(&mut s, ConstraintOf::FixPoint(pref(a0)));
    add(&mut s, ConstraintOf::Horizontal(Orient::Line(cref(la))));
    let k = DimensionKind::Angle {
        a: cref(la),
        b: cref(lb),
        flip_a: false,
        flip_b: false,
        ccw: None,
    };
    dim(&mut s, k, 45.0);
    dim(&mut s, DimensionKind::Aligned { a: a0, b: a1 }, 30.0);
    let find = |s: &Sketch, k: DimensionKind| {
        s.dimensions
            .iter()
            .find(|(_, d)| d.kind.same(&k))
            .unwrap()
            .0
    };
    let (id, len) = (
        find(&s, k),
        find(&s, DimensionKind::Aligned { a: a0, b: a1 }),
    );
    let dir = |s: &Sketch, l: CurveId| {
        let (p, q) = s.curve_ends(l).unwrap();
        (s.pos(q) - s.pos(p)).normalize()
    };
    let at = |deg: f64| v(deg.to_radians().cos(), deg.to_radians().sin());
    let sweep = |s: &Sketch| crate::dimension::angle_sweep(s, &s.dimensions[id]).unwrap();
    SketchOp::SetDimensionValue { id, value: -30.0 }
        .apply(&mut s)
        .unwrap();
    assert!(
        (dir(&s, lb) - at(-30.0)).length() < 1e-6,
        "{:?}",
        dir(&s, lb)
    );
    assert!((sweep(&s) + 30.0).abs() < 1e-6, "{}", sweep(&s));
    // Outside: 330° the long way on, still anticlockwise, the lines as they were.
    SketchOp::SetAngleOutside { id, outside: true }
        .apply(&mut s)
        .unwrap();
    assert!((s.dimensions[id].value - 330.0).abs() < 1e-9);
    assert_eq!(
        crate::dimension::angle_ccw(&s, s.dimensions[id].kind),
        Some(true)
    );
    assert!((sweep(&s) - 330.0).abs() < 1e-6, "{}", sweep(&s));
    assert!(
        (dir(&s, lb) - at(-30.0)).length() < 1e-6,
        "{:?}",
        dir(&s, lb)
    );
    // The first line turned round: the second swings round with it, 330° on from it still.
    let long = s.line_length(lb).unwrap();
    SketchOp::SetDimensionValue {
        id: len,
        value: -30.0,
    }
    .apply(&mut s)
    .unwrap();
    assert!(
        (dir(&s, la) - v(-1.0, 0.0)).length() < 1e-6,
        "{:?}",
        dir(&s, la)
    );
    assert!(
        (dir(&s, lb) - at(150.0)).length() < 1e-6,
        "{:?}",
        dir(&s, lb)
    );
    assert!((s.line_length(lb).unwrap() - long).abs() < 1e-6);
    let m = crate::dimension::measured_value(&s, &s.dimensions[id]).unwrap();
    assert!((m - 330.0).abs() < 1e-6, "{m}");
    // The second line turned round by its own length: it stays turned, the angle holding.
    let kb = DimensionKind::Aligned { a: b0, b: b1 };
    dim(&mut s, kb, long);
    let lid = find(&s, kb);
    SketchOp::SetDimensionValue {
        id: lid,
        value: -long,
    }
    .apply(&mut s)
    .unwrap();
    assert!(
        (dir(&s, lb) - at(-30.0)).length() < 1e-6,
        "{:?}",
        dir(&s, lb)
    );
    assert!(
        (dir(&s, la) - v(-1.0, 0.0)).length() < 1e-6,
        "{:?}",
        dir(&s, la)
    );
    let m = crate::dimension::measured_value(&s, &s.dimensions[id]).unwrap();
    assert!((m - 330.0).abs() < 1e-6, "{m}");
    assert!(!analyze(&s).has_conflicts());
}
