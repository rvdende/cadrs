//! Mate connectors (P3B.2, `intro-to-assemblies.md` A6.1, A6.4–A6.6, X5): local coordinate
//! systems that mates join.
//!
//! A [`ConnectorFrame`] is an origin, a primary axis **Z** and a secondary axis **X** (Y = Z ×
//! X), in the coordinates of the instance it belongs to (the source part's own coordinates), so
//! it moves with the instance. A [`MateConnector`] is such a frame on an instance, with what
//! defines it ([`ConnectorAnchor`]): today an **implicit** point picked on a face, edge or vertex
//! while a mate dialog is open ([`ImplicitPoint`], by the persistent names of P3.2, so it
//! re-resolves after the Part Studio is edited: [`resolve_implicit`]); the explicit connectors of
//! the Part Studio's Mate connector feature (P3.8, P3B.7) become another anchor kind. The frame
//! last resolved is stored too, as the fallback when the referenced entity is gone.
//!
//! **Implicit points** ([`implicit_points`], the same finder the triad's relocate snap uses):
//!
//! - a planar face: its area **centroid**; the **centres** of its circular edges and holes,
//!   the **midpoints** of its straight edges, its **vertices**, and the **virtual sharps** where
//!   an arc (a fillet or rounded corner) joins two straight edges (the corner the arc rounds
//!   off);
//! - a cylindrical face (a hole or a shaft): the **middle of its axis** (between its end circles:
//!   the centre of the cut, negative, space of a hole) and the centres of its circles;
//! - a circular edge: **only its centre** (A6.6); an arc: its centre and its ends;
//! - a straight edge: its midpoint and its ends; a vertex: itself.
//!
//! **Axes**: on a planar face (and the midpoints, vertices and sharps picked on it) Z is the
//! face's outward normal; on a circle or a cylinder Z is its axis, pointing the same way for
//! every circle on that axis (the component of largest size positive), so a rod's end and a
//! hole's edge line up without a flip; on a straight edge picked by itself, Z is the normal of
//! its first planar face and X runs along the edge. Otherwise X is the model X laid onto the
//! plane normal to Z (else the model Y).

use cadrs_sketch::Vec3;
use nalgebra::{Matrix3, Vector3};
use serde::{Deserialize, Serialize};

use super::{InstanceId, Pose};
use crate::solid::{EdgeName, FaceName, Solid, SolidEdge, VertexName};

/// A local coordinate system: origin, primary axis Z and secondary axis X (unit, orthogonal).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ConnectorFrame {
    pub origin: Vec3,
    pub z: Vec3,
    pub x: Vec3,
}

impl Default for ConnectorFrame {
    fn default() -> Self {
        Self { origin: [0.0; 3], z: [0.0, 0.0, 1.0], x: [1.0, 0.0, 0.0] }
    }
}

fn v(a: Vec3) -> Vector3<f64> {
    Vector3::from(a)
}

fn a3(v: Vector3<f64>) -> Vec3 {
    [v.x, v.y, v.z]
}

/// X for a frame whose Z is `z`: the model X laid onto the plane normal to Z, else the model Y.
pub fn x_for(z: Vec3) -> Vec3 {
    let z = v(z);
    for a in [Vector3::x(), Vector3::y()] {
        let x = a - z * a.dot(&z);
        if x.norm() > 0.2 {
            return a3(x.normalize());
        }
    }
    a3(z.cross(&Vector3::x()).try_normalize(1e-12).unwrap_or(Vector3::y()))
}

/// The direction `n` or its opposite, whichever has its largest component positive (Z wins ties,
/// then Y), so every circle on one axis gives the same Z.
pub fn canonical_axis(n: Vec3) -> Vec3 {
    let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
    let k = if az >= ax.max(ay) - 1e-9 {
        2
    } else if ay >= ax - 1e-9 {
        1
    } else {
        0
    };
    if n[k] < 0.0 { [-n[0], -n[1], -n[2]] } else { n }
}

impl ConnectorFrame {
    /// A frame at `origin` with Z along `z` and X along `x` made normal to Z (if `x` is nearly
    /// along Z, [`x_for`] is used instead).
    pub fn new(origin: Vec3, z: Vec3, x: Vec3) -> Self {
        let zn = v(z).try_normalize(1e-300).unwrap_or(Vector3::z());
        let xv = v(x) - zn * v(x).dot(&zn);
        let xn = if xv.norm() > 1e-6 { xv.normalize() } else { v(x_for(a3(zn))) };
        Self { origin, z: a3(zn), x: a3(xn) }
    }

    pub fn y(&self) -> Vec3 {
        a3(v(self.z).cross(&v(self.x)))
    }

    /// The rotation whose columns are X, Y, Z.
    pub fn rotation(&self) -> Matrix3<f64> {
        Matrix3::from_columns(&[v(self.x), v(self.y()), v(self.z)])
    }

    /// The frame as a placement: its own coordinates to its parent's.
    pub fn pose(&self) -> Pose {
        let r = self.rotation();
        Pose {
            rotation: [[r[(0, 0)], r[(0, 1)], r[(0, 2)]], [r[(1, 0)], r[(1, 1)], r[(1, 2)]], [r[(2, 0)], r[(2, 1)], r[(2, 2)]]],
            translation: self.origin,
        }
    }

