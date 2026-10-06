//! Geometric constraints.
//!
//! [`ConstraintOf`] is generic over how it refers to points and curves:
//! - [`Constraint`] (stored in a [`Sketch`]) refers to points and curves by ID, or to the sketch
//!   origin and the projected plane axes, which are fixed references;
//! - [`ConstraintSpec`] refers to them by position, so a tool can describe the constraints of
//!   geometry it is about to create in the same command ([`crate::SketchOp::AddConstraints`]).
//!
//! Coincidence between curve ends is **structural**: curves that meet share a point ID, so it
//! needs no record (and no solver equation). The glyphs still show it. A record is only needed
//! for a point on the origin ([`Constraint::Coincident`] with [`PointRef::Origin`]).
//!
//! Every variant is a plain equation the M6 solver can evaluate:
//! - `Coincident(a, b)`: `a − b = 0` (2 equations);
//! - `PointOnCurve(p, c)`: distance from `p` to the line or circle is 0;
//! - `Midpoint(p, l)`: `p − (l.a + l.b)/2 = 0`;
//! - `Horizontal(o)` / `Vertical(o)`: `Δy = 0` / `Δx = 0` over a line's ends or two points;
//! - `Parallel(a, b)`, `Perpendicular(a, b)`: cross / dot product of the directions is 0;
//! - `Tangent(a, b)`: a line–circle or circle–circle tangency (arcs count as circles), a
//!   line–ellipse tangency, or G1 where a Bézier curve meets another curve end to end;
//! - `Equal(a, b)`: equal line lengths, or equal radii;
//! - `Normal(l, c)`: the line passes through the circle's center (or, for an ellipse, meets
//!   it square to it at the line's nearer end);
//! - `Concentric(a, b)`: two circles or arcs share a center;
//! - `FixPoint(p)` / `FixCurve(c)`: the point (or all of the curve's points and its radius) is
//!   held where it is: the solver treats it as a constant, not an unknown;
//! - `SymmetricPoints(a, b, axis)`: `a` and `b` are mirror images in the line `axis` (their
//!   midpoint on it, their chord perpendicular to it);
//! - `SymmetricCurves(a, b, axis)`: two lines, circles or arcs are mirror images (their
//!   corresponding points are symmetric; circles also have equal radii);
//! - `EqualOffset(a, b, c, d)`: `d` is as far from `c` as `b` is from `a` (the pieces of an
//!   offset chain share its one distance).
//! - `EqualDistance(c, a, b)`: `|a − c| = |b − c|` (a polygon's corners, hidden).
//! - `Curvature(a, b)`: two curves meeting end to end, at least one a Bézier curve, join with
//!   G2 continuity: tangent there, with the same curvature (S12.14).
//!
//! The solver ([`crate::solve`]) turns these into equations.

use serde::{Deserialize, Serialize};

use crate::{CurveId, CurveKind, MERGE_EPS, PointId, Sketch, Vec2};

/// A point a stored constraint refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PointRef {
    Point(PointId),
    /// The sketch origin (fixed).
    Origin,
}

/// A curve a stored constraint refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CurveRef {
    Curve(CurveId),
    /// The sketch X axis (the projected plane axis through the origin, fixed).
    XAxis,
    /// The sketch Y axis.
    YAxis,
}

/// A point a constraint spec refers to by position.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PointSpec {
    /// The sketch point at this position.
    At(Vec2),
    Origin,
}

/// A curve a constraint spec refers to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CurveSpec {
    /// An existing curve.
    Id(CurveId),
    /// The line or arc whose ends are at these two positions (in either order).
    Between(Vec2, Vec2),
    /// The circle with this center and radius.
    Circle(Vec2, f64),
    XAxis,
    YAxis,
}

/// What a horizontal or vertical constraint applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Orient<P, C> {
    /// A line.
    Line(C),
    /// Two points (aligned horizontally or vertically).
    Points(P, P),
}

