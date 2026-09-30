//! Snapping and inference: where a sketch tool places its next point, and which constraints
//! that point gets (`docs/PLAN.md`, "Snapping and inference engine"; `reference/onshape/
//! inference.md`).
//!
//! [`infer`] is a pure function of the cursor, the zoom, the sketch and the tool's state. It
//! returns every candidate in reach, best first, ranked by class and then by distance:
//!
//! 1. existing points: curve ends, centers, the sketch origin;
//! 2. midpoints of lines;
//! 3. intersections of lines, circles and arcs, including the projected plane axes;
//! 4. on-curve points (the nearest point of a line, circle, arc or axis), combined with an
//!    active horizontal/vertical guide when that guide crosses the curve near the cursor;
//! 5. and 6. guides: horizontal or vertical from the tool's anchor (the start of the line
//!    being drawn); parallel or perpendicular to a recently hovered ("woken") line,
//!    perpendicular to a line ending at the anchor (the previous segment of a chain), tangent
//!    to an arc ending there; and alignment with woken points. Two guides that cross near the
//!    cursor combine.
//!
//! All tolerances are in screen pixels (distances in mm times `px_per_mm`), so the same
//! screen-space cursor offset gives the same decision at any zoom. Holding Shift
//! ([`ToolContext::suppress`]) turns inference off: the cursor is used as it is.

use crate::geom::ArcGeom;
use crate::{
    ConstraintOf, ConstraintSpec, CurveId, CurveKind, CurveRef, CurveSpec, Orient, PointId,
    PointRef, PointSpec, Sketch, Vec2,
};

/// Snap to a point (end, center, origin, midpoint, intersection) within this many pixels.
pub const POINT_PX: f64 = 7.0;
/// Snap onto a curve within this many pixels.
pub const CURVE_PX: f64 = 5.0;
/// Horizontal/vertical guides snap when the cursor is less than this many pixels off them
/// (Onshape's is tight: 6 px off vertical does not snap, `NOTES.md`).
pub const GUIDE_PX: f64 = 5.0;

/// A curve as an infinite carrier plus the part of it that exists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Segment(Vec2, Vec2),
    /// An infinite line (a plane axis) through `point` along the unit vector `dir`.
    Line {
        point: Vec2,
        dir: Vec2,
    },
    Circle {
        center: Vec2,
        radius: f64,
    },
    Arc(ArcGeom),
    Ellipse(crate::geom::EllipseGeom),
}

impl Shape {
    /// The nearest point of the shape to `p`.
    pub fn closest(&self, p: Vec2) -> Vec2 {
        match *self {
            Shape::Segment(a, b) => {
                let ab = b - a;
                let len2 = ab.dot(ab);
                if len2 < 1e-300 {
                    return a;
                }
                a + ab * ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
            }
            Shape::Line { point, dir } => point + dir * (p - point).dot(dir),
            Shape::Circle { center, radius } => {
                let d = p - center;
                if d.length() < 1e-300 {
                    center + Vec2::new(radius, 0.0)
                } else {
                    center + d.normalize() * radius
                }
            }
            Shape::Arc(g) => {
                let d = p - g.center;
                if d.length() > 1e-300 && g.contains_angle(d.angle()) {
                    g.center + d.normalize() * g.radius
                } else if p.distance(g.start()) <= p.distance(g.end()) {
                    g.start()
                } else {
                    g.end()
                }
            }
            Shape::Ellipse(g) => g.closest(p),
        }
    }

    /// True if `p`, a point on the carrier, lies on the existing part.
    fn contains(&self, p: Vec2) -> bool {
        match *self {
            Shape::Segment(a, b) => {
                let ab = b - a;
                let len2 = ab.dot(ab);
                let t = (p - a).dot(ab) / len2.max(1e-300);
                (-1e-9..=1.0 + 1e-9).contains(&t)
            }
            Shape::Line { .. } | Shape::Circle { .. } | Shape::Ellipse(_) => true,
            Shape::Arc(g) => g.contains_angle((p - g.center).angle()),
        }
    }

