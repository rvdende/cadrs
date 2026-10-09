//! Links from a sketch to geometry outside it (T5): Use/project (S20), Pierce (S12.11) and
//! imprinting (S21).
//!
//! A [`Link`] names a part edge, a curved face's silhouette or another sketch's curve, by
//! persistent name (P3.2). Given the parts and features before the sketch, [`LinkContext`]
//! works out where the link puts a projected curve in the sketch plane
//! ([`LinkContext::shape`]) or where the linked curve pierces the plane
//! ([`LinkContext::pierce`]), and finds what a link refers to after a rebuild
//! ([`LinkContext::resolve`]). [`imprint`] lists the edges of the part faces that lie in a
//! sketch's plane.

use cadrs_sketch::projection::Projected;
use cadrs_sketch::{
    CurveId, CurveKind, EdgeName, FaceName, Imprint, ImprintShape, Link, PlaneFrame, Sketch,
    Vec2, Vec3, synthetic_curve,
};

use crate::document::Feature;
use crate::ids::FeatureId;
use crate::solid::Solid;

// Vector helpers.
fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: Vec3, k: f64) -> Vec3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn len(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: Vec3) -> Vec3 {
    let l = len(a);
    if l < 1e-15 { a } else { scale(a, 1.0 / l) }
}

/// A curve in space.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve3 {
    Line(Vec3, Vec3),
    /// A full circle: its center, unit normal, radius and a unit direction in its plane.
    Circle {
        center: Vec3,
        normal: Vec3,
        radius: f64,
        u: Vec3,
    },
    /// An arc from `start` through `mid` to `end` on the circle.
    Arc {
        center: Vec3,
        normal: Vec3,
        radius: f64,
        start: Vec3,
        mid: Vec3,
        end: Vec3,
    },
    /// An ellipse in a plane (only projected onto parallel planes), or the curve `offset` outside
    /// it (P3.7: an offset ellipse, 0 for the ellipse itself).
    Ellipse {
        center: Vec3,
        major: Vec3,
        minor: f64,
        normal: Vec3,
        offset: f64,
    },
    /// Any other curve (a spline edge, a cylinder's edge cut at a slant), as points on it in
    /// order (a closed curve lists each once).
    Sampled { points: Vec<Vec3>, closed: bool },
}

/// At most this many points of a curve are kept for [`Curve3::Sampled`] (and so for the
/// spline a Use makes of it).
const MAX_SAMPLES: usize = 48;

/// A curve through the points of a polyline on it (its exact points): every point while there
/// are at most [`MAX_SAMPLES`], else that many spread evenly along it (the ends kept).
pub fn sampled(pts: &[Vec3]) -> Option<Curve3> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let closed = n > 3 && len(sub(pts[0], pts[n - 1])) < 1e-9;
    let pts = if closed { &pts[..n - 1] } else { pts };
    let m = pts.len();
    if m <= MAX_SAMPLES {
        return Some(Curve3::Sampled { points: pts.to_vec(), closed });
    }
    // Arc length at each point (closing back to the first for a closed curve).
    let mut at = vec![0.0];
    for w in pts.windows(2) {
        at.push(at.last().unwrap() + len(sub(w[1], w[0])));
    }
    let total = at[m - 1] + if closed { len(sub(pts[0], pts[m - 1])) } else { 0.0 };
    let k = if closed { MAX_SAMPLES } else { MAX_SAMPLES - 1 };
    let mut out: Vec<Vec3> = Vec::new();
    let mut j = 0;
    for i in 0..=k {
        if closed && i == k {
            break;
        }
        let want = total * i as f64 / k as f64;
        while j + 1 < m && at[j + 1] <= want {
            j += 1;
        }
        // The nearer of the two points around it.
        let pick = if j + 1 < m && at[j + 1] - want < want - at[j] { j + 1 } else { j };
        if out.last().is_none_or(|q| len(sub(*q, pts[pick])) > 0.0) {
            out.push(pts[pick]);
        }
    }
    if !closed && out.last().is_some_and(|q| len(sub(*q, pts[m - 1])) > 0.0) {
        out.push(pts[m - 1]);
    }
    Some(Curve3::Sampled { points: out, closed })
}

