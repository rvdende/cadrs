//! The view context menu and the view dialogs (P3C.2, D2.9, D4.8–D4.11, D7.1, D7.3–D7.5;
//! `lesson-view-context-menu.png`).
//!
//! - **Context menu** (right-click on a view): Show/Hide hidden lines, Tangent edges ▸ (Hidden,
//!   Solid, Phantom), Show/Hide shaded view, Show threads (P3C.8), Show part intersections,
//!   Display state ▸ (assemblies); Create projected view, View properties…, Bring to front, Send
//!   to back; Insert BOM (P3C.5); Switch to <referenced tab>, Move to sheet…, Align view
//!   vertical, Align view horizontal, Suppress alignment with parent, Show/hide sketches…;
//!   Clear selection, Zoom to fit, Delete. Every change is one undoable drawing edit.
//! - **View properties** (D2.9): the reference (with a link to its tab), the scale (D4.7) and
//!   the sheet (D7.3).
//! - **Move to sheet…** (D7.3): a sheet dropdown; the view goes with its annotations, its
//!   children stay.
//! - **Show/hide sketches…** (D7.4): the referenced studio's sketches as a searchable checklist.

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::ElementId;
use cadrs_drawing::style::TangentEdges;
use cadrs_drawing::{DrawingOp, View, ViewId};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, CheckboxChange, CheckboxState, DialogClose, MenuEntry, Select, SelectState, form_row};

use super::view_tools::{ViewTool, edit_drawing, scales, sheet_center};
use super::views::{ViewCache, sheet_bounds, studio, view_at};
use super::{DrawingUi, active_drawing};
use crate::{ActiveDocument, AppState};

pub struct ViewMenuPlugin;

impl Plugin for ViewMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_view_menu_action)
            .add_observer(on_sketch_check)
            .add_observer(on_scale_change)
            .add_systems(Update, filter_sketch_list.run_if(in_state(AppState::Document)));
    }
}

/// The anchor of a view's context menu.
#[derive(Component, Clone, Copy)]
struct ViewMenuFor(ViewId);

/// The active drawing's view `id` (with its sheet index).
fn find_view(world: &World, id: ViewId) -> Option<(usize, View)> {
    let doc = world.get_resource::<ActiveDocument>()?;
    let (_, d) = active_drawing(doc)?;
    d.view(id).map(|(i, v)| (i, v.clone()))
}

/// Right-click on the sheet: an annotation's menu over an annotation, the view menu over a view,
/// else the sheet's menu.
pub fn open_space_menu(world: &mut World, pos: Vec2) {
    // An annotation's menu first (P3C.3).
    if super::annotations::open_annotation_menu(world, pos) {
        return;
    }
    // A note's or table's (P3C.4).
    if super::notes::open_item_menu(world, pos) {
        return;
    }
    let rect = *world.resource::<crate::viewport::ViewportRect>();
    let hit = (|| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let ui = world.resource::<DrawingUi>();
        let ((element, _), view) = super::current_view(doc, ui)?;
        let (_, d) = active_drawing(doc)?;
        let area = super::sheet_area(&rect, ui);
        let p = super::screen_to_sheet(view, area, pos);
        let pad = 6.0 / view.ppm as f64;
        view_at(d, ui.sheet_index(element, d), world.resource::<ViewCache>(), p, pad)
    })();
    match hit {
        Some(v) => {
            // Highlighted (not selected) while its menu is open.
            world.resource_mut::<DrawingUi>().hovered = Some(v);
            open_view_menu(world, pos, v);
        }
        None => super::sheet_dialog::open_sheet_menu(world, pos),
    }
}