    fn carrier(&self) -> Option<Carrier> {
        Some(match *self {
            Shape::Segment(a, b) => Carrier::Line(a, (b - a).normalize()),
            Shape::Line { point, dir } => Carrier::Line(point, dir),
            Shape::Circle { center, radius } => Carrier::Circle(center, radius),
            Shape::Arc(g) => Carrier::Circle(g.center, g.radius),
            // Crossings with an ellipse are found numerically (see `ellipse_crossings`).
            Shape::Ellipse(_) => return None,
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum Carrier {
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
}

/// Where two infinite lines (point, unit direction) cross; `None` if they are parallel.
pub fn line_line(p: Vec2, d: Vec2, q: Vec2, e: Vec2) -> Option<Vec2> {
    let den = d.cross(e);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (q - p).cross(e) / den;
    Some(p + d * t)
}

/// Where an infinite line (point, unit direction) crosses a circle (0, 1 or 2 points).
pub fn line_circle(p: Vec2, d: Vec2, c: Vec2, r: f64) -> Vec<Vec2> {
    let foot = p + d * (c - p).dot(d);
    let h2 = r * r - (foot - c).dot(foot - c);
    if h2 < -1e-12 * r * r {
        return vec![];
    }
    let h = h2.max(0.0).sqrt();
    if h < 1e-12 * r {
        return vec![foot];
    }
    vec![foot - d * h, foot + d * h]
}

/// Where two circles cross (0, 1 or 2 points; none for concentric circles).
pub fn circle_circle(c0: Vec2, r0: f64, c1: Vec2, r1: f64) -> Vec<Vec2> {
    let d = c0.distance(c1);
    let scale = r0.max(r1).max(1e-300);
    if d < 1e-12 * scale || d > r0 + r1 + 1e-12 * scale || d < (r0 - r1).abs() - 1e-12 * scale {
        return vec![];
    }
    let a = (r0 * r0 - r1 * r1 + d * d) / (2.0 * d);
    let h2 = r0 * r0 - a * a;
    let u = (c1 - c0) / d;
    let m = c0 + u * a;
    if h2 <= 1e-12 * scale * scale {
        return vec![m];
    }
    let h = h2.sqrt();
    vec![m + u.perp() * h, m - u.perp() * h]
}

/// Where an ellipse's whole curve crosses another shape's whole carrier (Final re-audit, S8):
/// sign changes of the other's side function around the ellipse, refined by bisection.
fn ellipse_crossings(g: &crate::geom::EllipseGeom, other: &Shape) -> Vec<Vec2> {
    use std::f64::consts::TAU;
    let side = |p: Vec2| match *other {
        Shape::Segment(a, b) => (b - a).normalize().cross(p - a),
        Shape::Line { point, dir } => dir.cross(p - point),
        Shape::Circle { center, radius } => p.distance(center) - radius,
        Shape::Arc(h) => p.distance(h.center) - h.radius,
        Shape::Ellipse(h) => h.implicit(p),
    };
    const N: usize = 720;
    let f = |t: f64| side(g.point_at(t));
    let mut out = Vec::new();
    let mut prev = f(0.0);
    for i in 1..=N {
        let (ta, tb) = ((i - 1) as f64 * TAU / N as f64, i as f64 * TAU / N as f64);
        let cur = f(tb);
        if prev == 0.0 {
            out.push(g.point_at(ta));
        } else if cur != 0.0 && prev.signum() != cur.signum() {
            let (mut lo, mut hi, mut flo) = (ta, tb, prev);
            for _ in 0..60 {
                let mid = (lo + hi) / 2.0;
                let fm = f(mid);
                if fm.signum() == flo.signum() {
                    lo = mid;
                    flo = fm;
                } else {
                    hi = mid;
                }
            }
            out.push(g.point_at((lo + hi) / 2.0));
        }
        prev = cur;
    }
    out
}

/// Where two shapes cross, on the existing parts of both.
pub fn intersect(a: &Shape, b: &Shape) -> Vec<Vec2> {
    // An ellipse crossing anything (Final re-audit, S8: intersection inference on ellipses).
    if let Shape::Ellipse(g) = a {
        return ellipse_crossings(g, b).into_iter().filter(|p| b.contains(*p)).collect();
    }
    if let Shape::Ellipse(g) = b {
        return ellipse_crossings(g, a).into_iter().filter(|p| a.contains(*p)).collect();
    }
    let (Some(ca), Some(cb)) = (a.carrier(), b.carrier()) else {
        return Vec::new();
    };
    let pts = match (ca, cb) {
        (Carrier::Line(p, d), Carrier::Line(q, e)) => line_line(p, d, q, e).into_iter().collect(),
        (Carrier::Line(p, d), Carrier::Circle(c, r))
        | (Carrier::Circle(c, r), Carrier::Line(p, d)) => line_circle(p, d, c, r),
        (Carrier::Circle(c0, r0), Carrier::Circle(c1, r1)) => circle_circle(c0, r0, c1, r1),
    };
    pts.into_iter()
        .filter(|p| a.contains(*p) && b.contains(*p))
        .collect()
}

/// Every curve of the sketch as a [`Shape`], plus the two plane axes.
pub fn shapes(s: &Sketch) -> Vec<(CurveRef, Shape)> {
    let mut out: Vec<(CurveRef, Shape)> = s
        .curves
        .iter()
        .filter_map(|(k, c)| {
            let shape = match c.kind {
                CurveKind::Line { a, b } => Shape::Segment(s.pos(a), s.pos(b)),
                CurveKind::Circle { center, radius } => Shape::Circle {
                    center: s.pos(center),
                    radius,
                },
                CurveKind::Arc { .. } => Shape::Arc(s.arc_geom(k)?),
                CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => Shape::Ellipse(s.ellipse_geom(k)?),
                CurveKind::Spline { .. } => return None,
                // Nothing snaps onto a Bézier curve (its ends and handles are points).
                CurveKind::Bezier { .. } => return None,
            };
            Some((CurveRef::Curve(k), shape))
        })
        .collect();
    out.push((
        CurveRef::XAxis,
        Shape::Line {
            point: Vec2::ZERO,
            dir: Vec2::new(1.0, 0.0),
        },
    ));
    out.push((
        CurveRef::YAxis,
        Shape::Line {
            point: Vec2::ZERO,
            dir: Vec2::new(0.0, 1.0),
        },
    ));
    out
}

/// What the active tool is doing, for inference.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolContext {
    /// The point the new piece starts from (the line being drawn); it is not a snap target.
    pub anchor: Option<Vec2>,
    /// Offer horizontal/vertical from the anchor (the line tool).
    pub hv_from_anchor: bool,
    /// Recently hovered points, most recent first: alignment guides come only from these.
    pub woken: Vec<PointRef>,
    /// Recently hovered lines: parallel and perpendicular guides come from these (and from
    /// lines ending at the anchor).
    pub woken_curves: Vec<CurveId>,
    /// Shift is held: no inference.
    pub suppress: bool,
    /// Points that are not targets (a dragged point, S11.4).
    pub exclude: Vec<PointId>,
    /// Curves that are not targets (the ones a dragged point belongs to).
    pub exclude_curves: Vec<CurveId>,
}

/// The reference a horizontal or vertical guide runs through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Guide {
    /// The tool's anchor: the new line itself is horizontal or vertical.
    Anchor,
    /// Alignment with a woken point.
    Point(PointRef),
}

/// A constraint the placed point gets when the click is committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placed {
    /// On an existing point (shared structurally) or the origin.
    Coincident(PointRef),
    OnCurve(CurveRef),
    Midpoint(CurveId),
    /// Level with the guide's point (for [`Guide::Anchor`]: the new line is horizontal).
    Horizontal(Guide),
    Vertical(Guide),
    /// The new line (from the anchor) is parallel to this line.
    Parallel(CurveRef),
    /// The new line is perpendicular to this line.
    Perpendicular(CurveRef),
    /// The new line is tangent to this arc (which ends at the anchor).
    Tangent(CurveRef),
}

impl Placed {
    /// The existing curve this constraint relates the new geometry to, for highlighting.
    pub fn reference_curve(&self) -> Option<CurveRef> {
        match *self {
            Placed::OnCurve(c)
            | Placed::Parallel(c)
            | Placed::Perpendicular(c)
            | Placed::Tangent(c) => Some(c),
            Placed::Midpoint(c) => Some(CurveRef::Curve(c)),
            _ => None,
        }
    }
}

/// What a candidate snaps to (drives the feedback: square, orange curve, glyphs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Point(PointId),
    Origin,
    Midpoint(CurveId),
    Intersection(CurveRef, CurveRef),
    OnCurve(CurveRef),
    /// Only horizontal/vertical guides (see the candidate's constraints).
    Guide,
}

/// A place the cursor can snap to.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub pos: Vec2,
    pub kind: Kind,
    /// The constraints the placed point gets.
    pub constraints: Vec<Placed>,
    /// Distance from the cursor, in pixels.
    pub distance_px: f64,
}

impl Candidate {
    /// The guides (with their direction: true for horizontal) this candidate uses.
    pub fn guides(&self) -> impl Iterator<Item = (bool, Guide)> + '_ {
        self.constraints.iter().filter_map(|c| match *c {
            Placed::Horizontal(g) => Some((true, g)),
            Placed::Vertical(g) => Some((false, g)),
            _ => None,
        })
    }
}

fn point_ref_pos(s: &Sketch, r: PointRef) -> Option<Vec2> {
    match r {
        PointRef::Point(p) => s.points.get(p).map(|p| p.pos),
        PointRef::Origin => Some(Vec2::ZERO),
    }
}

/// A guide line through `through` along the unit vector `dir`, and the constraint snapping
/// onto it gives.
#[derive(Debug, Clone, Copy)]
struct ActiveGuide {
    through: Vec2,
    dir: Vec2,
    placed: Placed,
    distance_px: f64,
}

impl ActiveGuide {
    fn shape(&self) -> Shape {
        Shape::Line {
            point: self.through,
            dir: self.dir,
        }
    }

    fn project(&self, p: Vec2) -> Vec2 {
        self.through + self.dir * (p - self.through).dot(self.dir)
    }
}

