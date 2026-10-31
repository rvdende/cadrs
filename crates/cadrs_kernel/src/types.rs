//! Kernel-independent geometry and result types: what goes into [`crate::Kernel`] operations
//! (profiles, feature specs) and what comes out (ids, face and edge info, meshes, mass
//! properties, history). Lengths are mm and angles radians throughout.

use crate::native::tol;
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
    /// The plane's origin (sketch coordinates (0, 0)).
    pub origin: Point3<f64>,
    /// The sketch x axis, in the plane.
    pub x_dir: Unit<Vector3<f64>>,
    /// The plane's normal; the sketch y axis is `normal × x_dir`.
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

    /// The sketch y axis: `normal × x_dir`.
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
    /// The segment from `a` to `b`.
    Line {
        /// Start point.
        a: Point2<f64>,
        /// End point.
        b: Point2<f64>,
        /// The sketch curve it came from.
        source: Option<u64>,
    },
    /// An arc from `start_angle` sweeping `sweep` radians: counter-clockwise when `sweep` is
    /// positive, clockwise when it is negative.
    Arc {
        /// Centre.
        center: Point2<f64>,
        /// Radius (above zero).
        radius: f64,
        /// Angle of the start point from the plane's x axis.
        start_angle: f64,
        /// Signed angle swept from the start.
        sweep: f64,
        /// The sketch curve it came from.
        source: Option<u64>,
    },
    /// A whole circle: counter-clockwise, or clockwise when `radius` is negative.
    Circle {
        /// Centre.
        center: Point2<f64>,
        /// Radius; its sign gives the direction.
        radius: f64,
        /// The sketch curve it came from.
        source: Option<u64>,
    },
    /// A whole ellipse, counter-clockwise from the end of its major axis.
    Ellipse {
        /// Centre.
        center: Point2<f64>,
        /// Semi-axis along `rotation`.
        major_radius: f64,
        /// The other semi-axis.
        minor_radius: f64,
        /// Angle of the major axis from the plane's x axis.
        rotation: f64,
        /// The sketch curve it came from.
        source: Option<u64>,
    },
    /// Part of an ellipse: the points `center + R(rotation)·(major·cos t, minor·sin t)` for `t`
    /// from `start` through `sweep` (counter-clockwise when positive and `minor_radius` is
    /// positive). `minor_radius` may be larger than `major_radius`, or negative.
    EllipseArc {
        /// Centre.
        center: Point2<f64>,
        /// Semi-axis along `rotation`.
        major_radius: f64,
        /// The other semi-axis (its sign flips the direction).
        minor_radius: f64,
        /// Angle of the major axis from the plane's x axis.
        rotation: f64,
        /// Parameter `t` of the start point.
        start: f64,
        /// Signed parameter range swept from `start`.
        sweep: f64,
        /// The sketch curve it came from.
        source: Option<u64>,
    },
    /// Part of the curve `offset` from an ellipse (P3.7, X13: an offset of an ellipse isn't an
    /// ellipse): the points `e(t) + offset·n(t)`, where `e(t)` is the [`Curve2::EllipseArc`]
    /// with the same parameters and `n(t)` its unit normal pointing away from the centre, for
    /// `t` from `start` through `sweep` (a whole turn for the closed curve). Positive offsets
    /// lie outside the ellipse.
    OffsetEllipseArc {
        /// Centre of the base ellipse.
        center: Point2<f64>,
        /// The base ellipse's semi-axis along `rotation`.
        major_radius: f64,
        /// The base ellipse's other semi-axis.
        minor_radius: f64,
        /// Angle of the major axis from the plane's x axis.
        rotation: f64,
        /// Parameter `t` of the start point.
        start: f64,
        /// Signed parameter range swept from `start`.
        sweep: f64,
        /// Distance from the ellipse (positive outside).
        offset: f64,
        /// The sketch curve it came from.
        source: Option<u64>,
    },
    /// A cubic Bézier curve (Final, S12.14: a sketch's Bézier curve): from `poles[0]` to
    /// `poles[3]`, with control points `poles[1]` and `poles[2]`.
    Bezier {
        /// The four control points.
        poles: [Point2<f64>; 4],
        /// The sketch curve it came from.
        source: Option<u64>,
    },
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
            Curve2::Arc {
                center,
                radius,
                start_angle,
                sweep,
                ..
            } => {
                let t = start_angle + sweep * s;
                Point2::new(center.x + radius * t.cos(), center.y + radius * t.sin())
            }
            Curve2::Circle { center, radius, .. } => {
                let t = std::f64::consts::TAU * s;
                Point2::new(center.x + radius * t.cos(), center.y + radius * t.sin())
            }
            Curve2::Ellipse {
                center,
                major_radius,
                minor_radius,
                rotation,
                ..
            } => ellipse_point(
                center,
                major_radius,
                minor_radius,
                rotation,
                std::f64::consts::TAU * s,
            ),
            Curve2::EllipseArc {
                center,
                major_radius,
                minor_radius,
                rotation,
                start,
                sweep,
                ..
            } => ellipse_point(
                center,
                major_radius,
                minor_radius,
                rotation,
                start + sweep * s,
            ),
            Curve2::OffsetEllipseArc {
                center,
                major_radius,
                minor_radius,
                rotation,
                start,
                sweep,
                offset,
                ..
            } => offset_ellipse_point(
                center,
                major_radius,
                minor_radius,
                rotation,
                offset,
                start + sweep * s,
            ),
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
            Curve2::Arc { sweep, .. }
            | Curve2::EllipseArc { sweep, .. }
            | Curve2::OffsetEllipseArc { sweep, .. } => {
                sweep.abs() >= std::f64::consts::TAU - tol::ANGULAR
            }
            Curve2::Line { .. } | Curve2::Bezier { .. } => false,
        }
    }
}

