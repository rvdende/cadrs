//! Kernel-independent geometry and result types.

use nalgebra::{Isometry3, Matrix3, Point2, Point3, Unit, Vector3};
use serde::{Deserialize, Serialize};

/// A body (solid) owned by a kernel session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BodyId(pub u64);

/// A face name that stays valid across rebuilds (see `naming`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FaceId(pub u64);

/// An edge name that stays valid across rebuilds (see `naming`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EdgeId(pub u64);

/// A sketch plane: an origin and orthonormal in-plane axes, in model coordinates (mm).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub origin: Point3<f64>,
    pub x_dir: Unit<Vector3<f64>>,
    pub normal: Unit<Vector3<f64>>,
}

impl Plane {
    /// The Top plane (XY, normal +Z).
    pub fn top() -> Self {
        Self {
            origin: Point3::origin(),
            x_dir: Vector3::x_axis(),
            normal: Vector3::z_axis(),
        }
    }

    pub fn y_dir(&self) -> Unit<Vector3<f64>> {
        Unit::new_normalize(self.normal.cross(&self.x_dir))
    }

    /// Maps a point in plane coordinates to model coordinates.
    pub fn to_model(&self, p: Point2<f64>) -> Point3<f64> {
        self.origin + self.x_dir.into_inner() * p.x + self.y_dir().into_inner() * p.y
    }
}

/// An exact boundary curve in sketch-plane coordinates.
///
/// `source` carries the sketch curve id the segment came from, so side faces generated from it
/// can be named after it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Curve2 {
    Line { a: Point2<f64>, b: Point2<f64>, source: Option<u64> },
    /// An arc from `start_angle` sweeping `sweep` radians: counter-clockwise when `sweep` is
    /// positive, clockwise when it is negative.
    Arc { center: Point2<f64>, radius: f64, start_angle: f64, sweep: f64, source: Option<u64> },
    Circle { center: Point2<f64>, radius: f64, source: Option<u64> },
    Ellipse {
        center: Point2<f64>,
        major_radius: f64,
        minor_radius: f64,
        /// Angle of the major axis from the plane's x axis.
        rotation: f64,
        source: Option<u64>,
    },
    /// Part of an ellipse: the points `center + R(rotation)·(major·cos t, minor·sin t)` for `t`
    /// from `start` through `sweep` (counter-clockwise when positive and `minor_radius` is
    /// positive). `minor_radius` may be larger than `major_radius`, or negative.
    EllipseArc {
        center: Point2<f64>,
        major_radius: f64,
        minor_radius: f64,
        rotation: f64,
        start: f64,
        sweep: f64,
        source: Option<u64>,
    },
    /// Part of the curve `offset` from an ellipse (P3.7, X13: an offset of an ellipse isn't an
    /// ellipse): the points `e(t) + offset·n(t)`, where `e(t)` is the [`Curve2::EllipseArc`]
    /// with the same parameters and `n(t)` its unit normal pointing away from the centre, for
    /// `t` from `start` through `sweep` (a whole turn for the closed curve). Positive offsets
    /// lie outside the ellipse.
    OffsetEllipseArc {
        center: Point2<f64>,
        major_radius: f64,
        minor_radius: f64,
        rotation: f64,
        start: f64,
        sweep: f64,
        offset: f64,
        source: Option<u64>,
    },
    /// A cubic Bézier curve (Final, S12.14: a sketch's Bézier curve): from `poles[0]` to
    /// `poles[3]`, with control points `poles[1]` and `poles[2]`.
    Bezier { poles: [Point2<f64>; 4], source: Option<u64> },
}

/// The point at `t` of the cubic Bézier curve with these poles.
pub fn bezier_point(p: &[Point2<f64>; 4], t: f64) -> Point2<f64> {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Point2::new(
        a * p[0].x + b * p[1].x + c * p[2].x + d * p[3].x,
        a * p[0].y + b * p[1].y + c * p[2].y + d * p[3].y,
    )
}

impl Curve2 {
    /// The point where the curve starts (for a circle or a whole ellipse, the point at angle 0).
    pub fn start(&self) -> Point2<f64> {
        self.point_at(0.0)
    }

    /// The point where the curve ends (a circle or a whole ellipse ends where it starts).
    pub fn end(&self) -> Point2<f64> {
        self.point_at(1.0)
    }