/// The snap candidates for `cursor` (sketch mm), best first. Empty when nothing is in reach or
/// inference is suppressed; the tool then uses the cursor as it is.
pub fn infer(cursor: Vec2, px_per_mm: f64, s: &Sketch, ctx: &ToolContext) -> Vec<Candidate> {
    if ctx.suppress || !px_per_mm.is_finite() || px_per_mm <= 0.0 {
        return Vec::new();
    }
    let px = |d: f64| d * px_per_mm;
    // The anchor itself is not a target (the piece would have no length).
    let away_from_anchor = |p: Vec2| ctx.anchor.is_none_or(|a| px(a.distance(p)) >= 1.0);
    let by_distance = |v: &mut Vec<Candidate>| {
        v.sort_by(|a, b| a.distance_px.total_cmp(&b.distance_px));
    };
    let mut out = Vec::new();

    // 1. Points.
    let mut points: Vec<Candidate> = s
        .points
        .iter()
        .filter(|(k, _)| !ctx.exclude.contains(k))
        .map(|(k, p)| (PointRef::Point(k), Kind::Point(k), p.pos))
        .chain(std::iter::once((
            PointRef::Origin,
            Kind::Origin,
            Vec2::ZERO,
        )))
        .filter(|(_, _, pos)| away_from_anchor(*pos))
        .filter_map(|(r, kind, pos)| {
            let d = px(pos.distance(cursor));
            (d <= POINT_PX).then(|| Candidate {
                pos,
                kind,
                constraints: vec![Placed::Coincident(r)],
                distance_px: d,
            })
        })
        .collect();
    by_distance(&mut points);
    out.extend(points);

    // 2. Midpoints of lines and arcs.
    let mut mids: Vec<Candidate> = s
        .curves
        .iter()
        .filter(|(k, _)| !ctx.exclude_curves.contains(k))
        .filter_map(|(k, c)| match c.kind {
            CurveKind::Line { a, b } => Some((k, s.pos(a).midpoint(s.pos(b)))),
            CurveKind::Arc { .. } => Some((k, s.arc_geom(k)?.mid())),
            _ => None,
        })
        .filter(|(_, m)| away_from_anchor(*m))
        .filter_map(|(k, m)| {
            let d = px(m.distance(cursor));
            (d <= POINT_PX).then(|| Candidate {
                pos: m,
                kind: Kind::Midpoint(k),
                constraints: vec![Placed::Midpoint(k)],
                distance_px: d,
            })
        })
        .collect();
    by_distance(&mut mids);
    out.extend(mids);

    // 2b. An ellipse's other axis ends (its major point is class 1): points on it.
    let mut ends: Vec<Candidate> = s
        .curves
        .keys()
        .filter(|k| !ctx.exclude_curves.contains(k))
        .filter_map(|k| s.ellipse_geom(k).map(|g| (k, g)))
        .flat_map(|(k, g)| {
            [g.center - g.a, g.center + g.b(), g.center - g.b()].map(|p| (k, p))
        })
        .filter(|(_, p)| away_from_anchor(*p))
        .filter_map(|(k, p)| {
            let d = px(p.distance(cursor));
            (d <= POINT_PX).then(|| Candidate {
                pos: p,
                kind: Kind::OnCurve(CurveRef::Curve(k)),
                constraints: vec![Placed::OnCurve(CurveRef::Curve(k))],
                distance_px: d,
            })
        })
        .collect();
    by_distance(&mut ends);
    out.extend(ends);

    // 3. Intersections (not where a point already is: that is class 1).
    let mut shapes = shapes(s);
    shapes.retain(|(r, _)| !matches!(r, CurveRef::Curve(k) if ctx.exclude_curves.contains(k)));
    let is_existing_point = |p: Vec2| {
        px(p.distance(Vec2::ZERO)) < 1e-6
            || s
                .points
                .iter()
                .any(|(k, q)| !ctx.exclude.contains(&k) && px(q.pos.distance(p)) < 1e-6)
    };
    let mut crossings = Vec::new();
    // Only curves near the cursor can cross there.
    let near: Vec<&(CurveRef, Shape)> = shapes
        .iter()
        .filter(|(_, a)| px(a.closest(cursor).distance(cursor)) <= POINT_PX)
        .collect();
    for (i, (ra, a)) in near.iter().map(|x| (&x.0, &x.1)).enumerate() {
        for (rb, b) in near[i + 1..].iter().map(|x| (&x.0, &x.1)) {
            for p in intersect(a, b) {
                let d = px(p.distance(cursor));
                if d <= POINT_PX && away_from_anchor(p) && !is_existing_point(p) {
                    crossings.push(Candidate {
                        pos: p,
                        kind: Kind::Intersection(*ra, *rb),
                        constraints: vec![Placed::OnCurve(*ra), Placed::OnCurve(*rb)],
                        distance_px: d,
                    });
                }
            }
        }
    }
    by_distance(&mut crossings);
    out.extend(crossings);

    // Guides in reach (used by classes 4–6).
    let mut guides: Vec<ActiveGuide> = Vec::new();
    let mut add_guide = |through: Vec2, dir: Vec2, placed: Placed| {
        let d = cursor - through;
        let off = px(d.cross(dir).abs());
        if off < GUIDE_PX && px(d.dot(dir).abs()) > GUIDE_PX {
            guides.push(ActiveGuide {
                through,
                dir,
                placed,
                distance_px: off,
            });
        }
    };
    let (x, y) = (Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0));
    // Directions along an axis are covered by horizontal/vertical.
    let oblique = |d: Vec2| d.x.abs() > 1e-9 && d.y.abs() > 1e-9;
    if ctx.hv_from_anchor
        && let Some(a) = ctx.anchor
    {
        add_guide(a, x, Placed::Horizontal(Guide::Anchor));
        add_guide(a, y, Placed::Vertical(Guide::Anchor));
        let at_anchor = |p: PointId| px(s.pos(p).distance(a)) < 1e-6;
        // Lines and arcs ending at the anchor: perpendicular / tangent.
        for (k, c) in &s.curves {
            match c.kind {
                CurveKind::Line { a: p, b: q } if at_anchor(p) || at_anchor(q) => {
                    let u = (s.pos(q) - s.pos(p)).normalize();
                    if oblique(u) {
                        add_guide(a, u.perp(), Placed::Perpendicular(CurveRef::Curve(k)));
                    }
                }
                CurveKind::Arc { start, end, .. } if at_anchor(start) || at_anchor(end) => {
                    if let Some(g) = s.arc_geom(k) {
                        let t = if at_anchor(start) {
                            g.start_tangent()
                        } else {
                            g.end_tangent()
                        };
                        if oblique(t) {
                            add_guide(a, t, Placed::Tangent(CurveRef::Curve(k)));
                        }
                    }
                }
                _ => {}
            }
        }
        // Woken lines: parallel and perpendicular.
        for k in &ctx.woken_curves {
            if let Some(CurveKind::Line { a: p, b: q }) = s.curves.get(*k).map(|c| c.kind) {
                if at_anchor(p) || at_anchor(q) {
                    continue;
                }
                let u = (s.pos(q) - s.pos(p)).normalize();
                if oblique(u) {
                    add_guide(a, u, Placed::Parallel(CurveRef::Curve(*k)));
                    add_guide(a, u.perp(), Placed::Perpendicular(CurveRef::Curve(*k)));
                }
            }
        }
    }
    for w in &ctx.woken {
        if matches!(w, PointRef::Point(k) if ctx.exclude.contains(k)) {
            continue;
        }
        if let Some(p) = point_ref_pos(s, *w)
            && ctx.anchor.is_none_or(|a| px(a.distance(p)) > 1e-6)
        {
            add_guide(p, x, Placed::Horizontal(Guide::Point(*w)));
            add_guide(p, y, Placed::Vertical(Guide::Point(*w)));
        }
    }
    // Nearest first (anchor guides first on ties: they were added first).
    guides.sort_by(|a, b| a.distance_px.total_cmp(&b.distance_px));

    // 4. On-curve, possibly where a guide crosses the curve.
    // A line drawn from a point on a plane axis along that axis is horizontal or vertical (the
    // guide from the anchor gives that), not "on the axis" (`ex2-step2.png`).
    let along_axis = |r: CurveRef| {
        ctx.hv_from_anchor
            && ctx.anchor.is_some_and(|a| match r {
                CurveRef::XAxis => px(a.y.abs()) < 1e-6,
                CurveRef::YAxis => px(a.x.abs()) < 1e-6,
                CurveRef::Curve(_) => false,
            })
    };
    let mut on_curve: Vec<Candidate> = shapes
        .iter()
        .filter(|(r, _)| !along_axis(*r))
        .filter_map(|(r, shape)| {
            let q = shape.closest(cursor);
            let d = px(q.distance(cursor));
            if d > CURVE_PX || !away_from_anchor(q) {
                return None;
            }
            let crossing = guides.iter().find_map(|g| {
                intersect(&g.shape(), shape)
                    .into_iter()
                    .map(|p| (p, px(p.distance(cursor))))
                    .filter(|(p, dp)| *dp <= POINT_PX && away_from_anchor(*p))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(p, dp)| (p, dp, g.placed))
            });
            Some(match crossing {
                Some((p, dp, placed)) => Candidate {
                    pos: p,
                    kind: Kind::OnCurve(*r),
                    constraints: vec![Placed::OnCurve(*r), placed],
                    distance_px: dp,
                },
                None => Candidate {
                    pos: q,
                    kind: Kind::OnCurve(*r),
                    constraints: vec![Placed::OnCurve(*r)],
                    distance_px: d,
                },
            })
        })
        .collect();
    by_distance(&mut on_curve);
    out.extend(on_curve);

    // 5–6. Guides: the nearest one, combined with the nearest other one that crosses it near
    // the cursor.
    if let Some(g1) = guides.first() {
        let second = guides[1..].iter().find_map(|g2| {
            let p = line_line(g1.through, g1.dir, g2.through, g2.dir)?;
            (px(p.distance(cursor)) < GUIDE_PX * 1.5).then_some((g2, p))
        });
        let (pos, constraints, distance_px) = match second {
            Some((g2, p)) => (
                p,
                vec![g1.placed, g2.placed],
                g1.distance_px.max(g2.distance_px),
            ),
            None => (g1.project(cursor), vec![g1.placed], g1.distance_px),
        };
        if away_from_anchor(pos) {
            out.push(Candidate {
                pos,
                kind: Kind::Guide,
                constraints,
                distance_px,
            });
        }
    }
    out
}

