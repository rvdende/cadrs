//! Explicit mate connectors in an assembly (P3B.7, `intro-to-assemblies.md` A1.2, A22, A23,
//! X5, X10; `ex4-step9.png`, `ex4-step10.png`, `lesson-mate-connector-dialog.png`):
//!
//! - **Shown on the instances**: each instance carries its part's Part Studio connectors
//!   ([`cadrs_core::solid::Solid::connectors`]; they travel with the part, A22.2, A22.7), and the
//!   assembly's own connectors sit on their owner instances. **K** shows or hides them all (in
//!   both tabs, [`crate::pattern::MateConnectorsShown`]). The mate dialog picks them (a click
//!   near a connector's origin takes it before the implicit points under the pointer).
//! - **The Mate connector tool** (the toolbar button, **Ctrl+M**, A1.2, A22.3) makes a connector
//!   of the assembly (it stays in the assembly, A22.2): the same dialog as the Part Studio's
//!   ([`crate::pattern_dialog::connector_rows`]): Origin type (On entity, Between entities),
//!   Origin entity (the implicit point of the face, edge or vertex clicked), Between entity (of
//!   the same instance), **Realign** (primary and secondary axes to an edge's direction or a
//!   face's normal), **Move** (X, Y, Z, Rotation), **Owner entity** (the instance it moves with),
//!   flip and reorient.
//! - **Editing a mate's connector** (A23.2–A23.4): a mate expanded in the Mate Features list
//!   shows its two connectors (hovering one highlights it); right-click → **Edit** opens the
//!   same dialog on it: Realign (A23.3: the secondary axis to a slot's edge), Move, Flip,
//!   Reorient. The edits are stored on the mate's connector
//!   ([`cadrs_core::assembly::connector::ConnectorEdit`]); the mate's instance moves to follow
//!   (shown while editing, solved on ✓ as one undo step).

use std::sync::Arc;

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use cadrs_core::ElementId;
use cadrs_core::assembly::commands::{SetLocalConnector, SetMateFeature};
use cadrs_core::assembly::connector::{
    ConnectorAnchor, ConnectorFrame, EntityRef, LocalConnector, LocalConnectorId, MateConnector, entity_direction,
};
use cadrs_core::assembly::mate::{MateId, MateKind};
use cadrs_core::assembly::solver::SolveOptions;
use cadrs_core::assembly::{InstanceId, Pose};
use cadrs_core::mate::OriginType;
use cadrs_core::solid::Solid;
use cadrs_sketch::units::Quantity;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, FeatureDialogAccept, FeatureDialogCancel, NumberFieldCommit, NumberFieldState, SelectChange, SelectionListActivate,
    SelectionListRemove,
};

use super::connectors::{self, ConnectorGizmos, ConnectorHaloGizmos, entity_of, v3};
use crate::applied::AppliedField;
use crate::applied_dialog::Role;
use crate::parts::PartCache;
use crate::viewport::{PickRequest, PlaneHighlight, ViewportArea, ViewportDrag, ViewportRect, ViewportView};
use crate::{ActiveDocument, AppState};

pub struct ConnectorToolPlugin;

impl Plugin for ConnectorToolPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConnectorHover>()
            .add_systems(
                Update,
                (tool_picks, sync_dialog, draw_explicit_connectors, draw_session)
                    .chain()
                    .after(crate::viewport::apply_view_to_camera)
                    .before(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| commands.remove_resource::<ConnectorSession>())
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_select)
            .add_observer(on_checkbox)
            .add_observer(on_number)
            .add_observer(on_button)
            .add_observer(on_list_remove)
            .add_observer(on_list_activate);
    }
}

/// What the dialog makes or edits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// A new connector of the assembly.
    New(LocalConnectorId),
    /// One of the assembly's connectors.
    Local(LocalConnectorId),
    /// A mate's connector (its index among the mate's connectors, then tabs; A23.2).
    Mate { mate: MateId, index: usize },
}

/// The open Mate connector dialog of an assembly.
#[derive(Resource, Debug, Clone)]
pub struct ConnectorSession {
    pub element: ElementId,
    pub target: Target,
    pub name: String,
    pub origin_type: OriginType,
    /// The connector (its instance is its owner), once an origin entity is picked.
    pub connector: Option<MateConnector>,
    pub realign: bool,
    pub move_on: bool,
    pub owner_on: bool,
    pub field: AppliedField,
    /// The entity last under the pointer (its points stay pickable near it).
    pub sticky: Option<(InstanceId, EntityRef)>,
}

impl ConnectorSession {
    fn valid(&self) -> bool {
        self.connector.is_some_and(|c| self.origin_type == OriginType::OnEntity || c.edit.between.is_some())
    }

    /// The connector with the dialog's switches applied (Between off: no between entity; Realign
    /// or Move off: none of theirs).
    fn effective(&self) -> Option<MateConnector> {
        let mut c = self.connector?;
        if self.origin_type == OriginType::OnEntity {
            c.edit.between = None;
        }
        if !self.realign {
            c.edit.primary = None;
            c.edit.secondary = None;
        }
        if !self.move_on {
            c.edit.translation = [0.0; 3];
            c.edit.rotation = 0.0;
        }
        Some(c)
    }
}

