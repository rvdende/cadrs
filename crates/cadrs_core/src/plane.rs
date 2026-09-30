//! The Plane feature (P3.7, PS12.2–12.3): reference planes of eight types.
//!
//! A plane's frame ([`PlaneFrame`]: origin, in-plane `u` and `v`, normal `u × v`) is worked out
//! from the geometry its entities resolve to ([`RefGeom`]) by [`frame`]:
//!
//! | Type | Entities | Frame |
//! |---|---|---|
//! | Offset | a plane or planar face | the plane moved `offset` along its normal (against it when flipped) |
//! | Plane point | a plane and a point | the plane's axes, through the point |
//! | Line angle | a line, optionally a reference plane or point | through the line, turned `angle` about it from the plane through the line closest to the reference (by default the world Z) |
//! | Point normal | a line (or a circle's axis) and a point | through the point, square to the line |
//! | Three point | three points | through them: origin at the first, `u` towards the second |
//! | Mid plane | two planes, or two points | halfway between parallel planes; else the bisector (the other one with Flip alignment); square to the segment between two points at its middle |
//! | Curve point | a curve and a point on it (else its start) | through the point, square to the curve's tangent there |
//! | Fit | three or more points (vertices, sketch points, the points of curves) | the least-squares plane through them, at their centroid |
//!
//! Unless a type sets them, `u` is the world X projected onto the plane (Y for planes square to
//! X), so a plane parallel to a default plane gets its axes. **Flip normal** turns the plane
//! over (`v` reversed). Entities are resolved in the rebuild ([`crate::rebuild`]), so a plane
//! follows the faces, edges and sketches it is built on; sketches on it (`PlaneRef::Feature`)
//! follow it (see [`crate::parts::regenerate`]).

use cadrs_sketch::{CurveId, PlaneFrame, PlaneRef, PointId, Vec3};
use serde::{Deserialize, Serialize};

use crate::document::{EdgeRef, FaceRef, VertexRef};
use crate::ids::FeatureId;

/// The Plane dialog's type (PS12.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlaneType {
    #[default]
    Offset,
    PlanePoint,
    LineAngle,
    PointNormal,
    ThreePoint,
    MidPlane,
    CurvePoint,
    Fit,
    /// Tangent to a cylindrical or conical face: through a point (on it, or outside a
    /// cylinder), else at an angle round its axis.
    Tangent,
}

impl PlaneType {
    pub const ALL: [PlaneType; 9] = [
        PlaneType::Offset,
        PlaneType::PlanePoint,
        PlaneType::LineAngle,
        PlaneType::PointNormal,
        PlaneType::ThreePoint,
        PlaneType::MidPlane,
        PlaneType::CurvePoint,
        PlaneType::Fit,
        PlaneType::Tangent,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PlaneType::Offset => "Offset",
            PlaneType::PlanePoint => "Plane point",
            PlaneType::LineAngle => "Line angle",
            PlaneType::PointNormal => "Point normal",
            PlaneType::ThreePoint => "Three point",
            PlaneType::MidPlane => "Mid plane",
            PlaneType::CurvePoint => "Curve point",
            PlaneType::Fit => "Fit",
            PlaneType::Tangent => "Tangent",
        }
    }
}

/// Something a Plane feature is built on (its Entities field).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PlaneEntity {
    /// A default plane or another Plane feature.
    Plane(PlaneRef),
    /// A planar face of a part.
    Face(FaceRef),
    /// An edge of a part (a line, a circle's axis, a curve).
    Edge(EdgeRef),
    /// A vertex of a part.
    Vertex(VertexRef),
    /// A point of a sketch.
    SketchPoint { sketch: FeatureId, point: PointId },
    /// A curve of a sketch.
    SketchCurve { sketch: FeatureId, curve: CurveId },
    /// The origin.
    Origin,
}