    /// The point at `s` in 0..=1 along the curve.
    pub fn point_at(&self, s: f64) -> Point2<f64> {
        match *self {
            Curve2::Line { a, b, .. } => a + (b - a) * s,
            Curve2::Arc { center, radius, start_angle, sweep, .. } => {
                let t = start_angle + sweep * s;
                Point2::new(center.x + radius * t.cos(), center.y + radius * t.sin())
            }
            Curve2::Circle { center, radius, .. } => {
                let t = std::f64::consts::TAU * s;
                Point2::new(center.x + radius * t.cos(), center.y + radius * t.sin())
            }
            Curve2::Ellipse { center, major_radius, minor_radius, rotation, .. } => ellipse_point(
                center,
                major_radius,
                minor_radius,
                rotation,
                std::f64::consts::TAU * s,
            ),
            Curve2::EllipseArc { center, major_radius, minor_radius, rotation, start, sweep, .. } => {
                ellipse_point(center, major_radius, minor_radius, rotation, start + sweep * s)
            }
            Curve2::OffsetEllipseArc { center, major_radius, minor_radius, rotation, start, sweep, offset, .. } => {
                offset_ellipse_point(center, major_radius, minor_radius, rotation, offset, start + sweep * s)
            }
            Curve2::Bezier { ref poles, .. } => bezier_point(poles, s),
        }
    }

    /// The sketch curve this segment came from.
    pub fn source(&self) -> Option<u64> {
        match *self {
            Curve2::Line { source, .. }
            | Curve2::Arc { source, .. }
            | Curve2::Circle { source, .. }
            | Curve2::Ellipse { source, .. }
            | Curve2::EllipseArc { source, .. }
            | Curve2::OffsetEllipseArc { source, .. }
            | Curve2::Bezier { source, .. } => source,
        }
    }

    /// True for a curve that closes on itself (a circle, a whole ellipse or a whole offset
    /// ellipse).
    pub fn is_closed(&self) -> bool {
        match *self {
            Curve2::Circle { .. } | Curve2::Ellipse { .. } => true,
            Curve2::Arc { sweep, .. } | Curve2::EllipseArc { sweep, .. } | Curve2::OffsetEllipseArc { sweep, .. } => {
                sweep.abs() >= std::f64::consts::TAU - 1e-9
            }
            Curve2::Line { .. } | Curve2::Bezier { .. } => false,
        }
    }
}

/// `center + R(rotation)·(major·cos t, minor·sin t)`.
pub fn ellipse_point(center: Point2<f64>, major: f64, minor: f64, rotation: f64, t: f64) -> Point2<f64> {
    let (x, y) = (major * t.cos(), minor * t.sin());
    let (s, c) = rotation.sin_cos();
    Point2::new(center.x + c * x - s * y, center.y + s * x + c * y)
}

/// The point at `t` of the curve `offset` from the ellipse of [`ellipse_point`]: the ellipse's
/// point moved `offset` along its outward unit normal (the gradient of `x²/a² + y²/b²`, which
/// points outward whatever the signs of the radii).
pub fn offset_ellipse_point(center: Point2<f64>, major: f64, minor: f64, rotation: f64, offset: f64, t: f64) -> Point2<f64> {
    let (x, y) = (major * t.cos(), minor * t.sin());
    let (gx, gy) = (x / (major * major), y / (minor * minor));
    let l = (gx * gx + gy * gy).sqrt().max(1e-300);
    let (x, y) = (x + offset * gx / l, y + offset * gy / l);
    let (s, c) = rotation.sin_cos();
    Point2::new(center.x + c * x - s * y, center.y + s * x + c * y)
}

/// A closed loop of curves, joined end to start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    pub curves: Vec<Curve2>,
}

/// One planar face to feed a feature: an outer loop and hole loops on a plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub outer: Loop,
    pub holes: Vec<Loop>,
    /// The caller's id for the region (like a curve's `source`), so the faces made from it can
    /// be named after it; its index in the profile if `None`.
    #[serde(default)]
    pub source: Option<u64>,
}

/// An open chain of curves, joined end to start (a surface or thin extrude of sketch curves that
/// don't close, PS4.10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    pub curves: Vec<Curve2>,
    /// The caller's id for the chain (its faces are named as a region's sides).
    #[serde(default)]
    pub source: Option<u64>,
}

/// The sketch regions selected for a feature, all on one plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub plane: Plane,
    pub regions: Vec<Region>,
    /// Open chains (only surface and thin extrudes use them).
    #[serde(default)]
    pub chains: Vec<Chain>,
}

impl Profile {
    /// A profile of closed regions only.
    pub fn new(plane: Plane, regions: Vec<Region>) -> Self {
        Self {
            plane,
            regions,
            chains: Vec::new(),
        }
    }
}