/// The circle through three points in space: center and unit normal.
fn circumcircle(a: Vec3, b: Vec3, c: Vec3) -> Option<(Vec3, Vec3)> {
    let (ab, ac) = (sub(b, a), sub(c, a));
    let n = cross(ab, ac);
    let n2 = dot(n, n);
    if n2 < 1e-18 * dot(ab, ab).max(1e-300) * dot(ac, ac).max(1e-300) {
        return None;
    }
    let t = add(
        scale(cross(n, ab), dot(ac, ac)),
        scale(cross(ac, n), dot(ab, ab)),
    );
    Some((add(a, scale(t, 1.0 / (2.0 * n2))), unit(n)))
}

/// An edge polyline as the curve it samples (lines have two points; circles and arcs are
/// tessellated exactly on the circle, closed circles repeat their first point).
pub fn curve_of_polyline(pts: &[Vec3]) -> Option<Curve3> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    if n == 2 {
        return Some(Curve3::Line(pts[0], pts[1]));
    }
    let closed = len(sub(pts[0], pts[n - 1])) < 1e-9;
    let (a, b, c) = if closed {
        (pts[0], pts[(n - 1) / 3], pts[2 * (n - 1) / 3])
    } else {
        (pts[0], pts[n / 2], pts[n - 1])
    };
    // Only a curve every sample lies on (the polyline's points are on the exact edge): an
    // ellipse's or a spline's edge is none of these (P3.7: a half offset ellipse imprinted as a
    // circle's arc made false regions).
    let tol = |r: f64| 1e-6 * (1.0 + r);
    let Some((center, normal)) = circumcircle(a, b, c) else {
        let (p, q) = (pts[0], pts[n - 1]);
        let d = sub(q, p);
        let l = len(d).max(1e-300);
        let straight = pts.iter().all(|x| len(cross(sub(*x, p), d)) / l < tol(l));
        return straight.then_some(Curve3::Line(p, q));
    };
    let radius = len(sub(a, center));
    let on_circle = pts.iter().all(|x| {
        let v = sub(*x, center);
        (len(v) - radius).abs() < tol(radius) && dot(v, normal).abs() < tol(radius)
    });
    if !on_circle {
        return None;
    }
    Some(if closed {
        Curve3::Circle {
            center,
            normal,
            radius,
            u: unit(sub(a, center)),
        }
    } else {
        Curve3::Arc {
            center,
            normal,
            radius,
            start: a,
            mid: b,
            end: c,
        }
    })
}

/// A part edge as the curve it lies on: the kernel's exact circle for a circular edge (P3.4:
/// Use takes its edges from the kernel), else the curve its polyline samples.
pub fn edge_curve(e: &crate::solid::SolidEdge) -> Option<Curve3> {
    let pts = &e.points;
    let (Some(c), Some(first), Some(last)) = (e.circle, pts.first(), pts.last()) else {
        return curve_of_polyline(pts).or_else(|| sampled(pts));
    };
    let normal = unit(c.normal);
    // A point of the polyline put exactly on the circle.
    let on = |p: Vec3| {
        let v = sub(p, c.center);
        let v = sub(v, scale(normal, dot(v, normal)));
        add(c.center, scale(unit(v), c.radius))
    };
    let closed = pts.len() > 2 && len(sub(*first, *last)) < 1e-9;
    Some(if closed {
        Curve3::Circle {
            center: c.center,
            normal,
            radius: c.radius,
            u: unit(sub(on(*first), c.center)),
        }
    } else {
        Curve3::Arc {
            center: c.center,
            normal,
            radius: c.radius,
            start: on(*first),
            mid: on(e.midpoint()),
            end: on(*last),
        }
    })
}

/// How closely two directions must line up to count as parallel.
const PARALLEL: f64 = 1e-9;

