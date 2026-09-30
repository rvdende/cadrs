//! Mate connectors (P3.8, X11, PS15.2, PS24.1, PS26.1, PS27.2): coordinate systems a feature
//! can refer to, as Onshape's.
//!
//! - **Implicit** connectors ([`ConnectorOrigin`]) sit on the geometry: a planar face's centroid
//!   (Z its outward normal), a face of revolution's axis (at the centroid's height), a circular
//!   edge's centre (Z out of the planar face it bounds), a straight edge's midpoint (Z along it),
//!   a vertex or sketch point, or the origin. The app shows them as dots on the face or edge
//!   under the pointer while a field takes connectors.
//! - **Explicit** connectors are the Mate connector feature ([`MateConnectorFeature`]): an
//!   implicit connector moved (X, Y, Z along its own axes), turned about its Z axis, with its
//!   primary (Z) axis flipped or its secondary (X) axis turned by quarter turns. It builds no
//!   body; its frame goes into [`crate::rebuild::Build::connectors`] and shows as a triad (K
//!   toggles them, PS27.7).
//! - A connector is a [`PlaneFrame`]: its origin, X (`u`), Y (`v`) and Z (`u × v`). Holes drill
//!   along −Z from it (PS15.2), a circular pattern turns about Z (PS24.1), a mirror reflects in
//!   its XY plane (PS26.1), a linear pattern or an extrude goes along Z.

use cadrs_sketch::{PlaneFrame, PointId, Vec3};
use serde::{Deserialize, Serialize};

use crate::document::{EdgeRef, FaceRef, Feature, VertexRef};
use crate::ids::FeatureId;
use crate::parts::Part;
use crate::solid::{add, cross, dot, len, scale, sub};

/// Where an implicit mate connector sits.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ConnectorOrigin {
    /// The Part Studio's origin, with its axes.
    Origin,
    /// A planar face's centroid (Z its outward normal), or a face of revolution's axis.
    Face(FaceRef),
    /// A circular edge's centre (Z out of the flat face it bounds), or a straight edge's midpoint
    /// (Z along it).
    Edge(EdgeRef),
    /// A vertex, with the Part Studio's axes.
    Vertex(VertexRef),
    /// A sketch point, with the sketch's axes.
    SketchPoint { sketch: FeatureId, point: PointId },
    /// A sketch curve (P3B.7, A22.8): a circle's or arc's centre, a line's midpoint, with the
    /// sketch's axes (Z its normal). So a sketch places a connector where there is no solid
    /// geometry ("Edge of Hole Positions", `ex4-step8.png`).
    SketchCurve { sketch: FeatureId, curve: cadrs_sketch::CurveId },
}

impl ConnectorOrigin {
    /// The feature it depends on (the one that made the face, edge or vertex, or the sketch).
    pub fn parent(&self) -> Option<FeatureId> {
        match self {
            ConnectorOrigin::Origin => None,
            ConnectorOrigin::Face(f) => Some(FeatureId(f.face.op)),
            ConnectorOrigin::Edge(e) => Some(FeatureId(e.edge.op())),
            ConnectorOrigin::Vertex(v) => Some(v.part.feature),
            ConnectorOrigin::SketchPoint { sketch, .. } | ConnectorOrigin::SketchCurve { sketch, .. } => Some(*sketch),
        }
    }

    /// The part the entity is on (a face's, an edge's or a vertex's; none for the origin and
    /// sketch entities).
    pub fn part(&self) -> Option<crate::ids::PartId> {
        match self {
            ConnectorOrigin::Face(f) => Some(f.part),
            ConnectorOrigin::Edge(e) => Some(e.part),
            ConnectorOrigin::Vertex(v) => Some(v.part),
            _ => None,
        }
    }
}

/// A mate connector a feature refers to: a Mate connector feature, or an implicit one.
// The implicit variant holds a face, edge or vertex reference inline (as `FaceRef` and friends
// do elsewhere), so the enum stays `Copy`; a few hundred bytes per reference is fine.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ConnectorRef {
    Feature(FeatureId),
    Implicit(ConnectorOrigin),
}

