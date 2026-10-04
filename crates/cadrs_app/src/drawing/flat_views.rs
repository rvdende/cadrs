//! Flat pattern views in drawings (P3I.7, SM16; lesson `16-drawings`, exercise E3), the app
//! side of [`cadrs_drawing::flat_view`]:
//!
//! - **Insert view → Flat patterns** (SM16.2): the browser's type filters get a flat pattern
//!   button; with it on, the tree lists every sheet metal part's flat pattern ("Flat pattern of
//!   Sheet Metal Box", `ex3-drawings/step-08`). Picking one makes the ghost a flat pattern view (Top), placed like any view;
//!   the tool then goes on to projected views of it, as Onshape's does.
//! - **Drawing**: bend lines in the view's up and down pens, bend notes along their lines (or
//!   off them with a leader). A selected view draws them orange.
//! - **Context menu** of a flat view (`t0112.3.png`): Show/hide ▸ (Hide/Show bend lines, Hide/Show
//!   bend notes, Show/Hide hidden lines), View orientation ▸ (Top, Bottom, Rotate 90°), Tangent edges ▸
//!   (Hidden, Solid, Phantom), Adjust linestyle… (greyed), View properties…, Order ▸ (greyed, as
//!   on part views), Align view ▸, Switch to, Move to sheet…, Copy (greyed), Clear selection,
//!   Zoom to fit, Delete. A right-click on a bend note opens Hide bend notes.
//! - **Dragging a bend note** (SM16.4): press on a note and drag its node; drop it near its bend
//!   line and it goes back on the line, else it stays where dropped with a leader. One undoable
//!   view edit ("Move bend note").
//! - **View properties** (`properties-flatpattern.png`): Up and Down bend lines' weight and
//!   colour.
//! - **Create drawing of flat pattern** (SM16.1): [`open_create_drawing_of_flat`] opens the
//!   Create Drawing dialog; OK makes the drawing and arms Insert view with the flat. The Parts
//!   list's menu offers it for sheet metal parts; the flat view panel's menu (P3I.3) calls the
//!   same function.
//!
//! Every change goes through [`cadrs_drawing::DrawingOp::SetView`].

use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::{ElementId, PartId};
use cadrs_drawing::flat_view::{self as fv, BendLineStyle, FlatSettings};
use cadrs_drawing::style::TangentEdges;
use cadrs_drawing::{NamedView, ObjectRef, View, ViewId};
use cadrs_ui::prelude::*;
use cadrs_ui::{MenuEntry, Select, SelectState, form_row};

use super::view_tools::{InsertViewState, ViewTool};
use super::views::{Stroke, ViewCache};
use super::{DrawingUi, active_drawing, current_view, screen_to_sheet, sheet_area};
use crate::viewport::{ActiveKind, ViewportArea, ViewportRect};
use crate::{ActiveDocument, AppState};

pub struct FlatViewsPlugin;

impl Plugin for FlatViewsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NoteDrag>()
            .add_systems(
                Update,
                note_pointer
                    .after(super::notes::NotesInputSet)
                    .before(super::view_tools::ViewToolsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_observer(on_flat_menu_action);
    }
}

fn rgb(c: [u8; 3]) -> Color {
    Color::srgb_u8(c[0], c[1], c[2])
}

/// A flat view's bend lines as strokes: in their pens, or all in `highlight`.
pub fn bend_strokes(v: &View, g: &cadrs_core::views::ViewGeometry, highlight: Option<Color>) -> Vec<Stroke> {
    let Some(flat) = &g.flat else { return Vec::new() };
    let mut out = Vec::new();
    for l in fv::bend_lines(v, flat) {
        let color = highlight.unwrap_or(rgb(l.style.color));
        let medium = l.style.weight >= 0.35;
        for d in l.dashes() {
            out.push(Stroke { points: d.iter().map(|p| Vec2::new(p[0] as f32, p[1] as f32)).collect(), medium, color });
        }
    }
    out
}

