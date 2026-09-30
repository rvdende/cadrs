//! The T3 entity tools' geometry (`intro-to-sketching.md` S3.2–S10), following Onshape's help
//! (`reference/onshape/entity_tools.md`):
//!
//! - **Midpoint line** ([`midpoint_line`]): the first click is the middle; the line grows both
//!   ways, its middle held at a snapped point by a Midpoint constraint.
//! - **Aligned rectangle** ([`aligned_corners`], [`aligned_constraints`]): the first side's two
//!   ends, then the width square to it; opposite sides parallel, one corner square.
//! - **3-point circle**: [`crate::geom::circle_through`].
//! - **Polygon** ([`add_polygon`], [`set_polygon_sides`]): 3–50 equal sides on a construction
//!   circle, with its side count as a [`DimensionKind::Sides`] value ("6x"). Onshape's names:
//!   an *inscribed* polygon has the circle inscribed in it (the circle touches each side's
//!   middle, a hollow point there); a *circumscribed* one has the circle round it (its corners
//!   on the circle) (`entity_tools/polygon-inscribed-circumscribed.png`).
//! - **Slot** ([`slot`]): a line or arc becomes the spine of a slot: end arcs centred on its
//!   ends, joined by tangent sides (lines along a line, concentric arcs along an arc), with a
//!   Ø width on an end arc. Slots made together share one width (Equal).
//! - **Ellipse**: [`crate::CurveKind::Ellipse`].
//! - **Sketch fillet** ([`fillet`]): trims two lines meeting at a corner and joins them with a
//!   tangent arc. The corner stays as a *virtual sharp* (a hollow point on both lines), so
//!   dimensions and constraints to it hold. The first fillet gets the radius dimension, the
//!   others made in the same use are Equal to it (one "R" drives them all,
//!   `entity_tools/sketchfilletvertexexample.png`).
//! - **Sketch chamfer** ([`chamfer`]): trims two lines and joins them with a line, with two
//!   distances measured along the lines from the corner, which stays as a point with dash-dot
//!   construction extensions to the chamfer's ends (`entity_tools/chamfer-creation-01.png`).
//! - **Point**: [`crate::SketchOp::AddPoint`].

use std::f64::consts::{PI, TAU};

use crate::constraint::{ConstraintOf, CurveRef, PointRef};
use crate::{Curve, CurveId, CurveKind, Dimension, DimensionKind, PointId, Sketch, Vec2};

/// The fewest and most sides a polygon has (S7.4).
pub const MIN_SIDES: u32 = 3;
pub const MAX_SIDES: u32 = 50;
/// The side count a new polygon starts with.
pub const DEFAULT_SIDES: u32 = 6;

// ---------------------------------------------------------------------------------------------
// Midpoint line and aligned rectangle

/// The ends of a midpoint line with its middle at `mid` and one end at `end`.
pub fn midpoint_line(mid: Vec2, end: Vec2) -> (Vec2, Vec2) {
    (mid * 2.0 - end, end)
}

/// The corners of an aligned rectangle: the first side from `p0` to `p1`, and the width set by
/// `p2` (its distance from that side's line, on its side). In order around the rectangle.
pub fn aligned_corners(p0: Vec2, p1: Vec2, p2: Vec2) -> [Vec2; 4] {
    let u = (p1 - p0).normalize();
    let n = u.perp();
    let w = (p2 - p0).dot(n);
    [p0, p1, p1 + n * w, p0 + n * w]
}

/// The constraints an aligned rectangle gets (by position, for
/// [`crate::SketchOp::AddConstraints`]): one corner square and opposite sides parallel, like a
/// corner rectangle's but without Horizontal, so it keeps its angle.
pub fn aligned_constraints(c: [Vec2; 4]) -> Vec<crate::ConstraintSpec> {
    use crate::constraint::CurveSpec;
    let side = |i: usize| CurveSpec::Between(c[i], c[(i + 1) % 4]);
    vec![
        ConstraintOf::Perpendicular(side(0), side(1)),
        ConstraintOf::Parallel(side(0), side(2)),
        ConstraintOf::Parallel(side(1), side(3)),
    ]
}

// ---------------------------------------------------------------------------------------------
// Polygon

/// The side count for a drag of `steps` (positive: more sides) from `base`, kept in range.
pub fn sides_for(base: u32, steps: i32) -> u32 {
    (base as i32 + steps).clamp(MIN_SIDES as i32, MAX_SIDES as i32) as u32
}

/// A polygon's corners: `n` of them round `center`. `angle` is the direction of the size
/// point: a corner for a circumscribed polygon (corners on the circle of radius `radius`), the
/// middle of a side for an inscribed one (the circle touches its sides).
pub fn polygon_corners(center: Vec2, radius: f64, angle: f64, n: u32, inscribed: bool) -> Vec<Vec2> {
    let n = n.max(MIN_SIDES);
    let step = TAU / n as f64;
    let (r, a0) = if inscribed {
        (radius / (PI / n as f64).cos(), angle + step / 2.0)
    } else {
        (radius, angle)
    };
    (0..n)
        .map(|k| center + Vec2::from_angle(a0 + step * k as f64) * r)
        .collect()
}

/// The parts of the polygon built on construction circle `circle`: its sides in order round
/// it, its corners, and (inscribed) the points where the circle touches the sides.
#[derive(Debug, Clone, PartialEq)]
pub struct PolygonParts {
    pub sides: Vec<CurveId>,
    pub corners: Vec<PointId>,
    pub touch: Vec<PointId>,
}

/// Finds the polygon on `circle`, if one is there.
pub fn polygon_parts(s: &Sketch, circle: CurveId, inscribed: bool) -> Option<PolygonParts> {
    let circle_ref = CurveRef::Curve(circle);
    let mut sides = Vec::new();
    let mut touch = Vec::new();
    if inscribed {
        // Hollow points on the circle, each the midpoint of a side.
        for c in s.constraints.values() {
            if let ConstraintOf::PointOnCurve(PointRef::Point(p), cr) = *c
                && cr == circle_ref
            {
                let side = s.constraints.values().find_map(|m| match *m {
                    ConstraintOf::Midpoint(PointRef::Point(q), CurveRef::Curve(l)) if q == p => {
                        Some(l)
                    }
                    _ => None,
                });
                if let Some(l) = side
                    && !sides.contains(&l)
                {
                    sides.push(l);
                    touch.push(p);
                }
            }
        }
    } else {
        let on: Vec<PointId> = s
            .constraints
            .values()
            .filter_map(|c| match *c {
                ConstraintOf::PointOnCurve(PointRef::Point(p), cr) if cr == circle_ref => Some(p),
                _ => None,
            })
            .collect();
        for (k, c) in &s.curves {
            if let CurveKind::Line { a, b } = c.kind
                && on.contains(&a)
                && on.contains(&b)
            {
                sides.push(k);
            }
        }
    }
    if sides.len() < MIN_SIDES as usize {
        return None;
    }
    // Order the sides round the center.
    let center = match s.curves.get(circle)?.kind {
        CurveKind::Circle { center, .. } => s.pos(center),
        _ => return None,
    };
    let mid = |l: CurveId| {
        let (a, b) = s.curve_ends(l).unwrap_or_default();
        s.pos(a).midpoint(s.pos(b))
    };
    let mut order: Vec<usize> = (0..sides.len()).collect();
    order.sort_by(|i, j| {
        crate::geom::norm_angle((mid(sides[*i]) - center).angle())
            .total_cmp(&crate::geom::norm_angle((mid(sides[*j]) - center).angle()))
    });
    let sides: Vec<CurveId> = order.iter().map(|i| sides[*i]).collect();
    let touch: Vec<PointId> = if inscribed {
        order.iter().map(|i| touch[*i]).collect()
    } else {
        Vec::new()
    };
    let mut corners = Vec::new();
    for l in &sides {
        if let Some((a, b)) = s.curve_ends(*l) {
            for p in [a, b] {
                if !corners.contains(&p) {
                    corners.push(p);
                }
            }
        }
    }
    Some(PolygonParts {
        sides,
        corners,
        touch,
    })
}

