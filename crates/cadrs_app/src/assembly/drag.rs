//! Dragging instances under their mates (P3B.3, `intro-to-assemblies.md` A16.4, X6):
//!
//! - **Drag in the view**: press on an instance (not on the triad) and move: the point under the
//!   pointer follows the pointer in the plane facing the viewer through it, as far as the mates
//!   and limits allow ([`cadrs_core::assembly::solver::drag`]): a slider slides and stops at its
//!   limit, a revolute turns, a fully constrained or fixed instance stays. Other instances follow
//!   where their mates make them. Releasing records one undo step ("Drag instance").
//! - **Triad drags** (A16.4: "the triad gives precise, value-driven drags") go through
//!   [`constrained_many`]: the triad asks for a placement, the mates decide what of it happens. An
//!   instance with no mates takes the placement exactly.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use cadrs_core::ElementId;
use cadrs_core::assembly::commands::MoveInstances;
use cadrs_core::assembly::solver::Pull;
use cadrs_core::assembly::{Assembly, InstanceId, Pose};

use crate::parts::PartCache;
use crate::viewport::{ViewportArea, ViewportRect, ViewportView, pointer_over_viewport};
use crate::{ActiveDocument, AppState};

pub struct DragPlugin;

impl Plugin for DragPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InstanceDrag>()
            .add_systems(
                Update,
                instance_drag
                    .after(super::triad::TriadSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut d: ResMut<InstanceDrag>| {
                *d = InstanceDrag::default()
            });
    }
}

/// A drag in the view: the press, then the drag itself.
#[derive(Resource, Default)]
pub struct InstanceDrag {
    press: Option<Press>,
    active: Option<Active>,
}

impl InstanceDrag {
    pub fn dragging(&self) -> bool {
        self.active.is_some()
    }
}

struct Press {
    instance: InstanceId,
    targets: Vec<InstanceId>,
    /// The grabbed point, in the instance's coordinates and in the assembly's.
    local: [f64; 3],
    hit: Vec3,
    at: Vec2,
}

struct Active {
    instance: InstanceId,
    targets: Vec<InstanceId>,
    local: [f64; 3],
    hit: Vec3,
    element: ElementId,
    /// The dragged assembly (the last frame's placements) and the document's.
    model: Assembly,
    base: Assembly,
    solids: HashMap<InstanceId, Arc<cadrs_core::Solid>>,
}

/// Whether `instance` is held by any mate or group (else it moves freely).
fn is_mated(model: &Assembly, instance: InstanceId) -> bool {
    model
        .mates
        .iter()
        .any(|f| !f.suppressed && f.involves(instance))
}

/// Resolve selected tree rows and geometry picks to solver occurrences. A parent row moves
/// all its leaves; an expanded child row moves that occurrence. Dragging an unselected part
/// still moves only that part. Preserve document order and remove overlapping selections.
fn movement_targets(
    doc: &ActiveDocument,
    selection: &crate::viewport::Selection,
    grabbed: InstanceId,
) -> Vec<InstanceId> {
    let Some(asm) = doc.active_element().and_then(|e| e.assembly_model()) else {
        return Vec::new();
    };
    let occurrences = cadrs_core::assembly::structure::occurrences(&doc.doc, asm);
    let selected: Vec<InstanceId> = occurrences
        .iter()
        .filter(|o| {
            selection
                .0
                .iter()
                .filter_map(|p| p.part())
                .any(|p| p == o.view_part || p == o.top.part_id())
        })
        .map(|o| o.id)
        .collect();
    let in_selection = occurrences
        .iter()
        .any(|o| selected.contains(&o.id) && (o.id == grabbed || o.top == grabbed));
    if in_selection {
        selected
    } else {
        occurrences
            .iter()
            .filter(|o| o.id == grabbed || o.top == grabbed)
            .map(|o| o.id)
            .collect()
    }
}

