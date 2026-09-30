//! The mate dialog (P3B.2, P3B.3; `intro-to-assemblies.md` A6.1–A6.11, A7, A8–A12;
//! `ex2-step7.png`, `ex2-step9.png`, `ex2-step12.png`, `ex2-step16.png`,
//! `lesson-mate-dialog-offset.png`, `lesson-mate-dialog-limits.png`), opened by the toolbar's
//! mate buttons (M for Fastened), or by editing a mate:
//!
//! - The title ("Revolute 1"), ✓ and ✕; the **mate type** dropdown with all ten types (the type
//!   can be changed at any time without picking again); the **Mate connectors** field with its
//!   two rows, "Mate connector of <instance>" (long names cut in the middle, so two rows stay
//!   apart), each with the connector glyph and ✕, and **Reorder items** (⇅, drag, Done). A Pin
//!   slot's first connector is the slot's (A9.2).
//! - **Tangent** (A11) takes two **entities** instead (faces, straight edges, vertices; no
//!   connectors), has **Tangent propagation** (on by default) and Flip (the other side).
//! - **Width** (A12) has two fields: **Tab mate connectors** (one or two) and **Width mate
//!   connectors** (exactly two); a pick goes to the active field (click a field to make it
//!   active; the tabs hand over when they have two), and an instance can't be in both (the field
//!   turns red).
//! - **Offset** ✓ (connector mates): X, Y, Z, "Rotate about X/Y/Z" and Rotation angle;
//!   **Limits** ✓ (Revolute, Slider, Cylindrical, Pin slot, Planar): min / max for each of the
//!   type's DOF (X, Y, Z travel, Z angle). Values take units and expressions ("0.5 in",
//!   "90 deg"). **Simulation connection** (P3F.5) bonds the two parts in a simulation.
//! - The bottom row: **Flip primary axis**, **Reorient secondary axis** (90° steps; both act on
//!   the first connector, or the second when the first's instance can't move, A6.8),
//!   **Animate** ▶ (A6.9: plays the mate's first DOF, see [`super::animate`]), **Solve**, help.
//! - **Picking** (A6.4–A6.7): hovering a face, edge or vertex of an instance shows its implicit
//!   connector points (dots; the nearest one as a connector glyph), Shift locks the entity (the
//!   hover highlight stays on it); a click takes the nearest point. The second pick **solves**:
//!   the first movable instance moves so the connectors coincide
//!   ([`cadrs_core::assembly::solver`], snap only).
//! - **Solving** (A6.11): not continuously. A pick, a flip, reorient, type, offset or limit
//!   change places the moving instance for this mate only; **Solve** re-solves every mate;
//!   **✓** solves every mate and adds the mate with the new placements as one undo step. ✕
//!   puts everything back.

use std::collections::HashMap;

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use cadrs_core::ElementId;
use cadrs_core::assembly::commands::{AddMateFeature, SetMateFeature};
use cadrs_core::assembly::connector::{ConnectorAnchor, EntityRef, MateConnector, surface_of};
use cadrs_core::assembly::mate::{Dof, Mate, MateFeature, MateId, MateKind, MateLimits, MateOffset, MateType, next_name};
use cadrs_core::assembly::solver::{Drive, SolveOptions, tangent_supported};
use cadrs_core::assembly::{Assembly, InstanceId, Pose};
use cadrs_sketch::units::Quantity;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, NumberField, NumberFieldCommit,
    NumberFieldState, OptionRow, Select, SelectChange, SelectState, SelectionList, SelectionListActivate, SelectionListMove,
    SelectionListRemove, SelectionListState,
};

use super::connectors::{self, ConnectorGizmos, ConnectorHaloGizmos, entity_of, v3};
use crate::parts::PartCache;
use crate::viewport::{PickRequest, PlaneHighlight, ViewportArea, ViewportDrag, ViewportRect, ViewportView};
use crate::{ActiveDocument, AppState};

pub struct MateDialogPlugin;

/// The mate dialog's systems (the animation runs after them).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MateDialogSet;

impl Plugin for MateDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (mate_picks, mate_lock, sync_preview, sync_mate_dialog, draw_mate)
                .chain()
                .in_set(MateDialogSet)
                .after(crate::viewport::apply_view_to_camera)
                .before(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |mut commands: Commands| commands.remove_resource::<MateSession>())
        .add_observer(on_accept)
        .add_observer(on_cancel)
        .add_observer(on_select)
        .add_observer(on_checkbox)
        .add_observer(on_number)
        .add_observer(on_button)
        .add_observer(on_list_remove)
        .add_observer(on_list_move)
        .add_observer(on_list_activate);
    }
}

/// The open mate dialog.
#[derive(Resource, Debug, Clone)]
pub struct MateSession {
    pub element: ElementId,
    pub id: MateId,
    pub name: String,
    /// Editing an existing mate (✓ replaces it).
    pub editing: bool,
    /// The edited mate was suppressed (kept on ✓).
    pub suppressed: bool,
    pub mate_type: MateType,
    /// The picked connectors (at most two), in the field's order: Tangent's entities, Width's
    /// Width pair.
    pub connectors: Vec<MateConnector>,
    /// Width: the Tab connectors (at most two).
    pub tabs: Vec<MateConnector>,
    /// Width: the field that takes picks (0 Tabs, 1 Widths).
    pub field: usize,
    /// A pick was refused (Width: an instance in both fields): that field is red.
    pub error: Option<usize>,
    pub offset_on: bool,
    pub offset: MateOffset,
    pub limits_on: bool,
    /// X, Y, Z min / max (mm) and angle min / max (radians); kept while Limits is off.
    pub x_limits: (f64, f64),
    pub y_limits: (f64, f64),
    pub z_limits: (f64, f64),
    pub angle_limits: (f64, f64),
    /// Tangent: propagation (A11.2) and the side (A11.3).
    pub propagate: bool,
    pub flip: bool,
    /// The placements shown (solved for this mate so far).
    pub preview: HashMap<InstanceId, Pose>,
    /// The entity Shift locked.
    pub locked: Option<(InstanceId, EntityRef)>,
    /// The entity last under the pointer: its points stay pickable while the pointer is near
    /// them, off the entity (a hole's axis middle is over the empty bore, A6.4).
    pub sticky: Option<(InstanceId, EntityRef)>,
    /// P3F.4: offsets typed as expressions naming the assembly's variables.
    pub exprs: Vec<(cadrs_core::assembly::mate::OffsetSlot, String)>,
    /// P3F.5 (A6.3): **Simulation connection**: the two parts are bonded in a simulation.
    pub simulation: bool,
}