    /// The frame of a placement (its columns as X, Y, Z).
    pub fn of_pose(p: &Pose) -> Self {
        let r = p.rotation_matrix();
        Self { origin: p.translation, z: a3(r.column(2).into()), x: a3(r.column(0).into()) }
    }

    /// The frame moved by `pose` (e.g. from an instance's coordinates to the assembly's).
    pub fn moved(&self, pose: &Pose) -> Self {
        Self { origin: pose.apply(self.origin), z: pose.rotate(self.z), x: pose.rotate(self.x) }
    }

    /// **Flip primary axis** (A6.8): Z reversed (a half turn about X).
    pub fn flipped(&self) -> Self {
        Self { z: a3(-v(self.z)), ..*self }
    }

    /// **Reorient secondary axis** (A6.8): X turned by `quarters` × 90° about Z.
    pub fn reoriented(&self, quarters: u8) -> Self {
        let mut f = *self;
        for _ in 0..quarters % 4 {
            f.x = f.y();
        }
        f
    }

    /// This frame followed by `local` (a placement in this frame's own coordinates): the frame
    /// `local` puts relative to this one.
    pub fn then_local(&self, local: &Pose) -> Self {
        Self::of_pose(&local.then(&self.pose()))
    }
}

/// A face, edge or vertex of a part, by its persistent name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityRef {
    Face(FaceName),
    Edge(EdgeName),
    Vertex(VertexName),
}

/// Which implicit point of an entity (A6.4), by the persistent names of what it is computed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImplicitPoint {
    /// The area centroid of a planar face.
    FaceCentroid(FaceName),
    /// The middle of a cylindrical face's axis, between its end circles (the centre of a hole's
    /// cut space, or of a shaft).
    AxisMiddle(FaceName),
    /// The centre of a circular edge or arc.
    CircleCenter(EdgeName),
    /// The point halfway along an edge.
    EdgeMidpoint(EdgeName),
    Vertex(VertexName),
    /// Where the straight edges on either side of the arc `arc` of the planar face `face` meet
    /// (the corner a fillet rounds off).
    VirtualSharp { face: FaceName, arc: EdgeName },
    /// The centre of a spherical face (a ball, a socket).
    SphereCenter(FaceName),
}

impl ImplicitPoint {
    /// "Centroid", "Center", … (hover text, connector names).
    pub fn describe(&self) -> &'static str {
        match self {
            ImplicitPoint::FaceCentroid(_) => "Centroid",
            ImplicitPoint::AxisMiddle(_) => "Axis center",
            ImplicitPoint::CircleCenter(_) => "Center",
            ImplicitPoint::EdgeMidpoint(_) => "Midpoint",
            ImplicitPoint::Vertex(_) => "Vertex",
            ImplicitPoint::VirtualSharp { .. } => "Virtual sharp",
            ImplicitPoint::SphereCenter(_) => "Center",
        }
    }
}

/// What kind of surface a Tangent mate's entity is (A11), with its size; the frame of a
/// [`ConnectorAnchor::Surface`] connector says where it is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SurfaceKind {
    /// A plane: the frame's origin on it, Z its outward normal.
    Plane,
    /// A cylinder: the frame's origin on its axis, Z along the axis.
    Cylinder { radius: f64 },
    /// A sphere: the frame's origin at its centre.
    Sphere { radius: f64 },
    /// A straight edge: the frame's origin on it, Z along it.
    Line,
    /// A vertex: the frame's origin.
    Point,
}

impl SurfaceKind {
    /// "Face", "Edge", "Vertex" (the dialog's entity rows).
    pub fn noun(self) -> &'static str {
        match self {
            SurfaceKind::Line => "Edge",
            SurfaceKind::Point => "Vertex",
            _ => "Face",
        }
    }
}

/// Identifies an assembly's own explicit mate connector ([`LocalConnector`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LocalConnectorId(pub uuid::Uuid);

impl LocalConnectorId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    pub const fn from_u128(v: u128) -> Self {
        Self(uuid::Uuid::from_u128(v))
    }
}

impl Default for LocalConnectorId {
    fn default() -> Self {
        Self::new()
    }
}

/// What defines a connector.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)] // Stored per mate; the persistent names are what make it large.
pub enum ConnectorAnchor {
    /// An implicit point, picked on `owner` (which sets the axes of vertices and midpoints).
    Implicit { point: ImplicitPoint, owner: EntityRef },
    /// A fixed frame in the instance's coordinates (an instance origin, the assembly Origin;
    /// tests).
    Frame,
    /// A Tangent mate's entity (A11.1): a face, edge or vertex as a surface ([`surface_of`]).
    Surface { entity: EntityRef, kind: SurfaceKind },
    /// An explicit connector of the source Part Studio (P3B.7, A22.2): the Mate connector
    /// feature `feature`, owned by the instance's part, so it travels with the part
    /// ([`Solid::connectors`]).
    Explicit { feature: crate::ids::FeatureId },
    /// An explicit connector of the assembly itself (A22.2: it stays in that assembly):
    /// [`super::Assembly::connectors`], resolved before solving ([`super::resolve_local_connectors`]).
    Local { id: LocalConnectorId },
}