impl PlaneEntity {
    /// The feature it comes from (for [`crate::Feature::parents`]).
    pub fn feature(&self) -> Option<FeatureId> {
        match self {
            PlaneEntity::Plane(PlaneRef::Feature(f)) => Some(FeatureId(f.feature)),
            PlaneEntity::Plane(_) | PlaneEntity::Origin => None,
            PlaneEntity::Face(f) => Some(FeatureId(f.face.op)),
            PlaneEntity::Edge(e) => Some(FeatureId(e.edge.op())),
            PlaneEntity::Vertex(v) => Some(v.part.feature),
            PlaneEntity::SketchPoint { sketch, .. } | PlaneEntity::SketchCurve { sketch, .. } => Some(*sketch),
        }
    }
}

/// A Plane feature's parameters (the Plane dialog: `ex4-step5.png`, `ex4-step6.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaneFeature {
    pub kind: PlaneType,
    pub entities: Vec<PlaneEntity>,
    /// Offset distance (mm).
    pub offset: f64,
    pub offset_expr: String,
    /// Line angle (degrees).
    pub angle: f64,
    pub angle_expr: String,
    /// The opposite direction (the offset or the angle).
    #[serde(default)]
    pub flip: bool,
    /// Flip normal: the plane faces the other way.
    #[serde(default)]
    pub flip_normal: bool,
    /// Flip alignment: a mid plane of two planes at an angle takes the other bisector.
    #[serde(default)]
    pub flip_alignment: bool,
}

impl Default for PlaneFeature {
    fn default() -> Self {
        Self {
            kind: PlaneType::Offset,
            entities: Vec::new(),
            offset: 25.0,
            offset_expr: "25 mm".into(),
            angle: 45.0,
            angle_expr: "45 deg".into(),
            flip: false,
            flip_normal: false,
            flip_alignment: false,
        }
    }
}

impl PlaneFeature {
    /// Why it can't be built from its entities (counting them by kind), if it can't.
    pub fn problem(&self) -> Option<&'static str> {
        let n = self.entities.len();
        match self.kind {
            _ if n == 0 => Some("Select entities for the plane"),
            PlaneType::Offset if n != 1 => Some("Select one plane or planar face"),
            PlaneType::PlanePoint if n != 2 => Some("Select a plane and a point"),
            PlaneType::LineAngle if n > 2 => Some("Select a line and a reference"),
            PlaneType::PointNormal if n != 2 => Some("Select a line and a point"),
            PlaneType::ThreePoint if n != 3 => Some("Select three points"),
            PlaneType::MidPlane if n != 2 => Some("Select two planes or two points"),
            PlaneType::CurvePoint if n > 2 => Some("Select a curve and a point"),
            PlaneType::Fit if n < 1 => Some("Select points to fit"),
            PlaneType::Tangent if n > 2 => Some("Select a cylindrical or conical face and a point"),
            _ => None,
        }
    }
}

/// What an entity is, geometrically.
#[derive(Debug, Clone, PartialEq)]
pub enum RefGeom {
    Plane(PlaneFrame),
    /// A straight line: a point on it and its unit direction (from its first point to its
    /// last).
    Line { point: Vec3, dir: Vec3 },
    /// A circle or arc: its axis is a line, and it is a curve (from `start`, counter-clockwise
    /// about `normal`).
    Circle { center: Vec3, normal: Vec3, radius: f64, start: Vec3 },
    /// Another curve, as points along it.
    Curve(Vec<Vec3>),
    /// A cylindrical or conical face: a point on its axis, the axis (unit), the radius there
    /// and how fast the radius grows along the axis (0 for a cylinder).
    Cone { point: Vec3, axis: Vec3, radius: f64, slope: f64 },
    Point(Vec3),
}

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
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: Vec3) -> Option<Vec3> {
    let l = norm(a);
    (l > 1e-12).then(|| scale(a, 1.0 / l))
}

/// The frame with normal `n` through `origin` whose `u` is the world X projected onto the plane
/// (Y when the plane is square to X).
pub fn frame_with_normal(origin: Vec3, n: Vec3) -> Option<PlaneFrame> {
    let n = unit(n)?;
    let x = [1.0, 0.0, 0.0];
    let axis = if dot(x, n).abs() > 0.99 { [0.0, 1.0, 0.0] } else { x };
    let u = unit(sub(axis, scale(n, dot(axis, n))))?;
    Some(PlaneFrame { origin, u, v: cross(n, u) })
}

