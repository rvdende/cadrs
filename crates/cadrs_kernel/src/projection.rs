//! Drawing-view projection (P3C.2): hidden-line removal of bodies seen along a view direction.
//!
//! [`crate::Kernel::project`] projects bodies orthographically onto a view plane and sorts the
//! projected edges into visible and hidden, and into sharp edges, smooth (tangent) edges and
//! outlines (silhouettes), like the lines of a drawing view. Everything here is plain data (no
//! backend types), so a projection can be cached, sent between threads and drawn anywhere.
//!
//! # 2D frame
//! A [`ViewFrame`] is an eye direction `dir` (from the eye into the scene) and a sheet-right
//! direction `x` (made perpendicular to `dir`). 2D points are in model units (mm) relative to
//! `origin`: `x` along [`ViewFrame::x`], `y` along [`ViewFrame::up`] = `x × dir`. A front view
//! (looking along +Y, `x` = +X) has 2D y = +Z; a top view (`dir` = −Z, `x` = +X) has 2D y = +Y.
//!
//! # Source topology
//! Each projected edge keeps where it came from ([`ProjSource`]): the index of the body in the
//! list passed to `project`, the body's [`EdgeId`] for sharp and smooth edges, and the
//! [`FaceId`] for the outline of a cylinder. Drawing annotations (P3C.3) attach to model
//! topology through these (the caller adds the persistent names, see [`ProjSource::edge_name`]).
//! The backend's HLR reports no identity, so [`attach_sources`] finds it geometrically: a
//! projected piece belongs to the 3D edge whose projection runs through it; where several 3D
//! edges project onto the same line (a box's front and back edges seen from the side), a
//! visible piece takes the one nearest the eye and a hidden piece the farthest.

use nalgebra::{Point2, Point3, Vector3};
use serde::{Deserialize, Serialize};

use crate::naming::{EdgeName, FaceName};
use crate::{EdgeId, FaceId, FaceInfo, SurfaceKind};

/// Where a view looks from.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewFrame {
    /// The model point at 2D (0, 0).
    pub origin: Point3<f64>,
    /// Unit direction of sight, from the eye into the scene.
    pub dir: Vector3<f64>,
    /// Unit direction of 2D +x (perpendicular to `dir`).
    pub x: Vector3<f64>,
}

impl ViewFrame {
    /// A frame looking along `dir` with 2D x along `x` (made perpendicular and unit length).
    pub fn new(dir: Vector3<f64>, x: Vector3<f64>) -> Self {
        let dir = dir.normalize();
        let x = (x - dir * x.dot(&dir)).normalize();
        Self {
            origin: Point3::origin(),
            dir,
            x,
        }
    }

    /// Unit direction of 2D +y: `x × dir`.
    pub fn up(&self) -> Vector3<f64> {
        self.x.cross(&self.dir)
    }

    /// A model point in the view's 2D frame.
    pub fn to_2d(&self, p: &Point3<f64>) -> Point2<f64> {
        let d = p - self.origin;
        Point2::new(d.dot(&self.x), d.dot(&self.up()))
    }

    /// How far a point is along the direction of sight (larger is farther from the eye).
    pub fn depth(&self, p: &Point3<f64>) -> f64 {
        (p - self.origin).dot(&self.dir)
    }

    /// A model direction in the view's 2D frame (not normalized).
    pub fn dir_2d(&self, v: &Vector3<f64>) -> nalgebra::Vector2<f64> {
        nalgebra::Vector2::new(v.dot(&self.x), v.dot(&self.up()))
    }
}

/// How to project.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectOptions {
    /// Chordal deviation of the edge polylines (mm).
    pub tolerance: f64,
    /// Compute hidden edges too.
    pub hidden: bool,
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            tolerance: 0.01,
            hidden: true,
        }
    }
}

/// Seen or hidden behind material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProjVisibility {
    Visible,
    Hidden,
}

