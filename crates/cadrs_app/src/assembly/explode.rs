//! The **Exploded views** panel (P3B.8, `intro-to-assemblies.md` A1.8, X16): the right strip's
//! Exploded views button docks it beside the viewport of an assembly
//! ([`cadrs_core::assembly::explode`]).
//!
//! - **Add exploded view** makes "Exploded view n" and opens it for editing. Each view is a row;
//!   clicking it shows the assembly exploded (a view state: the mates and the document's
//!   placements don't change); its menu has **Edit**, **Rename** and **Delete**.
//! - The **slider** (0–100 %) plays the steps between assembled and exploded, one after another;
//!   **Trail lines** draws each moved instance's path.
//! - **Editing** a view: the step list (each step's instances and motion, with ↑ ↓ to reorder and
//!   ✕ to delete; clicking one loads it into the editor to change it), and the step editor: the
//!   **Instances** field follows the selection; **Move** or **Rotate** along / about **X**, **Y**
//!   or **Z** by the **Distance** / **Angle** typed, or drag an arrow of the triad drawn at the
//!   instances: the drag sets the distance and adds the step when released. **Add step** (or
//!   **Update step**) and **Done**. Every change is one undo step
//!   ([`cadrs_core::assembly::explode::SetExplodedView`]).
//!
//! Names: `exploded-views-panel`, `exploded-view-add`, rows `exploded-view-row-<k>` (menu items
//! `exploded-view-edit`, `…-rename`, `…-delete`), `exploded-view-slider`,
//! `exploded-view-percent`, `exploded-view-trails`; the editor's `exploded-step-<k>` (with
//! `…-up`, `…-down`, `…-delete`), `explode-step-instances`, `explode-kind-move`,
//! `explode-kind-rotate`, `explode-axis-x|y|z`, `explode-amount`, `explode-step-add`,
//! `exploded-view-done`; `exploded-view-export-image` (P3F.6: Export image… of the view shown).

use std::collections::HashMap;

use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::ElementId;
use cadrs_core::assembly::explode::{self, DeleteExplodedView, ExplodeMotion, ExplodeStep, ExplodedView, ExplodedViewId, SetExplodedView};
use cadrs_core::assembly::{InstanceId, Pose};
use cadrs_sketch::units::Quantity;
use cadrs_ui::dialog_fields::{NumberField, NumberFieldCommit, Slider, SliderChange};
use cadrs_ui::menu::{ContextMenuAnchor, Menu, MenuAction, MenuItem};
use cadrs_ui::name_popup::{NamePopup, NamePopupCommit};
use cadrs_ui::prelude::*;
use cadrs_ui::{CheckboxChange, SelectionList, open_context_menu};

use crate::appearance::{SidePanel, side_panel_header, side_panel_node};
use crate::parts::PartCache;
use crate::viewport::{ActiveKind, Selection, ViewportArea, ViewportRect, ViewportView, pointer_over_viewport};
use crate::{ActiveDocument, AppState};

pub struct ExplodePlugin;

impl Plugin for ExplodePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ExplodeUi>()
            .init_gizmo_group::<TrailGizmos>()
            .add_systems(Startup, |mut store: ResMut<GizmoConfigStore>| {
                let (c, _) = store.config_mut::<TrailGizmos>();
                c.line.width = 2.0;
                c.depth_bias = -1.0;
                c.render_layers = bevy::camera::visibility::RenderLayers::layer(crate::viewport::OVERLAY_LAYER);
            })
            .add_systems(
                Update,
                (follow_selection, triad_drag, apply_explode, sync_panel, compute_draw, draw)
                    .chain()
                    .after(crate::viewport::apply_view_to_camera)
                    .before(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut ui: ResMut<ExplodeUi>| *ui = ExplodeUi::default())
            .add_observer(on_button)
            .add_observer(on_slider)
            .add_observer(on_trails)
            .add_observer(on_amount)
            .add_observer(on_popup)
            .add_observer(on_row_menu)
            .add_observer(on_menu_action);
    }
}

/// Trail lines and the explode triad: over the model.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct TrailGizmos;

/// The step being made or changed.
#[derive(Debug, Clone, PartialEq)]
pub struct StepEdit {
    /// The step changed (its index), or a new one.
    pub index: Option<usize>,
    pub instances: Vec<InstanceId>,
    pub rotate: bool,
    /// 0 X, 1 Y, 2 Z.
    pub axis: usize,
    /// mm, or radians.
    pub amount: f64,
}

impl Default for StepEdit {
    fn default() -> Self {
        Self { index: None, instances: Vec::new(), rotate: false, axis: 0, amount: 0.0 }
    }
}

/// The panel's view state: the view shown, the slider, the view being edited.
#[derive(Resource, Debug, Clone, Default)]
pub struct ExplodeUi {
    /// The exploded view shown (and its assembly).
    pub active: Option<(ElementId, ExplodedViewId)>,
    /// The slider, 0 (assembled) to 1 (exploded).
    pub fraction: f64,
    /// The view whose steps are being edited, and the step editor.
    pub editing: Option<StepEdit>,
    /// A triad arrow drag: its axis, where it started along the axis (mm) and the amount then.
    drag: Option<(usize, f64, f64)>,
}

impl ExplodeUi {
    pub fn editing(&self) -> bool {
        self.editing.is_some()
    }
}