/// `center + R(rotation)·(major·cos t, minor·sin t)`.
pub fn ellipse_point(
    center: Point2<f64>,
    major: f64,
    minor: f64,
    rotation: f64,
    t: f64,
) -> Point2<f64> {
    let (x, y) = (major * t.cos(), minor * t.sin());
    let (s, c) = rotation.sin_cos();
    Point2::new(center.x + c * x - s * y, center.y + s * x + c * y)
}

/// The point at `t` of the curve `offset` from the ellipse of [`ellipse_point`]: the ellipse's
/// point moved `offset` along its outward unit normal (the gradient of `x²/a² + y²/b²`, which
/// points outward whatever the signs of the radii).
pub fn offset_ellipse_point(
    center: Point2<f64>,
    major: f64,
    minor: f64,
    rotation: f64,
    offset: f64,
    t: f64,
) -> Point2<f64> {
    let (x, y) = (major * t.cos(), minor * t.sin());
    let (gx, gy) = (x / (major * major), y / (minor * minor));
    let l = (gx * gx + gy * gy).sqrt().max(tol::TINY);
    let (x, y) = (x + offset * gx / l, y + offset * gy / l);
    let (s, c) = rotation.sin_cos();
    Point2::new(center.x + c * x - s * y, center.y + s * x + c * y)
}

/// A closed loop of curves, joined end to start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    /// The curves in order, each starting where the one before ends (the last ends where the
    /// first starts). Any direction: the kernel orients loops itself.
    pub curves: Vec<Curve2>,
}

/// One planar face to feed a feature: an outer loop and hole loops on a plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// The outer boundary.
    pub outer: Loop,
    /// Holes, inside `outer` and apart from each other.
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
    /// The curves in order, each starting where the one before ends.
    pub curves: Vec<Curve2>,
    /// The caller's id for the chain (its faces are named as a region's sides).
    #[serde(default)]
    pub source: Option<u64>,
}

/// The sketch regions selected for a feature, all on one plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// The sketch plane every curve lies on.
    pub plane: Plane,
    /// Closed regions (the faces a solid feature sweeps).
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
    UpToNext {
        /// Offset distance (see the type).
        offset: f64,
    },
    /// To a face of a body. A planar face parallel to the sketch plane gives a flat end at its
    /// height; another planar face trims the end along its plane; a curved face ends the sweep
    /// where it meets the face's body.
    UpToFace {
        /// The face's body.
        body: BodyId,
        /// The face.
        face: FaceId,
        /// Offset distance (see the type).
        offset: f64,
    },
    /// To a body: the end takes the shape of the body's faces the sweep meets.
    UpToPart {
        /// The body.
        body: BodyId,
        /// Offset distance (see the type).
        offset: f64,
    },
    /// To the height of a point.
    UpToVertex {
        /// The point.
        point: Point3<f64>,
        /// Offset distance (see the type).
        offset: f64,
    },
    /// Through every body of [`ExtrudeSpec::scene`] (to their far extent).
    ThroughAll,
}

/// What an extrude makes (PS4.1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BodyKind {
    /// A solid: the regions swept, capped at both ends.
    Solid,
    /// Sheets swept by the profile's curves (its regions' boundaries and its open chains); no
    /// caps.
    Surface,
    /// A wall along the profile's curves: `left` mm to the left of each curve (inside a region,
    /// seen from the plane normal) and `right` mm to the right (PS4.11).
    Thin {
        /// Wall thickness to the left of each curve (mm).
        left: f64,
        /// Wall thickness to the right of each curve (mm).
        right: f64,
    },
}