/// The constraints (as specs, for [`crate::SketchOp::AddConstraints`]) that a point placed at
/// `pos` with the inferred `placed` constraints gets. `anchor` is the tool's anchor; when
/// `line_from_anchor` is set (the line tool), a guide through the anchor makes the new line
/// horizontal or vertical, otherwise it aligns the two points.
///
/// Coincidence with an existing point needs no record: the new geometry shares that point.
pub fn placed_specs(
    s: &Sketch,
    pos: Vec2,
    placed: &[Placed],
    anchor: Option<Vec2>,
    line_from_anchor: bool,
) -> Vec<ConstraintSpec> {
    let here = PointSpec::At(pos);
    let point = |r: PointRef| match r {
        PointRef::Point(p) => PointSpec::At(s.pos(p)),
        PointRef::Origin => PointSpec::Origin,
    };
    let curve = |r: CurveRef| match r {
        CurveRef::Curve(c) => CurveSpec::Id(c),
        CurveRef::XAxis => CurveSpec::XAxis,
        CurveRef::YAxis => CurveSpec::YAxis,
    };
    let orient = |g: Guide| -> Option<Orient<PointSpec, CurveSpec>> {
        match g {
            Guide::Anchor => {
                let a = anchor?;
                Some(if line_from_anchor {
                    Orient::Line(CurveSpec::Between(a, pos))
                } else {
                    Orient::Points(here, PointSpec::At(a))
                })
            }
            Guide::Point(r) => Some(Orient::Points(here, point(r))),
        }
    };
    let new_line = || {
        anchor
            .filter(|_| line_from_anchor)
            .map(|a| CurveSpec::Between(a, pos))
    };
    placed
        .iter()
        .filter_map(|p| match *p {
            Placed::Coincident(PointRef::Point(_)) => None,
            Placed::Coincident(PointRef::Origin) => {
                Some(ConstraintOf::Coincident(here, PointSpec::Origin))
            }
            Placed::OnCurve(c) => Some(ConstraintOf::PointOnCurve(here, curve(c))),
            Placed::Midpoint(c) => Some(ConstraintOf::Midpoint(here, CurveSpec::Id(c))),
            Placed::Horizontal(g) => orient(g).map(ConstraintOf::Horizontal),
            Placed::Vertical(g) => orient(g).map(ConstraintOf::Vertical),
            Placed::Parallel(c) => new_line().map(|l| ConstraintOf::Parallel(l, curve(c))),
            Placed::Perpendicular(c) => {
                new_line().map(|l| ConstraintOf::Perpendicular(l, curve(c)))
            }
            Placed::Tangent(c) => new_line().map(|l| ConstraintOf::Tangent(curve(c), l)),
        })
        .collect()
}

/// A dragged point's snap (S11.4): lying on a plane axis reads as being level with the origin
/// (a point dragged until it is straight above the origin gets Vertical, as the course
/// describes), with the origin's dotted alignment guide instead of the highlighted axis.
pub fn drag_candidate(mut c: Candidate) -> Candidate {
    let mut aligned = false;
    for p in &mut c.constraints {
        match *p {
            Placed::OnCurve(CurveRef::XAxis) => {
                *p = Placed::Horizontal(Guide::Point(PointRef::Origin));
                aligned = true;
            }
            Placed::OnCurve(CurveRef::YAxis) => {
                *p = Placed::Vertical(Guide::Point(PointRef::Origin));
                aligned = true;
            }
            _ => {}
        }
    }
    if aligned && matches!(c.kind, Kind::OnCurve(CurveRef::XAxis | CurveRef::YAxis)) {
        c.kind = Kind::Guide;
    }
    c
}

/// S11.4: the constraints a dragged point `p` gets where inference snapped it (`placed`, from
/// [`infer`] with the point and its curves excluded): coincident with a point or the origin,
/// on a curve, at a midpoint, or level with a woken point.
pub fn drag_constraints(p: PointId, placed: &[Placed]) -> Vec<crate::Constraint> {
    let here = PointRef::Point(p);
    placed
        .iter()
        .filter_map(|c| match *c {
            Placed::Coincident(r) => Some(ConstraintOf::Coincident(here, r)),
            Placed::OnCurve(c) => Some(ConstraintOf::PointOnCurve(here, c)),
            Placed::Midpoint(c) => Some(ConstraintOf::Midpoint(here, CurveRef::Curve(c))),
            Placed::Horizontal(Guide::Point(r)) => {
                Some(ConstraintOf::Horizontal(Orient::Points(here, r)))
            }
            Placed::Vertical(Guide::Point(r)) => Some(ConstraintOf::Vertical(Orient::Points(here, r))),
            _ => None,
        })
        .collect()
}

