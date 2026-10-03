//! Sketch geometry for cadrs. No Bevy here: geometry, constraints, the solver, snapping and
//! hit-testing live in this crate so they can be unit-tested without a window.
//!
//! - The data model: [`Sketch`] with points, curves (lines, circles, arcs, ellipses) and
//!   dimensions, and the sketch-plane frames (world ↔ sketch coordinates).
//! - [`SketchOp`]: every edit the drawing tools make, applied by `cadrs_core`'s command layer.
//! - [`geom`]: vector math and arc construction (3-point and tangent arcs).
//! - [`hit`]: hit-testing and box selection, with tolerances in screen pixels.
//! - [`entity`]: the T3 entity tools' geometry (midpoint line, aligned rectangle, polygon,
//!   slot, fillet, chamfer); ellipses are a curve kind of their own.
//! - [`modify`]: Trim, Extend and Split (S17, S18).
//! - [`region`]: closed-region detection (the grey fill).
//! - [`constraint`]: geometric constraints (created automatically by the tools in M5).
//! - [`infer`]: snapping and inference, a pure function of the cursor and the sketch.
//! - [`dimension`]: the Dimension tool's geometry: which dimension a selection makes, its
//!   value and how it is drawn (M7); [`units`]: values with units and arithmetic.
//! - [`solve`]: the constraint solver, degree-of-freedom analysis and dragging (M6). Driving
//!   dimensions are equations too.

pub mod constraint;
pub mod diagnostics;
pub mod dimension;
pub mod edit;
pub mod entity;
pub mod external;
pub mod face_offset;
pub mod geom;
pub mod hit;
pub mod infer;
pub mod modify;
pub mod ops;
pub mod projection;
pub mod region;
pub mod solve;
pub mod spline;
pub mod text;
pub mod units;

use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};

pub use constraint::{
    Constraint, ConstraintKind, ConstraintOf, ConstraintSpec, CurveRef, CurveSpec, Fit, Orient,
    PointRef, PointSpec,
};
pub use geom::ArcGeom;
pub use hit::{Entity as SketchEntity, Hit};
pub use ops::SketchOp;
pub use region::Region;

new_key_type! {
    /// Identifies a point in a [`Sketch`].
    pub struct PointId;
    /// Identifies a curve in a [`Sketch`].
    pub struct CurveId;
    /// Identifies a dimension in a [`Sketch`].
    pub struct DimensionId;
    /// Identifies a constraint in a [`Sketch`].
    pub struct ConstraintId;
    /// Identifies a text entity (S16) in a [`Sketch`].
    pub struct TextId;
}

/// A 2D vector in sketch-plane coordinates, in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: Self) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

/// A 3D vector in world coordinates (millimetres, Z up).
pub type Vec3 = [f64; 3];

/// Which plane a sketch lies on: a default plane, or a planar face of a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PlaneRef {
    /// The XY plane.
    #[default]
    Top,
    /// The XZ plane.
    Front,
    /// The YZ plane.
    Right,
    /// A planar face of a part (sketch on face).
    Face(FacePlane),
    /// A Plane feature (P3.7, PS12.2), with its frame as of the last regeneration.
    Feature(FeaturePlane),
}

/// A Plane feature used as a sketch plane (or an extrude direction, a split tool, …): the
/// feature and its frame as of the last regeneration (kept so a sketch stays put if the plane
/// fails).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FeaturePlane {
    pub feature: uuid::Uuid,
    pub origin: Vec3,
    pub u: Vec3,
    pub v: Vec3,
}

impl FeaturePlane {
    pub fn new(feature: uuid::Uuid, f: PlaneFrame) -> Self {
        Self {
            feature,
            origin: f.origin,
            u: f.u,
            v: f.v,
        }
    }

    pub fn frame(&self) -> PlaneFrame {
        PlaneFrame {
            origin: self.origin,
            u: self.u,
            v: self.v,
        }
    }
}

impl PartialEq for FeaturePlane {
    fn eq(&self, other: &Self) -> bool {
        let bits = |v: &Vec3| v.map(f64::to_bits);
        self.feature == other.feature
            && bits(&self.origin) == bits(&other.origin)
            && bits(&self.u) == bits(&other.u)
            && bits(&self.v) == bits(&other.v)
    }
}

impl Eq for FeaturePlane {}

impl std::hash::Hash for FeaturePlane {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.feature.hash(state);
        for v in [self.origin, self.u, self.v] {
            v.map(f64::to_bits).hash(state);
        }
    }
}

pub use cadrs_kernel::naming::{EdgeName, FaceName, FaceOrigin, OpId, VertexName};

/// Which face of an extruded solid, as schema v3 documents (and the prism mesh of
/// `cadrs_core::solid`) name it. Documents now store [`FaceName`]s; [`FaceTag::to_name`] maps
/// one to the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FaceTag {
    /// The cap at the far end of the extrusion (the "top" face), of region `region`.
    End { region: u32 },
    /// The cap on the sketch plane (the "bottom" face).
    Start { region: u32 },
    /// The side face swept by a sketch curve (a line gives a planar face, an arc or circle a
    /// cylindrical one).
    Side { region: u32, curve: CurveId },
}

impl FaceTag {
    /// The persistent name of this face of the extrude `op`, with the region's index (in the
    /// extrude's list) standing for its key; the document loader puts the key in.
    pub fn to_name(self, op: OpId) -> FaceName {
        use slotmap::Key;
        FaceName::new(
            op,
            match self {
                FaceTag::End { region } => FaceOrigin::Cap {
                    region: region as u64,
                    end: true,
                },
                FaceTag::Start { region } => FaceOrigin::Cap {
                    region: region as u64,
                    end: false,
                },
                FaceTag::Side { region, curve } => FaceOrigin::Side {
                    region: region as u64,
                    curve: curve.data().as_ffi(),
                },
            },
        )
    }
}

/// Identifies an edge of an extruded solid, as schema v3 documents name it (see [`FaceTag`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeTag {
    /// Where the side face of `curve` meets a cap (`end`: the far cap; otherwise the start).
    Cap {
        region: u32,
        curve: CurveId,
        end: bool,
    },
    /// Along the extrusion, where boundary curve `from` ends and `to` begins.
    Lateral {
        region: u32,
        from: CurveId,
        to: CurveId,
    },
}

impl EdgeTag {
    /// The two faces that meet at the edge.
    pub fn faces(self) -> (FaceTag, FaceTag) {
        match self {
            EdgeTag::Cap { region, curve, end } => (
                FaceTag::Side { region, curve },
                if end {
                    FaceTag::End { region }
                } else {
                    FaceTag::Start { region }
                },
            ),
            EdgeTag::Lateral { region, from, to } => (
                FaceTag::Side { region, curve: from },
                FaceTag::Side { region, curve: to },
            ),
        }
    }