/// How far one end of an extrude goes (P3.3, PS4.3), measured from the (offset) sketch plane
/// along the extrude direction. `offset` is the "Offset distance": positive stops that far short
/// of the target (towards the sketch plane), negative goes past it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExtrudeEnd {
    /// A depth (mm).
    Blind(f64),
    /// To the first faces of [`ExtrudeSpec::scene`] the sweep meets; the end takes their shape.
    UpToNext { offset: f64 },
    /// To a face of a body. A planar face parallel to the sketch plane gives a flat end at its
    /// height; another planar face trims the end along its plane; a curved face ends the sweep
    /// where it meets the face's body.
    UpToFace { body: BodyId, face: FaceId, offset: f64 },
    /// To a body: the end takes the shape of the body's faces the sweep meets.
    UpToPart { body: BodyId, offset: f64 },
    /// To the height of a point.
    UpToVertex { point: Point3<f64>, offset: f64 },
    /// Through every body of [`ExtrudeSpec::scene`] (to their far extent).
    ThroughAll,
}

/// What an extrude makes (PS4.1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BodyKind {
    Solid,
    /// Sheets swept by the profile's curves (its regions' boundaries and its open chains); no
    /// caps.
    Surface,
    /// A wall along the profile's curves: `left` mm to the left of each curve (inside a region,
    /// seen from the plane normal) and `right` mm to the right (PS4.11).
    Thin { left: f64, right: f64 },
}

/// A planar face of a body to extrude, like a sketch region (PS4.2).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FaceInput {
    pub body: BodyId,
    pub face: FaceId,
    /// The caller's id for it (its caps are named after it, like a region's `source`).
    pub source: u64,
}

/// A full extrude (P3.3): the body type, the direction, the ends and the options of Onshape's
/// Extrude dialog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeSpec {
    pub body: BodyKind,
    /// The unit direction of the first end (the plane normal, or against it when flipped, or a
    /// picked direction, PS4.6). It must not lie in the plane.
    pub direction: Unit<Vector3<f64>>,
    /// The sweep starts this far along `direction` from the sketch plane (Starting offset, PS4.5).
    pub start_offset: f64,
    /// The first end (along `direction`).
    pub end: ExtrudeEnd,
    /// Symmetric (PS4.7): `end` (Blind or Through all) is the total, split evenly both ways.
    pub symmetric: bool,
    /// Second end position (PS4.8): an end against `direction`.
    pub second: Option<ExtrudeEnd>,
    /// The bodies Up to next and Through all consider (the other parts).
    pub scene: Vec<BodyId>,
    /// Planar faces extruded along with the profile's regions.
    pub faces: Vec<FaceInput>,
}

impl ExtrudeSpec {
    /// A blind solid extrude of `depth` along `direction`.
    pub fn blind(direction: Unit<Vector3<f64>>, depth: f64) -> Self {
        Self {
            body: BodyKind::Solid,
            direction,
            start_offset: 0.0,
            end: ExtrudeEnd::Blind(depth),
            symmetric: false,
            second: None,
            scene: Vec::new(),
            faces: Vec::new(),
        }
    }
}

/// How far one end of a revolve turns (P3.4, PS7.3), about [`RevolveSpec::axis`] from the
/// profile's plane. Angles are in radians; `offset` is the "Offset angle" of an "Up to" end:
/// positive stops that far short of the target, negative goes past it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RevolveEnd {
    /// A given angle.
    Angle(f64),
    /// To the first faces of [`RevolveSpec::scene`] the sweep meets.
    UpToNext { offset: f64 },
    /// To a face of a body: a plane through the axis gives an angle; any other face ends the
    /// sweep where it meets the face's body.
    UpToFace { body: BodyId, face: FaceId, offset: f64 },
    /// To a body: the end takes the shape of the body's faces the sweep meets.
    UpToPart { body: BodyId, offset: f64 },
    /// To the angle of a point about the axis.
    UpToVertex { point: Point3<f64>, offset: f64 },
}

/// A full revolve (P3.4): the body type, the axis, the ends and the options of Onshape's
/// Revolve dialog (PS7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevolveSpec {
    pub body: BodyKind,
    /// The first end turns counter-clockwise about `axis.dir` (right-handed); the second end
    /// the other way.
    pub axis: Axis,
    /// A whole turn (Onshape's "Full", the default); `end`, `symmetric` and `second` are then
    /// ignored.
    pub full: bool,
    pub end: RevolveEnd,
    /// Symmetric: `end` (an angle) is the total, split evenly both ways.
    pub symmetric: bool,
    /// Second end position: an end the other way round.
    pub second: Option<RevolveEnd>,
    /// The bodies Up to next considers (the other parts).
    pub scene: Vec<BodyId>,
    /// Planar faces revolved along with the profile's regions (solid revolves).
    #[serde(default)]
    pub faces: Vec<FaceInput>,
}

