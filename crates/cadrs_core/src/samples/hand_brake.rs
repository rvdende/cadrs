//! The Hand Brake stand-in (P3C.6, D14, `fixtures/hand_brake_standin.cadrs`).
//!
//! Onshape's public "Exercise: Hand Brake Update" document can't be copied, so the drawing update
//! exercise starts from this Handle Part Studio, built from the course's pictures
//! (`ex3-step4.png`, `ex3-step5.png`, `ex3-step6.png`, `ex3-step9.png` … `ex3-step11.png`) with our
//! own features, in mm, **before** the edits of D14.4–D14.6. Axes: the plate lies in the Front
//! plane (sketch x = X, sketch y = Z) and is extruded 8 mm along the Front plane's normal (−Y).
//!
//! - **Main Sketch** (on Front, fully defined, `ex3-step4.png` in its original values): a bar
//!   along the top, its top-left corner on the origin, **225** from its left edge to the large
//!   hole's centre (→ 250 in D14.4), **20** high (the bar height, → 25), with three **Ø5.5** holes
//!   on its middle line, the first **15** from the left edge (the left offset, → 25) and the last
//!   **100** further on (→ 125), the middle one halfway. From the bar an arm runs down to the
//!   right: the large **Ø25.4** hole's centre is **3** below the bar's underside, the **Ø8.25**
//!   hole (→ Ø15) at the arm's end is **46** right of and **50** below it, and the arm ends in an
//!   **R16** arc round it. The arm's upper edge makes **33°** with the vertical through the large
//!   hole, its lower edge **36°** with the horizontal through the end hole (the bar's underside is parallel to it); both are tangent to the R16 arc. A
//!   small circle, equal to the Ø8.25 hole, sits **29** left of the large hole at its height (it is
//!   deleted in D14.4).
//! - **Extrude 1**: the plate, 8 mm (New): the part **Handle Plate**.
//! - **Fillet 1**: **R30** on the corner where the bar's top meets the arm's upper edge. The
//!   course's Handle sheet shows an R30.00 whose source the visible sketch doesn't show; the
//!   fillet is our assumption (recorded in the gap list).
//! - **Sketch 2 / Extrude 2**: the grip bar, a **Ø25** circle (two half circles) on Right round the axis through the
//!   plate's middle (y −4, z −12.5), New, **Blind 10** towards −X and a **second end** 160 towards
//!   +X (→ 175 in D14.5): the part **Handle Grip**.
//! - **Sketch 3 / Extrude 3**: the slot the plate sits in, 8 wide (y −8…0), removed from the grip
//!   only, from x 0 on (the 10 mm lip at the end stays).
//! - **Plane 1**, **Sketch 4**, **Hole 1**: three points on a plane 20 in front of Front at
//!   x 25, 87.5 and 150 (the plate's holes after the edit), z −12.5, and a Hole feature: ISO
//!   Counterbore, Clearance **M5** Normal (Ø5.5, ⌴Ø9.75 ↧5), Through all, from the part, merge
//!   scope Handle Grip (→ M6: Ø6.6, ⌴Ø11.25 ↧6 in D14.6).
//!
//! [`assembly`] adds the Hydraulic Brake Unit (P3C.5) and [`drawing`] the finished "Hand Brake
//! Drawing" with its sheets Assembly, Handle and Grip, as `ex3-drawing.png` shows them. Every id
//! is fixed, so the fixture is regenerated exactly (see `cadrs_core/tests/hand_brake.rs`).

use cadrs_sketch::constraint::{ConstraintOf, CurveSpec, Orient, PointSpec};
use cadrs_sketch::{CurveId, CurveKind, Dimension, DimensionKind, PlaneRef, PointId, Sketch, SketchOp, Vec2};