/// A curve in space projected onto a sketch plane, if cadrs can draw the projection.
pub fn project(c: Curve3, frame: &PlaneFrame) -> Option<Projected> {
    let s = |p: Vec3| frame.to_sketch(p);
    let big_n = unit(frame.normal());
    let shape = match c {
        Curve3::Line(a, b) => Projected::Line(s(a), s(b)),
        Curve3::Circle {
            center,
            normal,
            radius,
            ..
        } => {
            let cos = dot(normal, big_n).abs();
            if cos > 1.0 - PARALLEL {
                Projected::Circle(s(center), radius)
            } else {
                // The diameter parallel to the plane keeps its length; the one across shrinks.
                let d = unit(cross(normal, big_n));
                let (p, q) = (s(center), s(add(center, scale(d, radius))));
                if cos < PARALLEL {
                    Projected::Line(s(sub(center, scale(d, radius))), q)
                } else {
                    Projected::Ellipse {
                        center: p,
                        major: q,
                        minor: radius * cos,
                    }
                }
            }
        }
        Curve3::Arc {
            center,
            normal,
            radius,
            start,
            mid,
            end,
        } => {
            let cos = dot(normal, big_n).abs();
            if cos > 1.0 - PARALLEL {
                Projected::arc_through(s(start), s(mid), s(end))?
            } else if cos < PARALLEL {
                // Edge-on: a segment between the extreme points.
                let pts = [s(start), s(mid), s(end)];
                let dir = pts[2] - pts[0];
                let dir = if dir.length() < 1e-9 { pts[1] - pts[0] } else { dir }.normalize();
                let key = |p: &Vec2| p.dot(dir);
                let lo = pts.iter().copied().min_by(|a, b| key(a).total_cmp(&key(b)))?;
                let hi = pts.iter().copied().max_by(|a, b| key(a).total_cmp(&key(b)))?;
                Projected::Line(lo, hi)
            } else {
                // An elliptical arc: on the circle's projection (as for a whole circle), from
                // whichever end makes it run counter-clockwise through the middle.
                let d = unit(cross(normal, big_n));
                let (c, major, minor) = (s(center), s(add(center, scale(d, radius))), radius * cos);
                let e = cadrs_sketch::geom::EllipseGeom::new(c, major, minor);
                let (ts, tm, te) = (e.param_of(s(start)), e.param_of(s(mid)), e.param_of(s(end)));
                let ccw = cadrs_sketch::geom::norm_angle(tm - ts) < cadrs_sketch::geom::norm_angle(te - ts);
                let (start, end) = if ccw { (s(start), s(end)) } else { (s(end), s(start)) };
                Projected::EllipseArc { center: c, major, minor, start, end }
            }
        }
        Curve3::Sampled { points, closed } => {
            let pts: Vec<Vec2> = points.iter().map(|p| s(*p)).collect();
            // Seen edge-on (all on one line): the segment between its extremes.
            let lo = *pts.first()?;
            let hi = pts.iter().copied().max_by(|a, b| a.distance(lo).total_cmp(&b.distance(lo)))?;
            let size = hi.distance(lo);
            let dir = (hi - lo).normalize();
            let straight = size > 0.0 && pts.iter().all(|p| dir.cross(*p - lo).abs() < 1e-9 * (1.0 + size));
            if straight {
                let key = |p: &Vec2| p.dot(dir);
                let a = pts.iter().copied().min_by(|x, y| key(x).total_cmp(&key(y)))?;
                let b = pts.iter().copied().max_by(|x, y| key(x).total_cmp(&key(y)))?;
                Projected::Line(a, b)
            } else {
                Projected::Spline { points: pts, closed }
            }
        }
        Curve3::Ellipse {
            center,
            major,
            minor,
            normal,
            offset,
        } => {
            if dot(normal, big_n).abs() > 1.0 - PARALLEL {
                if offset == 0.0 {
                    Projected::Ellipse {
                        center: s(center),
                        major: s(major),
                        minor,
                    }
                } else {
                    Projected::EllipseOffset {
                        center: s(center),
                        major: s(major),
                        minor,
                        distance: offset,
                    }
                }
            } else {
                return None;
            }
        }
    };
    (!shape.degenerate()).then_some(shape)
}