/// The mate connector row hovered in the Mate Features list (A23.1): its mate and index.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ConnectorHover(pub Option<(MateId, usize)>);

/// The dialog.
#[derive(Component)]
struct ToolDialog;

/// What the dialog was built from (it is rebuilt when this changes).
#[derive(Component, Debug, Clone, PartialEq)]
struct ToolKey(String);

// ---------------------------------------------------------------------------------------------
// Opening

/// The next "Mate connector N" of the assembly.
fn next_name(asm: &cadrs_core::assembly::Assembly) -> String {
    let n = asm.connectors.iter().filter_map(|c| c.name.strip_prefix("Mate connector")?.trim().parse::<u32>().ok()).max().unwrap_or(0);
    format!("Mate connector {}", n + 1)
}

fn close_others(world: &mut World) {
    world.remove_resource::<super::relation_dialog::RelationSession>();
    super::mate_dialog::cancel(world);
    world.remove_resource::<super::group_dialog::GroupSession>();
    super::animate::stop(world);
}

/// The Mate connector tool (the toolbar button, Ctrl+M): a new connector of the assembly.
pub fn open_new(world: &mut World) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(asm) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
    let name = next_name(asm);
    close_others(world);
    world.insert_resource(ConnectorSession {
        element,
        target: Target::New(LocalConnectorId::new()),
        name,
        origin_type: OriginType::OnEntity,
        connector: None,
        realign: false,
        move_on: false,
        owner_on: true,
        field: AppliedField::ConnectorOrigin,
        sticky: None,
    });
    world.resource_mut::<crate::viewport::Selection>().0.clear();
}

/// **Add mate connector to instance origin…** (P3B.9, X15): the Mate connector dialog with a new
/// connector at the instance's origin (its part's origin; a subassembly's or rigid studio's own
/// origin, on its first part), ✓ adds it to the assembly.
pub fn open_at_instance_origin(world: &mut World, instance: InstanceId) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(asm) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
    let Some(inst) = asm.instance(instance) else { return };
    let name = next_name(asm);
    // The connector's owner is a part: the instance itself, or its first part (the origin moved
    // into that part's coordinates).
    let (owner, frame) = if inst.source.is_composite() {
        match cadrs_core::assembly::structure::occurrences(&doc.doc, asm).into_iter().find(|o| o.top == instance) {
            Some(o) => (o.id, ConnectorFrame::default().moved(&o.in_top.inverse())),
            None => return,
        }
    } else {
        (instance, ConnectorFrame::default())
    };
    close_others(world);
    world.insert_resource(session_on(element, Target::New(LocalConnectorId::new()), name, MateConnector::at(owner, frame)));
    world.resource_mut::<crate::viewport::Selection>().0.clear();
}

/// Edits one of the assembly's connectors.
pub fn edit_local(world: &mut World, id: LocalConnectorId) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(l) = doc.active_element().and_then(|e| e.assembly_model()?.local_connector(id).cloned()) else { return };
    close_others(world);
    world.insert_resource(session_on(element, Target::Local(id), l.name, l.connector));
}

/// **Edit** on a mate's connector (A23.2): the dialog on it.
pub fn edit_mate_connector(world: &mut World, mate: MateId, index: usize) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(m) = doc.active_element().and_then(|e| e.assembly_model()?.mate(mate)?.mate().cloned()) else { return };
    let Some(c) = m.all_connectors().nth(index).copied() else { return };
    if c.surface_kind().is_some() || c.is_origin() {
        return;
    }
    close_others(world);
    world.insert_resource(session_on(element, Target::Mate { mate, index }, "Mate connector".into(), c));
}

fn session_on(element: ElementId, target: Target, name: String, c: MateConnector) -> ConnectorSession {
    ConnectorSession {
        element,
        target,
        name,
        origin_type: if c.edit.between.is_some() { OriginType::BetweenEntities } else { OriginType::OnEntity },
        connector: Some(c),
        realign: c.edit.realigned(),
        move_on: c.edit.moved(),
        owner_on: true,
        field: AppliedField::ConnectorOrigin,
        sticky: None,
    }
}

// ---------------------------------------------------------------------------------------------
// Frames and names

/// The source part solid of an occurrence of the active assembly, and its placement.
pub fn solid_of(doc: &ActiveDocument, parts: &mut super::AssemblyParts, i: InstanceId) -> Option<(Pose, Arc<Solid>)> {
    let (pose, element, part) = super::occurrence_source(doc, parts, i)?;
    let pose = parts.preview.get(&i).copied().unwrap_or(pose);
    let b = parts.build(&doc.doc, element)?;
    Some((pose, b.part(part)?.solid.clone()))
}