/// A geometric constraint, generic over its point and curve references (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConstraintOf<P, C> {
    Coincident(P, P),
    PointOnCurve(P, C),
    /// The point is the line's midpoint.
    Midpoint(P, C),
    Horizontal(Orient<P, C>),
    Vertical(Orient<P, C>),
    Parallel(C, C),
    Perpendicular(C, C),
    Tangent(C, C),
    /// Equal lengths (two lines) or equal radii (two circles or arcs).
    Equal(C, C),
    /// The line (first) is normal to the curve (second): through a circle's or arc's center,
    /// or square to an ellipse where the line's nearer end meets it (S12.10).
    Normal(C, C),
    /// Two circles or arcs share their center.
    Concentric(C, C),
    /// The point stays where it is.
    FixPoint(P),
    /// The curve's points (and a circle's radius) stay where they are.
    FixCurve(C),
    /// The two points mirror each other in the line (the last reference).
    SymmetricPoints(P, P, C),
    /// The two curves mirror each other in the line (the last reference).
    SymmetricCurves(C, C, C),
    /// The offset of the fourth curve from the third equals the offset of the second from the
    /// first (lines: the distance between parallel lines; circles and arcs: the difference of
    /// the radii).
    EqualOffset(C, C, C, C),
    /// The first point is midway between the other two (a center-point rectangle's center,
    /// between opposite corners). Not drawn as a glyph.
    Center(P, P, P),
    /// The second and third points are equally far from the first (a polygon's corners from
    /// its center, S7). Not drawn as a glyph.
    EqualDistance(P, P, P),
    /// The curve is a projection ("Use", S20) of something outside the sketch: it is fixed
    /// where the link puts it, and moves when the document regenerates. Deleting the
    /// constraint leaves free geometry.
    Use(C, crate::Link),
    /// The point is where a curve outside the sketch pierces the sketch plane (S12.11): fixed
    /// there, and moved when the document regenerates.
    Pierce(P, crate::Link),
    /// A text box's width is its height times the string's aspect ratio (S16). Hidden.
    TextAspect(crate::TextId),
    /// The two curves meet end to end with G2 continuity (S12.14, Final): tangent at the point
    /// they share and with equal curvature there. At least one is a Bézier curve; the other a
    /// Bézier curve, a line (curvature 0) or an arc (1/r).
    Curvature(C, C),
}

/// A constraint stored in a sketch.
pub type Constraint = ConstraintOf<PointRef, CurveRef>;
/// A constraint described by positions, resolved against the sketch when it is added.
pub type ConstraintSpec = ConstraintOf<PointSpec, CurveSpec>;

impl<P: Copy, C: Copy> Orient<P, C> {
    fn map<P2, C2>(
        self,
        fp: &mut impl FnMut(P) -> Option<P2>,
        fc: &mut impl FnMut(C) -> Option<C2>,
    ) -> Option<Orient<P2, C2>> {
        Some(match self {
            Orient::Line(c) => Orient::Line(fc(c)?),
            Orient::Points(a, b) => Orient::Points(fp(a)?, fp(b)?),
        })
    }
}

impl<P: Copy, C: Copy> ConstraintOf<P, C> {
    /// The same constraint with its references converted (`None` if one does not convert).
    pub fn map<P2, C2>(
        self,
        mut fp: impl FnMut(P) -> Option<P2>,
        mut fc: impl FnMut(C) -> Option<C2>,
    ) -> Option<ConstraintOf<P2, C2>> {
        use ConstraintOf::*;
        Some(match self {
            Coincident(a, b) => Coincident(fp(a)?, fp(b)?),
            PointOnCurve(p, c) => PointOnCurve(fp(p)?, fc(c)?),
            Midpoint(p, c) => Midpoint(fp(p)?, fc(c)?),
            Horizontal(o) => Horizontal(o.map(&mut fp, &mut fc)?),
            Vertical(o) => Vertical(o.map(&mut fp, &mut fc)?),
            Parallel(a, b) => Parallel(fc(a)?, fc(b)?),
            Perpendicular(a, b) => Perpendicular(fc(a)?, fc(b)?),
            Tangent(a, b) => Tangent(fc(a)?, fc(b)?),
            Equal(a, b) => Equal(fc(a)?, fc(b)?),
            Normal(a, b) => Normal(fc(a)?, fc(b)?),
            Concentric(a, b) => Concentric(fc(a)?, fc(b)?),
            FixPoint(p) => FixPoint(fp(p)?),
            FixCurve(c) => FixCurve(fc(c)?),
            SymmetricPoints(a, b, l) => SymmetricPoints(fp(a)?, fp(b)?, fc(l)?),
            SymmetricCurves(a, b, l) => SymmetricCurves(fc(a)?, fc(b)?, fc(l)?),
            EqualOffset(a, b, c, d) => EqualOffset(fc(a)?, fc(b)?, fc(c)?, fc(d)?),
            Center(p, a, b) => Center(fp(p)?, fp(a)?, fp(b)?),
            EqualDistance(p, a, b) => EqualDistance(fp(p)?, fp(a)?, fp(b)?),
            Use(c, l) => Use(fc(c)?, l),
            Pierce(p, l) => Pierce(fp(p)?, l),
            TextAspect(t) => TextAspect(t),
            Curvature(a, b) => Curvature(fc(a)?, fc(b)?),
        })
    }

