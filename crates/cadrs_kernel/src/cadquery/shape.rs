//! CadQuery's shapes (`cadquery/occ_impl/shapes.py`): one [`Shape`] type for vertices, edges,
//! wires, faces, shells, solids and compounds, with its kind in [`Shape::shape_type`].
//!
//! Faces, shells, solids and compounds are bodies of the module's session ([`super::session`]);
//! a face, shell or solid picked out of a body is a [`Part`] of it (cheap to list and select
//! among), copied out into a body of its own the first time an operation needs one. Edges,
//! wires and vertices are exact curves and points (their own [`EdgeGeom`]s), remembering the
//! body edge they were taken from so that fillets and chamfers find it again.

use super::geom::{BoundBox, Location};
use super::{BodyRef, CqError, CqResult, brep, insert, session, value_err};
use crate::Kernel;
use crate::native::curve::Curve;
use crate::native::math::{Affine, P3, V3};
use crate::native::surface::Surface;
use crate::native::topo::Body;
use std::sync::{Arc, OnceLock};

/// A shape's topological kind (`ShapeType()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ShapeType {
    /// A point.
    Vertex,
    /// A bounded curve.
    Edge,
    /// Edges joined end to end.
    Wire,
    /// A bounded surface.
    Face,
    /// Faces joined at their edges.
    Shell,
    /// A volume bounded by shells.
    Solid,
    /// Solids sharing faces (never made here).
    CompSolid,
    /// Any collection of shapes.
    Compound,
}

impl ShapeType {
    /// CadQuery's name of it (`'Solid'`, …).
    pub fn name(self) -> &'static str {
        match self {
            ShapeType::Vertex => "Vertex",
            ShapeType::Edge => "Edge",
            ShapeType::Wire => "Wire",
            ShapeType::Face => "Face",
            ShapeType::Shell => "Shell",
            ShapeType::Solid => "Solid",
            ShapeType::CompSolid => "CompSolid",
            ShapeType::Compound => "Compound",
        }
    }

    /// The kind of a name (as the selectors and `ShapeType()` spell them).
    pub fn of_name(s: &str) -> Option<Self> {
        Some(match s {
            "Vertex" => ShapeType::Vertex,
            "Edge" => ShapeType::Edge,
            "Wire" => ShapeType::Wire,
            "Face" => ShapeType::Face,
            "Shell" => ShapeType::Shell,
            "Solid" => ShapeType::Solid,
            "CompSolid" => ShapeType::CompSolid,
            "Compound" => ShapeType::Compound,
            _ => return None,
        })
    }
}

/// A curve's or surface's geometric kind (`geomType()`), as CadQuery's strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeomType {
    /// "LINE"
    Line,
    /// "CIRCLE"
    Circle,
    /// "ELLIPSE"
    Ellipse,
    /// "BSPLINE"
    BSpline,
    /// "PLANE"
    Plane,
    /// "CYLINDER"
    Cylinder,
    /// "CONE"
    Cone,
    /// "SPHERE"
    Sphere,
    /// "TORUS"
    Torus,
    /// "REVOLUTION"
    Revolution,
    /// "EXTRUSION"
    Extrusion,
    /// The kind's own name: "Vertex", "Wire", "Shell", "CompSolid" (OCCT's for a solid) or
    /// "Compound".
    Topo(ShapeType),
}

impl GeomType {
    /// CadQuery's string.
    pub fn name(self) -> &'static str {
        match self {
            GeomType::Line => "LINE",
            GeomType::Circle => "CIRCLE",
            GeomType::Ellipse => "ELLIPSE",
            GeomType::BSpline => "BSPLINE",
            GeomType::Plane => "PLANE",
            GeomType::Cylinder => "CYLINDER",
            GeomType::Cone => "CONE",
            GeomType::Sphere => "SPHERE",
            GeomType::Torus => "TORUS",
            GeomType::Revolution => "REVOLUTION",
            GeomType::Extrusion => "EXTRUSION",
            // (OCCT's table maps a solid to "CompSolid": its key appears twice.)
            GeomType::Topo(ShapeType::Solid) => "CompSolid",
            GeomType::Topo(t) => t.name(),
        }
    }
}

/// An edge's geometry: `curve` run from `t0` to `t1` (`t0 < t1`).
#[derive(Debug, Clone)]
pub struct EdgeGeom {
    /// The curve.
    pub curve: Curve,
    /// Where the edge starts on it.
    pub t0: f64,
    /// Where it ends.
    pub t1: f64,
}

