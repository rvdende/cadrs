//! What a CAM Studio tab draws in the view besides its parts:
//!
//! - **Toolpaths** of the active job (each visible toolpath's latest result, in model
//!   coordinates): rapids faint red, cuts blue, plunges and ramps green, leads cyan, links yellow,
//!   retracts purple, beam cuts orange. The selected toolpath's lines are wider and brighter; one
//!   that is out of date is grey. While the toolpath dialog is open its live preview is drawn
//!   alone.
//! - **Stock**: the active job's stock, see-through (Show stock on the toolbar).
//! - **Work coordinate systems**: each setup's and each shown work plane's triad (X red, Y green,
//!   Z blue).

use std::collections::HashMap;

use bevy::gizmos::GizmoAsset;
use bevy::gizmos::config::{GizmoLineConfig, GizmoLineJoint};
use bevy::gizmos::retained::Gizmo;
use bevy::prelude::*;
use cadrs_core::cam::cadrs_cam::toolpath::{FeedKind, Segment};
use cadrs_core::cam::cadrs_cam::{Frame, P3};
use cadrs_core::cam::program;
use cadrs_core::cam::{CamId, OpKind};

use super::{CamUi, Computed, Status, active_cam};
use crate::{ActiveDocument, AppState};

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (sync_toolpaths, sync_stock, draw_frames)
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        );
    }
}

/// The colour of a move.
pub fn move_color(s: &Segment, beam: bool) -> Color {
    if s.is_rapid() {
        return Color::srgba_u8(0xe0, 0x3c, 0x31, 0x70);
    }
    if beam && s.beam {
        return Color::srgb_u8(0xff, 0x8a, 0x00);
    }
    match s.kind {
        Some(FeedKind::Plunge) | Some(FeedKind::Ramp) => Color::srgb_u8(0x2e, 0xa0, 0x43),
        Some(FeedKind::LeadIn) | Some(FeedKind::LeadOut) => Color::srgb_u8(0x17, 0xa2, 0xb8),
        Some(FeedKind::Link) => Color::srgb_u8(0xd4, 0xa1, 0x06),
        Some(FeedKind::Retract) => Color::srgb_u8(0x8e, 0x44, 0xad),
        _ => Color::srgb_u8(0x1f, 0x6f, 0xeb),
    }
}

pub fn v3(p: P3) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// Polylines of segments in model coordinates, a run per colour.
pub fn fill_motion(
    asset: &mut GizmoAsset,
    segs: &[Segment],
    frame: &Frame,
    beam: bool,
    tint: Option<Color>,
) {
    let mut run: Vec<Vec3> = Vec::new();
    let mut color: Option<Color> = None;
    let flush = |asset: &mut GizmoAsset, run: &mut Vec<Vec3>, color: Option<Color>| {
        if run.len() >= 2
            && let Some(c) = color
        {
            asset.linestrip(run.drain(..), c);
        }
        run.clear();
    };
    for s in segs {
        if s.from == s.to {
            continue;
        }
        let c = tint.unwrap_or_else(|| move_color(s, beam));
        let a = v3(frame.to_world(s.from));
        let b = v3(frame.to_world(s.to));
        if color != Some(c) || run.last().is_none_or(|l| l.distance(a) > 1e-4) {
            flush(asset, &mut run, color);
            color = Some(c);
            run.push(a);
        }
        run.push(b);
    }
    flush(asset, &mut run, color);
}

/// A drawn toolpath: what it was drawn from.
#[derive(Component)]
struct ToolpathGizmo {
    op: CamId,
    key: (u64, bool, bool, bool),
}

