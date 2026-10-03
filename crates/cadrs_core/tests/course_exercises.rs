//! T1: the two exercises of Onshape's "Introduction to Sketching" course
//! (`reference/onshape/training/intro-to-sketching.md` S14, S15), built through the command
//! layer with the same sketch edits the tools make in the scenarios `course_ex1_basic` and
//! `course_ex2_intermediate`, must end fully defined, and the region the course selects must
//! have the area computed **independently** here from the exercise drawings (analytic formulas,
//! not our geometry code), to 0.01 mm².

use cadrs_core::commands::{AddSketch, EditSketch};
use cadrs_core::{Document, ElementId, FeatureId, History};
use cadrs_sketch::constraint::{ConstraintOf, CurveRef, CurveSpec, Orient, PointRef, PointSpec};
use cadrs_sketch::geom::{ArcGeom, arc_through, dist_point_segment, tangent_arc};
use cadrs_sketch::region::regions;
use cadrs_sketch::{
    CurveId, CurveKind, Dimension, DimensionKind, PlaneRef, PointId, Sketch, SketchOp, Vec2,
};
use std::f64::consts::PI;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// A sketch being built through commands, like the scenario's clicks.
struct Build {
    doc: Document,
    history: History,
    element: ElementId,
    feature: FeatureId,
}

impl Build {
    fn new(plane: PlaneRef) -> Self {
        let mut doc = Document::new("Exercise");
        let mut history = History::default();
        let element = doc.elements[0].id;
        let feature = FeatureId::new();
        history
            .execute(
                &mut doc,
                &AddSketch {
                    element,
                    feature,
                    plane: Some(plane),
                },
            )
            .unwrap();
        Self {
            doc,
            history,
            element,
            feature,
        }
    }

    fn s(&self) -> &Sketch {
        &self
            .doc
            .element(self.element)
            .unwrap()
            .feature(self.feature)
            .unwrap()
            .sketch()
            .unwrap()
            .geometry
    }

    fn op(&mut self, op: SketchOp) {
        let label = op.label();
        self.history
            .execute(
                &mut self.doc,
                &EditSketch {
                    element: self.element,
                    feature: self.feature,
                    op,
                },
            )
            .unwrap_or_else(|e| panic!("{label}: {e}"));
    }

    fn ops(&mut self, ops: Vec<SketchOp>) {
        self.op(SketchOp::Batch(ops));
    }