    /// The persistent name of this edge of the extrude `op`: the edge between the two faces.
    /// (A v3 lateral edge also said which way round the region's boundary ran; where two such
    /// edges lie between the same two faces, the first by position is taken, and a stale choice
    /// is corrected by the geometric fallback when the link resolves.)
    pub fn to_name(self, op: OpId) -> EdgeName {
        let (a, b) = self.faces();
        EdgeName::new(a.to_name(op), b.to_name(op), 0)
    }
}

/// Schema v3 documents stored [`FaceTag`]s and [`EdgeTag`]s where v4 stores [`FaceName`]s and
/// [`EdgeName`]s. While [`legacy::read_v3`] runs, those fields read the old form and convert
/// it with the operation left nil and the region as its index in the extrude's list; the
/// loader (`cadrs_core::store`) then fills in the feature the reference names and the region's
/// key ([`Link::map_faces`]).
pub mod legacy {
    use std::cell::Cell;

    use serde::{Deserialize, Deserializer};

    use super::{EdgeName, EdgeTag, FaceName, FaceTag};

    thread_local! {
        static V3: Cell<bool> = const { Cell::new(false) };
    }

    /// Runs `f` (a deserialization) reading v3 face and edge tags.
    pub fn read_v3<R>(f: impl FnOnce() -> R) -> R {
        let before = V3.with(|c| c.replace(true));
        let out = f();
        V3.with(|c| c.set(before));
        out
    }

    fn v3() -> bool {
        V3.with(Cell::get)
    }

    pub fn face<'de, D: Deserializer<'de>>(d: D) -> Result<FaceName, D::Error> {
        if v3() {
            Ok(FaceTag::deserialize(d)?.to_name(uuid::Uuid::nil()))
        } else {
            FaceName::deserialize(d)
        }
    }

    pub fn edge<'de, D: Deserializer<'de>>(d: D) -> Result<EdgeName, D::Error> {
        if v3() {
            Ok(EdgeTag::deserialize(d)?.to_name(uuid::Uuid::nil()))
        } else {
            EdgeName::deserialize(d)
        }
    }
}

/// What a projected ("used", S20) entity or a pierced point is linked to, outside the sketch.
/// The link is stored in a [`ConstraintOf::Use`] or [`ConstraintOf::Pierce`] constraint;
/// `cadrs_core` re-evaluates it whenever the document regenerates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Link {
    /// An edge of the part made by the extrude `feature`.
    Edge {
        feature: uuid::Uuid,
        #[serde(deserialize_with = "legacy::edge")]
        edge: EdgeName,
    },
    /// One of the silhouette lines of a curved face of the part made by `feature`, seen along
    /// the sketch plane's normal (`index`: 0 or 1, in the order found around the face).
    Silhouette {
        feature: uuid::Uuid,
        #[serde(deserialize_with = "legacy::face")]
        face: FaceName,
        index: u8,
    },
    /// A curve of another sketch.
    SketchCurve { feature: uuid::Uuid, curve: CurveId },
    /// Where a plane (a default plane or a Plane feature) cuts the sketch plane (Final re-audit,
    /// S12.10: Normal to a plane): a line, used as construction.
    Plane(PlaneRef),
}

impl Link {
    /// The feature the link depends on.
    pub fn feature(&self) -> uuid::Uuid {
        match *self {
            Link::Edge { feature, .. }
            | Link::Silhouette { feature, .. }
            | Link::SketchCurve { feature, .. } => feature,
            Link::Plane(PlaneRef::Feature(fp)) => fp.feature,
            Link::Plane(PlaneRef::Face(fp)) => fp.feature,
            // A default plane: no feature.
            Link::Plane(_) => uuid::Uuid::nil(),
        }
    }

    /// The link with every face name it holds passed through `f` (with the link's feature).
    pub fn map_faces(self, f: impl Fn(FaceName, uuid::Uuid) -> FaceName) -> Self {
        match self {
            Link::Edge { feature, edge } => Link::Edge {
                feature,
                edge: EdgeName::new(f(edge.faces[0], feature), f(edge.faces[1], feature), edge.index),
            },
            Link::Silhouette {
                feature,
                face,
                index,
            } => Link::Silhouette {
                feature,
                face: f(face, feature),
                index,
            },
            l => l,
        }
    }
}

/// A planar face used as a sketch plane: the feature that made it, which face, and its frame
/// as of the last regeneration (kept so the sketch stays put if the face goes away), with a
/// point on the face then (`seed`), for finding the face again when its name no longer
/// resolves.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FacePlane {
    /// The id of the feature (an extrude) that made the face's part.
    pub feature: uuid::Uuid,
    /// The face's persistent name.
    #[serde(deserialize_with = "legacy::face")]
    pub face: FaceName,
    pub origin: Vec3,
    pub u: Vec3,
    pub v: Vec3,
    /// A point on the face as of the last regeneration (`None` in documents from before P3.2
    /// until they regenerate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Vec3>,
}

impl FacePlane {
    pub fn frame(&self) -> PlaneFrame {
        PlaneFrame {
            origin: self.origin,
            u: self.u,
            v: self.v,
        }
    }

    /// The same face with a new frame.
    pub fn with_frame(self, f: PlaneFrame) -> Self {
        Self {
            origin: f.origin,
            u: f.u,
            v: f.v,
            ..self
        }
    }
}

impl PartialEq for FacePlane {
    fn eq(&self, other: &Self) -> bool {
        let bits = |v: &Vec3| v.map(f64::to_bits);
        self.feature == other.feature
            && self.face == other.face
            && bits(&self.origin) == bits(&other.origin)
            && bits(&self.u) == bits(&other.u)
            && bits(&self.v) == bits(&other.v)
            && self.seed.map(|s| bits(&s)) == other.seed.map(|s| bits(&s))
    }
}

impl Eq for FacePlane {}

impl std::hash::Hash for FacePlane {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.feature.hash(state);
        self.face.hash(state);
        for v in [self.origin, self.u, self.v] {
            v.map(f64::to_bits).hash(state);
        }
        self.seed.map(|s| s.map(f64::to_bits)).hash(state);
    }
}

impl PlaneRef {
    /// The default planes.
    pub const ALL: [PlaneRef; 3] = [PlaneRef::Top, PlaneRef::Front, PlaneRef::Right];