impl RevolveSpec {
    /// A solid revolve about `axis`: a whole turn when `angle` is `None`.
    pub fn solid(axis: Axis, angle: Option<f64>) -> Self {
        Self {
            body: BodyKind::Solid,
            axis,
            full: angle.is_none(),
            end: RevolveEnd::Angle(angle.unwrap_or(std::f64::consts::TAU)),
            symmetric: false,
            second: None,
            scene: Vec::new(),
            faces: Vec::new(),
        }
    }
}

/// One curve of a sweep path (P3.7, PS19.2): an edge of a body or a sketch curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PathCurve {
    Edge { body: BodyId, edge: EdgeId },
    Sketch { plane: Plane, curve: Curve2 },
    /// A (rational) Bézier curve in space by its poles and weights (a piece of a helix, say).
    Bezier3 { poles: Vec<Point3<f64>>, weights: Vec<f64> },
}

/// How a sweep's profile turns as it follows the path (Onshape's profile control, PS19.6).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum SweepControl {
    /// The profile keeps its angle to the path (no twist about it).
    #[default]
    None,
    /// The profile keeps its orientation in space (it is only moved along the path).
    KeepOrientation,
    /// The profile's plane keeps containing this direction.
    LockDirection(Unit<Vector3<f64>>),
}

/// A sweep (P3.7, PS19): a profile's regions (or, for surfaces and thin walls, its curves)
/// moved along a path of connected curves. When the profile's plane crosses the path away
/// from its ends, the sweep runs both ways from there (PS19.5); a closed path starts there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepSpec {
    pub body: BodyKind,
    /// Connected curves, in any order.
    pub path: Vec<PathCurve>,
    pub control: SweepControl,
    /// Planar faces swept with the profile's regions.
    #[serde(default)]
    pub faces: Vec<FaceInput>,
}

/// One section of a loft (P3.7, PS20.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LoftSection {
    /// The regions of a sketch: together they must be one closed contour (PS20.5); regions
    /// that share curves are joined (the Funnel's rim band and the disc inside it).
    Profile(Profile),
    /// A face of a body (its outer boundary). Planar faces are capped flat; a non-planar face
    /// (P3.10, PS20.1) is itself the loft's cap.
    Face(FaceInput),
    /// A point: only the first or the last section.
    Point(Point3<f64>),
}

/// How a loft leaves its first or arrives at its last section (PS20.4).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum LoftCondition {
    /// No condition: the smoothest surface through the sections.
    #[default]
    Default,
    /// Square to the section's plane.
    NormalToProfile,
    /// Along the section's plane, flaring out from it (or in, towards a smaller neighbour).
    TangentToProfile,
    /// (P3.10) Tangent to the faces next to a face section across its boundary edges: the loft
    /// continues them smoothly (G1). Only for [`LoftSection::Face`] ends.
    MatchTangent,
    /// (P3.10) As `MatchTangent`, and with the same normal curvature across the boundary (G2).
    /// Only for face ends of a loft of two sections.
    MatchCurvature,
    /// (P3.11) As `NormalToProfile`, along a picked direction (a unit vector; turned to point
    /// along the loft) instead of the section's normal.
    NormalDirection([f64; 3]),
    /// (P3.11) As `TangentToProfile`, in the plane normal to a picked direction instead of the
    /// section's plane.
    TangentDirection([f64; 3]),
}

/// A loft end's condition and its magnitude (the derivative's length relative to the loft's
/// length through the section centres; 1 is the natural speed).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LoftEnd {
    pub condition: LoftCondition,
    pub magnitude: f64,
}

impl Default for LoftEnd {
    fn default() -> Self {
        Self {
            condition: LoftCondition::Default,
            magnitude: 1.0,
        }
    }
}

/// A loft (P3.7, PS20): sections in order, and the end conditions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoftSpec {
    /// Solid, Surface (no caps) or Thin (the surface thickened: `left` inside, `right`
    /// outside).
    pub body: BodyKind,
    pub sections: Vec<LoftSection>,
    pub start: LoftEnd,
    pub end: LoftEnd,
    /// The caller's id for the loft's faces (like a profile region's `source`).
    pub source: u64,
}