impl ConnectorRef {
    /// How a field shows it: an explicit connector's name ("Pattern Axis"), else "Mate
    /// connector" (as Onshape shows an implicit one, `ex5-step5.png`).
    pub fn label(&self, features: &[Feature]) -> String {
        match self {
            ConnectorRef::Feature(f) => features.iter().find(|x| x.id == *f).map_or("Mate connector".into(), |x| x.name.clone()),
            ConnectorRef::Implicit(_) => "Mate connector".into(),
        }
    }

    pub fn parent(&self) -> Option<FeatureId> {
        match self {
            ConnectorRef::Feature(f) => Some(*f),
            ConnectorRef::Implicit(o) => o.parent(),
        }
    }
}

/// How a Mate connector's origin is found (`ex4-step5.png`, `ex4-step8.png`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OriginType {
    /// At the origin entity's implicit connector.
    #[default]
    OnEntity,
    /// Midway between the origin entity and the **Between entity** (A22.4): the origin
    /// entity's point and its projection onto the other entity (a flat face's plane, an axis or
    /// a straight edge's line), or the other entity's point.
    BetweenEntities,
}

impl OriginType {
    pub const ALL: [OriginType; 2] = [OriginType::OnEntity, OriginType::BetweenEntities];

    pub fn label(self) -> &'static str {
        match self {
            OriginType::OnEntity => "On entity",
            OriginType::BetweenEntities => "Between entities",
        }
    }
}

fn yes() -> bool {
    true
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// The Mate connector feature (X11; P3B.7, A22; Onshape's dialog, `ex4-step5.png`: Origin
/// type, Origin entity, Between entity, **Realign** (primary and secondary axes to model
/// references), **Move** (X, Y, Z translation and a rotation about Z), **Owner entity**, and the
/// flip primary / reorient secondary buttons).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MateConnectorFeature {
    #[serde(default, skip_serializing_if = "is_default")]
    pub origin_type: OriginType,
    /// "Origin entity".
    pub origin: Option<ConnectorOrigin>,
    /// "Between entity" (Between entities).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub between: Option<ConnectorOrigin>,
    /// **Realign**: Z along `primary_axis`'s direction, X along `secondary_axis`'s (made square
    /// to Z); either may be empty.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub realign: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_axis: Option<ConnectorOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_axis: Option<ConnectorOrigin>,
    /// **Move**: the offsets below apply (P3.8 documents, which have no such field, had them
    /// always on).
    #[serde(default = "yes")]
    pub move_on: bool,
    /// **Owner entity** (A22.5): the part the connector belongs to, which it travels with into
    /// assemblies. Unchecked or empty, the origin entity's part owns it. (P3.11's "Owner part"
    /// is the same field.)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub owner_on: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<crate::ids::PartId>,
    /// Z (and Y) reversed.
    #[serde(default)]
    pub flip_primary: bool,
    /// Quarter turns of X about Z (0–3).
    #[serde(default)]
    pub reorient: u8,
    /// Translation along the connector's own X, Y, Z (mm), and as typed.
    #[serde(default)]
    pub offset: [f64; 3],
    #[serde(default = "zero_exprs")]
    pub offset_expr: [String; 3],
    /// A turn about Z (degrees), and as typed.
    #[serde(default)]
    pub rotation: f64,
    #[serde(default = "zero_deg")]
    pub rotation_expr: String,
    /// P3.11 (P3.8 judge): "Alignment": an entity whose direction the connector's primary (Z)
    /// axis takes (its X stays as square to it as it can), before the flips and moves.
    #[serde(default)]
    pub alignment: Option<crate::document::DirectionRef>,
}

/// A frame with the same origin as `base`, its Z along `z` and its X `base`'s X made square to
/// it (P3.11, the connector's Alignment).
pub fn aligned(base: PlaneFrame, z: Vec3) -> PlaneFrame {
    let z = unit(z);
    let u = sub(base.u, scale(z, dot(base.u, z)));
    if len(u) < 1e-9 {
        return frame_with_z(base.origin, z);
    }
    let u = unit(u);
    PlaneFrame { origin: base.origin, u, v: cross(z, u) }
}