/// Where a curve in space crosses a plane: every point (sketch coordinates).
pub fn crossings(c: Curve3, frame: &PlaneFrame) -> Vec<Vec2> {
    let big_n = unit(frame.normal());
    let dist = |p: Vec3| frame.distance(p) / len(frame.normal()).max(1e-300);
    let circle_hits = |center: Vec3, normal: Vec3, radius: f64| -> Vec<Vec3> {
        // Points C + r(cos t·u + sin t·v) with distance 0.
        let u = unit(if cross(normal, big_n).iter().all(|x| x.abs() < 1e-12) {
            return Vec::new();
        } else {
            cross(normal, big_n)
        });
        let v = cross(normal, u);
        let (a, b, d) = (radius * dot(u, big_n), radius * dot(v, big_n), dist(center));
        let r = (a * a + b * b).sqrt();
        if r < 1e-12 || d.abs() > r * (1.0 + 1e-12) {
            return Vec::new();
        }
        let base = b.atan2(a);
        let off = (-d / r).clamp(-1.0, 1.0).acos();
        [base + off, base - off]
            .iter()
            .map(|t| add(center, add(scale(u, radius * t.cos()), scale(v, radius * t.sin()))))
            .collect()
    };
    let pts: Vec<Vec3> = match c {
        Curve3::Line(a, b) => {
            let (da, db) = (dist(a), dist(b));
            if (da - db).abs() < 1e-12 {
                return Vec::new();
            }
            // The whole line (an edge may end short of the plane when it moves).
            let t = da / (da - db);
            vec![add(a, scale(sub(b, a), t))]
        }
        Curve3::Circle {
            center,
            normal,
            radius,
            ..
        } => circle_hits(center, normal, radius),
        Curve3::Arc {
            center,
            normal,
            radius,
            start,
            mid,
            end,
        } => {
            // Only the points on the arc: between its ends, on the side of its middle.
            let chord = sub(end, start);
            let side = |p: Vec3| dot(cross(chord, sub(p, start)), normal);
            let want = side(mid);
            circle_hits(center, normal, radius)
                .into_iter()
                .filter(|p| side(*p) * want >= -1e-12)
                .collect()
        }
        Curve3::Ellipse { .. } => Vec::new(),
        // Where its segments cross (sampled: about on the curve).
        Curve3::Sampled { points, closed } => {
            let n = points.len();
            let segs = if closed { n } else { n.saturating_sub(1) };
            (0..segs)
                .filter_map(|i| {
                    let (a, b) = (points[i], points[(i + 1) % n]);
                    let (da, db) = (dist(a), dist(b));
                    (da.signum() != db.signum() || da == 0.0)
                        .then(|| if (da - db).abs() < 1e-300 { a } else { add(a, scale(sub(b, a), da / (da - db))) })
                })
                .collect()
        }
    };
    pts.into_iter().map(|p| frame.to_sketch(p)).collect()
}

/// Half the length of a plane's trace used in a sketch (mm; Normal to a plane, S12.10).
pub const PLANE_TRACE_HALF: f64 = 50.0;

/// Where the plane `p` cuts the sketch plane `frame`: a line `2 × PLANE_TRACE_HALF` long,
/// centred where it passes nearest the sketch's origin; `None` if the planes are parallel.
pub fn plane_trace(p: &PlaneFrame, frame: &PlaneFrame) -> Option<Curve3> {
    let (np, ns) = (unit(p.normal()), unit(frame.normal()));
    let d = cross(np, ns);
    if len(d) < 1e-9 {
        return None;
    }
    let d = unit(d);
    // In the sketch plane, the direction square to the trace; walk from the sketch origin
    // along it to the plane.
    let w = cross(ns, d);
    let k = dot(w, np);
    if k.abs() < 1e-12 {
        return None;
    }
    let t = dot(sub(p.origin, frame.origin), np) / k;
    let c = add(frame.origin, scale(w, t));
    Some(Curve3::Line(add(c, scale(d, -PLANE_TRACE_HALF)), add(c, scale(d, PLANE_TRACE_HALF))))
}