#[derive(Component)]
struct ExplodePanel;

#[derive(Component, Debug, Clone, PartialEq)]
struct PanelKey(String);

#[derive(Component, Debug, Clone, Copy)]
struct ViewRow(ExplodedViewId);

#[derive(Component, Debug, Clone, Copy)]
struct ViewMenu(ExplodedViewId);

#[derive(Component, Debug, Clone, Copy)]
struct RenamePopup(ExplodedViewId);

fn element(world: &World) -> Option<ElementId> {
    world.get_resource::<ActiveDocument>().and_then(super::active_assembly)
}

fn view_of(world: &World, id: ExplodedViewId) -> Option<ExplodedView> {
    world.get_resource::<ActiveDocument>()?.active_element()?.assembly_model()?.exploded_view(id).cloned()
}

fn active_view(world: &World) -> Option<ExplodedView> {
    let (el, id) = world.resource::<ExplodeUi>().active?;
    (element(world)? == el).then_some(())?;
    view_of(world, id)
}

/// The motion of the step being edited.
fn edit_motion(e: &StepEdit, centre: Vec3) -> ExplodeMotion {
    let mut a = [0.0; 3];
    a[e.axis.min(2)] = 1.0;
    if e.rotate {
        ExplodeMotion::Rotate { point: [centre.x as f64, centre.y as f64, centre.z as f64], axis: a, angle: e.amount }
    } else {
        ExplodeMotion::Translate { direction: a, distance: e.amount }
    }
}

/// The view with the step being edited in it (a new step last, a changed one in its place).
fn with_edit(v: &ExplodedView, e: &StepEdit, centre: Vec3) -> ExplodedView {
    let mut v = v.clone();
    if e.instances.is_empty() {
        return v;
    }
    let st = ExplodeStep { instances: e.instances.clone(), motion: edit_motion(e, centre) };
    match e.index {
        Some(i) if i < v.steps.len() => v.steps[i] = st,
        _ => v.steps.push(st),
    }
    v
}

/// The middle of the instances' bounding box, as shown now (the triad's origin, a turn's
/// centre).
fn centre_of(cache: &PartCache, instances: &[InstanceId]) -> Option<Vec3> {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for p in cache.parts.iter().filter(|p| instances.contains(&InstanceId::of_part(p.id))) {
        for q in &p.solid.positions {
            let v = Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32);
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    (lo.x <= hi.x).then(|| (lo + hi) / 2.0)
}

/// The middle of an instance's source part (its own coordinates), for trails.
fn anchor_of(doc: &ActiveDocument, parts: &mut super::AssemblyParts, id: InstanceId) -> [f64; 3] {
    let Some(asm) = doc.active_element().and_then(|e| e.assembly_model()) else { return [0.0; 3] };
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for o in cadrs_core::assembly::structure::occurrences(&doc.doc, asm).into_iter().filter(|o| o.top == id) {
        let Some(b) = parts.build(&doc.doc, o.element) else { continue };
        let Some(p) = b.part(o.part) else { continue };
        for q in &p.solid.positions {
            let w = o.in_top.apply(*q);
            for k in 0..3 {
                lo[k] = lo[k].min(w[k]);
                hi[k] = hi[k].max(w[k]);
            }
        }
    }
    if lo[0] > hi[0] {
        return [0.0; 3];
    }
    [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0)
}

// ---------------------------------------------------------------------------------------------
// The view state

/// The step editor's Instances follow the selection.
fn follow_selection(selection: Res<Selection>, mut ui: ResMut<ExplodeUi>) {
    if !selection.is_changed() {
        return;
    }
    let now = super::selected_instances(&selection);
    if let Some(e) = ui.editing.as_mut()
        && e.instances != now
    {
        e.instances = now;
    }
}

/// Puts the exploded placements of the active view (with the step being edited) into the view.
fn apply_explode(world: &mut World) {
    let ui = world.resource::<ExplodeUi>().clone();
    let asm = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model().cloned());
    let view = active_view(world);
    let want: HashMap<InstanceId, Pose> = match (asm, view) {
        (Some(asm), Some(v)) => {
            let (v, t) = match &ui.editing {
                Some(e) => {
                    // While editing: every step played, the edited one too.
                    let c = centre_of(world.resource::<PartCache>(), &e.instances).unwrap_or(Vec3::ZERO);
                    (with_edit(&v, e, c), 1.0)
                }
                None => (v, ui.fraction),
            };
            explode::exploded_poses(&asm, &v, t).into_iter().collect()
        }
        _ => HashMap::new(),
    };
    let mut parts = world.resource_mut::<super::AssemblyParts>();
    if parts.exploded != want {
        parts.exploded = want;
    }
}