    /// "Top", "Front", "Right" or "Face".
    pub fn name(self) -> &'static str {
        match self {
            PlaneRef::Top => "Top",
            PlaneRef::Front => "Front",
            PlaneRef::Right => "Right",
            PlaneRef::Face(_) => "Face",
            PlaneRef::Feature(_) => "Plane",
        }
    }

    /// How a selection field shows the plane: "Top plane" (a face is named by its part; the
    /// app does that).
    pub fn display_name(self) -> String {
        match self {
            PlaneRef::Face(_) => "Face".into(),
            PlaneRef::Feature(_) => "Plane".into(),
            p => format!("{} plane", p.name()),
        }
    }

    /// The face, if the sketch is on one.
    pub fn face(self) -> Option<FacePlane> {
        match self {
            PlaneRef::Face(f) => Some(f),
            _ => None,
        }
    }

    /// The Plane feature, if it is one (P3.7).
    pub fn feature_plane(self) -> Option<FeaturePlane> {
        match self {
            PlaneRef::Feature(f) => Some(f),
            _ => None,
        }
    }

    /// The plane's sketch coordinate frame.
    pub fn frame(self) -> PlaneFrame {
        // Onshape's default planes, seen from their front side: Top from +Z with X right and Y
        // up; Front from -Y with X right and Z up; Right from +X with Y right and Z up.
        let (u, v) = match self {
            PlaneRef::Top => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            PlaneRef::Front => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            PlaneRef::Right => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            PlaneRef::Face(f) => return f.frame(),
            PlaneRef::Feature(f) => return f.frame(),
        };
        PlaneFrame {
            origin: [0.0; 3],
            u,
            v,
        }
    }
}

/// A sketch plane's coordinate frame: sketch point `(x, y)` is at `origin + x·u + y·v` in the
/// world. `u` and `v` are orthonormal; the plane's normal is `u × v`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlaneFrame {
    pub origin: Vec3,
    pub u: Vec3,
    pub v: Vec3,
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

impl PlaneFrame {
    /// The plane's normal (`u × v`), the side it faces.
    pub fn normal(&self) -> Vec3 {
        let (u, v) = (self.u, self.v);
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    }

    /// Sketch coordinates to world coordinates.
    pub fn to_world(&self, p: Vec2) -> Vec3 {
        let (o, u, v) = (self.origin, self.u, self.v);
        [
            o[0] + p.x * u[0] + p.y * v[0],
            o[1] + p.x * u[1] + p.y * v[1],
            o[2] + p.x * u[2] + p.y * v[2],
        ]
    }

    /// World coordinates to sketch coordinates (the orthogonal projection onto the plane).
    pub fn to_sketch(&self, p: Vec3) -> Vec2 {
        let d = sub(p, self.origin);
        Vec2::new(dot(d, self.u), dot(d, self.v))
    }

    /// Signed distance of a world point from the plane, along the normal.
    pub fn distance(&self, p: Vec3) -> f64 {
        dot(sub(p, self.origin), self.normal())
    }