/// The view's context menu.
pub fn open_view_menu(world: &mut World, pos: Vec2, id: ViewId) {
    let Some((_, v)) = find_view(world, id) else {
        return;
    };
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let reference = doc
        .doc
        .element(ElementId(v.reference.element))
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "the referenced tab".into());
    // Insert BOM is for a view of an assembly (P3C.5, D11.1).
    let is_assembly = cadrs_core::drawing_assembly::is_assembly(&doc.doc, ElementId(v.reference.element));
    let theme = world.resource::<Theme>().clone();
    let tangent = |t: TangentEdges, name: &'static str, label: &'static str| {
        let item = MenuItem::new(name, label);
        MenuEntry::Item(if v.tangent_edges == t { item.icon("check") } else { item })
    };
    let menu = Menu::new("view-context-menu")
        .min_width(230.0)
        .item(MenuItem::new(
            "view-menu-hidden-lines",
            if v.hidden_lines { "Hide hidden lines" } else { "Show hidden lines" },
        ))
        .item(MenuItem::new("view-menu-tangent-edges", "Tangent edges").submenu(vec![
            tangent(TangentEdges::Hidden, "view-menu-tangent-hidden", "Hidden"),
            tangent(TangentEdges::Solid, "view-menu-tangent-solid", "Solid"),
            tangent(TangentEdges::Phantom, "view-menu-tangent-phantom", "Phantom"),
        ]))
        .item(MenuItem::new(
            "view-menu-shaded",
            if v.shaded { "Hide shaded view" } else { "Show shaded view" },
        ))
        .item(MenuItem::new("view-menu-threads", if v.threads { "Hide threads" } else { "Show threads" }))
        .item(MenuItem::new(
            "view-menu-part-intersections",
            if v.part_intersections { "Hide part intersections" } else { "Show part intersections" },
        ))
        .item(
            MenuItem::new("view-menu-display-state", "Display state")
                .disabled(true)
                .submenu(vec![]),
        )
        .separator()
        .item(MenuItem::new("view-menu-projected", "Create projected view"))
        .item(MenuItem::new("view-menu-properties", "View properties…"))
        .item(MenuItem::new("view-menu-front", "Bring to front").disabled(true))
        .item(MenuItem::new("view-menu-back", "Send to back").disabled(true))
        .separator()
        .item(MenuItem::new("view-menu-bom", format!("Insert BOM for {reference}…")).disabled(!is_assembly))
        .separator()
        .item(MenuItem::new("view-menu-switch", format!("Switch to {reference}")))
        .item(MenuItem::new("view-menu-move-to-sheet", "Move to sheet…"))
        .item(MenuItem::new("view-menu-align-vertical", "Align view vertical"))
        .item(MenuItem::new("view-menu-align-horizontal", "Align view horizontal"))
        .item(
            MenuItem::new(
                "view-menu-suppress-alignment",
                if v.align_suppressed { "Restore alignment with parent" } else { "Suppress alignment with parent" },
            )
            .disabled(v.fold.is_none() || v.parent.is_none()),
        )
        .item(MenuItem::new("view-menu-sketches", "Show/hide sketches…"));
    // Undo a crop, break or broken-out section (P3C.8).
    let mut menu = menu;
    let removals = [
        (v.crop.is_some(), "view-menu-remove-crop", "Remove crop"),
        (!v.breaks.is_empty(), "view-menu-remove-breaks", "Remove breaks"),
        (v.broken_out.is_some(), "view-menu-remove-broken-out", "Remove broken-out section"),
    ];
    let extra = removals.iter().filter(|r| r.0).count();
    for (on, name, label) in removals {
        if on {
            menu = menu.item(MenuItem::new(name, label));
        }
    }
    let menu = menu
        .separator()
        .item(MenuItem::new("view-menu-clear-selection", "Clear selection"))
        .item(MenuItem::new("view-menu-zoom", "Zoom to fit"))
        .item(MenuItem::new("view-menu-delete", "Delete"));
    // The menu is tall (20 items): open it where it fits below the anchor, so it is never cut
    // off at the window's edge.
    let window_h = world.resource::<cadrs_ui::RenderSurface>().size.y as f32;
    let menu_h = (20 + extra) as f32 * 27.0 + 4.0 * 9.0 + 12.0;
    let pos = Vec2::new(pos.x, pos.y.min((window_h - menu_h - 6.0).max(0.0)));
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, pos, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((ViewMenuFor(id), DespawnOnExit(AppState::Document)));
    world.flush();
}