/// Drags an arrow of the explode triad: the distance along its axis.
#[allow(clippy::too_many_arguments)]
fn triad_drag(
    mut inputs: MessageReader<PointerInput>,
    mut ui: ResMut<ExplodeUi>,
    cache: Res<PartCache>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    hover: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut grab: ResMut<super::ViewportGrab>,
    mut commands: Commands,
) {
    let editing = ui.editing.clone();
    let Some(e) = editing.filter(|e| !e.instances.is_empty() && !e.rotate) else {
        inputs.clear();
        ui.drag = None;
        return;
    };
    // The triad sits where the instances are shown before this step's own motion.
    let Some(c) = centre_of(&cache, &e.instances) else {
        inputs.clear();
        return;
    };
    let mut a0 = [0.0f32; 3];
    a0[e.axis] = e.amount as f32;
    let origin = c - Vec3::from_array(a0);
    let v = view.view;
    let len = 70.0 * v.scale;
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let over = pointer_over_viewport(&hover, &q_area);
    // The parameter along `axis` through `origin` nearest the pointer's ray.
    let along = |axis: Vec3, pos: Vec2| -> Option<f64> {
        let (o, d) = v.ray(rect.offset(pos));
        let w = o - origin;
        let (a, b, cc) = (axis.dot(axis), axis.dot(d), d.dot(d));
        let den = a * cc - b * b;
        if den.abs() < 1e-6 {
            return None;
        }
        Some(((b * d.dot(w) - cc * axis.dot(w)) / -den) as f64)
    };
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) if over => {
                let at = rect.offset(pos);
                let hit = axes.iter().enumerate().find(|(_, ax)| {
                    let (p0, p1) = (v.project(origin), v.project(origin + **ax * len));
                    let d = p1 - p0;
                    let t = ((at - p0).dot(d) / d.length_squared().max(1e-6)).clamp(0.0, 1.0);
                    t > 0.15 && (p0 + d * t).distance(at) < 8.0
                });
                if let Some((k, ax)) = hit
                    && let Some(t) = along(*ax, pos)
                {
                    let amount = if k == e.axis { e.amount } else { 0.0 };
                    ui.drag = Some((k, t - amount, amount));
                    grab.0 = true;
                }
            }
            PointerAction::Move { .. } => {
                if let Some((k, start, _)) = ui.drag
                    && let Some(t) = along(axes[k], pos)
                    && let Some(ed) = ui.editing.as_mut()
                {
                    ed.axis = k;
                    ed.amount = ((t - start) * 100.0).round() / 100.0;
                }
            }
            // The drag makes (or changes) the step.
            PointerAction::Release(PointerButton::Primary) | PointerAction::Cancel if ui.drag.take().is_some() => {
                commands.queue(commit_step);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Edits

fn set_view(world: &mut World, v: ExplodedView) -> bool {
    let Some(element) = element(world) else { return false };
    super::run(world, &SetExplodedView { element, view: v })
}

/// Add step / Update step (and the end of a triad drag).
fn commit_step(world: &mut World) {
    let Some(v) = active_view(world) else { return };
    let Some(e) = world.resource::<ExplodeUi>().editing.clone() else { return };
    if e.instances.is_empty() || e.amount == 0.0 {
        return;
    }
    // A turn is about the instances' middle before the step.
    let c = centre_of(world.resource::<PartCache>(), &e.instances).unwrap_or(Vec3::ZERO);
    let c = if e.rotate { c } else { Vec3::ZERO };
    let nv = with_edit(&v, &e, c);
    if set_view(world, nv) {
        let mut ui = world.resource_mut::<ExplodeUi>();
        if let Some(ed) = ui.editing.as_mut() {
            *ed = StepEdit { instances: Vec::new(), axis: e.axis, rotate: e.rotate, ..StepEdit::default() };
        }
        world.resource_mut::<Selection>().0.clear();
    }
}

/// Opens the view `id` for editing (shown fully exploded).
pub fn edit_view(world: &mut World, id: ExplodedViewId) {
    let Some(element) = element(world) else { return };
    let mut ui = world.resource_mut::<ExplodeUi>();
    ui.active = Some((element, id));
    ui.fraction = 1.0;
    ui.editing = Some(StepEdit::default());
    world.resource_mut::<Selection>().0.clear();
}

/// Add exploded view: a new, empty view, opened for editing.
pub fn add_view(world: &mut World) {
    let Some(asm) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model().cloned()) else { return };
    let v = ExplodedView::new(ExplodedViewId::new(), explode::next_name(&asm));
    let id = v.id;
    if set_view(world, v) {
        edit_view(world, id);
    }
}

/// A length in the workspace unit and precision ("120 mm", "2.374 in").
fn length_text(units: &cadrs_sketch::units::Units, mm: f64) -> String {
    format!("{} {}", units.value(mm, Quantity::Length), units.length.symbol())
}

/// An angle (radians) in degrees at the workspace precision ("45°").
fn angle_text(units: &cadrs_sketch::units::Units, rad: f64) -> String {
    format!("{}°", units.value(rad.to_degrees(), Quantity::Angle))
}

/// A step's instances ("Retaining Plate +3") and motion ("Z 120 mm", "Z 45°").
fn step_label(world: &World, st: &ExplodeStep) -> (String, String) {
    let cache = world.resource::<PartCache>();
    let units = world.resource::<crate::WorkspaceUnits>().0;
    let names: Vec<String> = st.instances.iter().map(|i| cache.part_name(super::part_of(*i)).map(str::to_string).unwrap_or_else(|| instance_name(world, *i))).collect();
    let first = names.first().map(|n| n.rsplit_once(" <").map(|(a, _)| a.to_string()).unwrap_or(n.clone())).unwrap_or_default();
    let who = if names.len() > 1 { format!("{first} +{}", names.len() - 1) } else { first };
    let axis = |a: [f64; 3]| ["X", "Y", "Z"][(0..3).max_by(|x, y| a[*x].abs().total_cmp(&a[*y].abs())).unwrap_or(0)];
    let what = match st.motion {
        ExplodeMotion::Translate { direction, distance } => {
            let d = dominant(direction);
            format!("{} {}", axis(direction), length_text(&units, distance * direction[d].signum()))
        }
        ExplodeMotion::Rotate { axis: a, angle, .. } => {
            let d = dominant(a);
            format!("{} {}", axis(a), angle_text(&units, angle * a[d].signum()))
        }
    };
    (who, what)
}