/// [`drag_constraints`] without the ones the dragged point `p` already met where the drag
/// started (`base`, the sketch before the drag): dropping a point back where it was, or where it
/// was already level with a point or on a curve, adds nothing (Onshape infers what the drop
/// changes, not what already held).
pub fn drag_constraints_from(base: &Sketch, p: PointId, placed: &[Placed]) -> Vec<crate::Constraint> {
    let Some(start) = base.points.get(p).map(|q| q.pos) else {
        return Vec::new();
    };
    const EPS: f64 = 1e-6;
    let pos = |r: PointRef| point_ref_pos(base, r);
    let on = |c: CurveRef| {
        shapes(base)
            .into_iter()
            .find(|(r, _)| *r == c)
            .is_some_and(|(_, sh)| sh.closest(start).distance(start) < EPS)
    };
    drag_constraints(p, placed)
        .into_iter()
        .filter(|c| {
            let held = match *c {
                ConstraintOf::Coincident(_, r) => pos(r).is_some_and(|q| q.distance(start) < EPS),
                ConstraintOf::PointOnCurve(_, c) => on(c),
                ConstraintOf::Midpoint(_, CurveRef::Curve(k)) => match base.curves.get(k).map(|c| c.kind) {
                    Some(CurveKind::Line { a, b }) => base.pos(a).midpoint(base.pos(b)).distance(start) < EPS,
                    _ => false,
                },
                ConstraintOf::Horizontal(Orient::Points(_, r)) => {
                    pos(r).is_some_and(|q| (q.y - start.y).abs() < EPS)
                }
                ConstraintOf::Vertical(Orient::Points(_, r)) => {
                    pos(r).is_some_and(|q| (q.x - start.x).abs() < EPS)
                }
                _ => false,
            };
            !held
        })
        .collect()
}

