//! Cosmetic threads in the 3D view (P3F.3; `essential-tips.md` T10.2): a tapped hole is modelled
//! as its plain tap drill (no helical geometry, cheap to rebuild), and its thread is drawn on it
//! instead, as a drawing shows it (P3C.8): the thread's major diameter as a ¾ circle at the entry
//! (ISO 6410's open arc) and a fine helix at the thread's pitch down the hole's wall for the
//! thread's length. The thread is also in the hole's callout (its feature name, "M10x1.50 ↧ 20
//! mm") and in drawings' thread display and hole callouts.
//!
//! The threads come from [`cadrs_core::views::threads`] (the axis, entry and length recovered
//! from the hole's edges) and are rebuilt when the parts change.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use cadrs_core::{ElementKind, Feature, Part};

use crate::parts::PartCache;
use crate::{ActiveDocument, AppState};

pub struct ThreadsPlugin;

impl Plugin for ThreadsPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<ThreadGizmos>()
            .init_resource::<CosmeticThreads>()
            .add_systems(Startup, configure)
            .add_systems(
                Update,
                (update_threads, draw_threads).chain().after(crate::parts::PartsSet).run_if(in_state(AppState::Document)),
            );
    }
}

/// The cosmetic threads' lines: drawn against the parts' depth, a little toward the viewer.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct ThreadGizmos;

fn configure(mut store: ResMut<GizmoConfigStore>) {
    use bevy::gizmos::config::GizmoLineJoint;
    let (config, _) = store.config_mut::<ThreadGizmos>();
    config.line.width = 1.3;
    config.line.joints = GizmoLineJoint::Round(4);
    config.depth_bias = -6e-5;
    config.render_layers = RenderLayers::layer(0);
}

/// The polylines of the active studio's cosmetic threads, and what they were made for.
#[derive(Resource, Default)]
pub struct CosmeticThreads {
    key: Option<(cadrs_core::ElementId, u64, usize)>,
    pub lines: Vec<Vec<Vec3>>,
}

/// The lines of one thread: the ¾ arc of the major diameter at the entry, and a helix of
/// `pitch` on the tap drill's wall (just inside it) for the thread length.
pub fn thread_lines(t: &cadrs_drawing::annotation::ThreadInfo, pitch: f64) -> Vec<Vec<Vec3>> {
    let axis = Vec3::new(t.axis[0] as f32, t.axis[1] as f32, t.axis[2] as f32).normalize_or_zero();
    if axis == Vec3::ZERO {
        return Vec::new();
    }
    let c = Vec3::new(t.center[0] as f32, t.center[1] as f32, t.center[2] as f32);
    let u = axis.any_orthonormal_vector();
    let v = axis.cross(u);
    let ring = |r: f32, a: f32| u * (r * a.cos()) + v * (r * a.sin());
    let major = t.major as f32 / 2.0;
    // The entry arc: three quarters, a hair out of the entry face so it isn't buried in it.
    let lift = -axis * 0.02;
    let arc: Vec<Vec3> = (0..=54).map(|i| c + lift + ring(major, i as f32 / 54.0 * 1.5 * std::f32::consts::PI)).collect();
    // The helix just inside the wall (the hole is empty there, so it shows looking in).
    let r = t.minor as f32 / 2.0 * 0.985;
    let len = t.length as f32;
    let pitch = (pitch as f32).clamp(0.2, len.max(0.2));
    let turns = len / pitch;
    let steps = ((turns * 24.0).ceil() as usize).clamp(8, 4000);
    let helix: Vec<Vec3> = (0..=steps)
        .map(|i| {
            let s = i as f32 / steps as f32;
            c + axis * (s * len) + ring(r, s * turns * std::f32::consts::TAU)
        })
        .collect();
    vec![arc, helix]
}

/// Each tapped hole's threads with its pitch (mm).
pub fn threads_of(parts: &[&Part], features: &[Feature]) -> Vec<(cadrs_drawing::annotation::ThreadInfo, f64)> {
    let mut out = Vec::new();
    for f in features {
        let Some(h) = f.hole() else { continue };
        if h.spec.hole_type != cadrs_core::hole::HoleType::Tapped {
            continue;
        }
        let pitch = h.spec.pitch_mm().unwrap_or(1.0);
        // This hole alone, with the sketches its points are on.
        let only: Vec<Feature> = features.iter().filter(|g| g.hole().is_none() || g.id == f.id).cloned().collect();
        for t in cadrs_core::views::threads(parts, &only) {
            out.push((t, pitch));
        }
    }
    out
}

fn update_threads(doc: Option<Res<ActiveDocument>>, cache: Res<PartCache>, mut threads: ResMut<CosmeticThreads>) {
    let Some(el) = doc.as_ref().and_then(|d| d.active_element()) else { return };
    if !matches!(el.kind, ElementKind::PartStudio { .. }) {
        if !threads.lines.is_empty() {
            threads.lines.clear();
            threads.key = None;
        }
        return;
    }
    let key = (el.id, cache.generation, cache.parts.len());
    if threads.key == Some(key) {
        return;
    }
    threads.key = Some(key);
    let features = el.active_features();
    let parts: Vec<&Part> = cache.parts.iter().collect();
    threads.lines = threads_of(&parts, &features).iter().flat_map(|(t, p)| thread_lines(t, *p)).collect();
}

fn draw_threads(threads: Res<CosmeticThreads>, cache: Res<PartCache>, mut gizmos: Gizmos<ThreadGizmos>) {
    if cache.parts.is_empty() {
        return;
    }
    let color = Color::srgb_u8(0x2a, 0x3a, 0x5a);
    for line in &threads.lines {
        gizmos.linestrip(line.iter().copied(), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_is_an_arc_and_a_helix_at_its_pitch() {
        // M10×1.5, 15 long, entering at the origin going down −Z.
        let t = cadrs_drawing::annotation::ThreadInfo { center: [0.0; 3], axis: [0.0, 0.0, -1.0], major: 10.0, minor: 8.5, length: 15.0, through: false };
        let l = thread_lines(&t, 1.5);
        assert_eq!(l.len(), 2);
        // The arc: the major radius, three quarters round.
        assert!(l[0].iter().all(|p| ((p.x * p.x + p.y * p.y).sqrt() - 5.0).abs() < 1e-4));
        // The helix: 10 turns down 15 mm on the wall.
        let (first, last) = (l[1][0], *l[1].last().unwrap());
        assert!((first.z - 0.0).abs() < 1e-5 && (last.z + 15.0).abs() < 1e-4, "{first} {last}");
        assert!(l[1].iter().all(|p| (p.x * p.x + p.y * p.y).sqrt() < 4.25));
        assert_eq!(l[1].len(), 241);
    }
}