fn instance_name(world: &World, i: InstanceId) -> String {
    let doc = world.resource::<ActiveDocument>();
    doc.active_element()
        .and_then(|e| e.assembly_model()?.instance(i).cloned())
        .map(|inst| inst.name(&cadrs_core::assembly::source_part_name(&doc.doc, &inst.source, None)))
        .unwrap_or_else(|| "instance".into())
}

// ---------------------------------------------------------------------------------------------
// The panel

fn sync_panel(
    doc: Option<Res<ActiveDocument>>,
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    ui: Res<ExplodeUi>,
    cache: Res<PartCache>,
    q_panel: Query<Entity, With<ExplodePanel>>,
    mut commands: Commands,
) {
    let want = *open == SidePanel::ExplodedViews && *kind == ActiveKind::Assembly && doc.as_ref().is_some_and(|d| super::active_assembly(d).is_some());
    if !want {
        for e in &q_panel {
            commands.entity(e).try_despawn();
        }
        if open.is_changed() || kind.is_changed() {
            // Leaving the panel leaves the exploded view shown until it's put back at 0 %; but
            // an edit ends.
            commands.queue(|world: &mut World| world.resource_mut::<ExplodeUi>().editing = None);
        }
        return;
    }
    if doc.as_ref().is_some_and(|d| d.is_changed()) || open.is_changed() || ui.is_changed() || cache.is_changed() || q_panel.is_empty() {
        commands.queue(rebuild);
    }
}