/// Remembers recently hovered points (or lines) for guides ("wake up", `inference.md`).
pub fn wake<T: PartialEq>(woken: &mut Vec<T>, p: T, max: usize) {
    woken.retain(|w| *w != p);
    woken.insert(0, p);
    woken.truncate(max);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SketchOp;
    use proptest::prelude::*;

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
            .find(|(_, c)| {
                matches!(c.kind, CurveKind::Line { a: p, b: q } if s.pos(p) == a && s.pos(q) == b)
            })
            .unwrap()
            .0
    }

    fn circle(s: &mut Sketch, c: Vec2, r: f64) -> CurveId {
        SketchOp::AddCircle {
            center: c,
            radius: r,
            construction: false,
        }
        .apply(s)
        .unwrap();
        s.curves
            .iter()
            .find(|(_, cv)| matches!(cv.kind, CurveKind::Circle { radius, .. } if radius == r))
            .unwrap()
            .0
    }

    const PPM: f64 = 4.0;

    fn best(cursor: Vec2, s: &Sketch, ctx: &ToolContext) -> Option<Candidate> {
        infer(cursor, PPM, s, ctx).into_iter().next()
    }

    #[test]
    fn ranking_points_then_midpoints_then_intersections_then_curves() {
        let mut s = Sketch::new();
        let l = line(&mut s, Vec2::new(10.0, 10.0), Vec2::new(30.0, 10.0));
        let ctx = ToolContext::default();
        // On the endpoint.
        let c = best(Vec2::new(30.5, 10.3), &s, &ctx).unwrap();
        let end = s.point_at(Vec2::new(30.0, 10.0), 1e-9).unwrap();
        assert_eq!(c.kind, Kind::Point(end));
        assert_eq!(c.pos, Vec2::new(30.0, 10.0));
        assert_eq!(
            c.constraints,
            vec![Placed::Coincident(PointRef::Point(end))]
        );
        // Near the midpoint (1 px off): the midpoint, over the line itself.
        let c = best(Vec2::new(20.25, 10.0), &s, &ctx).unwrap();
        assert_eq!(c.kind, Kind::Midpoint(l));
        assert_eq!(c.pos, Vec2::new(20.0, 10.0));
        // Elsewhere on the line (3 px off it): on-curve, projected onto it.
        let c = best(Vec2::new(14.0, 10.75), &s, &ctx).unwrap();
        assert_eq!(c.kind, Kind::OnCurve(CurveRef::Curve(l)));
        assert!(c.pos.distance(Vec2::new(14.0, 10.0)) < 1e-12);
        // Empty space: nothing (no woken points, no anchor).
        assert!(infer(Vec2::new(20.0, 30.0), PPM, &s, &ctx).is_empty());
        // The origin.
        let c = best(Vec2::new(0.5, -0.5), &s, &ctx).unwrap();
        assert_eq!(c.kind, Kind::Origin);
        assert_eq!(c.pos, Vec2::ZERO);
    }

    #[test]
    fn lines_along_an_axis_from_it_are_horizontal_or_vertical() {
        let s = Sketch::new();
        let ctx = ToolContext {
            anchor: Some(Vec2::ZERO),
            hv_from_anchor: true,
            ..ToolContext::default()
        };
        // From the origin along X: horizontal, not "on the X axis".
        let c = best(Vec2::new(30.0, 0.3), &s, &ctx).unwrap();
        assert_eq!(c.constraints, vec![Placed::Horizontal(Guide::Anchor)]);
        assert!(c.pos.distance(Vec2::new(30.0, 0.0)) < 1e-12);
        let specs = placed_specs(&s, c.pos, &c.constraints, ctx.anchor, true);
        assert!(matches!(specs[..], [ConstraintOf::Horizontal(Orient::Line(_))]));
        // Along Y from a point on the Y axis: vertical.
        let ctx = ToolContext {
            anchor: Some(Vec2::new(0.0, -10.0)),
            ..ctx
        };
        let c = best(Vec2::new(0.2, 40.0), &s, &ctx).unwrap();
        assert_eq!(c.constraints, vec![Placed::Vertical(Guide::Anchor)]);
        // Ending on an axis from elsewhere is still "on the axis".
        let ctx = ToolContext {
            anchor: Some(Vec2::new(10.0, 10.0)),
            ..ctx
        };
        let c = best(Vec2::new(20.0, 0.3), &s, &ctx).unwrap();
        assert_eq!(c.constraints, vec![Placed::OnCurve(CurveRef::XAxis)]);
    }

    #[test]
    fn a_drop_adds_nothing_that_already_held_at_the_start() {
        let mut s = Sketch::new();
        let c = circle(&mut s, Vec2::new(0.0, 30.0), 8.0);
        let CurveKind::Circle { center, .. } = s.curves[c].kind else {
            unreachable!()
        };
        let other = s.add_point(Vec2::new(50.0, 30.0));
        // Level with `other` and straight above the origin where the drag started: dropping
        // it back there adds neither.
        let placed = [
            Placed::Horizontal(Guide::Point(PointRef::Point(other))),
            Placed::Vertical(Guide::Point(PointRef::Origin)),
        ];
        assert!(drag_constraints_from(&s, center, &placed).is_empty());
        // From somewhere else, both are new.
        s.points[center].pos = Vec2::new(10.0, 20.0);
        assert_eq!(drag_constraints_from(&s, center, &placed).len(), 2);
    }

    #[test]
    fn a_dragged_circle_center_aligned_with_the_origin_gets_vertical() {
        // S11.4: dragging a circle's center until it is straight above the origin.
        let mut s = Sketch::new();
        let c = circle(&mut s, Vec2::new(20.0, 30.0), 8.0);
        let CurveKind::Circle { center, .. } = s.curves[c].kind else {
            unreachable!()
        };
        let ctx = ToolContext {
            woken: vec![PointRef::Origin],
            exclude: vec![center],
            exclude_curves: vec![c],
            ..ToolContext::default()
        };
        // 1 px right of the Y axis (0.25 mm at 4 px/mm): snaps onto it, which for a drag reads
        // as vertical with the origin.
        let cand = drag_candidate(best(Vec2::new(0.25, 30.0), &s, &ctx).unwrap());
        assert_eq!(cand.pos, Vec2::new(0.0, 30.0));
        assert_eq!(cand.kind, Kind::Guide);
        assert_eq!(
            drag_constraints(center, &cand.constraints),
            vec![ConstraintOf::Vertical(Orient::Points(
                PointRef::Point(center),
                PointRef::Origin
            ))]
        );
        // Away from the axes (where the plane axes do not reach): vertical with a woken point.
        let other = s.add_point(Vec2::new(50.0, -40.0));
        let ctx = ToolContext {
            woken: vec![PointRef::Point(other)],
            ..ctx
        };
        let cand = best(Vec2::new(50.3, 30.0), &s, &ctx).unwrap();
        assert_eq!(cand.pos, Vec2::new(50.0, 30.0));
        assert_eq!(
            drag_constraints(center, &cand.constraints),
            vec![ConstraintOf::Vertical(Orient::Points(
                PointRef::Point(center),
                PointRef::Point(other)
            ))]
        );
        // The dragged point never snaps to itself or its own circle.
        let at_self = infer(Vec2::new(20.0, 30.0), PPM, &s, &ctx);
        assert!(at_self.iter().all(|k| k.kind != Kind::Point(center)));
        assert!(
            at_self
                .iter()
                .all(|k| k.kind != Kind::OnCurve(CurveRef::Curve(c)))
        );
    }

    #[test]
    fn intersections_including_axes() {
        let mut s = Sketch::new();
        let a = line(&mut s, Vec2::new(10.0, 10.0), Vec2::new(30.0, 30.0));
        let b = line(&mut s, Vec2::new(10.0, 34.0), Vec2::new(30.0, 12.0));
        let c = circle(&mut s, Vec2::new(30.0, 0.0), 10.0);
        let ctx = ToolContext::default();
        let p = line_line(
            Vec2::new(10.0, 10.0),
            Vec2::new(1.0, 1.0).normalize(),
            Vec2::new(10.0, 34.0),
            Vec2::new(20.0, -22.0).normalize(),
        )
        .unwrap();
        let got = best(p + Vec2::new(0.5, 0.0), &s, &ctx).unwrap();
        assert!(matches!(got.kind, Kind::Intersection(..)));
        assert!(got.pos.distance(p) < 1e-9);
        assert!(
            got.constraints
                .contains(&Placed::OnCurve(CurveRef::Curve(a)))
        );
        assert!(
            got.constraints
                .contains(&Placed::OnCurve(CurveRef::Curve(b)))
        );
        // The circle crosses the X axis at (20, 0) and (40, 0).
        let got = best(Vec2::new(20.6, 0.4), &s, &ctx).unwrap();
        assert_eq!(
            got.kind,
            Kind::Intersection(CurveRef::Curve(c), CurveRef::XAxis)
        );
        assert!(got.pos.distance(Vec2::new(20.0, 0.0)) < 1e-9);
        // The bottom of the circle is on-curve.
        let got = best(Vec2::new(30.0, -10.5), &s, &ctx).unwrap();
        assert_eq!(got.kind, Kind::OnCurve(CurveRef::Curve(c)));
    }

    #[test]
    fn horizontal_and_vertical_from_the_anchor() {
        let s = Sketch::new();
        let ctx = ToolContext {
            anchor: Some(Vec2::new(-40.0, 5.0)),
            hv_from_anchor: true,
            ..default_ctx()
        };
        // 3 px below horizontal: snaps level with the anchor.
        let c = best(Vec2::new(-10.0, 5.0 - 3.0 / PPM), &s, &ctx).unwrap();
        assert_eq!(c.kind, Kind::Guide);
        assert_eq!(c.pos, Vec2::new(-10.0, 5.0));
        assert_eq!(c.constraints, vec![Placed::Horizontal(Guide::Anchor)]);
        // 6 px off: no snap (`NOTES.md`).
        assert!(best(Vec2::new(-10.0, 5.0 + 6.0 / PPM), &s, &ctx).is_none());
        // Where the horizontal crosses the Y axis: on the axis and horizontal.
        let c = best(Vec2::new(0.3, 5.2), &s, &ctx).unwrap();
        assert_eq!(c.kind, Kind::OnCurve(CurveRef::YAxis));
        assert_eq!(c.pos, Vec2::new(0.0, 5.0));
        // Vertical.
        let c = best(Vec2::new(-40.0 + 2.0 / PPM, 30.0), &s, &ctx).unwrap();
        assert_eq!(c.constraints, vec![Placed::Vertical(Guide::Anchor)]);
        assert_eq!(c.pos, Vec2::new(-40.0, 30.0));
        // Without hv_from_anchor (rectangle, circle): nothing.
        let ctx = ToolContext {
            hv_from_anchor: false,
            ..ctx
        };
        assert!(best(Vec2::new(-10.0, 5.0), &s, &ctx).is_none());
    }

    fn default_ctx() -> ToolContext {
        ToolContext::default()
    }

    #[test]
    fn alignment_needs_a_woken_point() {
        let mut s = Sketch::new();
        line(&mut s, Vec2::new(10.0, 10.0), Vec2::new(30.0, 10.0));
        let p = s.point_at(Vec2::new(30.0, 10.0), 1e-9).unwrap();
        let cursor = Vec2::new(30.0 + 2.0 / PPM, 40.0);
        // Not woken: nothing (`NOTES.md`: no inference lines unless a point was hovered).
        assert!(best(cursor, &s, &default_ctx()).is_none());
        let mut woken = Vec::new();
        wake(&mut woken, PointRef::Point(p), 3);
        let ctx = ToolContext {
            woken,
            ..default_ctx()
        };
        let c = best(cursor, &s, &ctx).unwrap();
        assert_eq!(c.kind, Kind::Guide);
        assert_eq!(c.pos, Vec2::new(30.0, 40.0));
        assert_eq!(
            c.constraints,
            vec![Placed::Vertical(Guide::Point(PointRef::Point(p)))]
        );
        // Horizontal from the anchor and vertical through the woken point together.
        let ctx = ToolContext {
            anchor: Some(Vec2::new(0.0, 40.0)),
            hv_from_anchor: true,
            ..ctx
        };
        let c = best(Vec2::new(30.0 + 2.0 / PPM, 40.0 - 1.0 / PPM), &s, &ctx).unwrap();
        assert_eq!(c.pos, Vec2::new(30.0, 40.0));
        assert_eq!(
            c.constraints,
            vec![
                Placed::Horizontal(Guide::Anchor),
                Placed::Vertical(Guide::Point(PointRef::Point(p)))
            ]
        );
    }

    #[test]
    fn guide_crossing_a_curve_combines_both() {
        let mut s = Sketch::new();
        let l = line(&mut s, Vec2::new(1.0, 1.0), Vec2::new(31.0, 31.0));
        let ctx = ToolContext {
            anchor: Some(Vec2::new(-30.0, 10.0)),
            hv_from_anchor: true,
            ..default_ctx()
        };
        // Near (10, 10), where the horizontal from the anchor crosses the line.
        let c = best(Vec2::new(10.5, 10.3), &s, &ctx).unwrap();
        assert_eq!(c.kind, Kind::OnCurve(CurveRef::Curve(l)));
        assert!(c.pos.distance(Vec2::new(10.0, 10.0)) < 1e-9);
        assert_eq!(
            c.constraints,
            vec![
                Placed::OnCurve(CurveRef::Curve(l)),
                Placed::Horizontal(Guide::Anchor)
            ]
        );
    }

    #[test]
    fn the_anchor_is_not_a_target() {
        let mut s = Sketch::new();
        line(&mut s, Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0));
        let ctx = ToolContext {
            anchor: Some(Vec2::new(20.0, 0.0)),
            hv_from_anchor: true,
            ..default_ctx()
        };
        let all = infer(Vec2::new(20.2, 0.1), PPM, &s, &ctx);
        assert!(
            all.iter()
                .all(|c| c.pos.distance(Vec2::new(20.0, 0.0)) > 1e-6)
        );
    }

    #[test]
    fn shift_suppresses_inference() {
        let mut s = Sketch::new();
        line(&mut s, Vec2::new(10.0, 10.0), Vec2::new(30.0, 10.0));
        let ctx = ToolContext {
            suppress: true,
            anchor: Some(Vec2::ZERO),
            hv_from_anchor: true,
            woken: vec![PointRef::Origin],
            woken_curves: vec![],
            ..ToolContext::default()
        };
        for cursor in [
            Vec2::new(30.0, 10.0),
            Vec2::new(20.0, 10.0),
            Vec2::ZERO,
            Vec2::new(50.0, 0.1),
        ] {
            assert!(infer(cursor, PPM, &s, &ctx).is_empty());
        }
    }

    #[test]
    fn tolerance_is_in_pixels_at_any_zoom() {
        let mut s = Sketch::new();
        let l = line(&mut s, Vec2::new(-20.0, 0.5), Vec2::new(20.0, 0.5));
        for ppm in [0.05, 0.5, 4.0, 40.0, 400.0] {
            let at = |px_off: f64| {
                infer(Vec2::new(10.0, 0.5 + px_off / ppm), ppm, &s, &default_ctx())
                    .into_iter()
                    .next()
                    .map(|c| c.kind)
            };
            // Points far away (in px) do not interfere at high zoom; at low zoom everything
            // is within a few px, so only check the zooms where the line is long enough.
            if ppm >= 4.0 {
                assert_eq!(
                    at(4.0),
                    Some(Kind::OnCurve(CurveRef::Curve(l))),
                    "ppm {ppm}"
                );
                assert_eq!(at(6.0), None, "ppm {ppm}");
            }
        }
    }

    #[test]
    fn committed_lines_get_their_inferred_constraints() {
        let mut s = Sketch::new();
        let l = line(&mut s, Vec2::new(10.0, 10.0), Vec2::new(30.0, 10.0));
        let ctx = ToolContext::default();
        // Start on the line's midpoint, end near horizontal of the start.
        let start = best(Vec2::new(20.2, 10.1), &s, &ctx).unwrap();
        let ctx2 = ToolContext {
            anchor: Some(start.pos),
            hv_from_anchor: true,
            ..default_ctx()
        };
        let end = best(Vec2::new(45.0, 10.3), &s, &ctx2).unwrap();
        assert_eq!(end.pos, Vec2::new(45.0, 10.0));
        // (45, 10) is also on the extension of the line, which is not a curve: only the guide.
        let mut specs = placed_specs(&s, start.pos, &start.constraints, None, false);
        specs.extend(placed_specs(
            &s,
            end.pos,
            &end.constraints,
            Some(start.pos),
            true,
        ));
        SketchOp::Batch(vec![
            SketchOp::AddPolyline {
                points: vec![start.pos, end.pos],
                closed: false,
                construction: false,
                label: "Add line",
            },
            SketchOp::AddConstraints(specs),
        ])
        .apply(&mut s)
        .unwrap();
        let cs: Vec<crate::Constraint> = s.constraints.values().copied().collect();
        assert_eq!(cs.len(), 2);
        assert!(
            matches!(cs[0], ConstraintOf::Midpoint(PointRef::Point(_), CurveRef::Curve(c)) if c == l)
        );
        assert!(
            matches!(cs[1], ConstraintOf::Horizontal(Orient::Line(CurveRef::Curve(c))) if c != l)
        );
        // Snapping onto the origin records a coincidence with it.
        let o = best(Vec2::new(0.2, 0.2), &s, &ctx).unwrap();
        let specs = placed_specs(&s, o.pos, &o.constraints, None, false);
        assert_eq!(
            specs,
            vec![ConstraintOf::Coincident(
                PointSpec::At(Vec2::ZERO),
                PointSpec::Origin
            )]
        );
    }

    #[test]
    fn parallel_perpendicular_and_tangent_guides() {
        let mut s = Sketch::new();
        // A slanted line, and a chain segment ending at the anchor (10, 0).
        let slanted = line(&mut s, Vec2::new(40.0, 0.0), Vec2::new(60.0, 10.0));
        let prev = line(&mut s, Vec2::new(0.0, -10.0), Vec2::new(10.0, 0.0));
        let anchor = Vec2::new(10.0, 0.0);
        let mut ctx = ToolContext {
            anchor: Some(anchor),
            hv_from_anchor: true,
            ..default_ctx()
        };
        // Perpendicular to the previous segment: direction (-1, 1).
        let c = best(
            anchor + Vec2::new(-5.0, 5.0) + Vec2::new(0.3, 0.0),
            &s,
            &ctx,
        )
        .unwrap();
        assert_eq!(
            c.constraints,
            vec![Placed::Perpendicular(CurveRef::Curve(prev))]
        );
        assert!(c.pos.distance(anchor + Vec2::new(-4.85, 4.85)) < 1e-9);
        // Parallel to the slanted line only once it is woken.
        let along = anchor + Vec2::new(-20.0, -10.0) + Vec2::new(0.0, 0.4);
        assert!(best(along, &s, &ctx).is_none());
        wake(&mut ctx.woken_curves, slanted, 3);
        let c = best(along, &s, &ctx).unwrap();
        assert_eq!(
            c.constraints,
            vec![Placed::Parallel(CurveRef::Curve(slanted))]
        );
        let specs = placed_specs(&s, c.pos, &c.constraints, Some(anchor), true);
        assert_eq!(
            specs,
            vec![ConstraintOf::Parallel(
                CurveSpec::Between(anchor, c.pos),
                CurveSpec::Id(slanted)
            )]
        );
        // Tangent to an arc starting at the anchor.
        let mut s = Sketch::new();
        let r = 10.0;
        let st = Vec2::from_angle(0.5) * r;
        SketchOp::AddArc {
            center: Vec2::ZERO,
            start: st,
            end: Vec2::from_angle(1.5) * r,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let arc = s.curves.keys().next().unwrap();
        let ctx = ToolContext {
            anchor: Some(st),
            hv_from_anchor: true,
            ..default_ctx()
        };
        let t = Vec2::from_angle(0.5).perp();
        let c = best(st - t * 20.0 + t.perp() * 0.5, &s, &ctx).unwrap();
        assert_eq!(c.constraints, vec![Placed::Tangent(CurveRef::Curve(arc))]);
    }

    #[test]
    fn intersection_helpers() {
        assert!(circle_circle(Vec2::ZERO, 1.0, Vec2::ZERO, 2.0).is_empty());
        assert_eq!(
            circle_circle(Vec2::ZERO, 1.0, Vec2::new(2.0, 0.0), 1.0).len(),
            1
        );
        assert!(
            line_line(
                Vec2::ZERO,
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 0.0)
            )
            .is_none()
        );
        let seg = Shape::Segment(Vec2::new(0.0, -1.0), Vec2::new(0.0, 1.0));
        let far = Shape::Segment(Vec2::new(5.0, 3.0), Vec2::new(-5.0, 3.0));
        assert!(intersect(&seg, &far).is_empty());
        let arc = Shape::Arc(ArcGeom::ccw(
            Vec2::ZERO,
            Vec2::new(1.0, 0.0),
            Vec2::new(-1.0, 0.0),
        ));
        let x = Shape::Line {
            point: Vec2::new(0.0, 0.5),
            dir: Vec2::new(1.0, 0.0),
        };
        assert_eq!(intersect(&arc, &x).len(), 2);
        let below = Shape::Line {
            point: Vec2::new(0.0, -0.5),
            dir: Vec2::new(1.0, 0.0),
        };
        assert!(intersect(&arc, &below).is_empty());
    }

    fn v2() -> impl Strategy<Value = Vec2> {
        (-100.0..100.0f64, -100.0..100.0f64).prop_map(|(x, y)| Vec2::new(x, y))
    }

    proptest! {
        #[test]
        fn line_line_lies_on_both(a in v2(), b in v2(), c in v2(), d in v2()) {
            prop_assume!(a.distance(b) > 1e-3 && c.distance(d) > 1e-3);
            let (u, w) = ((b - a).normalize(), (d - c).normalize());
            prop_assume!(u.cross(w).abs() > 1e-3);
            let p = line_line(a, u, c, w).unwrap();
            let scale = 1.0 + p.length();
            prop_assert!((p - a).cross(u).abs() < 1e-9 * scale * 100.0);
            prop_assert!((p - c).cross(w).abs() < 1e-9 * scale * 100.0);
        }

        #[test]
        fn segment_crossings_are_on_both_segments(a in v2(), b in v2(), c in v2(), d in v2()) {
            prop_assume!(a.distance(b) > 1e-2 && c.distance(d) > 1e-2);
            let (s1, s2) = (Shape::Segment(a, b), Shape::Segment(c, d));
            for p in intersect(&s1, &s2) {
                prop_assert!(crate::geom::dist_point_segment(p, a, b) < 1e-6);
                prop_assert!(crate::geom::dist_point_segment(p, c, d) < 1e-6);
            }
            // Symmetric.
            prop_assert_eq!(intersect(&s1, &s2).len(), intersect(&s2, &s1).len());
        }

        #[test]
        fn line_circle_points_are_on_both(p in v2(), q in v2(), c in v2(), r in 0.1..80.0f64) {
            prop_assume!(p.distance(q) > 1e-3);
            let d = (q - p).normalize();
            let pts = line_circle(p, d, c, r);
            let dist = (c - p).cross(d).abs();
            // Two crossings iff the line passes inside the circle.
            if dist < r * (1.0 - 1e-6) { prop_assert_eq!(pts.len(), 2); }
            if dist > r * (1.0 + 1e-6) { prop_assert_eq!(pts.len(), 0); }
            for x in pts {
                prop_assert!((x.distance(c) - r).abs() < 1e-6 * (1.0 + r));
                prop_assert!((x - p).cross(d).abs() < 1e-6 * (1.0 + r + p.length()));
            }
        }

        #[test]
        fn circle_circle_points_are_on_both(c0 in v2(), c1 in v2(), r0 in 0.1..80.0f64, r1 in 0.1..80.0f64) {
            for x in circle_circle(c0, r0, c1, r1) {
                prop_assert!((x.distance(c0) - r0).abs() < 1e-6 * (1.0 + r0 + r1));
                prop_assert!((x.distance(c1) - r1).abs() < 1e-6 * (1.0 + r0 + r1));
            }
            let d = c0.distance(c1);
            if d > (r0 - r1).abs() * (1.0 + 1e-6) + 1e-9 && d < (r0 + r1) * (1.0 - 1e-6) {
                prop_assert_eq!(circle_circle(c0, r0, c1, r1).len(), 2);
            }
        }

        /// Zooming by `k` while the sketch is scaled by `1/k` shows the same picture on screen,
        /// so every decision must be the same (positions scale with the sketch).
        #[test]
        fn decisions_are_stable_in_pixel_space(
            lines in prop::collection::vec((v2(), v2()), 1..5),
            circles in prop::collection::vec((v2(), 1.0..40.0f64), 0..3),
            cursor in v2(),
            anchor in prop::option::of(v2()),
            ppm in 0.5..20.0f64,
            k in prop::sample::select(vec![0.01, 0.1, 0.5, 3.0, 25.0, 400.0]),
        ) {
            let build = |scale: f64| {
                let mut s = Sketch::new();
                for (a, b) in &lines {
                    if a.distance(*b) > 1e-3 {
                        s.add_line(*a * scale, *b * scale);
                    }
                }
                for (c, r) in &circles {
                    SketchOp::AddCircle { center: *c * scale, radius: r * scale, construction: false }
                        .apply(&mut s)
                        .unwrap();
                }
                s
            };
            let (s1, s2) = (build(1.0), build(1.0 / k));
            let ctx = |scale: f64| ToolContext {
                anchor: anchor.map(|a| a * scale),
                hv_from_anchor: true,
                woken: vec![PointRef::Origin],
                woken_curves: vec![],
                suppress: false,
                ..ToolContext::default()
            };
            let a = infer(cursor, ppm, &s1, &ctx(1.0));
            let b = infer(cursor / k, ppm * k, &s2, &ctx(1.0 / k));
            let (a, b) = (a.first(), b.first());
            prop_assert_eq!(a.is_some(), b.is_some());
            if let (Some(a), Some(b)) = (a, b) {
                // Same keys (both sketches were built the same way).
                prop_assert_eq!(a.kind, b.kind);
                prop_assert_eq!(&a.constraints, &b.constraints);
                prop_assert!((a.pos / k).distance(b.pos) < 1e-6 * (1.0 + a.pos.length()) / k);
                prop_assert!((a.distance_px - b.distance_px).abs() < 1e-6);
            }
        }
    }
}