/// What kind of line a projected edge is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProjClass {
    /// An edge where two faces meet at an angle.
    Sharp,
    /// An edge between tangent faces (a fillet's boundary): a drawing's tangent edge.
    Smooth,
    /// The silhouette of a curved face (the sides of a cylinder seen side-on).
    Outline,
}

/// The exact 2D curve of a projected edge, where it is a line or a circular arc.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ProjCurve {
    Line {
        start: Point2<f64>,
        end: Point2<f64>,
    },
    /// An arc from `start` through `mid` to `end` (a whole circle when `full`).
    Arc {
        center: Point2<f64>,
        radius: f64,
        start: Point2<f64>,
        mid: Point2<f64>,
        end: Point2<f64>,
        full: bool,
    },
    /// Anything else (an ellipse, a spline): use the polyline.
    Polyline,
}

/// The model topology a projected edge came from.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ProjSource {
    /// Index of the body in the list given to [`crate::Kernel::project`].
    pub body: usize,
    /// The 3D edge (sharp and smooth edges).
    pub edge: Option<EdgeId>,
    /// The face (outlines of cylinders).
    pub face: Option<FaceId>,
    /// Persistent names, filled in by the caller that knows the body's names (`cadrs_core`).
    pub edge_name: Option<EdgeName>,
    pub face_name: Option<FaceName>,
}

/// One projected edge (or a piece of one: HLR splits edges where they go behind something).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjEdge {
    pub visibility: ProjVisibility,
    pub class: ProjClass,
    pub curve: ProjCurve,
    /// The edge as a polyline within the tolerance (at least two points).
    pub points: Vec<Point2<f64>>,
    pub source: Option<ProjSource>,
}

impl ProjEdge {
    /// Length of the polyline (exact for lines).
    pub fn length(&self) -> f64 {
        self.points.windows(2).map(|w| (w[1] - w[0]).norm()).sum()
    }

    /// The point `t` (0..1) of the way along the polyline.
    pub fn point_at(&self, t: f64) -> Point2<f64> {
        let total = self.length();
        let mut left = total * t.clamp(0.0, 1.0);
        for w in self.points.windows(2) {
            let l = (w[1] - w[0]).norm();
            if left <= l && l > 0.0 {
                return w[0] + (w[1] - w[0]) * (left / l);
            }
            left -= l;
        }
        *self.points.last().unwrap_or(&Point2::origin())
    }
}

/// The projected edges of a view.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    pub edges: Vec<ProjEdge>,
}

impl Projection {
    /// 2D bounding box `(min, max)` of every edge, or `None` when there are none.
    pub fn bounds(&self) -> Option<(Point2<f64>, Point2<f64>)> {
        let mut it = self.edges.iter().flat_map(|e| e.points.iter());
        let first = *it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| {
            (
                Point2::new(lo.x.min(p.x), lo.y.min(p.y)),
                Point2::new(hi.x.max(p.x), hi.y.max(p.y)),
            )
        }))
    }

    /// The edges of one visibility and class.
    pub fn of(&self, visibility: ProjVisibility, class: ProjClass) -> impl Iterator<Item = &ProjEdge> {
        self.edges
            .iter()
            .filter(move |e| e.visibility == visibility && e.class == class)
    }
}