fn rebuild(world: &mut World) {
    let Some(asm) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model().cloned()) else { return };
    let ui = world.resource::<ExplodeUi>().clone();
    let units = world.resource::<crate::WorkspaceUnits>().0;
    let active = ui.active.filter(|(el, _)| Some(*el) == element(world)).map(|(_, id)| id);
    let active_v = active.and_then(|id| asm.exploded_view(id).cloned());
    let steps: Vec<(String, String)> = active_v.as_ref().map(|v| v.steps.iter().map(|s| step_label(world, s)).collect()).unwrap_or_default();
    let edit = ui.editing.clone().filter(|_| active_v.is_some());
    let edit_items: Vec<String> = edit
        .as_ref()
        .map(|e| e.instances.iter().map(|i| world.resource::<PartCache>().part_name(super::part_of(*i)).map(str::to_string).unwrap_or_else(|| instance_name(world, *i))).collect())
        .unwrap_or_default();
    let amount_text = edit
        .as_ref()
        .map(|e| if e.rotate { format!("{} deg", units.value(e.amount.to_degrees(), Quantity::Angle)) } else { length_text(&units, e.amount) })
        .unwrap_or_default();
    let key = PanelKey(format!(
        "{:?}|{:?}|{:.4}|{:?}|{:?}|{:?}|{}",
        asm.exploded_views.iter().map(|v| (&v.id, &v.name, v.trails)).collect::<Vec<_>>(),
        active,
        ui.fraction,
        steps,
        edit.as_ref().map(|e| (e.index, e.rotate, e.axis)),
        edit_items,
        amount_text
    ));
    let mut q = world.query_filtered::<(Entity, &PanelKey), With<ExplodePanel>>();
    let old: Vec<(Entity, PanelKey)> = q.iter(world).map(|(e, k)| (e, k.clone())).collect();
    if old.iter().any(|(_, k)| *k == key) {
        return;
    }
    // Only the slider moved: keep the panel (the slider drags smoothly).
    let slider_only = old.first().is_some_and(|(_, k)| k.0.split('|').enumerate().all(|(i, part)| i == 2 || key.0.split('|').nth(i) == Some(part)));
    if slider_only {
        let pct = format!("{}%", (ui.fraction * 100.0).round());
        let mut q_t = world.query::<(&Name, &mut Text)>();
        for (n, mut t) in q_t.iter_mut(world) {
            if n.as_str() == "exploded-view-percent" && t.0 != pct {
                t.0 = pct.clone();
            }
        }
        for (e, _) in &old {
            world.entity_mut(*e).insert(key.clone());
        }
        return;
    }
    for (e, _) in old {
        world.entity_mut(e).despawn();
    }
    let t = world.resource::<Theme>().clone();
    let trails = active_v.as_ref().is_some_and(|v| v.trails);
    let views: Vec<(ExplodedViewId, String, usize)> = asm.exploded_views.iter().map(|v| (v.id, v.name.clone(), v.steps.len())).collect();
    let panel = world.spawn((Name::new("exploded-views-panel"), ExplodePanel, key, DespawnOnExit(AppState::Document), side_panel_node(&t))).id();
    let mut commands = world.commands();
    commands.entity(panel).insert(Node {
        width: Val::Px(300.0),
        flex_shrink: 0.0,
        flex_direction: FlexDirection::Column,
        border: UiRect::new(Val::Px(1.0), Val::Px(1.0), Val::ZERO, Val::ZERO),
        overflow: Overflow::clip(),
        ..default()
    });
    commands.entity(panel).with_children(|p| {
        side_panel_header(p, &t, "Exploded views", "exploded-views-panel-close");
        p.spawn(Node { padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(6.0), Val::Px(4.0)), ..default() }).with_children(|r| {
            r.spawn(cadrs_ui::Button::new("exploded-view-add").label("Add exploded view").icon("plus").small().build(&t));
        });
        if views.is_empty() {
            p.spawn((
                Name::new("exploded-views-empty"),
                t.text("No exploded views yet", 11.5, FontWeight::NORMAL, t.muted_foreground),
                Node { margin: UiRect::all(Val::Px(10.0)), max_width: Val::Px(235.0), ..default() },
            ));
        }
        for (k, (id, name, n)) in views.iter().enumerate() {
            let is_active = active == Some(*id);
            let mut row = p.spawn((
                TreeItem::new(format!("exploded-view-row-{}", k + 1), name.clone()).icon("explode", 15.0).icon_color(t.muted_foreground).left(8.0).selected(is_active).build(&t),
                ViewRow(*id),
                ContextMenuTarget,
                Tooltip::new(format!("{name}: {n} step{}", if *n == 1 { "" } else { "s" })),
            ));
            row.entry::<Node>().and_modify(|mut nd| nd.height = Val::Px(26.0));
        }
        let Some(v) = active_v.as_ref() else { return };
        // The slider (0 % assembled … 100 % exploded) and the trail lines.
        p.spawn(Node {
            padding: UiRect::new(Val::Px(10.0), Val::Px(8.0), Val::Px(8.0), Val::Px(4.0)),
            column_gap: Val::Px(8.0),
            align_items: AlignItems::Center,
            border: UiRect::top(Val::Px(1.0)),
            ..default()
        })
        .insert(BorderColor::all(t.panel_border))
        .with_children(|r| {
            // Assembled (0 %) at the left end, Exploded (100 %) at the right.
            r.spawn(t.text("Assembled", 10.5, FontWeight::NORMAL, t.muted_foreground));
            r.spawn(Slider::new("exploded-view-slider").value(if edit.is_some() { 1.0 } else { ui.fraction as f32 }).width(84.0).tooltip("0 % assembled … 100 % exploded").build(&t));
            r.spawn(t.text("Exploded", 10.5, FontWeight::NORMAL, t.muted_foreground));
            r.spawn((Name::new("exploded-view-percent"), t.text(format!("{}%", (if edit.is_some() { 1.0 } else { ui.fraction } * 100.0).round()), 11.5, FontWeight::MEDIUM, t.foreground)));
        });
        p.spawn(Node { padding: UiRect::new(Val::Px(10.0), Val::Px(8.0), Val::Px(2.0), Val::Px(6.0)), ..default() })
            .with_children(|r| {
                r.spawn(Checkbox::new("exploded-view-trails").label("Trail lines").checked(trails).build(&t));
            });
        let Some(e) = edit.as_ref() else {
            p.spawn(Node { padding: UiRect::new(Val::Px(10.0), Val::Px(8.0), Val::Px(2.0), Val::Px(6.0)), ..default() }).with_children(|r| {
                r.spawn(cadrs_ui::Button::new("exploded-view-edit-button").label("Edit steps").icon("edit").small().ghost().build(&t));
                // P3F.6 (P3.7): the exploded view as a picture for instructions and manuals.
                r.spawn(cadrs_ui::Button::new("exploded-view-export-image").label("Export image…").icon("image").small().ghost().build(&t));
            });
            return;
        };
        // The step list.
        p.spawn((
            t.text(format!("{}: steps", v.name), 11.5, FontWeight::BOLD, t.foreground),
            Node { margin: UiRect::new(Val::Px(10.0), Val::ZERO, Val::Px(6.0), Val::Px(2.0)), ..default() },
        ));
        if steps.is_empty() {
            p.spawn((t.text("No steps yet", 11.0, FontWeight::NORMAL, t.muted_foreground), Node { margin: UiRect::left(Val::Px(12.0)), ..default() }));
        }
        for (k, (who, what)) in steps.iter().enumerate() {
            let name = format!("exploded-step-{}", k + 1);
            let mut row = p.spawn((
                TreeItem::new(name.clone(), format!("{}. {who}", k + 1)).left(10.0).selected(e.index == Some(k)).build(&t),
                Tooltip::new(format!("Step {}: {who}, {what}. Click to change it", k + 1)),
            ));
            row.entry::<Node>().and_modify(|mut nd| {
                nd.height = Val::Px(24.0);
                nd.padding.right = Val::Px(4.0);
                nd.column_gap = Val::Px(4.0);
            });
            row.with_children(|r| {
                // The motion, right-aligned ("Z 120 mm").
                r.spawn((
                    Name::new(format!("{name}-motion")),
                    t.text(what.clone(), 11.0, FontWeight::MEDIUM, t.muted_foreground),
                    Node { flex_shrink: 0.0, margin: UiRect::left(Val::Auto), ..default() },
                    Pickable::IGNORE,
                ));
                for (suffix, icon, tip) in [("up", "arrow-up", "Move up"), ("down", "arrow-down", "Move down"), ("delete", "delete", "Delete step")] {
                    r.spawn(IconButton::new(format!("{name}-{suffix}"), icon).tooltip(tip).build(&t)).entry::<Node>().and_modify(|mut nd| {
                        nd.width = Val::Px(18.0);
                        nd.height = Val::Px(18.0);
                        nd.flex_shrink = 0.0;
                    });
                }
            });
        }
        // The step editor.
        let head = match e.index {
            Some(i) => format!("Change step {}", i + 1),
            None => "New step".to_string(),
        };
        p.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(5.0),
            padding: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(8.0), Val::Px(8.0)),
            margin: UiRect::top(Val::Px(6.0)),
            border: UiRect::top(Val::Px(1.0)),
            ..default()
        })
        .insert(BorderColor::all(t.panel_border))
        .with_children(|b| {
            b.spawn(t.text(head, 11.5, FontWeight::BOLD, t.foreground));
            b.spawn(SelectionList::new("explode-step-instances").placeholder("Instances (select in the view)").items(edit_items.clone()).active(true).build(&t));
            b.spawn(Node { column_gap: Val::Px(3.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                r.spawn(ToolButton::new("explode-kind-move", "transform").icon_size(15.0).selected(!e.rotate).tooltip("Move along an axis").build(&t));
                r.spawn(ToolButton::new("explode-kind-rotate", "revolve").icon_size(15.0).selected(e.rotate).tooltip("Rotate about an axis").build(&t));
                r.spawn(Node { width: Val::Px(8.0), ..default() });
                for (k, a) in ["x", "y", "z"].iter().enumerate() {
                    r.spawn(cadrs_ui::Button::new(format!("explode-axis-{a}")).label(a.to_uppercase()).small().ghost().selected(e.axis == k).build(&t))
                        .insert(Tooltip::new(format!("{} {}", if e.rotate { "About" } else { "Along" }, a.to_uppercase())));
                }
            });
            b.spawn(NumberField::new("explode-amount", if e.rotate { "Angle" } else { "Distance" }).label_width(58.0).text(amount_text.clone()).build(&t));
            b.spawn((
                t.text("Or drag an arrow of the triad in the view", 10.5, FontWeight::NORMAL, t.muted_foreground),
                Node { max_width: Val::Px(235.0), ..default() },
            ));
            b.spawn(Node { column_gap: Val::Px(6.0), margin: UiRect::top(Val::Px(4.0)), ..default() }).with_children(|r| {
                let ok = !e.instances.is_empty() && e.amount != 0.0;
                r.spawn(cadrs_ui::Button::new("explode-step-add").label(if e.index.is_some() { "Update step" } else { "Add step" }).small().disabled(!ok).build(&t));
                r.spawn(cadrs_ui::Button::new("exploded-view-done").label("Done").small().ghost().build(&t));
            });
        });
    });
    let mut q_area = world.query_filtered::<(Entity, &ChildOf), With<ViewportArea>>();
    let Some((area, parent)) = q_area.iter(world).next().map(|(e, c)| (e, c.parent())) else { return };
    let at = world.get::<Children>(parent).and_then(|c| c.iter().position(|e| e == area)).map_or(0, |i| i + 1);
    world.entity_mut(parent).insert_children(at, &[panel]);
}