use crate::applied::{EdgeOrFace, FilletFeature, HoleFeature, HolePoint};
use crate::command::{CommandError, History};
use crate::commands::{AddFeature, AddSketch, EditSketch, RenameFeature, RenamePart};
use crate::document::{BooleanOp, Document, EdgeRef, EndCondition, EndType, FeatureKind};
use crate::hole::{Fit, HoleEnd, HoleSpec, HoleStandard, HoleStart, HoleStyle, HoleType};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::plane::{PlaneEntity, PlaneFeature, PlaneType};
use crate::samples::drawing_bracket::extrude;
use crate::samples::gear_cover::{DocHistory, Studio};

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x4a2d_b2a7_0000_0000_0000_0000_0000_0000 | n)
}

pub const MAIN_SKETCH: FeatureId = id(1);
pub const EXTRUDE_1: FeatureId = id(2);
pub const FILLET_1: FeatureId = id(3);
pub const SKETCH_2: FeatureId = id(4);
pub const EXTRUDE_2: FeatureId = id(5);
pub const SKETCH_3: FeatureId = id(6);
pub const EXTRUDE_3: FeatureId = id(7);
pub const PLANE_1: FeatureId = id(8);
pub const SKETCH_4: FeatureId = id(9);
pub const HOLE_1: FeatureId = id(10);

/// The plate and the grip.
pub const PLATE: PartId = PartId::new(EXTRUDE_1, 0);
pub const GRIP: PartId = PartId::new(EXTRUDE_2, 0);
pub const PLATE_NAME: &str = "Handle Plate";
pub const GRIP_NAME: &str = "Handle Grip";

/// The document's and the studio's ids and names.
pub const DOCUMENT_NAME: &str = "Hand Brake Update (stand-in)";
pub const STUDIO_NAME: &str = "Handle";
pub const DRAWING_NAME: &str = "Hand Brake Drawing";

/// The Main Sketch's values that D14.4 changes: before and after.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MainValues {
    /// From the bar's left edge to the large hole's centre (225 → 250).
    pub length: f64,
    /// The bar's height (20 → 25).
    pub bar: f64,
    /// From the left edge to the first Ø5.5 hole (15 → 25).
    pub offset: f64,
    /// From the first Ø5.5 hole to the last (100 → 125).
    pub spacing: f64,
    /// The hole at the arm's end (Ø8.25 → Ø15).
    pub end_hole: f64,
}

pub const BEFORE: MainValues = MainValues { length: 225.0, bar: 20.0, offset: 15.0, spacing: 100.0, end_hole: 8.25 };
pub const AFTER: MainValues = MainValues { length: 250.0, bar: 25.0, offset: 25.0, spacing: 125.0, end_hole: 15.0 };

// The values D14.4 keeps.
pub const SMALL_HOLE: f64 = 5.5;
pub const LARGE_HOLE: f64 = 25.4;
/// The large hole's centre below the bar's underside.
pub const DROP: f64 = 3.0;
/// The end hole from the large hole: right and down.
pub const END_DX: f64 = 46.0;
pub const END_DY: f64 = 50.0;
pub const END_R: f64 = 16.0;
/// The upper edge's angle to the vertical, the lower edge's to the bar's underside (degrees).
pub const UPPER_ANGLE: f64 = 33.0;
pub const LOWER_ANGLE: f64 = 36.0;
/// The small circle, left of the large hole.
pub const SMALL_DX: f64 = 29.0;
/// Extrude 1.
pub const THICKNESS: f64 = 8.0;
/// Fillet 1.
pub const FILLET_R: f64 = 30.0;
/// The grip: diameter, its axis (y, z), the first end (−X) and the second end (+X) before and
/// after D14.5.
pub const GRIP_D: f64 = 25.0;
pub const GRIP_Y: f64 = -THICKNESS / 2.0;
pub const GRIP_Z: f64 = -12.5;
pub const GRIP_FIRST: f64 = 10.0;
pub const GRIP_SECOND_BEFORE: f64 = 160.0;
pub const GRIP_SECOND_AFTER: f64 = 175.0;
/// The grip's holes (x) and Plane 1's offset from Front.
pub const GRIP_HOLES: [f64; 3] = [25.0, 87.5, 150.0];
pub const PLANE_OFFSET: f64 = 20.0;
/// Hole 1's size before and after D14.6.
pub const HOLE_SIZE_BEFORE: &str = "M5";
pub const HOLE_SIZE_AFTER: &str = "M6";