/// Changes one view with `f`, as an undoable edit labelled `label`.
pub fn set_view(world: &mut World, id: ViewId, label: &str, f: impl FnOnce(&mut View)) {
    let Some((_, mut v)) = find_view(world, id) else {
        return;
    };
    f(&mut v);
    edit_drawing(world, DrawingOp::SetView { view: v, label: label.into() });
}

fn on_view_menu_action(ev: On<MenuAction>, q: Query<&ViewMenuFor>, mut commands: Commands) {
    let Ok(target) = q.get(ev.entity) else {
        return;
    };
    let id = target.0;
    let item = ev.item.clone();
    commands.queue(move |w: &mut World| view_menu_action(w, id, &item));
}

fn view_menu_action(w: &mut World, id: ViewId, item: &str) {
    let Some((_, v)) = find_view(w, id) else {
        return;
    };
    match item {
        "view-menu-hidden-lines" => {
            let on = !v.hidden_lines;
            set_view(w, id, if on { "Show hidden lines" } else { "Hide hidden lines" }, |v| v.hidden_lines = on);
        }
        "view-menu-tangent-hidden" => set_view(w, id, "Tangent edges: hidden", |v| v.tangent_edges = TangentEdges::Hidden),
        "view-menu-tangent-solid" => set_view(w, id, "Tangent edges: solid", |v| v.tangent_edges = TangentEdges::Solid),
        "view-menu-tangent-phantom" => {
            set_view(w, id, "Tangent edges: phantom", |v| v.tangent_edges = TangentEdges::Phantom)
        }
        "view-menu-shaded" => {
            let on = !v.shaded;
            set_view(w, id, if on { "Show shaded view" } else { "Hide shaded view" }, |v| v.shaded = on);
        }
        "view-menu-threads" => {
            let on = !v.threads;
            set_view(w, id, if on { "Show threads" } else { "Hide threads" }, |v| v.threads = on);
        }
        "view-menu-remove-crop" => set_view(w, id, "Remove crop", |v| v.crop = None),
        "view-menu-remove-breaks" => set_view(w, id, "Remove breaks", |v| v.breaks.clear()),
        "view-menu-remove-broken-out" => set_view(w, id, "Remove broken-out section", |v| v.broken_out = None),
        "view-menu-part-intersections" => {
            let on = !v.part_intersections;
            set_view(w, id, "Show part intersections", |v| v.part_intersections = on);
        }
        "view-menu-projected" => {
            let mut ui = w.resource_mut::<DrawingUi>();
            ui.tool = ViewTool::Projected { parent: Some(id) };
            ui.selected = vec![id];
        }
        "view-menu-properties" => open_view_properties(w, id),
        "view-menu-bom" => {
            super::bom_tools::open_bom_tool(w);
            let asm = ElementId(v.reference.element);
            if w.resource::<super::bom_tools::BomUi>().assembly != Some(asm) {
                w.resource_mut::<super::bom_tools::BomUi>().assembly = Some(asm);
                super::bom_tools::refresh(w);
            }
        }
        "view-menu-switch" => {
            if let Some(mut doc) = w.get_resource_mut::<ActiveDocument>() {
                doc.set_active(ElementId(v.reference.element));
            }
        }
        "view-menu-move-to-sheet" => open_move_to_sheet(w, id),
        "view-menu-align-vertical" | "view-menu-align-horizontal" => {
            let mut ui = w.resource_mut::<DrawingUi>();
            ui.tool = ViewTool::Align {
                view: id,
                vertical: item == "view-menu-align-vertical",
            };
            // The view is hovered (light orange) while the edge is picked, not selected.
            ui.selected.clear();
        }
        "view-menu-suppress-alignment" => {
            let on = !v.align_suppressed;
            set_view(
                w,
                id,
                if on { "Suppress alignment with parent" } else { "Restore alignment with parent" },
                |v| v.align_suppressed = on,
            );
            // Restoring alignment puts the view back on its parent's fold line.
            if !on
                && let Some(doc) = w.get_resource::<ActiveDocument>()
                && let Some((_, d)) = active_drawing(doc)
                && let Some((sheet, nv)) = d.view(id)
                && let Some(p) = d.aligned_parent(nv, sheet)
                && let Some(n) = nv.fold_on_sheet(p.rotation)
            {
                let a = cadrs_drawing::view::aligned_anchor(p.anchor, n, nv.anchor, None);
                edit_drawing(w, DrawingOp::MoveViews { moves: vec![(id, a)] });
            }
        }
        "view-menu-sketches" => open_show_sketches(w, id),
        "view-menu-clear-selection" => w.resource_mut::<DrawingUi>().selected.clear(),
        "view-menu-zoom" => zoom_to_view(w, id),
        "view-menu-delete" => {
            edit_drawing(w, DrawingOp::DeleteViews { ids: vec![id] });
            w.resource_mut::<DrawingUi>().selected.retain(|s| *s != id);
        }
        _ => {}
    }
}