/// The edits of a connector in the mate-connector dialog (P3B.7, A22.6, A23.3–A23.4), applied
/// to its frame in this order: **Between** (the origin moved halfway to the entity `between` of
/// the same part), **Realign** (Z along `primary`'s direction, X along `secondary`'s), Flip and
/// Reorient (the connector's own), then **Move** (`translation` along its own X, Y, Z, then a turn
/// of `rotation` radians about Z). Each realign entity keeps the direction it last resolved to,
/// used when the entity is gone.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ConnectorEdit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub between: Option<EntityRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<(EntityRef, Vec3)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<(EntityRef, Vec3)>,
    #[serde(default)]
    pub translation: Vec3,
    #[serde(default)]
    pub rotation: f64,
}

impl ConnectorEdit {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether Realign has an entity.
    pub fn realigned(&self) -> bool {
        self.primary.is_some() || self.secondary.is_some()
    }

    /// Whether Move has a value.
    pub fn moved(&self) -> bool {
        self.translation != [0.0; 3] || self.rotation != 0.0
    }
}

/// The direction an entity of the part `s` gives for Realign: a straight edge's direction, a
/// circle's axis, a flat face's outward normal, a cylinder's axis.
pub fn entity_direction(s: &Solid, e: &EntityRef) -> Option<Vec3> {
    match e {
        EntityRef::Edge(en) => {
            let edge = s.edge(en)?;
            if let Some(c) = edge.circle {
                return Some(c.normal);
            }
            ends(edge).map(|(a, b)| sub(b, a))
        }
        EntityRef::Face(f) => {
            if let Some(n) = face_normal(s, f) {
                return Some(n);
            }
            match surface_of(s, e)? {
                (fr, SurfaceKind::Cylinder { .. }) => Some(fr.z),
                _ => None,
            }
        }
        EntityRef::Vertex(_) => None,
    }
}

/// The point of the entity `e` of the part `s` that a Between connector at `p` goes halfway to:
/// `p` projected onto a flat face's plane, a cylinder's axis or a straight edge's line; a
/// circle's centre; a vertex.
pub fn between_point(s: &Solid, e: &EntityRef, p: Vec3) -> Option<Vec3> {
    if let EntityRef::Edge(en) = e
        && let Some(c) = s.edge(en)?.circle
    {
        return Some(c.center);
    }
    let (f, kind) = surface_of(s, e)?;
    Some(match kind {
        SurfaceKind::Plane => {
            let n = v(f.z);
            a3(v(p) - n * (v(p) - v(f.origin)).dot(&n))
        }
        SurfaceKind::Cylinder { .. } | SurfaceKind::Line => {
            let d = v(f.z);
            a3(v(f.origin) + d * (v(p) - v(f.origin)).dot(&d))
        }
        SurfaceKind::Sphere { .. } | SurfaceKind::Point => f.origin,
    })
}

/// A mate connector on an instance (A6.1): its anchor, the frame last resolved (instance
/// coordinates) and the in-mate adjustments **Flip primary axis** and **Reorient secondary
/// axis** (A6.8), applied on top.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MateConnector {
    pub instance: InstanceId,
    pub anchor: ConnectorAnchor,
    pub frame: ConnectorFrame,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// Quarter turns of X about Z.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub reorient: u8,
    /// Its edits in the mate-connector dialog (P3B.7, A23.2–A23.4: Edit on a mate's connector).
    #[serde(default, skip_serializing_if = "ConnectorEdit::is_default")]
    pub edit: ConnectorEdit,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

impl MateConnector {
    /// A connector at a fixed frame of an instance.
    pub fn at(instance: InstanceId, frame: ConnectorFrame) -> Self {
        Self { instance, anchor: ConnectorAnchor::Frame, frame, flip: false, reorient: 0, edit: ConnectorEdit::default() }
    }

    /// The assembly's **Origin** as a connector (A16.3: an instance fastened to the origin).
    pub fn origin() -> Self {
        Self::at(super::InstanceId::ORIGIN, ConnectorFrame::default())
    }

    /// Whether it is the assembly's Origin.
    pub fn is_origin(&self) -> bool {
        self.instance == super::InstanceId::ORIGIN
    }

    /// An explicit connector of the source Part Studio: the Mate connector feature `feature`
    /// carried by the instance's part at `frame` (part coordinates).
    pub fn explicit(instance: InstanceId, feature: crate::ids::FeatureId, frame: ConnectorFrame) -> Self {
        Self { instance, anchor: ConnectorAnchor::Explicit { feature }, frame, flip: false, reorient: 0, edit: ConnectorEdit::default() }
    }

    /// A Tangent mate's entity: `kind` at `frame` (instance coordinates, see [`surface_of`]).
    pub fn surface(instance: InstanceId, entity: EntityRef, frame: ConnectorFrame, kind: SurfaceKind) -> Self {
        Self { instance, anchor: ConnectorAnchor::Surface { entity, kind }, frame, flip: false, reorient: 0, edit: ConnectorEdit::default() }
    }