fn on_button(a: On<Activate>, q: Query<&Name>, q_row: Query<&ViewRow>, mut commands: Commands) {
    if let Ok(r) = q_row.get(a.entity).copied() {
        // A row: show that view (exploded), or put it away.
        commands.queue(move |world: &mut World| {
            let Some(element) = element(world) else { return };
            let mut ui = world.resource_mut::<ExplodeUi>();
            if ui.active == Some((element, r.0)) && ui.editing.is_none() {
                ui.active = None;
                ui.fraction = 0.0;
            } else {
                ui.active = Some((element, r.0));
                ui.fraction = 1.0;
                ui.editing = None;
            }
        });
        return;
    }
    let Ok(n) = q.get(a.entity) else { return };
    let n = n.as_str().to_string();
    commands.queue(move |world: &mut World| button(world, &n));
}

fn button(world: &mut World, n: &str) {
    match n {
        "exploded-view-add" => add_view(world),
        "exploded-view-edit-button" => {
            if let Some((_, id)) = world.resource::<ExplodeUi>().active {
                edit_view(world, id);
            }
        }
        "exploded-view-export-image" => {
            let name = world.resource::<ExplodeUi>().active.and_then(|(el, id)| {
                let doc = world.get_resource::<ActiveDocument>()?;
                let e = doc.doc.element(el)?;
                Some(format!("{} - {}", e.name, e.assembly_model()?.exploded_view(id)?.name))
            });
            crate::export_image::open(world, name);
        }
        "exploded-view-done" => {
            let mut ui = world.resource_mut::<ExplodeUi>();
            ui.editing = None;
            ui.fraction = 1.0;
        }
        "explode-step-add" => commit_step(world),
        "explode-kind-move" | "explode-kind-rotate" => {
            if let Some(e) = world.resource_mut::<ExplodeUi>().editing.as_mut() {
                let rotate = n == "explode-kind-rotate";
                if e.rotate != rotate {
                    e.rotate = rotate;
                    e.amount = 0.0;
                }
            }
        }
        "explode-axis-x" | "explode-axis-y" | "explode-axis-z" => {
            if let Some(e) = world.resource_mut::<ExplodeUi>().editing.as_mut() {
                e.axis = match n {
                    "explode-axis-x" => 0,
                    "explode-axis-y" => 1,
                    _ => 2,
                };
            }
        }
        _ => {
            let Some(rest) = n.strip_prefix("exploded-step-") else { return };
            let (k, action) = match rest.split_once('-') {
                Some((k, a)) => (k, a),
                None => (rest, "select"),
            };
            let Ok(k) = k.parse::<usize>() else { return };
            step_action(world, k - 1, action);
        }
    }
}