    /// Where the ray `origin + t·dir` crosses the plane, in sketch coordinates. `None` if the
    /// ray is parallel to the plane. (The view is orthographic, so rays may start behind the
    /// plane; `t` is not restricted.)
    pub fn intersect_ray(&self, origin: Vec3, dir: Vec3) -> Option<Vec2> {
        let denom = dot(dir, self.normal());
        if denom.abs() < 1e-12 {
            return None;
        }
        let t = -self.distance(origin) / denom;
        let hit = [
            origin[0] + dir[0] * t,
            origin[1] + dir[1] * t,
            origin[2] + dir[2] * t,
        ];
        Some(self.to_sketch(hit))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub pos: Vec2,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CurveKind {
    Line {
        a: PointId,
        b: PointId,
    },
    Circle {
        center: PointId,
        radius: f64,
    },
    Arc {
        center: PointId,
        start: PointId,
        end: PointId,
    },
    /// An ellipse (S8): its center, the end of its major semi-axis (a point on the ellipse,
    /// which sets the axis direction and the major radius) and its minor radius, square to the
    /// major axis. Added in T3; older documents simply have none (no schema change).
    Ellipse {
        center: PointId,
        major: PointId,
        minor: f64,
    },
    /// The curve `distance` outside an ellipse (negative: inside; P3.7, X13 "offset of an
    /// ellipse", PS21.2): the ellipse with this center, major point and minor radius, moved
    /// along its normals. The Offset tool makes it from an ellipse (sharing its center and
    /// major point; the minor radius follows the ellipse's and the distance the Offset
    /// dimension, see [`solve::sync_offsets`]), and Use makes one from a part edge that is one.
    EllipseOffset {
        center: PointId,
        major: PointId,
        minor: f64,
        distance: f64,
    },
    /// An interpolated spline (Onshape's `skInterpolatedSpline`): the C2 cubic through its
    /// points, kept in [`Sketch::splines`] under the curve's id ([`spline`]). `start` and `end`
    /// are its first and last points (the same point for a closed spline).
    Spline {
        start: PointId,
        end: PointId,
    },
    /// A cubic Bézier curve (Final re-audit, S12.14: the spline the Curvature constraint joins
    /// with G2 continuity; Onshape's *Bézier curve* under the Spline tool): its ends `a` and
    /// `b`, and its control points `c1` (after `a`) and `c2` (before `b`), drawn as hollow
    /// handles joined to the ends by dashed lines. Older documents simply have none.
    Bezier {
        a: PointId,
        c1: PointId,
        c2: PointId,
        b: PointId,
    },
}

impl CurveKind {
    /// The curve's scalar unknown: a circle's radius or an ellipse's minor radius.
    pub fn scalar(&self) -> Option<f64> {
        match *self {
            CurveKind::Circle { radius, .. } => Some(radius),
            CurveKind::Ellipse { minor, .. } => Some(minor),
            _ => None,
        }
    }

    /// Sets the curve's scalar unknown (see [`CurveKind::scalar`]).
    pub fn set_scalar(&mut self, v: f64) {
        match self {
            CurveKind::Circle { radius, .. } => *radius = v,
            CurveKind::Ellipse { minor, .. } => *minor = v,
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub kind: CurveKind,
    pub construction: bool,
}

/// The sketch axis a distance is measured along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    /// Its unit direction.
    pub fn dir(self) -> Vec2 {
        match self {
            Axis::Horizontal => Vec2::new(1.0, 0.0),
            Axis::Vertical => Vec2::new(0.0, 1.0),
        }
    }
}

/// What a dimension measures.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DimensionKind {
    /// The horizontal (sketch X) distance between two points.
    Horizontal { a: PointId, b: PointId },
    /// The vertical (sketch Y) distance between two points.
    Vertical { a: PointId, b: PointId },
    /// The straight distance between two points.
    Aligned { a: PointId, b: PointId },
    /// A circle's diameter.
    Diameter { curve: CurveId },
    /// An arc's radius.
    Radius { curve: CurveId },
    /// The perpendicular distance from a point (or the origin) to a line (or a sketch axis).
    PointLine { p: PointRef, line: CurveRef },
    /// A diametral dimension (P3.4, X13): twice the perpendicular distance from a point to a
    /// (construction) centreline, drawn from the point to its mirror image across the line and
    /// labelled "Ø", as Onshape does when the label is placed across the centreline (the
    /// diameter a revolve about the line will have there).
    Diametral { p: PointRef, line: CurveRef },
    /// The angle (degrees, 0–180) between two lines, measured between the rays that leave
    /// their intersection along each line's direction (from its first point to its second),
    /// reversed where `flip_*` is set. The quadrant the label was placed in picks the flips
    /// (`reference/onshape/dimension/dimension-anglequadrants.png`).
    Angle {
        a: CurveRef,
        b: CurveRef,
        flip_a: bool,
        flip_b: bool,
    },
    /// The distance from a point to a circle or arc (its whole circle), measured along the line
    /// through the center: to the circle's near side, or its far side (`far`). Which one comes
    /// from where the circle was clicked: on the point's side of the curve gives the near side
    /// (Onshape's "outside"/"inside", `intro-to-sketching.md` S13.4).
    PointCircle {
        p: PointRef,
        circle: CurveId,
        far: bool,
    },
    /// The perpendicular distance from a line to a circle or arc: to its near side, or its far
    /// side (`far`).
    LineCircle {
        line: CurveRef,
        circle: CurveId,
        far: bool,
    },
    /// The distance between two circles or arcs along the line through their centers (or,
    /// with an `axis`, horizontally or vertically: between their left/right or top/bottom
    /// extremes), each taken on its near side (facing the other) or its far side. Concentric
    /// ones (a ring's width) are measured radially, along the label's direction
    /// ([`Dimension::offset`] is its angle).
    CircleCircle {
        a: CurveId,
        b: CurveId,
        far_a: bool,
        far_b: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        axis: Option<Axis>,
    },
    /// How far `target` is offset from `source` (the Offset tool): the distance between two
    /// parallel lines, or the difference of two concentric radii (measured radially along the
    /// label's direction, [`Dimension::offset`]).
    Offset { source: CurveId, target: CurveId },
    /// An ellipse's major or minor axis (S8): its value is the whole axis, twice the semi-axis,
    /// as Onshape draws it across the ellipse (`entity_tools/ellipse-04.png`; the help calls it
    /// the axis diameter).
    EllipseRadius { curve: CurveId, major: bool },
    /// A polygon's side count (S7.4): the polygon built on the construction circle `circle`,
    /// inscribed in it or circumscribed about it. Not an equation: setting its value rebuilds
    /// the polygon with that many sides ([`crate::entity::set_polygon_sides`]).
    Sides { circle: CurveId, inscribed: bool },
}

/// A driving dimension: an equation the solver keeps (M6). The record keeps the value and
/// where its label sits.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub kind: DimensionKind,
    /// The value in millimetres.
    pub value: f64,
    /// Where the dimension line sits: for distances, the signed offset (mm) from the first
    /// measured point, perpendicular to the measured direction (for a horizontal dimension,
    /// along +Y); for radius and diameter, the angle (radians) of the leader; for an angle, the
    /// radius (mm) of its arc.
    pub offset: f64,
    /// Where the value sits along the dimension: for distances, from the middle of the
    /// dimension line along the measured direction (mm); for radius and diameter, how far
    /// beyond the rim (mm; 0 puts it at the default distance). Dragging the label sets it.
    #[serde(default)]
    pub along: f64,
    /// A driven (reference) dimension: it shows the measured value and is not solved. A
    /// dimension placed where it would over-define the sketch is created driven, as Onshape
    /// does (`reference/onshape/dimension.md`, "Over-defining").
    #[serde(default)]
    pub driven: bool,
}

impl Dimension {
    /// A dimension with its label at the default place along it.
    pub fn new(kind: DimensionKind, value: f64, offset: f64) -> Self {
        Self {
            kind,
            value,
            offset,
            along: 0.0,
            driven: false,
        }
    }
}

impl DimensionKind {
    /// True if the dimension refers to this point or curve.
    pub fn uses_point(&self, p: PointId) -> bool {
        match *self {
            DimensionKind::Horizontal { a, b }
            | DimensionKind::Vertical { a, b }
            | DimensionKind::Aligned { a, b } => a == p || b == p,
            DimensionKind::PointLine { p: q, .. }
            | DimensionKind::Diametral { p: q, .. }
            | DimensionKind::PointCircle { p: q, .. } => {
                q == PointRef::Point(p)
            }
            _ => false,
        }
    }

    pub fn uses_curve(&self, c: CurveId) -> bool {
        let c = CurveRef::Curve(c);
        match *self {
            DimensionKind::Diameter { curve } | DimensionKind::Radius { curve } => {
                CurveRef::Curve(curve) == c
            }
            DimensionKind::PointLine { line, .. } | DimensionKind::Diametral { line, .. } => line == c,
            DimensionKind::Angle { a, b, .. } => a == c || b == c,
            DimensionKind::PointCircle { circle, .. } => CurveRef::Curve(circle) == c,
            DimensionKind::LineCircle { line, circle, .. } => {
                line == c || CurveRef::Curve(circle) == c
            }
            DimensionKind::CircleCircle { a, b, .. } => {
                CurveRef::Curve(a) == c || CurveRef::Curve(b) == c
            }
            DimensionKind::Offset { source, target } => {
                CurveRef::Curve(source) == c || CurveRef::Curve(target) == c
            }
            DimensionKind::EllipseRadius { curve, .. } | DimensionKind::Sides { circle: curve, .. } => {
                CurveRef::Curve(curve) == c
            }
            _ => false,
        }
    }

    /// True for a dimension measured radially between concentric curves, whose
    /// [`Dimension::offset`] is the angle of its dimension line rather than a perpendicular
    /// offset.
    pub fn radial(&self, s: &Sketch) -> bool {
        let center = |c: CurveId| match s.curves.get(c).map(|c| c.kind) {
            Some(CurveKind::Circle { center, .. } | CurveKind::Arc { center, .. }) => {
                Some(s.pos(center))
            }
            _ => None,
        };
        match *self {
            DimensionKind::CircleCircle { a, b, .. } | DimensionKind::Offset { source: a, target: b } => {
                match (center(a), center(b)) {
                    (Some(p), Some(q)) => p.distance(q) < 1e-6,
                    // An ellipse and its offset (P3.7): the dimension runs between them along
                    // their shared normal, at the ellipse parameter its offset holds.
                    _ => {
                        matches!(*self, DimensionKind::Offset { .. })
                            && s.ellipse_geom(a).is_some()
                            && s.ellipse_geom(b).is_some()
                    }
                }
            }
            _ => false,
        }
    }