/// A connector's frame in the assembly (its instance's placement now; the Origin at the
/// identity; an assembly connector resolved).
pub fn world_frame(doc: &ActiveDocument, parts: &mut super::AssemblyParts, c: &MateConnector) -> Option<ConnectorFrame> {
    if c.is_origin() {
        return Some(c.local_frame(None));
    }
    if let ConnectorAnchor::Local { id } = c.anchor
        && let Some(l) = doc.active_element().and_then(|e| e.assembly_model()?.local_connector(id).map(|l| l.connector))
    {
        let (pose, solid) = solid_of(doc, parts, l.instance)?;
        let base = l.local_frame(Some(&solid));
        return Some(c.adjust_on(None, base).moved(&pose));
    }
    let (pose, solid) = solid_of(doc, parts, c.instance)?;
    Some(c.local_frame(Some(&solid)).moved(&pose))
}

/// "Slot Plate" for an instance (its part's name without the number).
fn part_name(cache: &PartCache, i: InstanceId) -> String {
    let n = cache.part_name(super::occurrence_part(i)).unwrap_or("instance");
    n.rsplit_once(" <").map(|(p, _)| p).unwrap_or(n).to_string()
}

/// How a field or a list names a connector: "Origin", "Mate connector 2" (an explicit one's
/// name), "Mate connector of Slot Plate" (an implicit one), "Face of Pin" (a Tangent entity).
pub fn connector_label(doc: &ActiveDocument, cache: &PartCache, c: &MateConnector) -> String {
    if c.is_origin() {
        return "Origin".into();
    }
    match c.anchor {
        ConnectorAnchor::Explicit { feature } => {
            let model = doc.active_element().and_then(|e| e.assembly_model());
            let name = model.and_then(|m| {
                let o = cadrs_core::assembly::structure::occurrences(&doc.doc, m).into_iter().find(|o| o.id == c.instance)?;
                Some(doc.doc.element(o.element)?.feature(feature)?.name.clone())
            });
            format!("{} of {}", name.unwrap_or_else(|| "Mate connector".into()), part_name(cache, c.instance))
        }
        ConnectorAnchor::Local { id } => {
            let name = doc.active_element().and_then(|e| Some(e.assembly_model()?.local_connector(id)?.name.clone()));
            name.unwrap_or_else(|| "Mate connector".into())
        }
        _ => match c.surface_kind() {
            Some(k) => format!("{} of {}", k.noun(), part_name(cache, c.instance)),
            None => format!("Mate connector of {}", part_name(cache, c.instance)),
        },
    }
}

fn entity_label(cache: &PartCache, i: InstanceId, e: &EntityRef) -> String {
    let what = match e {
        EntityRef::Face(_) => "Face",
        EntityRef::Edge(_) => "Edge",
        EntityRef::Vertex(_) => "Vertex",
    };
    format!("{what} of {}", part_name(cache, i))
}

/// The explicit connectors in the view: each instance's Part Studio connectors and the
/// assembly's own, as mate connectors with their frames in the assembly.
pub fn explicit_connectors(doc: &ActiveDocument, parts: &mut super::AssemblyParts, cache: &PartCache) -> Vec<(MateConnector, ConnectorFrame)> {
    // Every occurrence's placement and source once (looking each part's up walked the whole
    // assembly per part, every frame: quadratic in a large assembly's occurrences).
    let occ: std::collections::HashMap<InstanceId, (Pose, cadrs_core::ElementId, cadrs_core::PartId)> = doc
        .active_element()
        .and_then(|e| e.assembly_model())
        .map(|m| cadrs_core::assembly::structure::occurrences(&doc.doc, m).into_iter().map(|o| (o.id, (o.pose, o.element, o.part))).collect())
        .unwrap_or_default();
    // The source's last finished rebuild (never waiting: this is drawn every frame).
    let solid = |parts: &mut super::AssemblyParts, i: InstanceId| -> Option<(Pose, Arc<Solid>)> {
        let (pose, element, part) = match parts.ghosts.iter().find(|g| g.id == i) {
            Some(g) => (g.pose, g.source.element(), g.source.part()?),
            None => *occ.get(&i)?,
        };
        let pose = parts.preview.get(&i).copied().unwrap_or(pose);
        let b = parts.build_within(&doc.doc, element, Some(std::time::Duration::ZERO)).0?;
        Some((pose, b.part(part)?.solid.clone()))
    };
    let mut out = Vec::new();
    for p in &cache.parts {
        if cache.is_hidden(p.id) || p.solid.connectors.is_empty() {
            continue;
        }
        let i = super::occurrence_of(p.id);
        let Some((pose, _)) = solid(parts, i) else { continue };
        let back = pose.inverse();
        for sc in &p.solid.connectors {
            let w = ConnectorFrame::new(sc.frame.origin, sc.frame.normal(), sc.frame.u);
            // The view's parts are in assembly coordinates: back to the part's for the anchor.
            out.push((MateConnector::explicit(i, sc.feature, w.moved(&back)), w));
        }
    }
    let locals: Vec<LocalConnector> = doc.active_element().and_then(|e| e.assembly_model()).map(|m| m.connectors.clone()).unwrap_or_default();
    for l in locals {
        let Some((pose, s)) = solid(parts, l.connector.instance) else { continue };
        if cache.is_hidden(super::occurrence_part(l.connector.instance)) {
            continue;
        }
        let local = l.connector.local_frame(Some(&s));
        let mut c = MateConnector::at(l.connector.instance, local);
        c.anchor = ConnectorAnchor::Local { id: l.id };
        out.push((c, local.moved(&pose)));
    }
    out
}

