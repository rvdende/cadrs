//! The Dimension tool and dimension editing (M7), following `reference/onshape/NOTES.md`
//! ("Dimension tool", `screens/16`–`17`) and `reference/onshape/dimension.md`:
//!
//! - **Dimension (D):** hovering an edge or point highlights it orange; clicking picks it (drawn
//!   in a stronger orange). As soon as the picks can be dimensioned, the dimension follows the
//!   cursor: its kind depends on the picks and where the cursor is (see
//!   [`cadrs_sketch::dimension::propose`]). Clicking another entity that fits adds it (a point
//!   and a line, two points, two lines); clicking anywhere else places the dimension, which is
//!   recorded as a driving dimension with its current value, and opens the **value editor**
//!   ([`cadrs_ui::DimEdit`]) with the value selected. Enter evaluates the text (units and
//!   arithmetic, [`cadrs_sketch::units::eval`]) and sets the value; the solver moves the
//!   geometry and the status colours update. A value that cannot be used shows red with a
//!   message and is not committed. Esc closes the editor, keeping the measured value.
//! - **Existing dimensions:** double-click a value to edit it; drag a value to move the label
//!   (one "Move dimension" step); click to select it, then Delete removes it.
//! - **Circles and arcs** are dimensioned to their near or far side by where they were clicked
//!   (S13.4): the tool keeps each pick's click position for
//!   [`cadrs_sketch::dimension::propose_at`].
//! - **The first dimension scales the sketch** (S13.3): typing the value of a sketch's first
//!   (length) dimension scales all of its geometry about the origin (or the dimension's own
//!   point when the sketch is not attached to the origin), so the typed value holds without
//!   distorting the shape. The value and the scaling are one undo step.
//!
//! Every change goes through [`EditSketch`] commands, so it is undoable.

use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use cadrs_core::commands::EditSketch;
use cadrs_sketch::dimension::{accepts, propose_at};
use cadrs_sketch::hit::hit_test;
use cadrs_sketch::units;
use cadrs_sketch::{Dimension, DimensionId, DimensionKind, SketchEntity, SketchOp};
use cadrs_ui::inline_edit::DOUBLE_CLICK_TIME;
use cadrs_ui::{DimEdit, DimEditBox, DimEditCancel, DimEditCommit, Theme};

use crate::sketch::{ActiveSketchTool, PartStudioMode, SketchSession, SketchTool};
use crate::sketch_glyphs::SketchOverlay;
use crate::sketch_tools::{
    QuickDimFlow, SVec2, SketchScreen, SketchToolsSet, over_viewport, session_sketch,
    world_sketch,
};
use crate::viewport::{ViewportArea, ViewportRect};
use crate::{ActiveDocument, AppState};

pub struct SketchDimensionPlugin;

impl Plugin for SketchDimensionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DimensionTool>()
            .init_resource::<DimensionEditor>()
            .add_systems(
                Update,
                (
                    dimension_pointer.run_if(in_state(PartStudioMode::Sketching)),
                    sync_dim_edit,
                )
                    .chain()
                    .in_set(SketchToolsSet)
                    .after(crate::sketch_tools::sketch_pointer)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(PartStudioMode::Sketching), reset_dimension_tool)
            .add_observer(on_commit)
            .add_observer(on_dimension_menu)
            .add_observer(on_cancel);
    }
}

/// What the Dimension tool has picked, and the dimension following the cursor.
#[derive(Resource, Debug, Default)]
pub struct DimensionTool {
    pub picks: Vec<SketchEntity>,
    /// Where each pick was clicked (sketch mm), for circles' near or far side.
    pub clicks: Vec<SVec2>,
    /// The dimension the picks make with the label at the cursor (drawn with a live value).
    pub preview: Option<Dimension>,
}