    /// What its value measures (a length in mm or an angle in degrees).
    pub fn quantity(&self) -> units::Quantity {
        match self {
            DimensionKind::Angle { .. } => units::Quantity::Angle,
            DimensionKind::Sides { .. } => units::Quantity::Count,
            _ => units::Quantity::Length,
        }
    }

    /// The points it refers to (for colouring a conflicting dimension's geometry).
    pub fn points(&self) -> Vec<PointId> {
        match *self {
            DimensionKind::Horizontal { a, b }
            | DimensionKind::Vertical { a, b }
            | DimensionKind::Aligned { a, b } => vec![a, b],
            DimensionKind::PointLine {
                p: PointRef::Point(p),
                ..
            }
            | DimensionKind::Diametral {
                p: PointRef::Point(p),
                ..
            }
            | DimensionKind::PointCircle {
                p: PointRef::Point(p),
                ..
            } => vec![p],
            _ => vec![],
        }
    }

    /// The curves it refers to.
    pub fn curves(&self) -> Vec<CurveId> {
        let mut out = Vec::new();
        let mut push = |c: CurveRef| {
            if let CurveRef::Curve(k) = c {
                out.push(k);
            }
        };
        match *self {
            DimensionKind::Diameter { curve } | DimensionKind::Radius { curve } => {
                push(CurveRef::Curve(curve))
            }
            DimensionKind::PointLine { line, .. } | DimensionKind::Diametral { line, .. } => push(line),
            DimensionKind::Angle { a, b, .. } => {
                push(a);
                push(b);
            }
            DimensionKind::PointCircle { circle, .. } => push(CurveRef::Curve(circle)),
            DimensionKind::LineCircle { line, circle, .. } => {
                push(line);
                push(CurveRef::Curve(circle));
            }
            DimensionKind::CircleCircle { a, b, .. }
            | DimensionKind::Offset { source: a, target: b } => {
                push(CurveRef::Curve(a));
                push(CurveRef::Curve(b));
            }
            DimensionKind::EllipseRadius { curve, .. } | DimensionKind::Sides { circle: curve, .. } => {
                push(CurveRef::Curve(curve))
            }
            _ => {}
        }
        out
    }
}

/// A 2D sketch in its plane's coordinates (see [`PlaneFrame`]). Curves refer to shared point
/// IDs, so coincidence at a shared point is structural. The plane itself is a parameter of the
/// sketch feature that owns the geometry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sketch {
    pub points: SlotMap<PointId, Point>,
    pub curves: SlotMap<CurveId, Curve>,
    #[serde(default)]
    pub dimensions: SlotMap<DimensionId, Dimension>,
    #[serde(default)]
    pub constraints: SlotMap<ConstraintId, Constraint>,
    /// What composite entities (polygons, slots, fillets, chamfers) generate between their own
    /// pieces and do not show (Onshape draws no glyphs on them,
    /// `entity_tools/polygon-inscribed-circumscribed.png`).
    #[serde(default, skip_serializing_if = "Quiet::is_empty")]
    pub quiet: Quiet,
    /// Text entities (S16): their string and style, on a construction box of four lines.
    #[serde(default, skip_serializing_if = "SlotMap::is_empty")]
    pub texts: SlotMap<TextId, text::SketchText>,
    /// The edges of the part faces the sketch lies on (S21 imprinting): boundaries for the
    /// region finder, not curves. Kept up to date by `cadrs_core` when the document
    /// regenerates (empty with "Disable imprinting").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imprint: Vec<Imprint>,
    /// Use and Pierce constraints whose source is gone (S20.2): the sketch is in error and the
    /// geometry they hold is flagged. Set when the document regenerates.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub broken: std::collections::BTreeSet<ConstraintId>,
    /// The points and end tangents of each [`CurveKind::Spline`], by curve.
    #[serde(default, skip_serializing_if = "slotmap::SecondaryMap::is_empty")]
    pub splines: slotmap::SecondaryMap<CurveId, spline::SplineData>,
    /// The expressions typed into driving dimensions that name variables (P3F.4:
    /// `#piston_d + #clearance`), kept next to the value they evaluated to (the dimension's
    /// `value`). The document re-evaluates them when a variable changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expressions: Vec<(DimensionId, String)>,
}

/// A boundary imprinted from a part face's edge (S21): see [`Sketch::imprint`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Imprint {
    /// Its id in regions (a synthetic curve id, see [`synthetic_curve`]).
    pub id: CurveId,
    pub shape: ImprintShape,
    /// The part edge it comes from: sketch tools snap to it and use it ([`external`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Link>,
}

/// The shape of an [`Imprint`], in sketch coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ImprintShape {
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
    /// Counter-clockwise from `start_angle` through `sweep` (radians, positive).
    Arc {
        center: Vec2,
        radius: f64,
        start_angle: f64,
        sweep: f64,
    },
}

/// A curve id that no sketch curve has: regions use them for imprinted edges and the pieces
/// of text outlines (`kind` tells the families apart, `n` numbers the members).
pub fn synthetic_curve(kind: u32, n: u32) -> CurveId {
    let idx = 0xC000_0000u64 | (n as u64 & 0x3FFF_FFFF);
    let version = (kind as u64) * 2 + 1;
    CurveId::from(slotmap::KeyData::from_ffi((version << 32) | idx))
}

/// True for an id made by [`synthetic_curve`].
pub fn is_synthetic(c: CurveId) -> bool {
    use slotmap::Key;
    (c.data().as_ffi() & 0xFFFF_FFFF) >= 0xC000_0000
}

/// Constraints without glyphs, and shared points without a coincident glyph (see
/// [`Sketch::quiet`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quiet {
    #[serde(default)]
    pub constraints: std::collections::BTreeSet<ConstraintId>,
    #[serde(default)]
    pub points: std::collections::BTreeSet<PointId>,
}

impl Quiet {
    pub fn is_empty(&self) -> bool {
        self.constraints.is_empty() && self.points.is_empty()
    }
}

impl PartialEq for Sketch {
    fn eq(&self, other: &Self) -> bool {
        self.points.len() == other.points.len()
            && self.curves.len() == other.curves.len()
            && self
                .points
                .iter()
                .all(|(k, v)| other.points.get(k) == Some(v))
            && self
                .curves
                .iter()
                .all(|(k, v)| other.curves.get(k) == Some(v))
            && self.dimensions.len() == other.dimensions.len()
            && self
                .dimensions
                .iter()
                .all(|(k, v)| other.dimensions.get(k) == Some(v))
            && self.constraints.len() == other.constraints.len()
            && self
                .constraints
                .iter()
                .all(|(k, v)| other.constraints.get(k) == Some(v))
            && self.quiet == other.quiet
            && self.texts.len() == other.texts.len()
            && self
                .texts
                .iter()
                .all(|(k, v)| other.texts.get(k) == Some(v))
            && self.imprint == other.imprint
            && self.broken == other.broken
            && self.splines.len() == other.splines.len()
            && self
                .splines
                .iter()
                .all(|(k, v)| other.splines.get(k) == Some(v))
            && self.expressions == other.expressions
    }
}