    /// The points the constraint refers to.
    pub fn points(&self) -> Vec<P> {
        use ConstraintOf::*;
        match *self {
            Coincident(a, b)
            | Horizontal(Orient::Points(a, b))
            | Vertical(Orient::Points(a, b)) => {
                vec![a, b]
            }
            PointOnCurve(p, _) | Midpoint(p, _) | FixPoint(p) | Pierce(p, _) => vec![p],
            SymmetricPoints(a, b, _) => vec![a, b],
            Center(p, a, b) | EqualDistance(p, a, b) => vec![p, a, b],
            _ => vec![],
        }
    }

    /// The curves the constraint refers to.
    pub fn curves(&self) -> Vec<C> {
        use ConstraintOf::*;
        match *self {
            PointOnCurve(_, c) | Midpoint(_, c) => vec![c],
            Horizontal(Orient::Line(c)) | Vertical(Orient::Line(c)) => vec![c],
            Parallel(a, b)
            | Perpendicular(a, b)
            | Tangent(a, b)
            | Equal(a, b)
            | Normal(a, b)
            | Concentric(a, b)
            | Curvature(a, b) => vec![a, b],
            FixCurve(c) | Use(c, _) => vec![c],
            SymmetricPoints(_, _, l) => vec![l],
            SymmetricCurves(a, b, l) => vec![a, b, l],
            EqualOffset(a, b, c, d) => vec![a, b, c, d],
            _ => vec![],
        }
    }
}

impl Constraint {
    /// True if the constraint refers to this point.
    pub fn uses_point(&self, p: PointId) -> bool {
        self.points().contains(&PointRef::Point(p))
    }

    /// True if the constraint refers to this curve.
    pub fn uses_curve(&self, c: CurveId) -> bool {
        self.curves().contains(&CurveRef::Curve(c))
    }

    /// The same constraint with its two references swapped, for the symmetric ones.
    pub fn swapped(&self) -> Option<Constraint> {
        use ConstraintOf::*;
        Some(match *self {
            Coincident(a, b) => Coincident(b, a),
            Horizontal(Orient::Points(a, b)) => Horizontal(Orient::Points(b, a)),
            Vertical(Orient::Points(a, b)) => Vertical(Orient::Points(b, a)),
            Parallel(a, b) => Parallel(b, a),
            Perpendicular(a, b) => Perpendicular(b, a),
            Tangent(a, b) => Tangent(b, a),
            Equal(a, b) => Equal(b, a),
            Concentric(a, b) => Concentric(b, a),
            Curvature(a, b) => Curvature(b, a),
            SymmetricPoints(a, b, l) => SymmetricPoints(b, a, l),
            SymmetricCurves(a, b, l) => SymmetricCurves(b, a, l),
            _ => return None,
        })
    }

    /// True if both say the same thing (the same constraint, maybe with its references in the
    /// other order).
    pub fn same_as(&self, other: &Constraint) -> bool {
        self == other || self.swapped().as_ref() == Some(other)
    }

    /// True if it holds by construction or says nothing (a point coincident with itself).
    pub fn is_trivial(&self) -> bool {
        match *self {
            ConstraintOf::Coincident(a, b) => a == b,
            ConstraintOf::Horizontal(Orient::Points(a, b))
            | ConstraintOf::Vertical(Orient::Points(a, b)) => a == b,
            ConstraintOf::Parallel(a, b)
            | ConstraintOf::Perpendicular(a, b)
            | ConstraintOf::Tangent(a, b)
            | ConstraintOf::Equal(a, b)
            | ConstraintOf::Normal(a, b)
            | ConstraintOf::Concentric(a, b)
            | ConstraintOf::Curvature(a, b) => a == b,
            ConstraintOf::SymmetricCurves(a, b, l) => a == b || a == l || b == l,
            ConstraintOf::EqualOffset(a, b, c, d) => (a, b) == (c, d),
            ConstraintOf::Center(_, a, b) | ConstraintOf::EqualDistance(_, a, b) => a == b,
            // The origin and the axes are fixed already.
            ConstraintOf::FixPoint(PointRef::Origin) => true,
            ConstraintOf::FixCurve(c) => !matches!(c, CurveRef::Curve(_)),
            _ => false,
        }
    }
}