impl MateSession {
    fn new(element: ElementId, id: MateId, name: String, t: MateType) -> Self {
        Self {
            element,
            id,
            name,
            editing: false,
            suppressed: false,
            mate_type: t,
            connectors: Vec::new(),
            tabs: Vec::new(),
            field: 0,
            error: None,
            offset_on: false,
            offset: MateOffset::default(),
            limits_on: false,
            x_limits: (0.0, 0.0),
            y_limits: (0.0, 0.0),
            z_limits: (0.0, 0.0),
            angle_limits: (0.0, 0.0),
            propagate: true,
            flip: false,
            preview: HashMap::new(),
            locked: None,
            sticky: None,
            exprs: Vec::new(),
            simulation: false,
        }
    }

    fn limits(&self, d: Dof) -> (f64, f64) {
        match d {
            Dof::X => self.x_limits,
            Dof::Y => self.y_limits,
            Dof::Z => self.z_limits,
            Dof::Angle => self.angle_limits,
        }
    }

    pub fn mate(&self) -> Option<Mate> {
        let [a, b] = [self.connectors.first()?, self.connectors.get(1)?];
        let mut m = if self.mate_type == MateType::Width {
            if self.tabs.is_empty() {
                return None;
            }
            Mate::width(self.tabs.clone(), [*a, *b])
        } else {
            Mate::new(self.mate_type, *a, *b)
        };
        m.offset = (self.offset_on && self.mate_type.has_offset()).then_some(self.offset);
        m.limits = (self.limits_on && self.mate_type.has_limits()).then(|| {
            let mut l = MateLimits::default();
            for d in self.mate_type.limit_dofs() {
                l.set(*d, Some(self.limits(*d)));
            }
            l
        });
        m.propagate = self.propagate;
        m.flip = self.flip && self.mate_type == MateType::Tangent;
        m.simulation = self.simulation;
        Some(m)
    }

    fn feature(&self) -> Option<MateFeature> {
        let mut f = MateFeature::new(self.id, self.name.clone(), MateKind::Mate(self.mate()?));
        f.suppressed = self.suppressed;
        if self.offset_on {
            f.exprs = self.exprs.clone();
        }
        Some(f)
    }

    /// The assembly as it would be with this mate, at the shown placements.
    pub fn model(&self, doc: &Assembly) -> Assembly {
        let mut m = doc.clone();
        for i in &mut m.instances {
            if let Some(p) = self.preview.get(&i.id) {
                i.pose = *p;
            }
        }
        m.mates.retain(|f| f.id != self.id);
        if let Some(mut f) = self.feature() {
            f.suppressed = false;
            match doc.mates.iter().position(|x| x.id == self.id) {
                Some(k) => m.mates.insert(k.min(m.mates.len()), f),
                None => m.mates.push(f),
            }
        }
        m
    }

    /// Which connector Flip and Reorient act on: the first, unless its instance can't move.
    fn adjusted(&self, doc: &Assembly) -> usize {
        let ground = cadrs_core::assembly::solver::grounded(doc);
        match self.connectors.first() {
            Some(c) if (ground.contains(&c.instance) || c.is_origin()) && self.connectors.len() > 1 => 1,
            _ => 0,
        }
    }

    /// The instance that moves when the mate is solved.
    pub fn mover(&self, doc: &Assembly) -> Option<InstanceId> {
        if self.mate_type == MateType::Width {
            let ground = cadrs_core::assembly::solver::grounded(doc);
            return self.tabs.iter().chain(&self.connectors).map(|c| c.instance).find(|i| !ground.contains(i));
        }
        let k = self.adjusted(doc);
        self.connectors.get(k).map(|c| c.instance)
    }

    /// Drives that keep the snap inside the limits (0, clamped).
    fn drives(&self) -> Vec<Drive> {
        let Some(m) = self.mate() else { return Vec::new() };
        let mut out = Vec::new();
        for d in self.mate_type.limit_dofs() {
            if let Some((lo, hi)) = m.limit(*d) {
                out.push(Drive { mate: self.id, dof: *d, value: 0.0f64.clamp(lo, hi) });
            }
        }
        out
    }

    /// Every connector picked (both fields of a Width).
    fn all(&self) -> impl Iterator<Item = &MateConnector> {
        self.tabs.iter().chain(self.connectors.iter())
    }

    /// Whether another pick is wanted.
    fn wants_pick(&self) -> bool {
        match self.mate_type {
            MateType::Width => (self.field == 0 && self.tabs.len() < 2) || (self.field == 1 && self.connectors.len() < 2),
            _ => self.connectors.len() < 2,
        }
    }
}

/// The dialog.
#[derive(Component)]
struct MateDialog;

/// What the dialog was built for; it is rebuilt when this changes.
#[derive(Component, Debug, Clone, PartialEq)]
struct MateLayout {
    name: String,
    mate_type: MateType,
    offset_on: bool,
    limits_on: bool,
    propagate: bool,
}

impl MateLayout {
    fn of(s: &MateSession) -> Self {
        Self { name: s.name.clone(), mate_type: s.mate_type, offset_on: s.offset_on, limits_on: s.limits_on, propagate: s.propagate }
    }
}

/// What a dialog widget edits.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Type,
    Connectors,
    Tabs,
    Widths,
    OffsetX,
    OffsetY,
    OffsetZ,
    RotateAbout,
    RotationAngle,
    Min(Dof),
    Max(Dof),
}

/// The toolbar's mate buttons.
pub fn toolbar_type(name: &str) -> Option<MateType> {
    Some(match name {
        "mate-fastened" => MateType::Fastened,
        "mate-revolute" => MateType::Revolute,
        "mate-slider" => MateType::Slider,
        "mate-cylindrical" => MateType::Cylindrical,
        "mate-pin-slot" => MateType::PinSlot,
        "mate-planar" => MateType::Planar,
        "mate-ball" => MateType::Ball,
        "mate-parallel" => MateType::Parallel,
        "mate-tangent" => MateType::Tangent,
        "mate-width" => MateType::Width,
        _ => return None,
    })
}

/// Opens a new mate dialog of type `t` on the active assembly (a mate button, M). With a dialog
/// already open, its type changes instead.
pub fn open_mate_dialog(world: &mut World, t: MateType) {
    if let Some(mut s) = world.get_resource_mut::<MateSession>() {
        set_type(&mut s, t);
        let s = s.clone();
        local_solve(world, s);
        return;
    }
    world.remove_resource::<super::relation_dialog::RelationSession>();
    world.remove_resource::<super::group_dialog::GroupSession>();
    super::animate::stop(world);
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
    let name = next_name(&model.mates, t.label());
    world.insert_resource(MateSession::new(element, MateId::new(), name, t));
    world.resource_mut::<crate::viewport::Selection>().0.clear();
}

