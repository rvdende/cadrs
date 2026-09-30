//! Dragging instances under their mates (P3B.3, `intro-to-assemblies.md` A16.4, X6):
//!
//! - **Drag in the view**: press on an instance (not on the triad) and move: the point under the
//!   pointer follows the pointer in the plane facing the viewer through it, as far as the mates
//!   and limits allow ([`cadrs_core::assembly::solver::drag`]): a slider slides and stops at its
//!   limit, a revolute turns, a fully constrained or fixed instance stays. Other instances follow
//!   where their mates make them. Releasing records one undo step ("Drag instance").
//! - **Triad drags** (A16.4: "the triad gives precise, value-driven drags") go through
//!   [`constrained`]: the triad asks for a placement, the mates decide what of it happens. An
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
                instance_drag.after(super::triad::TriadSet).run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut d: ResMut<InstanceDrag>| *d = InstanceDrag::default());
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
    /// The grabbed point, in the instance's coordinates and in the assembly's.
    local: [f64; 3],
    hit: Vec3,
    at: Vec2,
}

struct Active {
    instance: InstanceId,
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
    model.mates.iter().any(|f| !f.suppressed && f.involves(instance))
}

/// The placements that result from asking `instance` to go to `desired` (a triad drag, typed
/// value or menu item), within the mates: three points of the instance about `anchor` (the
/// triad's origin, assembly coordinates) pulled toward where `desired` puts them. Only the
/// placements that change (against `model`).
pub fn constrained(
    model: &Assembly,
    solids: &HashMap<InstanceId, Arc<cadrs_core::Solid>>,
    instance: InstanceId,
    desired: Pose,
    anchor: [f64; 3],
) -> Vec<(InstanceId, Pose)> {
    if !is_mated(model, instance) {
        return vec![(instance, desired)];
    }
    let Some(pose) = model.instance(instance).map(|i| i.pose) else { return Vec::new() };
    let c = pose.inverse().apply(anchor);
    let s = 25.0;
    let pulls: Vec<Pull> = [[0.0, 0.0, 0.0], [s, 0.0, 0.0], [0.0, s, 0.0]]
        .into_iter()
        .map(|d| {
            let p = [c[0] + d[0], c[1] + d[1], c[2] + d[2]];
            Pull { view: None, instance, point: p, target: desired.apply(p) }
        })
        .collect();
    let sol = cadrs_core::assembly::drag(model, solids, &pulls);
    sol.changed(model)
}

/// A triad drag in progress: the placements `desired` leads to, shown as a preview. Returns the
/// dragged instance's placement as solved.
pub fn preview_move(world: &mut World, instance: InstanceId, desired: Pose, anchor: [f64; 3]) -> Option<Pose> {
    let (model, solids) = super::mate_dialog::model_and_solids(world)?;
    // A subassembly moves as its first part does (P3B.4).
    let sub = world.get_resource::<ActiveDocument>().and_then(|d| super::first_part_of(d, instance));
    let (mover, want) = match sub {
        Some((leaf, in_top)) => (leaf, in_top.then(&desired)),
        None => (instance, desired),
    };
    let poses = constrained(&model, &solids, mover, want, anchor);
    let own = poses.iter().find(|(i, _)| *i == mover).map(|(_, p)| *p).or_else(|| model.instance(mover).map(|i| i.pose));
    world.resource_mut::<super::AssemblyParts>().preview = poses.into_iter().collect();
    match sub {
        Some((_, in_top)) => own.map(|p| in_top.inverse().then(&p)),
        None => own,
    }
}

/// Moves `instance` toward `desired` within its mates, as one undo step labelled `label`.
pub fn commit_move(world: &mut World, element: ElementId, instance: InstanceId, desired: Pose, anchor: [f64; 3], label: &str) {
    let Some((model, solids)) = super::mate_dialog::model_and_solids(world) else { return };
    let (instance, desired) = match world.get_resource::<ActiveDocument>().and_then(|d| super::first_part_of(d, instance)) {
        Some((leaf, in_top)) => (leaf, in_top.then(&desired)),
        None => (instance, desired),
    };
    let poses = constrained(&model, &solids, instance, desired, anchor);
    world.resource_mut::<super::AssemblyParts>().preview.clear();
    if !poses.is_empty() {
        super::run(world, &MoveInstances { element, poses, label: label.into() });
    }
}

/// Presses, drags and releases instances in the view.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn instance_drag(
    mut inputs: MessageReader<PointerInput>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
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
        *state = InstanceDrag::default();
        return;
    };
    let over = pointer_over_viewport(&hover, &q_area);
    // P3B.8: while an exploded view is edited, a press is the explode triad's.
    let blocked = busy.0.is_some() || busy.1.is_some() || busy.2.is_some() || busy.3.is_some() || busy.4.is_some() || busy.5.editing();
    let mut last_move: Option<Vec2> = None;
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
                let Some((part, _, t)) = crate::parts::pick_face(&cache, &view.view, rect.offset(pos)) else { continue };
                // The part's occurrence (a part inside a subassembly, P3B.4).
                let instance = super::occurrence_of(part);
                let Some(inst) = super::flat_model(&doc).and_then(|m| m.instance(instance).cloned()) else { continue };
                if inst.fixed {
                    continue;
                }
                let (o, d) = view.view.ray(rect.offset(pos));
                let hit = o + d * t;
                let local = inst.pose.inverse().apply([hit.x as f64, hit.y as f64, hit.z as f64]);
                state.press = Some(Press { instance, local, hit, at: pos });
            }
            PointerAction::Move { .. } if state.press.is_some() || state.active.is_some() => last_move = Some(pos),
            PointerAction::Release(PointerButton::Primary) | PointerAction::Cancel => {
                state.press = None;
                if let Some(a) = state.active.take() {
                    let poses = a.model.instances.iter().map(|i| (i.id, i.pose)).collect::<Vec<_>>();
                    let changed: Vec<(InstanceId, Pose)> = poses
                        .into_iter()
                        .filter(|(id, p)| a.base.instance(*id).is_some_and(|i| i.pose != *p))
                        .collect();
                    parts.preview.clear();
                    let cancel = matches!(input.action, PointerAction::Cancel);
                    if !changed.is_empty() && !cancel {
                        let el = a.element;
                        commands.queue(move |world: &mut World| {
                            super::run(world, &MoveInstances { element: el, poses: changed, label: "Drag instance".into() });
                        });
                    }
                }
            }
            _ => {}
        }
    }
    let Some(pos) = last_move else { return };
    if state.active.is_none() {
        let Some(p) = state.press.as_ref() else { return };
        if p.at.distance(pos) < 4.0 {
            return;
        }
        let Some(el_model) = doc.active_element().and_then(|e| e.assembly_model()).cloned() else { return };
        let model = cadrs_core::assembly::structure::solver_model(&doc.doc, &el_model);
        let solids = cadrs_core::assembly::occurrence_solids(&doc.doc, &el_model, |e| parts.build(&doc.doc, e));
        grab.0 = true;
        state.active = Some(Active { instance: p.instance, local: p.local, hit: p.hit, element, base: model.clone(), model, solids });
    }
    let Some(a) = state.active.as_mut() else { return };
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
    let pull = Pull { view, instance: a.instance, point: a.local, target: [target.x as f64, target.y as f64, target.z as f64] };
    let sol = cadrs_core::assembly::drag(&a.model, &a.solids, &[pull]);
    for (id, p) in &sol.poses {
        if let Some(i) = a.model.instance_mut(*id) {
            i.pose = *p;
        }
    }
    parts.preview = sol.changed(&a.base).into_iter().collect();
}