impl ConstraintSpec {
    /// Resolves the positions against the sketch (`None` if something is not there).
    pub fn resolve(&self, s: &Sketch) -> Option<Constraint> {
        self.map(|p| resolve_point(s, p), |c| resolve_curve(s, c))
    }
}

fn resolve_point(s: &Sketch, p: PointSpec) -> Option<PointRef> {
    match p {
        PointSpec::At(pos) => s.point_at(pos, MERGE_EPS).map(PointRef::Point),
        PointSpec::Origin => Some(PointRef::Origin),
    }
}

fn resolve_curve(s: &Sketch, c: CurveSpec) -> Option<CurveRef> {
    let near = |id: PointId, p: Vec2| s.pos(id).distance(p) <= MERGE_EPS;
    match c {
        CurveSpec::Id(id) => s.curves.contains_key(id).then_some(CurveRef::Curve(id)),
        CurveSpec::XAxis => Some(CurveRef::XAxis),
        CurveSpec::YAxis => Some(CurveRef::YAxis),
        CurveSpec::Between(a, b) => s
            .curves
            .keys()
            .find(|k| {
                s.curve_ends(*k)
                    .is_some_and(|(p, q)| (near(p, a) && near(q, b)) || (near(p, b) && near(q, a)))
            })
            .map(CurveRef::Curve),
        CurveSpec::Circle(center, radius) => s
            .curves
            .iter()
            .find(|(_, cv)| {
                matches!(cv.kind, CurveKind::Circle { center: c, radius: r }
                    if near(c, center) && (r - radius).abs() <= MERGE_EPS)
            })
            .map(|(k, _)| CurveRef::Curve(k)),
    }
}

/// The constraints a rectangle's four sides get, as Onshape creates them (`screens/12a`):
/// the side opposite the first corner is horizontal and perpendicular to the next side, and
/// opposite sides are parallel. With the shared corners this fixes all four directions (the
/// same four degrees of freedom as two horizontals and two verticals), and draws the same
/// glyphs as Onshape: "⊥ —" on that side and "∥" on two others.
///
/// `corners` are in [`rect_corners`](crate) order: the first corner, then along X, the
/// opposite corner, then along Y.
pub fn rectangle_constraints(corners: [Vec2; 4]) -> Vec<ConstraintSpec> {
    let side = |i: usize| CurveSpec::Between(corners[i], corners[(i + 1) % 4]);
    // Sides: 0 = first corner → along X, 1, 2 = opposite corner → along Y, 3 = back to first.
    vec![
        ConstraintOf::Perpendicular(side(2), side(3)),
        ConstraintOf::Horizontal(Orient::Line(side(2))),
        ConstraintOf::Parallel(side(3), side(1)),
        ConstraintOf::Parallel(side(0), side(2)),
    ]
}

/// The constraint tools (`reference/onshape/constraints.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstraintKind {
    Coincident,
    Concentric,
    Parallel,
    Tangent,
    Horizontal,
    Vertical,
    Perpendicular,
    Equal,
    Midpoint,
    Normal,
    Symmetric,
    Fix,
    /// S12.11: a point and an edge outside the sketch that crosses its plane (the app picks
    /// the edge; see [`ConstraintOf::Pierce`]).
    Pierce,
    /// S12.14: two curves meeting end to end, at least one a Bézier curve (G2).
    Curvature,
}

