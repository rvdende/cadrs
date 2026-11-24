//! **H** in a Part Studio: hides what is selected (parts, sketches, Plane features, default
//! planes), or shows it again when all of it is hidden already, as one undo step (the default
//! planes' visibility is view state, not the document's). With the feature list's **Select all
//! of this type** (every sketch, say), H then hides or shows them all.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use cadrs_core::{FeatureId, FeatureKind, PartId};
use cadrs_ui::ShortcutKey;

use crate::ActiveDocument;
use crate::parts::PartCache;
use crate::viewport::{Pick, PlaneKind, PlanesVisible, Selection};

pub struct HideKeyPlugin;

impl Plugin for HideKeyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, hide_key);
    }
}

/// H (no modifiers, no field typed in, no dialog, not while sketching) in a Part Studio.
#[allow(clippy::too_many_arguments)]
fn hide_key(
    mut keys: MessageReader<KeyboardInput>,
    focus: Res<bevy::input_focus::InputFocus>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    kind: Res<crate::viewport::ActiveKind>,
    sketch: Option<Res<crate::sketch::SketchSession>>,
    buttons: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    for k in keys.read() {
        if k.state != ButtonState::Pressed || k.shortcut_code() != KeyCode::KeyH {
            continue;
        }
        let modified = buttons.any_pressed([
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
        ]);
        if modified
            || focus.get().is_some()
            || !q_dialogs.is_empty()
            || sketch.is_some()
            || *kind != crate::viewport::ActiveKind::PartStudio
        {
            continue;
        }
        commands.queue(toggle_selected);
    }
}

/// What a selection's visibility is made of.
#[derive(Debug, Default, PartialEq)]
struct Targets {
    parts: Vec<PartId>,
    /// Sketches and Plane features (shown or hidden with their eye).
    sketches: Vec<FeatureId>,
    planes: Vec<PlaneKind>,
}

/// The parts, sketches and planes `picks` stand for: a part's faces, edges and vertices its
/// part; a sketch's regions, curves and points the sketch; another feature's row its parts.
fn targets(picks: &[Pick], el: &cadrs_core::Element, cache: &PartCache) -> Targets {
    let mut t = Targets::default();
    let part = |p: PartId, t: &mut Targets| {
        if !t.parts.contains(&p) {
            t.parts.push(p);
        }
    };
    let sketch = |f: FeatureId, t: &mut Targets| {
        if !t.sketches.contains(&f) {
            t.sketches.push(f);
        }
    };
    for p in picks {
        match *p {
            Pick::Part(x) | Pick::Face(x, _) | Pick::Edge(x, _) | Pick::Vertex(x, _) => {
                part(x, &mut t)
            }
            Pick::Region(f, _) | Pick::SketchCurve(f, _) | Pick::SketchPoint(f, _) => {
                sketch(f, &mut t)
            }
            Pick::Plane(k) => {
                if !t.planes.contains(&k) {
                    t.planes.push(k);
                }
            }
            Pick::Feature(f) => match el.feature(f).map(|x| &x.kind) {
                Some(FeatureKind::Sketch(_) | FeatureKind::Plane(_)) => sketch(f, &mut t),
                Some(_) => {
                    for p in cache.parts.iter().filter(|p| p.feature == f) {
                        part(p.id, &mut t);
                    }
                }
                None => {}
            },
            _ => {}
        }
    }
    t
}

/// Hides what is selected, or shows it when all of it is hidden (one undo step).
pub fn toggle_selected(world: &mut World) {
    let picks = world.resource::<Selection>().0.clone();
    let Some(el) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element().cloned())
    else {
        return;
    };
    let t = targets(&picks, &el, world.resource::<PartCache>());
    if t == Targets::default() {
        return;
    }
    let planes = *world.resource::<PlanesVisible>();
    let cache = world.resource::<PartCache>();
    let any_shown = t.parts.iter().any(|p| !cache.row_hidden(*p))
        || t.sketches
            .iter()
            .any(|f| crate::feature_menu::sketch_shown(world, *f))
        || t.planes.iter().any(|k| planes.shows(*k));
    let hide = any_shown;
    let parts = world.resource::<PartCache>().visibility_targets(&t.parts);
    let element = el.id;
    let mut commands: Vec<Box<dyn cadrs_core::Command>> = Vec::new();
    if !parts.is_empty() {
        commands.push(Box::new(cadrs_core::commands::SetPartsHidden {
            element,
            parts,
            hidden: hide,
        }));
    }
    for sketch in &t.sketches {
        commands.push(Box::new(cadrs_core::commands::SetSketchVisibility {
            element,
            sketch: *sketch,
            visible: Some(!hide),
        }));
    }
    if !commands.is_empty()
        && let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
    {
        let _ = doc.execute(&cadrs_core::command::Batch {
            label: if hide { "Hide" } else { "Show" }.into(),
            scope: cadrs_core::Scope::Element(element),
            commands,
        });
    }
    let mut pv = world.resource_mut::<PlanesVisible>();
    for k in t.planes {
        pv.set(k, !hide);
    }
}

/// The feature list's **Select all of this type**: every feature of `feature`'s type (every
/// sketch, every fillet…) selected, the default planes too for a Plane feature.
pub fn select_same_type(world: &mut World, feature: FeatureId) {
    let Some(el) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element().cloned())
    else {
        return;
    };
    let Some(f) = el.feature(feature) else { return };
    let ty = cadrs_core::feature_list::type_label(&f.kind);
    let mut picks: Vec<Pick> = el
        .features()
        .iter()
        .filter(|g| cadrs_core::feature_list::type_label(&g.kind) == ty)
        .map(|g| Pick::Feature(g.id))
        .collect();
    if matches!(f.kind, FeatureKind::Plane(_)) {
        picks.extend(PlaneKind::ALL.iter().map(|k| Pick::Plane(*k)));
    }
    world.resource_mut::<Selection>().0 = picks;
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_core::{Element, Feature};

    #[test]
    fn a_selection_stands_for_parts_sketches_and_planes() {
        let mut el = Element::part_studio("Part Studio 1");
        let (sk, pl) = (FeatureId::new(), FeatureId::new());
        let fs = el.features_mut().unwrap();
        fs.push(Feature::new(
            sk,
            "Sketch 1",
            FeatureKind::Sketch(Default::default()),
        ));
        fs.push(Feature::new(
            pl,
            "Plane 1",
            FeatureKind::Plane(cadrs_core::plane::PlaneFeature::default()),
        ));
        let part = PartId::new(FeatureId::new(), 0);
        let picks = [
            Pick::Feature(sk),
            Pick::Feature(pl),
            Pick::Plane(PlaneKind::Top),
            Pick::Part(part),
            Pick::Part(part),
        ];
        let t = targets(&picks, &el, &PartCache::default());
        assert_eq!(t.sketches, vec![sk, pl]);
        assert_eq!(t.parts, vec![part]);
        assert_eq!(t.planes, vec![PlaneKind::Top]);
    }
}