/// A planar face of a body to extrude, like a sketch region (PS4.2).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FaceInput {
    /// The face's body.
    pub body: BodyId,
    /// The planar face.
    pub face: FaceId,
    /// The caller's id for it (its caps are named after it, like a region's `source`).
    pub source: u64,
}

/// A full extrude (P3.3): the body type, the direction, the ends and the options of Onshape's
/// Extrude dialog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeSpec {
    /// Solid, Surface or Thin.
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
    UpToNext {
        /// Offset angle (see the type).
        offset: f64,
    },
    /// To a face of a body: a plane through the axis gives an angle; any other face ends the
    /// sweep where it meets the face's body.
    UpToFace {
        /// The face's body.
        body: BodyId,
        /// The face.
        face: FaceId,
        /// Offset angle (see the type).
        offset: f64,
    },
    /// To a body: the end takes the shape of the body's faces the sweep meets.
    UpToPart {
        /// The body.
        body: BodyId,
        /// Offset angle (see the type).
        offset: f64,
    },
    /// To the angle of a point about the axis.
    UpToVertex {
        /// The point.
        point: Point3<f64>,
        /// Offset angle (see the type).
        offset: f64,
    },
}

/// A full revolve (P3.4): the body type, the axis, the ends and the options of Onshape's
/// Revolve dialog (PS7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevolveSpec {
    /// Solid, Surface or Thin.
    pub body: BodyKind,
    /// The first end turns counter-clockwise about `axis.dir` (right-handed); the second end
    /// the other way.
    pub axis: Axis,
    /// A whole turn (Onshape's "Full", the default); `end`, `symmetric` and `second` are then
    /// ignored.
    pub full: bool,
    /// The first end.
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
    /// An edge of a body.
    Edge {
        /// The edge's body.
        body: BodyId,
        /// The edge.
        edge: EdgeId,
    },
    /// A sketch curve on its plane.
    Sketch {
        /// The sketch plane.
        plane: Plane,
        /// The curve in the plane's coordinates.
        curve: Curve2,
    },
    /// A (rational) Bézier curve in space by its poles and weights (a piece of a helix, say).
    Bezier3 {
        /// Control points, at least two.
        poles: Vec<Point3<f64>>,
        /// One weight per pole, or empty for a polynomial curve.
        weights: Vec<f64>,
    },
    /// A B-spline curve in space (a projected curve, say).
    Bspline(Bspline3),
}

/// A clamped (rational) B-spline curve in space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bspline3 {
    /// Its degree.
    pub degree: usize,
    /// Clamped knots: as many as the poles plus the degree plus one.
    pub knots: Vec<f64>,
    /// Control points.
    pub poles: Vec<Point3<f64>>,
    /// One weight per pole, or empty for a polynomial curve.
    pub weights: Vec<f64>,
}

/// A sweep of curves along a path (Onshape's surface Sweep of edges or curves): a sheet. See
/// [`Kernel::sweep_curves`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurveSweepSpec {
    /// The profile: curves joined end to end into one open chain.
    pub profile: Vec<PathCurve>,
    /// The path: curves joined end to end.
    pub path: Vec<PathCurve>,
    /// How the profile turns as it goes.
    pub frame: SweepFrame,
    /// The caller's id for the sweep's face.
    pub source: u64,
}

/// How a [`CurveSweepSpec`]'s profile turns along its path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SweepFrame {
    /// It keeps its angle to the path (turning as little as it can about it).
    Follow,
    /// It keeps its orientation in space.
    KeepOrientation,
    /// It keeps its angle to these faces along the path (none: the faces along a path of
    /// edges).
    LockFaces(Vec<(BodyId, FaceId)>),
}

/// A Face blend (Onshape's, rolling ball): a blend between two faces that meet along an edge,
/// the walls trimmed to it and joined with it into one sheet. See [`Kernel::face_blend`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceBlendSpec {
    /// The first wall's face.
    pub side1: (BodyId, FaceId),
    /// The second wall's face.
    pub side2: (BodyId, FaceId),
    /// The ball's size.
    pub size: BlendSize,
    /// The caller's id for the blend face.
    pub source: u64,
}

/// How big a face blend's ball is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BlendSize {
    /// A constant radius (mm).
    Radius(f64),
    /// The radius whose contacts on the two faces lie this far apart (mm): a constant width.
    Width(f64),
}