impl ConstraintKind {
    /// In the order of Onshape's Constraints menu.
    pub const ALL: [ConstraintKind; 12] = [
        ConstraintKind::Coincident,
        ConstraintKind::Concentric,
        ConstraintKind::Parallel,
        ConstraintKind::Tangent,
        ConstraintKind::Horizontal,
        ConstraintKind::Vertical,
        ConstraintKind::Perpendicular,
        ConstraintKind::Equal,
        ConstraintKind::Midpoint,
        ConstraintKind::Normal,
        ConstraintKind::Symmetric,
        ConstraintKind::Fix,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ConstraintKind::Coincident => "Coincident",
            ConstraintKind::Concentric => "Concentric",
            ConstraintKind::Parallel => "Parallel",
            ConstraintKind::Tangent => "Tangent",
            ConstraintKind::Horizontal => "Horizontal",
            ConstraintKind::Vertical => "Vertical",
            ConstraintKind::Perpendicular => "Perpendicular",
            ConstraintKind::Equal => "Equal",
            ConstraintKind::Midpoint => "Midpoint",
            ConstraintKind::Normal => "Normal",
            ConstraintKind::Symmetric => "Symmetric",
            ConstraintKind::Fix => "Fix",
            ConstraintKind::Pierce => "Pierce",
            ConstraintKind::Curvature => "Curvature",
        }
    }

    /// The keyboard shortcut, as shown in the menu.
    pub fn shortcut(self) -> &'static str {
        match self {
            ConstraintKind::Coincident => "I",
            ConstraintKind::Concentric => "Shift+O",
            ConstraintKind::Parallel => "B",
            ConstraintKind::Tangent => "T",
            ConstraintKind::Horizontal => "H",
            ConstraintKind::Vertical => "V",
            ConstraintKind::Perpendicular => "Shift+L",
            ConstraintKind::Equal => "E",
            ConstraintKind::Midpoint => "Shift+M",
            ConstraintKind::Normal => "Shift+K",
            ConstraintKind::Symmetric => "Shift+Q",
            ConstraintKind::Fix => "Shift+J",
            ConstraintKind::Pierce => "Shift+G",
            ConstraintKind::Curvature => "Shift+U",
        }
    }

    /// The undo label ("Add horizontal").
    pub fn undo_label(self) -> &'static str {
        match self {
            ConstraintKind::Coincident => "Add coincident",
            ConstraintKind::Concentric => "Add concentric",
            ConstraintKind::Parallel => "Add parallel",
            ConstraintKind::Tangent => "Add tangent",
            ConstraintKind::Horizontal => "Add horizontal",
            ConstraintKind::Vertical => "Add vertical",
            ConstraintKind::Perpendicular => "Add perpendicular",
            ConstraintKind::Equal => "Add equal",
            ConstraintKind::Midpoint => "Add midpoint",
            ConstraintKind::Normal => "Add normal",
            ConstraintKind::Symmetric => "Add symmetric",
            ConstraintKind::Fix => "Add fix",
            ConstraintKind::Pierce => "Add pierce",
            ConstraintKind::Curvature => "Add curvature",
        }
    }
}

/// Whether a selection fits a constraint tool.
#[derive(Debug, Clone, PartialEq)]
pub enum Fit {
    /// The constraints to add.
    Complete(Vec<Constraint>),
    /// Not enough yet (the tool waits for more picks).
    Partial,
    /// The selection does not fit this constraint.
    Invalid,
}

/// A selected entity as a constraint tool sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Pick {
    Point(PointRef),
    Line(CurveRef),
    Round(CurveRef),
    /// An ellipse: only a point on it (Coincident) and Fix apply.
    Ellipse(CurveRef),
    /// A Bézier curve: Tangent and Curvature where it meets another curve end to end,
    /// Symmetric and Fix.
    Bezier(CurveRef),
}

/// True if the two curves meet end to end (share an end point).
fn share_end(s: &Sketch, a: CurveRef, b: CurveRef) -> bool {
    let (CurveRef::Curve(a), CurveRef::Curve(b)) = (a, b) else { return false };
    match (s.curve_ends(a), s.curve_ends(b)) {
        (Some((a0, a1)), Some((b0, b1))) => a != b && [a0, a1].iter().any(|p| *p == b0 || *p == b1),
        _ => false,
    }
}

fn classify(s: &Sketch, e: crate::SketchEntity) -> Option<Pick> {
    use crate::SketchEntity as E;
    Some(match e {
        E::Point(p) => Pick::Point(PointRef::Point(p)),
        E::Origin => Pick::Point(PointRef::Origin),
        E::Curve(c) => match s.curves.get(c)?.kind {
            CurveKind::Line { .. } => Pick::Line(CurveRef::Curve(c)),
            // An elliptical arc constrains as its ellipse.
            CurveKind::Ellipse { .. } | CurveKind::EllipseArc { .. } => Pick::Ellipse(CurveRef::Curve(c)),
            CurveKind::Bezier { .. } => Pick::Bezier(CurveRef::Curve(c)),
            _ => Pick::Round(CurveRef::Curve(c)),
        },
        E::Dimension(_) | E::Constraint(_) | E::Text(_) => return None,
    })
}