/// A flat view's centermarks (round holes, counterbores and countersinks, forms; SM16.3) as
/// thin strokes.
pub fn centermark_strokes(style: &cadrs_drawing::DrawingStyle, v: &View, g: &cadrs_core::views::ViewGeometry, color: Color) -> Vec<Stroke> {
    let Some(flat) = &g.flat else { return Vec::new() };
    fv::centermarks(style, v, flat)
        .into_iter()
        .map(|l| Stroke { points: l.iter().map(|p| Vec2::new(p[0] as f32, p[1] as f32)).collect(), medium: false, color })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Insert view

/// The browser's name of a flat pattern reference.
pub fn flat_label(st: &InsertViewState, r: &ObjectRef) -> Option<String> {
    let (f, index) = r.part?;
    let id = PartId { feature: cadrs_core::FeatureId(f), index };
    st.flats.iter().find(|(e, p, _)| e.0 == r.element && *p == id).map(|(_, _, n)| n.clone())
}

/// The Flat patterns filter button.
pub fn flat_filter_button(f: &mut ChildSpawnerCommands, t: &Theme, st: &InsertViewState) {
    f.spawn((
        ToolButton::new("insert-view-filter-flat", "flat-pattern")
            .icon_size(16.0)
            .selected(st.filter_flat)
            .tooltip("Flat patterns")
            .build(t),
        observe(|_: On<Activate>, mut s: ResMut<InsertViewState>| {
            s.filter_flat = !s.filter_flat;
        }),
    ));
}

/// The flat patterns in the browser (with the filter on).
pub fn flat_rows(tree: &mut ChildSpawnerCommands, t: &Theme, st: &InsertViewState, q: &str) {
    let shown: Vec<&(ElementId, PartId, String)> = st.flats.iter().filter(|(_, _, n)| q.is_empty() || n.to_lowercase().contains(q)).collect();
    if shown.is_empty() {
        tree.spawn((
            t.text("No sheet metal flat patterns in this document", t.font_sm, bevy::text::FontWeight::NORMAL, t.muted_foreground),
            Node { margin: UiRect::all(Val::Px(10.0)), ..default() },
        ));
    }
    // Grouped under their Part Studio (`ex3-drawings/step-08`).
    let mut last: Option<ElementId> = None;
    for (i, (e, p, name)) in shown.into_iter().enumerate() {
        if last != Some(*e) {
            last = Some(*e);
            let studio = st.studios.iter().find(|s| s.id == *e).map(|s| s.name.clone()).unwrap_or_default();
            tree.spawn(
                TreeItem::new(format!("browser-flat-studio-{}", i + 1), studio)
                    .icon("part-studio", 16.0)
                    .icon_color(t.muted_foreground)
                    .left(4.0)
                    .height(28.0)
                    .build(t),
            );
        }
        let r = ObjectRef { element: e.0, part: Some((p.feature.0, p.index)) };
        tree.spawn((
            TreeItem::new(format!("browser-flat-{}", i + 1), name.clone())
                .icon("flat-pattern", 20.0)
                .icon_color(t.tool_foreground)
                .selected(st.flat && st.reference == Some(r))
                .left(24.0)
                .height(32.0)
                .build(t),
            Tooltip::new(name.clone()),
            observe(move |_: On<Activate>, mut state: ResMut<InsertViewState>| {
                state.reference = Some(r);
                state.flat = true;
                state.orientation = NamedView::Top;
            }),
        ));
    }
}

/// Arms Insert view with the flat pattern of `r` (after Create drawing of flat pattern).
pub fn arm_flat(world: &mut World, r: ObjectRef) {
    super::view_tools::open_insert_view(world);
    let mut s = world.resource_mut::<InsertViewState>();
    s.reference = Some(r);
    s.flat = true;
    s.filter_flat = true;
    s.orientation = NamedView::Top;
    s.browser_open = false;
}

/// **Create drawing of flat pattern** (SM16.1) of part `r`: the Create Drawing dialog; OK opens
/// the new drawing with Insert view armed for the flat. What the flat view panel's right-click
/// menu (P3I.3) calls.
pub fn open_create_drawing_of_flat(world: &mut World, r: ObjectRef) {
    super::create_dialog::open_create_drawing_flat(world, r);
}

/// Whether part `part` of Part Studio `element` is a sheet metal part with a flat pattern.
pub fn is_sheet_metal_part(doc: &cadrs_core::Document, element: ElementId, part: PartId) -> bool {
    let Some(el) = doc.element(element) else { return false };
    let build = cadrs_core::rebuild::build(el.features());
    cadrs_core::flat_drawing::flat_parts(&build.sheet_metal).contains(&part)
}

// ---------------------------------------------------------------------------------------------
// Menus

/// The anchor of a flat view's menu (or a bend note's).
#[derive(Component, Clone, Copy)]
struct FlatMenuFor(ViewId);

fn find_view(world: &World, id: ViewId) -> Option<View> {
    let doc = world.get_resource::<ActiveDocument>()?;
    let (_, d) = active_drawing(doc)?;
    d.view(id).map(|(_, v)| v.clone())
}

/// A flat pattern view's context menu (`t0112.3.png`).
pub fn open_flat_view_menu(world: &mut World, pos: Vec2, id: ViewId) {
    let Some(v) = find_view(world, id) else { return };
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let reference = doc.doc.element(ElementId(v.reference.element)).map(|e| e.name.clone()).unwrap_or_else(|| "the referenced tab".into());
    let f = v.flat.clone().unwrap_or_default();
    let theme = world.resource::<Theme>().clone();
    let tangent = |t: TangentEdges, name: &'static str, label: &'static str| {
        let item = MenuItem::new(name, label);
        MenuEntry::Item(if v.tangent_edges == t { item.icon("check") } else { item })
    };
    let orientation = |o: NamedView, name: &'static str| {
        let item = MenuItem::new(name, o.label());
        MenuEntry::Item(if v.frame == o.frame() { item.icon("check") } else { item })
    };
    // Onshape's flat view menu (`16-drawings/t0112.3.png`): Show/hide ▸, View orientation ▸,
    // Tangent edges ▸, Adjust linestyle…, View properties…, Order ▸, Align view ▸, Switch to,
    // Move to sheet…, Copy, Clear selection, Zoom to fit, Delete. cadrs has no line style
    // overrides or view copy yet, so those two are greyed; Order is greyed as on part views.
    let menu = Menu::new("flat-view-context-menu")
        .min_width(220.0)
        .item(MenuItem::new("flat-menu-show-hide", "Show/hide").submenu(vec![
            MenuEntry::Item(MenuItem::new("flat-menu-bend-lines", if f.bend_lines_hidden { "Show bend lines" } else { "Hide bend lines" })),
            MenuEntry::Item(MenuItem::new("flat-menu-bend-notes-sub", if f.bend_notes_hidden { "Show bend notes" } else { "Hide bend notes" })),
            MenuEntry::Item(MenuItem::new("view-menu-hidden-lines", if v.hidden_lines { "Hide hidden lines" } else { "Show hidden lines" })),
        ]))
        .item(MenuItem::new("flat-menu-orientation", "View orientation").submenu(vec![
            orientation(NamedView::Top, "flat-menu-orientation-top"),
            orientation(NamedView::Bottom, "flat-menu-orientation-bottom"),
            MenuEntry::Item(MenuItem::new("flat-menu-rotate-ccw", "Rotate 90° counterclockwise")),
            MenuEntry::Item(MenuItem::new("flat-menu-rotate-cw", "Rotate 90° clockwise")),
        ]))
        .item(MenuItem::new("flat-menu-tangent-edges", "Tangent edges").submenu(vec![
            tangent(TangentEdges::Hidden, "view-menu-tangent-hidden", "Hidden"),
            tangent(TangentEdges::Solid, "view-menu-tangent-solid", "Solid"),
            tangent(TangentEdges::Phantom, "view-menu-tangent-phantom", "Phantom"),
        ]))
        .item(MenuItem::new("flat-menu-linestyle", "Adjust linestyle…").disabled(true))
        .separator()
        .item(MenuItem::new("view-menu-properties", "View properties…"))
        .item(MenuItem::new("flat-menu-order", "Order").submenu(vec![
            MenuEntry::Item(MenuItem::new("view-menu-front", "Bring to front").disabled(true)),
            MenuEntry::Item(MenuItem::new("view-menu-back", "Send to back").disabled(true)),
        ]))
        .item(MenuItem::new("flat-menu-align", "Align view").submenu(vec![
            MenuEntry::Item(MenuItem::new("view-menu-align-vertical", "Align view vertical")),
            MenuEntry::Item(MenuItem::new("view-menu-align-horizontal", "Align view horizontal")),
        ]))
        .separator()
        .item(MenuItem::new("view-menu-switch", format!("Switch to {reference}")))
        .item(MenuItem::new("view-menu-move-to-sheet", "Move to sheet…"))
        .item(MenuItem::new("flat-menu-copy", "Copy").disabled(true))
        .separator()
        .item(MenuItem::new("view-menu-clear-selection", "Clear selection"))
        .item(MenuItem::new("view-menu-zoom", "Zoom to fit"))
        .item(MenuItem::new("view-menu-delete", "Delete"));
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, pos, menu.build(&theme));
    commands.entity(anchor).insert((FlatMenuFor(id), DespawnOnExit(AppState::Document)));
    world.flush();
}