/// A Projected curve (Onshape's, Curve to face): curves projected along a direction onto faces.
/// See [`Kernel::project_curve`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectCurveSpec {
    /// The curves to project, joined end to end.
    pub curves: Vec<PathCurve>,
    /// The faces they land on.
    pub faces: Vec<(BodyId, FaceId)>,
    /// The way they are projected (a unit vector).
    pub direction: Vector3<f64>,
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
    /// Solid, Surface or Thin.
    pub body: BodyKind,
    /// Connected curves, in any order.
    pub path: Vec<PathCurve>,
    /// How the profile turns along the path.
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
    /// The condition.
    pub condition: LoftCondition,
    /// Its magnitude (1 is the natural speed).
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
    /// The sections in order, at least two.
    pub sections: Vec<LoftSection>,
    /// The condition at the first section.
    pub start: LoftEnd,
    /// The condition at the last section.
    pub end: LoftEnd,
    /// The caller's id for the loft's faces (like a profile region's `source`).
    pub source: u64,
}

/// A loft through curves (Onshape's loft of wire profiles, with guides): one sheet face
/// through the profiles, bent to follow the guides. See [`Kernel::loft_curves`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurveLoftSpec {
    /// The profiles in order, at least two.
    pub sections: Vec<CurveSection>,
    /// Guide curves: each a chain of curves crossing every profile.
    pub guides: Vec<Vec<PathCurve>>,
    /// The condition at the first profile.
    pub start: CurveLoftEnd,
    /// The condition at the last profile.
    pub end: CurveLoftEnd,
    /// The caller's id for the loft's face (like a profile region's `source`).
    pub source: u64,
}

/// A profile of a [`CurveLoftSpec`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CurveSection {
    /// Curves joined end to end into one open chain.
    Wire(Vec<PathCurve>),
    /// A point: only the first or the last profile.
    Point(Point3<f64>),
}

/// How a [`CurveLoftSpec`] leaves its first or arrives at its last profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurveLoftEnd {
    /// `Default`, `NormalToProfile` (square to the profile's plane), `NormalDirection`, or
    /// `MatchTangent` (`MatchCurvature` is taken as it) to [`CurveLoftEnd::faces`].
    pub condition: LoftCondition,
    /// Its magnitude (1 is the natural speed).
    pub magnitude: f64,
    /// The faces Match tangent continues: those along the profile.
    pub faces: Vec<(BodyId, FaceId)>,
}

impl Default for CurveLoftEnd {
    fn default() -> Self {
        Self {
            condition: LoftCondition::Default,
            magnitude: 1.0,
            faces: Vec::new(),
        }
    }
}

/// What splits a body (P3.7, PS18.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SplitTool {
    /// An unbounded plane.
    Plane(Plane),
    /// A face of a body (not extended).
    Face {
        /// The face's body.
        body: BodyId,
        /// The face.
        face: FaceId,
    },
    /// A surface body (a sheet).
    Body(BodyId),
    /// Curves lying on the faces (a Split of faces only): each face is cut along the stretch of
    /// them that runs across it.
    Curves(Vec<PathCurve>),
}

/// A circle in space: its center, the unit normal of its plane and its radius.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Circle3 {
    /// Centre.
    pub center: Point3<f64>,
    /// Unit normal of its plane.
    pub normal: Unit<Vector3<f64>>,
    /// Radius (mm).
    pub radius: f64,
}

/// Where a line crosses a face of a body ([`crate::Kernel::ray_hits`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RayHit {
    /// The face crossed.
    pub face: FaceId,
    /// The distance from the ray's origin along its (unit) direction; negative behind it.
    pub t: f64,
    /// Where it crosses.
    pub point: Point3<f64>,
}

/// An axis-aligned box (mm).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    /// The least x, y and z.
    pub min: Point3<f64>,
    /// The greatest x, y and z.
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
    /// A depth along the normal (negative: against it).
    Blind(f64),
    /// Total depth, split evenly on both sides of the plane (Onshape "Symmetric").
    Symmetric(f64),
    /// Two independent directions (Onshape "second end position"): from `-backward` to
    /// `forward` along the normal.
    TwoSided {
        /// Depth along the normal.
        forward: f64,
        /// Depth against the normal.
        backward: f64,
    },
}

/// A boolean operation ([`crate::Kernel::boolean`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoolOp {
    /// Onshape "Add".
    Union,
    /// Onshape "Remove".
    Subtract,
    /// Onshape "Intersect".
    Intersect,
}

/// An infinite line: a point on it and its unit direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Axis {
    /// A point on the axis.
    pub origin: Point3<f64>,
    /// Its direction (turns are counter-clockwise seen from its tip).
    pub dir: Unit<Vector3<f64>>,
}