/// What links resolve against: the parts made before the sketch and the features before it.
pub struct LinkContext<'a> {
    pub solids: Vec<(FeatureId, &'a Solid)>,
    pub features: &'a [Feature],
}

impl<'a> LinkContext<'a> {
    /// The solid of a part `feature` made that `has` the entity: among its parts first, then
    /// any part (a boolean may have joined its part to another); else its first part.
    fn solid_where(&self, feature: uuid::Uuid, has: impl Fn(&Solid) -> bool) -> Option<&'a Solid> {
        let mine = || self.solids.iter().filter(|(id, _)| id.0 == feature).map(|(_, s)| *s);
        mine()
            .find(|s| has(s))
            .or_else(|| self.solids.iter().map(|(_, s)| *s).find(|s| has(s)))
            .or_else(|| mine().next())
    }

    fn solid_with_edge(&self, feature: uuid::Uuid, edge: &crate::solid::EdgeName) -> Option<&'a Solid> {
        self.solid_where(feature, |s| s.edges.iter().any(|e| e.name.base() == edge.base()))
    }

    fn solid_with_face(&self, feature: uuid::Uuid, face: &cadrs_sketch::FaceName) -> Option<&'a Solid> {
        self.solid_where(feature, |s| s.faces.iter().any(|f| f.name.base() == face.base()))
    }

    fn solid_with_vertex(&self, feature: uuid::Uuid, vertex: &cadrs_sketch::VertexName) -> Option<&'a Solid> {
        self.solid_where(feature, |s| s.vertices.iter().any(|v| v.name.base() == vertex.base()))
    }

    /// The exact curve of an edge an extrude swept from a sketch ellipse or offset ellipse (P3.7,
    /// PS21.4: the Funnel's rim is an offset ellipse, which its mesh polyline can't tell): the
    /// sketch curve moved along the sketch's normal to where the edge is. `None` for other
    /// edges.
    fn swept_ellipse(&self, e: &crate::solid::SolidEdge) -> Option<Curve3> {
        use slotmap::Key;
        for face in e.name.faces {
            let cadrs_sketch::FaceOrigin::Side { curve, .. } = face.origin else {
                continue;
            };
            let Some(ext) = self.features.iter().find(|f| f.id.0 == face.op).and_then(|f| f.extrude()) else {
                continue;
            };
            if ext.direction.is_some() {
                continue;
            }
            for sk_id in ext.sketches() {
                let Some(sk) = self.features.iter().find(|f| f.id == sk_id).and_then(|f| f.sketch()) else {
                    continue;
                };
                let Some(plane) = sk.plane else { continue };
                let frame = plane.frame();
                let g = &sk.geometry;
                let Some((_, c)) = g.curves.iter().find(|(id, _)| id.data().as_ffi() == curve) else {
                    continue;
                };
                let (center, major, minor, offset) = match c.kind {
                    CurveKind::Ellipse { center, major, minor } => (center, major, minor, 0.0),
                    CurveKind::EllipseOffset { center, major, minor, distance } => (center, major, minor, distance),
                    _ => return None,
                };
                let n = unit(frame.normal());
                let h = |p: Vec3| dot(sub(p, frame.origin), n);
                let first = *e.points.first()?;
                let size = 1.0 + len(sub(first, frame.origin));
                if e.points.iter().any(|p| (h(*p) - h(first)).abs() > 1e-6 * size) {
                    return None;
                }
                let w = |p: Vec2| add(frame.to_world(p), scale(n, h(first)));
                return Some(Curve3::Ellipse {
                    center: w(g.pos(center)),
                    major: w(g.pos(major)),
                    minor,
                    normal: n,
                    offset,
                });
            }
        }
        None
    }

    /// The linked curve in space (the edge with the link's exact name; see
    /// [`LinkContext::resolve`] for renamed ones).
    pub fn curve(&self, link: Link, frame: &PlaneFrame) -> Option<Curve3> {
        match link {
            Link::Edge { feature, edge } => {
                let e = self.solid_with_edge(feature, &edge)?.edge(&edge)?;
                self.swept_ellipse(e).or_else(|| edge_curve(e))
            }
            Link::Silhouette {
                feature,
                face,
                index,
            } => {
                let lines = silhouettes(self.solid_with_face(feature, &face)?, &face, frame.normal());
                let (a, b) = *lines.get(index as usize)?;
                Some(Curve3::Line(a, b))
            }
            Link::Plane(p) => plane_trace(&p.frame(), frame),
            // A flat pattern's line lies in the flat, not in space ([`crate::parts::regenerate`]
            // places it from the build's flat pattern).
            Link::FlatLine { .. } => None,
            // A vertex is a point (see [`LinkContext::pierce`]).
            Link::Vertex { .. } => None,
            Link::SketchCurve { feature, curve } => {
                let f = self.features.iter().find(|f| f.id.0 == feature)?;
                let sk = f.sketch()?;
                let src = sk.plane?.frame();
                let g = &sk.geometry;
                let w = |p: Vec2| src.to_world(p);
                let normal = unit(src.normal());
                Some(match g.curves.get(curve)?.kind {
                    CurveKind::Line { a, b } => Curve3::Line(w(g.pos(a)), w(g.pos(b))),
                    CurveKind::Circle { center, radius } => Curve3::Circle {
                        center: w(g.pos(center)),
                        normal,
                        radius,
                        u: src.u,
                    },
                    CurveKind::Arc { center, .. } => {
                        let a = g.arc_geom(curve)?;
                        Curve3::Arc {
                            center: w(g.pos(center)),
                            normal,
                            radius: a.radius,
                            start: w(a.start()),
                            mid: w(a.mid()),
                            end: w(a.end()),
                        }
                    }
                    CurveKind::Ellipse { center, major, minor } => Curve3::Ellipse {
                        center: w(g.pos(center)),
                        major: w(g.pos(major)),
                        minor,
                        normal,
                        offset: 0.0,
                    },
                    CurveKind::EllipseOffset { center, major, minor, distance } => Curve3::Ellipse {
                        center: w(g.pos(center)),
                        major: w(g.pos(major)),
                        minor,
                        normal,
                        offset: distance,
                    },
                    // Other curves as points along them.
                    CurveKind::Spline { .. } | CurveKind::Bezier { .. } | CurveKind::EllipseArc { .. } => {
                        let pts: Vec<Vec3> = cadrs_sketch::hit::curve_polyline(g, curve).into_iter().map(w).collect();
                        return sampled(&pts);
                    }
                })
            }
        }
    }

    /// What `link` refers to after a rebuild, as a link by current names: the same link if its
    /// names still exist; else the renamed entity (a split face's piece, a re-indexed edge)
    /// nearest where the linked geometry was, or, as a geometric fallback, the one entity whose
    /// projection still lies on it. `at` are points of the linked sketch geometry as the last
    /// regeneration left it (on a projected curve, or the pierced point); `pierce`: the link
    /// holds a pierced point. `None`: a lost reference.
    pub fn resolve(&self, link: Link, frame: &PlaneFrame, at: &[Vec2], pierce: bool) -> Option<Link> {
        match link {
            Link::Edge { feature, edge } => {
                let solid = self.solid_with_edge(feature, &edge)?;
                // How far the candidate's projection (or crossing) is from the linked geometry.
                let distance = |e: &crate::solid::SolidEdge| -> Option<f64> {
                    if at.is_empty() {
                        return None;
                    }
                    let c = edge_curve(e)?;
                    if pierce {
                        let hits = crossings(c, frame);
                        at.iter()
                            .map(|p| hits.iter().map(|h| h.distance(*p)).fold(f64::INFINITY, f64::min))
                            .reduce(f64::max)
                    } else {
                        let shape = project(c, frame)?;
                        at.iter().map(|p| projected_distance(&shape, *p)).reduce(|a, b| {
                            match (a, b) {
                                (Some(a), Some(b)) => Some(a.max(b)),
                                _ => None,
                            }
                        })?
                    }
                };
                let (i, _) = solid.resolve_edge(&edge, distance).ok()?;
                Some(Link::Edge {
                    feature,
                    edge: solid.edges[i].name,
                })
            }
            Link::Silhouette {
                feature,
                face,
                index,
            } => {
                let solid = self.solid_with_face(feature, &face)?;
                let (i, _) = solid.resolve_face(&face, None, None).ok()?;
                Some(Link::Silhouette {
                    feature,
                    face: solid.faces[i].name,
                    index,
                })
            }
            Link::Vertex { feature, vertex } => {
                let solid = self.solid_with_vertex(feature, &vertex)?;
                if solid.vertex(&vertex).is_some() {
                    return Some(link);
                }
                // Re-indexed: the vertex between the same faces nearest where the point was.
                let base = solid.canonical_vertex(&vertex).base();
                let near = *at.first()?;
                let d = |v: &crate::solid::SolidVertex| frame.to_sketch(v.point).distance(near);
                solid
                    .vertices
                    .iter()
                    .filter(|v| v.name.base() == base)
                    .min_by(|a, b| d(a).total_cmp(&d(b)))
                    .map(|v| Link::Vertex { feature, vertex: v.name })
            }
            Link::SketchCurve { .. } | Link::Plane(_) | Link::FlatLine { .. } => Some(link),
        }
    }

    /// Where the link puts a projected curve in the plane, or `None` if its source is gone
    /// (or cannot be projected).
    pub fn shape(&self, link: Link, frame: &PlaneFrame) -> Option<Projected> {
        project(self.curve(link, frame)?, frame)
    }

    /// Where the linked curve pierces the plane: the crossing nearest `near`.
    pub fn pierce(&self, link: Link, frame: &PlaneFrame, near: Vec2) -> Option<Vec2> {
        // A part vertex: where it is seen along the normal.
        if let Link::Vertex { feature, vertex } = link {
            return Some(frame.to_sketch(self.solid_with_vertex(feature, &vertex)?.vertex(&vertex)?.point));
        }
        crossings(self.curve(link, frame)?, frame)
            .into_iter()
            .min_by(|a, b| a.distance(near).total_cmp(&b.distance(near)))
    }
}