/// Gauss–Legendre nodes and weights on [-1, 1] (8 points).
const GAUSS8: [(f64, f64); 8] = [
    (-0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
    (-0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (-0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (-0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
];

impl EdgeGeom {
    /// The segment from `a` to `b` (an error when they meet, as OCCT's).
    pub fn line(a: V3, b: V3) -> CqResult<Self> {
        // (OCCT's `Precision::Confusion`: ends nearer are one point.)
        if (b - a).norm() <= 1e-7 {
            return value_err!("BRep_API: command not done (a line between coincident points)");
        }
        let (c, l) = Curve::line(P3::from(a), P3::from(b));
        Ok(Self {
            curve: c,
            t0: 0.0,
            t1: l,
        })
    }

    /// The point at parameter `t`.
    pub fn point(&self, t: f64) -> V3 {
        self.curve.point(t).coords
    }

    /// The start point.
    pub fn start(&self) -> V3 {
        self.point(self.t0)
    }

    /// The end point.
    pub fn end(&self) -> V3 {
        self.point(self.t1)
    }

    /// The unit tangent at parameter `t` (zero where the curve stops).
    pub fn tangent(&self, t: f64) -> V3 {
        let d = self.curve.tangent(t);
        let n = d.norm();
        if n > 0.0 { d / n } else { d }
    }

    /// Its length.
    pub fn length(&self) -> f64 {
        match &self.curve {
            Curve::Line { d, .. } => d.norm() * (self.t1 - self.t0),
            Curve::Circle { r, .. } => r * (self.t1 - self.t0),
            _ => self.curve.length(self.t0, self.t1),
        }
    }

    /// The same edge run the other way.
    pub fn reversed(&self) -> Self {
        let (c, a, b) = self.curve.reversed(self.t0, self.t1);
        Self {
            curve: c,
            t0: a.min(b),
            t1: a.max(b),
        }
    }

    /// Moved by an affine map.
    pub fn transformed(&self, m: &Affine) -> Self {
        Self {
            curve: self.curve.transformed(m),
            t0: self.t0,
            t1: self.t1,
        }
    }

    /// Whether it ends where it starts.
    pub fn is_closed(&self) -> bool {
        (self.start() - self.end()).norm() <= 1e-7 * (1.0 + self.start().norm())
    }

    /// Spans for quadrature: one per B-spline knot span, several for a closed conic.
    fn spans(&self) -> Vec<(f64, f64)> {
        let breaks: Vec<f64> = match &self.curve {
            Curve::Nurbs(n) => {
                let mut b: Vec<f64> = n
                    .breaks()
                    .into_iter()
                    .filter(|&u| u > self.t0 && u < self.t1)
                    .collect();
                b.insert(0, self.t0);
                b.push(self.t1);
                b
            }
            Curve::Line { .. } => vec![self.t0, self.t1],
            _ => {
                let n = (((self.t1 - self.t0) / 0.5).ceil() as usize).max(1);
                (0..=n)
                    .map(|i| self.t0 + (self.t1 - self.t0) * i as f64 / n as f64)
                    .collect()
            }
        };
        let mut out = Vec::new();
        for w in breaks.windows(2) {
            if w[1] > w[0] {
                // Halve each span a few times: B-spline spans can be long.
                let k = if matches!(self.curve, Curve::Nurbs(_)) {
                    4
                } else {
                    1
                };
                for j in 0..k {
                    let a = w[0] + (w[1] - w[0]) * j as f64 / k as f64;
                    let b = w[0] + (w[1] - w[0]) * (j + 1) as f64 / k as f64;
                    out.push((a, b));
                }
            }
        }
        out
    }

    /// ∫ over the edge of (point, 1) by arc length: the moments for the centroid.
    pub(crate) fn moments(&self) -> (V3, f64) {
        match &self.curve {
            Curve::Line { .. } => {
                let l = self.length();
                ((self.start() + self.end()) * 0.5 * l, l)
            }
            Curve::Circle { c, x, y, r } => {
                let (a, b) = (self.t0, self.t1);
                let s = c.coords * (r * (b - a))
                    + (x * (b.sin() - a.sin()) + y * (a.cos() - b.cos())) * (r * r);
                (s, r * (b - a))
            }
            _ => {
                let mut m = V3::zeros();
                let mut l = 0.0;
                for (a, b) in self.spans() {
                    let h = 0.5 * (b - a);
                    for (xi, w) in GAUSS8 {
                        let t = a + h * (xi + 1.0);
                        let ds = self.curve.tangent(t).norm() * w * h;
                        m += self.point(t) * ds;
                        l += ds;
                    }
                }
                (m, l)
            }
        }
    }

    /// The centroid (`Center()`).
    pub fn centroid(&self) -> V3 {
        let (m, l) = self.moments();
        if l > 0.0 { m / l } else { self.start() }
    }

    /// The parameter at a fraction `f` of the length from the start (`paramAt(d)`).
    pub fn param_at_fraction(&self, f: f64) -> f64 {
        match &self.curve {
            Curve::Line { .. } | Curve::Circle { .. } => self.t0 + (self.t1 - self.t0) * f,
            _ => {
                let total = self.length();
                let target = total * f;
                // Bisection on the length (monotone).
                let (mut lo, mut hi) = (self.t0, self.t1);
                for _ in 0..60 {
                    let mid = 0.5 * (lo + hi);
                    if self.curve.length(self.t0, mid) < target {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                0.5 * (lo + hi)
            }
        }
    }

    /// The parameter of the point of the edge nearest `q`.
    pub fn param_of(&self, q: V3) -> f64 {
        self.curve
            .closest(&P3::from(q), self.t0, self.t1)
            .clamp(self.t0, self.t1)
    }

    /// A tight box.
    pub fn bbox(&self) -> BoundBox {
        let mut pts = vec![self.start(), self.end()];
        match &self.curve {
            Curve::Line { .. } => {}
            Curve::Circle { c, x, y, r } => {
                // Extremes along each axis: d/dt (x_k cos t + y_k sin t) = 0.
                for k in 0..3 {
                    let base = (y[k]).atan2(x[k]);
                    for j in -4..=4 {
                        let t = base + j as f64 * std::f64::consts::PI;
                        if t > self.t0 && t < self.t1 {
                            pts.push(c.coords + (x * t.cos() + y * t.sin()) * *r);
                        }
                    }
                }
            }
            Curve::Ellipse { c, x, y, a, b } => {
                for k in 0..3 {
                    let base = (y[k] * b).atan2(x[k] * a);
                    for j in -4..=4 {
                        let t = base + j as f64 * std::f64::consts::PI;
                        if t > self.t0 && t < self.t1 {
                            pts.push(c.coords + x * (a * t.cos()) + y * (b * t.sin()));
                        }
                    }
                }
            }
            Curve::Nurbs(_) => {
                // Sample densely, then refine each sampled extreme by golden-section search.
                let spans = self.spans();
                for (a, b) in spans {
                    let n = 24;
                    let ts: Vec<f64> = (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect();
                    let ps: Vec<V3> = ts.iter().map(|&t| self.point(t)).collect();
                    pts.extend(ps.iter().copied());
                    for k in 0..3 {
                        for i in 1..n {
                            let (p0, p1, p2) = (ps[i - 1][k], ps[i][k], ps[i + 1][k]);
                            for sign in [1.0, -1.0] {
                                if sign * p1 >= sign * p0 && sign * p1 >= sign * p2 {
                                    let (mut lo, mut hi) = (ts[i - 1], ts[i + 1]);
                                    for _ in 0..40 {
                                        let m1 = lo + (hi - lo) * 0.381_966;
                                        let m2 = lo + (hi - lo) * 0.618_034;
                                        if sign * self.point(m1)[k] > sign * self.point(m2)[k] {
                                            hi = m2;
                                        } else {
                                            lo = m1;
                                        }
                                    }
                                    pts.push(self.point(0.5 * (lo + hi)));
                                }
                            }
                        }
                    }
                }
            }
        }
        BoundBox::of_points(pts.iter()).expect("an edge has points")
    }

    /// Its geometric kind.
    pub fn geom_type(&self) -> GeomType {
        match &self.curve {
            Curve::Line { .. } => GeomType::Line,
            Curve::Circle { .. } => GeomType::Circle,
            Curve::Ellipse { .. } => GeomType::Ellipse,
            Curve::Nurbs(_) => GeomType::BSpline,
        }
    }

    /// Points along it within `defl` of the curve (for containment tests).
    pub fn polyline(&self, defl: f64) -> Vec<V3> {
        let n = match &self.curve {
            Curve::Line { .. } => 1,
            Curve::Circle { r, .. } => {
                let step = if *r > defl {
                    2.0 * (1.0 - defl / r).acos()
                } else {
                    1.0
                };
                (((self.t1 - self.t0) / step.max(1e-3)).ceil() as usize).clamp(4, 4096)
            }
            _ => (self.spans().len() * 8).clamp(8, 4096),
        };
        (0..=n)
            .map(|i| self.point(self.t0 + (self.t1 - self.t0) * i as f64 / n as f64))
            .collect()
    }
}

/// A face, shell or solid of a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Part {
    Face(usize),
    Shell(usize),
    Solid(usize),
}

/// How a [`Shape`] holds its geometry.
#[derive(Clone)]
pub(crate) enum Repr {
    /// A whole body of the session: a solid, a compound of several, a shell or a face.
    Body { body: BodyRef, kind: ShapeType },
    /// A face, shell or solid of a body; its own body made the first time one is needed.
    Part {
        body: BodyRef,
        part: Part,
        own: Arc<OnceLock<BodyRef>>,
    },
    /// An edge, and the body edge it was taken from.
    Edge {
        geom: Arc<EdgeGeom>,
        from: Option<(BodyRef, usize)>,
    },
    /// Edges joined end to end, and the body edges they were taken from.
    Wire {
        edges: Arc<Vec<EdgeGeom>>,
        from: Option<(BodyRef, Arc<Vec<usize>>)>,
    },
    /// A point, and the body vertex it was taken from.
    Vertex {
        p: V3,
        from: Option<(BodyRef, usize)>,
    },
    /// Shapes together.
    Compound(Arc<Vec<Shape>>),
}

/// A shape (`cadquery.Shape` and its subclasses).
#[derive(Clone)]
pub struct Shape {
    pub(crate) repr: Repr,
    /// Construction geometry (drawn, but not made into faces).
    pub for_construction: bool,
    /// Its label (`label`).
    pub label: String,
}

impl std::fmt::Debug for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.shape_type().name())
    }
}

/// The identity of a shape, as OCCT's `IsSame` sees it: the same body and part, or the same
/// geometry object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShapeKey(pub u64, pub u8, pub u64);

/// What a body holds, as a shape kind.
pub(crate) fn kind_of(b: &Body) -> ShapeType {
    if b.solids.len() == 1 && b.solid_faces(0).len() == b.faces.len() && b.wires.is_empty() {
        ShapeType::Solid
    } else if !b.solids.is_empty() {
        ShapeType::Compound
    } else if b.faces.len() == 1 && b.wires.is_empty() {
        ShapeType::Face
    } else if b.shells.len() == 1 && b.wires.is_empty() {
        ShapeType::Shell
    } else {
        ShapeType::Compound
    }
}

/// A body of the faces of one solid of `b`, its shells kept.
pub(crate) fn solid_body(b: &Body, s: usize) -> Body {
    let shells = &b.solids[s].shells;
    let faces: Vec<usize> = shells
        .iter()
        .flat_map(|&sh| b.shells[sh].faces.iter().copied())
        .collect();
    let mut out = crate::native::kernel::subset(b, &faces, true);
    out.shells.clear();
    let mut k = 0;
    for &sh in shells {
        let n = b.shells[sh].faces.len();
        out.shells.push(crate::native::topo::Shell {
            faces: (k..k + n).collect(),
            closed: b.shells[sh].closed,
        });
        k += n;
    }
    out.solids = vec![crate::native::topo::Solid {
        shells: (0..shells.len()).collect(),
    }];
    out
}

/// A body of the faces of one shell of `b`.
fn shell_body(b: &Body, s: usize) -> Body {
    let sh = &b.shells[s];
    crate::native::kernel::subset(b, &sh.faces, sh.closed && false)
}

impl Shape {
    pub(crate) fn of(repr: Repr) -> Self {
        Self {
            repr,
            for_construction: false,
            label: String::new(),
        }
    }

    /// A shape of a body the session made.
    pub(crate) fn of_body(body: BodyRef) -> Self {
        let kind = kind_of(&brep(&body));
        Self::of(Repr::Body { body, kind })
    }

    /// A shape of a native body.
    pub(crate) fn of_native(b: Body) -> Self {
        let kind = kind_of(&b);
        Self::of(Repr::Body {
            body: insert(b),
            kind,
        })
    }

    /// An edge.
    pub fn edge(g: EdgeGeom) -> Self {
        Self::of(Repr::Edge {
            geom: Arc::new(g),
            from: None,
        })
    }

    /// A wire of edges joined end to end.
    pub fn wire(edges: Vec<EdgeGeom>) -> Self {
        Self::of(Repr::Wire {
            edges: Arc::new(edges),
            from: None,
        })
    }

    /// A vertex.
    pub fn vertex(p: V3) -> Self {
        Self::of(Repr::Vertex { p, from: None })
    }

    /// A compound (`Compound.makeCompound`).
    pub fn compound(shapes: Vec<Shape>) -> Self {
        Self::of(Repr::Compound(Arc::new(shapes)))
    }

    /// Marks it as construction geometry.
    pub fn construction(mut self, yes: bool) -> Self {
        self.for_construction = yes;
        self
    }

    /// Its kind.
    pub fn shape_type(&self) -> ShapeType {
        match &self.repr {
            Repr::Body { kind, .. } => *kind,
            Repr::Part {
                part: Part::Face(_),
                ..
            } => ShapeType::Face,
            Repr::Part {
                part: Part::Shell(_),
                ..
            } => ShapeType::Shell,
            Repr::Part {
                part: Part::Solid(_),
                ..
            } => ShapeType::Solid,
            Repr::Edge { .. } => ShapeType::Edge,
            Repr::Wire { .. } => ShapeType::Wire,
            Repr::Vertex { .. } => ShapeType::Vertex,
            Repr::Compound(_) => ShapeType::Compound,
        }
    }

    /// Its identity.
    pub fn key(&self) -> ShapeKey {
        let id = |b: &BodyRef| b.0.0;
        match &self.repr {
            Repr::Body { body, kind } => {
                let b = brep(body);
                match kind {
                    ShapeType::Solid => ShapeKey(id(body), 3, 0),
                    ShapeType::Face if b.shells.len() <= 1 => ShapeKey(id(body), 1, 0),
                    _ => ShapeKey(id(body), 0, 0),
                }
            }
            Repr::Part { body, part, .. } => match part {
                Part::Face(f) => ShapeKey(id(body), 1, *f as u64),
                Part::Shell(s) => ShapeKey(id(body), 2, *s as u64),
                Part::Solid(s) => ShapeKey(id(body), 3, *s as u64),
            },
            Repr::Edge { geom, from } => match from {
                Some((b, e)) => ShapeKey(id(b), 4, *e as u64),
                None => ShapeKey(u64::MAX, 4, Arc::as_ptr(geom) as u64),
            },
            Repr::Wire { edges, from } => match from {
                Some((b, ids)) => ShapeKey(
                    id(b),
                    5,
                    ids.first().copied().unwrap_or(0) as u64 * 1_000_003 + ids.len() as u64,
                ),
                None => ShapeKey(u64::MAX, 5, Arc::as_ptr(edges) as u64),
            },
            Repr::Vertex { p, from } => match from {
                Some((b, v)) => ShapeKey(id(b), 6, *v as u64),
                None => ShapeKey(
                    u64::MAX - 1,
                    6,
                    p.x.to_bits() ^ p.y.to_bits().rotate_left(21) ^ p.z.to_bits().rotate_left(42),
                ),
            },
            Repr::Compound(v) => ShapeKey(u64::MAX, 7, Arc::as_ptr(v) as u64),
        }
    }

    /// `isSame`.
    pub fn is_same(&self, o: &Shape) -> bool {
        self.key() == o.key()
    }

    // ---------------------------------------------------------------- body access

    /// The body and the faces of it that make this shape, for body shapes.
    pub(crate) fn faces_of(&self) -> Option<(Arc<Body>, &BodyRef, Vec<usize>)> {
        match &self.repr {
            Repr::Body { body, .. } => {
                let b = brep(body);
                let n = b.faces.len();
                Some((b, body, (0..n).collect()))
            }
            Repr::Part { body, part, .. } => {
                let b = brep(body);
                let f = match part {
                    Part::Face(f) => vec![*f],
                    Part::Shell(s) => b.shells[*s].faces.clone(),
                    Part::Solid(s) => b.solid_faces(*s),
                };
                Some((b, body, f))
            }
            _ => None,
        }
    }

    /// The shape's own body: a part copied out, a compound's members put together, edges and
    /// wires as a wire body. None for a vertex.
    pub(crate) fn body(&self) -> CqResult<BodyRef> {
        match &self.repr {
            Repr::Body { body, .. } => Ok(body.clone()),
            Repr::Part { body, part, own } => {
                if let Some(b) = own.get() {
                    return Ok(b.clone());
                }
                let b = brep(body);
                let nb = match part {
                    Part::Face(f) => crate::native::kernel::subset(&b, &[*f], false),
                    Part::Shell(s) => shell_body(&b, *s),
                    Part::Solid(s) => solid_body(&b, *s),
                };
                let r = insert(nb);
                let _ = own.set(r.clone());
                Ok(own.get().cloned().unwrap_or(r))
            }
            Repr::Compound(v) => {
                let mut out = Body::default();
                for s in v.iter() {
                    if matches!(s.shape_type(), ShapeType::Vertex) {
                        continue;
                    }
                    let b = brep(&s.body()?);
                    out.append(&b);
                }
                Ok(insert(out))
            }
            Repr::Edge { geom, .. } => {
                Ok(insert(super::make::wire_body(std::slice::from_ref(geom))?))
            }
            Repr::Wire { edges, .. } => Ok(insert(super::make::wire_body(edges)?)),
            Repr::Vertex { .. } => value_err!("a vertex has no body"),
        }
    }

    /// The B-rep of [`Self::body`].
    pub(crate) fn native(&self) -> CqResult<Arc<Body>> {
        Ok(brep(&self.body()?))
    }

    // ---------------------------------------------------------------- topology

    /// Sub-shapes of a kind, each once, in OCCT's order (`_entities`).
    pub fn entities(&self, kind: ShapeType) -> Vec<Shape> {
        if self.shape_type() == kind && !matches!(kind, ShapeType::Compound) {
            return vec![self.clone()];
        }
        match &self.repr {
            Repr::Compound(v) => {
                let mut seen = std::collections::HashSet::new();
                let mut out = Vec::new();
                if kind == ShapeType::Compound {
                    out.push(self.clone());
                }
                for s in v.iter() {
                    for e in s.entities(kind) {
                        if seen.insert(e.key()) {
                            out.push(e);
                        }
                    }
                }
                out
            }
            Repr::Edge { geom, from } => match kind {
                ShapeType::Vertex => {
                    let ids = from.as_ref().map(|(b, e)| (b.clone(), brep(b).edges[*e].v));
                    let mut out = vec![Shape::of(Repr::Vertex {
                        p: geom.start(),
                        from: ids.as_ref().map(|(b, v)| (b.clone(), v[0])),
                    })];
                    if !geom.is_closed() {
                        out.push(Shape::of(Repr::Vertex {
                            p: geom.end(),
                            from: ids.as_ref().map(|(b, v)| (b.clone(), v[1])),
                        }));
                    }
                    out
                }
                _ => vec![],
            },
            Repr::Wire { edges, from } => match kind {
                ShapeType::Edge => edges
                    .iter()
                    .enumerate()
                    .map(|(i, g)| {
                        Shape::of(Repr::Edge {
                            geom: Arc::new(g.clone()),
                            from: from.as_ref().map(|(b, ids)| (b.clone(), ids[i])),
                        })
                    })
                    .collect(),
                ShapeType::Vertex => {
                    let mut out: Vec<Shape> = Vec::new();
                    for (i, g) in edges.iter().enumerate() {
                        let p = g.start();
                        let v = from.as_ref().map(|(b, ids)| {
                            let e = &brep(b).edges[ids[i]];
                            let bb = brep(b);
                            let pv = if (bb.verts[e.v[0]].p.coords - p).norm()
                                <= (bb.verts[e.v[1]].p.coords - p).norm()
                            {
                                e.v[0]
                            } else {
                                e.v[1]
                            };
                            (b.clone(), pv)
                        });
                        if !out.iter().any(|o| (o.center() - p).norm() < 1e-9) {
                            out.push(Shape::of(Repr::Vertex { p, from: v }));
                        }
                    }
                    if let Some(last) = edges.last()
                        && !out.iter().any(|o| (o.center() - last.end()).norm() < 1e-9)
                    {
                        out.push(Shape::vertex(last.end()));
                    }
                    out
                }
                _ => vec![],
            },
            Repr::Vertex { .. } => vec![],
            Repr::Body { .. } | Repr::Part { .. } => self.body_entities(kind),
        }
    }

    fn body_entities(&self, kind: ShapeType) -> Vec<Shape> {
        let Some((b, body, faces)) = self.faces_of() else {
            return vec![];
        };
        let part = |p: Part| {
            Shape::of(Repr::Part {
                body: body.clone(),
                part: p,
                own: Arc::new(OnceLock::new()),
            })
        };
        match kind {
            ShapeType::Face => faces.into_iter().map(|f| part(Part::Face(f))).collect(),
            ShapeType::Edge => {
                let mut seen = vec![false; b.edges.len()];
                let mut out = Vec::new();
                for f in faces {
                    for l in &b.faces[f].loops {
                        for c in l {
                            let e = &b.edges[c.edge];
                            if e.degenerate || seen[c.edge] {
                                continue;
                            }
                            seen[c.edge] = true;
                            out.push(Shape::of(Repr::Edge {
                                geom: Arc::new(EdgeGeom {
                                    curve: e.curve.clone(),
                                    t0: e.t[0],
                                    t1: e.t[1],
                                }),
                                from: Some((body.clone(), c.edge)),
                            }));
                        }
                    }
                }
                if matches!(self.repr, Repr::Body { .. }) {
                    for w in &b.wires {
                        for &(ei, _) in &w.edges {
                            if !seen[ei] {
                                seen[ei] = true;
                                let e = &b.edges[ei];
                                out.push(Shape::of(Repr::Edge {
                                    geom: Arc::new(EdgeGeom {
                                        curve: e.curve.clone(),
                                        t0: e.t[0],
                                        t1: e.t[1],
                                    }),
                                    from: Some((body.clone(), ei)),
                                }));
                            }
                        }
                    }
                }
                out
            }
            ShapeType::Vertex => {
                let mut seen = vec![false; b.verts.len()];
                let mut out = Vec::new();
                let mut push = |v: usize, out: &mut Vec<Shape>| {
                    if !seen[v] {
                        seen[v] = true;
                        out.push(Shape::of(Repr::Vertex {
                            p: b.verts[v].p.coords,
                            from: Some((body.clone(), v)),
                        }));
                    }
                };
                for f in &faces {
                    for l in &b.faces[*f].loops {
                        for c in l {
                            let e = &b.edges[c.edge];
                            if e.degenerate {
                                continue;
                            }
                            let (a, z) = if c.fwd {
                                (e.v[0], e.v[1])
                            } else {
                                (e.v[1], e.v[0])
                            };
                            push(a, &mut out);
                            push(z, &mut out);
                        }
                    }
                }
                if matches!(self.repr, Repr::Body { .. }) {
                    for w in &b.wires {
                        for &(ei, _) in &w.edges {
                            push(b.edges[ei].v[0], &mut out);
                            push(b.edges[ei].v[1], &mut out);
                        }
                    }
                    for &v in &b.points {
                        push(v, &mut out);
                    }
                }
                out
            }
            ShapeType::Wire => {
                let mut out = Vec::new();
                for f in faces {
                    for li in 0..b.faces[f].loops.len() {
                        out.push(loop_wire(&b, body, f, li));
                    }
                }
                out
            }
            ShapeType::Shell => match &self.repr {
                Repr::Part {
                    part: Part::Face(_),
                    ..
                } => vec![],
                Repr::Part {
                    part: Part::Solid(s),
                    ..
                } => b.solids[*s]
                    .shells
                    .iter()
                    .map(|&s| part(Part::Shell(s)))
                    .collect(),
                Repr::Part {
                    part: Part::Shell(_),
                    ..
                } => vec![self.clone()],
                _ => (0..b.shells.len()).map(|s| part(Part::Shell(s))).collect(),
            },
            ShapeType::Solid => match &self.repr {
                Repr::Body { .. } => (0..b.solids.len()).map(|s| part(Part::Solid(s))).collect(),
                _ => vec![],
            },
            ShapeType::Compound => match &self.repr {
                Repr::Body {
                    kind: ShapeType::Compound,
                    ..
                } => vec![self.clone()],
                _ => vec![],
            },
            ShapeType::CompSolid => vec![],
        }
    }

    /// `Vertices()`.
    pub fn vertices(&self) -> Vec<Shape> {
        self.entities(ShapeType::Vertex)
    }
    /// `Edges()`.
    pub fn edges(&self) -> Vec<Shape> {
        self.entities(ShapeType::Edge)
    }
    /// `Wires()`.
    pub fn wires(&self) -> Vec<Shape> {
        self.entities(ShapeType::Wire)
    }
    /// `Faces()`.
    pub fn faces(&self) -> Vec<Shape> {
        self.entities(ShapeType::Face)
    }
    /// `Shells()`.
    pub fn shells(&self) -> Vec<Shape> {
        self.entities(ShapeType::Shell)
    }
    /// `Solids()`.
    pub fn solids(&self) -> Vec<Shape> {
        self.entities(ShapeType::Solid)
    }

    /// The members of a compound (iterating it); the shape itself otherwise.
    pub fn children(&self) -> Vec<Shape> {
        match &self.repr {
            Repr::Compound(v) => v.as_ref().clone(),
            Repr::Body {
                kind: ShapeType::Compound,
                ..
            } => {
                let s = self.solids();
                let b = self.faces_of().map(|(b, _, _)| b);
                // A compound body: its solids, then its sheets' faces.
                let mut out = s;
                if let Some(b) = b {
                    let in_solid: std::collections::HashSet<usize> =
                        (0..b.solids.len()).flat_map(|i| b.solid_faces(i)).collect();
                    for (i, sh) in b.shells.iter().enumerate() {
                        if !sh.faces.iter().any(|f| in_solid.contains(f)) {
                            out.push(Shape::of(Repr::Part {
                                body: self.body().expect("a body"),
                                part: if sh.faces.len() == 1 {
                                    Part::Face(sh.faces[0])
                                } else {
                                    Part::Shell(i)
                                },
                                own: Arc::new(OnceLock::new()),
                            }));
                        }
                    }
                }
                out
            }
            _ => vec![self.clone()],
        }
    }

    /// The edges of an edge or wire, in order.
    pub fn edge_geoms(&self) -> Vec<EdgeGeom> {
        match &self.repr {
            Repr::Edge { geom, .. } => vec![geom.as_ref().clone()],
            Repr::Wire { edges, .. } => edges.as_ref().clone(),
            _ => self
                .edges()
                .iter()
                .filter_map(|e| match &e.repr {
                    Repr::Edge { geom, .. } => Some(geom.as_ref().clone()),
                    _ => None,
                })
                .collect(),
        }
    }

    /// The body edge an edge was taken from.
    pub(crate) fn edge_from(&self) -> Option<(BodyRef, usize)> {
        match &self.repr {
            Repr::Edge { from, .. } => from.clone(),
            _ => None,
        }
    }

    // ---------------------------------------------------------------- measures

    /// The kind of measure CadQuery's mass properties use for it (OCCT's table, a compound by
    /// its first member): Edge (length), Face (area), Solid (volume); None for a vertex.
    pub fn mass_kind(&self) -> Option<ShapeType> {
        match self.shape_type() {
            ShapeType::Vertex => None,
            ShapeType::Edge | ShapeType::Wire => Some(ShapeType::Edge),
            ShapeType::Face | ShapeType::Shell => Some(ShapeType::Face),
            ShapeType::Solid | ShapeType::CompSolid => Some(ShapeType::Solid),
            ShapeType::Compound => match &self.repr {
                Repr::Compound(v) => match v.first() {
                    Some(c) => c.mass_kind(),
                    None => Some(ShapeType::Solid),
                },
                _ => Some(ShapeType::Solid),
            },
        }
    }

    /// The mass (length, area or volume, per [`Self::mass_kind`]) and its first moments.
    pub(crate) fn mass_with(&self, kind: ShapeType) -> CqResult<(f64, V3)> {
        match kind {
            ShapeType::Edge => {
                let mut m = V3::zeros();
                let mut l = 0.0;
                for g in self.edge_geoms() {
                    let (gm, gl) = g.moments();
                    m += gm;
                    l += gl;
                }
                Ok((l, m))
            }
            ShapeType::Face => {
                if let Repr::Compound(v) = &self.repr {
                    let mut a = 0.0;
                    let mut m = V3::zeros();
                    for s in v.iter() {
                        let (sa, sm) = s.mass_with(kind)?;
                        a += sa;
                        m += sm;
                    }
                    return Ok((a, m));
                }
                let Some((b, _, faces)) = self.faces_of() else {
                    return Ok((0.0, V3::zeros()));
                };
                if faces.len() == b.faces.len()
                    && let Repr::Body { body, .. } = &self.repr
                {
                    let mp = session(|k| k.mass_properties(body.0))?;
                    return Ok((
                        mp.surface_area,
                        mp_area_center(&b, &faces) * mp.surface_area,
                    ));
                }
                let mut a = 0.0;
                let mut m = V3::zeros();
                for f in faces {
                    let (fa, c) = crate::native::mass::face_area(&b, f);
                    a += fa;
                    m += c.coords * fa;
                }
                Ok((a, m))
            }
            _ => {
                if let Repr::Compound(v) = &self.repr {
                    let mut a = 0.0;
                    let mut m = V3::zeros();
                    for s in v.iter() {
                        if let Some(k) = s.mass_kind()
                            && k == ShapeType::Solid
                        {
                            let (sa, sm) = s.mass_with(kind)?;
                            a += sa;
                            m += sm;
                        }
                    }
                    return Ok((a, m));
                }
                match self.shape_type() {
                    ShapeType::Solid | ShapeType::Compound | ShapeType::CompSolid => {
                        let body = self.body()?;
                        let mp = session(|k| k.mass_properties(body.0))?;
                        Ok((mp.volume, mp.center_of_mass.coords * mp.volume))
                    }
                    _ => Ok((0.0, V3::zeros())),
                }
            }
        }
    }

    /// `Volume()`: CadQuery's mass (a face's is its area, an edge's its length).
    pub fn volume(&self) -> CqResult<f64> {
        match self.mass_kind() {
            Some(k) => Ok(self.mass_with(k)?.0),
            None => Err(CqError::Value("'NoneType' object is not callable".into())),
        }
    }

    /// `Area()`: the faces' total area.
    pub fn area(&self) -> CqResult<f64> {
        Ok(self.mass_with(ShapeType::Face)?.0)
    }

    /// `Length()`: an edge's or wire's length.
    pub fn length(&self) -> f64 {
        self.edge_geoms().iter().map(|g| g.length()).sum()
    }

    /// `Center()`: the centre of mass, by [`Self::mass_kind`].
    pub fn center(&self) -> V3 {
        if let Repr::Vertex { p, .. } = &self.repr {
            return *p;
        }
        match self.mass_kind() {
            Some(k) => match self.mass_with(k) {
                Ok((m, mom)) if m.abs() > 0.0 => mom / m,
                _ => self.bounding_box().map(|b| b.center()).unwrap_or_default(),
            },
            None => V3::zeros(),
        }
    }

    /// `BoundingBox()`: a tight box of the exact geometry.
    pub fn bounding_box(&self) -> CqResult<BoundBox> {
        match &self.repr {
            Repr::Vertex { p, .. } => Ok(BoundBox { min: *p, max: *p }),
            Repr::Edge { geom, .. } => Ok(geom.bbox()),
            Repr::Wire { edges, .. } => edges
                .iter()
                .map(|g| g.bbox())
                .reduce(|a, b| a.union(&b))
                .ok_or_else(|| CqError::Value("an empty wire".into())),
            Repr::Compound(v) => {
                let mut out: Option<BoundBox> = None;
                for s in v.iter() {
                    let b = s.bounding_box()?;
                    out = Some(match out {
                        Some(o) => o.union(&b),
                        None => b,
                    });
                }
                out.ok_or_else(|| CqError::Value("an empty compound has no box".into()))
            }
            _ => {
                let body = self.body()?;
                let bb = session(|k| k.bounding_box(body.0))?;
                Ok(BoundBox {
                    min: bb.min.coords,
                    max: bb.max.coords,
                })
            }
        }
    }

    /// `isValid()`: the body passes the kernel's check.
    pub fn is_valid(&self) -> bool {
        match &self.repr {
            Repr::Vertex { .. } | Repr::Edge { .. } | Repr::Wire { .. } => true,
            Repr::Compound(v) => v.iter().all(|s| s.is_valid()),
            _ => match self.body() {
                Ok(b) => session(|k| k.sound(b.0)),
                Err(_) => false,
            },
        }
    }

    // ---------------------------------------------------------------- geometry

    /// `geomType()`.
    pub fn geom_type(&self) -> GeomType {
        match &self.repr {
            Repr::Edge { geom, .. } => geom.geom_type(),
            Repr::Part {
                body,
                part: Part::Face(f),
                ..
            } => surface_type(&brep(body).faces[*f].surf),
            Repr::Body {
                body,
                kind: ShapeType::Face,
            } => surface_type(&brep(body).faces[0].surf),
            _ => GeomType::Topo(self.shape_type()),
        }
    }

    /// A face's body and index.
    pub(crate) fn face_ref(&self) -> Option<(Arc<Body>, usize)> {
        match &self.repr {
            Repr::Part {
                body,
                part: Part::Face(f),
                ..
            } => Some((brep(body), *f)),
            Repr::Body {
                body,
                kind: ShapeType::Face,
            } => Some((brep(body), 0)),
            _ => None,
        }
    }

    /// A face's normal (`normalAt`): at the point of it nearest `at`, or at the middle of its
    /// parameter box; as the face is oriented (out of the material).
    pub fn normal_at(&self, at: Option<V3>) -> CqResult<V3> {
        let Some((b, f)) = self.face_ref() else {
            return value_err!("normalAt needs a face, not a {}", self.shape_type().name());
        };
        let face = &b.faces[f];
        let (u, v) = match at {
            Some(q) => face.surf.project(&P3::from(q), None),
            None => {
                let ((u0, u1), (v0, v1)) = face_uv_bounds(&b, f);
                (0.5 * (u0 + u1), 0.5 * (v0 + v1))
            }
        };
        let n = face.surf.normal(u, v);
        let n = if face.sense { n } else { -n };
        Ok(n.normalize())
    }

    /// The face's surface parameters of a point (`paramAt`).
    pub fn face_param_at(&self, q: V3) -> CqResult<(f64, f64)> {
        let Some((b, f)) = self.face_ref() else {
            return value_err!("paramAt needs a face");
        };
        Ok(b.faces[f].surf.project(&P3::from(q), None))
    }

    /// The face's surface point at `(u, v)` (`positionAt`).
    pub fn face_point_at(&self, u: f64, v: f64) -> CqResult<V3> {
        let Some((b, f)) = self.face_ref() else {
            return value_err!("positionAt needs a face");
        };
        Ok(b.faces[f].surf.eval(u, v).coords)
    }

    /// The face's parameter box (`uvBounds`).
    pub fn uv_bounds(&self) -> CqResult<(f64, f64, f64, f64)> {
        let Some((b, f)) = self.face_ref() else {
            return value_err!("uvBounds needs a face");
        };
        let ((u0, u1), (v0, v1)) = face_uv_bounds(&b, f);
        Ok((u0, u1, v0, v1))
    }

    /// The face's surface, for planar faces their plane (origin on it, unit normal).
    pub(crate) fn face_plane(&self) -> Option<(V3, V3)> {
        let (b, f) = self.face_ref()?;
        let face = &b.faces[f];
        match &face.surf {
            Surface::Plane { f: fr } => {
                let n = if face.sense { fr.z } else { -fr.z };
                Some((fr.o.coords, n))
            }
            _ => None,
        }
    }

    /// A face's outer wire (`outerWire`): its first loop.
    pub fn outer_wire(&self) -> CqResult<Shape> {
        let Some((b, f)) = self.face_ref() else {
            return value_err!("outerWire needs a face");
        };
        let body = match &self.repr {
            Repr::Part { body, .. } | Repr::Body { body, .. } => body.clone(),
            _ => unreachable!(),
        };
        let _ = &b;
        Ok(loop_wire(&brep(&body), &body, f, 0))
    }

    /// A face's inner wires (`innerWires`).
    pub fn inner_wires(&self) -> CqResult<Vec<Shape>> {
        let Some((b, f)) = self.face_ref() else {
            return value_err!("innerWires needs a face");
        };
        let body = match &self.repr {
            Repr::Part { body, .. } | Repr::Body { body, .. } => body.clone(),
            _ => unreachable!(),
        };
        Ok((1..b.faces[f].loops.len())
            .map(|l| loop_wire(&b, &body, f, l))
            .collect())
    }

    // ---------------------------------------------------------------- 1D

    /// The first edge of an edge or wire.
    fn first_edge(&self) -> CqResult<EdgeGeom> {
        self.edge_geoms()
            .into_iter()
            .next()
            .ok_or_else(|| CqError::Value(format!("a {} has no edges", self.shape_type().name())))
    }

    /// `startPoint()`.
    pub fn start_point(&self) -> CqResult<V3> {
        Ok(self.first_edge()?.start())
    }

    /// `endPoint()`.
    pub fn end_point(&self) -> CqResult<V3> {
        self.edge_geoms()
            .last()
            .map(|g| g.end())
            .ok_or_else(|| CqError::Value("no edges".into()))
    }

    /// Where a fraction `d` of the length (or the parameter `d` of a lone edge's curve, `mode`
    /// "parameter") falls: the edge and its parameter.
    fn locate(&self, d: f64, by_length: bool) -> CqResult<(EdgeGeom, f64)> {
        let edges = self.edge_geoms();
        if edges.is_empty() {
            return value_err!("no edges");
        }
        if edges.len() == 1 {
            let g = edges[0].clone();
            let t = if by_length { g.param_at_fraction(d) } else { d };
            return Ok((g, t));
        }
        let lens: Vec<f64> = edges.iter().map(|g| g.length()).collect();
        let total: f64 = lens.iter().sum();
        let mut target = if by_length { d * total } else { d * total };
        for (g, l) in edges.iter().zip(&lens) {
            if target <= *l || std::ptr::eq(g, edges.last().unwrap()) {
                let f = if *l > 0.0 {
                    (target / l).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                return Ok((g.clone(), g.param_at_fraction(f)));
            }
            target -= l;
        }
        unreachable!()
    }

    /// `positionAt(d, mode)`.
    pub fn position_at(&self, d: f64, by_length: bool) -> CqResult<V3> {
        let (g, t) = self.locate(d, by_length)?;
        Ok(g.point(t))
    }

    /// `tangentAt(d, mode)`: the unit tangent.
    pub fn tangent_at(&self, d: f64, by_length: bool) -> CqResult<V3> {
        let (g, t) = self.locate(d, by_length)?;
        Ok(g.tangent(t))
    }

    /// `radius()`: a circular edge's (a wire's first edge's).
    pub fn radius(&self) -> CqResult<f64> {
        match self.first_edge()?.curve {
            Curve::Circle { r, .. } => Ok(r),
            _ => value_err!("Shape could not be reduced to a circle"),
        }
    }

    /// `arcCenter()`.
    pub fn arc_center(&self) -> CqResult<V3> {
        match self.first_edge()?.curve {
            Curve::Circle { c, .. } | Curve::Ellipse { c, .. } => Ok(c.coords),
            _ => value_err!("{} has no arc center", self.geom_type().name()),
        }
    }

    /// `IsClosed()`.
    pub fn is_closed(&self) -> bool {
        let e = self.edge_geoms();
        match (e.first(), e.last()) {
            (Some(a), Some(b)) => (a.start() - b.end()).norm() <= 1e-6 * (1.0 + a.start().norm()),
            _ => match &self.repr {
                Repr::Body { body, .. } => brep(body).shells.iter().all(|s| s.closed),
                _ => false,
            },
        }
    }

    // ---------------------------------------------------------------- moves

    /// The shape moved by an affine map (rigid, mirrored or scaled).
    pub fn transformed(&self, m: &Affine) -> CqResult<Shape> {
        let out = match &self.repr {
            Repr::Vertex { p, .. } => Repr::Vertex {
                p: m.m * p + m.t,
                from: None,
            },
            Repr::Edge { geom, .. } => Repr::Edge {
                geom: Arc::new(geom.transformed(m)),
                from: None,
            },
            Repr::Wire { edges, .. } => Repr::Wire {
                edges: Arc::new(edges.iter().map(|g| g.transformed(m)).collect()),
                from: None,
            },
            Repr::Compound(v) => Repr::Compound(Arc::new(
                v.iter()
                    .map(|s| s.transformed(m))
                    .collect::<CqResult<_>>()?,
            )),
            _ => {
                let body = self.body()?;
                let b = brep(&body);
                let det = m.m.determinant();
                let rigid = (m.m.transpose() * m.m - nalgebra::Matrix3::identity()).norm() < 1e-12;
                let nb = if rigid && det > 0.0 {
                    // Through the session: it keeps what it knows of moved bodies.
                    let iso = nalgebra::Isometry3::from_parts(
                        nalgebra::Translation3::from(m.t),
                        // (Exactly: `from_matrix` iterates from the identity and stays there for a half turn.)
                        nalgebra::UnitQuaternion::from_rotation_matrix(
                            &nalgebra::Rotation3::from_matrix_unchecked(m.m),
                        ),
                    );
                    let r = super::op1(|k| k.transform(body.0, &iso))?;
                    return Ok(self.keep_flags(Shape::of_body(r)));
                } else {
                    crate::native::affine::affine_body(&b, m)?
                };
                return Ok(self.keep_flags(Shape::of_native(nb)));
            }
        };
        Ok(self.keep_flags(Shape::of(out)))
    }

    fn keep_flags(&self, mut s: Shape) -> Shape {
        s.for_construction = self.for_construction;
        s.label = self.label.clone();
        s
    }

    /// `moved(loc)` / `located`: the shape placed by a location.
    pub fn moved(&self, loc: &Location) -> CqResult<Shape> {
        if loc.0 == Affine::identity() {
            return Ok(self.clone());
        }
        self.transformed(&loc.0)
    }

    /// `translate(v)`.
    pub fn translate(&self, v: V3) -> CqResult<Shape> {
        self.transformed(&Affine::translation(v))
    }

    /// `rotate(start, end, degrees)`: about the axis from `a` to `b`.
    pub fn rotate(&self, a: V3, b: V3, deg: f64) -> CqResult<Shape> {
        let axis = b - a;
        if axis.norm() == 0.0 {
            return value_err!("gp_Dir() - input vector has zero norm");
        }
        let r = nalgebra::Rotation3::from_axis_angle(
            &nalgebra::Unit::new_normalize(axis),
            deg.to_radians(),
        );
        let m = *r.matrix();
        self.transformed(&Affine { m, t: a - m * a })
    }

    /// `mirror(normal, base)`: reflected in the plane through `base` of normal `n`.
    pub fn mirror(&self, n: V3, base: V3) -> CqResult<Shape> {
        let n = n.normalize();
        let m = nalgebra::Matrix3::identity() - 2.0 * n * n.transpose();
        self.transformed(&Affine {
            m,
            t: base - m * base,
        })
    }

    /// `scale(factor)`: about the origin.
    pub fn scale(&self, f: f64) -> CqResult<Shape> {
        self.transformed(&Affine {
            m: nalgebra::Matrix3::identity() * f,
            t: V3::zeros(),
        })
    }
}

/// The centroid of faces of a body (for a whole body's area moments).
fn mp_area_center(b: &Body, faces: &[usize]) -> V3 {
    let mut a = 0.0;
    let mut m = V3::zeros();
    for &f in faces {
        let (fa, c) = crate::native::mass::face_area(b, f);
        a += fa;
        m += c.coords * fa;
    }
    if a > 0.0 { m / a } else { V3::zeros() }
}

/// A surface's kind.
pub(crate) fn surface_type(s: &Surface) -> GeomType {
    match s {
        Surface::Plane { .. } => GeomType::Plane,
        Surface::Cylinder { .. } => GeomType::Cylinder,
        Surface::Cone { .. } => GeomType::Cone,
        Surface::Sphere { .. } => GeomType::Sphere,
        Surface::Torus { .. } => GeomType::Torus,
        Surface::Revolution { .. } => GeomType::Revolution,
        Surface::Extrusion { .. } => GeomType::Extrusion,
        Surface::Nurbs(_) => GeomType::BSpline,
        #[allow(unreachable_patterns)]
        _ => GeomType::BSpline,
    }
}

/// A face's parameter box: its loops' p-curves' extent.
pub(crate) fn face_uv_bounds(b: &Body, f: usize) -> ((f64, f64), (f64, f64)) {
    let face = &b.faces[f];
    let mut u = (f64::INFINITY, f64::NEG_INFINITY);
    let mut v = (f64::INFINITY, f64::NEG_INFINITY);
    for l in &face.loops {
        for c in l {
            let e = &b.edges[c.edge];
            let n = 16;
            for i in 0..=n {
                let t = e.t[0] + (e.t[1] - e.t[0]) * i as f64 / n as f64;
                let q = c.pc.point(t);
                u = (u.0.min(q.x), u.1.max(q.x));
                v = (v.0.min(q.y), v.1.max(q.y));
            }
        }
    }
    if !u.0.is_finite() {
        return face.surf.param_box();
    }
    (u, v)
}

/// The wire of loop `l` of face `f`, its edges run as the loop runs.
pub(crate) fn loop_wire(b: &Body, body: &BodyRef, f: usize, l: usize) -> Shape {
    let lp = &b.faces[f].loops[l];
    let mut edges = Vec::new();
    let mut ids = Vec::new();
    for c in lp {
        let e = &b.edges[c.edge];
        if e.degenerate {
            continue;
        }
        let g = EdgeGeom {
            curve: e.curve.clone(),
            t0: e.t[0],
            t1: e.t[1],
        };
        edges.push(if c.fwd { g } else { g.reversed() });
        ids.push(c.edge);
    }
    Shape::of(Repr::Wire {
        edges: Arc::new(edges),
        from: Some((body.clone(), Arc::new(ids))),
    })
}