impl Sketch {
    pub fn new() -> Self {
        Self::default()
    }

    /// The expression a dimension's value came from (P3F.4), if it names variables.
    pub fn expression(&self, id: DimensionId) -> Option<&str> {
        self.expressions.iter().find(|(d, _)| *d == id).map(|(_, e)| e.as_str())
    }

    /// True if the sketch has no geometry.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
            && self.texts.is_empty()
            && self.curves.is_empty()
            && self.dimensions.is_empty()
            && self.constraints.is_empty()
    }

    pub fn add_point(&mut self, pos: Vec2) -> PointId {
        self.points.insert(Point { pos })
    }

    /// Adds a line between two new points and returns its id.
    pub fn add_line(&mut self, a: Vec2, b: Vec2) -> CurveId {
        let a = self.add_point(a);
        let b = self.add_point(b);
        self.curves.insert(Curve {
            kind: CurveKind::Line { a, b },
            construction: false,
        })
    }

    /// Removes a curve, any points no other curve uses, and dimensions that referred to them.
    pub fn remove_curve(&mut self, id: CurveId) -> Option<Curve> {
        let used = self.curve_points(id);
        let curve = self.curves.remove(id)?;
        self.splines.remove(id);
        self.dimensions.retain(|_, d| !d.kind.uses_curve(id));
        self.constraints.retain(|_, c| !c.uses_curve(id));
        for p in used {
            if !self.point_in_use(p) {
                self.remove_point(p);
            }
        }
        Some(curve)
    }

    /// Removes a point, every curve that uses it (and the points those leave unused), and
    /// dimensions that referred to them.
    pub fn remove_point(&mut self, id: PointId) {
        let users: Vec<CurveId> = self
            .curves
            .iter()
            .filter(|(k, c)| self.kind_points(*k, &c.kind).contains(&id))
            .map(|(k, _)| k)
            .collect();
        self.points.remove(id);
        self.dimensions.retain(|_, d| !d.kind.uses_point(id));
        self.constraints.retain(|_, c| !c.uses_point(id));
        for c in users {
            self.remove_curve(c);
        }
    }

    /// True if a curve uses the point.
    pub fn point_in_use(&self, p: PointId) -> bool {
        self.curves
            .iter()
            .any(|(k, c)| self.kind_points(k, &c.kind).contains(&p))
    }

    /// True for an ellipse's major point that nothing else uses: an internal handle, not drawn
    /// or picked (Onshape shows only an ellipse's center, `entity_tools/ellipse-04.png`).
    pub fn hidden_point(&self, p: PointId) -> bool {
        let mut ellipse = false;
        for (id, c) in &self.curves {
            match c.kind {
                CurveKind::Ellipse { major, .. } | CurveKind::EllipseOffset { major, .. } if major == p => ellipse = true,
                k if self.kind_points(id, &k).contains(&p) => return false,
                _ => {}
            }
        }
        ellipse
            && !self.constraints.values().any(|c| c.uses_point(p))
            && !self.dimensions.values().any(|d| d.kind.uses_point(p))
    }

    /// Points no curve uses that are held by two or more point-on-curve or midpoint
    /// constraints: a fillet's or chamfer's virtual sharp, a polygon's tangent points. Onshape
    /// draws them as hollow circles (`entity_tools/sketchfilletvertexexample.png`).
    pub fn hollow_point(&self, p: PointId) -> bool {
        if self.point_in_use(p) {
            return false;
        }
        let r = PointRef::Point(p);
        self.constraints
            .values()
            .filter(|c| {
                matches!(c, ConstraintOf::PointOnCurve(q, _) | ConstraintOf::Midpoint(q, _) if *q == r)
            })
            .count()
            >= 2
    }

    /// A fillet's or chamfer's corner (a *virtual sharp*): a point on two lines' extensions
    /// (two point-on-curve constraints to lines) that no regular curve uses.
    pub fn virtual_sharp(&self, p: PointId) -> bool {
        if self
            .curves
            .iter()
            .any(|(k, c)| !c.construction && self.kind_points(k, &c.kind).contains(&p))
        {
            return false;
        }
        let r = PointRef::Point(p);
        self.constraints
            .values()
            .filter(|c| match c {
                ConstraintOf::PointOnCurve(q, CurveRef::Curve(l)) if *q == r => matches!(
                    self.curves.get(*l).map(|c| c.kind),
                    Some(CurveKind::Line { .. })
                ),
                _ => false,
            })
            .count()
            >= 2
    }

    /// A fillet arc's virtual sharp: the corner both lines it joins still pass through.
    pub fn fillet_sharp(&self, arc: CurveId) -> Option<PointId> {
        let CurveKind::Arc { start, end, .. } = self.curves.get(arc)?.kind else {
            return None;
        };
        let line_at = |p: PointId| {
            self.curves_at(p)
                .find(|k| *k != arc && matches!(self.curves[*k].kind, CurveKind::Line { .. }))
        };
        let (l1, l2) = (line_at(start)?, line_at(end)?);
        self.points.keys().find(|p| {
            let r = PointRef::Point(*p);
            let on = |l: CurveId| {
                self.constraints
                    .values()
                    .any(|c| *c == ConstraintOf::PointOnCurve(r, CurveRef::Curve(l)))
            };
            on(l1) && on(l2)
        })
    }

    /// The curves that use a point.
    pub fn curves_at(&self, p: PointId) -> impl Iterator<Item = CurveId> + '_ {
        self.curves
            .iter()
            .filter(move |(k, c)| self.kind_points(*k, &c.kind).contains(&p))
            .map(|(k, _)| k)
    }

    /// The point at `pos` (within `eps` mm), if there is one.
    pub fn point_at(&self, pos: Vec2, eps: f64) -> Option<PointId> {
        self.points
            .iter()
            .filter(|(_, p)| p.pos.distance(pos) <= eps)
            .min_by(|a, b| a.1.pos.distance(pos).total_cmp(&b.1.pos.distance(pos)))
            .map(|(k, _)| k)
    }

    /// The point at `pos` if one is there already (so curves drawn from an existing endpoint
    /// share it), otherwise a new one.
    pub fn ensure_point(&mut self, pos: Vec2) -> PointId {
        self.point_at(pos, MERGE_EPS)
            .unwrap_or_else(|| self.add_point(pos))
    }

    pub fn pos(&self, p: PointId) -> Vec2 {
        self.points.get(p).map_or(Vec2::ZERO, |p| p.pos)
    }

    /// The points a curve uses (line ends; circle center; arc center, start, end).
    pub fn curve_points(&self, id: CurveId) -> Vec<PointId> {
        self.curves
            .get(id)
            .map(|c| self.kind_points(id, &c.kind))
            .unwrap_or_default()
    }

    /// The points curve `id` of kind `kind` uses: a spline's are all its points.
    pub fn kind_points(&self, id: CurveId, kind: &CurveKind) -> Vec<PointId> {
        match kind {
            CurveKind::Spline { .. } => match self.splines.get(id) {
                Some(d) => d.points.clone(),
                None => curve_points(kind),
            },
            _ => curve_points(kind),
        }
    }

    /// A spline's cubic Bézier spans ([`spline`]), or `None` for other curves.
    pub fn spline_spans(&self, id: CurveId) -> Option<Vec<spline::Bez>> {
        match self.curves.get(id)?.kind {
            CurveKind::Spline { .. } => Some(self.splines.get(id)?.spans(self)),
            _ => None,
        }
    }

    /// Adds a spline through new (or existing, where one is at the spot) points.
    pub fn add_spline(&mut self, pts: &[Vec2], periodic: bool, start_tangent: Option<Vec2>, end_tangent: Option<Vec2>, construction: bool) -> Option<CurveId> {
        if pts.len() < 2 || (periodic && pts.len() < 3) {
            return None;
        }
        let points: Vec<PointId> = pts.iter().map(|p| self.ensure_point(*p)).collect();
        let (start, end) = (points[0], if periodic { points[0] } else { points[points.len() - 1] });
        let id = self.curves.insert(Curve { kind: CurveKind::Spline { start, end }, construction });
        self.splines.insert(id, spline::SplineData { points, periodic, start_tangent: start_tangent.filter(|_| !periodic), end_tangent: end_tangent.filter(|_| !periodic) });
        Some(id)
    }

    /// An arc's geometry (counter-clockwise from start to end).
    pub fn arc_geom(&self, id: CurveId) -> Option<ArcGeom> {
        match self.curves.get(id)?.kind {
            CurveKind::Arc { center, start, end } => {
                Some(ArcGeom::ccw(self.pos(center), self.pos(start), self.pos(end)))
            }
            _ => None,
        }
    }

    /// A curve's end points (lines and arcs), in order.
    pub fn curve_ends(&self, id: CurveId) -> Option<(PointId, PointId)> {
        match self.curves.get(id)?.kind {
            CurveKind::Line { a, b } => Some((a, b)),
            CurveKind::Arc { start, end, .. } => Some((start, end)),
            CurveKind::Spline { start, end } => (start != end).then_some((start, end)),
            CurveKind::Bezier { a, b, .. } => Some((a, b)),
            CurveKind::Circle { .. } | CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => None,
        }
    }

    /// A Bézier curve's geometry.
    pub fn bezier_geom(&self, id: CurveId) -> Option<geom::BezierGeom> {
        match self.curves.get(id)?.kind {
            CurveKind::Bezier { a, c1, c2, b } => {
                Some(geom::BezierGeom::new([self.pos(a), self.pos(c1), self.pos(c2), self.pos(b)]))
            }
            _ => None,
        }
    }

    /// True for a Bézier curve's control point (a handle, not an end).
    pub fn bezier_handle(&self, p: PointId) -> bool {
        self.curves.values().any(|c| matches!(c.kind, CurveKind::Bezier { c1, c2, .. } if c1 == p || c2 == p))
    }

    /// An ellipse's geometry.
    pub fn ellipse_geom(&self, id: CurveId) -> Option<geom::EllipseGeom> {
        match self.curves.get(id)?.kind {
            CurveKind::Ellipse { center, major, minor } => {
                Some(geom::EllipseGeom::new(self.pos(center), self.pos(major), minor))
            }
            CurveKind::EllipseOffset { center, major, minor, distance } => {
                Some(geom::EllipseGeom::new(self.pos(center), self.pos(major), minor).with_offset(distance))
            }
            _ => None,
        }
    }

    /// The unit direction in which a line or arc leaves `p` (one of its ends), heading into
    /// the curve.
    pub fn direction_from(&self, id: CurveId, p: PointId) -> Option<Vec2> {
        match self.curves.get(id)?.kind {
            CurveKind::Line { a, b } => {
                let (from, to) = if a == p { (a, b) } else { (b, a) };
                Some((self.pos(to) - self.pos(from)).normalize())
            }
            CurveKind::Arc { start, .. } => {
                let g = self.arc_geom(id)?;
                Some(if start == p {
                    g.start_tangent()
                } else {
                    -g.end_tangent()
                })
            }
            CurveKind::Spline { start, end } if start != end => {
                let sp = self.spline_spans(id)?;
                Some(if start == p {
                    spline::bez_tangent(sp.first()?, 0.0)
                } else {
                    -spline::bez_tangent(sp.last()?, 1.0)
                })
            }
            CurveKind::Spline { .. } | CurveKind::Circle { .. } | CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => None,
            CurveKind::Bezier { a, .. } => {
                let g = self.bezier_geom(id)?;
                let d = if a == p { g.tangent_at(0.0) } else { -g.tangent_at(1.0) };
                (d.length() > 1e-12).then(|| d.normalize())
            }
        }
    }

    /// Replaces the point `gone` with `keep` everywhere (curves, constraints, dimensions) and
    /// removes it: the two become one point.
    pub fn merge_points(&mut self, keep: PointId, gone: PointId) {
        if keep == gone || !self.points.contains_key(keep) {
            return;
        }
        let swap = |p: PointId| if p == gone { keep } else { p };
        for c in self.curves.values_mut() {
            c.kind = match c.kind {
                CurveKind::Line { a, b } => CurveKind::Line { a: swap(a), b: swap(b) },
                CurveKind::Circle { center, radius } => CurveKind::Circle {
                    center: swap(center),
                    radius,
                },
                CurveKind::Arc { center, start, end } => CurveKind::Arc {
                    center: swap(center),
                    start: swap(start),
                    end: swap(end),
                },
                CurveKind::Ellipse { center, major, minor } => CurveKind::Ellipse {
                    center: swap(center),
                    major: swap(major),
                    minor,
                },
                CurveKind::EllipseOffset { center, major, minor, distance } => CurveKind::EllipseOffset {
                    center: swap(center),
                    major: swap(major),
                    minor,
                    distance,
                },
                CurveKind::Spline { start, end } => CurveKind::Spline { start: swap(start), end: swap(end) },
                CurveKind::Bezier { a, c1, c2, b } => CurveKind::Bezier {
                    a: swap(a),
                    c1: swap(c1),
                    c2: swap(c2),
                    b: swap(b),
                },
            };
        }
        for d in self.splines.values_mut() {
            for p in &mut d.points {
                *p = swap(*p);
            }
        }
        let touched: Vec<ConstraintId> = self
            .constraints
            .iter()
            .filter(|(_, c)| c.uses_point(gone))
            .map(|(k, _)| k)
            .collect();
        for k in touched {
            let mapped = self.constraints[k].map(
                |p| {
                    Some(match p {
                        PointRef::Point(q) => PointRef::Point(swap(q)),
                        o => o,
                    })
                },
                Some,
            );
            match mapped {
                Some(m) if !m.is_trivial() && !self.constraints.values().any(|x| *x == m) => {
                    self.constraints[k] = m;
                }
                _ => {
                    self.constraints.remove(k);
                }
            }
        }
        for d in self.dimensions.values_mut() {
            d.kind = match d.kind {
                DimensionKind::Horizontal { a, b } => DimensionKind::Horizontal { a: swap(a), b: swap(b) },
                DimensionKind::Vertical { a, b } => DimensionKind::Vertical { a: swap(a), b: swap(b) },
                DimensionKind::Aligned { a, b } => DimensionKind::Aligned { a: swap(a), b: swap(b) },
                DimensionKind::PointLine { p: PointRef::Point(p), line } => DimensionKind::PointLine {
                    p: PointRef::Point(swap(p)),
                    line,
                },
                DimensionKind::Diametral { p: PointRef::Point(p), line } => DimensionKind::Diametral {
                    p: PointRef::Point(swap(p)),
                    line,
                },
                DimensionKind::PointCircle { p: PointRef::Point(p), circle, far } => {
                    DimensionKind::PointCircle {
                        p: PointRef::Point(swap(p)),
                        circle,
                        far,
                    }
                }
                k => k,
            };
        }
        if self.quiet.points.remove(&gone) {
            self.quiet.points.insert(keep);
        }
        self.points.remove(gone);
    }

    /// The length of a line, or `None` if `id` is not a line.
    pub fn line_length(&self, id: CurveId) -> Option<f64> {
        match self.curves.get(id)?.kind {
            CurveKind::Line { a, b } => Some(self.points[a].pos.distance(self.points[b].pos)),
            _ => None,
        }
    }
}

