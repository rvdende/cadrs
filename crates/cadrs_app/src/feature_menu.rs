//! The feature context menu's P3D.1 items (IR5.5, `ex1-step2.png`), besides the ones the feature
//! list had (Edit, Rename, Suppress, folders, dependencies, the rollback bar, appearances):
//!
//! - **Copy sketch**: the sketch's geometry, constraints and dimensions go to the sketch
//!   clipboard; **Ctrl+V** while editing a sketch pastes them (one undo step), centred on the
//!   pointer when it is over the sketch plane.
//! - **Show dimensions** / **Hide dimensions**: a sketch that isn't being edited shows its
//!   dimensions in the view.
//! - **Show** / **Hide** a sketch (as its row's eye), **Show all sketches** (one undo step).
//! - **Zoom to selection**: fits the view to what the feature made (its faces), or to the
//!   sketch.
//! - **Dynamic suppression ▸**: suppression driven by a variable or a configuration; cadrs has
//!   neither yet, so its entries are shown disabled.
//! - **Edit healthy moment of …**: shown disabled until Repair (P3D.4).

use std::collections::HashSet;

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use cadrs_core::FeatureId;
use cadrs_core::commands::{EditSketch, SetSketchVisibility};
use cadrs_sketch::{Sketch, SketchOp, Vec2 as SVec2};
use cadrs_ui::{Notification, TextInputField, Theme, show_notification};

use crate::sketch::SketchSession;
use crate::{ActiveDocument, AppState};

pub struct FeatureMenuPlugin;

impl Plugin for FeatureMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShownDimensions>()
            .init_resource::<SketchClipboard>()
            .add_systems(Update, paste_shortcut.run_if(in_state(AppState::Document)));
    }
}

/// Sketches whose dimensions show while they aren't edited (Show dimensions).
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct ShownDimensions(pub HashSet<FeatureId>);

/// What Copy sketch took: the sketch's geometry and its name.
#[derive(Resource, Debug, Default, Clone)]
pub struct SketchClipboard(pub Option<(String, Sketch)>);

/// Copy sketch: the sketch goes to the clipboard.
pub fn copy_sketch(world: &mut World, id: FeatureId) {
    let found = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element()?.feature(id).and_then(|f| Some((f.name.clone(), f.sketch()?.geometry.clone()))));
    let Some((name, sketch)) = found else { return };
    world.resource_mut::<SketchClipboard>().0 = Some((name.clone(), sketch));
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    show_notification(
        &mut commands,
        &theme,
        Notification::info(format!("{name} copied. Paste it into a sketch with Ctrl+V.")).name("sketch-copied-toast"),
    );
    world.flush();
}

/// Ctrl+V while a sketch is edited: pastes the copied sketch.
fn paste_shortcut(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<TextInputField>>,
    session: Option<Res<SketchSession>>,
    clipboard: Res<SketchClipboard>,
    mut commands: Commands,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    for k in keys_in.read() {
        if k.state != ButtonState::Pressed || !ctrl || typing || k.key_code != KeyCode::KeyV {
            continue;
        }
        let (Some(s), Some((_, sketch))) = (session.as_ref(), clipboard.0.as_ref()) else { continue };
        if s.waiting_for_plane {
            continue;
        }
        let sketch = sketch.clone();
        let (element, feature) = (s.element, s.feature);
        commands.queue(move |world: &mut World| {
            // Centred on the pointer when it is over the plane; else where it was.
            let cursor = world.resource::<crate::sketch_tools::SketchDraw>().cursor;
            let offset = cursor.and_then(|c| bounds_center(&sketch).map(|m| c - m)).unwrap_or(SVec2::ZERO);
            let before: Vec<cadrs_sketch::CurveId> =
                crate::sketch_tools::world_sketch(world).map(|g| g.curves.keys().collect()).unwrap_or_default();
            let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
            let op = SketchOp::Paste { sketch: Box::new(sketch), offset };
            if let Err(e) = doc.execute(&EditSketch { element, feature, op }) {
                warn!("cannot paste the sketch: {e}");
                return;
            }
            // The pasted curves are selected.
            let pasted: Vec<cadrs_sketch::SketchEntity> = crate::sketch_tools::world_sketch(world)
                .map(|g| g.curves.keys().filter(|k| !before.contains(k)).map(cadrs_sketch::SketchEntity::Curve).collect())
                .unwrap_or_default();
            world.resource_mut::<crate::sketch_tools::SketchSelection>().0 = pasted;
        });
    }
}