/// Opens the dialog on an existing mate (Edit…, double-click).
pub fn edit_mate(world: &mut World, id: MateId) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(f) = doc.active_element().and_then(|e| e.assembly_model()?.mate(id).cloned()) else { return };
    let MateKind::Mate(m) = &f.kind else {
        if matches!(f.kind, MateKind::Replicate(_)) {
            super::replicate_dialog::edit(world, id);
        } else if matches!(f.kind, MateKind::Relation(_)) {
            super::relation_dialog::edit(world, id);
        } else if matches!(f.kind, MateKind::Variable(_)) {
            crate::variables_ui::edit_asm_variable(world, id);
        } else {
            super::group_dialog::edit_group(world, id);
        }
        return;
    };
    world.remove_resource::<super::group_dialog::GroupSession>();
    super::animate::stop(world);
    let lim = m.limits.unwrap_or_default();
    let mut s = MateSession::new(element, id, f.name.clone(), m.mate_type);
    s.editing = true;
    s.suppressed = f.suppressed;
    s.connectors = m.connectors.to_vec();
    s.tabs = m.tabs.clone();
    s.field = 1;
    s.offset_on = m.offset.is_some();
    s.offset = m.offset.unwrap_or_default();
    s.limits_on = m.limits.is_some();
    s.x_limits = lim.x.unwrap_or((0.0, 0.0));
    s.y_limits = lim.y.unwrap_or((0.0, 0.0));
    s.z_limits = lim.z.unwrap_or((0.0, 0.0));
    s.angle_limits = lim.angle.unwrap_or((0.0, 0.0));
    s.propagate = m.propagate;
    s.flip = m.flip;
    s.exprs = f.exprs.clone();
    s.simulation = m.simulation;
    world.insert_resource(s);
}

fn is_surface(c: &MateConnector) -> bool {
    matches!(c.anchor, ConnectorAnchor::Surface { .. })
}

/// Changes the type (A6.3: the picks stay when they fit the new type).
fn set_type(s: &mut MateSession, t: MateType) {
    if s.mate_type == t {
        return;
    }
    // A default name follows the type ("Fastened 1" → "Slider 1").
    let old = s.mate_type.label();
    if !s.editing || s.name.strip_prefix(old).is_some_and(|r| r.trim().parse::<u32>().is_ok()) {
        s.name = s.name.replacen(old, t.label(), 1);
    }
    let was = s.mate_type;
    s.mate_type = t;
    s.error = None;
    // Width keeps the picks as tabs; leaving Width, the tabs and widths become the pair.
    if t == MateType::Width && was != MateType::Width {
        s.tabs = std::mem::take(&mut s.connectors);
        s.tabs.truncate(2);
        s.field = if s.tabs.len() >= 2 { 1 } else { 0 };
    } else if was == MateType::Width && t != MateType::Width {
        let mut all: Vec<MateConnector> = std::mem::take(&mut s.tabs);
        all.append(&mut s.connectors);
        all.truncate(2);
        s.connectors = all;
    }
    // Tangent takes entities, the others connectors: picks of the other kind go.
    let want_surface = t == MateType::Tangent;
    s.connectors.retain(|c| is_surface(c) == want_surface);
    s.tabs.retain(|c| !is_surface(c));
    s.preview.clear();
}

/// The document's assembly and the source part solids of its instances.
pub fn model_and_solids(world: &mut World) -> Option<(Assembly, HashMap<InstanceId, std::sync::Arc<cadrs_core::Solid>>)> {
    world.resource_scope(|world, mut parts: Mut<super::AssemblyParts>| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let element_model = doc.active_element()?.assembly_model()?;
        // The parts at any depth (P3B.4): subassemblies rigid or flexible, as the solver sees
        // them; the solutions are placements of these occurrences.
        let model = cadrs_core::assembly::structure::solver_model(&doc.doc, element_model);
        // The source studios' rebuilds (cached in AssemblyParts).
        let solids = cadrs_core::assembly::occurrence_solids(&doc.doc, element_model, |e| parts.build(&doc.doc, e));
        Some((model, solids))
    })
}

/// Places the moving instance for this mate only (a pick, flip, reorient, type, offset or limit
/// change).
fn local_solve(world: &mut World, s: MateSession) {
    run_solve(world, s, true);
}

/// Solves (every mate unless `snap_only`) and shows the result.
fn run_solve(world: &mut World, mut s: MateSession, snap_only: bool) -> Option<Vec<(InstanceId, Pose)>> {
    let (doc_model, solids) = model_and_solids(world)?;
    let mut model = s.model(&doc_model);
    if s.mate().is_none() {
        s.preview.clear();
        world.insert_resource(s);
        return None;
    }
    let mover = s.mover(&doc_model);
    let pair = !matches!(s.mate_type, MateType::Tangent | MateType::Width);
    let opts = if snap_only && !pair {
        // Tangent and Width can't be snapped: solve this mate alone (and the groups). A Width's
        // tabs on two instances move alike (mirror-symmetric).
        let id = s.id;
        model.mates.retain(|f| f.id == id || f.mate().is_none());
        let ground = cadrs_core::assembly::solver::grounded(&doc_model);
        let movers: Vec<InstanceId> = if s.mate_type == MateType::Width {
            let mut v: Vec<InstanceId> = s.tabs.iter().map(|c| c.instance).filter(|i| !ground.contains(i)).collect();
            v.dedup();
            v
        } else {
            mover.into_iter().collect()
        };
        SolveOptions { movers, ..Default::default() }
    } else {
        SolveOptions {
            movers: mover.into_iter().collect(),
            snap: snap_only.then_some(s.id),
            drives: if snap_only { s.drives() } else { Vec::new() },
            snap_only,
            hold_free: false,
        }
    };
    let mut sol = cadrs_core::assembly::solve(&model, &solids, &opts);
    if !sol.converged {
        warn!("mate solve: residual {:e}", sol.residual);
    }
    // A Tangent's first placement: the moving entity is also drawn toward the picked face, so
    // the contact lands on the face that was picked, not somewhere on its unbounded surface.
    if snap_only && s.mate_type == MateType::Tangent
        && let (Some(mover), Some(m)) = (mover, s.mate())
        && let (Some(own), Some(other)) = (m.connectors.iter().find(|c| c.instance == mover), m.connectors.iter().find(|c| c.instance != mover))
    {
        let mut solved = model.clone();
        for (id, p) in &sol.poses {
            if let Some(i) = solved.instance_mut(*id) {
                i.pose = *p;
            }
        }
        let frame = |c: &MateConnector| c.local_frame(solids.get(&c.instance).map(|x| &**x));
        if let Some(op) = solved.instance(other.instance).map(|i| i.pose) {
            let target = frame(other).moved(&op).origin;
            let pull = cadrs_core::assembly::solver::Pull { view: None, instance: mover, point: frame(own).origin, target };
            sol = cadrs_core::assembly::drag(&solved, &solids, &[pull]);
        }
    }
    let changed = sol.changed(&doc_model);
    s.preview = changed.iter().copied().collect();
    world.insert_resource(s);
    Some(changed)
}