/// What splits a body (P3.7, PS18.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SplitTool {
    /// An unbounded plane.
    Plane(Plane),
    /// A face of a body (not extended).
    Face { body: BodyId, face: FaceId },
    /// A surface body (a sheet).
    Body(BodyId),
}

/// A circle in space: its center, the unit normal of its plane and its radius.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Circle3 {
    pub center: Point3<f64>,
    pub normal: Unit<Vector3<f64>>,
    pub radius: f64,
}

/// Where a line crosses a face of a body ([`crate::Kernel::ray_hits`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RayHit {
    pub face: FaceId,
    /// The distance from the ray's origin along its (unit) direction; negative behind it.
    pub t: f64,
    pub point: Point3<f64>,
}

/// An axis-aligned box (mm).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Point3<f64>,
    pub max: Point3<f64>,
}

impl Aabb {
    /// The largest value of `(p - origin)·dir` over the box's corners.
    pub fn max_along(&self, origin: Point3<f64>, dir: &Vector3<f64>) -> f64 {
        let mut best = f64::NEG_INFINITY;
        for i in 0..8 {
            let p = Point3::new(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            );
            best = best.max((p - origin).dot(dir));
        }
        best
    }

    /// The length of its diagonal.
    pub fn diagonal(&self) -> f64 {
        (self.max - self.min).norm()
    }
}

/// How far an extrude goes. Distances are in mm along the plane normal (negative = opposite).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Extent {
    Blind(f64),
    /// Total depth, split evenly on both sides of the plane (Onshape "Symmetric").
    Symmetric(f64),
    /// Two independent directions (Onshape "second end position"): from `-backward` to
    /// `forward` along the normal.
    TwoSided { forward: f64, backward: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoolOp {
    /// Onshape "Add".
    Union,
    /// Onshape "Remove".
    Subtract,
    /// Onshape "Intersect".
    Intersect,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Axis {
    pub origin: Point3<f64>,
    pub dir: Unit<Vector3<f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ChamferSpec {
    EqualDistance(f64),
    TwoDistances(f64, f64),
    DistanceAngle { distance: f64, angle: f64 },
}

/// How a fillet's size is given (P3.6, PS14.3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FilletSize {
    /// A constant radius (mm).
    Radius(f64),
    /// The chord between the fillet's two contact lines (mm), kept constant along the edge:
    /// where the faces meet at an angle `φ` between their normals, the radius there is
    /// `w / (2 sin(φ/2))`, so the radius varies along an edge whose faces' angle varies.
    Width(f64),
}

/// A fillet's cross section (P3.6, PS14.4). Conic and Curvature keep the contact lines of the
/// circular fillet of the same size; they are built for straight edges between flat faces.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum FilletProfile {
    /// A circular arc (OCCT's rolling-ball fillet).
    #[default]
    Circular,
    /// A conic through the two contact points, tangent to both faces, whose shoulder lies at
    /// `rho` of the way from the contact chord's middle to the corner (0 < rho < 1: an ellipse
    /// below 0.5, a parabola at 0.5, a hyperbola above; the circle's rho is
    /// `sin(θ/2)/(1 + sin(θ/2))` for faces at an angle θ).
    Conic { rho: f64 },
    /// A curvature-continuous (G2) section: a quintic Bezier whose curvature is zero where it
    /// meets the faces. `magnitude` (0 < m ≤ 1) is how far towards the corner its inner poles
    /// reach.
    Curvature { magnitude: f64 },
    /// (P3.10, PS14.6 Asymmetric) A section tangent to both faces whose contact lines are the
    /// circular fillet's for the size on the edge's first face and for `second` on its other
    /// face (`flip` swaps them): the conic with the circle's weight `sin(θ/2)`, which for faces
    /// at 90° is the quarter ellipse with those semi-axes. Straight edges between flat faces.
    Asymmetric { second: f64, flip: bool },
}

/// A fillet (P3.6, PS14.1–14.5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FilletSpec {
    pub size: FilletSize,
    /// Onshape's "Allow edge overflow": when off, a fillet that would run onto a face other
    /// than the faces meeting at its edge's ends is refused.
    pub allow_overflow: bool,
    #[serde(default)]
    pub profile: FilletProfile,
}

impl FilletSpec {
    pub fn radius(r: f64) -> Self {
        Self {
            size: FilletSize::Radius(r),
            allow_overflow: true,
            profile: FilletProfile::Circular,
        }
    }
}