fn segment_distance(p: Point2<f64>, a: Point2<f64>, b: Point2<f64>) -> (f64, f64) {
    let ab = b - a;
    let len2 = ab.norm_squared();
    let t = if len2 > 0.0 {
        ((p - a).dot(&ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((p - (a + ab * t)).norm(), t)
}

/// A body's topology for [`attach_sources`]: its edges as 3D polylines, its faces and its seam
/// edges (an edge with the same face on both sides, such as a cylinder's) with their face.
pub struct SourceBody<'a> {
    pub edges: &'a [(EdgeId, Vec<Point3<f64>>)],
    pub faces: &'a [FaceInfo],
    pub seams: &'a [(EdgeId, FaceId)],
}

/// Whether the straight 2D segment `start..end` is an outline of the cylinder `f`: parallel to
/// its projected axis, the radius away from it, and along the face.
fn cylinder_outline(frame: &ViewFrame, f: &FaceInfo, start: Point2<f64>, end: Point2<f64>, tol: f64) -> bool {
    let (SurfaceKind::Cylinder, Some(axis), Some(r)) = (f.kind, f.axis, f.radius) else {
        return false;
    };
    let d = end - start;
    if d.norm() < 1e-9 {
        return false;
    }
    let dn = d.normalize();
    let a2 = frame.dir_2d(&axis.dir);
    if a2.norm() < 1e-6 {
        return false;
    }
    let a2 = a2.normalize();
    if (a2.x * dn.y - a2.y * dn.x).abs() > 1e-4 {
        return false;
    }
    let o = frame.to_2d(&axis.origin);
    let off = nalgebra::center(&start, &end) - o;
    let dist = (off.x * a2.y - off.y * a2.x).abs();
    if (dist - r).abs() > tol {
        return false;
    }
    let along = |p: Point2<f64>| (p - o).dot(&a2);
    let (s0, s1) = (along(start).min(along(end)), along(start).max(along(end)));
    let cc = along(frame.to_2d(&f.center));
    cc >= s0 - tol && cc <= s1 + tol
}

/// Fills in [`ProjEdge::source`] by geometry (see the module docs). `tol` is how far (mm) a
/// projected piece may be from a 3D edge's projection and still belong to it.
///
/// Seam edges aren't lines of a drawing: a piece that comes from a seam becomes the face's
/// outline where the seam happens to lie on the silhouette of a cylinder (OCCT then reports
/// the seam instead of the outline), and is dropped otherwise.
pub fn attach_sources(proj: &mut Projection, frame: &ViewFrame, bodies: &[SourceBody], tol: f64) {
    struct Cand {
        body: usize,
        edge: EdgeId,
        pts: Vec<(Point2<f64>, f64)>,
        min: Point2<f64>,
        max: Point2<f64>,
    }
    let mut cands: Vec<Cand> = Vec::new();
    for (bi, b) in bodies.iter().enumerate() {
        for (id, poly) in b.edges {
            if poly.len() < 2 {
                continue;
            }
            let pts: Vec<(Point2<f64>, f64)> = poly.iter().map(|p| (frame.to_2d(p), frame.depth(p))).collect();
            let mut min = pts[0].0;
            let mut max = pts[0].0;
            for (p, _) in &pts {
                min = Point2::new(min.x.min(p.x), min.y.min(p.y));
                max = Point2::new(max.x.max(p.x), max.y.max(p.y));
            }
            cands.push(Cand {
                body: bi,
                edge: *id,
                pts,
                min,
                max,
            });
        }
    }
    // The depth where `p` is within `tol` of a candidate's projection.
    let near = |c: &Cand, p: Point2<f64>| -> Option<f64> {
        if p.x < c.min.x - tol || p.y < c.min.y - tol || p.x > c.max.x + tol || p.y > c.max.y + tol {
            return None;
        }
        let mut best: Option<(f64, f64)> = None;
        for w in c.pts.windows(2) {
            let (d, t) = segment_distance(p, w[0].0, w[1].0);
            if d <= tol && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, w[0].1 + (w[1].1 - w[0].1) * t));
            }
        }
        best.map(|(_, depth)| depth)
    };
    let mut drop = vec![false; proj.edges.len()];
    for (ei, e) in proj.edges.iter_mut().enumerate() {
        match e.class {
            ProjClass::Sharp | ProjClass::Smooth => {
                let samples = [e.point_at(0.25), e.point_at(0.5), e.point_at(0.75)];
                let mut best: Option<(usize, f64)> = None;
                for (ci, c) in cands.iter().enumerate() {
                    let mut depth = 0.0;
                    let mut ok = true;
                    for (k, s) in samples.iter().enumerate() {
                        match near(c, *s) {
                            Some(d) => {
                                if k == 1 {
                                    depth = d;
                                }
                            }
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if !ok {
                        continue;
                    }
                    let better = match (best, e.visibility) {
                        (None, _) => true,
                        (Some((_, bd)), ProjVisibility::Visible) => depth < bd - 1e-9,
                        (Some((_, bd)), ProjVisibility::Hidden) => depth > bd + 1e-9,
                    };
                    if better {
                        best = Some((ci, depth));
                    }
                }
                let Some((ci, _)) = best else {
                    continue;
                };
                let c = &cands[ci];
                let seam = bodies[c.body].seams.iter().find(|(id, _)| *id == c.edge).map(|(_, f)| *f);
                match seam {
                    None => {
                        e.source = Some(ProjSource {
                            body: c.body,
                            edge: Some(c.edge),
                            ..Default::default()
                        });
                    }
                    Some(face) => {
                        let on_outline = match (e.curve, bodies[c.body].faces.iter().find(|f| f.id == face)) {
                            (ProjCurve::Line { start, end }, Some(f)) => cylinder_outline(frame, f, start, end, tol),
                            _ => false,
                        };
                        if on_outline {
                            e.class = ProjClass::Outline;
                            e.source = Some(ProjSource {
                                body: c.body,
                                face: Some(face),
                                ..Default::default()
                            });
                        } else {
                            drop[ei] = true;
                        }
                    }
                }
            }
            ProjClass::Outline => {
                let ProjCurve::Line { start, end } = e.curve else {
                    continue;
                };
                'bodies: for (bi, b) in bodies.iter().enumerate() {
                    for f in b.faces {
                        if cylinder_outline(frame, f, start, end, tol) {
                            e.source = Some(ProjSource {
                                body: bi,
                                face: Some(f.id),
                                ..Default::default()
                            });
                            break 'bodies;
                        }
                    }
                }
            }
        }
    }
    let mut i = 0;
    proj.edges.retain(|_| {
        i += 1;
        !drop[i - 1]
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_follow_the_hlr_convention() {
        // Front: looking along +Y with x = +X, 2D y is +Z.
        let f = ViewFrame::new(Vector3::y(), Vector3::x());
        assert!((f.up() - Vector3::z()).norm() < 1e-12);
        assert_eq!(f.to_2d(&Point3::new(3.0, 7.0, 5.0)), Point2::new(3.0, 5.0));
        // Top: looking down (-Z) with x = +X, 2D y is +Y.
        let t = ViewFrame::new(-Vector3::z(), Vector3::x());
        assert!((t.up() - Vector3::y()).norm() < 1e-12);
    }

    #[test]
    fn sources_prefer_the_near_edge_when_visible() {
        let f = ViewFrame::new(Vector3::y(), Vector3::x());
        // Two parallel edges at y = 0 (near) and y = 10 (far) project onto the same line.
        let edges = vec![
            (EdgeId(0), vec![Point3::new(0.0, 10.0, 0.0), Point3::new(10.0, 10.0, 0.0)]),
            (EdgeId(1), vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)]),
        ];
        let line = |v| ProjEdge {
            visibility: v,
            class: ProjClass::Sharp,
            curve: ProjCurve::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(10.0, 0.0),
            },
            points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            source: None,
        };
        let mut p = Projection {
            edges: vec![line(ProjVisibility::Visible), line(ProjVisibility::Hidden)],
        };
        attach_sources(&mut p, &f, &[SourceBody { edges: &edges, faces: &[], seams: &[] }], 0.01);
        assert_eq!(p.edges[0].source.unwrap().edge, Some(EdgeId(1)));
        assert_eq!(p.edges[1].source.unwrap().edge, Some(EdgeId(0)));
    }
}