/// The open value editor.
#[derive(Resource, Debug, Default)]
pub struct DimensionEditor {
    pub open: Option<OpenEdit>,
    /// The dimension just placed as the sketch's first: its typed value scales the sketch.
    pub first: Option<DimensionId>,
    /// Dimensions whose editor opens next, one after the other as each is committed (a
    /// chamfer's two distances, S9.2). Esc drops them.
    pub next: std::collections::VecDeque<DimensionId>,
}

#[derive(Debug, Clone, Copy)]
pub struct OpenEdit {
    pub entity: Entity,
    pub id: DimensionId,
}

/// A dimension label drawn this frame: its dimension and screen box (center, half size).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DimLabel {
    pub id: DimensionId,
    pub center: Vec2,
    pub half: Vec2,
}

impl DimLabel {
    pub fn contains(&self, p: Vec2) -> bool {
        let d = (p - self.center).abs();
        d.x <= self.half.x && d.y <= self.half.y
    }
}

/// The dimension whose value label is under a screen point.
pub fn dim_at(overlay: &SketchOverlay, p: Vec2) -> Option<DimensionId> {
    overlay
        .dims
        .iter()
        .rev()
        .find(|l| l.contains(p))
        .map(|l| l.id)
}

/// The text a dimension shows: "50", "Ø20", "R8", "30°" (`live` shows 5 decimals, as while the
/// dimension follows the cursor), in the workspace units.
pub fn dimension_text(d: &Dimension, live: bool, u: &units::Units) -> String {
    let q = d.kind.quantity();
    let v = if live {
        u.live_value(d.value, q)
    } else {
        u.value(d.value, q)
    };
    match d.kind {
        DimensionKind::Diameter { .. } | DimensionKind::Diametral { .. } => format!("Ø{v}"),
        DimensionKind::Radius { .. } => format!("R{v}"),
        DimensionKind::Angle { .. } => format!("{v}°"),
        // A polygon's side count: "6x" (`entity_tools/polygon-inscribed-sides.png`).
        DimensionKind::Sides { .. } => format!("{v}x"),
        _ => v,
    }
}

/// [`dimension_text`] of dimension `id` of `sketch`. With the sketch dialog's **Show
/// expressions** on, one driven by variables shows its expression instead of its value (P3F.4:
/// "Ø#piston_d + #clearance"), as Onshape does.
pub fn dimension_text_of(sketch: &cadrs_sketch::Sketch, id: DimensionId, d: &Dimension, live: bool, u: &units::Units, expressions: bool) -> String {
    let v = dimension_text(d, live, u);
    match sketch.expression(id).filter(|_| expressions) {
        Some(e) => match d.kind {
            DimensionKind::Diameter { .. } | DimensionKind::Diametral { .. } => format!("Ø{e}"),
            DimensionKind::Radius { .. } => format!("R{e}"),
            _ => e.to_string(),
        },
        None => v,
    }
}

#[derive(Default)]
struct PointerState {
    press: Option<Vec2>,
    cursor: Option<Vec2>,
    /// The last click on a dimension label, for double-clicks.
    last_label_click: Option<(DimensionId, f32)>,
}

fn reset_dimension_tool(
    mut dim: ResMut<DimensionTool>,
    mut editor: ResMut<DimensionEditor>,
    mut commands: Commands,
) {
    dim.picks.clear();
    dim.preview = None;
    if let Some(open) = editor.open.take() {
        commands.entity(open.entity).try_despawn();
    }
}