/// How near (px) the pointer must be to an explicit connector's origin to take it.
pub const EXPLICIT_PX: f32 = 12.0;

/// The explicit connector nearest the pointer (within [`EXPLICIT_PX`]).
pub fn nearest_explicit(list: &[(MateConnector, ConnectorFrame)], view: &crate::camera::ViewState, at: Vec2) -> Option<(MateConnector, ConnectorFrame)> {
    list.iter()
        .map(|(c, f)| (view.project(v3(f.origin)).distance(at), *c, *f))
        .filter(|(d, ..)| *d <= EXPLICIT_PX)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c, f)| (c, f))
}

// ---------------------------------------------------------------------------------------------
// Accept, cancel

pub fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<ConnectorSession>().cloned() else { return };
    if !s.valid() {
        return;
    }
    let Some(c) = s.effective() else { return };
    let ok = match s.target {
        Target::New(id) | Target::Local(id) => {
            // The frame it resolves to now, kept as the fallback.
            let mut c = c;
            world.resource_scope(|world, mut parts: Mut<super::AssemblyParts>| {
                if let Some(doc) = world.get_resource::<ActiveDocument>()
                    && let Some((_, solid)) = solid_of(doc, &mut parts, c.instance)
                {
                    c.frame = c.base_frame(Some(&solid));
                }
            });
            super::run(world, &SetLocalConnector { element: s.element, connector: LocalConnector { id, name: s.name.clone(), connector: c } })
        }
        Target::Mate { mate, index } => {
            let Some((feature, poses)) = solve_mate(world, mate, index, c, false) else { return };
            super::run(world, &SetMateFeature { element: s.element, feature, poses })
        }
    };
    if ok {
        cancel(world);
    }
}

pub fn cancel(world: &mut World) {
    if world.remove_resource::<ConnectorSession>().is_some() {
        world.resource_mut::<super::AssemblyParts>().preview.clear();
    }
}

/// The mate with its connector `index` replaced by `c`, and the placements solving it (only
/// that mate snapped when `snap_only`, as the dialog previews it; every mate on ✓).
fn solve_mate(world: &mut World, mate: MateId, index: usize, c: MateConnector, snap_only: bool) -> Option<(cadrs_core::assembly::mate::MateFeature, Vec<(InstanceId, Pose)>)> {
    let doc_model = world.get_resource::<ActiveDocument>()?.active_element()?.assembly_model()?.clone();
    let mut feature = doc_model.mate(mate)?.clone();
    {
        let MateKind::Mate(m) = &mut feature.kind else { return None };
        let n = m.connectors.len();
        if index < n {
            m.connectors[index] = c;
        } else if let Some(t) = m.tabs.get_mut(index - n) {
            *t = c;
        }
    }
    let (mut model, solids) = super::mate_dialog::model_and_solids(world)?;
    if let Some(f) = model.mates.iter_mut().find(|f| f.id == mate) {
        *f = feature.clone();
    }
    let ground = cadrs_core::assembly::solver::grounded(&model);
    let m = feature.mate()?;
    let mover = m.all_connectors().map(|c| c.instance).find(|i| !ground.contains(i));
    // The preview keeps the mate's free motion where it is (a realigned slot's X turns the
    // connector, not the part, A23.3); only what the edit constrains moves.
    let opts = SolveOptions { movers: mover.into_iter().collect(), snap: snap_only.then_some(mate), snap_only, hold_free: snap_only, ..Default::default() };
    let sol = cadrs_core::assembly::solve(&model, &solids, &opts);
    Some((feature, sol.changed(&model)))
}

/// A change to the session, then (editing a mate's connector) the mate's instance placed again.
fn change(commands: &mut Commands, f: impl FnOnce(&mut ConnectorSession) + Send + 'static) {
    commands.queue(move |world: &mut World| {
        let Some(mut s) = world.get_resource::<ConnectorSession>().cloned() else { return };
        f(&mut s);
        world.insert_resource(s.clone());
        preview(world, &s);
    });
}

fn preview(world: &mut World, s: &ConnectorSession) {
    let Target::Mate { mate, index } = s.target else { return };
    let Some(c) = s.effective() else { return };
    let poses = solve_mate(world, mate, index, c, true).map(|(_, p)| p).unwrap_or_default();
    world.resource_mut::<super::AssemblyParts>().preview = poses.into_iter().collect();
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<ToolDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<ToolDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(cancel);
    }
}

// ---------------------------------------------------------------------------------------------
// The dialog's input (the same rows and roles as the Part Studio's; these act only while this
// dialog is open)