    /// The surface of a Tangent entity.
    pub fn surface_kind(&self) -> Option<SurfaceKind> {
        match self.anchor {
            ConnectorAnchor::Surface { kind, .. } => Some(kind),
            _ => None,
        }
    }

    /// A connector at an implicit point.
    pub fn implicit(instance: InstanceId, p: &ImplicitConnector) -> Self {
        Self {
            instance,
            anchor: ConnectorAnchor::Implicit { point: p.point, owner: p.owner },
            frame: p.frame,
            flip: false,
            reorient: 0,
            edit: ConnectorEdit::default(),
        }
    }

    /// Its anchor's frame in instance coordinates (before any edit), re-resolved on the part
    /// `solid` (the source part now) when given; the stored frame when the entity is gone.
    pub fn base_frame(&self, solid: Option<&Solid>) -> ConnectorFrame {
        match (self.anchor, solid) {
            (ConnectorAnchor::Implicit { point, owner }, Some(s)) => resolve_implicit(s, &point, &owner).unwrap_or(self.frame),
            (ConnectorAnchor::Surface { entity, .. }, Some(s)) => surface_of(s, &entity).map(|(f, _)| f).unwrap_or(self.frame),
            (ConnectorAnchor::Explicit { feature }, Some(s)) => s
                .connectors
                .iter()
                .find(|c| c.feature == feature)
                .map(|c| ConnectorFrame::new(c.frame.origin, c.frame.normal(), c.frame.u))
                .unwrap_or(self.frame),
            _ => self.frame,
        }
    }

    /// P3G.5 (ex-dv4): whether its entity is still on the part `solid` (the source part now):
    /// a fixed frame, the Origin and the assembly's own connectors always are; an implicit
    /// point, a Tangent entity or a source connector must resolve, and a missing part loses
    /// them all.
    pub fn resolves(&self, solid: Option<&Solid>) -> bool {
        if self.is_origin() {
            return true;
        }
        match (self.anchor, solid) {
            (ConnectorAnchor::Frame | ConnectorAnchor::Local { .. }, _) => true,
            (_, None) => false,
            (ConnectorAnchor::Implicit { point, owner }, Some(s)) => resolve_implicit(s, &point, &owner).is_some(),
            (ConnectorAnchor::Surface { entity, .. }, Some(s)) => surface_of(s, &entity).is_some(),
            (ConnectorAnchor::Explicit { feature }, Some(s)) => s.connectors.iter().any(|c| c.feature == feature),
        }
    }

    /// Its frame in instance coordinates, re-resolved on the part `solid` (the source part now)
    /// when given, with its edits, flip and reorient applied (see [`ConnectorEdit`]). Falls back
    /// to the stored frame when the entity is gone.
    pub fn local_frame(&self, solid: Option<&Solid>) -> ConnectorFrame {
        self.adjust_on(solid, self.base_frame(solid))
    }

    /// `f` with this connector's edits (resolved on `solid` when given), flip and reorient.
    pub fn adjust_on(&self, solid: Option<&Solid>, f: ConnectorFrame) -> ConnectorFrame {
        let e = &self.edit;
        let mut f = f;
        if let (Some(b), Some(s)) = (e.between, solid)
            && let Some(q) = between_point(s, &b, f.origin)
        {
            f.origin = a3((v(f.origin) + v(q)) / 2.0);
        }
        let dir = |r: &(EntityRef, Vec3)| solid.and_then(|s| entity_direction(s, &r.0)).unwrap_or(r.1);
        if let Some(p) = &e.primary {
            let z = dir(p);
            f = ConnectorFrame::new(f.origin, z, f.x);
        }
        if let Some(sa) = &e.secondary {
            let x = dir(sa);
            let xv = v(x) - v(f.z) * v(x).dot(&v(f.z));
            if xv.norm() > 1e-9 {
                f = ConnectorFrame::new(f.origin, f.z, a3(xv));
            }
        }
        let f = if self.flip { f.flipped() } else { f };
        let f = f.reoriented(self.reorient);
        if e.moved() {
            let p = Pose::rotation_about([0.0; 3], [0.0, 0.0, 1.0], e.rotation).then(&Pose::translation(e.translation));
            f.then_local(&p)
        } else {
            f
        }
    }

    /// `f` with this connector's edits (those needing the part skipped), flip and reorient.
    pub fn adjust(&self, f: ConnectorFrame) -> ConnectorFrame {
        self.adjust_on(None, f)
    }
}

/// An explicit mate connector of an assembly (P3B.7, A1.2, A22.2: the Mate connector tool in the
/// assembly, Ctrl+M): it stays in the assembly. `connector` is where it is: on an instance (its
/// owner), at an implicit point with its edits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalConnector {
    pub id: LocalConnectorId,
    /// "Mate connector 1".
    pub name: String,
    pub connector: MateConnector,
}

/// An implicit connector point found on an entity: which point, on what, its frame (part
/// coordinates) and, for circle centres, the circle's diameter (the "Diameter: …" readout).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImplicitConnector {
    pub point: ImplicitPoint,
    pub owner: EntityRef,
    pub frame: ConnectorFrame,
    pub diameter: Option<f64>,
}