/// The distance from a point to a projected shape (`None` for shapes without a formula here).
pub fn projected_distance(shape: &Projected, p: Vec2) -> Option<f64> {
    Some(match *shape {
        Projected::Line(a, b) => cadrs_sketch::geom::dist_point_segment(p, a, b),
        Projected::Circle(c, r) => (p.distance(c) - r).abs(),
        Projected::Arc { center, start, end } => {
            let g = cadrs_sketch::ArcGeom::ccw(center, start, end);
            g.distance(p)
        }
        Projected::Point(q) => p.distance(q),
        Projected::Ellipse { .. } => return None,
        Projected::Spline { ref points, closed } => {
            let spans = cadrs_sketch::spline::spans(points, closed, None, None);
            cadrs_sketch::spline::nearest(&spans, p)?.2
        }
        Projected::EllipseArc { center, major, minor, start, end } => {
            cadrs_sketch::EllipseArcGeom::ccw(center, major, minor, start, end).distance(p)
        }
        Projected::EllipseOffset { center, major, minor, distance } => {
            cadrs_sketch::geom::EllipseGeom::new(center, major, minor).with_offset(distance).distance(p)
        }
    })
}

/// Points on a sketch curve (its ends, middle, or four points around a circle), for telling
/// where a projected curve lies.
pub fn curve_samples(g: &Sketch, c: CurveId) -> Vec<Vec2> {
    let Some(curve) = g.curves.get(c) else {
        return Vec::new();
    };
    match curve.kind {
        CurveKind::Line { a, b } => {
            let (a, b) = (g.pos(a), g.pos(b));
            vec![a, b, Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)]
        }
        CurveKind::Circle { center, radius } => {
            let c = g.pos(center);
            [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)]
                .iter()
                .map(|(x, y)| Vec2::new(c.x + radius * x, c.y + radius * y))
                .collect()
        }
        CurveKind::Arc { .. } => g
            .arc_geom(c)
            .map(|a| vec![a.start(), a.mid(), a.end()])
            .unwrap_or_default(),
        CurveKind::Ellipse { .. } => Vec::new(),
        CurveKind::EllipseArc { .. } => g
            .ellipse_arc_geom(c)
            .map(|e| vec![e.start(), e.mid(), e.end()])
            .unwrap_or_default(),
        CurveKind::EllipseOffset { .. } => g
            .ellipse_geom(c)
            .map(|e| (0..4).map(|k| e.point_at(k as f64 * std::f64::consts::FRAC_PI_2)).collect())
            .unwrap_or_default(),
        CurveKind::Spline { .. } => g.curve_points(c).iter().map(|p| g.pos(*p)).collect(),
        CurveKind::Bezier { .. } => g
            .bezier_geom(c)
            .map(|b| vec![b.point_at(0.0), b.point_at(0.5), b.point_at(1.0)])
            .unwrap_or_default(),
    }
}