/// A chamfer's size (P3.6, PS14.7).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ChamferSpec {
    /// The same distance (mm) on both faces.
    EqualDistance(f64),
    /// The first distance on the edge's first face, the second on the other (mm). The first
    /// face is the one whose boundary runs along the edge (as Onshape's); along a tangent chain
    /// of edges, the face on the same side as the first edge's.
    TwoDistances(f64, f64),
    /// A distance on the first face and the chamfer's angle to it (radians).
    DistanceAngle {
        /// Distance on the first face (mm).
        distance: f64,
        /// Angle between the chamfer and the first face (radians).
        angle: f64,
    },
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
    Conic {
        /// The conic's shoulder, in (0, 1).
        rho: f64,
    },
    /// A curvature-continuous (G2) section: a quintic Bezier whose curvature is zero where it
    /// meets the faces. `magnitude` (0 < m ≤ 1) is how far towards the corner its inner poles
    /// reach.
    Curvature {
        /// How far the inner poles reach towards the corner, in (0, 1].
        magnitude: f64,
    },
    /// (P3.10, PS14.6 Asymmetric) A section tangent to both faces whose contact lines are the
    /// circular fillet's for the size on the edge's first face and for `second` on its other
    /// face (`flip` swaps them): the conic with the circle's weight `sin(θ/2)`, which for faces
    /// at 90° is the quarter ellipse with those semi-axes. Straight edges between flat faces.
    Asymmetric {
        /// The size on the edge's other face (mm).
        second: f64,
        /// Swap the two sizes.
        flip: bool,
    },
}

/// A fillet (P3.6, PS14.1–14.5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FilletSpec {
    /// Radius or width.
    pub size: FilletSize,
    /// Onshape's "Allow edge overflow": when off, a fillet that would run onto a face other
    /// than the faces meeting at its edge's ends is refused.
    pub allow_overflow: bool,
    /// The cross section (circular by default).
    #[serde(default)]
    pub profile: FilletProfile,
    /// Tangent propagation: the edges' tangent chains are filleted too (on by default). Off,
    /// only the edges are, stopping square where they run on smoothly into others.
    #[serde(default = "yes")]
    pub propagate: bool,
}

fn yes() -> bool {
    true
}

impl FilletSpec {
    /// A circular fillet of radius `r`, overflow allowed, tangent edges propagated.
    pub fn radius(r: f64) -> Self {
        Self {
            size: FilletSize::Radius(r),
            allow_overflow: true,
            profile: FilletProfile::Circular,
            propagate: true,
        }
    }
}

/// Where a chamfer's distances are measured (P3.6, PS14.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChamferMeasure {
    /// Along each face from the edge: exact between faces straight across the edge (planes, a
    /// cylinder's rim) and round a cylinder running along a straight edge. Where a face curves
    /// across the edge otherwise (a cone along its line, a sphere, a torus), and along a tangent
    /// chain any edge of which is such, the local chamfer puts its contacts where the ball whose
    /// contacts on the faces' tangent planes lie that far touches the faces: off by more the
    /// more a face curves within the distance (were it used on a cylinder of radius 10 meeting a
    /// plane at 127°, 1 would land 1.07 along the plane and 1.20 round the cylinder). The
    /// tool-and-boolean fallback measures round the faces.
    #[default]
    Offset,
    /// Along the faces' tangent planes at the edge, from where they meet. The same as Offset
    /// on a face that is straight across the edge (a plane, a cylinder's rim); on a face that
    /// curves across it (a sphere, a cylinder along a straight edge) of radius R a tangent
    /// distance d lands at the angle `atan(d/R)` round it from the edge: the arc `R atan(d/R)`
    /// (the chord `2R sin(atan(d/R)/2)`).
    Tangent,
}

/// A chamfer with its options (P3.6, PS14.7–14.9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamferOpts {
    /// The size.
    pub spec: ChamferSpec,
    /// How the distances are measured.
    pub measurement: ChamferMeasure,
    /// The first distance (and the angle) go on the edge's other face.
    pub flip: bool,
    /// Edges flipped on their own (Onshape's "Direction overrides"), on top of `flip`.
    pub flipped: Vec<EdgeId>,
}

impl ChamferOpts {
    /// `spec` measured along the faces, nothing flipped.
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
    /// The walls' corners extend sharp (faces meet where their offsets intersect: SMLib's
    /// `bExtendCorners`, OCCT's intersection join) instead of rounded (OCCT's default arc join:
    /// outward, a cylinder round each convex edge and a ball at each corner; inward, the cavity
    /// rounded at the body's concave edges).
    #[serde(default)]
    pub sharp: bool,
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
    /// mm: the total length of the body's wires (Phase 1b; 0 without wires). The centroid of a
    /// body with neither volume nor area is its wires' (by length), else its points' mean.
    #[serde(default)]
    pub length: f64,
}

