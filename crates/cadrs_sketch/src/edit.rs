//! Sketch tools that make new geometry from existing geometry (`intro-to-sketching.md` S19):
//!
//! - **Mirror** ([`mirror`]): copies of curves mirrored in a line, each linked to its source by
//!   a Symmetric constraint, so editing the source moves the copy. Ends on the mirror line are
//!   shared with the copy, and a mirrored end that lands on an existing point shares it. One
//!   that lands on another curve's circle or line near that curve's free end joins that end
//!   (the solver then moves the end onto the image): mirroring an arc that ends on one end of
//!   a symmetric arc joins the copy to its other end, "coincident + tangent" as in
//!   `intro-to-sketching.md` S15 step 11.
//! - **Offset** ([`offset`]): a chain of curves copied at a distance to one side. Lines stay
//!   parallel to their sources; circles and arcs share their source's center. The first piece
//!   gets the driving offset dimension and the others keep the same distance
//!   ([`ConstraintOf::EqualOffset`]); pieces that met still meet.
//! - **Scale** ([`scale`]): the whole sketch about a point (the first dimension of a sketch
//!   scales it, S13.3).

use std::collections::HashMap;

use crate::constraint::{ConstraintOf, CurveRef, PointRef};
use crate::geom::{ArcGeom, mirror_point};
use crate::infer::{circle_circle, line_circle, line_line};
use crate::{
    Curve, CurveId, CurveKind, Dimension, DimensionKind, MERGE_EPS, PointId, Sketch, Vec2,
};

/// Points closer than this (mm) to a mirror line lie on it (and are shared with the copy); a
/// mirrored point this close to an existing point shares it.
const ON_AXIS: f64 = 1e-6;

/// Mirrors `curves` in the line `axis`. Returns the copies (source, copy) in order.
pub fn mirror(s: &mut Sketch, axis: CurveId, curves: &[CurveId]) -> Result<Vec<(CurveId, CurveId)>, String> {
    let Some(CurveKind::Line { a, b }) = s.curves.get(axis).map(|c| c.kind) else {
        return Err("the mirror line is not a line".into());
    };
    let (p0, p1) = (s.pos(a), s.pos(b));
    if p0.distance(p1) < 1e-9 {
        return Err("the mirror line has no length".into());
    }
    let mut map: HashMap<PointId, PointId> = HashMap::new();
    let sources: Vec<CurveId> = curves.to_vec();
    let mut image = |s: &mut Sketch, p: PointId| -> PointId {
        if let Some(q) = map.get(&p) {
            return *q;
        }
        let pos = s.pos(p);
        let m = mirror_point(pos, p0, p1);
        let q = if m.distance(pos) < 2.0 * ON_AXIS {
            p
        } else if let Some(q) = s.point_at(m, ON_AXIS) {
            q
        } else if let Some(q) = free_end_near(s, m, &sources, axis) {
            // The free end slides along its own curve onto the image.
            if let Some(pt) = s.points.get_mut(q) {
                pt.pos = m;
            }
            q
        } else {
            s.add_point(m)
        };
        map.insert(p, q);
        q
    };
    let mut out = Vec::new();
    for &c in curves {
        if c == axis {
            continue;
        }
        let Some(curve) = s.curves.get(c).copied() else {
            continue;
        };
        let mut spline_copy = None;
        let kind = match curve.kind {
            CurveKind::Spline { start, end } => {
                let Some(mut d) = s.splines.get(c).cloned() else { continue };
                let u = (p1 - p0).normalize();
                let reflect = |v: Vec2| u * (2.0 * v.dot(u)) - v;
                d.points = d.points.iter().map(|p| image(s, *p)).collect();
                d.start_tangent = d.start_tangent.map(reflect);
                d.end_tangent = d.end_tangent.map(reflect);
                let k = CurveKind::Spline { start: image(s, start), end: image(s, end) };
                spline_copy = Some(d);
                k
            }
            CurveKind::Line { a, b } => {
                let (ma, mb) = (image(s, a), image(s, b));
                if ma == a && mb == b {
                    // On the mirror line: its own image.
                    continue;
                }
                CurveKind::Line { a: ma, b: mb }
            }
            CurveKind::Circle { center, radius } => CurveKind::Circle {
                center: image(s, center),
                radius,
            },
            // Mirroring turns an arc around: counter-clockwise from the image of its end.
            CurveKind::Arc { center, start, end } => CurveKind::Arc {
                center: image(s, center),
                start: image(s, end),
                end: image(s, start),
            },
            // Mirroring turns the minor axis to the major axis's other side: the same ellipse.
            CurveKind::Ellipse { center, major, minor } => CurveKind::Ellipse {
                center: image(s, center),
                major: image(s, major),
                minor,
            },
            CurveKind::EllipseOffset { center, major, minor, distance } => CurveKind::EllipseOffset {
                center: image(s, center),
                major: image(s, major),
                minor,
                distance,
            },
            // The same ellipse, turned around like an arc.
            CurveKind::EllipseArc { center, major, minor, start, end } => CurveKind::EllipseArc {
                center: image(s, center),
                major: image(s, major),
                minor,
                start: image(s, end),
                end: image(s, start),
            },
            CurveKind::Bezier { a, c1, c2, b } => CurveKind::Bezier {
                a: image(s, a),
                c1: image(s, c1),
                c2: image(s, c2),
                b: image(s, b),
            },
        };
        let copy = s.curves.insert(Curve {
            kind,
            construction: curve.construction,
        });
        if let Some(d) = spline_copy {
            s.splines.insert(copy, d);
        }
        s.add_constraint(ConstraintOf::SymmetricCurves(
            CurveRef::Curve(c),
            CurveRef::Curve(copy),
            CurveRef::Curve(axis),
        ));
        out.push((c, copy));
    }
    if out.is_empty() {
        return Err("nothing to mirror".into());
    }
    Ok(out)
}

/// A free end (used by one curve only) of a line or arc, not being mirrored, whose carrier
/// passes through `m` and which is near it (within a quarter of the curve's size).
fn free_end_near(s: &Sketch, m: Vec2, skip: &[CurveId], axis: CurveId) -> Option<PointId> {
    let tol = 1e-6 * (1.0 + m.length());
    let mut best: Option<(PointId, f64)> = None;
    for (k, c) in &s.curves {
        if skip.contains(&k) || k == axis {
            continue;
        }
        let (on_carrier, size) = match c.kind {
            CurveKind::Line { a, b } => {
                let (pa, pb) = (s.pos(a), s.pos(b));
                let f = crate::dimension::foot(m, pa, pb);
                (f.distance(m) < tol, pa.distance(pb))
            }
            CurveKind::Arc { .. } => match s.arc_geom(k) {
                Some(g) => ((m.distance(g.center) - g.radius).abs() < tol, g.radius),
                None => continue,
            },
            CurveKind::Circle { .. }
            | CurveKind::Ellipse { .. }
            | CurveKind::EllipseOffset { .. }
            | CurveKind::EllipseArc { .. }
            | CurveKind::Spline { .. }
            | CurveKind::Bezier { .. } => continue,
        };
        if !on_carrier {
            continue;
        }
        let Some((a, b)) = s.curve_ends(k) else {
            continue;
        };
        for e in [a, b] {
            let d = s.pos(e).distance(m);
            if s.curves_at(e).count() == 1 && d < 0.25 * size && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((e, d));
            }
        }
    }
    best.map(|(e, _)| e)
}