pub fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<MateSession>().cloned() else { return };
    let Some(feature) = s.feature() else { return };
    let element = s.element;
    let editing = s.editing;
    super::animate::stop(world);
    let Some(poses) = run_solve(world, s, false) else { return };
    let ok = if editing {
        super::run(world, &SetMateFeature { element, feature, poses })
    } else {
        super::run(world, &AddMateFeature { element, feature, poses })
    };
    if ok {
        cancel(world);
    }
}

pub fn cancel(world: &mut World) {
    super::animate::stop(world);
    world.remove_resource::<MateSession>();
    let mut parts = world.resource_mut::<super::AssemblyParts>();
    parts.preview.clear();
    // P3F.5 judge: the mate row opened (and its instances) doesn't stay selected.
    world.resource_mut::<crate::viewport::Selection>().0.clear();
    world.resource_mut::<super::mate_display::MateSelection>().0 = None;
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<MateDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<MateDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(cancel);
    }
}

/// Changes the session, then places the moving instance again.
fn change(commands: &mut Commands, f: impl FnOnce(&mut MateSession) + Send + 'static) {
    commands.queue(move |world: &mut World| {
        let Some(mut s) = world.get_resource::<MateSession>().cloned() else { return };
        f(&mut s);
        local_solve(world, s);
    });
}

fn on_select(ev: On<SelectChange>, q: Query<&Role>, mut commands: Commands) {
    let index = ev.index;
    match q.get(ev.entity) {
        Ok(Role::Type) => {
            if let Some(t) = MateType::ALL.get(index).copied() {
                change(&mut commands, move |s| set_type(s, t));
            }
        }
        Ok(Role::RotateAbout) => change(&mut commands, move |s| s.offset.axis = index.min(2) as u8),
        _ => {}
    }
}

fn on_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else { return };
    let on = ev.checked;
    match name.as_str() {
        "mate-offset-checkbox" => change(&mut commands, move |s| s.offset_on = on),
        "mate-limits-checkbox" => change(&mut commands, move |s| s.limits_on = on),
        "mate-propagation-checkbox" => change(&mut commands, move |s| s.propagate = on),
        "mate-simulation-checkbox" => change(&mut commands, move |s| s.simulation = on),
        _ => {}
    }
}

fn on_number(
    ev: On<NumberFieldCommit>,
    q: Query<&Role>,
    mut q_state: Query<&mut NumberFieldState>,
    units: Res<crate::WorkspaceUnits>,
    vars: Res<crate::variables_ui::ActiveVariables>,
    mut commands: Commands,
) {
    let Ok(role) = q.get(ev.entity).copied() else { return };
    let text = ev.text.trim().to_string();
    let angle = matches!(role, Role::RotationAngle | Role::Min(Dof::Angle) | Role::Max(Dof::Angle));
    let quantity = if angle { Quantity::Angle } else { Quantity::Length };
    // P3F.4: an offset may name the assembly's variables (`#gap / 2`).
    use cadrs_core::assembly::mate::OffsetSlot;
    let slot = match role {
        Role::OffsetX => Some(OffsetSlot::X),
        Role::OffsetY => Some(OffsetSlot::Y),
        Role::OffsetZ => Some(OffsetSlot::Z),
        Role::RotationAngle => Some(OffsetSlot::Angle),
        _ => None,
    };
    if let Some(slot) = slot {
        let expr = text.contains('#').then(|| text.clone());
        commands.queue(move |world: &mut World| {
            if let Some(mut s) = world.get_resource_mut::<MateSession>() {
                s.exprs.retain(|(k, _)| *k != slot);
                if let Some(e) = expr {
                    s.exprs.push((slot, e));
                }
            }
        });
    }
    let v = match vars.eval(&units.0, &text, quantity) {
        Ok(v) if v.is_finite() => v,
        _ => {
            if let Ok(mut s) = q_state.get_mut(ev.entity) {
                s.text = text;
                s.error = true;
            }
            return;
        }
    };
    if let Ok(mut s) = q_state.get_mut(ev.entity) {
        s.error = false;
    }
    let value = if angle { v.to_radians() } else { v };
    let enter = ev.enter;
    change(&mut commands, move |s| match role {
        Role::OffsetX => s.offset.translation[0] = value,
        Role::OffsetY => s.offset.translation[1] = value,
        Role::OffsetZ => s.offset.translation[2] = value,
        Role::RotationAngle => s.offset.angle = value,
        Role::Min(d) | Role::Max(d) => {
            let max = matches!(role, Role::Max(_));
            let slot = match d {
                Dof::X => &mut s.x_limits,
                Dof::Y => &mut s.y_limits,
                Dof::Z => &mut s.z_limits,
                Dof::Angle => &mut s.angle_limits,
            };
            if max {
                slot.1 = value;
            } else {
                slot.0 = value;
            }
        }
        _ => {}
    });
    if enter {
        commands.queue(|world: &mut World| world.resource_mut::<InputFocus>().clear());
    }
}

fn on_button(a: On<bevy::ui_widgets::Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(a.entity) else { return };
    // The toolbar's mate buttons and Group.
    if let Some(t) = toolbar_type(name.as_str()) {
        commands.queue(move |world: &mut World| open_mate_dialog(world, t));
        return;
    }
    // P3B.9: the relation buttons, and the Relations button's menu of them.
    if let Some(t) = super::relation_dialog::toolbar_type(name.as_str()) {
        commands.queue(move |world: &mut World| super::relation_dialog::open(world, t));
        return;
    }
    match name.as_str() {
        "relations" => {
            let e = a.entity;
            commands.queue(move |world: &mut World| super::relation_dialog::open_relations_menu(world, e));
        }
        "group" => commands.queue(super::group_dialog::open_group_dialog),
        // P3B.8: Replicate, and the Named positions and Exploded views panels.
        "replicate" => commands.queue(super::replicate_dialog::open),
        "named-positions" | "explode" => {
            let panel = if name.as_str() == "explode" { crate::appearance::SidePanel::ExplodedViews } else { crate::appearance::SidePanel::NamedPositions };
            commands.queue(move |world: &mut World| {
                let mut open = world.resource_mut::<crate::appearance::SidePanel>();
                *open = if *open == panel { crate::appearance::SidePanel::None } else { panel };
            });
        }
        "mate-flip" | "mate-reorient" => {
            let flip = name.as_str() == "mate-flip";
            commands.queue(move |world: &mut World| {
                let Some(mut s) = world.get_resource::<MateSession>().cloned() else { return };
                if s.mate_type == MateType::Tangent {
                    // A11.3: the other side.
                    if flip {
                        s.flip = !s.flip;
                    }
                } else {
                    let Some(doc) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model().cloned()) else { return };
                    let k = s.adjusted(&doc);
                    let list = if s.mate_type == MateType::Width { &mut s.tabs } else { &mut s.connectors };
                    if let Some(c) = list.get_mut(if s.mate_type == MateType::Width { 0 } else { k }) {
                        if flip {
                            c.flip = !c.flip;
                        } else {
                            c.reorient = (c.reorient + 1) % 4;
                        }
                    }
                }
                local_solve(world, s);
            });
        }
        "mate-solve" => {
            commands.queue(|world: &mut World| {
                if let Some(s) = world.get_resource::<MateSession>().cloned() {
                    run_solve(world, s, false);
                }
            });
        }
        "mate-animate" => commands.queue(super::animate::preview_dialog_mate),
        _ => {}
    }
}