/// Where a chamfer's distances are measured (P3.6, PS14.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChamferMeasure {
    /// Along each face from the edge.
    #[default]
    Offset,
    /// Along the faces' tangent planes at the edge, from where they meet. The same as Offset
    /// on a face that is straight across the edge (a plane, a cylinder's rim); on a face that
    /// curves across it (a sphere, a cylinder along a straight edge) of radius R a tangent
    /// distance d is the chord `2R sin(atan(d/R)/2)` from the edge.
    Tangent,
}

/// A chamfer with its options (P3.6, PS14.7–14.9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamferOpts {
    pub spec: ChamferSpec,
    pub measurement: ChamferMeasure,
    /// The first distance (and the angle) go on the edge's other face.
    pub flip: bool,
    /// Edges flipped on their own (Onshape's "Direction overrides"), on top of `flip`.
    pub flipped: Vec<EdgeId>,
}

impl ChamferOpts {
    pub fn new(spec: ChamferSpec) -> Self {
        Self {
            spec,
            measurement: ChamferMeasure::Offset,
            flip: false,
            flipped: Vec::new(),
        }
    }
}

/// A shell (P3.6, PS16).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellSpec {
    /// The faces to remove (the openings); ignored when `hollow`.
    pub remove: Vec<FaceId>,
    /// Wall thickness (mm).
    pub thickness: f64,
    /// The walls grow outside the body instead of inside it.
    pub outward: bool,
    /// A closed hollow body: no face is removed, the inside becomes a void.
    pub hollow: bool,
}

/// Volume, area, centroid and inertia, computed from exact geometry where the backend can.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MassProperties {
    /// mm³
    pub volume: f64,
    /// mm²
    pub surface_area: f64,
    /// Volume centroid (mm); multiply `volume` by a density for mass.
    pub center_of_mass: Point3<f64>,
    /// The volume's inertia tensor at unit density (mm⁵) about `center_of_mass`, axes parallel
    /// to the model axes: `I = ∫ (|r|² E − r rᵀ) dV`, so the diagonal holds the moments
    /// (`Ixx = ∫ (y² + z²) dV`) and the products carry the minus sign (`Ixy = −∫ x y dV`).
    /// Multiply by a density (kg/mm³) for kg·mm². Zero for a surface body. P3.5.
    #[serde(default)]
    pub inertia: Matrix3<f64>,
}

impl MassProperties {
    /// The inertia tensor about `point` instead of the centroid (parallel axis theorem):
    /// `I_p = I_c + V (|d|² E − d dᵀ)` with `d = c − p` (unit density).
    pub fn inertia_about(&self, point: Point3<f64>) -> Matrix3<f64> {
        let d = self.center_of_mass - point;
        self.inertia + (Matrix3::identity() * d.norm_squared() - d * d.transpose()) * self.volume
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceInfo {
    pub id: FaceId,
    pub kind: SurfaceKind,
    /// Set for planar faces, so they can be picked as sketch planes.
    pub plane: Option<Plane>,
    pub area: f64,
    /// The area centroid (on the face for planar faces; it may lie off a curved face).
    pub center: Point3<f64>,
    /// The axis of a cylinder, cone, sphere, torus or surface of revolution (P3.4: a revolve
    /// axis can be a cylindrical face).
    #[serde(default)]
    pub axis: Option<Axis>,
    /// The radius of a cylinder or sphere (a cone's reference radius, a torus's major radius)
    /// (P3.6).
    #[serde(default)]
    pub radius: Option<f64>,
}

/// The kind of curve an edge lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurveKind {
    Line,
    Circle,
    Ellipse,
    Other,
    /// A degenerate edge (a cone's apex): a point.
    Degenerate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeInfo {
    pub id: EdgeId,
    /// The faces on either side (the same face twice for a seam, `None` for a free edge).
    pub faces: [Option<FaceId>; 2],
    /// Exact length (mm).
    pub length: f64,
    pub curve: CurveKind,
    /// Where it starts and ends (the same point for a closed edge), and its middle point.
    pub start: Point3<f64>,
    pub end: Point3<f64>,
    pub mid: Point3<f64>,
    /// Unit tangents at the start and at the end, in the direction the edge runs.
    pub start_tangent: Vector3<f64>,
    pub end_tangent: Vector3<f64>,
    /// The exact circle of a circular edge or arc (P3.4: a revolve axis can be a circular edge,
    /// and Use projects the exact curve).
    #[serde(default)]
    pub circle: Option<Circle3>,
}

impl EdgeInfo {
    /// True for an edge that ends where it starts (a whole circle).
    pub fn is_closed(&self) -> bool {
        (self.end - self.start).norm() < 1e-9 && self.length > 1e-9
    }
}

/// A vertex of a body (its index among the body's vertices, like [`FaceId`] and [`EdgeId`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VertexId(pub u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VertexInfo {
    pub id: VertexId,
    pub point: Point3<f64>,
    /// The edges that end at it.
    pub edges: Vec<EdgeId>,
}

/// How finely to tessellate: the largest distance from a triangle or edge segment to the exact
/// surface (mm), and the largest angle between neighbouring normals (radians).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tessellation {
    pub deflection: f64,
    pub angle: f64,
}

