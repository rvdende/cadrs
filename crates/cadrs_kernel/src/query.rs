//! Types of the geometric queries of [`crate::Kernel`]: silhouette curves
//! ([`Kernel::silhouettes`](crate::Kernel::silhouettes)), minimum and maximum distances
//! ([`Kernel::min_distance`](crate::Kernel::min_distance),
//! [`Kernel::max_distance`](crate::Kernel::max_distance)) and faces made from a surface
//! ([`Kernel::face_from_surface`](crate::Kernel::face_from_surface)).

use nalgebra::{Point2, Point3, Vector3};

use crate::{BodyId, EdgeId, FaceId, VertexId};

/// How a body is seen for its silhouettes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SilhouetteView {
    /// Along a direction (a drawing view): where n·dir = 0.
    Parallel(Vector3<f64>),
    /// From an eye point (a perspective view): where n·(p − eye) = 0.
    Perspective(Point3<f64>),
}

/// One silhouette curve of a face, in model space.
#[derive(Debug, Clone, PartialEq)]
pub struct SilhouetteCurve {
    /// The face it lies on.
    pub face: FaceId,
    /// Points along it, its chords within the tolerance asked for.
    pub points: Vec<Point3<f64>>,
    /// The points' (u, v) on the face's surface.
    pub uv: Vec<Point2<f64>>,
    /// It ends where it starts (a whole circle, a loop round a torus).
    pub closed: bool,
}

impl SilhouetteCurve {
    /// The polyline's length.
    pub fn length(&self) -> f64 {
        self.points.windows(2).map(|w| (w[1] - w[0]).norm()).sum()
    }
}

/// One side of a distance query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistanceTarget {
    /// A point in space.
    Point(Point3<f64>),
    /// An edge of a body (with its vertices).
    Edge(BodyId, EdgeId),
    /// A face of a body, trimmed by its loops (with its edges and vertices).
    Face(BodyId, FaceId),
    /// Every face, edge and vertex of a body (its boundary: a point inside a solid is not at
    /// distance 0).
    Body(BodyId),
    /// A solid body with its inside: what it holds is at distance 0 from it, and a point inside
    /// is its own nearest point. Its boundary otherwise, as [`DistanceTarget::Body`]; a farthest
    /// point is on the boundary.
    Solid(BodyId),
}

/// Where a closest (or farthest) point lies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistanceLocation {
    /// The query's point.
    Point,
    /// A vertex.
    Vertex(VertexId),
    /// An edge at its curve's parameter.
    Edge(EdgeId, f64),
    /// A face at (u, v) of its surface.
    Face(FaceId, f64, f64),
}

/// A minimum or maximum distance and where it is reached on each side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistanceResult {
    /// The distance (mm).
    pub distance: f64,
    /// The point on the first side.
    pub a: Point3<f64>,
    /// The point on the second side.
    pub b: Point3<f64>,
    /// Where `a` lies.
    pub a_at: DistanceLocation,
    /// Where `b` lies.
    pub b_at: DistanceLocation,
}

/// How two solids meet ([`crate::Kernel::interference`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interference {
    /// The volume they share (mm³): 0 for solids that only touch or are apart.
    pub volume: f64,
    /// The least distance from a point of one to a point of the other, insides included (mm):
    /// 0 for solids that touch or overlap.
    pub gap: f64,
}

/// A rectangle of a surface's (u, v): `[[u0, u1], [v0, v1]]`.
pub type UvRect = [[f64; 2]; 2];