/// The silhouette lines of a curved face seen along `dir`: where its surface turns from facing
/// one way to the other (from the face's rulings), in the order found.
pub fn silhouettes(solid: &Solid, face: &FaceName, dir: Vec3) -> Vec<(Vec3, Vec3)> {
    let Some(index) = solid.faces.iter().position(|f| f.name == *face) else {
        return Vec::new();
    };
    let rulings: Vec<_> = solid.rulings.iter().filter(|r| r.face == index).collect();
    let dir = unit(dir);
    let mut out: Vec<(Vec3, Vec3)> = Vec::new();
    let mut push = |a: Vec3, b: Vec3| {
        if !out.iter().any(|(p, _)| len(sub(*p, a)) < 1e-6) {
            out.push((a, b));
        }
    };
    const ZERO: f64 = 1e-9;
    let n = rulings.len();
    for i in 0..n {
        let a = rulings[i];
        let da = dot(a.normal, dir);
        if da.abs() < ZERO {
            push(a.start, a.end);
            continue;
        }
        // Between two rulings (a closed face repeats its first ruling at the end).
        let Some(b) = rulings.get(i + 1).filter(|b| b.run == a.run) else {
            continue;
        };
        let db = dot(b.normal, dir);
        if db.abs() >= ZERO && da.signum() != db.signum() {
            let t = da / (da - db);
            let lerp = |p: Vec3, q: Vec3| add(p, scale(sub(q, p), t));
            push(lerp(a.start, b.start), lerp(a.end, b.end));
        }
    }
    out
}