/// Draws the active job's toolpaths (see the module docs).
#[allow(clippy::too_many_arguments)]
fn sync_toolpaths(
    doc: Option<Res<ActiveDocument>>,
    computed: Res<Computed>,
    ui: Res<CamUi>,
    dialog: Option<Res<super::dialogs::CamDialog>>,
    preview: Option<Res<super::preview::PreviewState>>,
    mut assets: ResMut<Assets<GizmoAsset>>,
    q: Query<(Entity, &ToolpathGizmo)>,
    mut commands: Commands,
) {
    let Some(doc) = doc else { return };
    let cam = active_cam(&doc).map(|(_, c)| c);
    // What should be drawn: (op, generation key, selected, stale, preview).
    let mut want: HashMap<CamId, (u64, bool, bool, bool)> = HashMap::new();
    // The back plot draws its own.
    let previewing = preview.is_some();
    if let (Some(cam), false) = (cam, previewing)
        && let Some(job) = cam.active()
    {
        let editing = dialog.as_ref().and_then(|d| match &d.draft {
            super::dialogs::Draft::Toolpath { op, .. } => Some(op.id),
            _ => None,
        });
        // While a toolpath is edited, its live preview alone: clear to see, and never mixed
        // with the others' lines (whose drawing order over it isn't fixed).
        let others = if editing.is_some() { 0 } else { usize::MAX };
        for s in job.setups.iter().filter(|s| !s.suppressed).take(others) {
            for o in &s.operations {
                if o.hidden || o.suppressed || !matches!(o.kind, OpKind::Toolpath(_)) {
                    continue;
                }
                if Some(o.id) == editing {
                    continue;
                }
                if computed.any(o.id).is_none() {
                    continue;
                }
                let stale = !matches!(computed.status(o.id), Status::Done(_));
                want.insert(
                    o.id,
                    (
                        computed.generation,
                        ui.selected.contains(&o.id),
                        stale,
                        false,
                    ),
                );
            }
        }
        // Drawn again for each new preview result (its own key: the studio's generation
        // needn't change between two of them).
        if let Some(op) = editing
            && let Some((pop, Ok(_))) = computed.preview()
            && pop == op
        {
            let key = computed.preview_key().unwrap_or(0) ^ computed.generation;
            want.insert(op, (key, true, false, true));
        }
    }
    // Drop what's no longer wanted or changed.
    let mut have = HashMap::new();
    for (e, g) in &q {
        match want.get(&g.op) {
            Some(k) if *k == g.key => {
                have.insert(g.op, e);
            }
            _ => commands.entity(e).despawn(),
        }
    }
    let Some(cam) = cam else { return };
    // Drawn in a fixed order (the map's isn't): the others first, the selected or previewed
    // one last, over them.
    let mut want: Vec<(CamId, (u64, bool, bool, bool))> = want.into_iter().collect();
    want.sort_by_key(|(op, k)| (k.1, k.3, *op));
    for (op, key) in want {
        if have.contains_key(&op) {
            continue;
        }
        let done = if key.3 {
            match computed.preview() {
                Some((_, Ok(d))) => d.clone(),
                _ => continue,
            }
        } else {
            match computed.any(op) {
                Some(d) => d.clone(),
                None => continue,
            }
        };
        let beam = cam
            .operation(op)
            .and_then(|(j, _, _)| j.machine.as_ref())
            .is_some_and(|m| m.kind.is_cutting());
        let rapid = cam
            .operation(op)
            .and_then(|(j, _, _)| j.machine.as_ref())
            .map_or(5000.0, |m| m.rapid_rate);
        let start = done.toolpath.start().unwrap_or([0.0; 3]);
        let segs = done.toolpath.motion(start, rapid);
        let mut asset = GizmoAsset::default();
        let tint = key.2.then_some(Color::srgb_u8(0x9a, 0xa0, 0xa6));
        fill_motion(&mut asset, &segs, &done.frame, beam, tint);
        let width = if key.1 { 2.4 } else { 1.4 };
        commands.spawn((
            Name::new(format!("cam-toolpath-{:x}", op.0)),
            ToolpathGizmo { op, key },
            Gizmo {
                handle: assets.add(asset),
                line_config: GizmoLineConfig {
                    width,
                    joints: GizmoLineJoint::Round(4),
                    ..default()
                },
                // The selected (or previewed) toolpath over the others, always: with one bias
                // for all, which is drawn over which changed from run to run.
                depth_bias: if key.1 { -0.001 } else { -0.0005 },
            },
            Transform::default(),
            DespawnOnExit(AppState::Document),
        ));
    }
}

#[derive(Component)]
struct StockMesh;