/// The touch points of inscribed polygons that are not drawn: Onshape shows one hollow point,
/// on the side the "Nx" label's leader points at (`entity_tools/polygon-inscribed-circumscribed.png`).
pub fn hidden_touch_points(s: &Sketch) -> Vec<PointId> {
    let mut out = Vec::new();
    for d in s.dimensions.values() {
        let crate::DimensionKind::Sides {
            circle,
            inscribed: true,
        } = d.kind
        else {
            continue;
        };
        let Some(parts) = polygon_parts(s, circle, true) else { continue };
        let Some(CurveKind::Circle { center, .. }) = s.curves.get(circle).map(|c| c.kind) else {
            continue;
        };
        let c = s.pos(center);
        let u = Vec2::from_angle(d.offset);
        // The side the leader points at (as `dimension::layout` picks it).
        let shown = parts
            .touch
            .iter()
            .copied()
            .max_by(|a, b| {
                (s.pos(*a) - c)
                    .normalize()
                    .dot(u)
                    .total_cmp(&(s.pos(*b) - c).normalize().dot(u))
            });
        out.extend(parts.touch.iter().copied().filter(|p| Some(*p) != shown));
    }
    out
}

/// Adds a polygon: its construction circle (center, radius), `n` sides and their constraints,
/// and its side-count value. Returns the circle.
pub fn add_polygon(
    s: &mut Sketch,
    center: Vec2,
    radius: f64,
    angle: f64,
    n: u32,
    inscribed: bool,
    construction: bool,
) -> Result<CurveId, String> {
    if !(radius.is_finite() && radius > 0.0) {
        return Err("a polygon needs a size".into());
    }
    let c = s.ensure_point(center);
    let circle = s.curves.insert(Curve {
        kind: CurveKind::Circle { center: c, radius },
        construction: true,
    });
    build_polygon(s, circle, angle, n, inscribed, construction)?;
    // The side count ("6x") sits outside, up and to the right, its arrow on the nearest side
    // (`polygon-inscribed-sides.png`). `offset` is the direction it sits in.
    s.dimensions.insert(Dimension::new(
        DimensionKind::Sides { circle, inscribed },
        n as f64,
        PI / 3.0,
    ));
    Ok(circle)
}

/// Builds the sides of a polygon on `circle` (which exists).
fn build_polygon(
    s: &mut Sketch,
    circle: CurveId,
    angle: f64,
    n: u32,
    inscribed: bool,
    construction: bool,
) -> Result<(), String> {
    let n = n.clamp(MIN_SIDES, MAX_SIDES);
    let (c, radius) = match s.curves.get(circle).map(|c| c.kind) {
        Some(CurveKind::Circle { center, radius }) => (center, radius),
        _ => return Err("the polygon's circle is gone".into()),
    };
    let center = s.pos(c);
    let pos = polygon_corners(center, radius, angle, n, inscribed);
    let corners: Vec<PointId> = pos.iter().map(|p| s.add_point(*p)).collect();
    s.quiet.points.extend(corners.iter().copied());
    let nn = corners.len();
    let mut sides = Vec::new();
    for k in 0..nn {
        sides.push(s.curves.insert(Curve {
            kind: CurveKind::Line {
                a: corners[k],
                b: corners[(k + 1) % nn],
            },
            construction,
        }));
    }
    let cref = CurveRef::Curve(circle);
    let pr = PointRef::Point;
    for k in 1..nn {
        s.add_quiet_constraint(ConstraintOf::Equal(
            CurveRef::Curve(sides[0]),
            CurveRef::Curve(sides[k]),
        ));
    }
    if inscribed {
        // The circle touches each side at its middle: a hollow point there.
        for (k, l) in sides.iter().enumerate() {
            let m = pos[k].midpoint(pos[(k + 1) % nn]);
            let t = s.add_point(m);
            s.quiet.points.insert(t);
            s.add_quiet_constraint(ConstraintOf::Midpoint(pr(t), CurveRef::Curve(*l)));
            s.add_quiet_constraint(ConstraintOf::PointOnCurve(pr(t), cref));
        }
        // Equal sides with their middles on a circle still leave an even polygon free to
        // shear (a square into a rhombus): two neighbouring corners equally far from the
        // center settle it.
        s.add_quiet_constraint(ConstraintOf::EqualDistance(pr(c), pr(corners[0]), pr(corners[1])));
    } else {
        for p in &corners {
            s.add_quiet_constraint(ConstraintOf::PointOnCurve(pr(*p), cref));
        }
    }
    Ok(())
}