// ---------------------------------------------------------------------------------------------
// The finder

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dist(a: Vec3, b: Vec3) -> f64 {
    v(sub(a, b)).norm()
}

/// The outward normal of a planar face: its plane's normal, turned to agree with the mesh's
/// (a cut's floor keeps the cutting tool's plane, which faces into the part).
pub(crate) fn face_normal(s: &Solid, f: &FaceName) -> Option<Vec3> {
    let face = s.face(f)?;
    let pl = face.plane?;
    let n = v(pl.u).cross(&v(pl.v)).normalize();
    let mut mesh = Vector3::zeros();
    for t in face.first_triangle..face.first_triangle + face.triangle_count {
        for k in 0..3 {
            if let Some(q) = s.normals.get(s.indices[3 * t + k] as usize) {
                mesh += v(*q);
            }
        }
    }
    Some(a3(if mesh.dot(&n) < 0.0 { -n } else { n }))
}

/// The area centroid of a face (its triangles).
fn area_centroid(s: &Solid, f: &FaceName) -> Option<Vec3> {
    let f = s.face(f)?;
    let mut sum = Vector3::zeros();
    let mut area = 0.0;
    for t in f.first_triangle..f.first_triangle + f.triangle_count {
        let [a, b, c] = [0, 1, 2].map(|k| v(s.positions[s.indices[3 * t + k] as usize]));
        let w = (b - a).cross(&(c - a)).norm() / 2.0;
        sum += (a + b + c) / 3.0 * w;
        area += w;
    }
    (area > 0.0).then(|| a3(sum / area))
}

/// True if the edge is a whole circle (its polyline closes).
fn full_circle(e: &SolidEdge) -> bool {
    match (e.circle, e.points.first(), e.points.last()) {
        (Some(c), Some(a), Some(b)) => e.points.len() > 2 && dist(*a, *b) < 1e-3 * c.radius.max(1e-9),
        _ => false,
    }
}

fn is_straight(e: &SolidEdge) -> bool {
    e.circle.is_none() && e.points.len() >= 2 && {
        let (a, b) = (v(e.points[0]), v(*e.points.last().unwrap()));
        let d = b - a;
        d.norm() > 1e-9 && e.points.iter().all(|p| (v(*p) - a).cross(&d).norm() / d.norm() < 1e-6 * d.norm().max(1.0))
    }
}

/// The ends of an open edge.
fn ends(e: &SolidEdge) -> Option<(Vec3, Vec3)> {
    let (a, b) = (*e.points.first()?, *e.points.last()?);
    (dist(a, b) > 1e-9).then_some((a, b))
}

/// The vertices at a point (within 1e-6 mm).
fn vertices_at(s: &Solid, p: Vec3) -> impl Iterator<Item = &crate::solid::SolidVertex> {
    s.vertices.iter().filter(move |q| dist(q.point, p) < 1e-6)
}

/// The Z of a point on `owner`: a planar face's normal; for an edge or vertex, the normal of its
/// first planar face.
fn owner_z(s: &Solid, owner: &EntityRef) -> Vec3 {
    let faces: Vec<FaceName> = match owner {
        EntityRef::Face(f) => vec![*f],
        EntityRef::Edge(e) => e.faces.to_vec(),
        EntityRef::Vertex(vn) => vn.faces.to_vec(),
    };
    faces.iter().find_map(|f| face_normal(s, f)).unwrap_or([0.0, 0.0, 1.0])
}

/// The frame of an implicit point on the part `s` (part coordinates), and a circle's diameter.
pub fn resolve_implicit(s: &Solid, point: &ImplicitPoint, owner: &EntityRef) -> Option<ConnectorFrame> {
    frame_of(s, point, owner).map(|(f, _)| f)
}

fn circle_frame(e: &SolidEdge) -> Option<(ConnectorFrame, Option<f64>)> {
    let c = e.circle?;
    let z = canonical_axis(c.normal);
    Some((ConnectorFrame::new(c.center, z, x_for(z)), Some(2.0 * c.radius)))
}