/// The chain through `curve`: the lines and arcs joined end to end with it (through points
/// where exactly two of them meet), in order, each with whether it runs backwards (from its
/// end to its start). A circle is a chain of its own. Also returns true if the chain is closed.
pub fn chain_of(s: &Sketch, curve: CurveId) -> (Vec<(CurveId, bool)>, bool) {
    let Some(c0) = s.curves.get(curve) else {
        return (Vec::new(), false);
    };
    let Some((a0, b0)) = s.curve_ends(curve) else {
        return (vec![(curve, false)], true);
    };
    let construction = c0.construction;
    // The single other chain curve at a point, if exactly one continues there.
    let next_at = |p: PointId, from: CurveId| -> Option<CurveId> {
        let others: Vec<CurveId> = s
            .curves_at(p)
            .filter(|k| {
                *k != from
                    && s.curve_ends(*k).is_some_and(|(x, y)| x == p || y == p)
                    && s.curves[*k].construction == construction
            })
            .collect();
        (others.len() == 1).then(|| others[0])
    };
    // Walk forward from the end, then backward from the start.
    let mut forward: Vec<(CurveId, bool)> = vec![(curve, false)];
    let (mut at, mut from) = (b0, curve);
    let mut closed = false;
    while let Some(k) = next_at(at, from) {
        if k == curve {
            closed = true;
            break;
        }
        if forward.iter().any(|(x, _)| *x == k) {
            break;
        }
        let (x, y) = s.curve_ends(k).unwrap_or((at, at));
        let reversed = y == at;
        forward.push((k, reversed));
        at = if reversed { x } else { y };
        from = k;
    }
    if closed {
        return (forward, true);
    }
    let mut backward: Vec<(CurveId, bool)> = Vec::new();
    let (mut at, mut from) = (a0, curve);
    while let Some(k) = next_at(at, from) {
        if k == curve || forward.iter().chain(&backward).any(|(x, _)| *x == k) {
            break;
        }
        let (x, y) = s.curve_ends(k).unwrap_or((at, at));
        // Walking backwards: the curve should end where we are to run forwards.
        let reversed = x == at;
        backward.push((k, reversed));
        at = if reversed { y } else { x };
        from = k;
    }
    backward.reverse();
    backward.extend(forward);
    (backward, false)
}

/// One offset piece before it is added to the sketch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OffsetShape {
    /// From the start to the end of the chain's direction of travel.
    Line(Vec2, Vec2),
    /// Counter-clockwise, around the source's center point.
    Arc {
        center: PointId,
        geom: ArcGeom,
    },
    Circle {
        center: PointId,
        radius: f64,
    },
    /// An ellipse offset (P3.7): the ellipse with the source's center and major point and this
    /// minor radius, `distance` outside it.
    Ellipse {
        center: PointId,
        major: PointId,
        minor: f64,
        distance: f64,
    },
}

impl OffsetShape {
    /// Points along it (for previews).
    pub fn polyline(&self, s: &Sketch) -> Vec<Vec2> {
        match *self {
            OffsetShape::Line(a, b) => vec![a, b],
            OffsetShape::Arc { geom, .. } => geom.tessellate(std::f64::consts::PI / 90.0, 8),
            OffsetShape::Circle { center, radius } => ArcGeom {
                center: s.pos(center),
                radius,
                start_angle: 0.0,
                sweep: std::f64::consts::TAU,
            }
            .tessellate(std::f64::consts::PI / 90.0, 16),
            OffsetShape::Ellipse { center, major, minor, distance } => {
                crate::geom::EllipseGeom::new(s.pos(center), s.pos(major), minor)
                    .with_offset(distance)
                    .tessellate(std::f64::consts::PI / 90.0, 16)
            }
        }
    }
}

/// The offset of a chain (from [`chain_of`]) by `distance`, to the left of its direction of
/// travel (or the right): one shape per piece, with neighbouring pieces meeting where their
/// offsets cross (or touch).
pub fn offset_geometry(
    s: &Sketch,
    chain: &[(CurveId, bool)],
    distance: f64,
    left: bool,
) -> Result<Vec<OffsetShape>, String> {
    if !(distance.is_finite() && distance > 0.0) {
        return Err("an offset must be positive".into());
    }
    let side = if left { distance } else { -distance };
    let mut out: Vec<OffsetShape> = Vec::new();
    for &(c, reversed) in chain {
        let curve = s.curves.get(c).ok_or("the curve is gone")?;
        out.push(match curve.kind {
            CurveKind::Line { a, b } => {
                let (p, q) = if reversed { (s.pos(b), s.pos(a)) } else { (s.pos(a), s.pos(b)) };
                let n = (q - p).normalize().perp();
                OffsetShape::Line(p + n * side, q + n * side)
            }
            CurveKind::Arc { center, .. } => {
                let g = s.arc_geom(c).ok_or("not an arc")?;
                // Running counter-clockwise, left is toward the center.
                let r = if reversed { g.radius + side } else { g.radius - side };
                if r <= 1e-9 {
                    return Err("the offset is larger than the arc's radius".into());
                }
                OffsetShape::Arc {
                    center,
                    geom: ArcGeom { radius: r, ..g },
                }
            }
            CurveKind::Circle { center, radius } => {
                let r = if reversed { radius + side } else { radius - side };
                if r <= 1e-9 {
                    return Err("the offset is larger than the circle's radius".into());
                }
                OffsetShape::Circle { center, radius: r }
            }
            // P3.7: an ellipse's offset is an offset curve of the same ellipse (not an ellipse);
            // offsetting an offset ellipse offsets that ellipse further.
            CurveKind::Ellipse { center, major, minor } | CurveKind::EllipseOffset { center, major, minor, .. } => {
                let d0 = match curve.kind {
                    CurveKind::EllipseOffset { distance, .. } => distance,
                    _ => 0.0,
                };
                // The parameter runs counter-clockwise for a positive minor radius; left of a
                // counter-clockwise run is inside.
                let ccw = minor > 0.0;
                let d = if ccw != reversed { d0 - side } else { d0 + side };
                let g = crate::geom::EllipseGeom::new(s.pos(center), s.pos(major), minor).with_offset(d);
                if !g.offset_is_smooth() {
                    return Err("the offset is larger than the ellipse's smallest radius of curvature".into());
                }
                OffsetShape::Ellipse { center, major, minor, distance: d }
            }
            CurveKind::Spline { .. } => return Err("a spline can't be offset yet".into()),
            CurveKind::Bezier { .. } => return Err("a Bézier curve can't be offset".into()),
            CurveKind::EllipseArc { .. } => return Err("an elliptical arc can't be offset yet".into()),
        });
    }
    // Where consecutive pieces meet (chains of lines and arcs).
    let n = out.len();
    let closed = n > 1 && {
        let first = s.curve_ends(chain[0].0);
        let last = s.curve_ends(chain[n - 1].0);
        let start_of = |e: Option<(PointId, PointId)>, rev: bool| e.map(|(a, b)| if rev { b } else { a });
        let end_of = |e: Option<(PointId, PointId)>, rev: bool| e.map(|(a, b)| if rev { a } else { b });
        start_of(first, chain[0].1).is_some() && start_of(first, chain[0].1) == end_of(last, chain[n - 1].1)
    };
    let joints = if closed { n } else { n.saturating_sub(1) };
    for i in 0..joints {
        let j = (i + 1) % n;
        let (end_i, start_j) = (end_of_shape(&out[i], s), start_of_shape(&out[j], s));
        let (Some(e), Some(st)) = (end_i, start_j) else {
            continue;
        };
        let meet = if e.distance(st) < 1e-9 {
            e
        } else {
            meeting_point(s, &out[i], &out[j], e.midpoint(st)).unwrap_or(e.midpoint(st))
        };
        set_end(&mut out[i], s, meet);
        set_start(&mut out[j], s, meet);
    }
    Ok(out)
}