/// The Main Sketch's points for `m` (sketch coordinates, mm).
#[derive(Debug, Clone, Copy)]
pub struct MainGeometry {
    /// Top-left (the origin), bottom-left, top-right corner, upper edge's tangent point,
    /// lower edge's tangent point, where the lower edge meets the bar's underside.
    pub p0: Vec2,
    pub p1: Vec2,
    pub p2: Vec2,
    pub p3: Vec2,
    pub p4: Vec2,
    pub p5: Vec2,
    /// The large hole, the end hole, the small circle.
    pub c1: Vec2,
    pub c2: Vec2,
    pub c0: Vec2,
    /// The three Ø5.5 holes and the left edge's midpoint.
    pub holes: [Vec2; 3],
    pub mid_left: Vec2,
    /// Where the vertical through the large hole meets the bar's top.
    pub top: Vec2,
    /// Where the horizontal through the end hole meets the lower edge (the 36° is measured
    /// there, below the arm).
    pub low: Vec2,
}

impl MainGeometry {
    pub fn of(m: MainValues) -> Self {
        let v = Vec2::new;
        let c1 = v(m.length, -m.bar - DROP);
        let c2 = v(c1.x + END_DX, c1.y - END_DY);
        let (su, cu) = UPPER_ANGLE.to_radians().sin_cos();
        let (sl, cl) = LOWER_ANGLE.to_radians().sin_cos();
        // Upper edge: normal (cos, sin) towards the outside, END_R from the end hole's centre.
        let p3 = v(c2.x + END_R * cu, c2.y + END_R * su);
        let p2 = v(c2.x + (END_R + su * c2.y) / cu, 0.0);
        // Lower edge: normal (−sin, −cos).
        let p4 = v(c2.x - END_R * sl, c2.y - END_R * cl);
        let p5 = v(c2.x - (END_R + cl * (-m.bar - c2.y)) / sl, -m.bar);
        let y = -m.bar / 2.0;
        Self {
            p0: v(0.0, 0.0),
            p1: v(0.0, -m.bar),
            p2,
            p3,
            p4,
            p5,
            c1,
            c2,
            c0: v(c1.x - SMALL_DX, c1.y),
            holes: [v(m.offset, y), v(m.offset + m.spacing / 2.0, y), v(m.offset + m.spacing, y)],
            mid_left: v(0.0, y),
            top: v(c1.x, 0.0),
            low: v(c2.x - END_R / sl, c2.y),
        }
    }
}

fn line(points: Vec<Vec2>, construction: bool) -> SketchOp {
    SketchOp::AddPolyline { points, closed: false, construction, label: "Add line" }
}

fn circle(c: Vec2, d: f64) -> SketchOp {
    SketchOp::AddCircle { center: c, radius: d / 2.0, construction: false }
}

fn circle_id(g: &Sketch, c: Vec2, r: f64) -> Option<CurveId> {
    g.curves.iter().find_map(|(id, k)| match k.kind {
        CurveKind::Circle { center, radius } if (radius - r).abs() < 1e-6 && g.pos(center).distance(c) < 1e-6 => Some(id),
        _ => None,
    })
}

fn arc_id(g: &Sketch) -> Option<CurveId> {
    g.curves.iter().find_map(|(id, k)| matches!(k.kind, CurveKind::Arc { .. }).then_some(id))
}

fn line_id(g: &Sketch, a: Vec2, b: Vec2) -> Option<CurveId> {
    g.curves.iter().find_map(|(id, k)| match k.kind {
        CurveKind::Line { a: s, b: e } if g.pos(s).distance(a) < 1e-6 && g.pos(e).distance(b) < 1e-6 => Some(id),
        _ => None,
    })
}