/// Points closer than this (mm) are the same point when curves are added.
pub const MERGE_EPS: f64 = 1e-6;

pub(crate) fn curve_points(kind: &CurveKind) -> Vec<PointId> {
    match *kind {
        CurveKind::Line { a, b } => vec![a, b],
        CurveKind::Circle { center, .. } => vec![center],
        CurveKind::Arc { center, start, end } => vec![center, start, end],
        CurveKind::Ellipse { center, major, .. } | CurveKind::EllipseOffset { center, major, .. } => vec![center, major],
        CurveKind::Spline { start, end } => if start == end { vec![start] } else { vec![start, end] },
        CurveKind::Bezier { a, c1, c2, b } => vec![a, c1, c2, b],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_remove_line() {
        let mut s = Sketch::new();
        let l = s.add_line(Vec2::ZERO, Vec2::new(3.0, 4.0));
        assert_eq!(s.line_length(l), Some(5.0));
        assert_eq!(s.points.len(), 2);
        s.remove_curve(l);
        assert!(s.curves.is_empty());
        assert!(s.points.is_empty());
        assert!(s.is_empty());
    }

    #[test]
    fn shared_points_survive_removal() {
        let mut s = Sketch::default();
        let a = s.add_point(Vec2::ZERO);
        let b = s.add_point(Vec2::new(1.0, 0.0));
        let c = s.add_point(Vec2::new(1.0, 1.0));
        let l1 = s.curves.insert(Curve {
            kind: CurveKind::Line { a, b },
            construction: false,
        });
        s.curves.insert(Curve {
            kind: CurveKind::Line { a: b, b: c },
            construction: false,
        });
        s.remove_curve(l1);
        assert!(!s.points.contains_key(a));
        assert!(s.points.contains_key(b));
        assert!(s.points.contains_key(c));
    }

    fn close3(a: Vec3, b: Vec3) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
    }

    #[test]
    fn plane_frames_match_the_default_planes() {
        assert!(close3(PlaneRef::Top.frame().normal(), [0.0, 0.0, 1.0]));
        assert!(close3(PlaneRef::Front.frame().normal(), [0.0, -1.0, 0.0]));
        assert!(close3(PlaneRef::Right.frame().normal(), [1.0, 0.0, 0.0]));
        assert_eq!(PlaneRef::Front.display_name(), "Front plane");
    }

    #[test]
    fn sketch_world_round_trip() {
        let p = Vec2::new(12.5, -7.25);
        let cases = [
            (PlaneRef::Top, [12.5, -7.25, 0.0]),
            (PlaneRef::Front, [12.5, 0.0, -7.25]),
            (PlaneRef::Right, [0.0, 12.5, -7.25]),
        ];
        for (plane, world) in cases {
            let f = plane.frame();
            assert!(close3(f.to_world(p), world), "{plane:?}");
            assert!(f.to_sketch(world).distance(p) < 1e-9);
            assert!(f.distance(world).abs() < 1e-9);
        }
    }

    #[test]
    fn projection_drops_the_normal_component() {
        let f = PlaneRef::Front.frame();
        // A point 30 mm in front of the Front plane (toward -Y, the side it faces).
        let q = f.to_sketch([4.0, -30.0, 9.0]);
        assert!(q.distance(Vec2::new(4.0, 9.0)) < 1e-9);
        assert!((f.distance([4.0, -30.0, 9.0]) - 30.0).abs() < 1e-9);
    }

    #[test]
    fn rays_hit_the_plane() {
        let f = PlaneRef::Top.frame();
        let hit = f.intersect_ray([3.0, 4.0, 100.0], [0.0, 0.0, -1.0]).unwrap();
        assert!(hit.distance(Vec2::new(3.0, 4.0)) < 1e-9);
        let oblique = f.intersect_ray([0.0, 0.0, 10.0], [1.0, 0.0, -1.0]).unwrap();
        assert!(oblique.distance(Vec2::new(10.0, 0.0)) < 1e-9);
        assert!(f.intersect_ray([0.0, 0.0, 10.0], [1.0, 0.0, 0.0]).is_none());
        let r = PlaneRef::Right.frame();
        let hit = r.intersect_ray([50.0, 2.0, 3.0], [-1.0, 0.0, 0.0]).unwrap();
        assert!(hit.distance(Vec2::new(2.0, 3.0)) < 1e-9);
    }
}