/// Rebuilds the polygon on `circle` with `n` sides (S7.4: the side count edited later), keeping
/// its circle and the direction of its first corner (or touch point).
pub fn set_polygon_sides(s: &mut Sketch, circle: CurveId, n: u32) -> Result<(), String> {
    let inscribed = s
        .dimensions
        .values()
        .find_map(|d| match d.kind {
            DimensionKind::Sides { circle: c, inscribed } if c == circle => Some(inscribed),
            _ => None,
        })
        .ok_or("no polygon on that circle")?;
    let parts = polygon_parts(s, circle, inscribed).ok_or("the polygon is gone")?;
    let center = match s.curves.get(circle).map(|c| c.kind) {
        Some(CurveKind::Circle { center, .. }) => s.pos(center),
        _ => return Err("the polygon's circle is gone".into()),
    };
    let first = if inscribed {
        s.pos(parts.touch[0])
    } else {
        let (a, _) = s.curve_ends(parts.sides[0]).ok_or("no side")?;
        s.pos(a)
    };
    let angle = (first - center).angle();
    let construction = s.curves[parts.sides[0]].construction;
    for l in &parts.sides {
        s.remove_curve(*l);
    }
    for p in parts.touch.iter().chain(&parts.corners) {
        if s.points.contains_key(*p) && !s.point_in_use(*p) {
            s.remove_point(*p);
        }
    }
    build_polygon(s, circle, angle, n, inscribed, construction)?;
    for d in s.dimensions.values_mut() {
        if let DimensionKind::Sides { circle: c, .. } = d.kind
            && c == circle
        {
            d.value = n as f64;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Slot

/// Makes a slot of `width` round `source` (a line or an arc). The width is a Ø dimension on its
/// first end arc, or, with `equal_to` (another slot's end arc, made in the same use of the
/// tool), an Equal constraint to it. Returns the first end arc.
pub fn slot(
    s: &mut Sketch,
    source: CurveId,
    width: f64,
    equal_to: Option<CurveId>,
    construction: bool,
) -> Result<CurveId, String> {
    if !(width.is_finite() && width > 0.0) {
        return Err("a slot needs a width".into());
    }
    let w = width / 2.0;
    let kind = s.curves.get(source).ok_or("the curve is gone")?.kind;
    let arc = |s: &mut Sketch, center: PointId, start: PointId, end: PointId| {
        s.curves.insert(Curve {
            kind: CurveKind::Arc { center, start, end },
            construction,
        })
    };
    let tangent = |s: &mut Sketch, a: CurveId, b: CurveId| {
        s.add_quiet_constraint(ConstraintOf::Tangent(CurveRef::Curve(a), CurveRef::Curve(b)));
    };
    let (cap0, cap1) = match kind {
        CurveKind::Line { a, b } => {
            let (pa, pb) = (s.pos(a), s.pos(b));
            if pa.distance(pb) <= width * 1e-6 {
                return Err("the line has no length".into());
            }
            let u = (pb - pa).normalize();
            let n = u.perp();
            let (la1, lb1) = (s.add_point(pa + n * w), s.add_point(pb + n * w));
            let (la2, lb2) = (s.add_point(pa - n * w), s.add_point(pb - n * w));
            s.quiet.points.extend([la1, lb1, la2, lb2, a, b]);
            let side = |s: &mut Sketch, p: PointId, q: PointId| {
                s.curves.insert(Curve {
                    kind: CurveKind::Line { a: p, b: q },
                    construction,
                })
            };
            let side1 = side(s, la1, lb1);
            let side2 = side(s, la2, lb2);
            // Counter-clockwise round the back of each end.
            let cap0 = arc(s, a, la1, la2);
            let cap1 = arc(s, b, lb2, lb1);
            for cap in [cap0, cap1] {
                tangent(s, cap, side1);
                tangent(s, cap, side2);
            }
            s.add_quiet_constraint(ConstraintOf::Equal(CurveRef::Curve(cap0), CurveRef::Curve(cap1)));
            (cap0, cap1)
        }
        CurveKind::Arc { center, start, end } => {
            let g = s.arc_geom(source).ok_or("no arc")?;
            if g.radius <= w * (1.0 + 1e-9) {
                return Err("the slot is wider than the arc".into());
            }
            let (ps, pe) = (s.pos(start), s.pos(end));
            let (rs, re) = ((ps - g.center).normalize(), (pe - g.center).normalize());
            let (o0, i0) = (s.add_point(ps + rs * w), s.add_point(ps - rs * w));
            let (o1, i1) = (s.add_point(pe + re * w), s.add_point(pe - re * w));
            s.quiet.points.extend([o0, i0, o1, i1, start, end, center]);
            let outer = arc(s, center, o0, o1);
            let inner = arc(s, center, i0, i1);
            let cap0 = arc(s, start, i0, o0);
            let cap1 = arc(s, end, o1, i1);
            for cap in [cap0, cap1] {
                tangent(s, cap, outer);
                tangent(s, cap, inner);
            }
            // The two end arcs are equal already (the inner arc's ends are both on its circle).
            (cap0, cap1)
        }
        _ => return Err("a slot needs a line or an arc".into()),
    };
    match equal_to.filter(|e| s.curves.contains_key(*e)) {
        Some(e) => {
            s.add_constraint(ConstraintOf::Equal(CurveRef::Curve(e), CurveRef::Curve(cap0)));
        }
        None => {
            // "Ø…" over the higher end, its leader coming straight down onto it
            // (`entity_tools/slot-examples.png`).
            // The higher end: its circle's top is on the end arc or where the arc meets a side.
            let top = |c: CurveId| s.arc_geom(c).map_or(f64::MIN, |g| g.mid().y);
            let cap = if top(cap1) > top(cap0) { cap1 } else { cap0 };
            let angle = PI / 2.0;
            s.dimensions.insert(Dimension::new(
                DimensionKind::Diameter { curve: cap },
                width,
                angle,
            ));
        }
    }
    let _ = cap1;
    Ok(cap0)
}

// ---------------------------------------------------------------------------------------------
// Fillet and chamfer

/// The two lines meeting at `corner` (exactly two curves there, both lines), each with its
/// other end.
pub fn corner_lines(s: &Sketch, corner: PointId) -> Option<[(CurveId, PointId); 2]> {
    let at: Vec<CurveId> = s.curves_at(corner).collect();
    let [l1, l2] = at[..] else { return None };
    let other = |l: CurveId| match s.curves.get(l)?.kind {
        CurveKind::Line { a, b } if a == corner => Some(b),
        CurveKind::Line { a, b } if b == corner => Some(a),
        _ => None,
    };
    Some([(l1, other(l1)?), (l2, other(l2)?)])
}

/// The two curves meeting at `corner` that a fillet can round (Final re-audit, S9.1: lines and
/// arcs, exactly two curves there), each with its other end.
pub fn fillet_corner(s: &Sketch, corner: PointId) -> Option<[(CurveId, PointId); 2]> {
    let at: Vec<CurveId> = s.curves_at(corner).collect();
    let [l1, l2] = at[..] else { return None };
    let other = |l: CurveId| match s.curves.get(l)?.kind {
        CurveKind::Line { a, b } | CurveKind::Arc { start: a, end: b, .. } if a == corner => Some(b),
        CurveKind::Line { a, b } | CurveKind::Arc { start: a, end: b, .. } if b == corner => Some(a),
        _ => None,
    };
    Some([(l1, other(l1)?), (l2, other(l2)?)])
}

/// A curve at a fillet corner as the fillet sees it: its whole line or circle.
#[derive(Debug, Clone, Copy)]
enum Carrier {
    /// A point and the unit direction.
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
}

impl Carrier {
    fn of(s: &Sketch, c: CurveId) -> Option<Self> {
        match s.curves.get(c)?.kind {
            CurveKind::Line { a, b } => {
                let (pa, pb) = (s.pos(a), s.pos(b));
                (pa.distance(pb) > 1e-12).then(|| Carrier::Line(pa, (pb - pa).normalize()))
            }
            CurveKind::Arc { .. } => s.arc_geom(c).map(|g| Carrier::Circle(g.center, g.radius)),
            _ => None,
        }
    }

    /// The carriers `r` away from it (both sides; a circle's inner one only if it's larger than
    /// nothing).
    fn offsets(self, r: f64) -> Vec<Carrier> {
        match self {
            Carrier::Line(p, u) => vec![Carrier::Line(p + u.perp() * r, u), Carrier::Line(p - u.perp() * r, u)],
            Carrier::Circle(c, q) => {
                let mut v = vec![Carrier::Circle(c, q + r)];
                if q - r > 1e-9 {
                    v.push(Carrier::Circle(c, q - r));
                }
                v
            }
        }
    }

    /// Where a circle centred at `o` touches it.
    fn foot(self, o: Vec2) -> Vec2 {
        match self {
            Carrier::Line(p, u) => p + u * (o - p).dot(u),
            Carrier::Circle(c, q) => c + (o - c).normalize() * q,
        }
    }

    /// Where two carriers cross.
    fn crossings(self, o: Carrier) -> Vec<Vec2> {
        match (self, o) {
            (Carrier::Line(p, u), Carrier::Line(q, v)) => {
                let den = u.cross(v);
                if den.abs() < 1e-12 {
                    return vec![];
                }
                vec![p + u * ((q - p).cross(v) / den)]
            }
            (Carrier::Line(p, u), Carrier::Circle(c, r)) | (Carrier::Circle(c, r), Carrier::Line(p, u)) => {
                let f = p + u * (c - p).dot(u);
                let h2 = r * r - f.distance(c).powi(2);
                if h2 < 0.0 {
                    return vec![];
                }
                let h = h2.sqrt();
                vec![f - u * h, f + u * h]
            }
            (Carrier::Circle(c0, r0), Carrier::Circle(c1, r1)) => {
                let d = c0.distance(c1);
                if d < 1e-12 || d > r0 + r1 || d < (r0 - r1).abs() {
                    return vec![];
                }
                let u = (c1 - c0) / d;
                let a = (r0 * r0 - r1 * r1 + d * d) / (2.0 * d);
                let h = (r0 * r0 - a * a).max(0.0).sqrt();
                let m = c0 + u * a;
                vec![m + u.perp() * h, m - u.perp() * h]
            }
        }
    }
}

/// True if `p`, on curve `c`'s carrier, lies on the curve strictly between its ends.
fn on_curve(s: &Sketch, c: CurveId, p: Vec2) -> bool {
    match s.curves.get(c).map(|c| c.kind) {
        Some(CurveKind::Line { a, b }) => {
            let (pa, pb) = (s.pos(a), s.pos(b));
            let t = (p - pa).dot(pb - pa) / pa.distance(pb).powi(2).max(1e-300);
            t > 1e-9 && t < 1.0 - 1e-9
        }
        Some(CurveKind::Arc { .. }) => s.arc_geom(c).is_some_and(|g| {
            let off = crate::geom::norm_angle((p - g.center).angle() - g.start_angle);
            off > 1e-9 && off < g.sweep - 1e-9
        }),
        _ => false,
    }
}

/// A fillet of `radius` where a line meets an arc or two arcs meet (S9.1): every circle of
/// that radius tangent to both carriers, kept if it touches both curves themselves and runs
/// smoothly from one into the other past the corner; the one touching nearest the corner.
fn fillet_geometry_curves(s: &Sketch, corner: PointId, radius: f64) -> Option<(Vec2, Vec2, Vec2)> {
    let [(c1, _), (c2, _)] = fillet_corner(s, corner)?;
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    let (k1, k2) = (Carrier::of(s, c1)?, Carrier::of(s, c2)?);
    let p = s.pos(corner);
    let (d1, d2) = (s.direction_from(c1, corner)?, s.direction_from(c2, corner)?);
    // A corner where the curves run on smoothly needs no fillet.
    if d1.cross(d2).abs() < 1e-6 && d1.dot(d2) < 0.0 {
        return None;
    }
    let mut best: Option<(f64, (Vec2, Vec2, Vec2))> = None;
    for o1 in k1.offsets(radius) {
        for o2 in k2.offsets(radius) {
            for o in o1.crossings(o2) {
                let (t1, t2) = (k1.foot(o), k2.foot(o));
                if !(on_curve(s, c1, t1) && on_curve(s, c2, t2)) {
                    continue;
                }
                // The short way round the fillet from t1 to t2 leaves t1 toward the corner and
                // arrives at t2 heading away from it.
                let ccw = (t1 - o).cross(t2 - o) > 0.0;
                let turn = |v: Vec2| if ccw { v.perp() } else { -v.perp() };
                let (at1, at2) = (turn(t1 - o), turn(t2 - o));
                if at1.dot(p - t1) <= 0.0 || at2.dot(t2 - p) <= 0.0 {
                    continue;
                }
                let score = t1.distance(p) + t2.distance(p);
                if best.is_none_or(|(b, _)| score < b) {
                    best = Some((score, (t1, t2, o)));
                }
            }
        }
    }
    best.map(|(_, g)| g)
}

/// Where a fillet of `radius` at `corner` touches its two lines, and its center: `None` if the
/// lines are in line or the fillet does not fit on them.
pub fn fillet_geometry(s: &Sketch, corner: PointId, radius: f64) -> Option<(Vec2, Vec2, Vec2)> {
    let Some([(_, a), (_, b)]) = corner_lines(s, corner) else {
        return fillet_geometry_curves(s, corner, radius);
    };
    let p = s.pos(corner);
    let (pa, pb) = (s.pos(a), s.pos(b));
    let (u1, u2) = ((pa - p).normalize(), (pb - p).normalize());
    let cos = u1.dot(u2).clamp(-1.0, 1.0);
    let theta = cos.acos();
    if !(1e-6..PI - 1e-6).contains(&theta) || radius.is_nan() || radius <= 0.0 {
        return None;
    }
    let t = radius / (theta / 2.0).tan();
    if t >= p.distance(pa) || t >= p.distance(pb) {
        return None;
    }
    let bis = (u1 + u2).normalize();
    let center = p + bis * (radius / (theta / 2.0).sin());
    Some((p + u1 * t, p + u2 * t, center))
}

/// The largest fillet that fits at `corner` (its lines' shorter length; where an arc meets it,
/// the largest radius [`fillet_geometry`] still finds, by bisection).
pub fn max_fillet(s: &Sketch, corner: PointId) -> Option<f64> {
    let Some([(_, a), (_, b)]) = corner_lines(s, corner) else {
        let [(_, a), (_, b)] = fillet_corner(s, corner)?;
        let p = s.pos(corner);
        let mut hi = p.distance(s.pos(a)).max(p.distance(s.pos(b))) * 4.0;
        let mut lo = 0.0;
        // Some radius must fit to begin with.
        let mut r = hi;
        while fillet_geometry(s, corner, r).is_none() {
            r /= 2.0;
            if r < 1e-9 {
                return None;
            }
        }
        lo = f64::max(lo, r);
        hi = hi.max(lo);
        if fillet_geometry(s, corner, hi).is_some() {
            return Some(hi);
        }
        for _ in 0..50 {
            let mid = (lo + hi) / 2.0;
            if fillet_geometry(s, corner, mid).is_some() { lo = mid } else { hi = mid }
        }
        return Some(lo);
    };
    let p = s.pos(corner);
    let (pa, pb) = (s.pos(a), s.pos(b));
    let (u1, u2) = ((pa - p).normalize(), (pb - p).normalize());
    let theta = u1.dot(u2).clamp(-1.0, 1.0).acos();
    Some(p.distance(pa).min(p.distance(pb)) * (theta / 2.0).tan())
}

/// Replaces `corner` with `with` in `line` (or an arc).
fn move_end(s: &mut Sketch, line: CurveId, corner: PointId, with: PointId) {
    if let Some(c) = s.curves.get_mut(line)
        && let CurveKind::Line { a, b } | CurveKind::Arc { start: a, end: b, .. } = &mut c.kind
    {
        if *a == corner {
            *a = with;
        } else if *b == corner {
            *b = with;
        }
    }
}

/// Fillets the corner where two lines, a line and an arc, or two arcs meet (S9.1). Returns
/// the fillet arc.
pub fn fillet(
    s: &mut Sketch,
    corner: PointId,
    radius: f64,
    equal_to: Option<CurveId>,
) -> Result<CurveId, String> {
    let [(l1, _), (l2, _)] = fillet_corner(s, corner).ok_or("not a corner of two lines or arcs")?;
    let (t1, t2, center) =
        fillet_geometry(s, corner, radius).ok_or("the fillet does not fit")?;
    let construction = s.curves[l1].construction && s.curves[l2].construction;
    let (p1, p2, pc) = (s.add_point(t1), s.add_point(t2), s.add_point(center));
    s.quiet.points.extend([p1, p2]);
    move_end(s, l1, corner, p1);
    move_end(s, l2, corner, p2);
    // Counter-clockwise, the short way round (toward the corner).
    let g = crate::ArcGeom::ccw(center, t1, t2);
    let (start, end) = if g.sweep <= PI { (p1, p2) } else { (p2, p1) };
    let arc = s.curves.insert(Curve {
        kind: CurveKind::Arc { center: pc, start, end },
        construction,
    });
    let pr = PointRef::Point;
    // The virtual sharp: the old corner, still on both lines.
    s.add_quiet_constraint(ConstraintOf::PointOnCurve(pr(corner), CurveRef::Curve(l1)));
    s.add_quiet_constraint(ConstraintOf::PointOnCurve(pr(corner), CurveRef::Curve(l2)));
    s.add_quiet_constraint(ConstraintOf::Tangent(CurveRef::Curve(arc), CurveRef::Curve(l1)));
    s.add_quiet_constraint(ConstraintOf::Tangent(CurveRef::Curve(arc), CurveRef::Curve(l2)));
    match equal_to.filter(|e| s.curves.contains_key(*e)) {
        Some(e) => {
            s.add_quiet_constraint(ConstraintOf::Equal(CurveRef::Curve(e), CurveRef::Curve(arc)));
        }
        None => {
            // "R…" outside the corner, along the bisector (`sketchfilletvertexexample.png`).
            let out = (s.pos(corner) - center).normalize();
            s.dimensions.insert(Dimension::new(
                DimensionKind::Radius { curve: arc },
                radius,
                out.angle(),
            ));
        }
    }
    Ok(arc)
}

/// Where a chamfer at `corner` meets its two lines, `d1` along the first and `d2` along the
/// second (in [`corner_lines`] order); `None` if it does not fit.
pub fn chamfer_geometry(s: &Sketch, corner: PointId, d1: f64, d2: f64) -> Option<(Vec2, Vec2)> {
    let [(_, a), (_, b)] = corner_lines(s, corner)?;
    let p = s.pos(corner);
    let (pa, pb) = (s.pos(a), s.pos(b));
    if !(d1 > 0.0 && d2 > 0.0) || d1 >= p.distance(pa) || d2 >= p.distance(pb) {
        return None;
    }
    let (u1, u2) = ((pa - p).normalize(), (pb - p).normalize());
    if u1.cross(u2).abs() < 1e-9 {
        return None;
    }
    Some((p + u1 * d1, p + u2 * d2))
}

/// A chamfer [`chamfer`] made: its line, its two construction extensions (their lengths are
/// the two distances) and its distance dimensions (none when linked to another chamfer).
#[derive(Debug, Clone, PartialEq)]
pub struct ChamferParts {
    pub line: CurveId,
    pub ext: [CurveId; 2],
    pub dims: Vec<crate::DimensionId>,
}

/// Chamfers the corner where two lines meet (S9.2): `d1` along the first line, `d2` along the
/// second. With `equal_to` (the extensions of a chamfer made earlier in the same use), its two
/// distances are held equal to that chamfer's instead of getting dimensions of their own.
pub fn chamfer(
    s: &mut Sketch,
    corner: PointId,
    d1: f64,
    d2: f64,
    equal_to: Option<[CurveId; 2]>,
) -> Result<ChamferParts, String> {
    let [(l1, o1), (l2, o2)] = corner_lines(s, corner).ok_or("not a corner of two lines")?;
    // A linked chamfer takes each distance along the edge running the same way as the first
    // chamfer's (the horizontal distance along a horizontal edge, T3 judge).
    let (mut d1, mut d2, mut equal_to) = (d1, d2, equal_to);
    if let Some(e) = equal_to.filter(|e| e.iter().all(|k| s.curves.contains_key(*k))) {
        let dir = |k: CurveId| {
            s.curve_ends(k)
                .map(|(a, b)| (s.pos(b) - s.pos(a)).normalize())
                .unwrap_or_default()
        };
        let c = s.pos(corner);
        let (u1, u2) = ((s.pos(o1) - c).normalize(), (s.pos(o2) - c).normalize());
        let (e0, e1) = (dir(e[0]), dir(e[1]));
        let straight = u1.dot(e0).abs() + u2.dot(e1).abs();
        let crossed = u1.dot(e1).abs() + u2.dot(e0).abs();
        if crossed > straight + 1e-9 {
            equal_to = Some([e[1], e[0]]);
            std::mem::swap(&mut d1, &mut d2);
        }
    }
    let (t1, t2) = chamfer_geometry(s, corner, d1, d2).ok_or("the chamfer does not fit")?;
    let construction = s.curves[l1].construction && s.curves[l2].construction;
    let (p1, p2) = (s.add_point(t1), s.add_point(t2));
    s.quiet.points.extend([p1, p2, corner]);
    move_end(s, l1, corner, p1);
    move_end(s, l2, corner, p2);
    let line = s.curves.insert(Curve {
        kind: CurveKind::Line { a: p1, b: p2 },
        construction,
    });
    // Dash-dot extensions from the chamfer's ends to the corner, in line with the edges.
    let mut ext = Vec::new();
    for (p, l) in [(p1, l1), (p2, l2)] {
        ext.push(s.curves.insert(Curve {
            kind: CurveKind::Line { a: corner, b: p },
            construction: true,
        }));
        s.add_quiet_constraint(ConstraintOf::PointOnCurve(PointRef::Point(corner), CurveRef::Curve(l)));
    }
    let ext = [ext[0], ext[1]];
    if let Some(e) = equal_to.filter(|e| e.iter().all(|k| s.curves.contains_key(*k))) {
        for i in 0..2 {
            s.add_quiet_constraint(ConstraintOf::Equal(CurveRef::Curve(e[i]), CurveRef::Curve(ext[i])));
        }
        return Ok(ChamferParts {
            line,
            ext,
            dims: Vec::new(),
        });
    }
    // The two distances, outside the corner (`chamfer-creation-01.png`).
    let c = s.pos(corner);
    let inside = (t1 + t2) * 0.5 - c;
    let dim = |s: &mut Sketch, p: PointId, t: Vec2, v: f64| {
        let u = (t - c).normalize();
        // `offset` is along −u.perp() from the corner (see `linear_frame`); away from the part.
        let n = -u.perp();
        let side = if n.dot(inside) > 0.0 { -1.0 } else { 1.0 };
        let mut d = Dimension::new(DimensionKind::Aligned { a: corner, b: p }, v, 0.0);
        d.offset = side * ((d1 + d2) * 0.4 + 3.0);
        s.dimensions.insert(d)
    };
    let k1 = dim(s, p1, t1, d1);
    let k2 = dim(s, p2, t2, d2);
    Ok(ChamferParts {
        line,
        ext,
        dims: vec![k1, k2],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solve::{self, Status};
    use crate::{SketchOp, region};

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    fn rect(s: &mut Sketch, w: f64, h: f64) {
        let c = [v(0.0, 0.0), v(w, 0.0), v(w, h), v(0.0, h)];
        SketchOp::Batch(vec![
            SketchOp::AddPolyline {
                points: c.to_vec(),
                closed: true,
                construction: false,
                label: "Add rectangle",
            },
            SketchOp::AddConstraints(crate::constraint::rectangle_constraints(c)),
        ])
        .apply(s)
        .unwrap();
    }

    fn side_lengths(s: &Sketch, parts: &PolygonParts) -> Vec<f64> {
        parts.sides.iter().map(|l| s.line_length(*l).unwrap()).collect()
    }

    fn circle_of(s: &Sketch) -> CurveId {
        s.curves
            .iter()
            .find(|(_, c)| matches!(c.kind, CurveKind::Circle { .. }))
            .unwrap()
            .0
    }

    #[test]
    fn midpoint_lines_grow_both_ways() {
        let (a, b) = midpoint_line(v(1.0, 2.0), v(4.0, 6.0));
        assert_eq!(a, v(-2.0, -2.0));
        assert_eq!(b, v(4.0, 6.0));
        assert_eq!(a.midpoint(b), v(1.0, 2.0));
    }

    #[test]
    fn aligned_rectangle_keeps_its_angle() {
        let c = aligned_corners(v(0.0, 0.0), v(30.0, 40.0), v(-8.0, 20.0));
        // The width is square to the first side.
        assert!((c[1] - c[0]).dot(c[2] - c[1]).abs() < 1e-9);
        assert!((c[0].distance(c[3]) - (v(-8.0, 20.0) - v(0.0, 0.0)).dot(v(-0.8, 0.6))).abs() < 1e-9);
        let mut s = Sketch::new();
        SketchOp::Batch(vec![
            SketchOp::AddPolyline {
                points: c.to_vec(),
                closed: true,
                construction: false,
                label: "Add rectangle",
            },
            SketchOp::AddConstraints(aligned_constraints(c)),
        ])
        .apply(&mut s)
        .unwrap();
        assert_eq!(s.constraints.len(), 3);
        // Four points: 8 unknowns, 3 constraints: position (2), angle, width and height.
        assert_eq!(solve::analyze(&s).dof, 5);
        assert_eq!(region::regions(&s).len(), 1);
    }

    #[test]
    fn three_point_circle() {
        let (c, r) = crate::geom::circle_through(v(10.0, 0.0), v(0.0, 10.0), v(-10.0, 0.0)).unwrap();
        assert!(c.distance(v(0.0, 0.0)) < 1e-9 && (r - 10.0).abs() < 1e-9);
        assert!(crate::geom::circle_through(v(0.0, 0.0), v(1.0, 1.0), v(2.0, 2.0)).is_none());
    }

    #[test]
    fn polygons_are_regular_and_keep_four_freedoms() {
        for inscribed in [false, true] {
            for n in 3..=8 {
                let mut s = Sketch::new();
                SketchOp::AddPolygon {
                    center: v(5.0, 5.0),
                    radius: 20.0,
                    angle: 0.3,
                    sides: n,
                    inscribed,
                    construction: false,
                }
                .apply(&mut s)
                .unwrap();
                let circle = circle_of(&s);
                let parts = polygon_parts(&s, circle, inscribed).unwrap();
                assert_eq!(parts.sides.len(), n as usize);
                let lens = side_lengths(&s, &parts);
                assert!(lens.iter().all(|l| (l - lens[0]).abs() < 1e-6));
                // Center (2), size and rotation.
                let a = solve::analyze(&s);
                assert_eq!(a.dof, 4, "n={n} inscribed={inscribed}");
                assert!(a.conflicting.is_empty());
                let far = parts.corners.iter().map(|p| s.pos(*p).distance(v(5.0, 5.0)));
                let r = if inscribed { 20.0 / (PI / n as f64).cos() } else { 20.0 };
                for d in far {
                    assert!((d - r).abs() < 1e-6, "n={n}");
                }
                // The side count is a value on the polygon.
                assert!(s.dimensions.values().any(|d| matches!(d.kind, DimensionKind::Sides { .. })
                    && d.value == n as f64));
            }
        }
    }

    #[test]
    fn polygon_diameter_sizes_it() {
        // S7.3: the circle's diameter sets the size; the sides follow.
        let mut s = Sketch::new();
        SketchOp::AddPolygon {
            center: v(0.0, 0.0),
            radius: 10.0,
            angle: 0.0,
            sides: 6,
            inscribed: false,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let circle = circle_of(&s);
        SketchOp::SetDimension {
            dimension: Dimension::new(DimensionKind::Diameter { curve: circle }, 50.0, 0.0),
            moves: vec![],
            radii: vec![(circle, 25.0)],
        }
        .apply(&mut s)
        .unwrap();
        let parts = polygon_parts(&s, circle, false).unwrap();
        for l in side_lengths(&s, &parts) {
            // A hexagon's side is its circumradius.
            assert!((l - 25.0).abs() < 1e-6);
        }
    }

    #[test]
    fn side_count_edits_rebuild_the_polygon() {
        for inscribed in [false, true] {
            let mut s = Sketch::new();
            SketchOp::AddPolygon {
                center: v(0.0, 0.0),
                radius: 10.0,
                angle: 0.5,
                sides: 6,
                inscribed,
                construction: false,
            }
            .apply(&mut s)
            .unwrap();
            let id = s
                .dimensions
                .iter()
                .find(|(_, d)| matches!(d.kind, DimensionKind::Sides { .. }))
                .unwrap()
                .0;
            SketchOp::SetDimensionValue { id, value: 8.0 }.apply(&mut s).unwrap();
            let circle = circle_of(&s);
            let parts = polygon_parts(&s, circle, inscribed).unwrap();
            assert_eq!(parts.sides.len(), 8);
            assert_eq!(s.dimensions[id].value, 8.0);
            assert_eq!(solve::analyze(&s).dof, 4);
            // Only the circle and the octagon are left (plus touch points).
            let lines = s.curves.values().filter(|c| matches!(c.kind, CurveKind::Line { .. })).count();
            assert_eq!(lines, 8);
            let expect_points = 1 + 8 + if inscribed { 8 } else { 0 };
            assert_eq!(s.points.len(), expect_points);
            // Out of range is refused.
            assert!(SketchOp::SetDimensionValue { id, value: 2.0 }.apply(&mut s.clone()).is_err());
            assert!(SketchOp::SetDimensionValue { id, value: 51.0 }.apply(&mut s.clone()).is_err());
        }
    }

    #[test]
    fn line_slot_follows_its_source() {
        let mut s = Sketch::new();
        SketchOp::AddPolyline {
            points: vec![v(0.0, 0.0), v(40.0, 0.0)],
            closed: false,
            construction: true,
            label: "Add line",
        }
        .apply(&mut s)
        .unwrap();
        let src = s.curves.keys().next().unwrap();
        SketchOp::Slot {
            source: src,
            width: 10.0,
            equal_to: None,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        // Two end arcs and two sides, a Ø10 on an end arc.
        assert_eq!(s.curves.len(), 5);
        let d = s.dimensions.values().next().unwrap();
        assert!(matches!(d.kind, DimensionKind::Diameter { .. }) && d.value == 10.0);
        let r = region::regions(&s);
        assert_eq!(r.len(), 1);
        let area = 40.0 * 10.0 + PI * 25.0;
        assert!((r[0].area() - area).abs() < 1e-6);
        // With the source fixed, the slot is fully defined.
        s.add_constraint(ConstraintOf::FixCurve(CurveRef::Curve(src)));
        assert_eq!(solve::analyze(&s).dof, 0);
        // S6.3: moving the source's end moves the slot.
        let (a, b) = s.curve_ends(src).unwrap();
        s.constraints.retain(|_, c| !matches!(c, ConstraintOf::FixCurve(_)));
        assert!(solve::drag(
            &mut s,
            &solve::Drag::Points(vec![(a, v(0.0, 0.0)), (b, v(60.0, 0.0))]),
            &Default::default(),
        ));
        let r = region::regions(&s);
        assert!((r[0].area() - (60.0 * 10.0 + PI * 25.0)).abs() < 1e-6);
        // S6.2: the width edits.
        let wid = s.dimensions.keys().next().unwrap();
        SketchOp::SetDimensionValue { id: wid, value: 16.0 }.apply(&mut s).unwrap();
        let r = region::regions(&s);
        assert!((r[0].area() - (60.0 * 16.0 + PI * 64.0)).abs() < 1e-6);
    }

    #[test]
    fn slots_made_together_share_a_width() {
        let mut s = Sketch::new();
        for y in [0.0, 30.0] {
            SketchOp::AddPolyline {
                points: vec![v(0.0, y), v(40.0, y)],
                closed: false,
                construction: true,
                label: "Add line",
            }
            .apply(&mut s)
            .unwrap();
        }
        let src: Vec<CurveId> = s.curves.keys().collect();
        let mut first = None;
        for c in &src {
            let cap = slot(&mut s, *c, 10.0, first, false).unwrap();
            first.get_or_insert(cap);
        }
        crate::solve::solve(&mut s);
        assert_eq!(s.dimensions.len(), 1);
        let wid = s.dimensions.keys().next().unwrap();
        SketchOp::SetDimensionValue { id: wid, value: 6.0 }.apply(&mut s).unwrap();
        let areas: Vec<f64> = region::regions(&s).iter().map(|r| r.area()).collect();
        assert_eq!(areas.len(), 2);
        for a in areas {
            assert!((a - (40.0 * 6.0 + PI * 9.0)).abs() < 1e-6);
        }
    }

    #[test]
    fn arc_slot() {
        let mut s = Sketch::new();
        SketchOp::AddArc {
            center: v(0.0, 0.0),
            start: v(50.0, 0.0),
            end: v(0.0, 50.0),
            construction: true,
        }
        .apply(&mut s)
        .unwrap();
        let src = s.curves.keys().next().unwrap();
        SketchOp::Slot {
            source: src,
            width: 10.0,
            equal_to: None,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let r = region::regions(&s);
        assert_eq!(r.len(), 1);
        // A quarter ring (R45..R55) plus a full Ø10 circle made of the two end halves.
        let area = PI / 4.0 * (55.0f64.powi(2) - 45.0f64.powi(2)) + PI * 25.0;
        assert!((r[0].area() - area).abs() < 1e-6, "{}", r[0].area());
        s.add_constraint(ConstraintOf::FixCurve(CurveRef::Curve(src)));
        let a = solve::analyze(&s);
        assert_eq!(a.dof, 0);
        assert!(a.conflicting.is_empty() && a.conflicting_dimensions.is_empty());
    }

    #[test]
    fn fillet_trims_and_is_tangent() {
        let mut s = Sketch::new();
        rect(&mut s, 50.0, 30.0);
        let corner = s.point_at(v(50.0, 30.0), 1e-9).unwrap();
        let arc = fillet(&mut s, corner, 5.0, None).unwrap();
        crate::solve::solve(&mut s);
        let g = s.arc_geom(arc).unwrap();
        assert!((g.radius - 5.0).abs() < 1e-9);
        assert!(g.center.distance(v(45.0, 25.0)) < 1e-9);
        // A quarter turn, the short way.
        assert!((g.sweep - PI / 2.0).abs() < 1e-9);
        // The lines end where the arc starts.
        assert!(s.point_at(v(45.0, 30.0), 1e-9).is_some());
        assert!(s.point_at(v(50.0, 25.0), 1e-9).is_some());
        // The virtual sharp stays, hollow, on both lines.
        assert!(s.hollow_point(corner));
        let r = region::regions(&s);
        assert!((r[0].area() - (1500.0 - 25.0 + PI * 25.0 / 4.0)).abs() < 1e-6);
        // Tangent + coincident (shared) + radius dimension.
        assert_eq!(
            s.constraints.values().filter(|c| matches!(c, ConstraintOf::Tangent(..))).count(),
            2
        );
        assert!(s.dimensions.values().any(|d| d.kind == DimensionKind::Radius { curve: arc }));
        // The radius drives it.
        let id = s.dimensions.keys().next().unwrap();
        SketchOp::SetDimensionValue { id, value: 8.0 }.apply(&mut s).unwrap();
        let g = s.arc_geom(arc).unwrap();
        assert!((g.radius - 8.0).abs() < 1e-6);
        assert!(g.center.distance(v(42.0, 22.0)) < 1e-6, "{:?}", g.center);
        // A second fillet in the same use is equal to the first.
        let c2 = s.point_at(v(0.0, 0.0), 1e-9).unwrap();
        let arc2 = fillet(&mut s, c2, 8.0, Some(arc)).unwrap();
        crate::solve::solve(&mut s);
        assert_eq!(s.dimensions.len(), 1);
        SketchOp::SetDimensionValue { id, value: 4.0 }.apply(&mut s).unwrap();
        assert!((s.arc_geom(arc2).unwrap().radius - 4.0).abs() < 1e-6);
        // Too big does not fit.
        let c3 = s.point_at(v(50.0, 0.0), 1e-9).unwrap();
        assert!(fillet(&mut s.clone(), c3, 40.0, None).is_err());
    }

    #[test]
    fn fillet_where_a_line_meets_an_arc() {
        // Final re-audit, S9.1: a line from the left ending at (40, 0) and an arc of radius 20
        // about (60, 0) rising from there (counter-clockwise from (60, 20) to (40, 0)). An R5
        // fillet outside the arc's circle: its center 5 above the line and 25 from (60, 0), at
        // (60 − √600, 5); it touches the line at (60 − √600, 0) and the arc at
        // (60 − 0.8·√600, 4).
        let mut s = Sketch::new();
        let (a, p, e) = (s.add_point(v(0.0, 0.0)), s.add_point(v(40.0, 0.0)), s.add_point(v(60.0, 20.0)));
        let c = s.add_point(v(60.0, 0.0));
        let line = s.curves.insert(Curve { kind: CurveKind::Line { a, b: p }, construction: false });
        let arc = s.curves.insert(Curve { kind: CurveKind::Arc { center: c, start: e, end: p }, construction: false });
        assert!(corner_lines(&s, p).is_none());
        assert!(fillet_corner(&s, p).is_some());
        let k = 600f64.sqrt();
        let (t1, t2, o) = fillet_geometry(&s, p, 5.0).unwrap();
        assert!(o.distance(v(60.0 - k, 5.0)) < 1e-9, "{o:?}");
        assert!(t1.distance(v(60.0 - k, 0.0)) < 1e-9 && t2.distance(v(60.0 - 0.8 * k, 4.0)) < 1e-9, "{t1:?} {t2:?}");
        let f = fillet(&mut s, p, 5.0, None).unwrap();
        crate::solve::solve(&mut s);
        assert!(crate::solve::conflicts(&s).is_empty());
        let g = s.arc_geom(f).unwrap();
        assert!((g.radius - 5.0).abs() < 1e-9 && g.center.distance(v(60.0 - k, 5.0)) < 1e-9);
        // The line and the arc end where the fillet starts; the arc keeps its circle.
        assert!(s.pos(s.curve_ends(line).unwrap().1).distance(v(60.0 - k, 0.0)) < 1e-9);
        let ga = s.arc_geom(arc).unwrap();
        assert!((ga.radius - 20.0).abs() < 1e-9 && ga.end().distance(v(60.0 - 0.8 * k, 4.0)) < 1e-9);
        // The corner stays as a hollow virtual sharp on both.
        assert!(s.hollow_point(p));
        // The radius drives it, still tangent to both: 8 from the line, 20 + 8 from the arc's
        // centre (the free geometry may move to make room).
        let id = s.dimensions.keys().next().unwrap();
        SketchOp::SetDimensionValue { id, value: 8.0 }.apply(&mut s).unwrap();
        let g = s.arc_geom(f).unwrap();
        assert!((g.radius - 8.0).abs() < 1e-6);
        let (la, lb) = s.curve_ends(line).unwrap();
        let (pa, pb) = (s.pos(la), s.pos(lb));
        assert!(((pb - pa).normalize().cross(g.center - pa).abs() - 8.0).abs() < 1e-6);
        let ga = s.arc_geom(arc).unwrap();
        assert!((g.center.distance(ga.center) - (ga.radius + 8.0)).abs() < 1e-6);
        // The largest that fits is found, and a bigger one is refused.
        let mut t = Sketch::new();
        let (a, p, e) = (t.add_point(v(0.0, 0.0)), t.add_point(v(40.0, 0.0)), t.add_point(v(60.0, 20.0)));
        let c = t.add_point(v(60.0, 0.0));
        t.curves.insert(Curve { kind: CurveKind::Line { a, b: p }, construction: false });
        t.curves.insert(Curve { kind: CurveKind::Arc { center: c, start: e, end: p }, construction: false });
        let max = max_fillet(&t, p).unwrap();
        assert!(max > 5.0 && fillet_geometry(&t, p, max * 1.01).is_none(), "{max}");
    }

    #[test]
    fn fillet_keeps_a_fully_defined_rectangle_defined() {
        let mut s = Sketch::new();
        rect(&mut s, 50.0, 30.0);
        let o = s.point_at(v(0.0, 0.0), 1e-9).unwrap();
        s.add_constraint(ConstraintOf::Coincident(PointRef::Point(o), PointRef::Origin));
        let (a, b, c) = (
            s.point_at(v(50.0, 0.0), 1e-9).unwrap(),
            s.point_at(v(50.0, 30.0), 1e-9).unwrap(),
            s.point_at(v(0.0, 30.0), 1e-9).unwrap(),
        );
        let _ = c;
        s.dimensions.insert(Dimension::new(DimensionKind::Horizontal { a: o, b: a }, 50.0, -5.0));
        s.dimensions.insert(Dimension::new(DimensionKind::Vertical { a, b }, 30.0, 5.0));
        assert_eq!(solve::analyze(&s).dof, 0);
        SketchOp::Fillet {
            corner: b,
            radius: 5.0,
            equal_to: None,
        }
        .apply(&mut s)
        .unwrap();
        let an = solve::analyze(&s);
        assert_eq!(an.dof, 0);
        assert!(an.curves.values().all(|st| *st == Status::Full));
    }

    #[test]
    fn chamfer_trims_with_two_distances() {
        let mut s = Sketch::new();
        rect(&mut s, 50.0, 30.0);
        let corner = s.point_at(v(50.0, 30.0), 1e-9).unwrap();
        let parts = chamfer(&mut s, corner, 5.0, 5.0, None).unwrap();
        let (line, d1, d2) = (parts.line, parts.dims[0], parts.dims[1]);
        crate::solve::solve(&mut s);
        assert!((s.line_length(line).unwrap() - 50f64.sqrt()).abs() < 1e-9);
        let r = region::regions(&s);
        assert_eq!(r.len(), 1);
        assert!((r[0].area() - (1500.0 - 12.5)).abs() < 1e-6);
        // Typed distances (Enter, then the second).
        SketchOp::SetDimensionValue { id: d1, value: 10.0 }.apply(&mut s).unwrap();
        SketchOp::SetDimensionValue { id: d2, value: 6.0 }.apply(&mut s).unwrap();
        let r = region::regions(&s);
        assert!((r[0].area() - (1500.0 - 30.0)).abs() < 1e-6, "{}", r[0].area());
        // The corner is still where it was, with construction extensions to it.
        assert!(s.pos(corner).distance(v(50.0, 30.0)) < 1e-6);
        assert_eq!(s.curves.values().filter(|c| c.construction).count(), 2);
        assert!(solve::analyze(&s).conflicting.is_empty());
        // A second chamfer in the same use is linked to the first: no dimensions of its own,
        // and editing the first's distances changes both.
        let c2 = s.point_at(v(0.0, 0.0), 1e-9).unwrap();
        let second = chamfer(&mut s, c2, 10.0, 6.0, Some(parts.ext)).unwrap();
        crate::solve::solve(&mut s);
        assert!(second.dims.is_empty());
        SketchOp::SetDimensionValue { id: d1, value: 4.0 }.apply(&mut s).unwrap();
        let len = |k: CurveId| s.line_length(k).unwrap();
        // Each distance goes along the edge running the same way as the first's (the
        // horizontal distance along a horizontal edge).
        let horizontal = |k: CurveId| {
            let (a, b) = s.curve_ends(k).unwrap();
            (s.pos(a).y - s.pos(b).y).abs() < 1e-6
        };
        for e in second.ext {
            let twin = parts.ext.into_iter().find(|f| horizontal(*f) == horizontal(e)).unwrap();
            assert!((len(e) - len(twin)).abs() < 1e-6);
        }
        assert!((len(parts.ext[0]) - 4.0).abs() < 1e-6);
        assert!(solve::analyze(&s).conflicting.is_empty());
        // Its pieces show no glyphs.
        assert!(s.quiet.points.contains(&c2));
    }

    #[test]
    fn points_are_dimensionable_and_saved() {
        let mut s = Sketch::new();
        SketchOp::AddPoint { pos: v(12.0, 7.0) }.apply(&mut s).unwrap();
        // A second point in the same place is refused.
        assert!(SketchOp::AddPoint { pos: v(12.0, 7.0) }.apply(&mut s.clone()).is_err());
        let p = s.points.keys().next().unwrap();
        let d = crate::dimension::propose(
            &s,
            &[crate::SketchEntity::Point(p), crate::SketchEntity::Origin],
            v(6.0, 12.0),
        )
        .unwrap();
        SketchOp::SetDimension {
            dimension: Dimension { value: 20.0, ..d },
            moves: vec![],
            radii: vec![],
        }
        .apply(&mut s)
        .unwrap();
        assert!((s.pos(p).x - 20.0).abs() < 1e-9);
        assert_eq!(solve::analyze(&s).dof, 1);
    }

    #[test]
    fn ellipse_solves_and_has_an_area() {
        let mut s = Sketch::new();
        SketchOp::AddEllipse {
            center: v(10.0, 5.0),
            major: v(40.0, 5.0),
            minor: 12.0,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let e = s.curves.keys().next().unwrap();
        let r = region::regions(&s);
        assert_eq!(r.len(), 1);
        assert!((r[0].area() - PI * 30.0 * 12.0).abs() < 1e-6);
        // Center (2), major point (2), minor radius.
        assert_eq!(solve::analyze(&s).dof, 5);
        // A point placed on it stays on it as the minor radius changes.
        SketchOp::AddPoint { pos: v(10.0, 17.0) }.apply(&mut s).unwrap();
        let p = s.point_at(v(10.0, 17.0), 1e-9).unwrap();
        SketchOp::AddConstraint {
            constraints: vec![ConstraintOf::PointOnCurve(PointRef::Point(p), CurveRef::Curve(e))],
            label: "Add coincident",
        }
        .apply(&mut s)
        .unwrap();
        // The dimensions are whole axes: 50 and 16 make semi-axes of 25 and 8.
        for (major, value) in [(true, 50.0), (false, 16.0)] {
            SketchOp::SetDimension {
                dimension: Dimension::new(DimensionKind::EllipseRadius { curve: e, major }, value, 0.0),
                moves: vec![],
                radii: vec![],
            }
            .apply(&mut s)
            .unwrap();
        }
        let g = s.ellipse_geom(e).unwrap();
        assert!((g.major() - 25.0).abs() < 1e-6 && (g.minor - 8.0).abs() < 1e-6);
        assert!(g.implicit(s.pos(p)).abs() < 1e-6);
        let r = region::regions(&s);
        assert!((r[0].area() - PI * 25.0 * 8.0).abs() < 1e-6);
        // Crossing a line splits it into regions whose areas add up.
        SketchOp::AddPolyline {
            points: vec![v(g.center.x, -40.0), v(g.center.x, 40.0)],
            closed: false,
            construction: false,
            label: "Add line",
        }
        .apply(&mut s)
        .unwrap();
        let total: f64 = region::regions(&s).iter().map(|r| r.area()).sum();
        assert_eq!(region::regions(&s).len(), 2);
        assert!((total - PI * 25.0 * 8.0).abs() < 1e-3, "{total}");
    }
}