/// The Main Sketch's geometry (`m`'s values), without constraints.
pub fn main_geometry(m: MainValues) -> SketchOp {
    let g = MainGeometry::of(m);
    let mut ops = vec![
        // The outline: underside, left edge, top, upper edge; the lower edge; the R16 arc
        // (counter-clockwise from the lower edge's end round the bottom to the upper edge's).
        line(vec![g.p5, g.p1, g.p0, g.p2, g.p3], false),
        line(vec![g.p4, g.p5], false),
        SketchOp::AddArc { center: g.c2, start: g.p4, end: g.p3, construction: false },
        circle(g.c1, LARGE_HOLE),
        circle(g.c2, m.end_hole),
        circle(g.c0, m.end_hole),
    ];
    for h in g.holes {
        ops.push(circle(h, SMALL_HOLE));
    }
    // Construction: the vertical through the large hole, the holes' middle line, the left
    // edge's midpoint.
    ops.push(line(vec![g.c1, g.top], true));
    ops.push(line(vec![g.holes[0], g.holes[2]], true));
    ops.push(line(vec![g.c2, g.low], true));
    ops.push(SketchOp::AddPoint { pos: g.mid_left });
    SketchOp::Batch(ops)
}

/// The constraints and dimensions that fully define the Main Sketch (`g` holds its geometry).
pub fn main_constraints(g: &Sketch, m: MainValues) -> Result<SketchOp, CommandError> {
    let p = MainGeometry::of(m);
    let at = PointSpec::At;
    let between = CurveSpec::Between;
    let small = m.end_hole / 2.0;
    let specs = vec![
        ConstraintOf::Coincident(at(p.p0), PointSpec::Origin),
        ConstraintOf::Vertical(Orient::Line(between(p.p1, p.p0))),
        ConstraintOf::Horizontal(Orient::Line(between(p.p0, p.p2))),
        ConstraintOf::Horizontal(Orient::Line(between(p.p5, p.p1))),
        ConstraintOf::Tangent(between(p.p2, p.p3), between(p.p4, p.p3)),
        ConstraintOf::Tangent(between(p.p4, p.p5), between(p.p4, p.p3)),
        ConstraintOf::Horizontal(Orient::Points(at(p.c0), at(p.c1))),
        ConstraintOf::Equal(CurveSpec::Circle(p.c0, small), CurveSpec::Circle(p.c2, small)),
        ConstraintOf::Vertical(Orient::Line(between(p.c1, p.top))),
        ConstraintOf::PointOnCurve(at(p.top), between(p.p0, p.p2)),
        ConstraintOf::Midpoint(at(p.mid_left), between(p.p1, p.p0)),
        ConstraintOf::Horizontal(Orient::Line(between(p.holes[0], p.holes[2]))),
        ConstraintOf::Horizontal(Orient::Points(at(p.holes[0]), at(p.mid_left))),
        ConstraintOf::Midpoint(at(p.holes[1]), between(p.holes[0], p.holes[2])),
        ConstraintOf::Equal(CurveSpec::Circle(p.holes[1], SMALL_HOLE / 2.0), CurveSpec::Circle(p.holes[0], SMALL_HOLE / 2.0)),
        ConstraintOf::Equal(CurveSpec::Circle(p.holes[2], SMALL_HOLE / 2.0), CurveSpec::Circle(p.holes[0], SMALL_HOLE / 2.0)),
        ConstraintOf::Horizontal(Orient::Line(between(p.c2, p.low))),
        ConstraintOf::PointOnCurve(at(p.low), between(p.p4, p.p5)),
    ];
    let missing = |what: &str| CommandError::Invalid(format!("Main Sketch: no {what}"));
    let pt = |q: Vec2, what: &str| -> Result<PointId, CommandError> { g.point_at(q, 1e-6).ok_or_else(|| missing(what)) };
    let dim = |kind: DimensionKind, value: f64, offset: f64| SketchOp::SetDimension {
        dimension: Dimension { kind, value, offset, along: 0.0, driven: false },
        moves: vec![],
        radii: vec![],
    };
    let (p0, p1) = (pt(p.p0, "top-left corner")?, pt(p.p1, "bottom-left corner")?);
    let p5 = pt(p.p5, "step")?;
    let (c1, c2, c0) = (pt(p.c1, "large hole")?, pt(p.c2, "end hole")?, pt(p.c0, "small circle")?);
    let (h0, h2) = (pt(p.holes[0], "first hole")?, pt(p.holes[2], "last hole")?);
    let large = circle_id(g, p.c1, LARGE_HOLE / 2.0).ok_or_else(|| missing("Ø25.4 circle"))?;
    let end = circle_id(g, p.c2, small).ok_or_else(|| missing("end hole circle"))?;
    let first = circle_id(g, p.holes[0], SMALL_HOLE / 2.0).ok_or_else(|| missing("Ø5.5 circle"))?;
    let arc = arc_id(g).ok_or_else(|| missing("R16 arc"))?;
    let vertical = line_id(g, p.c1, p.top).ok_or_else(|| missing("vertical"))?;
    let upper = line_id(g, p.p2, p.p3).ok_or_else(|| missing("upper edge"))?;
    let level = line_id(g, p.c2, p.low).ok_or_else(|| missing("horizontal through the end hole"))?;
    let lower = line_id(g, p.p4, p.p5).ok_or_else(|| missing("lower edge"))?;
    use cadrs_sketch::constraint::CurveRef as C;
    let d45 = std::f64::consts::FRAC_PI_4;
    let ops = vec![
        SketchOp::AddConstraints(specs),
        dim(DimensionKind::Horizontal { a: p0, b: c1 }, m.length, 22.0),
        dim(DimensionKind::Vertical { a: p0, b: p1 }, m.bar, -14.0),
        dim(DimensionKind::Horizontal { a: p0, b: h0 }, m.offset, 12.0),
        dim(DimensionKind::Horizontal { a: h0, b: h2 }, m.spacing, 12.0 + m.bar / 2.0),
        dim(DimensionKind::Diameter { curve: first }, SMALL_HOLE, -2.0 * d45),
        dim(DimensionKind::Diameter { curve: large }, LARGE_HOLE, d45),
        dim(DimensionKind::Diameter { curve: end }, m.end_hole, d45),
        dim(DimensionKind::Radius { curve: arc }, END_R, -d45),
        // The large hole's drop below the bar's underside, measured from the step where the
        // underside ends (clear of the bar height's 20 at the left).
        dim(DimensionKind::Vertical { a: p5, b: c1 }, DROP, 10.0),
        dim(DimensionKind::Horizontal { a: c0, b: c1 }, SMALL_DX, -30.0),
        dim(DimensionKind::Horizontal { a: c1, b: c2 }, END_DX, -70.0),
        dim(DimensionKind::Vertical { a: c1, b: c2 }, END_DY, 70.0),
        dim(
            DimensionKind::Angle { a: C::Curve(vertical), b: C::Curve(upper), flip_a: true, flip_b: false },
            UPPER_ANGLE,
            40.0,
        ),
        dim(
            // Between the lower edge and the horizontal through the end hole, below the arm
            // (the 36° of `ex3-step4.png`, outside the plate).
            DimensionKind::Angle { a: C::Curve(level), b: C::Curve(lower), flip_a: false, flip_b: false },
            LOWER_ANGLE,
            40.0,
        ),
    ];
    Ok(SketchOp::Batch(ops))
}