#[allow(clippy::too_many_arguments)]
fn dimension_pointer(
    mut inputs: MessageReader<PointerInput>,
    hover_map: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    screen: Res<SketchScreen>,
    tool: Res<ActiveSketchTool>,
    overlay: Res<SketchOverlay>,
    time: Res<Time>,
    mut dim: ResMut<DimensionTool>,
    editor: Res<DimensionEditor>,
    q_edit: Query<(&ComputedNode, &UiGlobalTransform), With<DimEditBox>>,
    mut state: Local<PointerState>,
    external: Res<crate::sketch_tools::ExternalSnap>,
    mut commands: Commands,
) {
    let (Some(map), Some(sketch)) = (
        screen.active,
        session_sketch(session.as_deref(), doc.as_deref()),
    ) else {
        inputs.clear();
        return;
    };
    // Picks and proposals are made on the copy with the face's part edges (a sketch on a
    // face measures to them; `place` uses the ones measured).
    let sketch: &cadrs_sketch::Sketch = session.as_deref().and_then(|s| external.get(s.feature)).map_or(sketch, |(_, e)| &e.sketch);
    let active = tool.tool == SketchTool::Dimension;
    if !active && (!dim.picks.is_empty() || dim.preview.is_some()) {
        dim.picks.clear();
        dim.preview = None;
    }
    // Picks that went away (undo).
    let alive = |e: &SketchEntity| match *e {
        SketchEntity::Point(p) => sketch.points.contains_key(p),
        SketchEntity::Curve(c) => sketch.curves.contains_key(c),
        SketchEntity::Origin => true,
        _ => false,
    };
    if !dim.picks.iter().all(alive) || dim.clicks.len() != dim.picks.len() {
        let kept: Vec<(SketchEntity, Option<SVec2>)> = dim
            .picks
            .iter()
            .enumerate()
            .filter(|(_, e)| alive(e))
            .map(|(i, e)| (*e, dim.clicks.get(i).copied()))
            .collect();
        dim.picks = kept.iter().map(|(e, _)| *e).collect();
        dim.clicks = kept
            .iter()
            .map(|(_, c)| c.unwrap_or(SVec2::new(f64::NAN, f64::NAN)))
            .collect();
    }
    let over = over_viewport(&hover_map, &q_area);
    // Over the open editor?
    let on_editor = |p: Vec2| {
        editor
            .open
            .and_then(|o| q_edit.get(o.entity).ok())
            .is_some_and(|(n, t)| {
                let size = n.size() * n.inverse_scale_factor();
                let center = t.translation * n.inverse_scale_factor();
                (p - center).abs().cmple(size / 2.0).all()
            })
    };
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Move { .. } => state.cursor = Some(pos),
            PointerAction::Press(PointerButton::Primary) => {
                state.cursor = Some(pos);
                state.press = (over && !on_editor(pos)).then_some(pos);
            }
            PointerAction::Release(PointerButton::Primary) => {
                let Some(press) = state.press.take() else {
                    continue;
                };
                if press.distance(pos) > crate::sketch_tools::DRAG_THRESHOLD {
                    continue;
                }
                // A click elsewhere closes the editor (keeping a valid typed value).
                if editor.open.is_some() {
                    commands.queue(|world: &mut World| close_editor(world, true));
                    if active {
                        continue;
                    }
                }
                // Double-clicking a value edits it.
                if let Some(id) = dim_at(&overlay, pos) {
                    let now = time.elapsed_secs();
                    match state.last_label_click {
                        Some((last, t)) if last == id && now - t <= DOUBLE_CLICK_TIME => {
                            state.last_label_click = None;
                            dim.picks.clear();
                            dim.preview = None;
                            commands.queue(move |world: &mut World| open_editor(world, id));
                        }
                        _ => state.last_label_click = Some((id, now)),
                    }
                    continue;
                }
                state.last_label_click = None;
                if !active {
                    continue;
                }
                let hit = hit_test(sketch, SVec2::new(pos.x as f64, pos.y as f64), |p| {
                    map.to_screen64(p)
                })
                .map(|h| h.entity);
                let click = map
                    .to_sketch(pos)
                    .unwrap_or(SVec2::new(f64::NAN, f64::NAN));
                let mut with = dim.picks.clone();
                let fits = hit.is_some_and(|e| {
                    !dim.picks.contains(&e) && dim.picks.len() < 2 && {
                        with.push(e);
                        accepts(sketch, &with)
                    }
                });
                if fits {
                    dim.picks = with;
                    dim.clicks.push(click);
                } else if let Some(d) = dim.preview.take() {
                    // Place it.
                    dim.picks.clear();
                    dim.clicks.clear();
                    commands.queue(move |world: &mut World| place(world, d));
                } else {
                    dim.picks.clear();
                    dim.clicks.clear();
                    if let Some(e) = hit
                        && accepts(sketch, &[e])
                    {
                        dim.picks.push(e);
                        dim.clicks.push(click);
                    }
                }
            }
            _ => {}
        }
    }
    // The dimension follows the cursor: its value centered 5 px above the pointer's tip
    // (`screens/16a`: pointer at (355, 103), text center (354, 98)).
    let want = if active && editor.open.is_none() && !dim.picks.is_empty() {
        let with: Vec<(SketchEntity, Option<SVec2>)> = dim
            .picks
            .iter()
            .enumerate()
            .map(|(i, e)| (*e, dim.clicks.get(i).copied().filter(|c| c.x.is_finite())))
            .collect();
        state
            .cursor
            .and_then(|c| map.to_sketch(c - Vec2::new(0.0, 5.0)))
            .and_then(|c| propose_at(sketch, &with, c))
    } else {
        None
    };
    if dim.preview != want {
        dim.preview = want;
    }
}