fn zero_exprs() -> [String; 3] {
    ["0 mm".into(), "0 mm".into(), "0 mm".into()]
}

fn zero_deg() -> String {
    "0 deg".into()
}

impl Default for MateConnectorFeature {
    fn default() -> Self {
        Self {
            origin_type: OriginType::OnEntity,
            between: None,
            realign: false,
            primary_axis: None,
            secondary_axis: None,
            move_on: true,
            owner_on: false,
            owner: None,
            origin: None,
            flip_primary: false,
            reorient: 0,
            offset: [0.0; 3],
            offset_expr: zero_exprs(),
            rotation: 0.0,
            rotation_expr: zero_deg(),
            alignment: None,
        }
    }
}

impl MateConnectorFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.origin.is_none() {
            return Some("Select an origin entity");
        }
        (self.origin_type == OriginType::BetweenEntities && self.between.is_none()).then_some("Select a between entity")
    }

    /// The features it refers to (its entities' makers and sketches).
    pub fn parents(&self) -> Vec<FeatureId> {
        let mut out = Vec::new();
        let between = if self.origin_type == OriginType::BetweenEntities { self.between } else { None };
        let axes = if self.realign { [self.primary_axis, self.secondary_axis] } else { [None, None] };
        for o in [self.origin, between].into_iter().chain(axes).flatten() {
            if let Some(p) = o.parent()
                && !out.contains(&p)
            {
                out.push(p);
            }
        }
        out
    }

    /// The part that owns it (A22.5): the Owner entity when set, else the origin entity's part
    /// (found among `parts` by the entity when its part id has changed).
    pub fn owner_part(&self, parts: &[Part]) -> Option<crate::ids::PartId> {
        if self.owner_on
            && let Some(o) = self.owner
        {
            return Some(o);
        }
        let o = self.origin.as_ref()?;
        match o {
            ConnectorOrigin::Face(f) => find_face(parts, f).map(|(p, _)| p.id),
            ConnectorOrigin::Edge(e) => parts
                .iter()
                .find(|p| p.id == e.part && p.solid.edge(&e.edge).is_some())
                .or_else(|| parts.iter().find(|p| p.solid.edge(&e.edge).is_some()))
                .map(|p| p.id),
            _ => o.part(),
        }
    }

    /// The frame before Flip, Reorient and Move: the origin entity's (or midway to the Between
    /// entity), realigned.
    pub fn base_frame(&self, features: &[Feature], parts: &[Part]) -> Result<PlaneFrame, String> {
        let origin = self.origin.as_ref().ok_or("Select an origin entity")?;
        let mut f = origin_frame(origin, features, parts)?;
        if self.origin_type == OriginType::BetweenEntities {
            let b = self.between.as_ref().ok_or("Select a between entity")?;
            let q = between_point(b, f.origin, features, parts)?;
            f.origin = scale(add(f.origin, q), 0.5);
        }
        if self.realign {
            if let Some(p) = &self.primary_axis {
                let z = unit(origin_direction(p, features, parts)?);
                // Keep X where it was, laid square to the new Z (else the default).
                let x = sub(f.u, scale(z, dot(f.u, z)));
                f = if len(x) > 1e-6 { frame_from(f.origin, z, x) } else { frame_with_z(f.origin, z) };
            }
            if let Some(sa) = &self.secondary_axis {
                let x = origin_direction(sa, features, parts)?;
                let z = unit(f.normal());
                let x = sub(x, scale(z, dot(x, z)));
                if len(x) < 1e-6 {
                    return Err("The secondary axis is along the primary axis".into());
                }
                f = frame_from(f.origin, z, x);
            }
        }
        Ok(f)
    }

    /// Its frame (base, then flip, reorient and move).
    pub fn frame(&self, features: &[Feature], parts: &[Part]) -> Result<PlaneFrame, String> {
        Ok(self.place(self.base_frame(features, parts)?))
    }

    /// Its frame on the base frame of its origin entity.
    pub fn place(&self, base: PlaneFrame) -> PlaneFrame {
        let mut f = base;
        if self.flip_primary {
            // A half turn about X: Z and Y reverse.
            f.v = scale(f.v, -1.0);
        }
        let turn = |f: PlaneFrame, a: f64| {
            let (c, s) = (a.cos(), a.sin());
            PlaneFrame { origin: f.origin, u: add(scale(f.u, c), scale(f.v, s)), v: sub(scale(f.v, c), scale(f.u, s)) }
        };
        f = turn(f, std::f64::consts::FRAC_PI_2 * f64::from(self.reorient % 4));
        if !self.move_on {
            return f;
        }
        let n = f.normal();
        f.origin = add(add(add(f.origin, scale(f.u, self.offset[0])), scale(f.v, self.offset[1])), scale(n, self.offset[2]));
        turn(f, self.rotation.to_radians())
    }
}