/// A step row's buttons: up, down, delete, or a click (loads it into the editor).
fn step_action(world: &mut World, k: usize, action: &str) {
    let Some(mut v) = active_view(world) else { return };
    if k >= v.steps.len() {
        return;
    }
    match action {
        "up" if k > 0 => v.steps.swap(k, k - 1),
        "down" if k + 1 < v.steps.len() => v.steps.swap(k, k + 1),
        "delete" => {
            v.steps.remove(k);
        }
        "select" => {
            let st = v.steps[k].clone();
            let (rotate, axis, amount) = match st.motion {
                ExplodeMotion::Translate { direction, distance } => (false, dominant(direction), distance * direction[dominant(direction)].signum()),
                ExplodeMotion::Rotate { axis, angle, .. } => (true, dominant(axis), angle * axis[dominant(axis)].signum()),
            };
            world.resource_mut::<Selection>().0 = st.instances.iter().map(|i| crate::viewport::Pick::Part(i.part_id())).collect();
            if let Some(e) = world.resource_mut::<ExplodeUi>().editing.as_mut() {
                *e = StepEdit { index: Some(k), instances: st.instances.clone(), rotate, axis, amount };
            }
            return;
        }
        _ => return,
    }
    if let Some(e) = world.resource_mut::<ExplodeUi>().editing.as_mut() {
        e.index = None;
    }
    set_view(world, v);
}

fn dominant(a: [f64; 3]) -> usize {
    (0..3).max_by(|x, y| a[*x].abs().total_cmp(&a[*y].abs())).unwrap_or(0)
}

fn on_slider(ev: On<SliderChange>, q: Query<&Name>, mut ui: ResMut<ExplodeUi>) {
    if q.get(ev.entity).map(|n| n.as_str()) == Ok("exploded-view-slider") {
        // Moving the slider leaves editing (the steps play).
        ui.editing = None;
        ui.fraction = ev.value.clamp(0.0, 1.0) as f64;
    }
}

fn on_trails(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("exploded-view-trails") {
        return;
    }
    let on = ev.checked;
    commands.queue(move |world: &mut World| {
        if let Some(mut v) = active_view(world)
            && v.trails != on
        {
            v.trails = on;
            set_view(world, v);
        }
    });
}

fn on_amount(ev: On<NumberFieldCommit>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("explode-amount") {
        return;
    }
    let (text, enter) = (ev.text.clone(), ev.enter);
    commands.queue(move |world: &mut World| {
        let units = world.resource::<crate::WorkspaceUnits>().0;
        let rotate = world.resource::<ExplodeUi>().editing.as_ref().is_some_and(|e| e.rotate);
        let q = if rotate { Quantity::Angle } else { Quantity::Length };
        let Ok(v) = units.eval(&text, q) else { return };
        if let Some(e) = world.resource_mut::<ExplodeUi>().editing.as_mut() {
            e.amount = if rotate { v.to_radians() } else { v };
        }
        if enter {
            commit_step(world);
        }
    });
}

fn on_row_menu(ev: On<ContextMenuRequested>, q: Query<&ViewRow>, mut commands: Commands) {
    let Ok(row) = q.get(ev.entity).copied() else { return };
    let (at, e) = (ev.position, ev.entity);
    commands.queue(move |world: &mut World| {
        let at = left_of(world, e, at, 170.0);
        let menu = Menu::new("exploded-view-menu")
            .min_width(170.0)
            .item_height(20.0)
            .item(MenuItem::new("exploded-view-edit", "Edit…").icon("edit"))
            .item(MenuItem::new("exploded-view-rename", "Rename"))
            .separator()
            .item(MenuItem::new("exploded-view-delete", "Delete").icon("remove-circle"));
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
        commands.entity(anchor).insert((ViewMenu(row.0), DespawnOnExit(AppState::Document)));
        world.flush();
    });
}

/// Left of a row (a menu `w` px wide), at its top.
fn left_of(world: &mut World, row: Entity, at: Vec2, w: f32) -> Vec2 {
    let Ok(e) = world.get_entity(row) else { return at };
    let (Some(node), Some(t)) = (e.get::<ComputedNode>(), e.get::<bevy::ui::UiGlobalTransform>()) else { return at };
    let s = node.inverse_scale_factor();
    let size = node.size() * s;
    let c = t.translation * s;
    Vec2::new(c.x - size.x / 2.0 - w - 6.0, c.y - size.y / 2.0)
}