/// Records a placed dimension and opens its value editor. A dimension that would over-define
/// the sketch is created driven (and gets no editor), as Onshape does.
fn place(world: &mut World, mut d: Dimension) {
    let Some(s) = world.get_resource::<SketchSession>() else {
        return;
    };
    let (element, feature) = (s.element, s.feature);
    // Measured to a part edge of the face (or its end), picked on the snap copy: the edge is
    // used first (construction, linked, as Onshape does), and the dimension measures to the
    // used curve. The two edits undo as one.
    let mut mark = None;
    let ext = world.resource::<crate::sketch_tools::ExternalSnap>().get(feature).map(|(_, e)| e.clone());
    if let Some(ext) = ext
        && let Some(base) = world_sketch(world).cloned()
    {
        let items = ext.dimension_uses(&base, &d.kind);
        if !items.is_empty() {
            let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
            let m = doc.history.undo_len();
            if let Err(e) = doc.execute(&EditSketch { element, feature, op: SketchOp::UseConstruction { items } }) {
                warn!("cannot use the part edge to dimension to: {e}");
                return;
            }
            let Some(kind) = world_sketch(world).and_then(|used| ext.relinked_dimension(&base, used, d.kind)) else {
                warn!("the part edge to dimension to was not used");
                world.resource_mut::<ActiveDocument>().undo();
                return;
            };
            d.kind = kind;
            mark = Some(m);
        }
    }
    let map = world.resource::<crate::sketch_tools::SketchScreen>().active;
    // The first dimension of a sketch scales it (S13.3), when its value is typed.
    let mut first = false;
    if let Some(sk) = world_sketch(world) {
        first = sk.dimensions.is_empty();
        d.driven = cadrs_sketch::dimension::over_defines(sk, &d);
        // Placed where it was shown while following the cursor.
        if let Some(map) = map {
            let u = world.resource::<crate::WorkspaceUnits>().0;
            let text = dimension_text(&d, true, &u);
            d = crate::sketch_draw::clear_of_arrows(sk, &map, d, &text);
        }
    }
    let op = SketchOp::SetDimension {
        dimension: d,
        moves: vec![],
        radii: vec![],
    };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = doc.execute(&EditSketch {
            element,
            feature,
            op,
        })
    {
        warn!("cannot add the dimension: {e}");
        if mark.is_some() {
            world.resource_mut::<ActiveDocument>().undo();
        }
        return;
    }
    if let Some(m) = mark {
        world.resource_mut::<ActiveDocument>().history.squash_since(m, "Dimension");
    }
    let id = world_sketch(world).and_then(|s| {
        s.dimensions
            .iter()
            .find(|(_, x)| x.kind == d.kind)
            .map(|(k, _)| k)
    });
    if let Some(id) = id
        && !d.driven
    {
        open_editor(world, id);
        if first && d.kind.quantity() == units::Quantity::Length {
            world.resource_mut::<DimensionEditor>().first = Some(id);
        }
    }
}