impl MassProperties {
    /// The inertia tensor about `point` instead of the centroid (parallel axis theorem):
    /// `I_p = I_c + V (|d|² E − d dᵀ)` with `d = c − p` (unit density).
    pub fn inertia_about(&self, point: Point3<f64>) -> Matrix3<f64> {
        let d = self.center_of_mass - point;
        self.inertia + (Matrix3::identity() * d.norm_squared() - d * d.transpose()) * self.volume
    }
}

/// The kind of surface a face lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceKind {
    /// A plane.
    Plane,
    /// A circular cylinder.
    Cylinder,
    /// A circular cone.
    Cone,
    /// A sphere.
    Sphere,
    /// A torus.
    Torus,
    /// Anything else (B-spline, revolution, extrusion).
    Other,
}

/// What [`crate::Kernel::faces`] reports about a face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceInfo {
    /// Its id (its index in the body's face list).
    pub id: FaceId,
    /// Its surface's kind.
    pub kind: SurfaceKind,
    /// Set for planar faces, so they can be picked as sketch planes.
    pub plane: Option<Plane>,
    /// Exact area (mm²).
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
    /// A straight line.
    Line,
    /// A circle or circular arc.
    Circle,
    /// An ellipse or elliptic arc.
    Ellipse,
    /// Anything else (B-spline).
    Other,
    /// A degenerate edge (a cone's apex): a point.
    Degenerate,
}

/// What [`crate::Kernel::edges`] reports about an edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeInfo {
    /// Its id (its index in the body's edge list).
    pub id: EdgeId,
    /// The faces on either side (the same face twice for a seam, `None` for a free edge).
    pub faces: [Option<FaceId>; 2],
    /// Exact length (mm).
    pub length: f64,
    /// Its curve's kind.
    pub curve: CurveKind,
    /// Where it starts.
    pub start: Point3<f64>,
    /// Where it ends (where it starts for a closed edge).
    pub end: Point3<f64>,
    /// Its middle point (at the middle of its parameter range).
    pub mid: Point3<f64>,
    /// The unit tangent at the start, in the direction the edge runs.
    pub start_tangent: Vector3<f64>,
    /// The unit tangent at the end, in the direction the edge runs.
    pub end_tangent: Vector3<f64>,
    /// The exact circle of a circular edge or arc (P3.4: a revolve axis can be a circular edge,
    /// and Use projects the exact curve).
    #[serde(default)]
    pub circle: Option<Circle3>,
    /// The faces either side meet tangentially along it (within [`crate::TANGENT_ANGLE`]): a
    /// smooth edge, which tangent propagation doesn't run along. (Picked on its own, an edge
    /// whose faces meet at a hair under that is still blended: only one whose faces' normals
    /// agree to a point's tolerance has nothing to take off.) Only with the edges' lengths
    /// ([`crate::Kernel::edges`]).
    #[serde(default)]
    pub smooth: bool,
}

impl EdgeInfo {
    /// True for an edge that ends where it starts (a whole circle).
    pub fn is_closed(&self) -> bool {
        (self.end - self.start).norm() < tol::ROUNDOFF && self.length > tol::ROUNDOFF
    }
}

/// A vertex of a body (its index among the body's vertices, like [`FaceId`] and [`EdgeId`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VertexId(pub u64);

/// What [`crate::Kernel::vertices`] reports about a vertex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VertexInfo {
    /// Its id.
    pub id: VertexId,
    /// Where it is.
    pub point: Point3<f64>,
    /// The edges that end at it.
    pub edges: Vec<EdgeId>,
}

/// How finely to tessellate: the largest distance from a triangle or edge segment to the exact
/// surface (mm), and the largest angle between neighbouring normals (radians).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tessellation {
    /// Largest distance from the mesh to the exact surface (mm), above zero.
    pub deflection: f64,
    /// Largest angle between neighbouring normals (radians), above zero.
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
    /// Vertex positions (mm).
    pub positions: Vec<Point3<f64>>,
    /// Unit outward surface normals, one per position.
    pub normals: Vec<Vector3<f64>>,
    /// Triangles as indices into `positions`, counter-clockwise seen from outside.
    pub indices: Vec<[u32; 3]>,
    /// One entry per triangle.
    pub triangle_faces: Vec<FaceId>,
    /// Edge polylines for drawing black edges and edge picking (a wire body's are its
    /// polylines).
    pub edges: Vec<(EdgeId, Vec<Point3<f64>>)>,
    /// A point body's points (its acorn vertices).
    #[serde(default)]
    pub points: Vec<(VertexId, Point3<f64>)>,
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
    /// The input body.
    pub body: BodyId,
    /// Its face (an id of that body).
    pub face: FaceId,
}

