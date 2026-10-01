//! Sample geometry for tests and scripted scenarios: the course's parts, built through the same
//! sketch edits a user would make.
//!
//! - The **Control Arm** of PS6 (`reference/onshape/training/intro-to-part-studios.md`,
//!   `ex1-step*.png`): a hub Ø70 with a Ø35 bore and a 45 × 10 keyway, eyes Ø35 with Ø20 holes
//!   107.5 mm either side, joined by webs tangent to the hub and the eyes. Extrude 1 takes the
//!   hub ring, the right web and the right eye ring, 40 mm; Extrude 2 the left web and the left
//!   eye ring, 25 mm (both New, so two parts that touch).

use cadrs_sketch::constraint::{ConstraintOf, CurveSpec, Orient, PointSpec, rectangle_constraints};
use cadrs_sketch::region::{region_at, regions};
use cadrs_sketch::{Dimension, DimensionKind, Sketch, SketchOp, Vec2};

use crate::document::{ExtrudeFeature, RegionRef};
use crate::ids::FeatureId;

pub const HUB_R: f64 = 35.0;
pub const BORE_R: f64 = 17.5;
pub const EYE_R: f64 = 17.5;
pub const EYE_HOLE_R: f64 = 10.0;
/// The eyes' centres are this far from the hub's.
pub const EYE_X: f64 = 107.5;
pub const EXTRUDE_1_DEPTH: f64 = 40.0;
pub const EXTRUDE_2_DEPTH: f64 = 25.0;

/// Points inside the regions Extrude 1 takes: the hub ring, the right web, the right eye ring
/// (in this order, so they are regions 0, 1 and 2 of the extrude).
pub const EXTRUDE_1_SEEDS: [Vec2; 3] = [
    Vec2::new(0.0, 26.0),
    Vec2::new(60.0, 0.0),
    Vec2::new(EYE_X + 14.0, 0.0),
];
/// Extrude 2: the left web, the left eye ring.
pub const EXTRUDE_2_SEEDS: [Vec2; 2] = [Vec2::new(-60.0, 0.0), Vec2::new(-EYE_X - 14.0, 0.0)];