fn on_select(ev: On<SelectChange>, q: Query<&Role>, session: Option<Res<ConnectorSession>>, mut commands: Commands) {
    if session.is_none() || q.get(ev.entity) != Ok(&Role::ConnectorOriginType) {
        return;
    }
    let t = OriginType::ALL[ev.index.min(1)];
    change(&mut commands, move |s| {
        s.origin_type = t;
        s.field = if t == OriginType::BetweenEntities { AppliedField::ConnectorBetween } else { AppliedField::ConnectorOrigin };
    });
}

fn on_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, session: Option<Res<ConnectorSession>>, mut commands: Commands) {
    let (Ok(name), Some(_)) = (q.get(ev.entity), session) else { return };
    let on = ev.checked;
    let name = name.as_str().to_string();
    let field = crate::pattern_dialog::field_after(&name, on);
    match name.as_str() {
        "mate-connector-realign-checkbox" => change(&mut commands, move |s| s.realign = on),
        "mate-connector-move-checkbox" => change(&mut commands, move |s| s.move_on = on),
        "mate-connector-owner-checkbox" => change(&mut commands, move |s| s.owner_on = on),
        _ => return,
    }
    if let Some(f) = field {
        change(&mut commands, move |s| s.field = f);
    }
}

fn on_number(
    ev: On<NumberFieldCommit>,
    q: Query<&Role>,
    mut q_state: Query<&mut NumberFieldState>,
    session: Option<Res<ConnectorSession>>,
    units: Res<crate::WorkspaceUnits>,
    mut commands: Commands,
) {
    if session.is_none() {
        return;
    }
    let Ok(role) = q.get(ev.entity).copied() else { return };
    if !matches!(role, Role::ConnectorX | Role::ConnectorY | Role::ConnectorZ | Role::ConnectorRotation) {
        return;
    }
    let angle = role == Role::ConnectorRotation;
    let text = ev.text.trim().to_string();
    let v = match units.0.eval(&text, if angle { Quantity::Angle } else { Quantity::Length }) {
        Ok(v) if v.is_finite() => v,
        _ => {
            if let Ok(mut st) = q_state.get_mut(ev.entity) {
                st.text = text;
                st.error = true;
            }
            return;
        }
    };
    if let Ok(mut st) = q_state.get_mut(ev.entity) {
        st.error = false;
    }
    change(&mut commands, move |s| {
        let Some(c) = s.connector.as_mut() else { return };
        match role {
            Role::ConnectorX => c.edit.translation[0] = v,
            Role::ConnectorY => c.edit.translation[1] = v,
            Role::ConnectorZ => c.edit.translation[2] = v,
            _ => c.edit.rotation = v.to_radians(),
        }
    });
    if ev.enter {
        commands.queue(|world: &mut World| world.resource_mut::<InputFocus>().clear());
    }
}

fn on_button(a: On<bevy::ui_widgets::Activate>, q: Query<&Role>, q_name: Query<&Name>, session: Option<Res<ConnectorSession>>, mut commands: Commands) {
    // The assembly toolbar's Mate connector button.
    if q_name.get(a.entity).is_ok_and(|n| n.as_str() == "mate-connector") {
        commands.queue(|world: &mut World| {
            if world.get_resource::<ActiveDocument>().and_then(super::active_assembly).is_some() {
                open_new(world);
            }
        });
        return;
    }
    if session.is_none() {
        return;
    }
    match q.get(a.entity) {
        Ok(Role::ConnectorFlip) => change(&mut commands, |s| {
            if let Some(c) = s.connector.as_mut() {
                c.flip = !c.flip;
            }
        }),
        Ok(Role::ConnectorReorient) => change(&mut commands, |s| {
            if let Some(c) = s.connector.as_mut() {
                c.reorient = (c.reorient + 1) % 4;
            }
        }),
        _ => {}
    }
}

fn on_list_remove(ev: On<SelectionListRemove>, q: Query<&Role>, session: Option<Res<ConnectorSession>>, mut commands: Commands) {
    if session.is_none() {
        return;
    }
    let Ok(role) = q.get(ev.entity).copied() else { return };
    change(&mut commands, move |s| match role {
        Role::ConnectorOrigin if !matches!(s.target, Target::Mate { .. }) => s.connector = None,
        Role::ConnectorBetween => {
            if let Some(c) = s.connector.as_mut() {
                c.edit.between = None;
            }
        }
        Role::ConnectorPrimary => {
            if let Some(c) = s.connector.as_mut() {
                c.edit.primary = None;
            }
        }
        Role::ConnectorSecondary => {
            if let Some(c) = s.connector.as_mut() {
                c.edit.secondary = None;
            }
        }
        _ => {}
    });
}

fn on_list_activate(ev: On<SelectionListActivate>, q: Query<&Role>, session: Option<Res<ConnectorSession>>, mut commands: Commands) {
    if session.is_none() {
        return;
    }
    let Some(field) = q.get(ev.entity).ok().and_then(|r| crate::pattern_dialog::list_field(*r)) else { return };
    change(&mut commands, move |s| s.field = field);
}