impl Default for Tessellation {
    /// 0.01 mm and 2.5° (the angle step of sketch arcs on screen).
    fn default() -> Self {
        Self {
            deflection: 0.01,
            angle: std::f64::consts::PI / 72.0,
        }
    }
}

/// A triangle mesh for display, tagged with the face each triangle belongs to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TriMesh {
    pub positions: Vec<Point3<f64>>,
    pub normals: Vec<Vector3<f64>>,
    pub indices: Vec<[u32; 3]>,
    /// One entry per triangle.
    pub triangle_faces: Vec<FaceId>,
    /// Edge polylines for drawing black edges and edge picking.
    pub edges: Vec<(EdgeId, Vec<Point3<f64>>)>,
}

/// What an operation did; feeds persistent naming (see [`crate::naming`]). Face ids are the
/// result body's, except inside an [`InputFace`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    /// New faces and what they came from (a profile curve, a cap, an input edge, ...).
    pub generated: Vec<(FaceId, Origin)>,
    /// Faces that continue a face of an input body, possibly trimmed or split (a face split in
    /// two is listed twice with the same input).
    pub modified: Vec<(FaceId, InputFace)>,
    /// Faces of the input bodies that are gone.
    pub deleted: Vec<InputFace>,
}

/// A face of one of an operation's input bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InputFace {
    pub body: BodyId,
    pub face: FaceId,
}

/// Where a new face came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Origin {
    /// Swept by a curve of a profile region. `region` is the region's `source` (its index in
    /// [`Profile::regions`] if it has none); `curve` is the curve's `source`, or
    /// [`unsourced_curve`] of its place in the region.
    ProfileCurve { region: u64, curve: u64 },
    /// The cap of profile region `region` (as above) where the sweep starts (on the profile
    /// plane for a blind extrude).
    StartCap { region: u64 },
    /// The cap of profile region `region` where the sweep ends.
    EndCap { region: u64 },
    /// Made from an edge of an input body (a fillet or chamfer face).
    FromEdge { body: BodyId, edge: EdgeId },
    /// Made from a vertex of an input body (a fillet's corner patch).
    FromVertex { body: BodyId, vertex: VertexId },
}

/// The curve id [`Origin::ProfileCurve`] uses for a profile curve without a `source`: the
/// `index`-th curve of loop `lp` (0 = the outer loop) of its region.
pub fn unsourced_curve(lp: usize, index: usize) -> u64 {
    u64::MAX - ((lp as u64) << 32 | index as u64)
}

pub type Transform = Isometry3<f64>;

/// A rigid motion or a reflection (P3.8: patterns and mirrors): `x ↦ linear·x + translation`,
/// `linear` orthonormal (det +1: a rotation, det −1: a reflection). [`Transform`] can't hold a
/// reflection, so the mirror needs this.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    pub linear: Matrix3<f64>,
    pub translation: Vector3<f64>,
}

impl Default for Motion {
    fn default() -> Self {
        Self::identity()
    }
}

impl Motion {
    pub fn identity() -> Self {
        Self { linear: Matrix3::identity(), translation: Vector3::zeros() }
    }

    pub fn translation(v: Vector3<f64>) -> Self {
        Self { linear: Matrix3::identity(), translation: v }
    }

    /// A turn by `angle` (radians, counter-clockwise seen from the tip of `axis.dir`) about
    /// `axis`.
    pub fn rotation(axis: &Axis, angle: f64) -> Self {
        let r = nalgebra::Rotation3::from_axis_angle(&axis.dir, angle).into_inner();
        let o = axis.origin.coords;
        Self { linear: r, translation: o - r * o }
    }

    /// The reflection in the plane through `point` with unit normal `normal`:
    /// `x ↦ x − 2 n (n·(x − p))`.
    pub fn reflection(point: Point3<f64>, normal: Unit<Vector3<f64>>) -> Self {
        let n = normal.into_inner();
        let linear = Matrix3::identity() - 2.0 * n * n.transpose();
        Self { linear, translation: 2.0 * n * n.dot(&point.coords) }
    }