/// Zoom to fit: the sheet view framing the view with a margin.
fn zoom_to_view(w: &mut World, id: ViewId) {
    let Some((_, v)) = find_view(w, id) else {
        return;
    };
    let g = w.resource::<ViewCache>().geometry(&v);
    let (lo, hi) = sheet_bounds(&v, g.as_deref());
    let rect = *w.resource::<crate::viewport::ViewportRect>();
    let Some(key) = w.get_resource::<ActiveDocument>().and_then(|doc| {
        let ui = w.resource::<DrawingUi>();
        super::current_view(doc, ui).map(|(k, _)| k)
    }) else {
        return;
    };
    let mut ui = w.resource_mut::<DrawingUi>();
    let area = super::sheet_area(&rect, &ui);
    let (bw, bh) = ((hi[0] - lo[0]).max(1.0) as f32, (hi[1] - lo[1]).max(1.0) as f32);
    let margin = 60.0;
    let ppm = ((area.width() - 2.0 * margin).max(20.0) / bw).min((area.height() - 2.0 * margin).max(20.0) / bh);
    ui.views.insert(
        key,
        super::SheetView {
            center: Vec2::new(((lo[0] + hi[0]) / 2.0) as f32, ((lo[1] + hi[1]) / 2.0) as f32),
            ppm: ppm.clamp(0.05, 400.0),
            fitted: false,
        },
    );
}

// ---------------------------------------------------------------------------------------------
// View properties

#[derive(Component, Clone)]
struct ViewPropsDialog {
    view: ViewId,
    scales: Vec<cadrs_drawing::Scale>,
    sheets: Vec<cadrs_drawing::SheetId>,
}