/// Solve one shared transform for all targets, so free parts keep their relative placements
/// and mated parts are solved together rather than overwriting each other's previews.
fn constrained_many(
    model: &Assembly,
    solids: &HashMap<InstanceId, Arc<cadrs_core::Solid>>,
    desired: &[(InstanceId, Pose)],
    anchor: [f64; 3],
) -> Vec<(InstanceId, Pose)> {
    let mut pulls = Vec::new();
    let mut free = Vec::new();
    for &(instance, want) in desired {
        let Some(i) = model.instance(instance).filter(|i| !i.fixed) else {
            continue;
        };
        if !is_mated(model, instance) {
            if i.pose != want {
                free.push((instance, want));
            }
            continue;
        }
        let c = i.pose.inverse().apply(anchor);
        for d in [[0.0, 0.0, 0.0], [25.0, 0.0, 0.0], [0.0, 25.0, 0.0]] {
            let point = [c[0] + d[0], c[1] + d[1], c[2] + d[2]];
            pulls.push(Pull {
                view: None,
                instance,
                point,
                target: want.apply(point),
            });
        }
    }
    let mut poses = if pulls.is_empty() {
        Vec::new()
    } else {
        cadrs_core::assembly::drag(model, solids, &pulls).changed(model)
    };
    for (id, pose) in free {
        if let Some((_, p)) = poses.iter_mut().find(|(i, _)| *i == id) {
            *p = pose;
        } else {
            poses.push((id, pose));
        }
    }
    poses
}

/// The requested placement defines a world transform, also applied to every selected target.
fn selection_motion(
    doc: &ActiveDocument,
    selection: &crate::viewport::Selection,
    model: &Assembly,
    solids: &HashMap<InstanceId, Arc<cadrs_core::Solid>>,
    instance: InstanceId,
    desired: Pose,
    anchor: [f64; 3],
) -> (Vec<(InstanceId, Pose)>, Option<Pose>) {
    let targets = movement_targets(doc, selection, instance);
    let from = doc
        .active_element()
        .and_then(|e| e.assembly_model()?.instance(instance))
        .map(|i| i.pose)
        .or_else(|| model.instance(instance).map(|i| i.pose));
    let Some(from) = from else {
        return (Vec::new(), None);
    };
    let delta = from.inverse().then(&desired);
    let wants: Vec<_> = targets
        .iter()
        .filter_map(|id| model.instance(*id).map(|i| (*id, i.pose.then(&delta))))
        .collect();
    let poses = constrained_many(model, solids, &wants, anchor);
    // Convert the representative occurrence's solved placement back to the widget's frame.
    let own = model
        .instance(instance)
        .filter(|i| targets.contains(&i.id))
        .or_else(|| targets.first().and_then(|id| model.instance(*id)))
        .map(|i| {
            let relative = i.pose.then(&from.inverse());
            let solved = poses
                .iter()
                .find(|(id, _)| *id == i.id)
                .map_or(i.pose, |(_, p)| *p);
            relative.inverse().then(&solved)
        });
    (poses, own)
}

/// A triad drag in progress: the placements `desired` leads to, shown as a preview. Returns the
/// dragged instance's placement as solved.
pub fn preview_move(
    world: &mut World,
    instance: InstanceId,
    desired: Pose,
    anchor: [f64; 3],
    selection: &crate::viewport::Selection,
) -> Option<Pose> {
    let (model, solids) = super::mate_dialog::model_and_solids(world)?;
    let doc = world.resource::<ActiveDocument>();
    let (poses, own) = selection_motion(doc, selection, &model, &solids, instance, desired, anchor);
    world.resource_mut::<super::AssemblyParts>().preview = poses.into_iter().collect();
    own
}