/// The frame with normal `n` and in-plane `u` (made square to `n`).
fn frame_nu(origin: Vec3, n: Vec3, u: Vec3) -> Option<PlaneFrame> {
    let n = unit(n)?;
    let u = unit(sub(u, scale(n, dot(u, n))))?;
    Some(PlaneFrame { origin, u, v: cross(n, u) })
}

/// Turns `v` about the unit axis `k` by `a` radians (Rodrigues).
fn rotate(v: Vec3, k: Vec3, a: f64) -> Vec3 {
    let (s, c) = a.sin_cos();
    add(add(scale(v, c), scale(cross(k, v), s)), scale(k, dot(k, v) * (1.0 - c)))
}

/// The same plane facing the other way (its `v` reversed).
pub fn flipped(f: PlaneFrame) -> PlaneFrame {
    PlaneFrame { v: scale(f.v, -1.0), ..f }
}

/// A point an entity stands for (a vertex, a sketch point, a circle's centre).
fn point_of(g: &RefGeom) -> Option<Vec3> {
    match g {
        RefGeom::Point(p) => Some(*p),
        RefGeom::Circle { center, .. } => Some(*center),
        _ => None,
    }
}

/// A line an entity stands for (a straight edge or sketch line, a circle's axis).
fn line_of(g: &RefGeom) -> Option<(Vec3, Vec3)> {
    match g {
        RefGeom::Line { point, dir } => Some((*point, *dir)),
        RefGeom::Circle { center, normal, .. } => Some((*center, unit(*normal)?)),
        _ => None,
    }
}

/// The point of a curve nearest `p` (or its start) and the unit tangent there, in the
/// direction it runs.
fn curve_at(g: &RefGeom, p: Option<Vec3>) -> Option<(Vec3, Vec3)> {
    match g {
        RefGeom::Line { point, dir } => {
            let q = p.map_or(*point, |p| add(*point, scale(*dir, dot(sub(p, *point), *dir))));
            Some((q, *dir))
        }
        RefGeom::Circle { center, normal, radius, start } => {
            let n = unit(*normal)?;
            let target = p.unwrap_or(*start);
            let r = sub(target, *center);
            let r = unit(sub(r, scale(n, dot(r, n))))?;
            Some((add(*center, scale(r, *radius)), cross(n, r)))
        }
        RefGeom::Curve(pts) => {
            if pts.len() < 2 {
                return None;
            }
            // The segment nearest the point (the first without one).
            let (i, q) = match p {
                None => (0, pts[0]),
                Some(p) => (0..pts.len() - 1)
                    .map(|i| {
                        let (a, b) = (pts[i], pts[i + 1]);
                        let d = sub(b, a);
                        let t = (dot(sub(p, a), d) / dot(d, d).max(1e-300)).clamp(0.0, 1.0);
                        let q = add(a, scale(d, t));
                        (i, q, norm(sub(p, q)))
                    })
                    .min_by(|a, b| a.2.total_cmp(&b.2))
                    .map(|(i, q, _)| (i, q))?,
            };
            // The segment's direction, smoothed with its neighbours'.
            let (a, b) = (pts[i], pts[i + 1]);
            let d = sub(b, a);
            let prev = if i > 0 { sub(a, pts[i - 1]) } else { d };
            let next = if i + 2 < pts.len() { sub(pts[i + 2], b) } else { d };
            Some((q, unit(add(add(prev, next), scale(d, 2.0)))?))
        }
        _ => None,
    }
}

/// The points an entity stands for in a Fit (a point, or the points of a curve or a plane's
/// origin).
fn fit_points(g: &RefGeom) -> Vec<Vec3> {
    match g {
        RefGeom::Point(p) => vec![*p],
        RefGeom::Line { point, dir } => vec![*point, add(*point, *dir)],
        RefGeom::Circle { center, normal, radius, start } => {
            let Some(n) = unit(*normal) else { return vec![*center] };
            let r = sub(*start, *center);
            let Some(r) = unit(sub(r, scale(n, dot(r, n)))) else { return vec![*center] };
            let s = cross(n, r);
            (0..8)
                .map(|k| {
                    let t = k as f64 * std::f64::consts::TAU / 8.0;
                    add(*center, add(scale(r, radius * t.cos()), scale(s, radius * t.sin())))
                })
                .collect()
        }
        RefGeom::Curve(pts) => pts.clone(),
        RefGeom::Plane(f) => vec![f.origin, add(f.origin, f.u), add(f.origin, f.v)],
        RefGeom::Cone { point, axis, .. } => vec![*point, add(*point, *axis)],
    }
}