/// View properties… (D2.9, D4.7, D7.3).
pub fn open_view_properties(world: &mut World, id: ViewId) {
    let Some((sheet_index, v)) = find_view(world, id) else {
        return;
    };
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let Some((_, d)) = active_drawing(doc) else {
        return;
    };
    let reference = super::reference_props(&doc.doc, Some(v.reference)).name.unwrap_or_default();
    let studio_name = doc
        .doc
        .element(ElementId(v.reference.element))
        .map(|e| e.name.clone())
        .unwrap_or_default();
    let sheets: Vec<(cadrs_drawing::SheetId, String)> = d.sheets.iter().map(|s| (s.id, s.name.clone())).collect();
    let scale = d.effective_scale(id).unwrap_or(v.scale);
    let list = scales(scale);
    let scale_selected = list.iter().position(|s| *s == scale).unwrap_or(0);
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let dialog = ViewPropsDialog {
        view: id,
        scales: list.clone(),
        sheets: sheets.iter().map(|s| s.0).collect(),
    };
    let inherited = v.scale_inherited && v.parent.is_some();
    let name = v.name.clone();
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("view-properties-dialog")
            .title(format!("View properties: {name}"))
            .width(400.0)
            .body(move |b| {
                let t = &tb;
                let lw = 110.0;
                b.spawn(form_row(t, "view-props-reference-row", "Reference", lw))
                    .with_children(|r| {
                        r.spawn((
                            t.text(reference.clone(), t.font_base, FontWeight::MEDIUM, t.foreground),
                            Node {
                                flex_grow: 1.0,
                                ..default()
                            },
                        ));
                        r.spawn((
                            Button::new("view-props-reference-link")
                                .icon("open-external")
                                .icon_size(14.0)
                                .ghost()
                                .tooltip(format!("Switch to {studio_name}"))
                                .build(t),
                            observe(|_: On<Activate>, q: Query<(Entity, &ViewPropsDialog)>, mut commands: Commands| {
                                if let Some((e, d)) = q.iter().next() {
                                    let id = d.view;
                                    commands.trigger(DialogClose { entity: e });
                                    commands.queue(move |w: &mut World| {
                                        if let Some((_, v)) = find_view(w, id)
                                            && let Some(mut doc) = w.get_resource_mut::<ActiveDocument>()
                                        {
                                            doc.set_active(ElementId(v.reference.element));
                                        }
                                    });
                                }
                            }),
                        ));
                    });
                let mut sc = Select::new("view-props-scale").width(Val::Px(200.0));
                for s in &list {
                    sc = sc.option(s.label(), true);
                }
                b.spawn(form_row(t, "view-props-scale-row", "Scale", lw))
                    .with_child(sc.selected(scale_selected).build(t));
                if inherited {
                    b.spawn((
                        Name::new("view-props-inherited-hint"),
                        t.text("Inherited from the parent view", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                        Node {
                            margin: UiRect::left(Val::Px(lw)),
                            ..default()
                        },
                    ));
                }
                let mut sh = Select::new("view-props-sheet").width(Val::Px(200.0));
                for (_, n) in &sheets {
                    sh = sh.option(n.clone(), true);
                }
                b.spawn(form_row(t, "view-props-sheet-row", "Sheet", lw))
                    .with_child(sh.selected(sheet_index).build(t));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("view-props-ok").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(apply_view_properties);
                    }),
                ));
                f.spawn((
                    Button::new("view-props-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, q: Query<Entity, With<ViewPropsDialog>>, mut commands: Commands| {
                        for e in &q {
                            commands.trigger(DialogClose { entity: e });
                        }
                    }),
                ));
            })
            .build(&theme),
        dialog,
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

/// A new scale is the view's own: the "Inherited from the parent view" hint goes.
fn on_scale_change(ev: On<cadrs_ui::SelectChange>, q: Query<&Name>, mut q_hint: Query<(&Name, &mut Node)>) {
    if !q.get(ev.entity).is_ok_and(|n| n.as_str() == "view-props-scale") {
        return;
    }
    for (name, mut node) in &mut q_hint {
        if name.as_str() == "view-props-inherited-hint" && node.display != Display::None {
            node.display = Display::None;
        }
    }
}

fn select_value(world: &mut World, name: &str) -> Option<usize> {
    let mut q = world.query::<(&Name, &SelectState)>();
    q.iter(world).find(|(n, _)| n.as_str() == name).map(|(_, s)| s.selected)
}

fn apply_view_properties(world: &mut World) {
    let mut q = world.query::<(Entity, &ViewPropsDialog)>();
    let Some((entity, dialog)) = q.iter(world).next().map(|(e, d)| (e, d.clone())) else {
        return;
    };
    let scale = select_value(world, "view-props-scale").and_then(|i| dialog.scales.get(i).copied());
    let sheet = select_value(world, "view-props-sheet").and_then(|i| dialog.sheets.get(i).copied());
    if let (Some(s), Some(doc)) = (scale, world.get_resource::<ActiveDocument>())
        && let Some((_, d)) = active_drawing(doc)
        && d.effective_scale(dialog.view) != Some(s)
    {
        set_view(world, dialog.view, "Edit view properties", |v| {
            v.scale = s;
            v.scale_inherited = false;
        });
    }
    if let (Some(sheet), Some((from, _))) = (sheet, find_view(world, dialog.view)) {
        let current = world
            .get_resource::<ActiveDocument>()
            .and_then(|doc| active_drawing(doc).map(|(_, d)| d.sheets[from].id));
        if current != Some(sheet) {
            edit_drawing(world, DrawingOp::MoveViewToSheet { id: dialog.view, sheet });
        }
    }
    world.trigger(DialogClose { entity });
}