fn on_list_remove(ev: On<SelectionListRemove>, q: Query<&Role>, mut commands: Commands) {
    let Ok(role) = q.get(ev.entity).copied() else { return };
    if !matches!(role, Role::Connectors | Role::Tabs | Role::Widths) {
        return;
    }
    let i = ev.index;
    commands.queue(move |world: &mut World| {
        if let Some(mut s) = world.get_resource_mut::<MateSession>() {
            let list = if role == Role::Tabs { &mut s.tabs } else { &mut s.connectors };
            if i < list.len() {
                list.remove(i);
            }
            if role == Role::Tabs {
                s.field = 0;
            }
            s.error = None;
            s.preview.clear();
        }
    });
}

fn on_list_move(ev: On<SelectionListMove>, q: Query<&Role>, mut commands: Commands) {
    let Ok(role) = q.get(ev.entity).copied() else { return };
    if !matches!(role, Role::Connectors | Role::Tabs | Role::Widths) {
        return;
    }
    let (from, to) = (ev.from, ev.to);
    change(&mut commands, move |s| {
        let list = if role == Role::Tabs { &mut s.tabs } else { &mut s.connectors };
        if from < list.len() && to < list.len() {
            let c = list.remove(from);
            list.insert(to, c);
        }
    });
}

/// Clicking a Width field makes it take the picks.
fn on_list_activate(ev: On<SelectionListActivate>, q: Query<&Role>, session: Option<ResMut<MateSession>>) {
    let (Ok(role), Some(mut s)) = (q.get(ev.entity), session) else { return };
    let field = match role {
        Role::Tabs => 0,
        Role::Widths => 1,
        _ => return,
    };
    if s.field != field {
        s.field = field;
    }
}

// ---------------------------------------------------------------------------------------------
// Picking

/// The Tangent side the two picked entities are nearer to now: flipped (the other side of a
/// plane, inside a hole) when that is nearer to tangent.
fn tangent_side(world: &World, s: &MateSession) -> bool {
    let Some(model) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model()) else { return false };
    let world_frame = |c: &MateConnector| {
        let pose = s.preview.get(&c.instance).copied().or_else(|| model.instance(c.instance).map(|i| i.pose))?;
        Some((c.surface_kind()?, c.frame.moved(&pose)))
    };
    let (Some((ka, fa)), Some((kb, fb))) = (world_frame(&s.connectors[0]), world_frame(&s.connectors[1])) else { return false };
    let err = |flip| cadrs_core::assembly::solver::tangent_error(ka, &fa, kb, &fb, flip);
    err(true) < err(false)
}

/// How near (px) the pointer must stay to the points of the entity it left for them to stay
/// pickable.
const STICKY_PX: f32 = 28.0;

/// The entity whose points the pointer works with: the Shift-locked one, the one under the
/// pointer, or the one it just left while it is near one of that entity's points.
fn working_entity(
    s: &MateSession,
    hovered: Option<(InstanceId, EntityRef)>,
    doc: &ActiveDocument,
    parts: &mut super::AssemblyParts,
    view: &crate::camera::ViewState,
    at: Vec2,
) -> Option<(InstanceId, EntityRef)> {
    if let Some(e) = s.locked.or(hovered) {
        return Some(e);
    }
    let (i, e) = s.sticky?;
    let pts = connectors::entity_points(doc, parts, i, e)?;
    let best = pts.nearest(view, at)?;
    (view.project(v3(pts.world(&best).origin)).distance(at) <= STICKY_PX).then_some((i, e))
}

/// What a click takes: a connector at the implicit point nearest the pointer, or (Tangent) the
/// entity as a surface.
fn pick_connector(
    s: &MateSession,
    doc: &ActiveDocument,
    parts: &mut super::AssemblyParts,
    instance: InstanceId,
    entity: EntityRef,
    view: &crate::camera::ViewState,
    at: Vec2,
) -> Option<MateConnector> {
    if s.mate_type == MateType::Tangent {
        let (_, element, part) = super::occurrence_source(doc, parts, instance)?;
        let build = parts.build(&doc.doc, element)?;
        let solid = build.part(part)?.solid.clone();
        let (frame, kind) = surface_of(&solid, &entity)?;
        if let Some(other) = s.connectors.first().and_then(|c| c.surface_kind())
            && !tangent_supported(other, kind)
        {
            return None;
        }
        return Some(MateConnector::surface(instance, entity, frame, kind));
    }
    let pts = connectors::entity_points(doc, parts, instance, entity)?;
    let best = pts.nearest(view, at)?;
    Some(MateConnector::implicit(instance, &best))
}

