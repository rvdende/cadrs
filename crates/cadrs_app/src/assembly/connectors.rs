//! Implicit mate connector points in the view (P3B.2, A3.2, A6.4–A6.6; `ex2-step7.png`,
//! `ex2-step16.png`): the points of the face, edge or vertex under the pointer
//! ([`cadrs_core::assembly::connector::implicit_points`], computed on the source part so the
//! connector keeps the part's own names and axes), drawn as small dots, with the one nearest
//! the pointer drawn as a connector glyph (a small triad: red X, green Y, blue Z, and a ring).
//! Holding **Shift** locks the entity, so a point in a tight spot can be reached.
//!
//! The mate dialog ([`super::mate_dialog`]) and the triad's relocate snap ([`super::triad`]) use
//! them.

use bevy::camera::visibility::RenderLayers;
use bevy::gizmos::config::GizmoLineJoint;
use bevy::prelude::*;
use cadrs_core::assembly::connector::{ConnectorFrame, EntityRef, ImplicitConnector, implicit_points};
use cadrs_core::assembly::{InstanceId, Pose};

use crate::ActiveDocument;
use crate::camera::ViewState;
use crate::viewport::Pick;

/// Connector glyphs and point dots: over everything.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct ConnectorGizmos;

/// The dark outline under the dots.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct ConnectorHaloGizmos;

pub fn configure(store: &mut GizmoConfigStore) {
    let (c, _) = store.config_mut::<ConnectorHaloGizmos>();
    c.line.width = 3.6;
    c.line.joints = GizmoLineJoint::Round(4);
    c.depth_bias = -0.99;
    c.render_layers = RenderLayers::layer(crate::viewport::OVERLAY_LAYER);
    let (c, _) = store.config_mut::<ConnectorGizmos>();
    c.line.width = 2.0;
    c.line.joints = GizmoLineJoint::Round(4);
    c.depth_bias = -1.0;
    c.render_layers = RenderLayers::layer(crate::viewport::OVERLAY_LAYER);
}

/// The entity a pick is on, as a connector reference.
pub fn entity_of(p: &Pick) -> Option<(InstanceId, EntityRef)> {
    // A part inside a subassembly: its occurrence (P3B.4).
    let i = super::occurrence_of(p.part()?);
    let e = match p {
        Pick::Face(_, f) => EntityRef::Face(*f),
        Pick::Edge(_, e) => EntityRef::Edge(*e),
        Pick::Vertex(_, v) => EntityRef::Vertex(*v),
        _ => return None,
    };
    Some((i, e))
}

/// The implicit points of an entity of an instance: in the source part's coordinates, with the
/// instance's placement now (`preview` first) to show them.
#[derive(Debug, Clone)]
pub struct EntityPoints {
    pub instance: InstanceId,
    pub entity: EntityRef,
    pub pose: Pose,
    pub points: Vec<ImplicitConnector>,
}

impl EntityPoints {
    /// A point's frame in assembly coordinates.
    pub fn world(&self, p: &ImplicitConnector) -> ConnectorFrame {
        p.frame.moved(&self.pose)
    }

    /// The point nearest the screen offset `at`.
    pub fn nearest(&self, view: &ViewState, at: Vec2) -> Option<ImplicitConnector> {
        self.points
            .iter()
            .map(|p| (view.project(v3(self.world(p).origin)).distance(at), *p))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p)
    }
}

pub fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// The points of `entity` of `instance`, from its source part's current rebuild.
pub fn entity_points(
    doc: &ActiveDocument,
    parts: &mut super::AssemblyParts,
    instance: InstanceId,
    entity: EntityRef,
) -> Option<EntityPoints> {
    let (pose, element, part) = super::occurrence_source(doc, parts, instance)?;
    let pose = parts.preview.get(&instance).copied().unwrap_or(pose);
    let build = parts.build(&doc.doc, element)?;
    let solid = build.part(part)?.solid.clone();
    Some(EntityPoints { instance, entity, pose, points: implicit_points(&solid, &entity) })
}

/// Draws the dots of `points` and, at `glyph`, a connector glyph.
pub fn draw_points(
    halo: &mut Gizmos<ConnectorHaloGizmos>,
    line: &mut Gizmos<ConnectorGizmos>,
    view: &ViewState,
    points: &[Vec3],
) {
    let rot = Quat::from_rotation_arc(Vec3::Z, view.back());
    let s = view.scale;
    for p in points {
        halo.circle(Isometry3d::new(*p, rot), 3.4 * s, Color::srgb_u8(0x22, 0x26, 0x2b)).resolution(16);
        for r in [0.6, 1.3, 2.0, 2.7] {
            line.circle(Isometry3d::new(*p, rot), r * s, Color::WHITE).resolution(14);
        }
    }
}

/// Onshape's connector glyph: a small triad (X red, Y green, Z blue) with a ring about Z.
pub fn draw_glyph(line: &mut Gizmos<ConnectorGizmos>, halo: &mut Gizmos<ConnectorHaloGizmos>, view: &ViewState, f: &ConnectorFrame, ring: Color) {
    let s = view.scale;
    let o = v3(f.origin);
    let (x, z) = (v3(f.x).normalize_or_zero(), v3(f.z).normalize_or_zero());
    let y = z.cross(x);
    let len = 17.0 * s;
    for (a, c) in [(x, Color::srgb_u8(0xe0, 0x2a, 0x2a)), (y, Color::srgb_u8(0x1f, 0xa8, 0x3c)), (z, Color::srgb_u8(0x22, 0x55, 0xe0))] {
        halo.line(o, o + a * len, Color::srgba(1.0, 1.0, 1.0, 0.9));
        line.line(o, o + a * len, c);
    }
    let pts: Vec<Vec3> = (0..=24)
        .map(|k| {
            let t = k as f32 / 24.0 * std::f32::consts::TAU;
            o + (x * t.cos() + y * t.sin()) * 6.0 * s
        })
        .collect();
    for w in pts.windows(2) {
        line.line(w[0], w[1], ring);
    }
}