fn start_of_shape(sh: &OffsetShape, _s: &Sketch) -> Option<Vec2> {
    match *sh {
        OffsetShape::Line(a, _) => Some(a),
        OffsetShape::Arc { geom, .. } => Some(geom.start()),
        OffsetShape::Circle { .. } | OffsetShape::Ellipse { .. } => None,
    }
}

fn end_of_shape(sh: &OffsetShape, _s: &Sketch) -> Option<Vec2> {
    match *sh {
        OffsetShape::Line(_, b) => Some(b),
        OffsetShape::Arc { geom, .. } => Some(geom.end()),
        OffsetShape::Circle { .. } | OffsetShape::Ellipse { .. } => None,
    }
}

/// Where two offset pieces' carriers cross, nearest `near`.
fn meeting_point(s: &Sketch, a: &OffsetShape, b: &OffsetShape, near: Vec2) -> Option<Vec2> {
    enum C {
        L(Vec2, Vec2),
        O(Vec2, f64),
    }
    let carrier = |sh: &OffsetShape| match *sh {
        OffsetShape::Line(p, q) => Some(C::L(p, (q - p).normalize())),
        OffsetShape::Arc { geom, .. } => Some(C::O(geom.center, geom.radius)),
        OffsetShape::Circle { center, radius } => Some(C::O(s.pos(center), radius)),
        OffsetShape::Ellipse { .. } => None,
    };
    let pts: Vec<Vec2> = match (carrier(a)?, carrier(b)?) {
        (C::L(p, d), C::L(q, e)) => line_line(p, d, q, e).into_iter().collect(),
        (C::L(p, d), C::O(c, r)) | (C::O(c, r), C::L(p, d)) => line_circle(p, d, c, r),
        (C::O(c0, r0), C::O(c1, r1)) => circle_circle(c0, r0, c1, r1),
    };
    pts.into_iter()
        .min_by(|x, y| x.distance(near).total_cmp(&y.distance(near)))
}

fn set_start(sh: &mut OffsetShape, _s: &Sketch, p: Vec2) {
    match sh {
        OffsetShape::Line(a, _) => *a = p,
        OffsetShape::Arc { geom, .. } => {
            let a0 = geom.start_angle;
            let a1 = a0 + geom.sweep;
            let new0 = (p - geom.center).angle();
            let sweep = crate::geom::norm_angle(a1 - new0);
            geom.start_angle = new0;
            geom.sweep = if sweep < 1e-9 { std::f64::consts::TAU } else { sweep };
        }
        OffsetShape::Circle { .. } | OffsetShape::Ellipse { .. } => {}
    }
}

fn set_end(sh: &mut OffsetShape, _s: &Sketch, p: Vec2) {
    match sh {
        OffsetShape::Line(_, b) => *b = p,
        OffsetShape::Arc { geom, .. } => {
            let new1 = (p - geom.center).angle();
            let sweep = crate::geom::norm_angle(new1 - geom.start_angle);
            geom.sweep = if sweep < 1e-9 { std::f64::consts::TAU } else { sweep };
        }
        OffsetShape::Circle { .. } | OffsetShape::Ellipse { .. } => {}
    }
}

/// Adds the offset of a chain to the sketch (see [`offset_geometry`]) with its constraints and
/// its driving offset dimension (`label`: the dimension's `offset` and `along`). Returns the
/// new curves (in chain order).
pub fn offset(
    s: &mut Sketch,
    chain: &[(CurveId, bool)],
    distance: f64,
    left: bool,
    label: (f64, f64),
) -> Result<Vec<CurveId>, String> {
    let shapes = offset_geometry(s, chain, distance, left)?;
    if shapes.is_empty() {
        return Err("nothing to offset".into());
    }
    let mut made: Vec<CurveId> = Vec::new();
    // Joints are shared: the end of one piece is the start of the next.
    let point = |s: &mut Sketch, p: Vec2| -> PointId {
        s.point_at(p, MERGE_EPS).unwrap_or_else(|| s.add_point(p))
    };
    for (&(src, reversed), shape) in chain.iter().zip(&shapes) {
        let construction = s.curves.get(src).is_some_and(|c| c.construction);
        let kind = match *shape {
            OffsetShape::Line(a, b) => {
                let (pa, pb) = (point(s, a), point(s, b));
                // Stored the way the source runs.
                if reversed {
                    CurveKind::Line { a: pb, b: pa }
                } else {
                    CurveKind::Line { a: pa, b: pb }
                }
            }
            OffsetShape::Arc { center, geom } => {
                let (st, en) = (point(s, geom.start()), point(s, geom.end()));
                CurveKind::Arc {
                    center,
                    start: st,
                    end: en,
                }
            }
            OffsetShape::Circle { center, radius } => CurveKind::Circle { center, radius },
            OffsetShape::Ellipse { center, major, minor, distance } => CurveKind::EllipseOffset { center, major, minor, distance },
        };
        let id = s.curves.insert(Curve { kind, construction });
        if matches!(kind, CurveKind::Line { .. }) {
            s.add_constraint(ConstraintOf::Parallel(CurveRef::Curve(src), CurveRef::Curve(id)));
        }
        made.push(id);
    }
    let lead = (chain[0].0, made[0]);
    for (i, &id) in made.iter().enumerate().skip(1) {
        s.add_constraint(ConstraintOf::EqualOffset(
            CurveRef::Curve(lead.0),
            CurveRef::Curve(lead.1),
            CurveRef::Curve(chain[i].0),
            CurveRef::Curve(id),
        ));
    }
    let dim = Dimension {
        kind: DimensionKind::Offset {
            source: lead.0,
            target: lead.1,
        },
        value: distance,
        offset: label.0,
        along: label.1,
        driven: false,
    };
    s.dimensions.insert(dim);
    Ok(made)
}