fn frame_of(s: &Solid, point: &ImplicitPoint, owner: &EntityRef) -> Option<(ConnectorFrame, Option<f64>)> {
    match point {
        ImplicitPoint::FaceCentroid(f) => {
            let z = face_normal(s, f)?;
            Some((ConnectorFrame::new(area_centroid(s, f)?, z, x_for(z)), None))
        }
        ImplicitPoint::AxisMiddle(f) => {
            let circles: Vec<&SolidEdge> = s.edges.iter().filter(|e| e.name.touches(f) && e.circle.is_some()).collect();
            let first = circles.first()?.circle?;
            let z = canonical_axis(first.normal);
            // The circles' centres along the axis: the middle of the extreme ones.
            let t = |p: Vec3| v(sub(p, first.center)).dot(&v(z));
            let (lo, hi) = circles.iter().map(|e| t(e.circle.unwrap().center)).fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
            let mid = v(first.center) + v(z) * ((lo + hi) / 2.0);
            Some((ConnectorFrame::new(a3(mid), z, x_for(z)), Some(2.0 * first.radius)))
        }
        ImplicitPoint::CircleCenter(e) => circle_frame(s.edge(e)?),
        ImplicitPoint::EdgeMidpoint(e) => {
            let edge = s.edge(e)?;
            let z = owner_z(s, owner);
            let x = ends(edge).map(|(a, b)| sub(b, a)).unwrap_or_else(|| x_for(z));
            Some((ConnectorFrame::new(edge.midpoint(), z, x), None))
        }
        ImplicitPoint::Vertex(vn) => {
            let p = s.vertex(vn)?.point;
            let z = owner_z(s, owner);
            let x = match owner {
                EntityRef::Edge(e) => s.edge(e).and_then(ends).map(|(a, b)| sub(b, a)).unwrap_or_else(|| x_for(z)),
                _ => x_for(z),
            };
            Some((ConnectorFrame::new(p, z, x), None))
        }
        ImplicitPoint::VirtualSharp { face, arc } => {
            let z = face_normal(s, face)?;
            let p = virtual_sharp(s, face, s.edge(arc)?)?;
            Some((ConnectorFrame::new(p, z, x_for(z)), None))
        }
        ImplicitPoint::SphereCenter(f) => match surface_of(s, &EntityRef::Face(*f))? {
            (frame, SurfaceKind::Sphere { radius }) => Some((frame, Some(2.0 * radius))),
            _ => None,
        },
    }
}

/// The corner an arc on a planar face rounds off: where the lines of the straight edges that meet
/// its two ends cross.
fn virtual_sharp(s: &Solid, face: &FaceName, arc: &SolidEdge) -> Option<Vec3> {
    let (p0, p1) = ends(arc)?;
    let line_at = |p: Vec3| -> Option<(Vector3<f64>, Vector3<f64>)> {
        s.edges.iter().filter(|e| e.name.touches(face) && e.name != arc.name && is_straight(e)).find_map(|e| {
            let (a, b) = ends(e)?;
            if dist(a, p) < 1e-6 {
                Some((v(a), (v(b) - v(a)).normalize()))
            } else if dist(b, p) < 1e-6 {
                Some((v(b), (v(a) - v(b)).normalize()))
            } else {
                None
            }
        })
    };
    let (a, da) = line_at(p0)?;
    let (b, db) = line_at(p1)?;
    // The closest points of the two lines (they cross in the face's plane).
    let w = a - b;
    let (aa, ab, bb, aw, bw) = (da.dot(&da), da.dot(&db), db.dot(&db), da.dot(&w), db.dot(&w));
    let den = aa * bb - ab * ab;
    if den.abs() < 1e-9 {
        return None;
    }
    let s1 = (ab * bw - bb * aw) / den;
    Some(a3(a + da * s1))
}