/// Where a new face came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Origin {
    /// Swept by a curve of a profile region. `region` is the region's `source` (its index in
    /// [`Profile::regions`] if it has none); `curve` is the curve's `source`, or
    /// [`unsourced_curve`] of its place in the region.
    ProfileCurve {
        /// The region's id.
        region: u64,
        /// The curve's id.
        curve: u64,
    },
    /// The cap of profile region `region` (as above) where the sweep starts (on the profile
    /// plane for a blind extrude).
    StartCap {
        /// The region's id.
        region: u64,
    },
    /// The cap of profile region `region` where the sweep ends.
    EndCap {
        /// The region's id.
        region: u64,
    },
    /// Made from an edge of an input body (a fillet or chamfer face).
    FromEdge {
        /// The input body.
        body: BodyId,
        /// Its edge.
        edge: EdgeId,
    },
    /// Made from a vertex of an input body (a fillet's corner patch).
    FromVertex {
        /// The input body.
        body: BodyId,
        /// Its vertex.
        vertex: VertexId,
    },
    /// Given besides [`Origin::ProfileCurve`] to a side face of a sweep along a path of
    /// several pieces (its lines and arcs, a tangent run of other curves): the piece, 0, 1, …
    /// along the path, it was swept along. A profile curve sweeps a face along each, as
    /// Onshape's sweep has them (a cylinder, a torus, a cylinder), each a face of its own.
    PathSegment {
        /// The piece's place along the path.
        segment: u32,
    },
}

/// The curve id [`Origin::ProfileCurve`] uses for a profile curve without a `source`: the
/// `index`-th curve of loop `lp` (0 = the outer loop) of its region.
pub fn unsourced_curve(lp: usize, index: usize) -> u64 {
    u64::MAX - ((lp as u64) << 32 | index as u64)
}

/// A rigid motion (rotation then translation).
pub type Transform = Isometry3<f64>;

/// A rigid motion or a reflection (P3.8: patterns and mirrors): `x ↦ linear·x + translation`,
/// `linear` orthonormal (det +1: a rotation, det −1: a reflection). [`Transform`] can't hold a
/// reflection, so the mirror needs this.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    /// The orthonormal linear part.
    pub linear: Matrix3<f64>,
    /// The translation, applied after `linear`.
    pub translation: Vector3<f64>,
}

impl Default for Motion {
    fn default() -> Self {
        Self::identity()
    }
}

impl Motion {
    /// The motion that moves nothing.
    pub fn identity() -> Self {
        Self {
            linear: Matrix3::identity(),
            translation: Vector3::zeros(),
        }
    }

    /// A move by `v`.
    pub fn translation(v: Vector3<f64>) -> Self {
        Self {
            linear: Matrix3::identity(),
            translation: v,
        }
    }

    /// A turn by `angle` (radians, counter-clockwise seen from the tip of `axis.dir`) about
    /// `axis`.
    pub fn rotation(axis: &Axis, angle: f64) -> Self {
        let r = nalgebra::Rotation3::from_axis_angle(&axis.dir, angle).into_inner();
        let o = axis.origin.coords;
        Self {
            linear: r,
            translation: o - r * o,
        }
    }

    /// The reflection in the plane through `point` with unit normal `normal`:
    /// `x ↦ x − 2 n (n·(x − p))`.
    pub fn reflection(point: Point3<f64>, normal: Unit<Vector3<f64>>) -> Self {
        let n = normal.into_inner();
        let linear = Matrix3::identity() - 2.0 * n * n.transpose();
        Self {
            linear,
            translation: 2.0 * n * n.dot(&point.coords),
        }
    }

    /// The rigid motion `t`.
    pub fn from_isometry(t: &Transform) -> Self {
        Self {
            linear: t.rotation.to_rotation_matrix().into_inner(),
            translation: t.translation.vector,
        }
    }

    /// `self` then `next`.
    pub fn then(&self, next: &Motion) -> Motion {
        Motion {
            linear: next.linear * self.linear,
            translation: next.linear * self.translation + next.translation,
        }
    }

    /// The motion that undoes this one.
    pub fn inverse(&self) -> Motion {
        let inv = self.linear.transpose();
        Motion {
            linear: inv,
            translation: -(inv * self.translation),
        }
    }

    /// `p` moved.
    pub fn point(&self, p: &Point3<f64>) -> Point3<f64> {
        Point3::from(self.linear * p.coords + self.translation)
    }

    /// `v` turned (or reflected); translations don't move vectors.
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
        Some(Transform::from_parts(
            nalgebra::Translation3::from(self.translation),
            nalgebra::UnitQuaternion::from_rotation_matrix(&r),
        ))
    }

    /// The 3 × 4 matrix `[linear | translation]` row by row.
    pub fn rows(&self) -> [f64; 12] {
        let (m, t) = (&self.linear, &self.translation);
        [
            m[(0, 0)],
            m[(0, 1)],
            m[(0, 2)],
            t.x,
            m[(1, 0)],
            m[(1, 1)],
            m[(1, 2)],
            t.y,
            m[(2, 0)],
            m[(2, 1)],
            m[(2, 2)],
            t.z,
        ]
    }
}