fn on_menu_action(ev: On<MenuAction>, q: Query<&ViewMenu, With<ContextMenuAnchor>>, mut commands: Commands) {
    let Ok(m) = q.get(ev.entity).copied() else { return };
    let item = ev.item.clone();
    commands.queue(move |world: &mut World| match item.as_str() {
        "exploded-view-edit" => edit_view(world, m.0),
        "exploded-view-rename" => {
            let Some(v) = view_of(world, m.0) else { return };
            let mut q = world.query::<(Entity, &ViewRow)>();
            let Some(row) = q.iter(world).find(|(_, r)| r.0 == m.0).map(|(e, _)| e) else { return };
            let at = left_of(world, row, Vec2::new(900.0, 200.0), 150.0);
            let theme = world.resource::<Theme>().clone();
            let mut c = world.commands();
            let popup = NamePopup::new("exploded-view-name", "Rename exploded view", at).value(v.name).spawn(&mut c, &theme);
            c.entity(popup).insert((RenamePopup(m.0), DespawnOnExit(AppState::Document)));
            world.flush();
        }
        "exploded-view-delete" => {
            let Some(element) = element(world) else { return };
            if super::run(world, &DeleteExplodedView { element, id: m.0 }) {
                let mut ui = world.resource_mut::<ExplodeUi>();
                if ui.active.is_some_and(|(_, id)| id == m.0) {
                    ui.active = None;
                    ui.editing = None;
                }
            }
        }
        _ => {}
    });
}

fn on_popup(ev: On<NamePopupCommit>, q: Query<&RenamePopup>, mut commands: Commands) {
    let Ok(p) = q.get(ev.entity).copied() else { return };
    let (value, popup) = (ev.value.trim().to_string(), ev.entity);
    commands.entity(popup).try_despawn();
    commands.queue(move |world: &mut World| {
        if let Some(mut v) = view_of(world, p.0)
            && !value.is_empty()
            && v.name != value
        {
            v.name = value;
            set_view(world, v);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// The trail lines of the view shown, and the explode triad while editing.
fn compute_draw(world: &mut World) {
    let ui = world.resource::<ExplodeUi>().clone();
    let Some(v) = active_view(world) else {
        world.remove_resource::<ExplodeDraw>();
        return;
    };
    let Some(asm) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model().cloned()) else {
        world.remove_resource::<ExplodeDraw>();
        return;
    };
    let (v, t) = match &ui.editing {
        Some(e) => {
            let c = centre_of(world.resource::<PartCache>(), &e.instances).unwrap_or(Vec3::ZERO);
            (with_edit(&v, e, c), 1.0)
        }
        None => (v, ui.fraction),
    };
    let view = world.resource::<ViewportView>().view;
    let mut lines: Vec<(Vec<Vec3>, Color)> = Vec::new();
    if v.trails && t > 0.0 {
        let ids: Vec<InstanceId> = v.steps.iter().flat_map(|s| s.instances.iter().copied()).collect();
        let anchors: HashMap<InstanceId, [f64; 3]> = world.resource_scope(|world, mut parts: Mut<super::AssemblyParts>| {
            let doc = world.resource::<ActiveDocument>();
            ids.iter().map(|i| (*i, anchor_of(doc, &mut parts, *i))).collect()
        });
        for l in explode::trails(&asm, &v, t, |i| anchors.get(&i).copied().unwrap_or([0.0; 3])) {
            lines.push((l.into_iter().map(|p| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)).collect(), Color::srgb_u8(0x3c, 0x40, 0x46)));
        }
    }
    // The triad at the instances being moved (the arrows drag, A1.8).
    let mut arrows: Vec<(Vec3, Vec3, Color)> = Vec::new();
    if let Some(e) = ui.editing.as_ref().filter(|e| !e.instances.is_empty() && !e.rotate)
        && let Some(c) = centre_of(world.resource::<PartCache>(), &e.instances)
    {
        let len = 70.0 * view.scale;
        let colours = [Color::srgb_u8(0xe0, 0x2a, 0x2a), Color::srgb_u8(0x1f, 0xa8, 0x3c), Color::srgb_u8(0x22, 0x55, 0xe0)];
        for (k, ax) in [Vec3::X, Vec3::Y, Vec3::Z].iter().enumerate() {
            let hot = ui.drag.is_some_and(|d| d.0 == k);
            let col = if hot { crate::parts::HOVER } else { colours[k] };
            arrows.push((c, c + *ax * len, col));
        }
    }
    world.insert_resource(ExplodeDraw { lines, arrows, scale: view.scale });
}

/// What [`draw`] draws this frame.
#[derive(Resource, Default)]
struct ExplodeDraw {
    lines: Vec<(Vec<Vec3>, Color)>,
    arrows: Vec<(Vec3, Vec3, Color)>,
    scale: f32,
}

fn draw(d: Option<Res<ExplodeDraw>>, mut gz: Gizmos<TrailGizmos>) {
    let Some(d) = d else { return };
    for (pts, col) in &d.lines {
        // Dashed, as Onshape draws its trail lines.
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let v = b - a;
            let n = (v.length() / (6.0 * d.scale)).ceil().max(1.0) as usize;
            for k in (0..n).step_by(2) {
                let s0 = k as f32 / n as f32;
                let s1 = ((k + 1) as f32 / n as f32).min(1.0);
                gz.line(a + v * s0, a + v * s1, *col);
            }
        }
    }
    for (a, b, col) in &d.arrows {
        gz.line(*a, *b, *col);
        let dir = (*b - *a).normalize_or_zero();
        let side = dir.any_orthonormal_vector() * 5.0 * d.scale;
        let back = *b - dir * 12.0 * d.scale;
        let side2 = dir.cross(side);
        for q in [side, -side, side2, -side2] {
            gz.line(*b, back + q, *col);
        }
    }
}