fn bounds_center(s: &Sketch) -> Option<SVec2> {
    let mut pts = s.points.values().map(|p| p.pos);
    let first = pts.next()?;
    let (lo, hi) = pts.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
    Some(lo.midpoint(hi))
}

/// Show dimensions / Hide dimensions.
pub fn toggle_dimensions(world: &mut World, id: FeatureId) {
    let mut shown = world.resource_mut::<ShownDimensions>();
    if !shown.0.remove(&id) {
        shown.0.insert(id);
    }
}

/// Show or hide a sketch (one undo step), as its row's eye does.
pub fn set_sketch_visible(world: &mut World, id: FeatureId, visible: bool) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(element) = doc.active_element().map(|e| e.id) else { return };
    let _ = doc.execute(&SetSketchVisibility { element, sketch: id, visible: Some(visible) });
}

/// Show all sketches: every hidden sketch of the Part Studio shown, as one undo step.
pub fn show_all_sketches(world: &mut World) {
    let hidden: Vec<FeatureId> = world.resource::<crate::parts::PartCache>().hidden_sketches.iter().copied().collect();
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(el) = doc.active_element() else { return };
    let element = el.id;
    // Only sketches hidden by the user or by a feature using them (not rolled back ones).
    let bar = el.rollback_index();
    let ids: Vec<FeatureId> = el
        .features()
        .iter()
        .enumerate()
        .filter(|(i, f)| f.sketch().is_some() && *i < bar && hidden.contains(&f.id) && !el.is_suppressed(f.id))
        .map(|(_, f)| f.id)
        .collect();
    if ids.is_empty() {
        return;
    }
    let mark = doc.history.undo_len();
    for sketch in ids {
        let _ = doc.execute(&SetSketchVisibility { element, sketch, visible: Some(true) });
    }
    doc.squash_element_since(mark, element, "Show all sketches");
}

/// Whether a sketch is shown in the view.
pub fn sketch_shown(world: &World, id: FeatureId) -> bool {
    !world.resource::<crate::parts::PartCache>().hidden_sketches.contains(&id)
}

/// Zoom to selection: the view fitted to the faces the feature made (or, for a sketch, its
/// curves), right of the feature panel.
pub fn zoom_to_feature(world: &mut World, id: FeatureId) {
    let mut pts: Vec<Vec3> = Vec::new();
    let sketch = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element()?.feature(id).and_then(|f| f.sketch().cloned()));
    if let Some(sk) = sketch {
        if let Some(plane) = sk.plane {
            let frame = plane.frame();
            for c in sk.geometry.curves.keys() {
                for p in cadrs_sketch::hit::curve_polyline(&sk.geometry, c) {
                    let w = frame.to_world(p);
                    pts.push(Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32));
                }
            }
        }
    } else {
        let cache = world.resource::<crate::parts::PartCache>();
        for part in &cache.parts {
            let s = &part.solid;
            for f in s.faces.iter().filter(|f| f.name.op == id.0) {
                for t in f.first_triangle..f.first_triangle + f.triangle_count {
                    for k in 0..3 {
                        if let Some(p) = s.indices.get(3 * t + k).and_then(|i| s.positions.get(*i as usize)) {
                            pts.push(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32));
                        }
                    }
                }
            }
        }
        if pts.is_empty() {
            // Nothing of its own left (a boolean, a deleted face): its parts.
            for part in cache.parts.iter().filter(|p| p.feature == id || p.features.contains(&id)) {
                pts.extend(part.solid.positions.iter().map(|q| Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32)));
            }
        }
    }
    if pts.is_empty() {
        return;
    }
    let size = world.resource::<crate::viewport::ViewportRect>().0.size();
    let inset = world.resource::<crate::viewport::DialogInset>().0;
    let mut view = world.resource_mut::<crate::viewport::ViewportView>();
    let to = view.target().fitted_beside(&pts, size, 0.6, inset);
    view.animate_to(to);
}