/// A click on an instance while the dialog waits for a connector: the implicit point of the
/// clicked (or Shift-locked) entity nearest the pointer (A6.4–A6.7).
#[allow(clippy::too_many_arguments)]
fn mate_picks(
    mut picks: MessageReader<PickRequest>,
    session: Option<Res<MateSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut parts: ResMut<super::AssemblyParts>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    vdrag: Res<ViewportDrag>,
    cache: Res<PartCache>,
    shown: Res<crate::pattern::MateConnectorsShown>,
    mut commands: Commands,
) {
    let (Some(s), Some(doc)) = (session, doc) else {
        picks.clear();
        return;
    };
    for p in picks.read() {
        if !s.wants_pick() {
            continue;
        }
        let at = rect.offset(vdrag.pointer());
        // P3B.7: an explicit connector near the pointer (A22.7), else the Origin (A16.3), else
        // the implicit point of the entity under the pointer.
        // (Not on the first connector's instance: two connectors can sit on one spot, as a
        // hinge's two do.)
        let first = (s.mate_type != MateType::Width).then(|| s.connectors.first().map(|c| c.instance)).flatten();
        let explicit = (shown.0 && s.mate_type.uses_connectors() && s.locked.is_none())
            .then(|| {
                let mut list = super::connector_tool::explicit_connectors(&doc, &mut parts, &cache);
                list.retain(|(c, _)| Some(c.instance) != first);
                super::connector_tool::nearest_explicit(&list, &view.view, at)
            })
            .flatten()
            .map(|(c, _)| c);
        let origin = (s.mate_type.uses_connectors() && p.0 == Some(crate::viewport::Pick::Origin)).then(MateConnector::origin);
        let conn = match explicit.or(origin) {
            Some(c) => {
                if s.mate_type != MateType::Width && s.connectors.first().is_some_and(|x| x.instance == c.instance) {
                    continue;
                }
                c
            }
            None => {
                let hovered = p.0.as_ref().and_then(entity_of);
                let entity = if s.mate_type == MateType::Tangent { s.locked.or(hovered) } else { working_entity(&s, hovered, &doc, &mut parts, &view.view, at) };
                let Some((instance, entity)) = entity else { continue };
                if s.mate_type != MateType::Width && s.connectors.first().is_some_and(|c| c.instance == instance) {
                    continue;
                }
                let Some(conn) = pick_connector(&s, &doc, &mut parts, instance, entity, &view.view, rect.offset(vdrag.pointer())) else { continue };
                conn
            }
        };
        commands.queue(move |world: &mut World| {
            let Some(mut s) = world.get_resource::<MateSession>().cloned() else { return };
            if !s.wants_pick() {
                return;
            }
            s.locked = None;
            if s.mate_type == MateType::Width {
                // A12.2: no instance in both fields.
                let other = if s.field == 0 { &s.connectors } else { &s.tabs };
                if other.iter().any(|c| c.instance == conn.instance) {
                    s.error = Some(s.field);
                    world.insert_resource(s);
                    return;
                }
                s.error = None;
                if s.field == 0 {
                    s.tabs.push(conn);
                    if s.tabs.len() >= 2 {
                        s.field = 1;
                    }
                } else {
                    s.connectors.push(conn);
                }
            } else {
                s.connectors.push(conn);
                // Tangent: the side the entities are on now (A11.3's Flip turns it over).
                if s.mate_type == MateType::Tangent && s.connectors.len() == 2 {
                    s.flip = tangent_side(world, &s);
                }
            }
            local_solve(world, s);
        });
    }
}

/// Shift locks the entity under the pointer (A6.5); releasing it unlocks. While locked, the
/// hover highlight stays on the locked entity ([`crate::viewport::HoverOverride`]).
fn mate_lock(
    keys: Res<ButtonInput<KeyCode>>,
    highlight: Res<PlaneHighlight>,
    session: Option<ResMut<MateSession>>,
    mut hover: ResMut<crate::viewport::HoverOverride>,
) {
    let Some(mut s) = session else {
        if hover.0.is_some() {
            hover.0 = None;
        }
        return;
    };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if let Some(e) = highlight.viewport.as_ref().and_then(entity_of)
        && s.sticky != Some(e)
    {
        s.sticky = Some(e);
    }
    if !shift {
        if s.locked.is_some() {
            s.locked = None;
        }
    } else if s.locked.is_none()
        && let Some(e) = highlight.viewport.as_ref().and_then(entity_of)
    {
        s.locked = Some(e);
    }
    let want = s.locked.map(|(i, e)| match e {
        EntityRef::Face(f) => crate::viewport::Pick::Face(super::occurrence_part(i), f),
        EntityRef::Edge(e) => crate::viewport::Pick::Edge(super::occurrence_part(i), e),
        EntityRef::Vertex(v) => crate::viewport::Pick::Vertex(super::occurrence_part(i), v),
    });
    if hover.0 != want {
        hover.0 = want;
    }
}

/// The session's placements are shown in the view (unless an animation is playing).
fn sync_preview(session: Option<Res<MateSession>>, playing: Option<Res<super::animate::Playback>>, mut parts: ResMut<super::AssemblyParts>) {
    let Some(s) = session else { return };
    if playing.is_some() {
        return;
    }
    if s.is_changed() && parts.preview != s.preview {
        parts.preview = s.preview.clone();
    }
}

/// The dots of the entity under the pointer, the connector the next click takes, and the picked
/// connectors (their entities in the selection amber, and their glyphs).
#[allow(clippy::too_many_arguments)]
fn draw_mate(
    session: Option<Res<MateSession>>,
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
    // The picked connectors (and entities).
    for c in s.all() {
        let Some(part) = cache.part(super::occurrence_part(c.instance)) else { continue };
        let owner = match c.anchor {
            ConnectorAnchor::Implicit { owner, .. } => Some(owner),
            ConnectorAnchor::Surface { entity, .. } => Some(entity),
            _ => None,
        };
        match owner {
            Some(EntityRef::Edge(e)) => {
                if let Some(e) = part.solid.edge(&e) {
                    edges.linestrip(e.points.iter().map(|p| v3(*p)), crate::parts::SELECTED);
                }
            }
            Some(EntityRef::Face(f)) => {
                if let Some(f) = part.solid.face(&f) {
                    for l in &f.loops {
                        let mut pts: Vec<Vec3> = l.iter().map(|p| v3(*p)).collect();
                        pts.extend(pts.first().copied());
                        edges.linestrip(pts, crate::parts::SELECTED);
                    }
                }
            }
            _ => {}
        }
        if is_surface(c) {
            continue;
        }
        let pose = s.preview.get(&c.instance).copied().or_else(|| {
            doc.active_element()?.assembly_model()?.instance(c.instance).map(|i| i.pose)
        });
        if let Some(pose) = pose {
            let f = c.adjust(c.base_frame(None)).moved(&pose);
            connectors::draw_glyph(&mut line, &mut halo, &v, &f, crate::parts::SELECTED);
        }
    }
    // The Origin (A16.3) as a picked connector.
    for c in s.all().filter(|c| c.is_origin()) {
        connectors::draw_glyph(&mut line, &mut halo, &v, &c.local_frame(None), crate::parts::SELECTED);
    }
    if !s.wants_pick() || s.mate_type == MateType::Tangent {
        return;
    }
    // The entity under the pointer (or locked, or just left).
    let at = rect.offset(vdrag.pointer());
    let Some((instance, entity)) = working_entity(&s, highlight.viewport.as_ref().and_then(entity_of), &doc, &mut parts, &v, at) else { return };
    if s.mate_type != MateType::Width && s.connectors.first().is_some_and(|c| c.instance == instance) {
        return;
    }
    let Some(pts) = connectors::entity_points(&doc, &mut parts, instance, entity) else { return };
    let dots: Vec<Vec3> = pts.points.iter().map(|p| v3(pts.world(p).origin)).collect();
    connectors::draw_points(&mut halo, &mut line, &v, &dots);
    if let Some(best) = pts.nearest(&v, rect.offset(vdrag.pointer())) {
        let ring = if s.locked.is_some() { Color::srgb_u8(0x22, 0x55, 0xe0) } else { crate::parts::HOVER };
        connectors::draw_glyph(&mut line, &mut halo, &v, &pts.world(&best), ring);
    }
}