// ---------------------------------------------------------------------------------------------
// Picking

/// A click in the view while the dialog is open: the active field's entity.
#[allow(clippy::too_many_arguments)]
fn tool_picks(
    mut picks: MessageReader<PickRequest>,
    session: Option<Res<ConnectorSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut parts: ResMut<super::AssemblyParts>,
    cache: Res<PartCache>,
    highlight: Res<PlaneHighlight>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    vdrag: Res<ViewportDrag>,
    mut commands: Commands,
) {
    let (Some(s), Some(doc)) = (session, doc) else {
        return;
    };
    // The entity under the pointer stays pickable near its points (as the mate dialog's).
    if let Some(e) = highlight.viewport.as_ref().and_then(entity_of)
        && s.sticky != Some(e)
    {
        commands.queue(move |world: &mut World| {
            if let Some(mut s) = world.get_resource_mut::<ConnectorSession>() {
                s.sticky = Some(e);
            }
        });
    }
    for p in picks.read() {
        let at = rect.offset(vdrag.pointer());
        let hovered = p.0.as_ref().and_then(entity_of);
        let owner = s.connector.map(|c| c.instance);
        let mut next = s.clone();
        match s.field {
            AppliedField::ConnectorOrigin => {
                let entity = hovered.or(s.sticky);
                let Some((instance, entity)) = entity else { continue };
                // Editing a mate's connector keeps its instance.
                if matches!(s.target, Target::Mate { .. }) && Some(instance) != owner {
                    continue;
                }
                let Some(pts) = connectors::entity_points(&doc, &mut parts, instance, entity) else { continue };
                let Some(best) = pts.nearest(&view.view, at) else { continue };
                let mut c = MateConnector::implicit(instance, &best);
                if let Some(old) = s.connector.filter(|o| o.instance == instance) {
                    c.flip = old.flip;
                    c.reorient = old.reorient;
                    c.edit = old.edit;
                }
                next.connector = Some(c);
                if s.origin_type == OriginType::BetweenEntities && c.edit.between.is_none() {
                    next.field = AppliedField::ConnectorBetween;
                }
            }
            AppliedField::ConnectorBetween | AppliedField::ConnectorPrimary | AppliedField::ConnectorSecondary => {
                let Some((instance, entity)) = hovered else { continue };
                if Some(instance) != owner {
                    continue;
                }
                let Some(c) = next.connector.as_mut() else { continue };
                match s.field {
                    AppliedField::ConnectorBetween => c.edit.between = if c.edit.between == Some(entity) { None } else { Some(entity) },
                    f => {
                        let Some((_, solid)) = solid_of(&doc, &mut parts, instance) else { continue };
                        let Some(d) = entity_direction(&solid, &entity) else { continue };
                        let primary = f == AppliedField::ConnectorPrimary;
                        let slot = if primary { &mut c.edit.primary } else { &mut c.edit.secondary };
                        *slot = if slot.is_some_and(|(e, _)| e == entity) { None } else { Some((entity, d)) };
                        // The axis now follows the entity: its earlier flip or quarter turns go.
                        if primary {
                            c.flip = false;
                        } else {
                            c.reorient = 0;
                        }
                    }
                }
            }
            AppliedField::ConnectorOwner => {
                // Another instance owns it: the connector is kept where it is, on that
                // instance (a fixed frame in its coordinates).
                let Some(part) = p.0.and_then(|p| p.part()) else { continue };
                let instance = super::occurrence_of(part);
                let Some(c) = s.connector.filter(|c| c.instance != instance) else { continue };
                if matches!(s.target, Target::Mate { .. }) {
                    continue;
                }
                let (Some(w), Some((pose, _))) = (world_frame(&doc, &mut parts, &c), solid_of(&doc, &mut parts, instance)) else { continue };
                next.connector = Some(MateConnector::at(instance, w.moved(&pose.inverse())));
            }
            _ => continue,
        }
        let _ = &cache;
        commands.queue(move |world: &mut World| {
            world.insert_resource(next.clone());
            preview(world, &next);
        });
    }
}

// ---------------------------------------------------------------------------------------------
// The dialog

fn texts(s: &ConnectorSession, units: &cadrs_sketch::units::Units) -> [String; 4] {
    let e = s.connector.map(|c| c.edit).unwrap_or_default();
    let len = |v: f64| units.with_unit(v, Quantity::Length);
    [len(e.translation[0]), len(e.translation[1]), len(e.translation[2]), units.with_unit(e.rotation.to_degrees(), Quantity::Angle)]
}