/// Scales the whole sketch about `center` by `factor`: points, circle radii and where the
/// dimension labels sit. Dimension values are left as they are (the caller sets the one it
/// scales to).
pub fn scale(s: &mut Sketch, center: Vec2, factor: f64) -> Result<(), String> {
    if !(factor.is_finite() && factor > 0.0) {
        return Err("a scale must be positive".into());
    }
    for p in s.points.values_mut() {
        p.pos = center + (p.pos - center) * factor;
    }
    for c in s.curves.values_mut() {
        if let Some(r) = c.kind.scalar() {
            c.kind.set_scalar(r * factor);
        }
    }
    let radial: Vec<bool> = s.dimensions.values().map(|d| d.kind.radial(s)).collect();
    for (d, radial) in s.dimensions.values_mut().zip(radial) {
        match d.kind {
            // The offset is the leader's angle: only the label's distance scales.
            DimensionKind::Diameter { .. }
            | DimensionKind::Radius { .. }
            | DimensionKind::EllipseRadius { .. }
            | DimensionKind::Sides { .. } => d.along *= factor,
            DimensionKind::Angle { .. } => d.offset *= factor,
            _ if radial => d.along *= factor,
            _ => {
                d.offset *= factor;
                d.along *= factor;
            }
        }
    }
    Ok(())
}

/// The edit that gives dimension `id` the value `value`. For a sketch's first dimension
/// (`first`: the one just placed when the sketch had none), the whole sketch is scaled to it as
/// well (S13.3: "the first dimension scales the whole sketch"), in the same step, about
/// [`scale_center`]. Not when something in the sketch is fixed (a Fix, or a curve Used from
/// outside the sketch, which can't move), or for an angle.
pub fn set_value_op(s: &Sketch, id: crate::DimensionId, value: f64, first: bool) -> crate::SketchOp {
    let set = crate::SketchOp::SetDimensionValue { id, value };
    let Some(d) = s.dimensions.get(id) else {
        return set;
    };
    let fixed = s
        .constraints
        .values()
        .any(|c| matches!(c, ConstraintOf::FixPoint(_) | ConstraintOf::FixCurve(_) | ConstraintOf::Use(..)));
    let measured = crate::dimension::measure(s, d.kind).unwrap_or(0.0);
    if !first || fixed || d.kind.quantity() != crate::units::Quantity::Length || measured <= 1e-9 {
        return set;
    }
    let factor = value / measured;
    if (factor - 1.0).abs() < 1e-12 {
        return set;
    }
    let center = scale_center(s, &d.kind);
    crate::SketchOp::Batch(vec![set, crate::SketchOp::Scale { center, factor }])
}