// ---------------------------------------------------------------------------------------------
// The dialog

fn connector_label(doc: &ActiveDocument, cache: &PartCache, c: &MateConnector) -> String {
    // Onshape names the part without the instance number; an explicit connector by its name
    // (P3B.7), the Origin as "Origin".
    super::connector_tool::connector_label(doc, cache, c)
}

struct Texts {
    offset: [String; 4],
    limits: Vec<(Dof, String, String)>,
}

fn texts(s: &MateSession, units: &cadrs_sketch::units::Units) -> Texts {
    let len = |v: f64| units.with_unit(v, Quantity::Length);
    let deg = |v: f64| units.with_unit(v.to_degrees(), Quantity::Angle);
    let limits = [Dof::X, Dof::Y, Dof::Z, Dof::Angle]
        .into_iter()
        .map(|d| {
            let (lo, hi) = s.limits(d);
            if d.is_angle() { (d, deg(lo), deg(hi)) } else { (d, len(lo), len(hi)) }
        })
        .collect();
    // P3F.4: an offset typed as an expression shows it.
    use cadrs_core::assembly::mate::OffsetSlot;
    let or_expr = |slot: OffsetSlot, v: String| s.exprs.iter().find(|(k, _)| *k == slot).map_or(v, |(_, e)| e.clone());
    Texts {
        offset: [
            or_expr(OffsetSlot::X, len(s.offset.translation[0])),
            or_expr(OffsetSlot::Y, len(s.offset.translation[1])),
            or_expr(OffsetSlot::Z, len(s.offset.translation[2])),
            or_expr(OffsetSlot::Angle, deg(s.offset.angle)),
        ],
        limits,
    }
}

fn field(b: &mut ChildSpawner, t: &Theme, role: Role, name: &str, label: &str, icon: Option<(&'static str, Color)>, text: &str) {
    let mut f = NumberField::new(name.to_string(), label.to_string()).text(text.to_string()).label_width(if label.len() > 2 { 84.0 } else { 28.0 });
    if let Some((i, c)) = icon {
        f = f.label_icon(i, c);
    }
    b.spawn((role, f.build(t)));
}

const RED: Color = Color::srgb(0.85, 0.18, 0.16);
const GREEN: Color = Color::srgb(0.16, 0.62, 0.25);
const BLUE: Color = Color::srgb(0.16, 0.36, 0.86);

/// The limit fields of a DOF: (name part, label, min icon, max icon, colour).
fn limit_look(d: Dof) -> (&'static str, &'static str, &'static str, &'static str, Color) {
    match d {
        Dof::X => ("x", "X", "chevron-left", "chevron-right", RED),
        Dof::Y => ("y", "Y", "chevron-down", "chevron-up", GREEN),
        Dof::Z => ("z", "Z", "arrow-down", "arrow-up", BLUE),
        Dof::Angle => ("angle", "Z", "undo", "redo", BLUE),
    }
}

struct Lists {
    connectors: Vec<String>,
    tabs: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn connector_list(t: &Theme, role: Role, name: &'static str, placeholder: &str, items: Vec<String>, active: bool, error: bool, reorder: bool) -> impl Bundle {
    let mut l = SelectionList::new(name)
        .placeholder(placeholder)
        .item_icon("mate-connector", "Mate connector")
        // Cut at the end, so the owner stays readable ("Mate connector 1 of Base Frame…"); the
        // whole name in the item's tooltip (P3B.7 judge).
        .tint_filled()
        .items(items)
        .active(active)
        .error(error);
    if reorder {
        l = l.reorderable(true);
    }
    (role, l.build(t))
}

fn mate_dialog(theme: &Theme, s: &MateSession, lists: Lists, tx: Texts) -> impl Bundle {
    let tb = theme.clone();
    let tf = theme.clone();
    let layout = MateLayout::of(s);
    let (mate_type, offset_on, limits_on, axis, propagate) = (s.mate_type, s.offset_on, s.limits_on, s.offset.axis, s.propagate);
    let simulation = s.simulation;
    let (active_field, error) = (s.field, s.error);
    let valid = s.mate().is_some();
    let can_animate = valid && !mate_type.dof().is_empty();
    (
        MateDialog,
        layout,
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("mate-dialog")
            .title(s.name.clone())
            .valid(valid)
            .width(262.0)
            .body(move |b| {
                let t = &tb;
                let mut select = Select::new("mate-type");
                for mt in MateType::ALL {
                    select = select.option(mt.label(), true);
                }
                let i = MateType::ALL.iter().position(|x| *x == mate_type).unwrap_or(0);
                b.spawn(Node { height: Val::Px(28.0), align_items: AlignItems::Center, ..default() })
                    .with_child((Role::Type, select.selected(i).build(t)));
                let margin = |mut n: Mut<Node>| {
                    n.flex_grow = 0.0;
                    n.margin = UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(3.0), Val::Px(4.0));
                };
                match mate_type {
                    MateType::Width => {
                        b.spawn(connector_list(t, Role::Tabs, "mate-tabs", "Tab mate connectors", lists.tabs, active_field == 0, error == Some(0), false))
                            .entry::<Node>()
                            .and_modify(margin);
                        b.spawn(connector_list(t, Role::Widths, "mate-widths", "Width mate connectors", lists.connectors, active_field == 1, error == Some(1), false))
                            .entry::<Node>()
                            .and_modify(margin);
                    }
                    MateType::Tangent => {
                        let active = lists.connectors.len() < 2;
                        b.spawn((
                            Role::Connectors,
                            SelectionList::new("mate-entities").placeholder("Entities").tint_filled().items(lists.connectors).active(active).build(t),
                        ))
                        .entry::<Node>()
                        .and_modify(margin);
                        b.spawn(OptionRow::new("mate-propagation", "Tangent propagation").checked(propagate).build(t));
                    }
                    _ => {
                        let active = lists.connectors.len() < 2;
                        b.spawn(connector_list(t, Role::Connectors, "mate-connectors", "Mate connectors", lists.connectors, active, false, true))
                            .entry::<Node>()
                            .and_modify(margin);
                    }
                }
                if mate_type.has_offset() {
                    b.spawn(OptionRow::new("mate-offset", "Offset").checked(offset_on).build(t));
                    if offset_on {
                        field(b, t, Role::OffsetX, "mate-offset-x", "X", Some(("arrow-down-right", RED)), &tx.offset[0]);
                        field(b, t, Role::OffsetY, "mate-offset-y", "Y", Some(("arrow-up-right", GREEN)), &tx.offset[1]);
                        field(b, t, Role::OffsetZ, "mate-offset-z", "Z", Some(("arrow-up", BLUE)), &tx.offset[2]);
                        b.spawn(Node { height: Val::Px(28.0), align_items: AlignItems::Center, margin: UiRect::left(Val::Px(18.0)), ..default() }).with_child((
                            Role::RotateAbout,
                            Select::new("mate-rotate-about")
                                .option("Rotate about X", true)
                                .option("Rotate about Y", true)
                                .option("Rotate about Z", true)
                                .selected(axis.min(2) as usize)
                                .build(t),
                        ));
                        field(b, t, Role::RotationAngle, "mate-rotation-angle", "Rotation angle", None, &tx.offset[3]);
                    }
                }
                if mate_type.has_limits() {
                    b.spawn(OptionRow::new("mate-limits", "Limits").checked(limits_on).build(t));
                    if limits_on {
                        for d in mate_type.limit_dofs() {
                            let (key, label, lo_icon, hi_icon, color) = limit_look(*d);
                            let (_, lo, hi) = tx.limits.iter().find(|(x, ..)| x == d).cloned().unwrap_or((*d, String::new(), String::new()));
                            field(b, t, Role::Min(*d), &format!("mate-limit-{key}-min"), label, Some((lo_icon, color)), &lo);
                            field(b, t, Role::Max(*d), &format!("mate-limit-{key}-max"), label, Some((hi_icon, color)), &hi);
                        }
                    }
                }
                // P3F.5: bonds the two parts in a simulation (the Simulation panel).
                b.spawn(OptionRow::new("mate-simulation", "Simulation connection").checked(simulation).build(t));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn(Node { flex_grow: 1.0, align_items: AlignItems::Center, column_gap: Val::Px(3.0), ..default() }).with_children(|r| {
                    r.spawn(IconButton::new("mate-flip", "flip-direction-up").icon_size(16.0).tooltip("Flip primary axis").build(t));
                    r.spawn(
                        IconButton::new("mate-reorient", "revolve")
                            .icon_size(16.0)
                            .tooltip("Reorient secondary axis")
                            .disabled(mate_type == MateType::Tangent)
                            .build(t),
                    );
                    r.spawn(
                        IconButton::new("mate-animate", "play")
                            .icon_size(16.0)
                            .tooltip("Animate mate DOF")
                            .disabled(!can_animate)
                            .build(t),
                    )
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.width = Val::Px(26.0);
                        n.height = Val::Px(26.0);
                    });
                    r.spawn(cadrs_ui::Button::new("mate-solve").label("Solve").outline().small().tooltip("Solve all mates").build(t));
                    r.spawn(Node { flex_grow: 1.0, ..default() });
                    r.spawn((Name::new("mate-help"), icon("help-filled", 14.0, Color::srgb_u8(0xa8, 0xa8, 0xa8)), Tooltip::new("Help")));
                });
            })
            .build(theme),
    )
}