fn sketch_geometry(s: &dyn Studio, el: ElementId, f: FeatureId) -> Result<Sketch, CommandError> {
    s.document()
        .element(el)
        .and_then(|e| e.feature(f))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
}

fn rename(s: &mut dyn Studio, el: ElementId, f: FeatureId, name: &str) -> Result<(), CommandError> {
    s.run(&RenameFeature { element: el, feature: f, name: name.into() })
}

/// The edge of part `part` nearest `p` (mm).
fn edge_at(s: &dyn Studio, el: ElementId, part: PartId, p: [f64; 3]) -> Result<EdgeRef, CommandError> {
    let features = s
        .document()
        .element(el)
        .map(|e| e.features().to_vec())
        .ok_or_else(|| CommandError::Invalid("studio not found".into()))?;
    let build = crate::rebuild::build(&features);
    let q = build
        .parts
        .iter()
        .find(|q| q.id == part)
        .ok_or_else(|| CommandError::Invalid(format!("the part did not build: {:?}", build.errors)))?;
    let e = q
        .solid
        .edges
        .iter()
        .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
        .filter(|e| e.distance(p) < 1e-3)
        .ok_or_else(|| CommandError::Invalid(format!("no edge at {p:?}")))?;
    Ok(EdgeRef { part, edge: e.name, seed: p })
}