/// A face, edge or vertex of the part `s` as a surface for the Tangent mate (A11.1), in part
/// coordinates: a planar face (origin at its centroid), a cylindrical face (its circular edges
/// share an axis and a radius; origin at the middle of the axis), a spherical face (its mesh
/// fits a sphere), a straight edge, or a vertex. None for anything else.
pub fn surface_of(s: &Solid, e: &EntityRef) -> Option<(ConnectorFrame, SurfaceKind)> {
    match e {
        EntityRef::Face(f) => {
            let face = s.face(f)?;
            if let Some(z) = face_normal(s, f) {
                return Some((ConnectorFrame::new(area_centroid(s, f)?, z, x_for(z)), SurfaceKind::Plane));
            }
            let circles: Vec<&SolidEdge> = s.edges.iter().filter(|e| e.name.touches(f) && e.circle.is_some()).collect();
            if let Some(first) = circles.first().and_then(|e| e.circle) {
                let axis = v(first.normal).normalize();
                let same = circles.iter().all(|e| {
                    let c = e.circle.unwrap();
                    let off = v(sub(c.center, first.center));
                    (c.radius - first.radius).abs() < 1e-6 * first.radius.max(1.0)
                        && v(c.normal).normalize().cross(&axis).norm() < 1e-6
                        && (off - axis * off.dot(&axis)).norm() < 1e-6 * first.radius.max(1.0)
                });
                if same {
                    let (frame, _) = frame_of(s, &ImplicitPoint::AxisMiddle(*f), e)?;
                    return Some((frame, SurfaceKind::Cylinder { radius: first.radius }));
                }
            }
            // A sphere through the face's mesh vertices: |p|² = 2 c·p + k, least squares.
            let mut pts: Vec<Vector3<f64>> = Vec::new();
            for t in face.first_triangle..face.first_triangle + face.triangle_count {
                for k in 0..3 {
                    pts.push(v(s.positions[s.indices[3 * t + k] as usize]));
                }
            }
            if pts.len() < 12 {
                return None;
            }
            let mut ata = nalgebra::Matrix4::<f64>::zeros();
            let mut atb = nalgebra::Vector4::<f64>::zeros();
            for p in &pts {
                let row = nalgebra::Vector4::new(2.0 * p.x, 2.0 * p.y, 2.0 * p.z, 1.0);
                ata += row * row.transpose();
                atb += row * p.norm_squared();
            }
            let sol = ata.lu().solve(&atb)?;
            let c = Vector3::new(sol[0], sol[1], sol[2]);
            let r2 = sol[3] + c.norm_squared();
            if r2 <= 0.0 {
                return None;
            }
            let r = r2.sqrt();
            let worst = pts.iter().map(|p| ((p - c).norm() - r).abs()).fold(0.0f64, f64::max);
            (worst < 1e-3 * r.max(1.0)).then(|| (ConnectorFrame::new(a3(c), [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), SurfaceKind::Sphere { radius: r }))
        }
        EntityRef::Edge(en) => {
            let edge = s.edge(en)?;
            if !is_straight(edge) {
                return None;
            }
            let (a, b) = ends(edge)?;
            let z = sub(b, a);
            Some((ConnectorFrame::new(edge.midpoint(), z, x_for(z)), SurfaceKind::Line))
        }
        EntityRef::Vertex(vn) => Some((ConnectorFrame::new(s.vertex(vn)?.point, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), SurfaceKind::Point)),
    }
}

/// The point of the (unbounded) surface `kind` at `f` nearest to `p` (same coordinates).
pub fn nearest_on_surface(kind: SurfaceKind, f: &ConnectorFrame, p: Vec3) -> Vec3 {
    let (o, z, q) = (v(f.origin), v(f.z), v(p));
    let r = match kind {
        SurfaceKind::Plane => q - z * (q - o).dot(&z),
        SurfaceKind::Cylinder { radius } => {
            let a = o + z * (q - o).dot(&z);
            let d = (q - a).try_normalize(1e-12).unwrap_or_else(|| v(x_for(f.z)));
            a + d * radius
        }
        SurfaceKind::Sphere { radius } => o + (q - o).try_normalize(1e-12).unwrap_or(z) * radius,
        SurfaceKind::Line => o + z * (q - o).dot(&z),
        SurfaceKind::Point => o,
    };
    a3(r)
}

/// How far `p` is from face `f` of the part (its triangles).
pub fn face_distance(s: &Solid, f: &FaceName, p: Vec3) -> f64 {
    let Some(face) = s.face(f) else { return f64::INFINITY };
    let q = v(p);
    let mut best = f64::INFINITY;
    for t in face.first_triangle..face.first_triangle + face.triangle_count {
        let [a, b, c] = [0, 1, 2].map(|k| v(s.positions[s.indices[3 * t + k] as usize]));
        best = best.min(point_triangle(q, a, b, c));
    }
    best
}

/// The distance from `p` to the triangle `a b c`.
fn point_triangle(p: Vector3<f64>, a: Vector3<f64>, b: Vector3<f64>, c: Vector3<f64>) -> f64 {
    let n = (b - a).cross(&(c - a));
    let nn = n.norm_squared();
    if nn > 1e-24 {
        // Inside the triangle's prism: the distance to its plane.
        let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, w)| (w - u).cross(&(p - u)).dot(&n) >= 0.0);
        if inside {
            return (p - a).dot(&n).abs() / nn.sqrt();
        }
    }
    let seg = |u: Vector3<f64>, w: Vector3<f64>| {
        let d = w - u;
        let t = if d.norm_squared() > 0.0 { ((p - u).dot(&d) / d.norm_squared()).clamp(0.0, 1.0) } else { 0.0 };
        (p - (u + d * t)).norm()
    };
    seg(a, b).min(seg(b, c)).min(seg(c, a))
}

/// The mesh normal of face `f` at its vertex nearest `p`.
fn normal_near(s: &Solid, f: &FaceName, p: Vec3) -> Option<Vector3<f64>> {
    let face = s.face(f)?;
    let mut best: Option<(f64, usize)> = None;
    for t in face.first_triangle..face.first_triangle + face.triangle_count {
        for k in 0..3 {
            let i = s.indices[3 * t + k] as usize;
            let d = dist(s.positions[i], p);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, i));
            }
        }
    }
    let (_, i) = best?;
    v(*s.normals.get(i)?).try_normalize(1e-12)
}

/// The faces tangent-continuous with `f` (its smooth neighbours, and theirs), for **Tangent
/// propagation** (A11.2): faces that share an edge along which their normals agree, `f` first.
pub fn tangent_faces(s: &Solid, f: &FaceName) -> Vec<FaceName> {
    let mut out = vec![*f];
    let mut k = 0;
    while k < out.len() {
        let cur = out[k];
        for e in s.edges.iter().filter(|e| e.name.touches(&cur)) {
            let [a, b] = e.name.faces;
            let other = if a == cur { b } else { a };
            if other == cur || out.contains(&other) || s.face(&other).is_none() {
                continue;
            }
            let p = e.midpoint();
            let smooth = match (normal_near(s, &cur, p), normal_near(s, &other, p)) {
                (Some(n1), Some(n2)) => n1.dot(&n2) > 0.999,
                _ => false,
            };
            if smooth {
                out.push(other);
            }
        }
        k += 1;
    }
    out
}