/// The bend notes of the active sheet's flat views: (view, its notes).
fn sheet_notes(world: &World) -> Vec<(View, Vec<fv::NoteGraphics>)> {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return Vec::new() };
    let Some((id, d)) = active_drawing(doc) else { return Vec::new() };
    let ui = world.resource::<DrawingUi>();
    let cache = world.resource::<ViewCache>();
    super::views::shown_views(d, ui.sheet_index(id, d), ui)
        .into_iter()
        .filter_map(|v| {
            let g = cache.geometry(&v)?;
            let notes = fv::bend_notes(&d.style, &v, g.flat.as_ref()?);
            Some((v, notes))
        })
        .collect()
}

/// Right-click on a bend note: its menu (Hide bend notes). Returns whether there was one.
pub fn open_bend_note_menu(world: &mut World, pos: Vec2) -> bool {
    let rect = *world.resource::<ViewportRect>();
    let Some(p) = (|| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let ui = world.resource::<DrawingUi>();
        let (_, view) = current_view(doc, ui)?;
        Some((screen_to_sheet(view, sheet_area(&rect, ui), pos), 4.0 / view.ppm as f64))
    })() else {
        return false;
    };
    let (p, tol) = p;
    let hit = sheet_notes(world).into_iter().find(|(_, notes)| fv::note_at(notes, [p.x as f64, p.y as f64], tol).is_some());
    let Some((v, _)) = hit else { return false };
    let theme = world.resource::<Theme>().clone();
    let menu = Menu::new("bend-note-context-menu")
        .min_width(180.0)
        .item(MenuItem::new("flat-menu-bend-notes", "Hide bend notes"));
    world.resource_mut::<DrawingUi>().hovered = Some(v.id);
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, pos, menu.build(&theme));
    commands.entity(anchor).insert((FlatMenuFor(v.id), DespawnOnExit(AppState::Document)));
    world.flush();
    true
}