/// The plane a Plane feature makes from its entities' geometry (see the module docs).
pub fn frame(p: &PlaneFeature, geoms: &[RefGeom]) -> Result<PlaneFrame, String> {
    let bad = |why: &str| Err(why.to_string());
    let planes: Vec<PlaneFrame> = geoms
        .iter()
        .filter_map(|g| match g {
            RefGeom::Plane(f) => Some(*f),
            _ => None,
        })
        .collect();
    let points: Vec<Vec3> = geoms.iter().filter_map(point_of).collect();
    let sign = if p.flip { -1.0 } else { 1.0 };
    let f = match p.kind {
        PlaneType::Offset => {
            let [base] = planes[..] else { return bad("Select one plane or planar face") };
            let n = unit(base.normal()).ok_or("The plane has no normal")?;
            PlaneFrame { origin: add(base.origin, scale(n, sign * p.offset)), ..base }
        }
        PlaneType::PlanePoint => {
            let (Some(base), Some(q)) = (planes.first(), geoms.iter().find_map(|g| match g {
                RefGeom::Point(q) => Some(*q),
                _ => None,
            })) else {
                return bad("Select a plane and a point");
            };
            PlaneFrame { origin: q, ..*base }
        }
        PlaneType::LineAngle => {
            let Some((q, d)) = geoms.iter().find_map(line_of) else { return bad("Select a line") };
            // The plane through the line nearest the reference: its normal is the reference's
            // component square to the line.
            let reference = geoms.iter().filter(|g| line_of(g).is_none_or(|l| l != (q, d))).find_map(|g| match g {
                RefGeom::Plane(f) => Some(f.normal()),
                RefGeom::Point(r) => Some(cross(d, sub(*r, q))),
                _ => None,
            });
            let square = |n: Vec3| unit(sub(n, scale(d, dot(n, d))));
            let n0 = reference
                .and_then(square)
                .or_else(|| square([0.0, 0.0, 1.0]))
                .or_else(|| square([1.0, 0.0, 0.0]))
                .ok_or("The line has no direction")?;
            let n = rotate(n0, d, sign * p.angle.to_radians());
            frame_nu(q, n, d).ok_or("The line has no direction")?
        }
        PlaneType::PointNormal => {
            let Some((_, d)) = geoms.iter().find_map(line_of) else { return bad("Select a line") };
            let q = geoms
                .iter()
                .find_map(|g| match g {
                    RefGeom::Point(q) => Some(*q),
                    _ => None,
                })
                .ok_or("Select a point")?;
            frame_with_normal(q, d).ok_or("The line has no direction")?
        }
        PlaneType::ThreePoint => {
            let [a, b, c] = points[..] else { return bad("Select three points") };
            let n = unit(cross(sub(b, a), sub(c, a))).ok_or("The three points lie on one line")?;
            frame_nu(a, n, sub(b, a)).ok_or("The points coincide")?
        }
        PlaneType::MidPlane => match (&planes[..], &points[..]) {
            ([a, b], _) => {
                let (n1, n2) = (unit(a.normal()).ok_or("bad plane")?, unit(b.normal()).ok_or("bad plane")?);
                if norm(cross(n1, n2)) < 1e-9 {
                    // Parallel: halfway along the first's normal.
                    let h = dot(sub(b.origin, a.origin), n1);
                    PlaneFrame { origin: add(a.origin, scale(n1, h / 2.0)), ..*a }
                } else {
                    // The bisector (the other one with Flip alignment), through the line where
                    // they meet (at its point nearest the first plane's origin).
                    let n = if p.flip_alignment { sub(n1, n2) } else { add(n1, n2) };
                    let line = unit(cross(n1, n2)).ok_or("bad planes")?;
                    // A point on both planes: solve n1·x = d1, n2·x = d2 in span(n1, n2) from
                    // the first plane's origin.
                    let (d1, d2) = (dot(n1, a.origin), dot(n2, b.origin));
                    let c = dot(n1, n2);
                    let det = 1.0 - c * c;
                    let base = sub(a.origin, scale(line, dot(a.origin, line)));
                    let (e1, e2) = (d1 - dot(n1, base), d2 - dot(n2, base));
                    let (k1, k2) = ((e1 - c * e2) / det, (e2 - c * e1) / det);
                    let on = add(add(base, scale(n1, k1)), scale(n2, k2));
                    let origin = add(on, scale(line, dot(sub(a.origin, on), line)));
                    frame_nu(origin, n, line).ok_or("bad planes")?
                }
            }
            (_, [a, b]) => {
                let n = sub(*b, *a);
                frame_with_normal(scale(add(*a, *b), 0.5), n).ok_or("The points coincide")?
            }
            _ => return bad("Select two planes or two points"),
        },
        PlaneType::CurvePoint => {
            let curve = geoms
                .iter()
                .find(|g| matches!(g, RefGeom::Line { .. } | RefGeom::Circle { .. } | RefGeom::Curve(_)))
                .ok_or("Select a curve")?;
            let q = geoms.iter().find_map(|g| match g {
                RefGeom::Point(q) => Some(*q),
                _ => None,
            });
            let (at, t) = curve_at(curve, q).ok_or("The curve has no tangent there")?;
            frame_with_normal(at, t).ok_or("The curve has no tangent there")?
        }
        PlaneType::Fit => {
            let pts: Vec<Vec3> = geoms.iter().flat_map(fit_points).collect();
            fit(&pts).ok_or("Select at least three points that don't lie on one line")?
        }
        PlaneType::Tangent => {
            let Some((c, a, r0, k)) = geoms.iter().find_map(|g| match g {
                RefGeom::Cone { point, axis, radius, slope } => Some((*point, unit(*axis)?, *radius, *slope)),
                _ => None,
            }) else {
                return bad("Select a cylindrical or conical face");
            };
            // A face's axis may point either way: angles turn counter-clockwise about it pointing
            // up (+Z, else +Y, else +X), whichever way the kernel gives it.
            let key = if a[2].abs() > 1e-9 { a[2] } else if a[1].abs() > 1e-9 { a[1] } else { a[0] };
            let (a, k) = if key < 0.0 { (scale(a, -1.0), -k) } else { (a, k) };
            // Angles round the axis from the world X projected square to it (Y when the axis
            // is along X).
            let x = [1.0, 0.0, 0.0];
            let base = if dot(x, a).abs() > 0.99 { [0.0, 1.0, 0.0] } else { x };
            let e1 = unit(sub(base, scale(a, dot(base, a)))).ok_or("The face has no axis")?;
            let e2 = cross(a, e1);
            let (phi, h) = match points.first() {
                Some(q) => {
                    let d = sub(*q, c);
                    let h = dot(d, a);
                    let radial = sub(d, scale(a, h));
                    let dist = norm(radial);
                    let r = r0 + k * h;
                    let phi_q = dot(radial, e2).atan2(dot(radial, e1));
                    // Outside a cylinder: the plane through the point touching it (two of them;
                    // Flip alignment takes the other).
                    let turn = if k.abs() < 1e-12 && dist > r * (1.0 + 1e-9) { (r / dist).acos() } else { 0.0 };
                    (phi_q + if p.flip_alignment { -turn } else { turn }, h)
                }
                None => (sign * p.angle.to_radians(), 0.0),
            };
            let e = add(scale(e1, phi.cos()), scale(e2, phi.sin()));
            let at = add(add(c, scale(a, h)), scale(e, r0 + k * h));
            // The surface's normal there (r = r0 + k·h grows along the axis) and its ruling.
            frame_nu(at, sub(e, scale(a, k)), add(a, scale(e, k))).ok_or("The face has no tangent plane there")?
        }
    };
    Ok(if p.flip_normal { flipped(f) } else { f })
}