// ---------------------------------------------------------------------------------------------
// Move to sheet

#[derive(Component, Clone)]
struct MoveToSheetDialog {
    view: ViewId,
    sheets: Vec<cadrs_drawing::SheetId>,
}

/// Move to sheet… (D7.3).
pub fn open_move_to_sheet(world: &mut World, id: ViewId) {
    let Some((sheet_index, v)) = find_view(world, id) else {
        return;
    };
    let Some((_, d)) = world.get_resource::<ActiveDocument>().and_then(|doc| active_drawing(doc)) else {
        return;
    };
    let sheets: Vec<(cadrs_drawing::SheetId, String)> = d.sheets.iter().map(|s| (s.id, s.name.clone())).collect();
    // Another sheet by default.
    let selected = if sheets.len() > 1 { (sheet_index + 1) % sheets.len() } else { 0 };
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let dialog = MoveToSheetDialog {
        view: id,
        sheets: sheets.iter().map(|s| s.0).collect(),
    };
    let name = v.name.clone();
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("move-to-sheet-dialog")
            .title(format!("Move {name} view to sheet"))
            .width(360.0)
            .body(move |b| {
                let t = &tb;
                let mut sh = Select::new("move-to-sheet-select").width(Val::Px(200.0));
                for (_, n) in &sheets {
                    sh = sh.option(n.clone(), true);
                }
                b.spawn(form_row(t, "move-to-sheet-row", "Sheet", 80.0))
                    .with_child(sh.selected(selected).build(t));
                b.spawn((
                    t.text(
                        "The view's annotations move with it; its projected views stay where they are.",
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Node {
                        max_width: Val::Px(320.0),
                        ..default()
                    },
                )).insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("move-to-sheet-ok").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(apply_move_to_sheet);
                    }),
                ));
                f.spawn((
                    Button::new("move-to-sheet-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, q: Query<Entity, With<MoveToSheetDialog>>, mut commands: Commands| {
                        for e in &q {
                            commands.trigger(DialogClose { entity: e });
                        }
                    }),
                ));
            })
            .build(&theme),
        dialog,
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

fn apply_move_to_sheet(world: &mut World) {
    let mut q = world.query::<(Entity, &MoveToSheetDialog)>();
    let Some((entity, dialog)) = q.iter(world).next().map(|(e, d)| (e, d.clone())) else {
        return;
    };
    if let Some(sheet) = select_value(world, "move-to-sheet-select").and_then(|i| dialog.sheets.get(i).copied()) {
        edit_drawing(world, DrawingOp::MoveViewToSheet { id: dialog.view, sheet });
        world.resource_mut::<DrawingUi>().selected.retain(|s| *s != dialog.view);
    }
    world.trigger(DialogClose { entity });
}

// ---------------------------------------------------------------------------------------------
// Show/hide sketches

#[derive(Component, Clone)]
struct SketchesDialog {
    view: ViewId,
    checked: Vec<uuid::Uuid>,
}

/// One sketch's row, with its name for the search.
#[derive(Component, Clone)]
struct SketchRow(uuid::Uuid, String);