/// Moves `instance` toward `desired` within its mates, as one undo step labelled `label`.
pub fn commit_move(
    world: &mut World,
    element: ElementId,
    instance: InstanceId,
    desired: Pose,
    anchor: [f64; 3],
    label: &str,
) {
    let selection = world.resource::<crate::viewport::Selection>().clone();
    commit_move_for(world, element, instance, desired, anchor, label, &selection);
}

pub(super) fn commit_move_for(
    world: &mut World,
    element: ElementId,
    instance: InstanceId,
    desired: Pose,
    anchor: [f64; 3],
    label: &str,
    selection: &crate::viewport::Selection,
) {
    if world
        .get_resource::<ActiveDocument>()
        .and_then(super::active_assembly)
        != Some(element)
    {
        return;
    }
    let Some((model, solids)) = super::mate_dialog::model_and_solids(world) else {
        return;
    };
    let doc = world.resource::<ActiveDocument>();
    let (poses, _) = selection_motion(doc, selection, &model, &solids, instance, desired, anchor);
    world.resource_mut::<super::AssemblyParts>().preview.clear();
    if !poses.is_empty() {
        super::run(
            world,
            &MoveInstances {
                element,
                poses,
                label: label.into(),
            },
        );
    }
}

/// Presses, drags and releases instances in the view.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn instance_drag(
    mut inputs: MessageReader<PointerInput>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    selection: Res<crate::viewport::Selection>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    hover: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    triad: Res<super::triad::Triad>,
    mut parts: ResMut<super::AssemblyParts>,
    mut grab: ResMut<super::ViewportGrab>,
    busy: (
        Option<Res<super::mate_dialog::MateSession>>,
        Option<Res<super::group_dialog::GroupSession>>,
        Option<Res<super::insert::InsertSession>>,
        Option<Res<super::animate::Playback>>,
        Option<Res<super::connector_tool::ConnectorSession>>,
        Res<super::explode::ExplodeUi>,
        // A board's layout edited in context takes the presses (`crate::board_in_studio`).
        Res<crate::eda::Eda2d>,
        Res<crate::section_view::SectionViews>,
    ),
    mut state: ResMut<InstanceDrag>,
    mut commands: Commands,
) {
    let Some(doc) = doc else {
        inputs.clear();
        return;
    };
    let Some(element) = super::active_assembly(&doc) else {
        inputs.clear();
        // (Only written when set: not an assembly tab every frame.)
        if state.press.is_some() || state.active.is_some() {
            *state = InstanceDrag::default();
        }
        if !parts.preview.is_empty() {
            parts.preview.clear();
        }
        return;
    };
    if state.active.as_ref().is_some_and(|a| a.element != element) {
        *state = InstanceDrag::default();
        parts.preview.clear();
    }
    let over = pointer_over_viewport(&hover, &q_area);
    // P3B.8: while an exploded view is edited, a press is the explode triad's.
    let blocked = busy.0.is_some()
        || busy.1.is_some()
        || busy.2.is_some()
        || busy.3.is_some()
        || busy.4.is_some()
        || busy.5.editing()
        || busy.6.plane().is_some();
    // Section-plane picking and dragging must never start or finish a part move.
    if busy.7.dialog.is_some() {
        inputs.clear();
        if state.press.is_some() || state.active.is_some() {
            *state = InstanceDrag::default();
            parts.preview.clear();
            grab.0 = false;
        }
        return;
    }
    let mut last_move: Option<Vec2> = None;
    let mut finish = None;
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) => {
                state.press = None;
                state.active = None;
                if !over || blocked || triad.dragging() || triad.hover.is_some() {
                    continue;
                }
                let Some((part, _, t)) =
                    crate::parts::pick_face(&cache, &view.view, rect.offset(pos))
                else {
                    continue;
                };
                // The part's occurrence (a part inside a subassembly, P3B.4).
                let instance = super::occurrence_of(part);
                let Some(inst) =
                    super::flat_model(&doc).and_then(|m| m.instance(instance).cloned())
                else {
                    continue;
                };
                if inst.fixed {
                    continue;
                }
                let (o, d) = view.view.ray(rect.offset(pos));
                let hit = o + d * t;
                let local = inst
                    .pose
                    .inverse()
                    .apply([hit.x as f64, hit.y as f64, hit.z as f64]);
                let targets = movement_targets(&doc, &selection, instance);
                state.press = Some(Press {
                    instance,
                    targets,
                    local,
                    hit,
                    at: pos,
                });
            }
            PointerAction::Move { .. } if state.press.is_some() || state.active.is_some() => {
                last_move = Some(pos)
            }
            PointerAction::Release(PointerButton::Primary) | PointerAction::Cancel => {
                if state.press.is_none() && state.active.is_none() {
                    continue;
                }
                let cancel = matches!(input.action, PointerAction::Cancel);
                finish = Some(cancel);
                if !cancel && (state.press.is_some() || state.active.is_some()) {
                    last_move = Some(pos);
                }
            }
            _ => {}
        }
    }
    if finish == Some(true) {
        grab.0 = false;
        *state = InstanceDrag::default();
        parts.preview.clear();
        return;
    }
    if let Some(pos) = last_move {
        update_drag(
            &mut state, &doc, element, &view, &rect, &mut parts, &mut grab, pos,
        );
    }
    if finish.is_some() {
        state.press = None;
        parts.preview.clear();
        if let Some(a) = state.active.take() {
            let changed: Vec<_> = a
                .model
                .instances
                .iter()
                .filter(|i| a.base.instance(i.id).is_some_and(|b| b.pose != i.pose))
                .map(|i| (i.id, i.pose))
                .collect();
            if !changed.is_empty() {
                let el = a.element;
                commands.queue(move |world: &mut World| {
                    super::run(
                        world,
                        &MoveInstances {
                            element: el,
                            poses: changed,
                            label: "Drag instances".into(),
                        },
                    );
                });
            }
        }
    }
}