fn on_flat_menu_action(ev: On<MenuAction>, q: Query<&FlatMenuFor>, mut commands: Commands) {
    let Ok(target) = q.get(ev.entity) else { return };
    let id = target.0;
    let item = ev.item.clone();
    commands.queue(move |w: &mut World| flat_menu_action(w, id, &item));
}

fn set_flat(w: &mut World, id: ViewId, label: &str, f: impl FnOnce(&mut FlatSettings)) {
    super::view_menu::set_view(w, id, label, |v| {
        let mut s = v.flat.clone().unwrap_or_default();
        f(&mut s);
        v.flat = Some(s);
    });
}

fn flat_menu_action(w: &mut World, id: ViewId, item: &str) {
    let Some(v) = find_view(w, id) else { return };
    let f = v.flat.clone().unwrap_or_default();
    match item {
        "flat-menu-bend-lines" => {
            let hide = !f.bend_lines_hidden;
            set_flat(w, id, if hide { "Hide bend lines" } else { "Show bend lines" }, |s| s.bend_lines_hidden = hide);
        }
        "flat-menu-bend-notes" | "flat-menu-bend-notes-sub" => {
            let hide = !f.bend_notes_hidden;
            set_flat(w, id, if hide { "Hide bend notes" } else { "Show bend notes" }, |s| s.bend_notes_hidden = hide);
        }
        "flat-menu-orientation-top" | "flat-menu-orientation-bottom" => {
            let o = if item == "flat-menu-orientation-top" { NamedView::Top } else { NamedView::Bottom };
            if v.frame != o.frame() {
                // The notes' places are in the view's 2D frame: they come out by themselves again.
                super::view_menu::set_view(w, id, "View orientation", |v| {
                    v.frame = o.frame();
                    if let Some(f) = v.flat.as_mut() {
                        f.notes.clear();
                    }
                });
            }
        }
        "flat-menu-rotate-ccw" | "flat-menu-rotate-cw" => {
            let turn = if item == "flat-menu-rotate-ccw" { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
            // About the view's middle, so it stays where it is on the sheet.
            let c = w.resource::<ViewCache>().geometry(&v).and_then(|g| g.bounds).map(|(lo, hi)| [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]).unwrap_or([0.0, 0.0]);
            super::view_menu::set_view(w, id, "Rotate view", |v| {
                let at = v.to_sheet(c);
                v.rotation = (v.rotation + turn).rem_euclid(std::f64::consts::TAU);
                let q = cadrs_drawing::view::rotate([c[0] * v.scale.factor(), c[1] * v.scale.factor()], v.rotation);
                v.anchor = [at[0] - q[0], at[1] - q[1]];
            });
        }
        other => super::view_menu::view_menu_action(w, id, other),
    }
}

// ---------------------------------------------------------------------------------------------
// View properties

fn weight_select(name: &str, current: f64, t: &Theme) -> impl Bundle {
    let mut s = Select::new(name.to_string()).width(Val::Px(90.0));
    for w in fv::WEIGHTS {
        s = s.option(format!("{w:.2} mm"), true);
    }
    let i = fv::WEIGHTS.iter().position(|w| (w - current).abs() < 1e-9).unwrap_or(2);
    s.selected(i).build(t)
}

fn color_select(name: &str, current: [u8; 3], t: &Theme) -> impl Bundle {
    let mut s = Select::new(name.to_string()).width(Val::Px(100.0));
    for (n, _) in fv::COLORS {
        s = s.option(n, true);
    }
    let i = fv::COLORS.iter().position(|(_, c)| *c == current).unwrap_or(0);
    s.selected(i).build(t)
}

/// The View properties dialog's Flat pattern rows: Up and Down bend lines' weight and colour.
pub fn properties_rows(b: &mut ChildSpawner, t: &Theme, f: &FlatSettings, lw: f32) {
    b.spawn((
        Name::new("view-props-flat-title"),
        t.text("Flat pattern", t.font_base, bevy::text::FontWeight::SEMIBOLD, t.foreground),
        Node { margin: UiRect::top(Val::Px(6.0)), ..default() },
    ));
    for (row, label, style, w, c) in [
        ("view-props-up-row", "Up bend lines", f.up, "view-props-up-weight", "view-props-up-color"),
        ("view-props-down-row", "Down bend lines", f.down, "view-props-down-weight", "view-props-down-color"),
    ] {
        b.spawn(form_row(t, row, label, lw)).with_children(|r| {
            r.spawn(weight_select(w, style.weight, t));
            r.spawn(Node { width: Val::Px(6.0), ..default() });
            r.spawn(color_select(c, style.color, t));
        });
    }
}

fn select_value(world: &mut World, name: &str) -> Option<usize> {
    let mut q = world.query::<(&Name, &SelectState)>();
    q.iter(world).find(|(n, _)| n.as_str() == name).map(|(_, s)| s.selected)
}

/// OK in View properties: the bend line pens, if changed.
pub fn apply_properties(world: &mut World, id: ViewId) {
    let Some(v) = find_view(world, id) else { return };
    let Some(f) = v.flat.clone() else { return };
    let pen = |world: &mut World, w: &str, c: &str, old: BendLineStyle| BendLineStyle {
        weight: select_value(world, w).and_then(|i| fv::WEIGHTS.get(i).copied()).unwrap_or(old.weight),
        color: select_value(world, c).and_then(|i| fv::COLORS.get(i).map(|x| x.1)).unwrap_or(old.color),
    };
    let up = pen(world, "view-props-up-weight", "view-props-up-color", f.up);
    let down = pen(world, "view-props-down-weight", "view-props-down-color", f.down);
    if up != f.up || down != f.down {
        set_flat(world, id, "Edit bend line style", |s| {
            s.up = up;
            s.down = down;
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Dragging bend notes

/// A bend note pressed on: dragged once the pointer moves a few pixels.
#[derive(Resource, Default, Clone, Copy)]
struct NoteDrag(Option<(ViewId, u32, Vec2, [f64; 2], bool)>);

#[allow(clippy::too_many_arguments)]
fn note_pointer(
    mut inputs: MessageReader<PointerInput>,
    kind: Res<ActiveKind>,
    doc: Option<Res<ActiveDocument>>,
    rect: Res<ViewportRect>,
    hover_map: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    ann: Res<super::annotations::AnnotationUi>,
    cache: Res<ViewCache>,
    mut drag: ResMut<NoteDrag>,
    mut dui: ResMut<DrawingUi>,
    mut commands: Commands,
) {
    if *kind != ActiveKind::Drawing {
        inputs.clear();
        return;
    }
    let Some(doc) = doc else {
        inputs.clear();
        return;
    };
    let Some((((element, _), view), (_, d))) = current_view(&doc, &dui).zip(active_drawing(&doc)) else {
        inputs.clear();
        return;
    };
    let index = dui.sheet_index(element, d);
    let area = sheet_area(&rect, &dui);
    let over = super::view_tools::pointer_over_sheet(&hover_map, &q_area) && q_menus.is_empty() && q_dialogs.is_empty();
    let px = 1.0 / view.ppm as f64;
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let p = screen_to_sheet(view, area, input.location.position);
        match input.action {
            PointerAction::Press(PointerButton::Primary) if over => {
                if ann.tool != super::annotations::AnnTool::None || dui.tool != ViewTool::None || dui.annotation_press {
                    continue;
                }
                let views = super::views::shown_views(d, index, &dui);
                let hit = views.iter().find_map(|v| {
                    let g = cache.geometry(v)?;
                    let notes = fv::bend_notes(&d.style, v, g.flat.as_ref()?);
                    let bend = fv::note_at(&notes, [p.x as f64, p.y as f64], 4.0 * px)?;
                    let node = notes.iter().find(|n| n.bend == bend)?.node;
                    Some((v.id, bend, node))
                });
                if let Some((vid, bend, node)) = hit {
                    dui.annotation_press = true;
                    dui.selected = vec![vid];
                    drag.0 = Some((vid, bend, p, node, false));
                }
            }
            PointerAction::Move { .. } => {
                let Some((vid, bend, start, node, moving)) = drag.0 else { continue };
                let moving = moving || (p - start).length() as f64 > 3.0 * px;
                drag.0 = Some((vid, bend, start, node, moving));
                if !moving {
                    continue;
                }
                let Some((_, v)) = d.view(vid) else { continue };
                let Some(g) = cache.geometry(v) else { continue };
                let Some(flat) = g.flat.as_ref() else { continue };
                let drop = [node[0] + (p.x - start.x) as f64, node[1] + (p.y - start.y) as f64];
                let mut s = v.flat.clone().unwrap_or_default();
                let current = s.note(bend).copied();
                s.set_note(fv::place_note(v, flat, bend, drop, current));
                dui.flat_preview = Some((vid, s));
            }
            PointerAction::Release(PointerButton::Primary) => {
                if drag.0.take().is_some_and(|x| x.4)
                    && let Some((vid, s)) = dui.flat_preview.take()
                {
                    commands.queue(move |w: &mut World| {
                        super::view_menu::set_view(w, vid, "Move bend note", |v| v.flat = Some(s));
                    });
                }
                dui.flat_preview = None;
            }
            PointerAction::Cancel => {
                drag.0 = None;
                dui.flat_preview = None;
            }
            _ => {}
        }
    }
    // Over a note: the view under it isn't highlighted (it's the note's).
    if drag.0.is_some() && !dui.annotation_hover {
        dui.annotation_hover = true;
    }
}

// ---------------------------------------------------------------------------------------------
// Scenario set-ups

/// `flat-drawing <what>` scenario commands (P3I.7):
/// - `box`: the E3 Sheet Metal Box stand-in in the active Part Studio: a 200 × 125 × 150 block
///   converted with its bottom edges bent and its top left open (thickness 1.5 mm, bend radius
///   1.5 mm): one part, renamed "Sheet Metal Box".
/// - `forms`: a 120 × 80 plate (Thicken, 1.5 mm) with two louvers (Form) and a Ø6.6 hole
///   counterbored Ø11 through it, renamed "Formed Plate" (SM16.3).
/// - `create <part name>`: Create drawing of flat pattern of the part (as the P3I.3 menu will).
pub fn script(world: &mut World, arg: &str) {
    let (cmd, rest) = arg.split_once(' ').unwrap_or((arg, ""));
    match cmd {
        "box" => sheet_metal_box(world),
        "forms" => formed_plate(world),
        "create" => {
            let name = rest.trim().to_string();
            let found = world.resource::<crate::parts::PartCache>().parts.iter().find(|p| p.name == name).map(|p| p.id);
            let element = world.get_resource::<ActiveDocument>().and_then(|d| d.active);
            match (found, element) {
                (Some(p), Some(e)) => open_create_drawing_of_flat(world, ObjectRef { element: e.0, part: Some((p.feature.0, p.index)) }),
                _ => warn!("flat-drawing create: no part {name:?}"),
            }
        }
        _ => warn!("flat-drawing: unknown {arg:?}"),
    }
}

fn sheet_metal_box(world: &mut World) {
    use cadrs_core::applied::EdgeOrFace;
    use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, RenamePart, SetExtrude};
    use cadrs_core::document::{EdgeRef, ExtrudeFeature, FaceRef, FeatureKind};
    use cadrs_core::sheetmetal::{SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};
    use cadrs_core::FeatureId;
    use cadrs_sketch::{PlaneRef, SketchOp, Vec2 as SVec2};
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(el) = doc.active else { return };
    let (w, dp, h) = (200.0, 125.0, 150.0);
    let s = FeatureId::new();
    let run = |doc: &mut ActiveDocument, c: &dyn cadrs_core::Command| {
        if let Err(e) = doc.execute(c) {
            warn!("flat-drawing box: {e}");
        }
    };
    run(&mut doc, &AddSketch { element: el, feature: s, plane: Some(PlaneRef::Top) });
    run(
        &mut doc,
        &EditSketch {
            element: el,
            feature: s,
            op: SketchOp::AddPolyline {
                points: vec![SVec2::new(0.0, 0.0), SVec2::new(w, 0.0), SVec2::new(w, dp), SVec2::new(0.0, dp)],
                closed: true,
                construction: false,
                label: "Add rectangle",
            },
        },
    );
    let e = FeatureId::new();
    run(&mut doc, &AddExtrude { element: el, feature: e, extrude: ExtrudeFeature::default() });
    run(
        &mut doc,
        &SetExtrude {
            element: el,
            feature: e,
            extrude: ExtrudeFeature { sketches: vec![s], depth: h, depth_expr: format!("{h} mm"), ..Default::default() },
            label: "Extrude".into(),
        },
    );
    let Some(features) = doc.doc.element(el).map(|e| e.features().to_vec()) else { return };
    let build = cadrs_core::rebuild::build(&features);
    let Some(block) = build.parts.first().cloned() else {
        warn!("flat-drawing box: no block");
        return;
    };
    let edge = |p: [f64; 3]| {
        let e = block.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))?;
        Some(EdgeOrFace::Edge(EdgeRef { part: block.id, edge: e.name, seed: p }))
    };
    let top = {
        let sd = &block.solid;
        let d = |c: [f64; 3]| (c[0] - w / 2.0).powi(2) + (c[1] - dp / 2.0).powi(2) + (c[2] - h).powi(2);
        sd.faces.iter().filter(|f| f.center.is_some()).min_by(|a, b| d(a.center.unwrap()).total_cmp(&d(b.center.unwrap()))).map(|f| FaceRef { part: block.id, face: f.name, seed: f.center.unwrap() })
    };
    let mut p = SheetMetalModelFeature::default_params();
    p.thickness = 1.5;
    p.bend_radius = 1.5;
    let x = SheetMetalModelFeature {
        operation: SheetMetalOp::Convert,
        parts: vec![block.id],
        exclude: top.into_iter().collect(),
        bends: [[w / 2.0, 0.0, 0.0], [w, dp / 2.0, 0.0], [w / 2.0, dp, 0.0], [0.0, dp / 2.0, 0.0]].iter().filter_map(|q| edge(*q)).collect(),
        params: p,
        exprs: SheetMetalExprs::of(&p),
        ..Default::default()
    };
    run(&mut doc, &AddFeature { element: el, feature: FeatureId::new(), base_name: "Sheet metal model".into(), kind: FeatureKind::SheetMetalModel(x) });
    let Some(features) = doc.doc.element(el).map(|e| e.features().to_vec()) else { return };
    let build = cadrs_core::rebuild::build(&features);
    if !build.errors.is_empty() {
        warn!("flat-drawing box: {:?}", build.errors);
    }
    if let Some(part) = cadrs_core::flat_drawing::flat_parts(&build.sheet_metal).first().copied() {
        run(&mut doc, &RenamePart { element: el, part, name: "Sheet Metal Box".into() });
    }
}

fn formed_plate(world: &mut World) {
    use cadrs_core::FeatureId;
    use cadrs_core::applied::{HoleFeature, HolePoint};
    use cadrs_core::commands::{AddFeature, AddSketch, EditSketch, RenamePart};
    use cadrs_core::document::{FaceRef, FeatureKind};
    use cadrs_core::hole::{HoleEnd, HoleSpec, HoleStart, HoleStyle, Length};
    use cadrs_core::sheetmetal::{SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};
    use cadrs_core::sheetmetal_form::{FormFeature, FormLocation, FormPick, FormSource, LIBRARY_NAME, LibraryForm};
    use cadrs_sketch::{PlaneRef, SketchOp, Vec2 as SVec2};
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(el) = doc.active else { return };
    let run = |doc: &mut ActiveDocument, c: &dyn cadrs_core::Command| {
        if let Err(e) = doc.execute(c) {
            warn!("flat-drawing forms: {e}");
        }
    };
    let sketch = |doc: &mut ActiveDocument, ops: Vec<SketchOp>| {
        let f = FeatureId::new();
        run(doc, &AddSketch { element: el, feature: f, plane: Some(PlaneRef::Top) });
        run(doc, &EditSketch { element: el, feature: f, op: SketchOp::Batch(ops) });
        f
    };
    let rect = vec![SVec2::new(0.0, 0.0), SVec2::new(120.0, 0.0), SVec2::new(120.0, 80.0), SVec2::new(0.0, 80.0)];
    let s = sketch(&mut doc, vec![SketchOp::AddPolyline { points: rect, closed: true, construction: false, label: "Add rectangle" }]);
    let Some(g) = doc.doc.element(el).and_then(|e| e.feature(s)).and_then(|f| f.sketch()).map(|k| k.geometry.clone()) else { return };
    let regions = cadrs_core::samples::region_refs(s, &g, &[SVec2::new(60.0, 40.0)]);
    let mut p = SheetMetalModelFeature::default_params();
    p.thickness = 1.5;
    p.bend_radius = 2.0;
    let model = FeatureId::new();
    let sm = SheetMetalModelFeature { operation: SheetMetalOp::Thicken, regions, params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
    run(&mut doc, &AddFeature { element: el, feature: model, base_name: "Sheet metal model".into(), kind: FeatureKind::SheetMetalModel(sm) });
    let Some(features) = doc.doc.element(el).map(|e| e.features().to_vec()) else { return };
    let build = cadrs_core::rebuild::build(&features);
    let Some(part) = build.parts.iter().find(|q| q.id.feature == model).cloned() else {
        warn!("flat-drawing forms: no plate");
        return;
    };
    let Some(top) = part.solid.faces.iter().find(|f| f.center.is_some_and(|c| (c[2] - 1.5).abs() < 1e-6)).map(|f| FaceRef { part: part.id, face: f.name, seed: f.center.unwrap_or_default() }) else { return };
    let pts = sketch(&mut doc, [(30.0, 25.0), (30.0, 55.0)].iter().map(|(x, y)| SketchOp::AddPoint { pos: SVec2::new(*x, *y) }).collect());
    let form = FormFeature {
        form: Some(FormPick { source: FormSource::Library(LibraryForm::Louver), name: "Louver".into(), document_name: LIBRARY_NAME.into(), studio: vec![] }),
        variables: LibraryForm::Louver.variables(),
        locations: vec![FormLocation::SketchPoints(pts)],
        targets: vec![top],
        flip: false,
    };
    run(&mut doc, &AddFeature { element: el, feature: FeatureId::new(), base_name: "Form".into(), kind: FeatureKind::Form(form) });
    let hp = sketch(&mut doc, vec![SketchOp::AddPoint { pos: SVec2::new(90.0, 40.0) }]);
    let Some(point) = doc.doc.element(el).and_then(|e| e.feature(hp)).and_then(|f| f.sketch()).and_then(|k| k.geometry.points.keys().next()) else { return };
    let spec = HoleSpec {
        style: HoleStyle::Counterbore,
        diameter: Length::mm(6.6),
        cbore_diameter: Length::mm(11.0),
        cbore_depth: Length::mm(0.5),
        end: HoleEnd::ThroughAll,
        start: HoleStart::Part,
        ..HoleSpec::default()
    };
    let hole = HoleFeature { points: vec![HolePoint { sketch: hp, point }], spec, ..HoleFeature::default() };
    run(&mut doc, &AddFeature { element: el, feature: FeatureId::new(), base_name: "Hole".into(), kind: FeatureKind::Hole(hole) });
    run(&mut doc, &RenamePart { element: el, part: part.id, name: "Formed Plate".into() });
}