/// The least-squares plane through points (at their centroid, normal along the smallest
/// principal axis, turned towards +Z (else +Y, +X); `u` along the largest one).
pub fn fit(pts: &[Vec3]) -> Option<PlaneFrame> {
    if pts.len() < 3 {
        return None;
    }
    let n = pts.len() as f64;
    let c = scale(pts.iter().fold([0.0; 3], |a, p| add(a, *p)), 1.0 / n);
    let mut m = nalgebra::Matrix3::<f64>::zeros();
    for p in pts {
        let d = nalgebra::Vector3::from(sub(*p, c));
        m += d * d.transpose();
    }
    let e = m.symmetric_eigen();
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|a, b| e.eigenvalues[*a].total_cmp(&e.eigenvalues[*b]));
    // Collinear points: the two smallest spreads are both zero.
    if e.eigenvalues[idx[1]] <= 1e-18 * e.eigenvalues[idx[2]].max(1e-300) {
        return None;
    }
    let col = |i: usize| -> Vec3 { [e.eigenvectors[(0, i)], e.eigenvectors[(1, i)], e.eigenvectors[(2, i)]] };
    let mut normal = col(idx[0]);
    let key = if normal[2].abs() > 1e-9 { normal[2] } else if normal[1].abs() > 1e-9 { normal[1] } else { normal[0] };
    if key < 0.0 {
        normal = scale(normal, -1.0);
    }
    frame_nu(c, normal, col(idx[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close3(a: Vec3, b: Vec3) {
        assert!(norm(sub(a, b)) < 1e-9, "{a:?} != {b:?}");
    }

    fn same(f: PlaneFrame, g: PlaneFrame) {
        close3(f.origin, g.origin);
        close3(f.u, g.u);
        close3(f.v, g.v);
    }

    fn feature(kind: PlaneType) -> PlaneFeature {
        PlaneFeature { kind, ..PlaneFeature::default() }
    }

    fn top() -> PlaneFrame {
        PlaneRef::Top.frame()
    }

    /// PS12.2, each type against a frame built by hand.
    #[test]
    fn eight_plane_types() {
        let s2 = std::f64::consts::FRAC_1_SQRT_2;
        // Offset: Top 76.2 down (the Funnel's Lower Plane, 3 in towards −Z): origin (0, 0, −76.2),
        // Top's axes. Flip normal turns it over.
        let mut p = PlaneFeature { offset: 76.2, flip: true, ..feature(PlaneType::Offset) };
        let lower = frame(&p, &[RefGeom::Plane(top())]).unwrap();
        same(lower, PlaneFrame { origin: [0.0, 0.0, -76.2], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] });
        p.flip_normal = true;
        same(frame(&p, &[RefGeom::Plane(top())]).unwrap(), PlaneFrame { origin: [0.0, 0.0, -76.2], u: [1.0, 0.0, 0.0], v: [0.0, -1.0, 0.0] });
        // Offset from Front (normal −Y) 10: y = −10.
        let p = PlaneFeature { offset: 10.0, ..feature(PlaneType::Offset) };
        same(frame(&p, &[RefGeom::Plane(PlaneRef::Front.frame())]).unwrap(), PlaneFrame { origin: [0.0, -10.0, 0.0], u: [1.0, 0.0, 0.0], v: [0.0, 0.0, 1.0] });
        // Plane point: Right's axes through (5, 6, 7).
        let p = feature(PlaneType::PlanePoint);
        same(
            frame(&p, &[RefGeom::Plane(PlaneRef::Right.frame()), RefGeom::Point([5.0, 6.0, 7.0])]).unwrap(),
            PlaneFrame { origin: [5.0, 6.0, 7.0], u: [0.0, 1.0, 0.0], v: [0.0, 0.0, 1.0] },
        );
        // Line angle: the X axis line, 45° from Top: normal Z turned 45° about X, (0, −s2, s2);
        // u along the line, v = n × u = (0, s2, s2).
        let p = PlaneFeature { angle: 45.0, ..feature(PlaneType::LineAngle) };
        let x_line = RefGeom::Line { point: [0.0, 0.0, 0.0], dir: [1.0, 0.0, 0.0] };
        same(frame(&p, &[x_line.clone(), RefGeom::Plane(top())]).unwrap(), PlaneFrame { origin: [0.0; 3], u: [1.0, 0.0, 0.0], v: [0.0, s2, s2] });
        // 0°: Top itself; flipped 45°: the other way, v = (0, s2, −s2).
        let zero = PlaneFeature { angle: 0.0, ..p.clone() };
        same(frame(&zero, &[x_line.clone(), RefGeom::Plane(top())]).unwrap(), top());
        let back = PlaneFeature { flip: true, ..p.clone() };
        same(frame(&back, &[x_line.clone(), RefGeom::Plane(top())]).unwrap(), PlaneFrame { origin: [0.0; 3], u: [1.0, 0.0, 0.0], v: [0.0, s2, -s2] });
        // Point normal: square to a line along Z through (1, 2, 3): Top's axes there.
        let p = feature(PlaneType::PointNormal);
        let z_line = RefGeom::Line { point: [9.0, 9.0, 0.0], dir: [0.0, 0.0, 1.0] };
        same(frame(&p, &[z_line, RefGeom::Point([1.0, 2.0, 3.0])]).unwrap(), PlaneFrame { origin: [1.0, 2.0, 3.0], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] });
        // Three point: (0,0,0), (10,0,0), (0,0,10): the XZ plane facing −Y (Front's frame).
        let p = feature(PlaneType::ThreePoint);
        let pts = [RefGeom::Point([0.0; 3]), RefGeom::Point([10.0, 0.0, 0.0]), RefGeom::Point([0.0, 0.0, 10.0])];
        same(frame(&p, &pts).unwrap(), PlaneRef::Front.frame());
        let line3 = [RefGeom::Point([0.0; 3]), RefGeom::Point([1.0, 0.0, 0.0]), RefGeom::Point([2.0, 0.0, 0.0])];
        assert!(frame(&p, &line3).is_err());
        // Mid plane between Top and the Lower Plane: z = −38.1 (the Funnel's Middle Plane).
        let p = feature(PlaneType::MidPlane);
        same(frame(&p, &[RefGeom::Plane(top()), RefGeom::Plane(lower)]).unwrap(), PlaneFrame { origin: [0.0, 0.0, -38.1], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] });
        // Between Top (n = Z) and Right (n = X), which meet along the Y axis: the bisector with
        // normal (X + Z)/√2 through the origin (u along Y, their common line: v = n × u);
        // Flip alignment the other one, normal (Z − X)/√2... (n1 − n2 = Z − X).
        let mid = frame(&p, &[RefGeom::Plane(top()), RefGeom::Plane(PlaneRef::Right.frame())]).unwrap();
        close3(mid.origin, [0.0; 3]);
        close3(mid.normal(), [s2, 0.0, s2]);
        let alt = PlaneFeature { flip_alignment: true, ..p.clone() };
        close3(frame(&alt, &[RefGeom::Plane(top()), RefGeom::Plane(PlaneRef::Right.frame())]).unwrap().normal(), [-s2, 0.0, s2]);
        // Between two points: square to the segment at its middle.
        let two = frame(&p, &[RefGeom::Point([0.0, 0.0, 0.0]), RefGeom::Point([0.0, 0.0, 8.0])]).unwrap();
        same(two, PlaneFrame { origin: [0.0, 0.0, 4.0], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] });
        // Curve point: on a circle r 10 about Z (from (10, 0, 0)) at (0, 10, 0): the tangent
        // there is −X, so the plane is square to X facing −X: u = Y projected = (0, 1, 0),
        // v = n × u = (−1, 0, 0) × (0, 1, 0) = (0, 0, −1).
        let p = feature(PlaneType::CurvePoint);
        let circle = RefGeom::Circle { center: [0.0; 3], normal: [0.0, 0.0, 1.0], radius: 10.0, start: [10.0, 0.0, 0.0] };
        same(frame(&p, &[circle.clone(), RefGeom::Point([0.0, 10.0, 0.0])]).unwrap(), PlaneFrame { origin: [0.0, 10.0, 0.0], u: [0.0, 1.0, 0.0], v: [0.0, 0.0, -1.0] });
        // Without a point: at the circle's start (10, 0, 0), tangent +Y (Front turned over).
        same(frame(&p, &[circle]).unwrap(), PlaneFrame { origin: [10.0, 0.0, 0.0], u: [1.0, 0.0, 0.0], v: [0.0, 0.0, -1.0] });
        // Fit: four points of the plane z = 2 x (normal (−2, 0, 1)/√5, towards +Z), centroid
        // (1, 1, 2); the largest spread is along (1, 0, 2)/√5 (x from 0 to 2 is 2√5 long, y 2).
        let p = feature(PlaneType::Fit);
        let pts = [[0.0, 0.0, 0.0], [2.0, 0.0, 4.0], [2.0, 2.0, 4.0], [0.0, 2.0, 0.0]].map(RefGeom::Point);
        let f = frame(&p, &pts).unwrap();
        let r5 = 5f64.sqrt();
        close3(f.origin, [1.0, 1.0, 2.0]);
        close3(f.normal(), [-2.0 / r5, 0.0, 1.0 / r5]);
        assert!((dot(f.u, [1.0 / r5, 0.0, 2.0 / r5]).abs() - 1.0).abs() < 1e-9);
    }

    /// The Tangent type on a cylinder r 10 about Z and a cone narrowing up it.
    #[test]
    fn tangent_planes() {
        let cyl = RefGeom::Cone { point: [0.0; 3], axis: [0.0, 0.0, 1.0], radius: 10.0, slope: 0.0 };
        // At angle 0 (from X): touching at (10, 0, 0), facing X, u along the ruling Z, v = X × Z.
        let p = PlaneFeature { angle: 0.0, ..feature(PlaneType::Tangent) };
        same(frame(&p, std::slice::from_ref(&cyl)).unwrap(), PlaneFrame { origin: [10.0, 0.0, 0.0], u: [0.0, 0.0, 1.0], v: [0.0, -1.0, 0.0] });
        // At 90°: touching at (0, 10, 0), facing Y, v = Y × Z = X.
        let p = PlaneFeature { angle: 90.0, ..feature(PlaneType::Tangent) };
        same(frame(&p, std::slice::from_ref(&cyl)).unwrap(), PlaneFrame { origin: [0.0, 10.0, 0.0], u: [0.0, 0.0, 1.0], v: [1.0, 0.0, 0.0] });
        // Through (20, 0, 5), outside: it touches where cos φ = r / d = 1/2, φ = 60°, at
        // (5, 5√3, 5); the plane holds the point: (15, −5√3, 0) · (1/2, √3/2, 0) = 0. Flip
        // alignment: the other tangent, φ = −60°.
        let p = feature(PlaneType::Tangent);
        let h3 = 3f64.sqrt() / 2.0;
        let f = frame(&p, &[cyl.clone(), RefGeom::Point([20.0, 0.0, 5.0])]).unwrap();
        close3(f.origin, [5.0, 10.0 * h3, 5.0]);
        close3(f.normal(), [0.5, h3, 0.0]);
        let alt = PlaneFeature { flip_alignment: true, ..feature(PlaneType::Tangent) };
        close3(frame(&alt, &[cyl, RefGeom::Point([20.0, 0.0, 5.0])]).unwrap().normal(), [0.5, -h3, 0.0]);
        // A cone r = 10 − h/2 (apex at (0, 0, 20)): at angle 0 it touches (10, 0, 0), normal
        // (X + Z/2)/|…| = (2, 0, 1)/√5, and the plane holds the apex: (−10, 0, 20) · (2, 0, 1) = 0.
        let cone = RefGeom::Cone { point: [0.0; 3], axis: [0.0, 0.0, 1.0], radius: 10.0, slope: -0.5 };
        let r5 = 5f64.sqrt();
        let f = frame(&PlaneFeature { angle: 0.0, ..feature(PlaneType::Tangent) }, &[cone]).unwrap();
        close3(f.origin, [10.0, 0.0, 0.0]);
        close3(f.normal(), [2.0 / r5, 0.0, 1.0 / r5]);
        assert!(dot(sub([0.0, 0.0, 20.0], f.origin), f.normal()).abs() < 1e-9);
    }
}