    /// The point nearest `p` (within 3 mm: the solver moves things a little).
    fn point(&self, p: Vec2) -> PointId {
        let s = self.s();
        let (k, d) = s
            .points
            .iter()
            .map(|(k, q)| (k, q.pos.distance(p)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert!(d < 3.0, "no point near {p:?} ({d})");
        k
    }

    /// The curve nearest `p` (within 3 mm).
    fn curve(&self, p: Vec2) -> CurveId {
        let s = self.s();
        let (k, d) = s
            .curves
            .keys()
            .map(|k| {
                let pts = cadrs_sketch::hit::curve_polyline(s, k);
                let d = pts
                    .windows(2)
                    .map(|w| dist_point_segment(p, w[0], w[1]))
                    .fold(f64::MAX, f64::min);
                (k, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert!(d < 3.0, "no curve near {p:?} ({d})");
        k
    }

    /// A driving dimension placed at its measured value (the Dimension tool's click), then
    /// given `value` (typed in its editor); `first` scales the sketch (S13.3).
    fn dimension(&mut self, kind: DimensionKind, value: f64, first: bool) {
        let measured = cadrs_sketch::dimension::measure(self.s(), kind).unwrap();
        self.op(SketchOp::SetDimension {
            dimension: Dimension::new(kind, measured, 10.0),
            moves: vec![],
            radii: vec![],
        });
        let id = self
            .s()
            .dimensions
            .iter()
            .find(|(_, d)| d.kind == kind)
            .map(|(k, _)| k)
            .unwrap();
        let op = cadrs_sketch::edit::set_value_op(self.s(), id, value, first);
        self.op(op);
    }

    fn line_ref(&self, p: Vec2) -> CurveRef {
        CurveRef::Curve(self.curve(p))
    }

    fn ends(&self, c: CurveId) -> (PointId, PointId) {
        self.s().curve_ends(c).unwrap()
    }

    fn fully_defined(&self) -> bool {
        let a = cadrs_sketch::solve::analyze(self.s());
        a.fully_constrained()
    }

    /// The area of the region containing `p`.
    fn area_at(&self, p: Vec2) -> f64 {
        let rs = regions(self.s());
        let i = cadrs_sketch::region::region_at(&rs, p).expect("a region there");
        rs[i].area()
    }
}

fn arc_op(a: ArcGeom) -> SketchOp {
    let a = a.to_ccw();
    SketchOp::AddArc {
        center: a.center,
        start: a.start(),
        end: a.end(),
        construction: false,
    }
}

/// Exercise 1 (Basic Sketching, `ex1-basic-drawing.png`), the analytic area of the region
/// selected in `ex1-step8.png`.
///
/// From the drawing: the three short vertical lines (the two right edges and the notch's back
/// line) are equal and each 50, and the outline is symmetric about the construction line
/// through the circle's center, so the right edge is 50 + 50 + 50 = 150 tall. The top and
/// bottom edges end on the big circle at its top and bottom (the line from the center to the
/// bottom edge's start is vertical), so the big circle's radius is 150 / 2 = 75. The ring is 35
/// wide, so the small circle's radius is 75 - 35 = 40. The bottom edge is 200 long, from below
/// the center to the right edge; the notch is 70 deep and 50 tall; the two holes are Ø25.
///
/// The step 8 screenshot selects the plate: the region right of the big circle. Its area is
/// the 200 x 150 rectangle, minus the half of the big circle inside it (the circle's region is
/// separate: its right half lies in the rectangle), minus the notch, minus the two holes:
///
///   A_plate = 200·150 − π·75²/2 − 70·50 − 2·π·12.5²
///           = 30000 − 8835.729 − 3500 − 981.748 = 16682.523 mm²
///
/// The orchestrator's figure, 30000 + π·75²/2 − 3500 − π·40² − 2·π·12.5² ≈ 29327.43 mm², is
/// the plate **plus the ring** (the big circle's region minus the small circle): the whole
/// part as it would extrude with the inner hole. That is what the readout shows with both
/// regions selected (the scenario's last screenshot). The course's step 8 selects only the
/// plate (the circle stays grey in `ex1-step8.png`), so the exercise's answer is 16682.523.
fn ex1_expected() -> (f64, f64) {
    let plate = 200.0 * 150.0 - PI * 75.0 * 75.0 / 2.0 - 70.0 * 50.0 - 2.0 * PI * 12.5 * 12.5;
    let ring = PI * 75.0 * 75.0 - PI * 40.0 * 40.0;
    (plate, plate + ring)
}

#[test]
fn exercise_1_is_fully_defined_and_its_area_matches_the_drawing() {
    let (plate, total) = ex1_expected();
    assert!((plate - 16682.523).abs() < 1e-3, "{plate}");
    assert!((total - 29327.43).abs() < 0.01, "{total}");

    let mut b = Build::new(PlaneRef::Top);
    // Step 2: two circles at the origin, the outline from the big circle's top round the
    // notch to its bottom, and a construction line from the origin (inferred constraints as
    // the tools add them).
    b.ops(vec![
        SketchOp::AddCircle {
            center: v(0.0, 0.0),
            radius: 60.0,
            construction: false,
        },
        SketchOp::AddConstraints(vec![ConstraintOf::Coincident(
            PointSpec::At(v(0.0, 0.0)),
            PointSpec::Origin,
        )]),
    ]);
    b.op(SketchOp::AddCircle {
        center: v(0.0, 0.0),
        radius: 28.0,
        construction: false,
    });
    let outline = [
        v(0.0, 60.0),
        v(160.0, 69.0),
        v(160.0, 21.0),
        v(104.0, 21.0),
        v(104.0, -21.0),
        v(160.0, -21.0),
        v(160.0, -60.0),
        v(0.0, -60.0),
    ];
    let seg = |i: usize| CurveSpec::Between(outline[i], outline[i + 1]);
    let big = CurveSpec::Circle(v(0.0, 0.0), 60.0);
    b.ops(vec![
        SketchOp::AddPolyline {
            points: outline.to_vec(),
            closed: false,
            construction: false,
            label: "Add line",
        },
        SketchOp::AddConstraints(vec![
            ConstraintOf::PointOnCurve(PointSpec::At(outline[0]), big),
            ConstraintOf::PointOnCurve(PointSpec::At(outline[0]), CurveSpec::YAxis),
            ConstraintOf::PointOnCurve(PointSpec::At(outline[7]), big),
            ConstraintOf::PointOnCurve(PointSpec::At(outline[7]), CurveSpec::YAxis),
            ConstraintOf::Vertical(Orient::Line(seg(1))),
            ConstraintOf::Horizontal(Orient::Line(seg(2))),
            ConstraintOf::Vertical(Orient::Line(seg(3))),
            ConstraintOf::Horizontal(Orient::Line(seg(4))),
            ConstraintOf::Vertical(Orient::Line(seg(5))),
        ]),
    ]);
    b.ops(vec![SketchOp::AddPolyline {
        points: vec![v(0.0, 0.0), v(104.0, -6.0)],
        closed: false,
        construction: true,
        label: "Add line",
    }]);
    // Step 3: midpoint, horizontal (construction line, top, bottom), vertical alignment of the
    // right edges, equal short lines.
    let notch = b.line_ref(v(104.0, 12.0));
    let add = |b: &mut Build, c: Vec<cadrs_sketch::Constraint>| {
        b.op(SketchOp::AddConstraint {
            constraints: c,
            label: "Add constraint",
        })
    };
    let m = PointRef::Point(b.point(v(104.0, -6.0)));
    add(&mut b, vec![ConstraintOf::Midpoint(m, notch)]);
    let axis = b.line_ref(v(60.0, -2.0));
    add(&mut b, vec![ConstraintOf::Horizontal(Orient::Line(axis))]);
    let top = b.line_ref(v(80.0, 64.0));
    add(&mut b, vec![ConstraintOf::Horizontal(Orient::Line(top))]);
    let bottom = b.line_ref(v(80.0, -60.0));
    add(&mut b, vec![ConstraintOf::Horizontal(Orient::Line(bottom))]);
    let (p3, p6) = (b.point(v(160.0, 21.0)), b.point(v(160.0, -21.0)));
    add(
        &mut b,
        vec![ConstraintOf::Vertical(Orient::Points(PointRef::Point(p3), PointRef::Point(p6)))],
    );
    let (upper, lower) = (b.line_ref(v(160.0, 45.0)), b.line_ref(v(160.0, -45.0)));
    add(
        &mut b,
        vec![ConstraintOf::Equal(upper, notch), ConstraintOf::Equal(upper, lower)],
    );
    // Step 4: the first dimension, 200 from the circle's center to the right edge, scales
    // the sketch.
    let center = b.point(v(0.0, 0.0));
    let right = b.line_ref(v(160.0, -45.0));
    b.dimension(
        DimensionKind::PointLine {
            p: PointRef::Point(center),
            line: right,
        },
        200.0,
        true,
    );
    assert!((b.s().pos(b.point(v(200.0, -75.0))).x - 200.0).abs() < 1.0, "scaled 1.25×");
    // Step 5: 50, 70, and the ring's 35 (big circle clicked inside, small one outside).
    let CurveRef::Curve(upper_id) = upper else { unreachable!() };
    let (a, bb) = b.ends(upper_id);
    b.dimension(DimensionKind::Aligned { a, b: bb }, 50.0, false);
    let tab = b.curve(v(165.0, -25.0));
    let (a, bb) = b.ends(tab);
    b.dimension(DimensionKind::Aligned { a, b: bb }, 70.0, false);
    let big = b.curve(v(-75.0, 0.0));
    let small = b.curve(v(-35.0, 0.0));
    b.dimension(
        DimensionKind::CircleCircle {
            a: big,
            b: small,
            far_a: false,
            far_b: true,
            axis: None,
        },
        35.0,
        false,
    );
    assert!(b.fully_defined(), "step 5 is fully defined");
    // Step 6: the tab holes, construction lines to the right edges' midpoints.
    for y in [50.0, -50.0] {
        b.op(SketchOp::AddCircle {
            center: v(165.0, y),
            radius: 10.0,
            construction: false,
        });
        let edge = if y > 0.0 { upper } else { lower };
        let CurveRef::Curve(edge_id) = edge else { unreachable!() };
        let (ea, eb) = b.ends(edge_id);
        let mid = b.s().pos(ea).midpoint(b.s().pos(eb));
        b.ops(vec![
            SketchOp::AddPolyline {
                points: vec![v(165.0, y), mid],
                closed: false,
                construction: true,
                label: "Add line",
            },
            SketchOp::AddConstraints(vec![ConstraintOf::Midpoint(
                PointSpec::At(mid),
                CurveSpec::Id(edge_id),
            )]),
        ]);
    }
    let (h1, h2) = (b.curve(v(155.0, 50.0)), b.curve(v(155.0, -50.0)));
    b.dimension(DimensionKind::Diameter { curve: h1 }, 25.0, false);
    let (k1, k2) = (b.line_ref(v(190.0, 50.0)), b.line_ref(v(190.0, -50.0)));
    add(
        &mut b,
        vec![
            ConstraintOf::Equal(CurveRef::Curve(h1), CurveRef::Curve(h2)),
            ConstraintOf::Equal(k1, k2),
            ConstraintOf::Horizontal(Orient::Line(k1)),
            ConstraintOf::Horizontal(Orient::Line(k2)),
        ],
    );
    let CurveRef::Curve(k1_id) = k1 else { unreachable!() };
    let (a, bb) = b.ends(k1_id);
    b.dimension(DimensionKind::Aligned { a, b: bb }, 35.0, false);
    assert!(b.fully_defined(), "step 6 is fully defined (all black)");
    // Step 8: the plate's region, and the plate with the ring.
    let got = b.area_at(v(100.0, 50.0));
    assert!((got - plate).abs() < 0.01, "plate {got} vs {plate}");
    let ring = b.area_at(v(-57.0, 0.0));
    assert!((got + ring - total).abs() < 0.01, "plate + ring {} vs {total}", got + ring);
}

// Exercise 2 (Intermediate Sketching, `ex2-intermediate-drawing.png`), the analytic area of
// the region selected in `ex2-step16.png` (the plate: its outline minus the keyhole slot and
// the rectangular cut-out).
//
// Outline, from the drawing, symmetric about the vertical center line x = 0 (A is the head's
// center, 200 above the base):
// - the base: 125 wide (x = ±62.5, y = 0); the sides: 50 tall;
// - the waist arcs: R125, from the side's top S = (62.5, 50), concave (their centers E lie
//   outside), tangent to the head R75 about A = (0, 200). Tangent circles on opposite sides
//   of the touching point: |E − A| = 75 + 125 = 200 and |E − S| = 125. Subtracting the two
//   circle equations gives 125·Ex − 300·Ey + 9218.75 = 0, i.e. Ex = 2.4·Ey − 73.75; putting it
//   in the first: 6.76·Ey² − 754·Ey + 5439.0625 = 0, so Ey = (754 + √421443.75) / 13.52 =
//   103.786 (the other root puts E inside the part) and Ex = 175.336;
// - the tangent point D = A + 75·(E − A)/200 = (65.751, 163.920);
// - Area = polygon (±62.5, 0), (62.5, 50), D, D', (−62.5, 50)
//          − 2 × the R125 segment over chord S–D (angle ∠SED)
//          + the R75 segment over chord D–D' (the head's 237.51° sweep).
// Keyhole slot, inside:
// - the head R35 = 75 − 40 (the 40 offset), about A;
// - the side arcs R40, tangent to the slot's vertical sides x = ±15 (the R15 bottom arc joins
//   them, 30 wide) and to the R35 head: their centers T = (−55, yT) with |T − A| = 40 + 35,
//   so yT = 200 − √(75² − 55²) = 149.010; the touching point C = A + 35·(T − A)/75;
// - the sides run from y = 100 (the "100" below A, where the R15 arc's center is) to yT;
// - Area = polygon (15, 100), (15, yT), C', C, (−15, yT), (−15, 100)
//          + the R35 segment over C'–C (265.67°) − 2 × the R40 segment over C–(−15, yT)
//          + the R15 half disc.
// Cut-out: 80 × 20.
// A segment of radius r over angle θ is r²/2·(θ − sin θ).
fn ex2_expected() -> f64 {
    let seg = |r: f64, th: f64| r * r / 2.0 * (th - th.sin());
    let angle = |p: Vec2, q: Vec2, c: Vec2| {
        let (u, w) = (p - c, q - c);
        u.cross(w).abs().atan2(u.dot(w))
    };
    let shoelace = |p: &[Vec2]| {
        (0..p.len())
            .map(|i| p[i].cross(p[(i + 1) % p.len()]))
            .sum::<f64>()
            / 2.0
    };
    let a = v(0.0, 200.0);
    let ey = (754.0 + (754.0f64 * 754.0 - 4.0 * 6.76 * 5439.0625).sqrt()) / (2.0 * 6.76);
    let e = v(2.4 * ey - 73.75, ey);
    assert!((e.distance(a) - 200.0).abs() < 1e-9 && (e.distance(v(62.5, 50.0)) - 125.0).abs() < 1e-9);
    let d = a + (e - a) * (75.0 / 200.0);
    let d2 = v(-d.x, d.y);
    let s = v(62.5, 50.0);
    let head_sweep = 2.0 * PI - angle(d, d2, a);
    let outline = shoelace(&[v(-62.5, 0.0), v(62.5, 0.0), s, d, d2, v(-62.5, 50.0)])
        - 2.0 * seg(125.0, angle(s, d, e))
        + seg(75.0, head_sweep);
    let yt = 200.0 - (75.0f64 * 75.0 - 55.0 * 55.0).sqrt();
    let t = v(-55.0, yt);
    let c = a + (t - a) * (35.0 / 75.0);
    let c2 = v(-c.x, c.y);
    let slot_head = 2.0 * PI - angle(c, c2, a);
    let keyhole = shoelace(&[v(15.0, 100.0), v(15.0, yt), c2, c, v(-15.0, yt), v(-15.0, 100.0)])
        + seg(35.0, slot_head)
        - 2.0 * seg(40.0, angle(c, v(-15.0, yt), t))
        + PI * 15.0 * 15.0 / 2.0;
    outline - keyhole - 80.0 * 20.0
}

#[test]
fn exercise_2_is_fully_defined_and_its_area_matches_the_drawing() {
    let expected = ex2_expected();
    assert!((expected - 24906.823).abs() < 1e-3, "{expected}");

    let mut b = Build::new(PlaneRef::Front);
    let add = |b: &mut Build, c: Vec<cadrs_sketch::Constraint>| {
        b.op(SketchOp::AddConstraint {
            constraints: c,
            label: "Add constraint",
        })
    };
    // Step 2: the center line, the base half-line and the side.
    b.ops(vec![
        SketchOp::AddPolyline {
            points: vec![v(0.0, 0.0), v(0.0, 180.0)],
            closed: false,
            construction: true,
            label: "Add line",
        },
        SketchOp::AddConstraints(vec![
            ConstraintOf::Coincident(PointSpec::At(v(0.0, 0.0)), PointSpec::Origin),
            ConstraintOf::Vertical(Orient::Line(CurveSpec::Between(v(0.0, 0.0), v(0.0, 180.0)))),
        ]),
    ]);
    let axis = b.curve(v(0.0, 90.0));
    b.ops(vec![
        SketchOp::AddPolyline {
            points: vec![v(0.0, 0.0), v(-60.0, 0.0), v(-60.0, 45.0)],
            closed: false,
            construction: false,
            label: "Add line",
        },
        SketchOp::AddConstraints(vec![
            ConstraintOf::Horizontal(Orient::Line(CurveSpec::Between(v(0.0, 0.0), v(-60.0, 0.0)))),
            ConstraintOf::Vertical(Orient::Line(CurveSpec::Between(v(-60.0, 0.0), v(-60.0, 45.0)))),
        ]),
    ]);
    // Step 3: the 3-point arc; step 4: mirror.
    b.op(arc_op(arc_through(v(-60.0, 45.0), v(-63.0, 160.0), v(-48.0, 100.0)).unwrap()));
    let (base, side, waist) = (
        b.curve(v(-30.0, 0.0)),
        b.curve(v(-60.0, 25.0)),
        b.curve(v(-48.3, 100.0)),
    );
    b.op(SketchOp::Mirror {
        axis,
        curves: vec![base, side, waist],
    });
    // Step 5: the tangent arc between the arcs' top ends (tangent at both).
    let top = b.point(v(-63.0, 160.0));
    let dir = -b.s().direction_from(waist, top).unwrap();
    let head = tangent_arc(b.s().pos(top), dir, v(63.0, 160.0)).unwrap();
    let h = head.to_ccw();
    let right_waist = b.curve(v(48.3, 100.0));
    b.ops(vec![
        arc_op(head),
        SketchOp::AddConstraints(vec![
            ConstraintOf::Tangent(CurveSpec::Id(waist), CurveSpec::Between(h.start(), h.end())),
            ConstraintOf::Tangent(
                CurveSpec::Id(right_waist),
                CurveSpec::Between(h.start(), h.end()),
            ),
        ]),
    ]);
    // Step 6: the head's center on the center line's top.
    let head_id = b.curve(v(0.0, h.center.y + h.radius));
    let hc = b.s().curve_points(head_id)[0];
    let axis_top = b.point(v(0.0, 180.0));
    add(
        &mut b,
        vec![ConstraintOf::Coincident(PointRef::Point(hc), PointRef::Point(axis_top))],
    );
    // Step 7: R75 (first: scales), 200, 125, 50, R125.
    b.dimension(DimensionKind::Radius { curve: head_id }, 75.0, true);
    let a = b.s().curve_points(head_id)[0];
    b.dimension(
        DimensionKind::PointLine {
            p: PointRef::Point(a),
            line: CurveRef::XAxis,
        },
        200.0,
        false,
    );
    let corners: Vec<PointId> = {
        let s = b.s();
        let mut v: Vec<PointId> = s
            .points
            .iter()
            .filter(|(_, p)| p.pos.y.abs() < 1e-6 && p.pos.x.abs() > 10.0)
            .map(|(k, _)| k)
            .collect();
        v.sort_by(|x, y| s.pos(*x).x.total_cmp(&s.pos(*y).x));
        v
    };
    b.dimension(
        DimensionKind::Horizontal {
            a: corners[0],
            b: corners[1],
        },
        125.0,
        false,
    );
    let right_side = b.curve(v(62.5, 25.0));
    let (sa, sb) = b.ends(right_side);
    b.dimension(DimensionKind::Aligned { a: sa, b: sb }, 50.0, false);
    b.dimension(DimensionKind::Radius { curve: waist }, 125.0, false);
    assert!(b.fully_defined(), "step 7: the outline is fully defined");
    // Step 8: offset the head 40 inward (it runs counter-clockwise: inward is its left).
    b.op(SketchOp::Offset {
        chain: vec![(head_id, false)],
        distance: 40.0,
        left: true,
        label: (PI / 2.0, 0.0),
    });
    let slot_head = b.curve(v(0.0, 235.0));
    // Steps 9–10: a vertical line; a tangent arc from its top to the offset arc's left end,
    // ending tangent to it.
    b.ops(vec![
        SketchOp::AddPolyline {
            points: vec![v(-15.0, 100.0), v(-15.0, 145.0)],
            closed: false,
            construction: false,
            label: "Add line",
        },
        SketchOp::AddConstraints(vec![ConstraintOf::Vertical(Orient::Line(
            CurveSpec::Between(v(-15.0, 100.0), v(-15.0, 145.0)),
        ))]),
    ]);
    let line = b.curve(v(-15.0, 120.0));
    let (sa, sb) = b.ends(slot_head);
    let left_end = if b.s().pos(sa).x < b.s().pos(sb).x { sa } else { sb };
    let p = b.s().pos(left_end);
    let t = tangent_arc(v(-15.0, 145.0), v(0.0, 1.0), p).unwrap();
    let tc = t.to_ccw();
    b.ops(vec![
        arc_op(t),
        SketchOp::AddConstraints(vec![
            ConstraintOf::Tangent(CurveSpec::Id(line), CurveSpec::Between(tc.start(), tc.end())),
            ConstraintOf::Tangent(CurveSpec::Id(slot_head), CurveSpec::Between(tc.start(), tc.end())),
        ]),
    ]);
    // The arc the tool made: the other curve at the line's top.
    let (la, lb) = b.ends(line);
    let line_top = if b.s().pos(la).y > b.s().pos(lb).y { la } else { lb };
    let side_arc = b.s().curves_at(line_top).find(|c| *c != line).unwrap();
    // Step 11: mirror both.
    b.op(SketchOp::Mirror {
        axis,
        curves: vec![line, side_arc],
    });
    // Step 12: the bottom arc.
    let (la, lb) = b.ends(line);
    let bottom = if b.s().pos(la).y < b.s().pos(lb).y { la } else { lb };
    let bp = b.s().pos(bottom);
    let right_line = b.curve(v(-bp.x, bp.y + 10.0));
    let arc = tangent_arc(bp, v(0.0, -1.0), v(-bp.x, bp.y)).unwrap();
    let ac = arc.to_ccw();
    b.ops(vec![
        arc_op(arc),
        SketchOp::AddConstraints(vec![
            ConstraintOf::Tangent(CurveSpec::Id(line), CurveSpec::Between(ac.start(), ac.end())),
            ConstraintOf::Tangent(
                CurveSpec::Id(right_line),
                CurveSpec::Between(ac.start(), ac.end()),
            ),
        ]),
    ]);
    let bottom_arc = b.s().curves_at(bottom).find(|c| *c != line).unwrap();
    // Step 13: the center-point rectangle, centered on the center line.
    let corners = [v(-40.0, 25.0), v(40.0, 25.0), v(40.0, 45.0), v(-40.0, 45.0)];
    let mut specs = cadrs_sketch::constraint::rectangle_constraints(corners);
    specs.push(ConstraintOf::PointOnCurve(PointSpec::At(v(0.0, 35.0)), CurveSpec::Id(axis)));
    b.ops(vec![
        SketchOp::AddCenterRectangle {
            center: v(0.0, 35.0),
            corners,
            construction: false,
        },
        SketchOp::AddConstraints(specs),
    ]);
    // Step 14: R15, R40, 100, 20, 25, 80.
    b.dimension(DimensionKind::Radius { curve: bottom_arc }, 15.0, false);
    b.dimension(DimensionKind::Radius { curve: side_arc }, 40.0, false);
    let a = b.s().curve_points(head_id)[0];
    let (la, lb) = b.ends(line);
    let bottom = if b.s().pos(la).y < b.s().pos(lb).y { la } else { lb };
    b.dimension(DimensionKind::Vertical { a, b: bottom }, 100.0, false);
    let rect_right = b.curve(v(40.0, 35.0));
    let (ra, rb) = b.ends(rect_right);
    b.dimension(DimensionKind::Aligned { a: ra, b: rb }, 20.0, false);
    let corner = b.point(v(-40.0, 25.0));
    b.dimension(
        DimensionKind::PointLine {
            p: PointRef::Point(corner),
            line: CurveRef::Curve(base),
        },
        25.0,
        false,
    );
    let rect_bottom = b.curve(v(-20.0, 25.0));
    let (ra, rb) = b.ends(rect_bottom);
    b.dimension(DimensionKind::Aligned { a: ra, b: rb }, 80.0, false);
    assert!(b.fully_defined(), "step 14: fully defined");
    // Step 16: the plate's region.
    let got = b.area_at(v(-50.0, 15.0));
    assert!((got - expected).abs() < 0.01, "area {got} vs {expected}");
    // The R35 head, R40 sides and R15 bottom are where the drawing puts them.
    let g = b.s().arc_geom(side_arc).unwrap();
    assert!((g.radius - 40.0).abs() < 1e-6 && g.center.distance(v(-55.0, 149.0098)) < 1e-3);
    let _ = CurveKind::Line { a: corner, b: corner };
}

/// S13.3: typing the first dimension's value scales the whole sketch about the origin (the
/// sketch is attached to it) in one undo step; later dimensions only move their geometry.
#[test]
fn first_dimension_scales_the_sketch_as_one_undo_step() {
    let mut b = Build::new(PlaneRef::Top);
    let corners = [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 25.0), v(0.0, 25.0)];
    b.ops(vec![
        SketchOp::AddPolyline {
            points: corners.to_vec(),
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
        SketchOp::AddConstraints(
            [
                cadrs_sketch::constraint::rectangle_constraints(corners),
                vec![ConstraintOf::Coincident(PointSpec::At(v(0.0, 0.0)), PointSpec::Origin)],
            ]
            .concat(),
        ),
    ]);
    b.op(SketchOp::AddCircle {
        center: v(28.0, 12.0),
        radius: 6.0,
        construction: false,
    });
    let bottom = b.curve(v(20.0, 0.0));
    let (a, bb) = b.ends(bottom);
    let kind = DimensionKind::Aligned { a, b: bb };
    b.op(SketchOp::SetDimension {
        dimension: Dimension::new(kind, 40.0, -8.0),
        moves: vec![],
        radii: vec![],
    });
    let placed = b.doc.clone();
    let id = b.s().dimensions.keys().next().unwrap();
    let op = cadrs_sketch::edit::set_value_op(b.s(), id, 100.0, true);
    assert!(matches!(op, SketchOp::Batch(_)), "the first dimension scales");
    b.op(op);
    // Everything 2.5× about the origin: the far corner and the circle.
    assert!(b.s().point_at(v(100.0, 62.5), 1e-6).is_some());
    let circle = b.curve(v(85.0, 30.0));
    match b.s().curves[circle].kind {
        CurveKind::Circle { center, radius } => {
            assert!(b.s().pos(center).distance(v(70.0, 30.0)) < 1e-6);
            assert!((radius - 15.0).abs() < 1e-9);
        }
        _ => unreachable!(),
    }
    // One undo step back to the placed dimension at 40.
    b.history.undo(&mut b.doc).unwrap();
    assert_eq!(b.doc, placed);
    // Not the first: only its own edge moves.
    let op = cadrs_sketch::edit::set_value_op(b.s(), id, 100.0, false);
    assert!(matches!(op, SketchOp::SetDimensionValue { .. }));
    b.op(op);
    assert!(b.s().point_at(v(100.0, 25.0), 1e-6).is_some());
    assert!(b.s().point_at(v(28.0, 12.0), 1e-6).is_some(), "the circle stays");
}