/// Where the first dimension scales a sketch about: the origin if the sketch is attached to it
/// (a constraint to the origin or a plane axis), otherwise the dimension's first point (or its
/// circle's center).
pub fn scale_center(s: &Sketch, d: &DimensionKind) -> Vec2 {
    let attached = s.constraints.values().any(|c| {
        c.points().contains(&PointRef::Origin)
            || c.curves()
                .iter()
                .any(|r| matches!(r, CurveRef::XAxis | CurveRef::YAxis))
    }) || matches!(
        d,
        DimensionKind::PointLine { p: PointRef::Origin, .. }
            | DimensionKind::PointLine {
                line: CurveRef::XAxis | CurveRef::YAxis,
                ..
            }
    );
    if attached {
        return Vec2::ZERO;
    }
    if let Some(p) = d.points().first() {
        return s.pos(*p);
    }
    for c in d.curves() {
        match s.curves.get(c).map(|c| c.kind) {
            Some(
                CurveKind::Circle { center, .. }
                | CurveKind::Arc { center, .. }
                | CurveKind::Ellipse { center, .. }
                | CurveKind::EllipseOffset { center, .. }
                | CurveKind::EllipseArc { center, .. },
            ) => {
                return s.pos(center);
            }
            Some(CurveKind::Line { a, .. } | CurveKind::Spline { start: a, .. } | CurveKind::Bezier { a, .. }) => return s.pos(a),
            None => {}
        }
    }
    Vec2::ZERO
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solve::{self, Status};
    use crate::{SketchOp, geom::ArcGeom};

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    /// Holds a curve where it is (so the solver moves the others).
    fn fix(s: &mut Sketch, c: CurveId) {
        s.add_constraint(ConstraintOf::FixCurve(CurveRef::Curve(c)));
    }

    fn line(s: &mut Sketch, a: Vec2, b: Vec2, construction: bool) -> CurveId {
        SketchOp::AddPolyline {
            points: vec![a, b],
            closed: false,
            construction,
            label: "Add line",
        }
        .apply(s)
        .unwrap();
        let (pa, pb) = (s.point_at(a, 1e-9).unwrap(), s.point_at(b, 1e-9).unwrap());
        s.curves
            .keys()
            .find(|k| s.curve_ends(*k) == Some((pa, pb)))
            .unwrap()
    }

    #[test]
    fn mirror_copies_follow_their_sources() {
        let mut s = Sketch::new();
        let axis = line(&mut s, v(0.0, -50.0), v(0.0, 50.0), true);
        fix(&mut s, axis);
        let l = line(&mut s, v(10.0, 0.0), v(30.0, 20.0), false);
        SketchOp::AddArc {
            center: v(20.0, -20.0),
            start: v(30.0, -20.0),
            end: v(20.0, -10.0),
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let arc = s.curves.keys().find(|k| s.arc_geom(*k).is_some()).unwrap();
        SketchOp::Mirror {
            axis,
            curves: vec![l, arc],
        }
        .apply(&mut s)
        .unwrap();
        assert_eq!(s.curves.len(), 5);
        assert_eq!(
            s.constraints
                .values()
                .filter(|c| matches!(c, ConstraintOf::SymmetricCurves(..)))
                .count(),
            2
        );
        // The copies are the mirror images.
        assert!(s.point_at(v(-10.0, 0.0), 1e-9).is_some());
        assert!(s.point_at(v(-30.0, 20.0), 1e-9).is_some());
        let copy_arc = s
            .curves
            .keys()
            .filter(|k| *k != arc)
            .find(|k| s.arc_geom(*k).is_some())
            .unwrap();
        let g = s.arc_geom(copy_arc).unwrap();
        assert!(g.center.distance(v(-20.0, -20.0)) < 1e-9);
        assert!((g.sweep - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        // Moving the source moves the copy (the Symmetric constraint).
        let p = s.point_at(v(30.0, 20.0), 1e-9).unwrap();
        SketchOp::MovePoints {
            moves: vec![(p, v(40.0, 25.0))],
        }
        .apply(&mut s)
        .unwrap();
        let moved = s.pos(p);
        let (a, b) = s.curve_ends(l).unwrap();
        let other = if a == p { b } else { a };
        let image = |q: Vec2| v(-q.x, q.y);
        assert!(s.point_at(image(moved), 1e-6).is_some(), "{moved:?}");
        assert!(s.point_at(image(s.pos(other)), 1e-6).is_some());
        // Undoable as one step through the command layer is tested in cadrs_core; here: the
        // arc copy's radius follows too.
        let g2 = s.arc_geom(copy_arc).unwrap();
        assert!((g2.radius - s.arc_geom(arc).unwrap().radius).abs() < 1e-6);
    }

    #[test]
    fn mirror_shares_points_on_the_axis_and_existing_points() {
        let mut s = Sketch::new();
        let axis = line(&mut s, v(0.0, 0.0), v(0.0, 100.0), true);
        // A base half-line from the axis, and a side line up from its end.
        let base = line(&mut s, v(0.0, 0.0), v(-60.0, 0.0), false);
        let side = line(&mut s, v(-60.0, 0.0), v(-60.0, 50.0), false);
        // A point already where the side's top mirrors to.
        let lone = line(&mut s, v(60.0, 50.0), v(80.0, 90.0), false);
        let before = s.points.len();
        SketchOp::Mirror {
            axis,
            curves: vec![base, side],
        }
        .apply(&mut s)
        .unwrap();
        // Only (60, 0) is new: (0, 0) is on the axis and (60, 50) existed.
        assert_eq!(s.points.len(), before + 1);
        let origin = s.point_at(v(0.0, 0.0), 1e-9).unwrap();
        assert_eq!(s.curves_at(origin).count(), 3);
        let top = s.point_at(v(60.0, 50.0), 1e-9).unwrap();
        assert!(s.curves_at(top).count() == 2 && s.curve_points(lone).contains(&top));
        // A closed outline: base, sides and copies plus a top line make a region.
        assert!(solve::conflicts(&s).is_empty());
    }

    #[test]
    fn symmetric_constraint_solves_points_and_circles() {
        let mut s = Sketch::new();
        let axis = line(&mut s, v(0.0, 0.0), v(10.0, 10.0), false);
        fix(&mut s, axis);
        let a = s.add_point(v(5.0, 0.0));
        let b = s.add_point(v(1.0, 3.0));
        s.add_constraint(ConstraintOf::SymmetricPoints(
            PointRef::Point(a),
            PointRef::Point(b),
            CurveRef::Curve(axis),
        ));
        SketchOp::AddCircle {
            center: v(20.0, 5.0),
            radius: 3.0,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        SketchOp::AddCircle {
            center: v(4.0, 18.0),
            radius: 5.0,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let circles: Vec<CurveId> = s
            .curves
            .iter()
            .filter(|(_, c)| matches!(c.kind, CurveKind::Circle { .. }))
            .map(|(k, _)| k)
            .collect();
        SketchOp::AddConstraint {
            constraints: vec![ConstraintOf::SymmetricCurves(
                CurveRef::Curve(circles[0]),
                CurveRef::Curve(circles[1]),
                CurveRef::Curve(axis),
            )],
            label: "Add symmetric",
        }
        .apply(&mut s)
        .unwrap();
        let (l0, l1) = crate::dimension::line_ends(&s, CurveRef::Curve(axis)).unwrap();
        let (pa, pb) = (s.pos(a), s.pos(b));
        assert!(mirror_point(pa, l0, l1).distance(pb) < 1e-7, "{pa:?} {pb:?}");
        let geo = |c: CurveId| match s.curves[c].kind {
            CurveKind::Circle { center, radius } => (s.pos(center), radius),
            _ => unreachable!(),
        };
        let ((c0, r0), (c1, r1)) = (geo(circles[0]), geo(circles[1]));
        assert!(mirror_point(c0, l0, l1).distance(c1) < 1e-7);
        assert!((r0 - r1).abs() < 1e-7);
        assert!(solve::conflicts(&s).is_empty());
    }

    #[test]
    fn symmetric_fit_takes_the_axis_first() {
        use crate::constraint::{ConstraintKind, Fit, fit};
        use crate::SketchEntity as E;
        let mut s = Sketch::new();
        let axis = line(&mut s, v(0.0, 0.0), v(0.0, 10.0), false);
        fix(&mut s, axis);
        let l1 = line(&mut s, v(2.0, 0.0), v(5.0, 5.0), false);
        let l2 = line(&mut s, v(-2.0, 0.0), v(-6.0, 5.0), false);
        let p = s.add_point(v(3.0, 3.0));
        let k = ConstraintKind::Symmetric;
        assert_eq!(fit(k, &s, &[E::Curve(axis)]), Fit::Partial);
        assert_eq!(fit(k, &s, &[E::Curve(axis), E::Curve(l1)]), Fit::Partial);
        assert!(matches!(
            fit(k, &s, &[E::Curve(axis), E::Curve(l1), E::Curve(l2)]),
            Fit::Complete(_)
        ));
        // A line and a point are not the same type; a point first is not an axis.
        assert_eq!(fit(k, &s, &[E::Curve(axis), E::Curve(l1), E::Point(p)]), Fit::Invalid);
        assert_eq!(fit(k, &s, &[E::Point(p)]), Fit::Invalid);
        // Solving two lines symmetric.
        let Fit::Complete(cs) = fit(k, &s, &[E::Curve(axis), E::Curve(l1), E::Curve(l2)]) else {
            unreachable!()
        };
        SketchOp::AddConstraint {
            constraints: cs,
            label: "Add symmetric",
        }
        .apply(&mut s)
        .unwrap();
        let (a1, b1) = s.curve_ends(l1).unwrap();
        for q in [a1, b1] {
            let m = mirror_point(s.pos(q), v(0.0, 0.0), v(0.0, 10.0));
            assert!(s.curve_points(l2).iter().any(|x| s.pos(*x).distance(m) < 1e-7));
        }
    }

    #[test]
    fn offset_line_arc_circle_and_flip() {
        // A line: offset to the left (up for a line running +X), then to the right.
        let mut s = Sketch::new();
        let l = line(&mut s, v(0.0, 0.0), v(40.0, 0.0), false);
        fix(&mut s, l);
        let chain = chain_of(&s, l).0;
        let up = offset_geometry(&s, &chain, 5.0, true).unwrap();
        assert_eq!(up, vec![OffsetShape::Line(v(0.0, 5.0), v(40.0, 5.0))]);
        let down = offset_geometry(&s, &chain, 5.0, false).unwrap();
        assert_eq!(down, vec![OffsetShape::Line(v(0.0, -5.0), v(40.0, -5.0))]);
        SketchOp::Offset {
            chain: chain.clone(),
            distance: 7.0,
            left: false,
            label: (3.0, 0.0),
        }
        .apply(&mut s)
        .unwrap();
        assert!(s.point_at(v(0.0, -7.0), 1e-9).is_some());
        let d = s.dimensions.values().next().unwrap();
        assert!(matches!(d.kind, DimensionKind::Offset { .. }));
        assert!((crate::dimension::measure(&s, d.kind).unwrap() - 7.0).abs() < 1e-9);
        // Changing the distance moves the offset line (it stays parallel).
        let id = s.dimensions.keys().next().unwrap();
        SketchOp::SetDimensionValue { id, value: 12.0 }.apply(&mut s).unwrap();
        let copy = s.curves.keys().find(|k| *k != l).unwrap();
        let (a, b) = s.curve_ends(copy).unwrap();
        assert!((s.pos(a).y + 12.0).abs() < 1e-7 && (s.pos(b).y + 12.0).abs() < 1e-7);

        // An arc offset inward keeps its center and angles; a circle outward.
        let mut s = Sketch::new();
        SketchOp::AddArc {
            center: v(0.0, 200.0),
            start: v(75.0, 200.0),
            end: v(-75.0, 200.0),
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let arc = s.curves.keys().next().unwrap();
        let chain = chain_of(&s, arc).0;
        // Running counter-clockwise, left is toward the center.
        SketchOp::Offset {
            chain,
            distance: 40.0,
            left: true,
            label: (std::f64::consts::FRAC_PI_2, 0.0),
        }
        .apply(&mut s)
        .unwrap();
        let inner = s.curves.keys().find(|k| *k != arc).unwrap();
        let g = s.arc_geom(inner).unwrap();
        assert!((g.radius - 35.0).abs() < 1e-9 && g.center.distance(v(0.0, 200.0)) < 1e-9);
        assert_eq!(s.curve_points(inner)[0], s.curve_points(arc)[0], "shares the center");
        let d = *s.dimensions.values().next().unwrap();
        assert!(d.kind.radial(&s));
        assert!((crate::dimension::measure(&s, d.kind).unwrap() - 40.0).abs() < 1e-9);
        // The offset arc's radius is defined (it is black), its ends slide (blue).
        let a = solve::analyze(&s);
        let (_, st, _) = match s.curves[inner].kind {
            CurveKind::Arc { center, start, end } => (center, start, end),
            _ => unreachable!(),
        };
        assert_eq!(a.point(st), Status::Under);

        let mut s = Sketch::new();
        SketchOp::AddCircle {
            center: v(0.0, 0.0),
            radius: 10.0,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let c = s.curves.keys().next().unwrap();
        let (chain, closed) = chain_of(&s, c);
        assert!(closed);
        let out = offset_geometry(&s, &chain, 3.0, false).unwrap();
        assert!(matches!(out[0], OffsetShape::Circle { radius, .. } if (radius - 13.0).abs() < 1e-12));
        assert!(offset_geometry(&s, &chain, 30.0, true).is_err());
    }

    /// P3.7 (X13, PS21.2 and PS21.4): the Funnel's rim. A 6 × 4 ellipse (a = 3, b = 2) offset
    /// 0.125 inward is an offset curve sharing the ellipse's center and major point; the two
    /// make a band and a disc. For a convex curve of area A and perimeter L, the curve d inside
    /// encloses A − L·d + π·d² (Steiner), so the band is L·d − π·d². Changing the dimension to
    /// 0.2 moves it; an offset of the offset 0.05 outward lies 0.075 inside the ellipse; and an
    /// inward offset past the smallest radius of curvature (b²/a = 4/3) is refused.
    #[test]
    fn offset_ellipse_makes_an_offset_curve() {
        let mut s = Sketch::new();
        SketchOp::AddEllipse { center: v(0.0, 0.0), major: v(3.0, 0.0), minor: 2.0, construction: false }
            .apply(&mut s)
            .unwrap();
        let e = s.curves.keys().next().unwrap();
        let (chain, closed) = chain_of(&s, e);
        assert!(closed);
        // Running counter-clockwise, left is inside.
        SketchOp::Offset { chain, distance: 0.125, left: true, label: (0.0, 0.0) }.apply(&mut s).unwrap();
        let rim = s.curves.keys().find(|k| *k != e).unwrap();
        let CurveKind::EllipseOffset { center, major, minor, distance } = s.curves[rim].kind else {
            panic!("an offset ellipse");
        };
        assert_eq!((center, major), (s.curve_points(e)[0], s.curve_points(e)[1]), "shares the ellipse's points");
        assert!((minor - 2.0).abs() < 1e-12 && (distance + 0.125).abs() < 1e-12);
        let g = s.ellipse_geom(rim).unwrap();
        assert!((g.point_at(0.0) - v(2.875, 0.0)).length() < 1e-12);
        assert!((g.point_at(std::f64::consts::FRAC_PI_2) - v(0.0, 1.875)).length() < 1e-12);
        let base = s.ellipse_geom(e).unwrap();
        let (area, l) = (base.area(), base.perimeter());
        let d = 0.125;
        let mut regions = crate::region::regions(&s);
        regions.sort_by(|a, b| a.area().total_cmp(&b.area()));
        assert_eq!(regions.len(), 2);
        assert!((regions[0].area() - (l * d - std::f64::consts::PI * d * d)).abs() < 1e-9, "{}", regions[0].area());
        assert!((regions[1].area() - (area - l * d + std::f64::consts::PI * d * d)).abs() < 1e-9);
        // The dimension drives it.
        let dim = s.dimensions.keys().next().unwrap();
        assert!((crate::dimension::measure(&s, s.dimensions[dim].kind).unwrap() - 0.125).abs() < 1e-12);
        SketchOp::SetDimensionValue { id: dim, value: 0.2 }.apply(&mut s).unwrap();
        assert!(matches!(s.curves[rim].kind, CurveKind::EllipseOffset { distance, .. } if (distance + 0.2).abs() < 1e-12));
        // The ellipse's minor radius changes: the offset follows.
        SketchOp::SetDimensionValue { id: dim, value: 0.125 }.apply(&mut s).unwrap();
        if let CurveKind::Ellipse { minor, .. } = &mut s.curves[e].kind {
            *minor = 2.5;
        }
        crate::solve::solve(&mut s);
        assert!(matches!(s.curves[rim].kind, CurveKind::EllipseOffset { minor, .. } if (minor - 2.5).abs() < 1e-12));
        if let CurveKind::Ellipse { minor, .. } = &mut s.curves[e].kind {
            *minor = 2.0;
        }
        crate::solve::solve(&mut s);
        // An offset of the offset, 0.05 outward (the Funnel's Sketch 2).
        let (chain, _) = chain_of(&s, rim);
        SketchOp::Offset { chain, distance: 0.05, left: false, label: (0.0, 0.0) }.apply(&mut s).unwrap();
        let band = s.curves.keys().find(|k| *k != e && *k != rim).unwrap();
        assert!(matches!(s.curves[band].kind, CurveKind::EllipseOffset { distance, .. } if (distance + 0.075).abs() < 1e-12));
        let (chain, _) = chain_of(&s, e);
        assert!(offset_geometry(&s, &chain, 1.4, true).is_err());
        assert!(offset_geometry(&s, &chain, 1.3, true).is_ok());
    }

    #[test]
    fn offset_chain_meets_at_corners_and_keeps_one_distance() {
        // An L: along +X then up. Its offset to the left (inside the corner) meets at (35, 5).
        let mut s = Sketch::new();
        let a = line(&mut s, v(0.0, 0.0), v(40.0, 0.0), false);
        let _b = line(&mut s, v(40.0, 0.0), v(40.0, 30.0), false);
        fix(&mut s, a);
        fix(&mut s, _b);
        let (chain, closed) = chain_of(&s, a);
        assert!(!closed);
        assert_eq!(chain.len(), 2);
        let shapes = offset_geometry(&s, &chain, 5.0, true).unwrap();
        assert_eq!(shapes[0], OffsetShape::Line(v(0.0, 5.0), v(35.0, 5.0)));
        assert_eq!(shapes[1], OffsetShape::Line(v(35.0, 5.0), v(35.0, 30.0)));
        // Picking the other piece gives the same chain.
        assert_eq!(chain_of(&s, _b).0.len(), 2);
        SketchOp::Offset {
            chain,
            distance: 5.0,
            left: true,
            label: (2.0, 0.0),
        }
        .apply(&mut s)
        .unwrap();
        assert_eq!(s.curves.len(), 4);
        // Both offsets follow a new distance.
        let id = s.dimensions.keys().next().unwrap();
        SketchOp::SetDimensionValue { id, value: 8.0 }.apply(&mut s).unwrap();
        assert!(s.point_at(v(32.0, 8.0), 1e-6).is_some(), "{:?}", s.points);
        // A tangent chain: a line and an arc continuing it.
        let mut s = Sketch::new();
        let l = line(&mut s, v(0.0, 0.0), v(20.0, 0.0), false);
        SketchOp::AddArc {
            center: v(20.0, 10.0),
            start: v(20.0, 0.0),
            end: v(30.0, 10.0),
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let (chain, _) = chain_of(&s, l);
        assert_eq!(chain.len(), 2);
        let shapes = offset_geometry(&s, &chain, 2.0, true).unwrap();
        match shapes[1] {
            OffsetShape::Arc { geom, .. } => {
                assert!((geom.radius - 8.0).abs() < 1e-9);
                assert!(geom.start().distance(v(20.0, 2.0)) < 1e-9);
            }
            _ => panic!("{shapes:?}"),
        }
        let _ = ArcGeom::ccw(v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0));
    }

    #[test]
    fn new_symmetric_and_offset_edits_move_only_the_second_entity() {
        use crate::constraint::{ConstraintKind, Fit, fit};
        use crate::SketchEntity as E;
        // Nothing fixed: applying Symmetric keeps the first line and the axis where they are.
        let mut s = Sketch::new();
        let axis = line(&mut s, v(0.0, 0.0), v(0.0, 10.0), true);
        let l1 = line(&mut s, v(-10.0, 0.0), v(-4.0, 8.0), false);
        let l2 = line(&mut s, v(3.0, -2.0), v(12.0, 6.0), false);
        let before: Vec<Vec2> = [axis, l1]
            .iter()
            .flat_map(|c| s.curve_points(*c))
            .map(|p| s.pos(p))
            .collect();
        let Fit::Complete(cs) = fit(
            ConstraintKind::Symmetric,
            &s,
            &[E::Curve(axis), E::Curve(l1), E::Curve(l2)],
        ) else {
            unreachable!()
        };
        SketchOp::AddConstraint {
            constraints: cs,
            label: "Add symmetric",
        }
        .apply(&mut s)
        .unwrap();
        let after: Vec<Vec2> = [axis, l1]
            .iter()
            .flat_map(|c| s.curve_points(*c))
            .map(|p| s.pos(p))
            .collect();
        assert_eq!(before, after, "the first line and the axis stay");
        assert!(s.point_at(v(10.0, 0.0), 1e-7).is_some() && s.point_at(v(4.0, 8.0), 1e-7).is_some());
        // Nothing fixed: typing an offset distance moves the copy, not the source.
        let mut s = Sketch::new();
        let l = line(&mut s, v(0.0, 0.0), v(40.0, 0.0), false);
        SketchOp::Offset {
            chain: vec![(l, false)],
            distance: 5.0,
            left: true,
            label: (3.0, 0.0),
        }
        .apply(&mut s)
        .unwrap();
        let id = s.dimensions.keys().next().unwrap();
        SketchOp::SetDimensionValue { id, value: 12.0 }.apply(&mut s).unwrap();
        let (a, b) = s.curve_ends(l).unwrap();
        assert_eq!((s.pos(a), s.pos(b)), (v(0.0, 0.0), v(40.0, 0.0)), "the source stays");
        assert!(s.point_at(v(0.0, 12.0), 1e-7).is_some());
    }

    #[test]
    fn scaling_keeps_the_shape() {
        let mut s = Sketch::new();
        let l = line(&mut s, v(10.0, 0.0), v(30.0, 0.0), false);
        SketchOp::AddCircle {
            center: v(0.0, 0.0),
            radius: 5.0,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        SketchOp::Scale {
            center: v(0.0, 0.0),
            factor: 2.0,
        }
        .apply(&mut s)
        .unwrap();
        assert_eq!(s.line_length(l), Some(40.0));
        assert!(s.point_at(v(20.0, 0.0), 1e-9).is_some());
        assert!(s.curves.values().any(|c| matches!(c.kind, CurveKind::Circle { radius, .. } if radius == 10.0)));
    }
}

/// Pastes `src`'s points, curves, constraints and dimensions into `dst`, moved by `offset`
/// (P3D.1: the feature menu's Copy sketch, then Ctrl+V in another sketch). The copies get new
/// ids; Use and Pierce links and text boxes stay behind (the pasted geometry is free), as do
/// imprints. Returns the new curves.
pub fn paste(dst: &mut Sketch, src: &Sketch, offset: Vec2) -> Vec<CurveId> {
    let mut points: HashMap<PointId, PointId> = HashMap::new();
    for (k, p) in &src.points {
        points.insert(k, dst.add_point(p.pos + offset));
    }
    let pt = |p: PointId| points.get(&p).copied();
    let mut curves: HashMap<CurveId, CurveId> = HashMap::new();
    for (k, c) in &src.curves {
        let kind = match c.kind {
            CurveKind::Line { a, b } => CurveKind::Line { a: points[&a], b: points[&b] },
            CurveKind::Circle { center, radius } => CurveKind::Circle { center: points[&center], radius },
            CurveKind::Arc { center, start, end } => CurveKind::Arc { center: points[&center], start: points[&start], end: points[&end] },
            CurveKind::Ellipse { center, major, minor } => CurveKind::Ellipse { center: points[&center], major: points[&major], minor },
            CurveKind::EllipseOffset { center, major, minor, distance } => {
                CurveKind::EllipseOffset { center: points[&center], major: points[&major], minor, distance }
            }
            CurveKind::EllipseArc { center, major, minor, start, end } => CurveKind::EllipseArc {
                center: points[&center],
                major: points[&major],
                minor,
                start: points[&start],
                end: points[&end],
            },
            CurveKind::Spline { start, end } => CurveKind::Spline { start: points[&start], end: points[&end] },
            CurveKind::Bezier { a, c1, c2, b } => CurveKind::Bezier { a: points[&a], c1: points[&c1], c2: points[&c2], b: points[&b] },
        };
        let new = dst.curves.insert(crate::Curve { kind, construction: c.construction });
        if let Some(d) = src.splines.get(k) {
            let mut d = d.clone();
            d.points = d.points.iter().map(|p| points[p]).collect();
            dst.splines.insert(new, d);
        }
        curves.insert(k, new);
    }
    let cv = |c: CurveId| curves.get(&c).copied();
    let pref = |p: PointRef| match p {
        PointRef::Point(k) => pt(k).map(PointRef::Point),
        PointRef::Origin => Some(PointRef::Origin),
    };
    let cref = |c: CurveRef| match c {
        CurveRef::Curve(k) => cv(k).map(CurveRef::Curve),
        other => Some(other),
    };
    for c in src.constraints.values() {
        if matches!(c, ConstraintOf::Use(..) | ConstraintOf::Pierce(..) | ConstraintOf::TextAspect(_)) {
            continue;
        }
        if let Some(m) = c.map(pref, cref) {
            dst.constraints.insert(m);
        }
    }
    for d in src.dimensions.values() {
        use crate::DimensionKind as K;
        let kind = match d.kind {
            K::Horizontal { a, b } => pt(a).zip(pt(b)).map(|(a, b)| K::Horizontal { a, b }),
            K::Vertical { a, b } => pt(a).zip(pt(b)).map(|(a, b)| K::Vertical { a, b }),
            K::Aligned { a, b } => pt(a).zip(pt(b)).map(|(a, b)| K::Aligned { a, b }),
            K::Diameter { curve } => cv(curve).map(|curve| K::Diameter { curve }),
            K::Radius { curve } => cv(curve).map(|curve| K::Radius { curve }),
            K::PointLine { p, line } => pref(p).zip(cref(line)).map(|(p, line)| K::PointLine { p, line }),
            K::Diametral { p, line } => pref(p).zip(cref(line)).map(|(p, line)| K::Diametral { p, line }),
            K::Angle { a, b, flip_a, flip_b } => cref(a).zip(cref(b)).map(|(a, b)| K::Angle { a, b, flip_a, flip_b }),
            K::PointCircle { p, circle, far } => pref(p).zip(cv(circle)).map(|(p, circle)| K::PointCircle { p, circle, far }),
            K::LineCircle { line, circle, far } => cref(line).zip(cv(circle)).map(|(line, circle)| K::LineCircle { line, circle, far }),
            K::CircleCircle { a, b, far_a, far_b, axis } => cv(a).zip(cv(b)).map(|(a, b)| K::CircleCircle { a, b, far_a, far_b, axis }),
            K::Offset { source, target } => cv(source).zip(cv(target)).map(|(source, target)| K::Offset { source, target }),
            K::EllipseRadius { curve, major } => cv(curve).map(|curve| K::EllipseRadius { curve, major }),
            K::Sides { circle, inscribed } => cv(circle).map(|circle| K::Sides { circle, inscribed }),
        };
        if let Some(kind) = kind {
            dst.dimensions.insert(crate::Dimension { kind, ..*d });
        }
    }
    curves.values().copied().collect()
}

#[cfg(test)]
mod paste_tests {
    use super::*;
    use crate::{Dimension, DimensionKind, SketchOp};

    /// P3D.1 (Copy sketch): a pasted rectangle with a width dimension keeps its constraints and
    /// dimension on the copies, moved by the offset.
    #[test]
    fn paste_copies_geometry_constraints_and_dimensions() {
        let mut src = Sketch::new();
        let v = Vec2::new;
        SketchOp::AddPolyline { points: vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 5.0), v(0.0, 5.0)], closed: true, construction: false, label: "r" }
            .apply(&mut src)
            .unwrap();
        let bottom = src.curves.keys().next().unwrap();
        src.constraints.insert(ConstraintOf::Horizontal(crate::Orient::Line(CurveRef::Curve(bottom))));
        let (a, b) = src.curve_ends(bottom).unwrap();
        src.dimensions.insert(Dimension::new(DimensionKind::Horizontal { a, b }, 10.0, -3.0));
        let mut dst = Sketch::new();
        SketchOp::Paste { sketch: Box::new(src.clone()), offset: v(20.0, 0.0) }.apply(&mut dst).unwrap();
        assert_eq!(dst.curves.len(), 4);
        assert_eq!(dst.constraints.len(), 1);
        assert_eq!(dst.dimensions.len(), 1);
        assert!(dst.point_at(v(30.0, 5.0), 1e-9).is_some());
        assert!(crate::solve::conflicts(&dst).is_empty());
    }
}