/// The implicit connector points of an entity of the part `s` (A6.4–A6.6), in part coordinates.
pub fn implicit_points(s: &Solid, entity: &EntityRef) -> Vec<ImplicitConnector> {
    let mut points: Vec<ImplicitPoint> = Vec::new();
    match entity {
        EntityRef::Face(f) => {
            let Some(face) = s.face(f) else { return Vec::new() };
            let edges: Vec<&SolidEdge> = s.edges.iter().filter(|e| e.name.touches(f)).collect();
            if face.plane.is_some() {
                points.push(ImplicitPoint::FaceCentroid(*f));
            } else if matches!(surface_of(s, entity), Some((_, SurfaceKind::Sphere { .. }))) {
                points.push(ImplicitPoint::SphereCenter(*f));
            } else if edges.iter().any(|e| e.circle.is_some()) {
                points.push(ImplicitPoint::AxisMiddle(*f));
            }
            for e in &edges {
                if e.circle.is_some() {
                    points.push(ImplicitPoint::CircleCenter(e.name));
                    if face.plane.is_some() && !full_circle(e) && virtual_sharp(s, f, e).is_some() {
                        points.push(ImplicitPoint::VirtualSharp { face: *f, arc: e.name });
                    }
                } else if is_straight(e) || ends(e).is_some() {
                    points.push(ImplicitPoint::EdgeMidpoint(e.name));
                }
            }
            for vx in &s.vertices {
                let on = face.loops.iter().flatten().any(|p| dist(*p, vx.point) < 1e-6)
                    || edges.iter().any(|e| ends(e).is_some_and(|(a, b)| dist(a, vx.point) < 1e-6 || dist(b, vx.point) < 1e-6));
                if on {
                    points.push(ImplicitPoint::Vertex(vx.name));
                }
            }
        }
        EntityRef::Edge(en) => {
            let Some(e) = s.edge(en) else { return Vec::new() };
            if e.circle.is_some() {
                points.push(ImplicitPoint::CircleCenter(*en));
            } else {
                points.push(ImplicitPoint::EdgeMidpoint(*en));
            }
            if !full_circle(e)
                && let Some((a, b)) = ends(e)
            {
                for p in [a, b] {
                    points.extend(vertices_at(s, p).map(|q| ImplicitPoint::Vertex(q.name)));
                }
            }
        }
        EntityRef::Vertex(vn) => points.push(ImplicitPoint::Vertex(*vn)),
    }
    let mut out: Vec<ImplicitConnector> = Vec::new();
    for p in points {
        if let Some((frame, diameter)) = frame_of(s, &p, entity)
            && !out.iter().any(|q| q.point == p)
        {
            out.push(ImplicitConnector { point: p, owner: *entity, frame, diameter });
        }
    }
    out
}

/// Every implicit point of the part (each face's, then the edges' and vertices' not already
/// found), for snapping without an entity under the pointer.
pub fn all_implicit_points(s: &Solid) -> Vec<ImplicitConnector> {
    let mut out: Vec<ImplicitConnector> = Vec::new();
    for f in &s.faces {
        for p in implicit_points(s, &EntityRef::Face(f.name)) {
            if !out.iter().any(|q| dist(q.frame.origin, p.frame.origin) < 1e-6) {
                out.push(p);
            }
        }
    }
    out
}

/// The implicit point of `entity` nearest to `near` (by the metric `distance`, e.g. pixels on
/// screen), within `max`.
pub fn nearest_point(points: &[ImplicitConnector], distance: impl Fn(Vec3) -> f64, max: f64) -> Option<ImplicitConnector> {
    points
        .iter()
        .map(|p| (distance(p.frame.origin), *p))
        .filter(|(d, _)| *d <= max)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_flip_reorient_and_compose() {
        let f = ConnectorFrame::new([1.0, 2.0, 3.0], [0.0, 0.0, 2.0], [1.0, 0.0, 0.5]);
        assert_eq!(f.z, [0.0, 0.0, 1.0]);
        assert_eq!(f.x, [1.0, 0.0, 0.0]);
        assert_eq!(f.y(), [0.0, 1.0, 0.0]);
        let r = f.reoriented(1);
        assert!(dist(r.x, [0.0, 1.0, 0.0]) < 1e-12);
        assert_eq!(f.reoriented(4), f);
        assert_eq!(f.flipped().z, [0.0, 0.0, -1.0]);
        let p = f.pose();
        assert!(dist(p.apply([0.0, 0.0, 1.0]), [1.0, 2.0, 4.0]) < 1e-12);
        let back = ConnectorFrame::of_pose(&p);
        assert!(dist(back.x, f.x) < 1e-12 && dist(back.z, f.z) < 1e-12);
        // An offset of 0.5 along the frame's Z.
        let o = f.then_local(&Pose::translation([0.0, 0.0, 0.5]));
        assert!(dist(o.origin, [1.0, 2.0, 3.5]) < 1e-12);
        assert_eq!(canonical_axis([0.0, 0.0, -1.0]), [0.0, 0.0, 1.0]);
        assert_eq!(canonical_axis([0.0, -1.0, 0.2]), [-0.0, 1.0, -0.2]);
    }
}