/// Advance the preview before committing a release, including when move/release events
/// arrive in the same frame. All requested placements are relative to the original poses.
#[allow(clippy::too_many_arguments)]
fn update_drag(
    state: &mut InstanceDrag,
    doc: &ActiveDocument,
    element: ElementId,
    view: &ViewportView,
    rect: &ViewportRect,
    parts: &mut super::AssemblyParts,
    grab: &mut super::ViewportGrab,
    pos: Vec2,
) {
    if state.active.is_none() {
        let Some(p) = state.press.as_ref() else {
            return;
        };
        if p.at.distance(pos) < 4.0 {
            return;
        }
        let Some(el_model) = doc
            .active_element()
            .and_then(|e| e.assembly_model())
            .cloned()
        else {
            return;
        };
        let model = cadrs_core::assembly::structure::solver_model(&doc.doc, &el_model);
        let solids = cadrs_core::assembly::occurrence_solids(&doc.doc, &el_model, |e| {
            parts.build(&doc.doc, e)
        });
        grab.0 = true;
        state.active = Some(Active {
            instance: p.instance,
            targets: p.targets.clone(),
            local: p.local,
            hit: p.hit,
            element,
            base: model.clone(),
            model,
            solids,
        });
    }
    let Some(a) = state.active.as_mut() else {
        return;
    };
    // The pointer on the plane facing the viewer through the grabbed point.
    let (o, d) = view.view.ray(rect.offset(pos));
    let n = view.view.back();
    let den = n.dot(d);
    if den.abs() < 1e-6 {
        return;
    }
    let target = o + d * (n.dot(a.hit - o) / den);
    // Measured across the view: the point goes where it looks nearest the pointer.
    let view = Some([n.x as f64, n.y as f64, n.z as f64]);
    let pull = Pull {
        view,
        instance: a.instance,
        point: a.local,
        target: [target.x as f64, target.y as f64, target.z as f64],
    };
    let changed = if a.targets.len() > 1 {
        let delta = Pose::translation([
            (target.x - a.hit.x) as f64,
            (target.y - a.hit.y) as f64,
            (target.z - a.hit.z) as f64,
        ]);
        let desired: Vec<_> = a
            .targets
            .iter()
            .filter_map(|id| a.base.instance(*id).map(|i| (*id, i.pose.then(&delta))))
            .collect();
        constrained_many(
            &a.model,
            &a.solids,
            &desired,
            [a.hit.x as f64, a.hit.y as f64, a.hit.z as f64],
        )
    } else {
        cadrs_core::assembly::drag(&a.model, &a.solids, &[pull]).changed(&a.model)
    };
    // (A frame whose solve diverged is dropped: the instances stay where the last one left
    // them, #191.)
    if changed.iter().any(|(_, p)| !p.is_finite()) {
        return;
    }
    for (id, p) in &changed {
        if let Some(i) = a.model.instance_mut(*id) {
            i.pose = *p;
        }
    }
    parts.preview = a
        .model
        .instances
        .iter()
        .filter(|i| a.base.instance(i.id).is_some_and(|b| b.pose != i.pose))
        .map(|i| (i.id, i.pose))
        .collect();
}