/// The dialog's rebuild key: the layout, plus whether it can be accepted and animated.
#[derive(Component, Debug, Clone, PartialEq)]
struct Footer(bool);

/// Spawns, updates and removes the dialog.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_mate_dialog(
    session: Option<Res<MateSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    units: Res<crate::WorkspaceUnits>,
    theme: Res<Theme>,
    q_dialog: Query<(Entity, &MateLayout, &Footer, &mut FeatureDialogState), With<MateDialog>>,
    mut q_list: Query<(&Role, &mut SelectionListState)>,
    mut q_select: Query<(&Role, &mut SelectState)>,
    mut q_num: Query<(&Role, &mut NumberFieldState)>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        for (e, ..) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let Some(doc) = doc else { return };
    let connectors: Vec<String> = s.connectors.iter().map(|c| connector_label(&doc, &cache, c)).collect();
    let tabs: Vec<String> = s.tabs.iter().map(|c| connector_label(&doc, &cache, c)).collect();
    let layout = MateLayout::of(&s);
    let valid = s.mate().is_some();
    let footer = Footer(valid);
    let tx = texts(&s, &units.0);
    let mut dialogs = q_dialog;
    let current = dialogs.iter_mut().next();
    match current {
        Some((_, l, f, mut st)) if *l == layout && *f == footer => {
            if st.valid != valid || st.title != s.name {
                st.valid = valid;
                st.title = s.name.clone();
            }
        }
        other => {
            if let Some((e, ..)) = other {
                commands.entity(e).try_despawn();
            }
            let Some(area) = q_area.iter().next() else { return };
            let d = commands.spawn((mate_dialog(&theme, &s, Lists { connectors, tabs }, tx), footer)).id();
            commands.entity(area).add_child(d);
            return;
        }
    }
    for (role, mut st) in &mut q_list {
        let (items, active, error) = match role {
            Role::Connectors => (&connectors, connectors.len() < 2, false),
            Role::Tabs => (&tabs, s.field == 0, s.error == Some(0)),
            Role::Widths => (&connectors, s.field == 1, s.error == Some(1)),
            _ => continue,
        };
        if st.items != *items || st.active != active || st.error != error {
            st.items = items.clone();
            st.active = active;
            st.error = error;
        }
    }
    for (role, mut st) in &mut q_select {
        let want = match role {
            Role::Type => MateType::ALL.iter().position(|x| *x == s.mate_type).unwrap_or(0),
            Role::RotateAbout => s.offset.axis.min(2) as usize,
            _ => continue,
        };
        if st.selected != want {
            st.selected = want;
        }
    }
    for (role, mut st) in &mut q_num {
        let want = match role {
            Role::OffsetX => tx.offset[0].clone(),
            Role::OffsetY => tx.offset[1].clone(),
            Role::OffsetZ => tx.offset[2].clone(),
            Role::RotationAngle => tx.offset[3].clone(),
            Role::Min(d) => tx.limits.iter().find(|(x, ..)| x == d).map(|l| l.1.clone()).unwrap_or_default(),
            Role::Max(d) => tx.limits.iter().find(|(x, ..)| x == d).map(|l| l.2.clone()).unwrap_or_default(),
            _ => continue,
        };
        if !st.error && st.text != want {
            st.text = want;
        }
    }
}