fn items(s: &ConnectorSession, doc: &ActiveDocument, cache: &PartCache, role: Role) -> Vec<String> {
    let Some(c) = s.connector else { return Vec::new() };
    let i = c.instance;
    match role {
        Role::ConnectorOrigin => vec![match c.anchor {
            ConnectorAnchor::Implicit { owner, .. } => entity_label(cache, i, &owner),
            _ => connector_label(doc, cache, &c),
        }],
        Role::ConnectorBetween => c.edit.between.iter().map(|e| entity_label(cache, i, e)).collect(),
        Role::ConnectorPrimary => c.edit.primary.iter().map(|(e, _)| entity_label(cache, i, e)).collect(),
        Role::ConnectorSecondary => c.edit.secondary.iter().map(|(e, _)| entity_label(cache, i, e)).collect(),
        Role::ConnectorOwner => vec![cache.part_name(super::occurrence_part(i)).unwrap_or("instance").to_string()],
        _ => Vec::new(),
    }
}

/// Spawns, rebuilds and removes the dialog.
#[allow(clippy::too_many_arguments)]
fn sync_dialog(
    session: Option<Res<ConnectorSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    units: Res<crate::WorkspaceUnits>,
    theme: Res<Theme>,
    q_dialog: Query<(Entity, &ToolKey), With<ToolDialog>>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let (Some(s), Some(doc)) = (session, doc) else {
        for (e, _) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let roles = [Role::ConnectorOrigin, Role::ConnectorBetween, Role::ConnectorPrimary, Role::ConnectorSecondary, Role::ConnectorOwner];
    let lists: Vec<(Role, Vec<String>)> = roles.iter().map(|r| (*r, items(&s, &doc, &cache, *r))).collect();
    let tx = texts(&s, &units.0);
    let flip = s.connector.is_some_and(|c| c.flip);
    let key = ToolKey(format!(
        "{} {:?} {} {} {} {:?} {} {:?} {:?} {} {}",
        s.name,
        s.origin_type,
        s.realign,
        s.move_on,
        s.owner_on,
        s.field,
        flip,
        lists,
        tx,
        s.valid(),
        s.connector.map(|c| c.reorient).unwrap_or(0)
    ));
    if q_dialog.iter().any(|(_, k)| *k == key) {
        return;
    }
    for (e, _) in &q_dialog {
        commands.entity(e).try_despawn();
    }
    let Some(area) = q_area.iter().next() else { return };
    let v = crate::pattern_dialog::ConnectorView {
        origin_type: s.origin_type,
        alignment: false,
        realign: s.realign,
        move_on: s.move_on,
        owner_on: s.owner_on,
        flip,
        texts: tx,
        owner_placeholder: "Select owner instance",
    };
    let field = s.field;
    let tb = theme.clone();
    let tf = theme.clone();
    let d = commands
        .spawn((
            ToolDialog,
            key,
            DespawnOnExit(AppState::Document),
            FeatureDialog::new("mate-connector-dialog")
                .title(s.name.clone())
                .valid(s.valid())
                .width(216.0)
                .body_padding(UiRect::ZERO)
                .body(move |b| {
                    let items_of = |r: Role| lists.iter().find(|(x, _)| *x == r).map(|(_, v)| v.clone()).unwrap_or_default();
                    crate::applied_dialog::body_column(b, |b| crate::pattern_dialog::connector_rows(b, &tb, &v, field, &items_of));
                })
                .footer(move |f| {
                    f.spawn(Node { flex_grow: 1.0, ..default() });
                    f.spawn((Name::new("mate-connector-help"), icon("help-filled", 14.0, Color::srgb_u8(0xa8, 0xa8, 0xa8)), Tooltip::new("Help")));
                    let _ = &tf;
                })
                .build(&theme),
        ))
        .id();
    commands.entity(area).add_child(d);
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// An entity of a part of the view (assembly coordinates) outlined: an edge, a face's loops.
fn outline(edges: &mut Gizmos<crate::parts::EdgeHighlightGizmos>, solid: &Solid, e: EntityRef, color: Color) {
    match e {
        EntityRef::Edge(e) => {
            if let Some(e) = solid.edge(&e) {
                edges.linestrip(e.points.iter().map(|p| v3(*p)), color);
            }
        }
        EntityRef::Face(f) => {
            if let Some(f) = solid.face(&f) {
                for l in &f.loops {
                    let mut pts: Vec<Vec3> = l.iter().map(|p| v3(*p)).collect();
                    pts.extend(pts.first().copied());
                    edges.linestrip(pts, color);
                }
            }
        }
        EntityRef::Vertex(_) => {}
    }
}

/// The explicit connectors on the instances (K hides them), the one a mate dialog would take
/// under the pointer in the hover colour, and a connector row hovered in the Mate Features list.
#[allow(clippy::too_many_arguments)]
fn draw_explicit_connectors(
    doc: Option<Res<ActiveDocument>>,
    mut parts: ResMut<super::AssemblyParts>,
    cache: Res<PartCache>,
    shown: Res<crate::pattern::MateConnectorsShown>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    vdrag: Res<ViewportDrag>,
    mate: Option<Res<super::mate_dialog::MateSession>>,
    hover: Res<ConnectorHover>,
    list_hover: Res<super::list::InstanceConnectorHover>,
    mut g: Gizmos<crate::parts::VertexGizmos>,
    mut halo: Gizmos<ConnectorHaloGizmos>,
    mut line: Gizmos<ConnectorGizmos>,
    mut edges: Gizmos<crate::parts::EdgeHighlightGizmos>,
) {
    let Some(doc) = doc else { return };
    if super::active_assembly(&doc).is_none() {
        return;
    }
    let v = view.view;
    if shown.0 {
        let list = explicit_connectors(&doc, &mut parts, &cache);
        let picking = mate.as_ref().is_some_and(|m| m.connectors.len() < 2 && m.mate_type.uses_connectors());
        let first = mate.as_ref().and_then(|m| m.connectors.first().map(|c| c.instance));
        let near = picking
            .then(|| {
                let open: Vec<(MateConnector, ConnectorFrame)> = list.iter().filter(|(c, _)| Some(c.instance) != first).copied().collect();
                nearest_explicit(&open, &v, rect.offset(vdrag.pointer()))
            })
            .flatten()
            .map(|(c, _)| c);
        for (c, f) in &list {
            let hl = (near == Some(*c)).then_some(crate::parts::HOVER);
            let pf = cadrs_sketch::PlaneFrame { origin: f.origin, u: f.x, v: f.y() };
            crate::pattern::connector_glyph(&mut g, &pf, v.scale, hl);
        }
    }
    // P3B.7 judge: the Part Studio connector row hovered in the Instances list (even with K).
    if let Some((instance, feature)) = list_hover.0 {
        for (c, f) in explicit_connectors(&doc, &mut parts, &cache) {
            if c.instance == instance && matches!(c.anchor, ConnectorAnchor::Explicit { feature: x } if x == feature) {
                connectors::draw_glyph(&mut line, &mut halo, &v, &f, crate::parts::HOVER);
            }
        }
    }
    // A23.1: the hovered connector row's connector and its entity.
    if let Some((mate_id, index)) = hover.0
        && let Some(c) = doc.active_element().and_then(|e| e.assembly_model()?.mate(mate_id)?.mate()?.all_connectors().nth(index).copied())
        && let Some(f) = world_frame(&doc, &mut parts, &c)
    {
        connectors::draw_glyph(&mut line, &mut halo, &v, &f, crate::parts::HOVER);
        // Its geometry too (A23.1).
        let owner = match c.anchor {
            ConnectorAnchor::Implicit { owner, .. } => Some(owner),
            ConnectorAnchor::Surface { entity, .. } => Some(entity),
            _ => None,
        };
        if let (Some(e), Some(part)) = (owner, cache.part(super::occurrence_part(c.instance))) {
            outline(&mut edges, &part.solid, e, crate::parts::HOVER);
        }
    }
}

/// While the dialog is open: the connector as it will be, its entities in the selection
/// colour, and the implicit points under the pointer for the Origin entity field.
#[allow(clippy::too_many_arguments)]
fn draw_session(
    session: Option<Res<ConnectorSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut parts: ResMut<super::AssemblyParts>,
    cache: Res<PartCache>,
    highlight: Res<PlaneHighlight>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    vdrag: Res<ViewportDrag>,
    mut halo: Gizmos<ConnectorHaloGizmos>,
    mut line: Gizmos<ConnectorGizmos>,
    mut edges: Gizmos<crate::parts::EdgeHighlightGizmos>,
) {
    let (Some(s), Some(doc)) = (session, doc) else { return };
    let v = view.view;
    if let Some(c) = s.effective() {
        let part = cache.part(super::occurrence_part(c.instance));
        let mut ents: Vec<EntityRef> = Vec::new();
        if let ConnectorAnchor::Implicit { owner, .. } = c.anchor {
            ents.push(owner);
        }
        ents.extend(c.edit.between);
        ents.extend(c.edit.primary.map(|(e, _)| e));
        ents.extend(c.edit.secondary.map(|(e, _)| e));
        if let Some(part) = part {
            for e in ents {
                outline(&mut edges, &part.solid, e, crate::parts::SELECTED);
            }
        }
        if let Some(f) = world_frame(&doc, &mut parts, &c) {
            connectors::draw_glyph(&mut line, &mut halo, &v, &f, crate::parts::SELECTED);
        }
    }
    if s.field != AppliedField::ConnectorOrigin {
        return;
    }
    let at = rect.offset(vdrag.pointer());
    let Some((instance, entity)) = highlight.viewport.as_ref().and_then(entity_of).or(s.sticky) else { return };
    if matches!(s.target, Target::Mate { .. }) && s.connector.is_some_and(|c| c.instance != instance) {
        return;
    }
    let Some(pts) = connectors::entity_points(&doc, &mut parts, instance, entity) else { return };
    let dots: Vec<Vec3> = pts.points.iter().map(|p| v3(pts.world(p).origin)).collect();
    connectors::draw_points(&mut halo, &mut line, &v, &dots);
    if let Some(best) = pts.nearest(&v, at) {
        connectors::draw_glyph(&mut line, &mut halo, &v, &pts.world(&best), crate::parts::HOVER);
    }
}