#[cfg(test)]
mod movement_tests {
    use super::*;
    use crate::viewport::{Pick, Selection};
    use cadrs_core::assembly::mate::{MateFeature, MateId, MateKind};
    use cadrs_core::assembly::{Instance, InstanceSource};
    use cadrs_core::{Document, FeatureId, PartId};

    fn scene() -> (ActiveDocument, [InstanceId; 4]) {
        let mut doc = Document::new("Move selection");
        let studio = doc.elements[0].id;
        let element = doc.elements[1].id;
        let ids = [
            InstanceId::new(),
            InstanceId::new(),
            InstanceId::new(),
            InstanceId::new(),
        ];
        doc.elements[1].assembly.instances = ids
            .iter()
            .enumerate()
            .map(|(n, id)| {
                Instance::new(
                    *id,
                    InstanceSource::Part {
                        element: studio,
                        part: PartId::new(FeatureId::new(), 0),
                    },
                    Pose::translation([n as f64 * 10.0, 0.0, 0.0]),
                )
            })
            .collect();
        let mut active = ActiveDocument::new(doc);
        active.active = Some(element);
        (active, ids)
    }

    fn selected(ids: &[InstanceId]) -> Selection {
        Selection(ids.iter().map(|i| Pick::Part(i.part_id())).collect())
    }

    fn close(a: Pose, b: Pose) {
        for (p, q) in a.translation.iter().zip(b.translation.iter()) {
            assert!((p - q).abs() < 1e-5, "{a:?} != {b:?}");
        }
        for (p, q) in a.rotation.iter().flatten().zip(b.rotation.iter().flatten()) {
            assert!((p - q).abs() < 1e-6, "{a:?} != {b:?}");
        }
    }