/// Where a point is relative to a solid ([`crate::Kernel::classify`], P3.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointClass {
    /// Strictly inside the material.
    Inside,
    /// Outside the material.
    Outside,
    /// On a face, within the tolerance.
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
    /// A point on the neutral plane.
    pub neutral: Point3<f64>,
    /// The neutral plane's normal when it isn't `pull` (a plane faces are turned about at an
    /// angle to the pull: each face hinges on its line on that plane).
    #[serde(default)]
    pub neutral_normal: Option<Unit<Vector3<f64>>>,
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

/// A face's new surface for [`Kernel::replace_faces`](crate::Kernel::replace_faces).
#[derive(Debug, Clone, PartialEq)]
pub struct FaceReplacement {
    /// The face to change.
    pub face: FaceId,
    /// Its new surface (the native kernel's exact surface type).
    pub surface: crate::native::surface::Surface,
    /// The part of the surface the face lies on, in the surface's own parameters
    /// `((u0, u1), (v0, v1))` (radians for angles): where the surface meets a neighbour more
    /// than once (a cylinder crossing a side's plane twice), each edge is the crossing inside
    /// it. None: the crossings nearest the old edges.
    pub domain: Option<((f64, f64), (f64, f64))>,
}

/// A variable radius along one edge (P3.10, PS14.6 Variable fillet): `(t, r)` pairs, `t` in
/// [0, 1] from the edge's `start` to its `end`, at least one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilletLaw {
    /// The edge.
    pub edge: EdgeId,
    /// `(t, r)` pairs: radius `r` (mm) at fraction `t` of the edge, interpolated between.
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
    /// Thickness along the faces' normals (mm).
    pub along: f64,
    /// Thickness against the faces' normals (mm).
    pub against: f64,
    /// The caller's id for the new faces.
    pub source: u64,
}

/// One boundary curve of a Fill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FillCurve {
    /// An edge of a body.
    Edge {
        /// The edge's body.
        body: BodyId,
        /// The edge.
        edge: EdgeId,
    },
    /// A sketch curve on its plane.
    Sketch {
        /// The sketch plane.
        plane: Plane,
        /// The curve in the plane's coordinates.
        curve: Curve2,
    },
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

/// What a body is (Onshape's body kinds, Phase 1b): its pieces are all solids, all sheets
/// (open shells of faces), all wires (chains of free edges) or all points; `Mixed` holds
/// several kinds (a compound); `Empty` nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BodyClass {
    /// Nothing.
    Empty,
    /// Acorn points only.
    Point,
    /// Wires (chains of free edges) only.
    Wire,
    /// Open shells of faces only.
    Sheet,
    /// Solids only.
    Solid,
    /// More than one kind (a compound).
    Mixed,
}

impl BodyClass {
    /// The kind a sweep of this kind makes (a point sweeps an edge, an edge a face, a face a
    /// solid), if it makes one.
    pub fn swept(self) -> Option<BodyClass> {
        match self {
            Self::Point => Some(Self::Wire),
            Self::Wire => Some(Self::Sheet),
            Self::Sheet => Some(Self::Solid),
            _ => None,
        }
    }
}

/// How [`crate::Kernel::sweep_body`] moves a body: straight along a vector, or turned about an
/// axis by an angle (radians, counter-clockwise seen from the tip of the axis; a whole turn
/// when its size is 2π or more).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BodySweep {
    /// Straight along the vector (mm).
    Translate(Vector3<f64>),
    /// About an axis.
    Rotate {
        /// The axis.
        axis: Axis,
        /// The angle (radians).
        angle: f64,
    },
}

/// A cylindrical box (SMLib's `CreateCylindricalBox`): the piece of a thick-walled tube about
/// `frame`'s normal from `frame`'s origin, `length` long, between `inner_radius` and
/// `outer_radius`, from `start_angle` to `end_angle` (radians, counter-clockwise about the
/// normal from `frame`'s x direction): an annular sector extruded.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CylindricalBox {
    /// Origin on the axis, `normal` along it, `x_dir` where angles start.
    pub frame: Plane,
    /// Length along the normal (mm), above zero.
    pub length: f64,
    /// Inner radius (mm), above zero.
    pub inner_radius: f64,
    /// Outer radius (mm), above `inner_radius`.
    pub outer_radius: f64,
    /// Start angle (radians), in [0, 2π).
    pub start_angle: f64,
    /// End angle (radians), after the start and at most a turn past it.
    pub end_angle: f64,
}