/// The edges of the part faces lying in the plane (either way round), as imprints (S21.1).
pub fn imprint(solids: &[(FeatureId, &Solid)], frame: &PlaneFrame) -> Vec<Imprint> {
    let n = unit(frame.normal());
    let mut out = Vec::new();
    for (feature, solid) in solids {
        for face in &solid.faces {
            let Some(p) = face.plane else { continue };
            let on_plane = dot(unit(p.normal()), n).abs() > 1.0 - PARALLEL
                && face
                    .loops
                    .first()
                    .and_then(|l| l.first())
                    .is_some_and(|q| frame.distance(*q).abs() < 1e-6);
            if !on_plane {
                continue;
            }
            for e in &solid.edges {
                if !e.name.touches(&face.name) {
                    continue;
                }
                let Some(shape) = edge_curve(e).and_then(|c| project(c, frame)) else {
                    continue;
                };
                let shape = match shape {
                    Projected::Line(a, b) => ImprintShape::Line(a, b),
                    Projected::Circle(c, r) => ImprintShape::Circle(c, r),
                    Projected::Arc { center, start, end } => {
                        let g = cadrs_sketch::ArcGeom::ccw(center, start, end);
                        ImprintShape::Arc {
                            center,
                            radius: g.radius,
                            start_angle: g.start_angle,
                            sweep: g.sweep,
                        }
                    }
                    _ => continue,
                };
                out.push(Imprint {
                    id: synthetic_curve(1, out.len() as u32),
                    shape,
                    link: Some(Link::Edge { feature: feature.0, edge: e.name }),
                });
            }
        }
    }
    out
}

/// The edges of a face (for Use on a whole face).
pub fn face_edges(solid: &Solid, face: &FaceName) -> Vec<EdgeName> {
    solid.face_edges(face)
}