/// A frame whose Z is `z` and whose X is the world's X (or Y, when Z is along X) made square to
/// it.
pub fn frame_with_z(origin: Vec3, z: Vec3) -> PlaneFrame {
    let z = unit(z);
    let helper = if z[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u = unit(sub(helper, scale(z, dot(helper, z))));
    let v = cross(z, u);
    PlaneFrame { origin, u, v }
}

/// The frame at `origin` with Z along `z` and X along `x` (square to `z`).
pub fn frame_from(origin: Vec3, z: Vec3, x: Vec3) -> PlaneFrame {
    let z = unit(z);
    let u = unit(sub(x, scale(z, dot(x, z))));
    PlaneFrame { origin, u, v: cross(z, u) }
}

/// The point of `b` that a Between-entities connector at `p` goes halfway to: `p` projected onto
/// a flat face's plane, onto a cylindrical face's axis or a straight edge's line; else `b`'s own
/// point (a circle's centre, a vertex).
pub fn between_point(b: &ConnectorOrigin, p: Vec3, features: &[Feature], parts: &[Part]) -> Result<Vec3, String> {
    let f = origin_frame(b, features, parts)?;
    let onto_plane = |f: &PlaneFrame| {
        let n = unit(f.normal());
        sub(p, scale(n, dot(sub(p, f.origin), n)))
    };
    let onto_line = |f: &PlaneFrame| {
        let d = unit(f.normal());
        add(f.origin, scale(d, dot(sub(p, f.origin), d)))
    };
    Ok(match b {
        ConnectorOrigin::Face(face) => {
            let planar = find_face(parts, face).is_some_and(|(part, i)| part.solid.faces[i].plane.is_some());
            if planar { onto_plane(&f) } else { onto_line(&f) }
        }
        ConnectorOrigin::Edge(e) => {
            let circle = parts.iter().find_map(|q| q.solid.edge(&e.edge)).is_some_and(|x| x.circle.is_some());
            if circle { f.origin } else { onto_line(&f) }
        }
        _ => f.origin,
    })
}

/// The direction an entity gives for **Realign** (A22.6): a straight edge's or sketch line's
/// direction, a circle's or cylinder's axis, a flat face's normal; else its connector's Z.
pub fn origin_direction(o: &ConnectorOrigin, features: &[Feature], parts: &[Part]) -> Result<Vec3, String> {
    if let ConnectorOrigin::SketchCurve { sketch, curve } = o
        && let Some(sk) = features.iter().find(|f| f.id == *sketch).and_then(|f| f.sketch())
        && let (Some(pl), Some(c)) = (sk.plane, sk.geometry.curves.get(*curve))
        && let cadrs_sketch::CurveKind::Line { a, b } = c.kind
    {
        let fr = pl.frame();
        return Ok(sub(fr.to_world(sk.geometry.pos(b)), fr.to_world(sk.geometry.pos(a))));
    }
    Ok(origin_frame(o, features, parts)?.normal())
}

fn unit(v: Vec3) -> Vec3 {
    let l = len(v).max(1e-300);
    scale(v, 1.0 / l)
}

/// The frame of an implicit connector, from the parts (their display meshes carry the kernel's
/// exact centroids, axes and circles) and the sketches.
pub fn origin_frame(o: &ConnectorOrigin, features: &[Feature], parts: &[Part]) -> Result<PlaneFrame, String> {
    let lost = || "The mate connector's entity no longer exists".to_string();
    match o {
        ConnectorOrigin::Origin => Ok(PlaneFrame { origin: [0.0; 3], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] }),
        ConnectorOrigin::Face(f) => {
            let (part, i) = find_face(parts, f).ok_or_else(lost)?;
            let face = &part.solid.faces[i];
            let center = face.center.or_else(|| part.solid.face_point(i)).ok_or_else(lost)?;
            if let Some(pl) = face.plane {
                // Z out of the material: a cut's faces carry the frame of the tool that made
                // them (a pocket floor's points down), so it is checked against the mesh.
                let v = match part.solid.face_normal(i) {
                    Some(n) if dot(n, pl.normal()) < 0.0 => scale(pl.v, -1.0),
                    _ => pl.v,
                };
                return Ok(PlaneFrame { origin: center, u: pl.u, v });
            }
            if let Some((o, d)) = face.axis {
                let at = add(o, scale(d, dot(sub(center, o), d)));
                return Ok(frame_with_z(at, d));
            }
            Err("A mate connector needs a flat face or a face of revolution".into())
        }
        ConnectorOrigin::Edge(e) => {
            let part = parts
                .iter()
                .find(|p| p.id == e.part && p.solid.edge(&e.edge).is_some())
                .or_else(|| parts.iter().find(|p| p.solid.edge(&e.edge).is_some()))
                .ok_or_else(lost)?;
            let edge = part.solid.edge(&e.edge).ok_or_else(lost)?;
            if let Some(c) = edge.circle {
                // Z out of the flat face the circle bounds (a hole's rim on a top face: up).
                let mut z = c.normal;
                for fname in e.edge.faces {
                    if let Some(pl) = part.solid.face(&fname).and_then(|x| x.plane) {
                        let n = pl.normal();
                        if dot(n, z).abs() > 1.0 - 1e-6 {
                            z = if dot(n, z) > 0.0 { z } else { scale(z, -1.0) };
                            break;
                        }
                    }
                }
                return Ok(frame_with_z(c.center, z));
            }
            let (a, b) = (edge.points[0], *edge.points.last().ok_or_else(lost)?);
            Ok(frame_with_z(edge.midpoint(), sub(b, a)))
        }
        ConnectorOrigin::Vertex(v) => {
            let point = parts
                .iter()
                .find_map(|p| p.solid.vertex(&v.vertex))
                .map(|x| x.point)
                .unwrap_or(v.point);
            Ok(PlaneFrame { origin: point, u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] })
        }
        ConnectorOrigin::SketchPoint { sketch, point } => {
            let sk = features.iter().find(|f| f.id == *sketch).and_then(|f| f.sketch()).ok_or_else(lost)?;
            let frame = sk.plane.ok_or_else(lost)?.frame();
            let p = sk.geometry.points.get(*point).ok_or_else(lost)?;
            Ok(PlaneFrame { origin: frame.to_world(p.pos), u: frame.u, v: frame.v })
        }
        ConnectorOrigin::SketchCurve { sketch, curve } => {
            use cadrs_sketch::CurveKind;
            let sk = features.iter().find(|f| f.id == *sketch).and_then(|f| f.sketch()).ok_or_else(lost)?;
            let frame = sk.plane.ok_or_else(lost)?.frame();
            let g = &sk.geometry;
            let c = g.curves.get(*curve).ok_or_else(lost)?;
            let at = match c.kind {
                CurveKind::Line { a, b } => (g.pos(a) + g.pos(b)) * 0.5,
                CurveKind::Circle { center, .. } | CurveKind::Arc { center, .. } => g.pos(center),
                CurveKind::Ellipse { center, .. } | CurveKind::EllipseOffset { center, .. } => g.pos(center),
                #[allow(unreachable_patterns)]
                _ => return Err("A mate connector needs a line, circle or arc of the sketch".into()),
            };
            Ok(PlaneFrame { origin: frame.to_world(at), u: frame.u, v: frame.v })
        }
    }
}