    #[test]
    fn widget_translation_and_rotation_move_the_selection_once() {
        let (doc, ids) = scene();
        let model = super::super::flat_model(&doc).unwrap();
        let selection = selected(&[ids[1], ids[0], ids[0]]);
        for delta in [
            Pose::translation([7.0, -3.0, 2.0]),
            Pose::rotation_about([5.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0.7),
        ] {
            for primary in [ids[0], ids[1]] {
                let want = model.instance(primary).unwrap().pose.then(&delta);
                let (poses, own) = selection_motion(
                    &doc,
                    &selection,
                    &model,
                    &HashMap::new(),
                    primary,
                    want,
                    [5.0, 0.0, 0.0],
                );
                assert_eq!(poses.len(), 2);
                for id in &ids[..2] {
                    close(
                        poses.iter().find(|(i, _)| i == id).unwrap().1,
                        model.instance(*id).unwrap().pose.then(&delta),
                    );
                }
                close(own.unwrap(), want);
            }
        }
        assert_eq!(
            movement_targets(&doc, &selection, ids[2]),
            vec![ids[2]],
            "unselected grab must not move the selection"
        );
    }

    #[test]
    fn fixed_and_grouped_parts_respect_constraints_while_free_selection_moves() {
        let (mut doc, ids) = scene();
        let asm = &mut doc.doc.elements[1].assembly;
        asm.instances[2].fixed = true;
        asm.mates.push(MateFeature::new(
            MateId::new(),
            "Grounded group",
            MateKind::Group {
                instances: vec![ids[0], ids[2]],
            },
        ));
        let model = super::super::flat_model(&doc).unwrap();
        let delta = Pose::translation([0.0, 8.0, 0.0]);
        let wants: Vec<_> = ids[..3]
            .iter()
            .map(|id| (*id, model.instance(*id).unwrap().pose.then(&delta)))
            .collect();
        let poses = constrained_many(&model, &HashMap::new(), &wants, [0.0; 3]);
        assert_eq!(poses.len(), 1);
        assert_eq!(poses[0].0, ids[1]);
        close(poses[0].1, wants[1].1);
        // A group mate can also carry an unselected member along with the selected one.
        doc.doc.elements[1].assembly.instances[2].fixed = false;
        let model = super::super::flat_model(&doc).unwrap();
        let poses = constrained_many(&model, &HashMap::new(), &wants[..2], [0.0; 3]);
        for id in &ids[..3] {
            close(
                poses.iter().find(|(i, _)| i == id).unwrap().1,
                model.instance(*id).unwrap().pose.then(&delta),
            );
        }
        assert!(!poses.iter().any(|(id, _)| *id == ids[3]));
    }

    #[test]
    fn nested_rows_resolve_to_occurrences_without_duplicate_moves() {
        let (mut doc, ids) = scene();
        let mut child = cadrs_core::Element::assembly("Child");
        child.assembly.instances = doc.doc.elements[1].assembly.instances[2..].to_vec();
        let child_id = child.id;
        doc.doc.elements.push(child);
        doc.doc.elements[1].assembly.instances.truncate(2);
        doc.doc.elements[1].assembly.instances[0].source =
            InstanceSource::Assembly { element: child_id };
        let asm = &doc.doc.elements[1].assembly;
        let occurrences = cadrs_core::assembly::structure::occurrences(&doc.doc, asm);
        let children: Vec<_> = occurrences.iter().filter(|o| o.top == ids[0]).collect();
        assert_eq!(children.len(), 2);
        let mut selection = selected(&ids[..2]);
        selection.0.push(Pick::Part(children[0].view_part));
        assert_eq!(movement_targets(&doc, &selection, children[0].id).len(), 3);
        assert_eq!(
            movement_targets(
                &doc,
                &Selection(vec![Pick::Part(children[1].view_part)]),
                children[1].id
            ),
            vec![children[1].id]
        );
        let model = super::super::flat_model(&doc).unwrap();
        let from = asm.instance(ids[0]).unwrap().pose;
        let delta = Pose::translation([4.0, 6.0, 0.0]);
        let (poses, own) = selection_motion(
            &doc,
            &selection,
            &model,
            &HashMap::new(),
            ids[0],
            from.then(&delta),
            [0.0; 3],
        );
        assert_eq!(poses.len(), 3);
        for (id, pose) in poses {
            close(pose, model.instance(id).unwrap().pose.then(&delta));
        }
        close(own.unwrap(), from.then(&delta));
    }

    #[test]
    fn dragging_previews_every_selected_part_and_commits_one_undo_step_or_cancels() {
        use bevy::picking::pointer::Location;
        for (cancel, batched, section) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
            (false, true, true),
        ] {
            let (doc, ids) = scene();
            let before = doc.doc.clone();
            let selection = selected(&ids[..2]);
            let targets = movement_targets(&doc, &selection, ids[0]);
            let mut app = App::new();
            app.add_message::<PointerInput>()
                .insert_resource(doc)
                .insert_resource(selection)
                .init_resource::<PartCache>()
                .init_resource::<HoverMap>()
                .init_resource::<super::super::triad::Triad>()
                .init_resource::<super::super::AssemblyParts>()
                .init_resource::<super::super::ViewportGrab>()
                .init_resource::<super::super::explode::ExplodeUi>()
                .init_resource::<crate::eda::Eda2d>()
                .init_resource::<crate::section_view::SectionViews>()
                .insert_resource(ViewportView {
                    view: crate::camera::ViewState {
                        scale: 1.0,
                        ..crate::camera::ViewState::standard(crate::camera::StandardView::Top)
                    },
                    ..default()
                })
                .insert_resource(ViewportRect(Rect::new(0.0, 0.0, 200.0, 200.0)))
                .insert_resource(InstanceDrag {
                    press: Some(Press {
                        instance: ids[0],
                        targets,
                        local: [0.0; 3],
                        hit: Vec3::ZERO,
                        at: Vec2::splat(100.0),
                    }),
                    active: None,
                })
                .add_systems(Update, instance_drag);
            let window = app.world_mut().spawn_empty().id();
            let target =
                bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window))
                    .normalize(None)
                    .unwrap();
            let location = Location {
                target,
                position: Vec2::new(120.0, 110.0),
            };
            if section && batched {
                app.world_mut()
                    .resource_mut::<crate::section_view::SectionViews>()
                    .dialog = before.elements.get(1).map(|e| e.id);
            }
            app.world_mut().write_message(PointerInput::new(
                PointerId::Mouse,
                location.clone(),
                PointerAction::Move {
                    delta: Vec2::new(20.0, 10.0),
                },
            ));
            if !batched {
                app.update();
                let preview = &app
                    .world()
                    .resource::<super::super::AssemblyParts>()
                    .preview;
                assert_eq!(preview.len(), 2);
                for id in &ids[..2] {
                    close(
                        preview[id],
                        before.elements[1]
                            .assembly
                            .instance(*id)
                            .unwrap()
                            .pose
                            .then(&Pose::translation([20.0, -10.0, 0.0])),
                    );
                }
                assert_eq!(
                    app.world().resource::<ActiveDocument>().doc,
                    before,
                    "preview must not edit the document"
                );
            }
            if section {
                app.world_mut()
                    .resource_mut::<crate::section_view::SectionViews>()
                    .dialog = before.elements.get(1).map(|e| e.id);
            }
            let action = if cancel {
                PointerAction::Cancel
            } else {
                PointerAction::Release(PointerButton::Primary)
            };
            app.world_mut()
                .write_message(PointerInput::new(PointerId::Mouse, location, action));
            app.update();
            assert!(
                app.world()
                    .resource::<super::super::AssemblyParts>()
                    .preview
                    .is_empty()
            );
            let mut active = app.world_mut().resource_mut::<ActiveDocument>();
            if cancel || section {
                assert_eq!(active.doc, before);
                assert_eq!(active.history.undo_len(), 0);
            } else {
                assert_eq!(active.history.undo_len(), 1);
                let after = active.doc.clone();
                for id in &ids[..2] {
                    close(
                        after.elements[1].assembly.instance(*id).unwrap().pose,
                        before.elements[1]
                            .assembly
                            .instance(*id)
                            .unwrap()
                            .pose
                            .then(&Pose::translation([20.0, -10.0, 0.0])),
                    );
                }
                for id in &ids[2..] {
                    assert_eq!(
                        after.elements[1].assembly.instance(*id),
                        before.elements[1].assembly.instance(*id)
                    );
                }
                assert!(active.undo().is_some());
                assert_eq!(active.doc, before);
                assert!(active.redo().is_some());
                assert_eq!(active.doc, after);
            }
        }
    }
}