/// Show/hide sketches… (D7.4).
pub fn open_show_sketches(world: &mut World, id: ViewId) {
    let Some((_, v)) = find_view(world, id) else {
        return;
    };
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let sketches: Vec<(uuid::Uuid, String)> = studio(&doc.doc, &v.reference)
        .map(|s| {
            s.features
                .iter()
                .filter(|f| f.sketch().is_some())
                .map(|f| (f.id.0, f.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let dialog = SketchesDialog {
        view: id,
        checked: v.sketches.clone(),
    };
    let shown = v.sketches.clone();
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("show-sketches-dialog")
            .title("Show/hide sketches")
            .width(320.0)
            .body(move |b| {
                let t = &tb;
                b.spawn(
                    TextInput::new("show-sketches-search")
                        .placeholder("Search sketches")
                        .width(Val::Percent(100.0))
                        .build(t),
                );
                b.spawn((
                    Name::new("show-sketches-list"),
                    Node {
                        flex_direction: FlexDirection::Column,
                        margin: UiRect::top(Val::Px(6.0)),
                        max_height: Val::Px(240.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|l| {
                    if sketches.is_empty() {
                        l.spawn(t.text("The referenced Part Studio has no sketches", t.font_sm, FontWeight::NORMAL, t.muted_foreground));
                    }
                    for (i, (sid, name)) in sketches.iter().enumerate() {
                        l.spawn((
                            Checkbox::new(format!("show-sketch-{}", i + 1))
                                .label(name.clone())
                                .checked(shown.contains(sid))
                                .build(t),
                            SketchRow(*sid, name.clone()),
                        ));
                    }
                });
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("show-sketches-ok").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(apply_show_sketches);
                    }),
                ));
                f.spawn((
                    Button::new("show-sketches-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, q: Query<Entity, With<SketchesDialog>>, mut commands: Commands| {
                        for e in &q {
                            commands.trigger(DialogClose { entity: e });
                        }
                    }),
                ));
            })
            .build(&theme),
        dialog,
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

fn on_sketch_check(ev: On<CheckboxChange>, q: Query<&SketchRow>, mut q_dialog: Query<&mut SketchesDialog>) {
    let Ok(row) = q.get(ev.entity) else {
        return;
    };
    for mut d in &mut q_dialog {
        d.checked.retain(|s| *s != row.0);
        if ev.checked {
            d.checked.push(row.0);
        }
    }
}

/// The search hides the rows whose sketch name doesn't match.
fn filter_sketch_list(
    q_search: Query<(&Name, &bevy::text::EditableText)>,
    mut q_rows: Query<(&SketchRow, &mut Node)>,
) {
    if q_rows.is_empty() {
        return;
    }
    let text = q_search
        .iter()
        .find(|(n, _)| n.as_str() == "show-sketches-search-field")
        .map(|(_, t)| t.value().to_string().to_lowercase())
        .unwrap_or_default();
    for (row, mut node) in &mut q_rows {
        let d = if text.trim().is_empty() || row.1.to_lowercase().contains(text.trim()) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != d {
            node.display = d;
        }
    }
}

fn apply_show_sketches(world: &mut World) {
    let mut q = world.query::<(Entity, &SketchesDialog)>();
    let Some((entity, dialog)) = q.iter(world).next().map(|(e, d)| (e, d.clone())) else {
        return;
    };
    // Keep the studio's order.
    let mut qc = world.query::<(&SketchRow, &CheckboxState)>();
    let checked: Vec<uuid::Uuid> = qc.iter(world).filter(|(_, c)| c.checked).map(|(r, _)| r.0).collect();
    let checked = if checked.is_empty() && !dialog.checked.is_empty() && qc.iter(world).next().is_none() {
        dialog.checked.clone()
    } else {
        checked
    };
    if find_view(world, dialog.view).is_some_and(|(_, v)| v.sketches != checked) {
        set_view(world, dialog.view, "Show/hide sketches", |v| v.sketches = checked);
    }
    world.trigger(DialogClose { entity });
}

/// The centre of a view (for the menu's zoom and tests).
pub fn view_center(world: &World, id: ViewId) -> Option<[f64; 2]> {
    let (_, v) = find_view(world, id)?;
    Some(sheet_center(world.resource::<ViewCache>(), &v))
}