/// The constraints a tool adds for a selection (in pick order), following Onshape: two points
/// coincide; a point and a curve make a point on the curve; two lines made coincident become
/// collinear; Horizontal and Vertical take lines or two points; Parallel and Equal take two or
/// more (the first against each other); Midpoint takes a point and a line; Concentric two
/// circles or arcs (or a point and one: the point goes to the center); Symmetric takes the axis
/// line first, then two points or two curves of the same type; Normal a line and a circle, arc
/// or ellipse; Fix takes anything.
pub fn fit(kind: ConstraintKind, s: &Sketch, sel: &[crate::SketchEntity]) -> Fit {
    use ConstraintOf as C;
    let picks: Option<Vec<Pick>> = sel.iter().map(|e| classify(s, *e)).collect();
    let Some(picks) = picks else {
        return Fit::Invalid;
    };
    if picks.is_empty() {
        return Fit::Partial;
    }
    let center = |c: CurveRef| -> Option<PointRef> {
        let CurveRef::Curve(k) = c else { return None };
        match s.curves.get(k)?.kind {
            CurveKind::Circle { center, .. } | CurveKind::Arc { center, .. } => {
                Some(PointRef::Point(center))
            }
            _ => None,
        }
    };
    let first = picks[0];
    let rest = &picks[1..];
    let mut out: Vec<Constraint> = Vec::new();
    let pairwise = |f: &dyn Fn(Pick, Pick) -> Option<Vec<Constraint>>| -> Fit {
        if rest.is_empty() {
            return Fit::Partial;
        }
        let mut v = Vec::new();
        for r in rest {
            match f(first, *r) {
                Some(c) => v.extend(c),
                None => return Fit::Invalid,
            }
        }
        Fit::Complete(v)
    };
    let result = match kind {
        ConstraintKind::Coincident => pairwise(&|a, b| {
            Some(match (a, b) {
                (Pick::Point(p), Pick::Point(q)) => vec![C::Coincident(p, q)],
                (Pick::Point(p), Pick::Line(c) | Pick::Round(c) | Pick::Ellipse(c))
                | (Pick::Line(c) | Pick::Round(c) | Pick::Ellipse(c), Pick::Point(p)) => {
                    vec![C::PointOnCurve(p, c)]
                }
                (Pick::Line(a), Pick::Line(CurveRef::Curve(b))) => {
                    let (p, q) = s.curve_ends(b)?;
                    vec![
                        C::PointOnCurve(PointRef::Point(p), a),
                        C::PointOnCurve(PointRef::Point(q), a),
                    ]
                }
                (Pick::Round(a), Pick::Round(b)) => vec![C::Concentric(a, b), C::Equal(a, b)],
                _ => return None,
            })
        }),
        ConstraintKind::Horizontal | ConstraintKind::Vertical => {
            let make = |o: Orient<PointRef, CurveRef>| {
                if kind == ConstraintKind::Horizontal {
                    C::Horizontal(o)
                } else {
                    C::Vertical(o)
                }
            };
            if picks.iter().all(|p| matches!(p, Pick::Line(_))) {
                for p in &picks {
                    if let Pick::Line(c) = p {
                        out.push(make(Orient::Line(*c)));
                    }
                }
                Fit::Complete(out)
            } else if let Pick::Point(a) = first
                && rest.iter().all(|p| matches!(p, Pick::Point(_)))
            {
                if rest.is_empty() {
                    Fit::Partial
                } else {
                    for p in rest {
                        if let Pick::Point(b) = p {
                            out.push(make(Orient::Points(a, *b)));
                        }
                    }
                    Fit::Complete(out)
                }
            } else {
                Fit::Invalid
            }
        }
        ConstraintKind::Parallel => pairwise(&|a, b| match (a, b) {
            (Pick::Line(a), Pick::Line(b)) => Some(vec![C::Parallel(a, b)]),
            _ => None,
        }),
        ConstraintKind::Perpendicular if picks.len() <= 2 => pairwise(&|a, b| match (a, b) {
            (Pick::Line(a), Pick::Line(b)) => Some(vec![C::Perpendicular(a, b)]),
            _ => None,
        }),
        ConstraintKind::Tangent => pairwise(&|a, b| match (a, b) {
            (Pick::Line(a), Pick::Round(b))
            | (Pick::Round(a), Pick::Line(b))
            | (Pick::Round(a), Pick::Round(b)) => Some(vec![C::Tangent(a, b)]),
            // A line tangent to an ellipse (Final re-audit, S8).
            (Pick::Line(a), Pick::Ellipse(b)) | (Pick::Ellipse(b), Pick::Line(a)) => Some(vec![C::Tangent(a, b)]),
            // A Bézier curve is tangent where it meets the other curve end to end.
            (Pick::Bezier(a), Pick::Line(b) | Pick::Round(b) | Pick::Bezier(b))
            | (Pick::Line(a) | Pick::Round(a), Pick::Bezier(b))
                if share_end(s, a, b) =>
            {
                Some(vec![C::Tangent(a, b)])
            }
            _ => None,
        }),
        // S12.14: G2 where two curves meet end to end, one of them a Bézier curve.
        ConstraintKind::Curvature if picks.len() <= 2 => pairwise(&|a, b| match (a, b) {
            (Pick::Bezier(a), Pick::Line(b) | Pick::Round(b) | Pick::Bezier(b))
            | (Pick::Line(a) | Pick::Round(a), Pick::Bezier(b))
                if share_end(s, a, b) =>
            {
                Some(vec![C::Curvature(a, b)])
            }
            _ => None,
        }),
        ConstraintKind::Equal => pairwise(&|a, b| match (a, b) {
            (Pick::Line(a), Pick::Line(b)) | (Pick::Round(a), Pick::Round(b)) => {
                Some(vec![C::Equal(a, b)])
            }
            _ => None,
        }),
        ConstraintKind::Midpoint if picks.len() <= 2 => pairwise(&|a, b| match (a, b) {
            (Pick::Point(p), Pick::Line(l)) | (Pick::Line(l), Pick::Point(p)) => {
                Some(vec![C::Midpoint(p, l)])
            }
            _ => None,
        }),
        // S12.10: a line and a circle, arc or ellipse, the line first.
        ConstraintKind::Normal if picks.len() <= 2 => pairwise(&|a, b| match (a, b) {
            (Pick::Line(l), Pick::Round(c) | Pick::Ellipse(c))
            | (Pick::Round(c) | Pick::Ellipse(c), Pick::Line(l)) => {
                Some(vec![C::Normal(l, c)])
            }
            _ => None,
        }),
        ConstraintKind::Concentric => pairwise(&|a, b| match (a, b) {
            (Pick::Round(a), Pick::Round(b)) => Some(vec![C::Concentric(a, b)]),
            (Pick::Point(p), Pick::Round(c)) | (Pick::Round(c), Pick::Point(p)) => {
                Some(vec![C::Coincident(p, center(c)?)])
            }
            _ => None,
        }),
        ConstraintKind::Symmetric => match picks.as_slice() {
            [Pick::Line(_)] | [Pick::Line(_), _] => Fit::Partial,
            [Pick::Line(axis), a, b] => {
                let kind_of = |c: CurveRef| match c {
                    CurveRef::Curve(id) => s.curves.get(id).map(|c| std::mem::discriminant(&c.kind)),
                    _ => None,
                };
                match (*a, *b) {
                    (Pick::Point(p), Pick::Point(q)) if p != q => {
                        Fit::Complete(vec![C::SymmetricPoints(p, q, *axis)])
                    }
                    (
                        Pick::Line(x) | Pick::Round(x) | Pick::Bezier(x),
                        Pick::Line(y) | Pick::Round(y) | Pick::Bezier(y),
                    )
                        if x != y
                            && x != *axis
                            && y != *axis
                            && kind_of(x).is_some()
                            && kind_of(x) == kind_of(y) =>
                    {
                        Fit::Complete(vec![C::SymmetricCurves(x, y, *axis)])
                    }
                    _ => Fit::Invalid,
                }
            }
            _ => Fit::Invalid,
        },
        ConstraintKind::Fix => {
            for p in &picks {
                out.push(match *p {
                    Pick::Point(p) => C::FixPoint(p),
                    Pick::Line(c) | Pick::Round(c) | Pick::Ellipse(c) | Pick::Bezier(c) => C::FixCurve(c),
                });
            }
            Fit::Complete(out)
        }
        ConstraintKind::Perpendicular
        | ConstraintKind::Midpoint
        | ConstraintKind::Normal
        | ConstraintKind::Curvature => Fit::Invalid,
        // A point waits for its edge, which is outside the sketch (the app completes it).
        ConstraintKind::Pierce => match picks.as_slice() {
            [Pick::Point(PointRef::Point(_))] => Fit::Partial,
            _ => Fit::Invalid,
        },
    };
    // Drop constraints that say nothing (the origin fixed) or are there already.
    match result {
        Fit::Complete(v) => {
            let v: Vec<Constraint> = v
                .into_iter()
                .filter(|c| !c.is_trivial() && !s.constraints.values().any(|x| x.same_as(c)))
                .collect();
            if v.is_empty() {
                Fit::Invalid
            } else {
                Fit::Complete(v)
            }
        }
        f => f,
    }
}