/// The dimension context menu (right-click on a value), grouped like Onshape's
/// (`dimension/dimension-driven-01.png`): Change to driving dimension (for a driven one),
/// Escape dimension, Delete, Confirm Sketch N | Copy sketch | Show all | Select ▸, Select
/// other… | Add comment | Zoom to fit, View normal to sketch plane.
pub fn open_dimension_menu(world: &mut World, id: DimensionId, at: Vec2) {
    let Some(d) = world_sketch(world).and_then(|s| s.dimensions.get(id).copied()) else {
        return;
    };
    let sketch_name = world
        .get_resource::<SketchSession>()
        .and_then(|s| {
            let doc = world.get_resource::<ActiveDocument>()?;
            Some(doc.doc.element(s.element)?.feature(s.feature)?.name.clone())
        })
        .unwrap_or_else(|| "sketch".into());
    let theme = world.resource::<Theme>().clone();
    world.resource_mut::<crate::sketch_tools::SketchSelection>().0 =
        vec![SketchEntity::Dimension(id)];
    use cadrs_ui::MenuItem as I;
    let mut menu = cadrs_ui::Menu::new("sketch-dimension-menu")
        .min_width(200.0)
        .item_height(23.0)
        .text_only();
    // Toggles driving <-> driven (S13.7).
    menu = if d.driven {
        menu.item(I::new("dimension-make-driving", "Change to driving dimension"))
    } else {
        menu.item(I::new("dimension-make-driven", "Change to driven dimension"))
    };
    menu = menu
        .item(I::new("dimension-escape", "Escape dimension"))
        .item(I::new("dimension-delete", "Delete"))
        .item(I::new("dimension-confirm", format!("Confirm {sketch_name}")))
        .separator()
        .item(I::new("dimension-copy-sketch", "Copy sketch").disabled(true))
        .separator()
        .item(I::new("dimension-show-all", "Show all"))
        .separator()
        .item(I::new("dimension-select", "Select").disabled(true).submenu(vec![]))
        .item(I::new("dimension-select-other", "Select other…").disabled(true))
        .separator()
        .item(I::new("dimension-comment", "Add comment").disabled(true))
        .separator()
        .item(I::new("dimension-zoom-to-fit", "Zoom to fit"))
        .item(I::new("dimension-normal-to", "View normal to sketch plane"));
    let mut commands = world.commands();
    let anchor = cadrs_ui::open_context_menu(&mut commands, at, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((DimensionMenuFor(id), DespawnOnExit(AppState::Document)));
    world.flush();
}

/// The anchor of a dimension's context menu.
#[derive(Component)]
struct DimensionMenuFor(DimensionId);

fn on_dimension_menu(
    ev: On<cadrs_ui::MenuAction>,
    q: Query<&DimensionMenuFor>,
    mut selection: ResMut<crate::sketch_tools::SketchSelection>,
    mut commands: Commands,
) {
    let Ok(anchor) = q.get(ev.entity) else {
        return;
    };
    let id = anchor.0;
    let op = match ev.item.as_str() {
        "dimension-escape" => {
            selection.0.clear();
            return;
        }
        "dimension-confirm" => {
            selection.0.clear();
            commands.queue(crate::sketch::accept_sketch);
            return;
        }
        "dimension-show-all" => {
            commands.queue(|world: &mut World| {
                world
                    .resource_mut::<crate::sketch::SketchViewSettings>()
                    .show_constraints = true;
            });
            return;
        }
        "dimension-zoom-to-fit" => {
            commands.queue(crate::viewport::zoom_to_fit);
            return;
        }
        "dimension-normal-to" => {
            commands.queue(crate::viewport::normal_to_sketch);
            return;
        }
        "dimension-make-driving" => SketchOp::SetDimensionDriven { id, driven: false },
        "dimension-make-driven" => SketchOp::SetDimensionDriven { id, driven: true },
        "dimension-delete" => SketchOp::Delete {
            curves: vec![],
            points: vec![],
            dimensions: vec![id],
            constraints: vec![],
        },
        _ => return,
    };
    selection.0.clear();
    commands.queue(move |world: &mut World| {
        let Some(s) = world.get_resource::<SketchSession>() else {
            return;
        };
        let (element, feature) = (s.element, s.feature);
        if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
            && let Err(e) = doc.execute(&EditSketch {
                element,
                feature,
                op,
            })
        {
            warn!("dimension menu: {e}");
        }
    });
}

/// Opens the value editor on a dimension (closing any other).
pub fn open_editor(world: &mut World, id: DimensionId) {
    close_editor(world, false);
    let Some(d) = world_sketch(world).and_then(|s| s.dimensions.get(id).copied()) else {
        return;
    };
    // A driven dimension only measures: nothing to edit.
    if d.driven {
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let u = world.resource::<crate::WorkspaceUnits>().0;
    let mut q = world.query_filtered::<Entity, With<ViewportArea>>();
    let viewport = q.iter(world).next();
    let Some(area) = world.resource::<crate::sketch_tools::SketchArea>().host(viewport, world.resource::<ViewportRect>()).0 else {
        return;
    };
    let q = d.kind.quantity();
    let entity = world
        .spawn((
            // "50 mm" with the number selected, as Onshape opens it (typing replaces the
            // number and keeps the unit).
            // P3F.4: a dimension driven by variables opens on its expression, all selected.
            match world_sketch(world).and_then(|s| s.expression(id).map(str::to_string)) {
                Some(e) => DimEdit::new("dimension-edit", e.clone()).select_prefix(e.chars().count()),
                None => DimEdit::new("dimension-edit", u.with_unit(d.value, q)).select_prefix(u.live_value(d.value, q).chars().count()),
            }
            .build(&theme),
            Visibility::Hidden,
            ZIndex(6),
        ))
        .insert(ChildOf(area))
        .id();
    world.resource_mut::<DimensionEditor>().open = Some(OpenEdit { entity, id });
}

/// Closes the value editor; with `commit`, a valid typed value is set first.
fn close_editor(world: &mut World, commit: bool) {
    let Some(open) = world.resource_mut::<DimensionEditor>().open.take() else {
        return;
    };
    if commit {
        let text = world
            .get::<Children>(open.entity)
            .and_then(|c| c.get(1).copied())
            .and_then(|row| world.get::<Children>(row).and_then(|c| c.get(1).copied()))
            .and_then(|f| world.get::<bevy::text::EditableText>(f))
            .map(|t| t.value().to_string());
        if let Some(text) = text
            && let Ok(v) = value_for(world, open.id, &text)
        {
            set_value(world, open.id, v, &text);
        }
    }
    if let Ok(e) = world.get_entity_mut(open.entity) {
        e.despawn();
    }
    world.resource_mut::<DimensionEditor>().first = None;
    let frame = world.resource::<bevy::diagnostic::FrameCount>().0;
    world.resource_mut::<QuickDimFlow>().consumed_frame = Some(frame);
    world.resource_mut::<InputFocus>().clear();
    if commit {
        open_next_editor(world);
    }
}

/// Opens the editor queued next (see [`edit_in_turn`]), if any.
fn open_next_editor(world: &mut World) {
    let next = world.resource_mut::<DimensionEditor>().next.pop_front();
    if let Some(id) = next {
        open_editor(world, id);
    }
}

/// Opens the value editor on each of `ids` in turn (the first now, the next as each is
/// committed): a fillet's radius, a chamfer's two distances (S9.1, S9.2), "10 mm" with the
/// number selected, as Onshape edits them (`entity_tools/sketchfilletvertexexample.png`).
pub fn edit_in_turn(world: &mut World, ids: &[DimensionId]) {
    let Some((first, rest)) = ids.split_first() else {
        return;
    };
    open_editor(world, *first);
    world.resource_mut::<DimensionEditor>().next = rest.iter().copied().collect();
}

/// The value typed for a dimension, or why it cannot be used.
fn value_for(world: &World, id: DimensionId, text: &str) -> Result<f64, String> {
    let d = world_sketch(world)
        .and_then(|s| s.dimensions.get(id).copied())
        .ok_or("The dimension is gone")?;
    let q = d.kind.quantity();
    // P3F.4: `#piston_d + #clearance` reads the Part Studio's variables.
    let v = crate::variables_ui::eval_field(world, text, q).map_err(|e| e.to_string())?;
    if v <= 0.0 {
        return Err("Enter a positive value".into());
    }
    if q == units::Quantity::Angle && v >= 180.0 {
        return Err("Enter an angle less than 180°".into());
    }
    Ok(v)
}

fn set_value(world: &mut World, id: DimensionId, value: f64, text: &str) {
    let Some(s) = world.get_resource::<SketchSession>() else {
        return;
    };
    let (element, feature) = (s.element, s.feature);
    // P3F.4: an expression naming a variable is kept with the value (and re-evaluated when the
    // variable changes); a plain value drops it.
    let expr = text.contains('#').then(|| text.trim().to_string());
    let old_expr = world_sketch(world).and_then(|s| s.expression(id).map(str::to_string));
    let same = world_sketch(world)
        .and_then(|s| s.dimensions.get(id))
        .is_some_and(|d| d.value == value);
    if same && expr == old_expr {
        return;
    }
    let first = world.resource::<DimensionEditor>().first == Some(id);
    let op = match world_sketch(world) {
        _ if same => SketchOp::SetDimensionExpr { id, expr: expr.clone() },
        Some(sk) if first => set_value_op(sk, id, value, true),
        _ => SketchOp::SetDimensionValue { id, value },
    };
    let op = if same || expr == old_expr { op } else { SketchOp::Batch(vec![op, SketchOp::SetDimensionExpr { id, expr }]) };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = doc.execute(&EditSketch {
            element,
            feature,
            op,
        })
    {
        warn!("cannot set the dimension: {e}");
    }
}

/// The edit that gives dimension `id` the value `value`, scaling the whole sketch for its first
/// dimension (see [`cadrs_sketch::edit::set_value_op`]).
pub fn set_value_op(s: &cadrs_sketch::Sketch, id: DimensionId, value: f64, first: bool) -> SketchOp {
    cadrs_sketch::edit::set_value_op(s, id, value, first)
}

fn editor_box(world: &World, field: Entity) -> Option<Entity> {
    let row = world.get::<ChildOf>(field)?.parent();
    Some(world.get::<ChildOf>(row)?.parent())
}

fn on_commit(ev: On<DimEditCommit>, mut commands: Commands) {
    let field = ev.entity;
    let text = ev.value.clone();
    commands.queue(move |world: &mut World| {
        let Some(open) = world.resource::<DimensionEditor>().open else {
            return;
        };
        if editor_box(world, field) != Some(open.entity) {
            return;
        }
        match value_for(world, open.id, &text) {
            Ok(v) => {
                set_value(world, open.id, v, &text);
                close_editor(world, false);
                open_next_editor(world);
            }
            Err(msg) => {
                if let Some(mut b) = world.get_mut::<DimEditBox>(open.entity) {
                    b.set_error(msg, text);
                }
                let frame = world.resource::<bevy::diagnostic::FrameCount>().0;
                world.resource_mut::<QuickDimFlow>().consumed_frame = Some(frame);
            }
        }
    });
}

fn on_cancel(ev: On<DimEditCancel>, mut commands: Commands) {
    let field = ev.entity;
    commands.queue(move |world: &mut World| {
        let Some(open) = world.resource::<DimensionEditor>().open else {
            return;
        };
        if editor_box(world, field) == Some(open.entity) {
            world.resource_mut::<DimensionEditor>().next.clear();
            close_editor(world, false);
        }
    });
}

/// Keeps the editor on its dimension's value (it is hidden until the label has been drawn);
/// closes it if the dimension went away.
#[allow(clippy::too_many_arguments)]
fn sync_dim_edit(
    mut editor: ResMut<DimensionEditor>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    overlay: Res<SketchOverlay>,
    viewport_rect: Res<ViewportRect>,
    sketch_area: Res<crate::sketch_tools::SketchArea>,
    mut q: Query<(&mut Node, &mut Visibility), With<DimEditBox>>,
    mut commands: Commands,
) {
    let Some(open) = editor.open else {
        return;
    };
    let gone = session_sketch(session.as_deref(), doc.as_deref())
        .is_none_or(|s| !s.dimensions.contains_key(open.id));
    if gone {
        editor.open = None;
        commands.entity(open.entity).try_despawn();
        return;
    }
    let Some(label) = overlay.dims.iter().find(|l| l.id == open.id) else {
        return;
    };
    let Ok((mut node, mut vis)) = q.get_mut(open.entity) else {
        return;
    };
    // The value row is level with the label and the box starts at the label's left edge,
    // over it (`screens/16b`); the label itself is not drawn while it is edited, so no half
    // value shows beside the box (T3 judge).
    let row_center = cadrs_ui::dim_edit::HEIGHT - 17.0;
    let rect = sketch_area.rect(&viewport_rect);
    let local = label.center - rect.0.min + Vec2::new(-label.half.x, -row_center);
    let (left, top) = (Val::Px(local.x.round()), Val::Px(local.y.round()));
    if node.left != left || node.top != top {
        node.left = left;
        node.top = top;
    }
    vis.set_if_neq(Visibility::Inherited);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts() {
        let d = |kind, value| Dimension::new(kind, value, 0.0);
        let mm = units::Units::default();
        let mut s = cadrs_sketch::Sketch::new();
        let c = s.add_line(SVec2::ZERO, SVec2::new(1.0, 0.0));
        let (a, b) = s.curve_ends(c).unwrap();
        assert_eq!(dimension_text(&d(DimensionKind::Aligned { a, b }, 50.0), false, &mm), "50");
        assert_eq!(
            dimension_text(&d(DimensionKind::Aligned { a, b }, 33.173_891), true, &mm),
            "33.17389"
        );
        assert_eq!(dimension_text(&d(DimensionKind::Diameter { curve: c }, 20.0), false, &mm), "Ø20");
        assert_eq!(dimension_text(&d(DimensionKind::Radius { curve: c }, 8.5), false, &mm), "R8.5");
        let cr = cadrs_sketch::CurveRef::Curve(c);
        assert_eq!(
            dimension_text(
                &d(
                    DimensionKind::Angle {
                        a: cr,
                        b: cr,
                        flip_a: false,
                        flip_b: false
                    },
                    47.7291
                ),
                false,
                &mm
            ),
            "47.729°"
        );
        // In inches.
        let inch = units::Units::new(units::LengthUnit::Inch, 3);
        assert_eq!(
            dimension_text(&d(DimensionKind::Aligned { a, b }, 50.8), false, &inch),
            "2"
        );
        assert_eq!(
            dimension_text(&d(DimensionKind::Diameter { curve: c }, 25.4), false, &inch),
            "Ø1"
        );
    }
}