/// The active job's stock, see-through.
#[allow(clippy::too_many_arguments)]
fn sync_stock(
    doc: Option<Res<ActiveDocument>>,
    ui: Res<CamUi>,
    cache: Res<crate::parts::PartCache>,
    preview: Option<Res<super::preview::PreviewState>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    q: Query<Entity, With<StockMesh>>,
    mut last: Local<Option<String>>,
    mut commands: Commands,
) {
    let shape = doc.as_ref().and_then(|d| {
        let (el, cam) = active_cam(d)?;
        let job = cam.active()?;
        if !ui.show_stock || preview.as_ref().is_some_and(|p| p.hides_stock()) {
            return None;
        }
        let (id, _, _) = cache.settled()?;
        if id != el {
            return None;
        }
        program::stock_shape(cam, job, &cache.parts)
    });
    let key = shape.as_ref().map(|s| format!("{s:?}"));
    if *last == key {
        return;
    }
    *last = key;
    for e in &q {
        commands.entity(e).despawn();
    }
    let Some(shape) = shape else { return };
    let tris = shape.triangles();
    let mut pos = Vec::with_capacity(tris.len() * 3);
    let mut nor = Vec::with_capacity(tris.len() * 3);
    for t in &tris {
        let (a, b, c) = (v3(t[0]), v3(t[1]), v3(t[2]));
        let n = (b - a).cross(c - a).normalize_or_zero();
        for p in [a, b, c] {
            pos.push(p.to_array());
            nor.push(n.to_array());
        }
    }
    let mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
    commands.spawn((
        Name::new("cam-stock"),
        StockMesh,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.55, 0.66, 0.82, 0.16),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            depth_bias: -1.0,
            ..default()
        })),
        Transform::default(),
        DespawnOnExit(AppState::Document),
    ));
}

/// A frame's triad at `size` mm.
pub fn triad(g: &mut Gizmos, f: &Frame, size: f32) {
    let o = v3(f.origin);
    g.arrow(o, o + v3(f.x) * size, Color::srgb_u8(0xd9, 0x2d, 0x20));
    g.arrow(o, o + v3(f.y) * size, Color::srgb_u8(0x1e, 0x9e, 0x3a));
    g.arrow(o, o + v3(f.z) * size, Color::srgb_u8(0x1f, 0x5f, 0xd6));
}

/// The setups' and shown work planes' triads, and the stock's outline.
fn draw_frames(
    doc: Option<Res<ActiveDocument>>,
    cache: Res<crate::parts::PartCache>,
    ui: Res<CamUi>,
    dialog: Option<Res<super::dialogs::CamDialog>>,
    mut g: Gizmos,
) {
    let Some(doc) = doc else { return };
    let Some((el, cam)) = active_cam(&doc) else {
        return;
    };
    let Some((id, _, _)) = cache.settled() else {
        return;
    };
    if id != el {
        return;
    }
    let Some(job) = cam.active() else { return };
    let parts = &cache.parts;
    // A triad sized to the stock.
    let size = program::stock_shape(cam, job, parts)
        .map(|s| {
            let b = s.bounds_in(&Frame::WORLD).size();
            (b[0].max(b[1]).max(b[2]) * 0.18).clamp(5.0, 60.0) as f32
        })
        .unwrap_or(20.0);
    // The setup or work plane being edited: its triad as the dialog has it now.
    if let Some(d) = &dialog {
        match &d.draft {
            super::dialogs::Draft::Setup { setup, job, .. } => {
                if let Some(j) = cam.job(*job) {
                    triad(
                        &mut g,
                        &program::work_frame(cam, j, &setup.wcs, parts),
                        size,
                    );
                }
            }
            super::dialogs::Draft::WorkPlane { op, .. } => {
                if let OpKind::WorkPlane(w) = &op.kind {
                    triad(&mut g, &program::work_frame(cam, job, w, parts), size * 0.8);
                }
            }
            _ => {}
        }
    }
    for s in job.setups.iter().filter(|s| !s.suppressed) {
        if s.wcs.show_triad {
            triad(&mut g, &program::setup_frame(cam, job, s, parts), size);
        }
        for o in &s.operations {
            if let OpKind::WorkPlane(w) = &o.kind
                && w.show_triad
                && !o.hidden
                && !o.suppressed
            {
                triad(&mut g, &program::work_frame(cam, job, w, parts), size * 0.8);
            }
        }
    }
    // The stock's edges (its outline reads better than the see-through faces alone).
    if ui.show_stock
        && let Some(shape) = program::stock_shape(cam, job, parts)
        && let program::StockShape::Box { frame, b } = shape
    {
        let c = b.corners().map(|p| v3(frame.to_world(p)));
        let color = Color::srgba(0.35, 0.47, 0.66, 0.8);
        for i in 0..4 {
            g.line(c[i], c[(i + 1) % 4], color);
            g.line(c[i + 4], c[(i + 1) % 4 + 4], color);
            g.line(c[i], c[i + 4], color);
        }
    }
}