impl Sketch {
    /// Adds a constraint unless it is trivial or already there. Returns true if added.
    pub fn add_constraint(&mut self, c: Constraint) -> bool {
        if c.is_trivial() || self.constraints.values().any(|x| x.same_as(&c)) {
            return false;
        }
        self.constraints.insert(c);
        true
    }

    /// Adds a constraint a composite entity generates between its own pieces, without a glyph
    /// (see [`Sketch::quiet`]). Returns true if added.
    pub fn add_quiet_constraint(&mut self, c: Constraint) -> bool {
        if c.is_trivial() || self.constraints.values().any(|x| x.same_as(&c)) {
            return false;
        }
        let k = self.constraints.insert(c);
        self.quiet.constraints.insert(k);
        true
    }

    /// Points shared by two or more curves: coincident by construction (drawn with a
    /// coincident glyph).
    pub fn shared_points(&self) -> Vec<PointId> {
        let mut uses: std::collections::HashMap<PointId, usize> = std::collections::HashMap::new();
        for c in self.curves.keys() {
            for p in self.curve_points(c) {
                *uses.entry(p).or_default() += 1;
            }
        }
        self.points
            .keys()
            .filter(|p| uses.get(p).is_some_and(|n| *n >= 2))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SketchOp;

    fn rect(s: &mut Sketch, corners: [Vec2; 4]) {
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
    }

    #[test]
    fn rectangle_gets_onshape_constraints() {
        let mut s = Sketch::new();
        let c = [
            Vec2::ZERO,
            Vec2::new(50.0, 0.0),
            Vec2::new(50.0, 30.0),
            Vec2::new(0.0, 30.0),
        ];
        rect(&mut s, c);
        assert_eq!(s.constraints.len(), 4);
        let line = |a: Vec2, b: Vec2| resolve_curve(&s, CurveSpec::Between(a, b)).unwrap();
        let (bottom, right, top, left) = (
            line(c[0], c[1]),
            line(c[1], c[2]),
            line(c[2], c[3]),
            line(c[3], c[0]),
        );
        let all: Vec<Constraint> = s.constraints.values().copied().collect();
        assert_eq!(
            all,
            vec![
                ConstraintOf::Perpendicular(top, left),
                ConstraintOf::Horizontal(Orient::Line(top)),
                ConstraintOf::Parallel(left, right),
                ConstraintOf::Parallel(bottom, top),
            ]
        );
        // Four shared corners: structural coincidence.
        assert_eq!(s.shared_points().len(), 4);
    }

    #[test]
    fn deleting_geometry_drops_its_constraints() {
        let mut s = Sketch::new();
        let c = [
            Vec2::ZERO,
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 5.0),
            Vec2::new(0.0, 5.0),
        ];
        rect(&mut s, c);
        let top = match resolve_curve(&s, CurveSpec::Between(c[2], c[3])).unwrap() {
            CurveRef::Curve(k) => k,
            _ => unreachable!(),
        };
        s.remove_curve(top);
        // Only Parallel(left, right) is left.
        assert_eq!(s.constraints.len(), 1);
        let p = s.point_at(c[1], 1e-9).unwrap();
        s.remove_point(p);
        assert!(s.constraints.is_empty());
    }