/// The part and display face a face reference is on now (by name, then where it was).
pub fn find_face<'a>(parts: &'a [Part], f: &FaceRef) -> Option<(&'a Part, usize)> {
    let ordered = parts.iter().filter(|p| p.id == f.part).chain(parts.iter().filter(|p| p.id != f.part));
    let candidates: Vec<&Part> = ordered.collect();
    for by_name in [true, false] {
        for p in &candidates {
            if let Ok((i, how)) = p.solid.resolve_face(&f.face, None, Some(f.seed)) {
                if by_name && how == cadrs_kernel::naming::Match::Geometric {
                    continue;
                }
                return Some((p, i));
            }
        }
    }
    None
}

/// The frame of a connector reference: an explicit connector's (from `connectors`, the frames
/// the rebuild found so far) or an implicit one's.
pub fn frame(
    c: &ConnectorRef,
    features: &[Feature],
    parts: &[Part],
    connectors: &std::collections::HashMap<FeatureId, PlaneFrame>,
) -> Result<PlaneFrame, String> {
    match c {
        ConnectorRef::Feature(f) => connectors.get(f).copied().ok_or_else(|| {
            let name = features.iter().find(|x| x.id == *f).map_or("The mate connector".to_string(), |x| x.name.clone());
            format!("{name} has no frame (it failed or is gone)")
        }),
        ConnectorRef::Implicit(o) => origin_frame(o, features, parts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_moves_along_its_own_axes() {
        let base = frame_with_z([1.0, 2.0, 3.0], [0.0, 0.0, 1.0]);
        assert_eq!(base.u, [1.0, 0.0, 0.0]);
        let m = MateConnectorFeature { offset: [1.0, 0.0, 2.0], ..MateConnectorFeature::default() };
        let f = m.place(base);
        assert!(len(sub(f.origin, [2.0, 2.0, 5.0])) < 1e-12);
        // Flipped: Z points down, so +Z offsets go down.
        let m = MateConnectorFeature { flip_primary: true, offset: [0.0, 0.0, 2.0], ..MateConnectorFeature::default() };
        let f = m.place(base);
        assert!(len(sub(f.normal(), [0.0, 0.0, -1.0])) < 1e-12);
        assert!(len(sub(f.origin, [1.0, 2.0, 1.0])) < 1e-12);
        // A quarter turn: X becomes Y.
        let m = MateConnectorFeature { rotation: 90.0, ..MateConnectorFeature::default() };
        let f = m.place(base);
        assert!(len(sub(f.u, [0.0, 1.0, 0.0])) < 1e-12);
        assert!(len(sub(f.normal(), [0.0, 0.0, 1.0])) < 1e-12);
    }

    #[test]
    fn alignment_turns_the_primary_axis() {
        // P3.11: aligned with +X, Z points along X and X (world X, now along Z) falls back to a
        // square one; aligned with a direction tilted in XZ, X stays in the XZ plane.
        let base = frame_with_z([1.0, 2.0, 3.0], [0.0, 0.0, 1.0]);
        let f = aligned(base, [2.0, 0.0, 0.0]);
        assert!(len(sub(f.normal(), [1.0, 0.0, 0.0])) < 1e-12);
        assert!(len(sub(f.origin, [1.0, 2.0, 3.0])) < 1e-12);
        assert!(dot(f.u, f.normal()).abs() < 1e-12 && dot(f.v, f.normal()).abs() < 1e-12);
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let f = aligned(base, [s, 0.0, s]);
        assert!(len(sub(f.normal(), [s, 0.0, s])) < 1e-12);
        assert!(len(sub(f.u, [s, 0.0, -s])) < 1e-12);
    }
}