/// Hole 1's spec: ISO counterbore, clearance `size` Normal, through all.
pub fn hole_spec(size: &str) -> HoleSpec {
    let mut spec = HoleSpec {
        standard: HoleStandard::Iso,
        style: HoleStyle::Counterbore,
        hole_type: HoleType::Clearance,
        size: size.into(),
        fit: Fit::Normal,
        start: HoleStart::Part,
        end: HoleEnd::ThroughAll,
        ..HoleSpec::default()
    };
    spec.apply_table();
    spec
}

/// Adds the Handle's features to the Part Studio `el` of `doc`.
pub fn build(doc: &mut Document, h: &mut History, el: ElementId) -> Result<(), CommandError> {
    build_in(&mut DocHistory(doc, h), el)
}

/// [`build`] through any [`Studio`].
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let v = Vec2::new;
    let m = BEFORE;
    let g = MainGeometry::of(m);
    // Main Sketch, fully defined.
    s.run(&AddSketch { element: el, feature: MAIN_SKETCH, plane: Some(PlaneRef::Front) })?;
    s.run(&EditSketch { element: el, feature: MAIN_SKETCH, op: main_geometry(m) })?;
    let op = main_constraints(&sketch_geometry(s, el, MAIN_SKETCH)?, m)?;
    s.run(&EditSketch { element: el, feature: MAIN_SKETCH, op })?;
    rename(s, el, MAIN_SKETCH, "Main Sketch")?;
    // Extrude 1: the plate (a point in the bar left of the first hole).
    extrude(s, el, MAIN_SKETCH, EXTRUDE_1, &[v(3.0, -3.0)], |e| {
        e.depth = THICKNESS;
        e.depth_expr = "8 mm".into();
    })?;
    // Fillet 1: R30 on the corner at the top of the upper edge (an edge along Y).
    let corner = edge_at(s, el, PLATE, [g.p2.x, -THICKNESS / 2.0, 0.0])?;
    s.run(&AddFeature::fillet(
        el,
        FILLET_1,
        FilletFeature {
            entities: vec![EdgeOrFace::Edge(corner)],
            size: FILLET_R,
            size_expr: "30 mm".into(),
            ..FilletFeature::default()
        },
    ))?;
    // Sketch 2 / Extrude 2: the grip bar, on Right (sketch x = Y, y = Z).
    s.run(&AddSketch { element: el, feature: SKETCH_2, plane: Some(PlaneRef::Right) })?;
    // Two half circles meeting at 45° and 225° (not one circle): a circle's seam lies along its
    // sketch's +x, which is the grip's silhouette in the Top view, and OCCT's hidden-line removal
    // loses pieces of an outline that runs along a seam.
    let c = v(GRIP_Y, GRIP_Z);
    let (a, b) = {
        let d = GRIP_D / 2.0 * std::f64::consts::FRAC_1_SQRT_2;
        (v(c.x + d, c.y + d), v(c.x - d, c.y - d))
    };
    s.run(&EditSketch {
        element: el,
        feature: SKETCH_2,
        op: SketchOp::Batch(vec![
            SketchOp::AddArc { center: c, start: a, end: b, construction: false },
            SketchOp::AddArc { center: c, start: b, end: a, construction: false },
        ]),
    })?;
    extrude(s, el, SKETCH_2, EXTRUDE_2, &[v(GRIP_Y, GRIP_Z)], |e| {
        e.flip = true;
        e.depth = GRIP_FIRST;
        e.depth_expr = "10 mm".into();
        e.second = Some(EndCondition {
            end: EndType::Blind,
            depth: GRIP_SECOND_BEFORE,
            depth_expr: "160 mm".into(),
            ..EndCondition::default()
        });
    })?;
    // Sketch 3 / Extrude 3: the slot, on Top (sketch x = X, y = Y), removed from the grip.
    s.run(&AddSketch { element: el, feature: SKETCH_3, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch {
        element: el,
        feature: SKETCH_3,
        op: SketchOp::AddPolyline {
            points: vec![v(0.0, -THICKNESS), v(220.0, -THICKNESS), v(220.0, 0.0), v(0.0, 0.0)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
    })?;
    extrude(s, el, SKETCH_3, EXTRUDE_3, &[v(100.0, -THICKNESS / 2.0)], |e| {
        e.op = BooleanOp::Remove;
        e.merge_scope = vec![GRIP];
        e.symmetric = true;
        e.depth = 60.0;
        e.depth_expr = "60 mm".into();
    })?;
    // Plane 1: Front, 20 towards −Y.
    s.run(&AddFeature {
        element: el,
        feature: PLANE_1,
        base_name: "Plane".into(),
        kind: FeatureKind::Plane(PlaneFeature {
            kind: PlaneType::Offset,
            entities: vec![PlaneEntity::Plane(PlaneRef::Front)],
            offset: PLANE_OFFSET,
            offset_expr: "20 mm".into(),
            ..PlaneFeature::default()
        }),
    })?;
    let features = s.document().element(el).map(|e| e.features().to_vec()).unwrap_or_default();
    let plane = crate::parts::plane_feature_ref(&features, PLANE_1).ok_or_else(|| CommandError::Invalid("Plane 1 did not build".into()))?;
    // Sketch 4 / Hole 1.
    s.run(&AddSketch { element: el, feature: SKETCH_4, plane: Some(plane) })?;
    s.run(&EditSketch {
        element: el,
        feature: SKETCH_4,
        op: SketchOp::Batch(GRIP_HOLES.iter().map(|x| SketchOp::AddPoint { pos: v(*x, GRIP_Z) }).collect()),
    })?;
    let points = sketch_geometry(s, el, SKETCH_4)?.points.keys().map(|point| HolePoint { sketch: SKETCH_4, point }).collect();
    s.run(&AddFeature::hole(
        el,
        HOLE_1,
        HoleFeature { points, merge_scope: vec![GRIP], spec: hole_spec(HOLE_SIZE_BEFORE), renamed: true, ..HoleFeature::default() },
    ))?;
    rename(s, el, HOLE_1, "Hole 1")?;
    // The course's names (the Main Sketch was Sketch 1).
    rename(s, el, SKETCH_2, "Sketch 2")?;
    rename(s, el, SKETCH_3, "Sketch 3")?;
    rename(s, el, SKETCH_4, "Sketch 4")?;
    s.run(&RenamePart { element: el, part: PLATE, name: PLATE_NAME.into() })?;
    s.run(&RenamePart { element: el, part: GRIP, name: GRIP_NAME.into() })?;
    Ok(())
}

/// The course's edits of the Handle studio (D14.4–D14.6) through the command layer, as the
/// scenario makes them in the dialogs: the Main Sketch's 225 → 250, 100 → 125, bar 20 → 25,
/// offset 15 → 25, Ø8.25 → Ø15 and the small circle deleted; Extrude 2's second end 160 → 175;
/// Hole 1 M5 → M6.
pub fn course_edits(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let bad = |w: &str| CommandError::Invalid(format!("course edits: {w}"));
    for (from, to, dia) in [
        (BEFORE.length, AFTER.length, false),
        (BEFORE.spacing, AFTER.spacing, false),
        (BEFORE.bar, AFTER.bar, false),
        (BEFORE.offset, AFTER.offset, false),
        (BEFORE.end_hole, AFTER.end_hole, true),
    ] {
        let g = sketch_geometry(s, el, MAIN_SKETCH)?;
        let id = g
            .dimensions
            .iter()
            .find(|(_, d)| (d.value - from).abs() < 1e-9 && matches!(d.kind, DimensionKind::Diameter { .. }) == dia)
            .map(|(id, _)| id)
            .ok_or_else(|| bad(&format!("no dimension {from}")))?;
        s.run(&EditSketch { element: el, feature: MAIN_SKETCH, op: SketchOp::SetDimensionValue { id, value: to } })?;
    }
    // The small circle: the circle left of the large hole at its height.
    let g = sketch_geometry(s, el, MAIN_SKETCH)?;
    let p = MainGeometry::of(AFTER);
    let small = circle_id(&g, p.c0, AFTER.end_hole / 2.0).ok_or_else(|| bad("no small circle"))?;
    s.run(&EditSketch {
        element: el,
        feature: MAIN_SKETCH,
        op: SketchOp::Delete { curves: vec![small], points: vec![], dimensions: vec![], constraints: vec![] },
    })?;
    let features = s.document().element(el).map(|e| e.features().to_vec()).unwrap_or_default();
    let mut e = features
        .iter()
        .find(|f| f.id == EXTRUDE_2)
        .and_then(|f| f.extrude())
        .cloned()
        .ok_or_else(|| bad("no Extrude 2"))?;
    if let Some(second) = &mut e.second {
        second.depth = GRIP_SECOND_AFTER;
        second.depth_expr = "175 mm".into();
    }
    s.run(&crate::commands::SetExtrude { element: el, feature: EXTRUDE_2, extrude: e, label: "Depth".into() })?;
    let mut h = features.iter().find(|f| f.id == HOLE_1).and_then(|f| f.hole()).cloned().ok_or_else(|| bad("no Hole 1"))?;
    h.spec.size = HOLE_SIZE_AFTER.into();
    h.spec.apply_table();
    s.run(&crate::commands::SetFeature { element: el, feature: HOLE_1, kind: FeatureKind::Hole(h), label: "Size".into() })?;
    Ok(())
}

/// The studio's element id in the fixture.
pub const STUDIO: ElementId = ElementId::from_u128(0x4a2d_b2a7_0000_0000_0000_0000_0000_0101);
/// The drawing's element id in the fixture.
pub const DRAWING: ElementId = ElementId::from_u128(0x4a2d_b2a7_0000_0000_0000_0000_0000_0102);

/// A new document holding the stand-in, as `fixtures/hand_brake_standin.cadrs` stores it: the
/// Handle Part Studio, the finished "Hand Brake Drawing", and (P3C.5) the Hydraulic Brake Unit
/// assembly with its studios Master Cylinder, Enclosures and Hardware.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(DOCUMENT_NAME);
    doc.id = crate::ids::DocumentId::from_u128(0x4a2d_b2a7_0000_0000_0000_0000_0000_0100);
    doc.units.length = cadrs_sketch::units::LengthUnit::Millimeter;
    let mut el = crate::document::Element::part_studio(STUDIO_NAME);
    el.id = STUDIO;
    doc.elements.push(el);
    let mut h = History::default();
    build(&mut doc, &mut h, STUDIO)?;
    assembly::build_in(&mut DocHistory(&mut doc, &mut h), STUDIO, STUDIO)?;
    let d = drawing::drawing(&doc, STUDIO, Some(assembly::ASSEMBLY))?;
    let mut del = crate::document::Element::drawing(DRAWING_NAME, d);
    del.id = DRAWING;
    // The tabs: Handle, the drawing, then the assembly and its studios.
    doc.elements.insert(1, del);
    Ok(doc)
}

pub mod assembly;
pub mod drawing;