    #[test]
    fn specs_resolve_by_position_and_skip_missing_geometry() {
        let mut s = Sketch::new();
        s.add_line(Vec2::ZERO, Vec2::new(10.0, 0.0));
        let spec = ConstraintOf::Coincident(PointSpec::At(Vec2::ZERO), PointSpec::Origin);
        let c = spec.resolve(&s).unwrap();
        assert!(matches!(
            c,
            ConstraintOf::Coincident(PointRef::Point(_), PointRef::Origin)
        ));
        // No point there.
        let missing =
            ConstraintOf::Coincident(PointSpec::At(Vec2::new(3.0, 3.0)), PointSpec::Origin);
        assert!(missing.resolve(&s).is_none());
        // Lines match in either direction.
        let h: ConstraintSpec = ConstraintOf::Horizontal(Orient::Line(CurveSpec::Between(
            Vec2::new(10.0, 0.0),
            Vec2::ZERO,
        )));
        assert!(h.resolve(&s).is_some());
        // Duplicates and trivial constraints are not added twice.
        assert!(s.add_constraint(h.resolve(&s).unwrap()));
        assert!(!s.add_constraint(h.resolve(&s).unwrap()));
        assert!(!s.add_constraint(ConstraintOf::Coincident(PointRef::Origin, PointRef::Origin)));
    }
}