/// The Control Arm sketch's curves: circles for the hub, the bore and the eyes, the keyway
/// rectangle, and the four web lines on the external tangents.
pub fn control_arm_geometry() -> SketchOp {
    let v = Vec2::new;
    let circle = |c: Vec2, r: f64| SketchOp::AddCircle {
        center: c,
        radius: r,
        construction: false,
    };
    let mut ops = vec![
        circle(v(0.0, 0.0), HUB_R),
        circle(v(0.0, 0.0), BORE_R),
        SketchOp::AddPolyline {
            points: vec![v(-22.5, -5.0), v(22.5, -5.0), v(22.5, 5.0), v(-22.5, 5.0)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
    ];
    // External tangents: the tangent points lie along (±cos t, ±sin t) from each center.
    let t = ((HUB_R - EYE_R) / EYE_X).acos();
    for side in [1.0, -1.0] {
        let cx = side * EYE_X;
        ops.push(circle(v(cx, 0.0), EYE_R));
        ops.push(circle(v(cx, 0.0), EYE_HOLE_R));
        for s in [1.0, -1.0] {
            let a = v(side * HUB_R * t.cos(), s * HUB_R * t.sin());
            let b = v(cx + side * EYE_R * t.cos(), s * EYE_R * t.sin());
            ops.push(SketchOp::AddPolyline {
                points: vec![a, b],
                closed: false,
                construction: false,
                label: "Add line",
            });
        }
    }
    SketchOp::Batch(ops)
}

/// The Ø20 dimension of the right eye's hole (a sketch dimension the tests change).
pub fn eye_hole_dimension(s: &Sketch) -> Option<SketchOp> {
    let curve = s.curves.iter().find_map(|(id, c)| match c.kind {
        cadrs_sketch::CurveKind::Circle { center, radius }
            if (radius - EYE_HOLE_R).abs() < 1e-9
                && s.pos(center).distance(Vec2::new(EYE_X, 0.0)) < 1e-9 =>
        {
            Some(id)
        }
        _ => None,
    })?;
    Some(SketchOp::SetDimension {
        dimension: Dimension {
            kind: DimensionKind::Diameter { curve },
            value: 2.0 * EYE_HOLE_R,
            offset: std::f64::consts::FRAC_PI_4,
            along: 0.0,
            driven: false,
        },
        moves: vec![],
        radii: vec![],
    })
}

/// The constraints and dimensions that fully define the Control Arm sketch, as the course draws it
/// (`ex1-step2.png`, `ex1-drawing.png`): the hub on the origin with its bore concentric, the
/// keyway a centered 45 × 10 rectangle, the eyes horizontal with the origin and symmetric about
/// the Y axis (250 overall, from the far side of one eye to the other's), each web line on and
/// tangent to the hub and its eye, and the diameters Ø70, Ø35, and the left eye's Ø35 and Ø20.
pub fn control_arm_constraints(g: &Sketch) -> SketchOp {
    let v = Vec2::new;
    let circle = |c: Vec2, r: f64| CurveSpec::Circle(c, r);
    let hub = circle(v(0.0, 0.0), HUB_R);
    let mut specs = vec![
        ConstraintOf::Coincident(PointSpec::At(v(0.0, 0.0)), PointSpec::Origin),
        ConstraintOf::Concentric(hub, circle(v(0.0, 0.0), BORE_R)),
    ];
    let corners = [v(-22.5, -5.0), v(22.5, -5.0), v(22.5, 5.0), v(-22.5, 5.0)];
    specs.extend(rectangle_constraints(corners));
    specs.push(ConstraintOf::Center(PointSpec::Origin, PointSpec::At(corners[0]), PointSpec::At(corners[2])));
    let t = ((HUB_R - EYE_R) / EYE_X).acos();
    for side in [1.0, -1.0] {
        let cx = side * EYE_X;
        let eye = circle(v(cx, 0.0), EYE_R);
        specs.push(ConstraintOf::Concentric(eye, circle(v(cx, 0.0), EYE_HOLE_R)));
        for sgn in [1.0, -1.0] {
            let a = v(side * HUB_R * t.cos(), sgn * HUB_R * t.sin());
            let b = v(cx + side * EYE_R * t.cos(), sgn * EYE_R * t.sin());
            let line = CurveSpec::Between(a, b);
            specs.push(ConstraintOf::PointOnCurve(PointSpec::At(a), hub));
            specs.push(ConstraintOf::PointOnCurve(PointSpec::At(b), eye));
            specs.push(ConstraintOf::Tangent(line, hub));
            specs.push(ConstraintOf::Tangent(line, eye));
        }
    }
    specs.push(ConstraintOf::Horizontal(Orient::Points(PointSpec::At(v(-EYE_X, 0.0)), PointSpec::Origin)));
    // The eyes and their holes symmetric about the Y axis: the right ones take the left ones'
    // positions and sizes (only the left eye is dimensioned, as in `ex1-step2.png`).
    for r in [EYE_R, EYE_HOLE_R] {
        specs.push(ConstraintOf::SymmetricCurves(
            circle(v(-EYE_X, 0.0), r),
            circle(v(EYE_X, 0.0), r),
            CurveSpec::YAxis,
        ));
    }
    let mut ops = vec![SketchOp::AddConstraints(specs)];
    let dim = |kind: DimensionKind, value: f64, offset: f64| SketchOp::SetDimension {
        dimension: Dimension {
            kind,
            value,
            offset,
            along: 0.0,
            driven: false,
        },
        moves: vec![],
        radii: vec![],
    };
    let circle_id = |c: Vec2, r: f64| {
        g.curves.iter().find_map(|(id, k)| match k.kind {
            cadrs_sketch::CurveKind::Circle { center, radius }
                if (radius - r).abs() < 1e-9 && g.pos(center).distance(c) < 1e-9 =>
            {
                Some(id)
            }
            _ => None,
        })
    };
    let d45 = std::f64::consts::FRAC_PI_4;
    for (c, r, off) in [
        (v(0.0, 0.0), HUB_R, 3.0 * d45),
        (v(0.0, 0.0), BORE_R, d45),
        (v(-EYE_X, 0.0), EYE_R, 3.0 * d45),
        (v(-EYE_X, 0.0), EYE_HOLE_R, d45),
    ] {
        if let Some(curve) = circle_id(c, r) {
            ops.push(dim(DimensionKind::Diameter { curve }, 2.0 * r, off));
        }
    }
    let point = |p: Vec2| g.point_at(p, 1e-9);
    if let (Some(a), Some(b), Some(c)) = (point(corners[0]), point(corners[1]), point(corners[2])) {
        ops.push(dim(DimensionKind::Horizontal { a, b }, 45.0, -12.0));
        ops.push(dim(DimensionKind::Vertical { a: b, b: c }, 10.0, 8.0));
    }
    // The overall length, 250: from the far side of one eye to the far side of the other.
    if let (Some(a), Some(b)) = (circle_id(v(-EYE_X, 0.0), EYE_R), circle_id(v(EYE_X, 0.0), EYE_R)) {
        ops.push(dim(
            DimensionKind::CircleCircle { a, b, far_a: true, far_b: true },
            2.0 * (EYE_X + EYE_R),
            -40.0,
        ));
    }
    SketchOp::Batch(ops)
}

/// The Control Arm sketch, with the eye hole's dimension.
pub fn control_arm_sketch() -> Sketch {
    let mut s = Sketch::new();
    control_arm_geometry().apply(&mut s).expect("control arm geometry");
    if let Some(op) = eye_hole_dimension(&s) {
        op.apply(&mut s).expect("eye hole dimension");
    }
    s
}

/// The regions of `sketch` (its geometry `g`) under `seeds`, as an extrude refers to them.
pub fn region_refs(sketch: FeatureId, g: &Sketch, seeds: &[Vec2]) -> Vec<RegionRef> {
    let rs = regions(g);
    seeds
        .iter()
        .filter_map(|p| region_at(&rs, *p).map(|i| RegionRef::new(sketch, &rs[i])))
        .collect()
}

/// An extrude of `regions`, `depth` mm deep, New.
pub fn extrude_of(regions: Vec<RegionRef>, depth: f64) -> ExtrudeFeature {
    ExtrudeFeature {
        regions,
        depth,
        depth_expr: format!("{depth} mm"),
        ..ExtrudeFeature::default()
    }
}

pub mod bracket;
pub mod bracket_pair;
pub mod conrod;
pub mod design_intent;
pub mod flange;
pub mod drawing_bracket;
pub mod drill;
pub mod gasket;
pub mod gear_cover;
pub mod hand_brake;
pub mod inspection;
pub mod linked_block;
pub mod motor_mount;
pub mod p3b9;
pub mod phone_case;
pub mod piston;
pub mod mates;
pub mod pneumatic;
pub mod pneumatic_ex2;
pub mod pneumatic_ex3;
pub mod reflector;
pub mod scale;
pub mod simulation;
pub mod step_stool;
pub mod ujoint;
pub mod ujoint_assembly;
pub mod ujoint_drawing;
pub mod drawing_bar;