    pub fn from_isometry(t: &Transform) -> Self {
        Self { linear: t.rotation.to_rotation_matrix().into_inner(), translation: t.translation.vector }
    }

    /// `self` then `next`.
    pub fn then(&self, next: &Motion) -> Motion {
        Motion { linear: next.linear * self.linear, translation: next.linear * self.translation + next.translation }
    }

    pub fn inverse(&self) -> Motion {
        let inv = self.linear.transpose();
        Motion { linear: inv, translation: -(inv * self.translation) }
    }

    pub fn point(&self, p: &Point3<f64>) -> Point3<f64> {
        Point3::from(self.linear * p.coords + self.translation)
    }

    pub fn vector(&self, v: &Vector3<f64>) -> Vector3<f64> {
        self.linear * v
    }

    /// True for a reflection (det −1).
    pub fn is_reflection(&self) -> bool {
        self.linear.determinant() < 0.0
    }

    /// The rigid motion, if it is one.
    pub fn to_isometry(&self) -> Option<Transform> {
        if self.is_reflection() {
            return None;
        }
        let r = nalgebra::Rotation3::from_matrix_unchecked(self.linear);
        Some(Transform::from_parts(nalgebra::Translation3::from(self.translation), nalgebra::UnitQuaternion::from_rotation_matrix(&r)))
    }

    /// The 3 × 4 matrix `[linear | translation]` row by row.
    pub fn rows(&self) -> [f64; 12] {
        let (m, t) = (&self.linear, &self.translation);
        [m[(0, 0)], m[(0, 1)], m[(0, 2)], t.x, m[(1, 0)], m[(1, 1)], m[(1, 2)], t.y, m[(2, 0)], m[(2, 1)], m[(2, 2)], t.z]
    }
}

/// Where a point is relative to a solid ([`crate::Kernel::classify`], P3.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointClass {
    Inside,
    Outside,
    OnBoundary,
}

/// A draft (P3.10, PS4.9, X3): faces turned about where they meet the neutral plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DraftSpec {
    /// The faces to draft.
    pub faces: Vec<FaceId>,
    /// The draft angle (radians). Positive leans the faces in towards the material as they run
    /// along `pull` from the neutral plane (a boss narrows as it rises); negative leans them out.
    pub angle: f64,
    /// The pull direction (the neutral plane's normal, or its opposite when flipped).
    pub pull: Unit<Vector3<f64>>,
    /// A point on the neutral plane (its normal is `pull`).
    pub neutral: Point3<f64>,
    /// Draft the faces tangent to the picked ones too.
    pub tangent_propagation: bool,
}

/// An offset of a solid's faces (P3.10: the Boolean feature's Subtract offset, PS5.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffsetSpec {
    /// How far every face moves outward (mm; negative moves inward).
    pub distance: f64,
    /// Faces moved by their own distance instead.
    pub faces: Vec<(FaceId, f64)>,
    /// Keep edges sharp (the offset faces meet where they intersect: a box stays a box), else
    /// round them (the body grown by a ball).
    pub sharp: bool,
}

/// A variable radius along one edge (P3.10, PS14.6 Variable fillet): `(t, r)` pairs, `t` in
/// [0, 1] from the edge's `start` to its `end`, at least one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilletLaw {
    pub edge: EdgeId,
    pub radii: Vec<(f64, f64)>,
}

/// A Thicken (Onshape's Thicken): surfaces and faces made solid with a thickness. Each sheet
/// is offset `along` mm along its faces' normals and `against` mm the other way; the solids
/// they make are fused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThickenSpec {
    /// Whole bodies (surfaces) to thicken.
    pub bodies: Vec<BodyId>,
    /// Faces of bodies to thicken (grouped by body into one sheet each).
    pub faces: Vec<FaceInput>,
    /// Planar sketch regions to thicken (their faces, normals along the plane's).
    pub profiles: Vec<Profile>,
    pub along: f64,
    pub against: f64,
    /// The caller's id for the new faces.
    pub source: u64,
}

/// One boundary curve of a Fill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FillCurve {
    Edge { body: BodyId, edge: EdgeId },
    Sketch { plane: Plane, curve: Curve2 },
}

/// A Fill (Onshape's Fill): a surface bounded by a closed chain of curves. A planar chain is
/// filled with its plane; four curves (or three, one side collapsing to a point) with a Coons
/// patch through them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FillSpec {
    /// Connected curves, in any order.
    pub curves: Vec<FillCurve>,
    /// The caller's id for the new face.
    pub source: u64,
}
