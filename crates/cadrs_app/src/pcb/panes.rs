//! The PCB Studio's right panes (PCB3.8), docked beside the viewport by the right edge's
//! toggles, one at a time:
//!
//! - **Component** (PCB4.6, PCB11.5–PCB11.7, `v8-component-properties-poster.png`): headed
//!   "Component"; read-only **Part name** (the package) and **Part number**; **Representation**:
//!   None (not shown in PCB Studio), From ECAD data (the generic box; its open-link icon opens the
//!   component view), Custom part. It shows the component clicked in the view (the board view
//!   stays) or the package of the component view. Choosing Custom part opens Select custom part
//!   ([`super::dialogs`]); once a part is accepted, **Translate** X/Y/Z with **Center** and
//!   **Rotate** X/Y/Z move it on the footprint (a live preview in the view), and ✓ accepts the
//!   move (one undo step) or Cancel drops it. Every change is a
//!   [`cadrs_core::pcb::SetRepresentation`] on the library, so every PCB Studio using the package
//!   follows.
//! - **Bill of materials** (PCB3.9, PCB3.10, PCB4.7): the shown board's components grouped by
//!   part number (Qty, Designator as a list with ranges, Part name, Part number). Clicking a row
//!   selects its components in the view and a click in the view selects its row; the row under
//!   the pointer lights its components; search matches are lit too. **Double-click a
//!   designator** to rename it in place (Enter; one undo step; a designator already on the board
//!   is refused). A row of several components first opens into one row per component
//!   (double-click its designators again to close it); those rows are edited the same way.
//!
//! Names: `pcb-component-pane` (`pcb-part-name`, `pcb-part-number`, `pcb-representation` with
//! `pcb-rep-none`, `pcb-rep-ecad`, `pcb-rep-ecad-link`, `pcb-rep-custom`; `pcb-custom-select`,
//! `pcb-translate-x|y|z`, `pcb-custom-center`, `pcb-rotate-x|y|z`, `pcb-custom-accept`,
//! `pcb-custom-cancel`), `pcb-bom-pane` (rows `pcb-bom-row-<i>` and `pcb-bom-row-<i>-<j>`,
//! designator cells `pcb-bom-designator-<i>` and `pcb-bom-designator-<i>-<j>`).

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::ElementId;
use cadrs_core::pcb::bom::{BomRow, bom};
use cadrs_core::pcb::{BoardId, ItemId, PartTransform, Representation, SetRefdes, SetRepresentation};
use cadrs_sketch::units::{Quantity, format_with_unit};
use cadrs_ui::inline_edit::{DoubleClick, DoubleClickable, InlineEdit, InlineEditCommit, InlineEditLabel, InlineEditOptions, begin_inline_edit};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, Column, Visuals, Notification, show_notification, NumberField, NumberFieldCommit, NumberFieldState, RadioChange, RadioGroup, TableHeader, TableRoot, TableRow};

use super::{CustomEdit, PcbPane, PcbUi, PcbView};
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

/// The panes' width (px).
pub const PANE_W: f32 = 360.0;

pub fn register(app: &mut App) {
    app.init_resource::<PaneKey>()
        .add_systems(Update, (bom_hover, sync_bom_rows).run_if(in_state(AppState::Document)))
        .add_observer(on_representation)
        .add_observer(on_ecad_link)
        .add_observer(on_number_commit)
        .add_observer(on_bom_row)
        .add_observer(on_bom_double_click)
        .add_observer(on_bom_commit);
}

/// The docked pane.
#[derive(Component)]
struct PaneRoot;

/// What the pane was built from.
#[derive(Resource, Default)]
struct PaneKey(Option<String>);

/// Rebuilds the pane on its next check (a cancelled dialog puts the radio back).
pub fn refresh(world: &mut World) {
    world.resource_mut::<PaneKey>().0 = None;
}

/// What the Component pane shows: the tab and board, the package and its part number.
#[derive(Clone, Debug, PartialEq)]
struct Subject {
    element: ElementId,
    board: BoardId,
    package: String,
    part_number: String,
    representation: Representation,
}

fn subject(world: &World) -> Option<Subject> {
    let (el, b, board) = super::shown(world)?;
    let ui = world.resource::<PcbUi>();
    let package = match &ui.view {
        PcbView::Component { package, .. } => package.clone(),
        PcbView::Board => board.component(*ui.selected.first()?)?.package.clone(),
    };
    let part_number = board.board.placements.iter().find(|p| p.package == package).map(|p| p.part_number.clone()).unwrap_or_default();
    let doc = world.resource::<ActiveDocument>();
    let representation = doc.doc.element(el)?.pcb()?.library.get(&package).clone();
    Some(Subject { element: el, board: b, package, part_number, representation })
}

/// Rebuilds the docked pane when what it shows changed (selection and hover only restyle the
/// BOM's rows, so a double click's two clicks land on the same cell).
pub fn sync_panes(world: &mut World) {
    let pane = world.resource::<PcbUi>().pane;
    let pcb = *world.resource::<crate::viewport::ActiveKind>() == crate::viewport::ActiveKind::PcbStudio;
    let key = if !pcb {
        None
    } else {
        match pane {
            PcbPane::None => None,
            PcbPane::Component => {
                let s = subject(world);
                // The edit follows the package shown: the custom part's accepted placement.
                if let Some(s) = &s {
                    let want = s.representation.custom().map(|c| c.transform);
                    let mut ui = world.resource_mut::<PcbUi>();
                    let stale = ui.edit.as_ref().is_some_and(|e| e.element != s.element || e.package != s.package);
                    if stale || want.is_none() {
                        ui.edit = None;
                    }
                    if let (Some(t), None) = (want, &ui.edit) {
                        ui.edit = Some(CustomEdit { element: s.element, package: s.package.clone(), transform: t });
                    }
                }
                let edit = world.resource::<PcbUi>().edit.clone();
                Some(format!("component {s:?} {edit:?}"))
            }
            PcbPane::Bom => {
                let ui = world.resource::<PcbUi>();
                let rows = super::shown(world).map(|(e, b, board)| (e, b, bom(&board)));
                let mut exp: Vec<_> = ui.bom_expanded.iter().cloned().collect();
                exp.sort();
                Some(format!("bom {rows:?} {exp:?}"))
            }
        }
    };
    let have: Vec<Entity> = world.query_filtered::<Entity, With<PaneRoot>>().iter(world).collect();
    if world.resource::<PaneKey>().0 == key && have.is_empty() == key.is_none() {
        return;
    }
    world.resource_mut::<PaneKey>().0 = key.clone();
    for e in have {
        world.entity_mut(e).despawn();
    }
    if key.is_none() {
        return;
    }
    let mut q_area = world.query_filtered::<(Entity, &ChildOf), With<ViewportArea>>();
    let Some((area, parent)) = q_area.iter(world).next().map(|(e, c)| (e, c.parent())) else { return };
    let theme = world.resource::<Theme>().clone();
    let panel = match pane {
        PcbPane::Component => spawn_component_pane(world, &theme),
        PcbPane::Bom => spawn_bom_pane(world, &theme),
        PcbPane::None => return,
    };
    let at = world.get::<Children>(parent).and_then(|c| c.iter().position(|e| e == area)).map_or(0, |i| i + 1);
    world.entity_mut(parent).insert_children(at, &[panel]);
}

fn pane_frame(world: &mut World, t: &Theme, name: &str, title: &str, icon: &str) -> Entity {
    let mut commands = world.commands();
    let e = commands
        .spawn((
            Name::new(name.to_string()),
            PaneRoot,
            DespawnOnExit(AppState::Document),
            Node {
                width: Val::Px(PANE_W),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                border: UiRect::new(Val::Px(1.0), Val::Px(1.0), Val::ZERO, Val::ZERO),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(t.background),
            BorderColor::all(t.panel_border),
        ))
        .id();
    let (title, icon, t2) = (title.to_string(), icon.to_string(), t.clone());
    commands.entity(e).with_children(move |p| {
        p.spawn((
            Node {
                height: Val::Px(34.0),
                flex_shrink: 0.0,
                padding: UiRect::new(Val::Px(10.0), Val::Px(4.0), Val::ZERO, Val::ZERO),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(t2.panel_border),
        ))
        .with_children(|h| {
            h.spawn((icon_bundle(&icon, 16.0, t2.primary), Pickable::IGNORE));
            h.spawn((t2.text(title, 13.0, FontWeight::BOLD, t2.foreground), Node { flex_grow: 1.0, ..default() }));
            h.spawn((
                IconButton::new("pcb-pane-close", "close").tooltip("Close").build(&t2),
                observe(|_: On<Activate>, mut ui: ResMut<PcbUi>| ui.pane = PcbPane::None),
            ));
        });
    });
    world.flush();
    e
}

fn icon_bundle(name: &str, size: f32, color: Color) -> impl Bundle {
    cadrs_ui::icon(name.to_string(), size, color)
}

// ---------------------------------------------------------------------------------------------
// Component pane

fn caption(t: &Theme, text: &str) -> impl Bundle {
    (t.text(text.to_string(), t.font_sm, FontWeight::MEDIUM, t.muted_foreground), Node { margin: UiRect::top(Val::Px(8.0)), ..default() })
}

fn read_only(t: &Theme, name: &str, text: &str) -> impl Bundle {
    (
        Name::new(name.to_string()),
        Node {
            height: Val::Px(28.0),
            padding: UiRect::horizontal(Val::Px(8.0)),
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(t.radius)),
            overflow: Overflow::clip(),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(t.input_disabled_background),
        BorderColor::all(t.input_border),
        Tooltip::new(text.to_string()),
        Children::spawn_one((t.text(text.to_string(), t.font_base, FontWeight::NORMAL, t.foreground), Pickable::IGNORE)),
    )
}

fn spawn_component_pane(world: &mut World, t: &Theme) -> Entity {
    let pane = pane_frame(world, t, "pcb-component-pane", "Component", "properties");
    let s = subject(world);
    let edit = world.resource::<PcbUi>().edit.clone();
    let units = world.resource::<ActiveDocument>().doc.units;
    let _ = units;
    let t = t.clone();
    let mut commands = world.commands();
    commands.entity(pane).with_children(move |p| {
        p.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::new(Val::Px(12.0), Val::Px(12.0), Val::Px(4.0), Val::Px(12.0)), row_gap: Val::Px(4.0), overflow: Overflow::scroll_y(), flex_grow: 1.0, ..default() })
            .with_children(|b| {
                let Some(s) = s else {
                    b.spawn((
                        Name::new("pcb-component-hint"),
                        Text::new("Click a component in the view to see its properties, or open one under Components."),
                        t.font(t.font_sm, FontWeight::NORMAL),
                        TextColor(t.muted_foreground),
                        Node { margin: UiRect::top(Val::Px(10.0)), max_width: Val::Percent(100.0), ..default() },
                    ));
                    return;
                };
                b.spawn(caption(&t, "Part name"));
                b.spawn(read_only(&t, "pcb-part-name", &s.package));
                b.spawn(caption(&t, "Part number"));
                b.spawn(read_only(&t, "pcb-part-number", if s.part_number.is_empty() { "–" } else { &s.part_number }));
                b.spawn(caption(&t, "Representation"));
                let sel = match s.representation {
                    Representation::None => 0,
                    Representation::FromEcad => 1,
                    Representation::Custom(_) => 2,
                };
                b.spawn(
                    RadioGroup::new("pcb-representation")
                        .option("pcb-rep-none", "None")
                        .option("pcb-rep-ecad", "From ECAD data")
                        .option_icon("open-external", "pcb-rep-ecad-link", "Open in the component view")
                        .option("pcb-rep-custom", "Custom part")
                        .selected(Some(sel))
                        .build(&t),
                );
                if let Representation::Custom(c) = &s.representation {
                    custom_section(b, &t, &c.source.label(), edit.as_ref().map(|e| e.transform).unwrap_or(c.transform), c.transform);
                }
            });
    });
    world.flush();
    pane
}

fn custom_section(b: &mut ChildSpawnerCommands, t: &Theme, source: &str, now: PartTransform, accepted: PartTransform) {
    b.spawn((
        Name::new("pcb-custom-source"),
        Text::new(source.to_string()),
        t.font(t.font_sm, FontWeight::NORMAL),
        TextColor(t.foreground),
        Node { margin: UiRect::new(Val::Px(19.0), Val::ZERO, Val::Px(2.0), Val::Px(4.0)), max_width: Val::Percent(100.0), ..default() },
    ));
    b.spawn((
        Button::new("pcb-custom-select").label("Select custom part").small().build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(|w: &mut World| {
                if let Some(s) = subject(w) {
                    super::dialogs::open_custom_part_dialog(w, s.element, &s.package);
                }
            });
        }),
    ))
    .entry::<Node>()
    .and_modify(|mut n| {
        n.align_self = AlignSelf::FlexStart;
        n.margin = UiRect::left(Val::Px(19.0));
    });
    b.spawn(caption(t, "Translate"));
    for (i, axis) in ["x", "y", "z"].into_iter().enumerate() {
        b.spawn(NumberField::new(format!("pcb-translate-{axis}"), axis.to_uppercase()).label_width(24.0).text(format_with_unit(now.translate[i], Quantity::Length)).build(t));
    }
    b.spawn((
        Button::new("pcb-custom-center").icon("origin").label("Center").small().tooltip("Put the part in the middle of the footprint, standing on the board").build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(center_part);
        }),
    ))
    .entry::<Node>()
    .and_modify(|mut n| {
        n.align_self = AlignSelf::FlexStart;
        n.margin = UiRect::top(Val::Px(4.0));
    });
    b.spawn(caption(t, "Rotate"));
    for (i, axis) in ["x", "y", "z"].into_iter().enumerate() {
        b.spawn(NumberField::new(format!("pcb-rotate-{axis}"), axis.to_uppercase()).label_width(24.0).text(format_with_unit(now.rotate[i], Quantity::Angle)).build(t));
    }
    let changed = now != accepted;
    b.spawn(Node { column_gap: Val::Px(8.0), margin: UiRect::top(Val::Px(10.0)), ..default() }).with_children(|r| {
        r.spawn((
            Button::new("pcb-custom-accept").icon("check").label("Accept").primary().small().disabled(!changed).tooltip("Accept the new position").build(t),
            observe(|_: On<Activate>, mut commands: Commands| {
                commands.queue(accept_edit);
            }),
        ));
        r.spawn((
            Button::new("pcb-custom-cancel").label("Cancel").small().disabled(!changed).build(t),
            observe(|_: On<Activate>, mut commands: Commands| {
                commands.queue(|w: &mut World| {
                    let accepted = subject(w).and_then(|s| s.representation.custom().map(|c| c.transform));
                    if let (Some(a), Some(e)) = (accepted, w.resource_mut::<PcbUi>().edit.as_mut()) {
                        e.transform = a;
                    }
                });
            }),
        ));
    });
}

/// The Representation radio: None and From ECAD data apply at once; Custom part opens Select
/// custom part (the radio goes back if it is cancelled).
fn on_representation(ev: On<RadioChange>, q: Query<&Name>, mut commands: Commands) {
    if !q.get(ev.entity).is_ok_and(|n| n.as_str() == "pcb-representation") {
        return;
    }
    let index = ev.index;
    commands.queue(move |w: &mut World| {
        let Some(s) = subject(w) else { return };
        let rep = match index {
            0 => Representation::None,
            1 => Representation::FromEcad,
            _ => {
                if s.representation.custom().is_none() {
                    super::dialogs::open_custom_part_dialog(w, s.element, &s.package);
                }
                return;
            }
        };
        let label = format!("{} for {}", rep.label(), s.package);
        let cmd = SetRepresentation { element: s.element, package: s.package.clone(), representation: rep, label };
        if let Err(e) = w.resource_mut::<ActiveDocument>().execute(&cmd) {
            warn!("representation: {e}");
        }
    });
}

/// The open-link icon by From ECAD data: the package's component view.
fn on_ecad_link(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    if !q.get(a.entity).is_ok_and(|n| n.as_str() == "pcb-rep-ecad-link") {
        return;
    }
    commands.queue(|w: &mut World| {
        if let Some(s) = subject(w) {
            super::open_component_view(w, s.element, s.board, &s.package);
        }
    });
}

/// A typed Translate or Rotate value: the preview moves.
fn on_number_commit(ev: On<NumberFieldCommit>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else { return };
    let (rotate, axis) = match name.as_str() {
        "pcb-translate-x" => (false, 0),
        "pcb-translate-y" => (false, 1),
        "pcb-translate-z" => (false, 2),
        "pcb-rotate-x" => (true, 0),
        "pcb-rotate-y" => (true, 1),
        "pcb-rotate-z" => (true, 2),
        _ => return,
    };
    let (entity, text) = (ev.entity, ev.text.clone());
    commands.queue(move |w: &mut World| {
        let units = w.resource::<ActiveDocument>().doc.units;
        let q = if rotate { Quantity::Angle } else { Quantity::Length };
        match units.eval(&text, q) {
            Ok(v) => {
                // A turn is about the part's middle, so it turns in place (P3H.4).
                let pts = if rotate {
                    subject(w).and_then(|s| s.representation.custom().cloned()).and_then(|c| super::view::custom_solid(w, &c.source)).map(|(s, _)| s.positions.clone()).unwrap_or_default()
                } else {
                    vec![]
                };
                if let Some(e) = w.resource_mut::<PcbUi>().edit.as_mut() {
                    if rotate {
                        let mut r = e.transform.rotate;
                        r[axis] = v;
                        e.transform = e.transform.rotated_in_place(r, &pts);
                    } else {
                        e.transform.translate[axis] = v;
                    }
                }
                if let Some(mut s) = w.get_mut::<NumberFieldState>(entity) {
                    s.text = format_with_unit(v, q);
                    s.error = false;
                }
            }
            Err(_) => {
                if let Some(mut s) = w.get_mut::<NumberFieldState>(entity) {
                    s.error = true;
                }
            }
        }
    });
}

/// Center: the part in the middle of the footprint, on the board (the rotation kept).
fn center_part(world: &mut World) {
    let Some(s) = subject(world) else { return };
    let Some(c) = s.representation.custom() else { return };
    let Some((solid, _)) = super::view::custom_solid(world, &c.source) else { return };
    if let Some(e) = world.resource_mut::<PcbUi>().edit.as_mut() {
        e.transform = e.transform.centered(&solid.positions);
    }
}

/// ✓: the moved part into the library (one undo step).
fn accept_edit(world: &mut World) {
    let Some(s) = subject(world) else { return };
    let Some(mut c) = s.representation.custom().cloned() else { return };
    let Some(e) = world.resource::<PcbUi>().edit.clone() else { return };
    if e.transform == c.transform {
        return;
    }
    c.transform = e.transform;
    let cmd = SetRepresentation { element: s.element, package: s.package.clone(), representation: Representation::Custom(Box::new(c)), label: format!("Move custom part of {}", s.package) };
    if let Err(err) = world.resource_mut::<ActiveDocument>().execute(&cmd) {
        warn!("custom part: {err}");
    }
}

// ---------------------------------------------------------------------------------------------
// BOM pane

/// A BOM row: its group and, for a component's own row, which one.
#[derive(Component, Clone, Debug)]
struct BomRowRef {
    items: Vec<ItemId>,
}

/// A designator cell: its group and component (`None`: the group's cell).
#[derive(Component, Clone, Debug)]
struct BomDesignator {
    group: usize,
    item: Option<ItemId>,
    refdes: String,
}

fn bom_columns() -> Vec<Column> {
    vec![
        Column::new("qty", "Qty").width(46.0),
        Column::new("designator", "Designator").width(96.0),
        Column::new("part-name", "Part name").width(112.0),
        Column::new("part-number", "Part number").width(PANE_W - 46.0 - 96.0 - 112.0 - 4.0),
    ]
}

fn spawn_bom_pane(world: &mut World, t: &Theme) -> Entity {
    let pane = pane_frame(world, t, "pcb-bom-pane", "Bill of materials", "bill-of-materials");
    let Some((_, _, board)) = super::shown(world) else { return pane };
    let rows = bom(&board);
    let expanded = world.resource::<PcbUi>().bom_expanded.clone();
    let refdes = |i: ItemId| board.component(i).map(|p| p.refdes.clone()).unwrap_or_default();
    let columns = bom_columns();
    // (row name, a component's own row, its components, cells: text, designator, open).
    type Cell = (String, Option<BomDesignator>, bool);
    let mut specs: Vec<(String, bool, BomRowRef, Vec<Cell>)> = Vec::new();
    for (gi, r) in rows.iter().enumerate() {
        let open = expanded.contains(&(r.package.clone(), r.part_number.clone()));
        let group: &BomRow = r;
        let single = group.items.len() == 1;
        let d = BomDesignator { group: gi, item: single.then(|| group.items[0]), refdes: group.designators() };
        specs.push((
            format!("pcb-bom-row-{gi}"),
            false,
            BomRowRef { items: group.items.clone() },
            vec![
                (group.quantity().to_string(), None, false),
                (group.designators(), Some(d), open),
                (group.package.clone(), None, false),
                (group.part_number.clone(), None, false),
            ],
        ));
        if open && !single {
            for (j, item) in group.items.iter().enumerate() {
                let d = BomDesignator { group: gi, item: Some(*item), refdes: refdes(*item) };
                specs.push((
                    format!("pcb-bom-row-{gi}-{j}"),
                    true,
                    BomRowRef { items: vec![*item] },
                    vec![("".into(), None, false), (refdes(*item), Some(d), false), (group.package.clone(), None, false), (group.part_number.clone(), None, false)],
                ));
            }
        }
    }
    let t = t.clone();
    let mut commands = world.commands();
    commands.entity(pane).with_children(move |p| {
        p.spawn((
            Name::new("pcb-bom-table"),
            TableRoot,
            Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::clip(), ..default() },
        ))
        .with_children(|tbl| {
            tbl.spawn(TableHeader::new("pcb-bom-header", columns.clone()).height(28.0).build(&t));
            tbl.spawn((Name::new("pcb-bom-rows"), cadrs_ui::TableBody, Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::scroll_y(), ..default() }))
                .with_children(|rows| {
                    for (name, instance, row_ref, cells) in specs {
                        let mut tr = TableRow::new(name.clone(), &columns).height(26.0);
                        for (ci, (text, desig, open)) in cells.into_iter().enumerate() {
                            let theme = t.clone();
                            let cell_name = desig.as_ref().map(|d| match d.item.filter(|_| instance) {
                                Some(_) => format!("pcb-bom-designator-{}", name.trim_start_matches("pcb-bom-row-")),
                                None => format!("pcb-bom-designator-{}", d.group),
                            });
                            let muted = instance && ci >= 2;
                            tr = tr.cell(move |cell| {
                                let fg = if muted { theme.muted_foreground } else { theme.foreground };
                                let mut e = cell.spawn(Node { width: Val::Percent(100.0), height: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(3.0), overflow: Overflow::clip(), ..default() });
                                if let (Some(d), Some(n)) = (desig, cell_name) {
                                    let caret = (d.item.is_none()).then_some(if open { "chevron-down" } else { "chevron-right" });
                                    let editable = d.item.is_some();
                                    let pad = if instance { 14.0 } else { 0.0 };
                                    e.insert((Name::new(n), DoubleClickable, Tooltip::new(if editable { "Double-click to rename" } else { "Double-click to list its components" })));
                                    e.entry::<Node>().and_modify(move |mut node| node.padding = UiRect::left(Val::Px(pad)));
                                    if editable {
                                        e.insert(InlineEdit::default());
                                    }
                                    e.insert(d);
                                    e.with_children(|x| {
                                        if let Some(c) = caret {
                                            x.spawn((cadrs_ui::icon(c, 11.0, theme.muted_foreground), Pickable::IGNORE));
                                        }
                                        x.spawn((theme.text(text, theme.font_base, FontWeight::MEDIUM, fg), cadrs_ui::ellipsis::Ellipsis::default().with_tooltip(), cadrs_ui::ellipsis::Ellipsis::node(), InlineEditLabel, Pickable::IGNORE));
                                    });
                                } else {
                                    e.insert(Pickable::IGNORE);
                                    e.with_children(|x| {
                                        x.spawn((theme.text(text, theme.font_base, FontWeight::MEDIUM, fg), cadrs_ui::ellipsis::Ellipsis::default().with_tooltip(), cadrs_ui::ellipsis::Ellipsis::node(), Pickable::IGNORE));
                                    });
                                }
                            });
                        }
                        rows.spawn((tr.build(&t), row_ref));
                    }
                });
        });
        p.spawn((
            Name::new("pcb-bom-hint"),
            t.text("Double-click a designator to rename it.", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
            Node { padding: UiRect::all(Val::Px(8.0)), flex_shrink: 0.0, ..default() },
        ));
    });
    world.flush();
    pane
}

/// A click on a row selects its components (Ctrl or Shift adds them).
fn on_bom_row(a: On<Activate>, q: Query<&BomRowRef>, keys: Res<ButtonInput<KeyCode>>, mut ui: ResMut<PcbUi>) {
    let Ok(r) = q.get(a.entity) else { return };
    if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        for i in &r.items {
            if !ui.selected.contains(i) {
                ui.selected.push(*i);
            }
        }
    } else {
        ui.selected = r.items.clone();
    }
}

/// The row under the pointer lights its components in the view.
fn bom_hover(q: Query<(&BomRowRef, &Hovered)>, mut ui: ResMut<PcbUi>) {
    let want: Vec<ItemId> = q.iter().find(|(_, h)| h.get()).map(|(r, _)| r.items.clone()).unwrap_or_default();
    if ui.bom_hover != want {
        ui.bom_hover = want;
    }
}

/// A row's own background, kept while it is tinted.
#[derive(Component, Clone, Copy)]
struct RowBase(Color, Color);

/// A row some (not all) of whose components are selected (P3H.4 judge): a light orange.
const PARTIAL_TINT: Color = Color::srgb(0.99, 0.93, 0.83);
/// A search match's row: a light blue, as the view's blue match tint.
const MATCH_ROW_TINT: Color = Color::srgb(0.85, 0.92, 0.99);

/// Rows show the selection (selected; a group with some members selected is tinted) and the
/// search matches (their own tint), both ways with the view.
#[allow(clippy::type_complexity)]
fn sync_bom_rows(ui: Res<PcbUi>, mut q: Query<(Entity, &BomRowRef, Has<cadrs_ui::Selected>, &mut Visuals, Option<&RowBase>)>, q_new: Query<(), Added<BomRowRef>>, mut commands: Commands) {
    if !ui.is_changed() && q_new.is_empty() {
        return;
    }
    let matches = ui.match_items();
    for (e, r, sel, mut vis, base) in &mut q {
        let all = !r.items.is_empty() && r.items.iter().all(|i| ui.selected.contains(i));
        if all != sel {
            if all {
                commands.entity(e).try_insert(cadrs_ui::Selected);
            } else {
                commands.entity(e).try_remove::<cadrs_ui::Selected>();
            }
        }
        let (own, own_hover) = match base {
            Some(b) => (b.0, b.1),
            None => {
                commands.entity(e).try_insert(RowBase(vis.background.normal, vis.background.hover));
                (vis.background.normal, vis.background.hover)
            }
        };
        let partial = !all && r.items.iter().any(|i| ui.selected.contains(i));
        let lit = !all && r.items.iter().any(|i| matches.contains(i));
        let want = if partial {
            PARTIAL_TINT
        } else if lit {
            MATCH_ROW_TINT
        } else {
            own
        };
        if vis.background.normal != want {
            vis.background.normal = want;
            vis.background.hover = if want == own { own_hover } else { want };
        }
    }
}

/// Double-click a designator: a group of several opens into one row per component (or closes);
/// a component's designator is edited in place.
fn on_bom_double_click(ev: On<DoubleClick>, q: Query<&BomDesignator>, mut commands: Commands) {
    let Ok(d) = q.get(ev.entity).cloned() else { return };
    let entity = ev.entity;
    commands.queue(move |w: &mut World| {
        if d.item.is_none() {
            let Some((_, _, board)) = super::shown(w) else { return };
            let Some(r) = bom(&board).into_iter().nth(d.group) else { return };
            let key = (r.package, r.part_number);
            let mut ui = w.resource_mut::<PcbUi>();
            if !ui.bom_expanded.remove(&key) {
                ui.bom_expanded.insert(key);
            }
            return;
        }
        let theme = w.resource::<Theme>().clone();
        let mut cm = w.commands();
        let mut o = InlineEditOptions::new("pcb-bom-edit");
        o.height = 22.0;
        begin_inline_edit(&mut cm, &theme, entity, d.refdes.clone(), o);
        w.flush();
    });
}

/// Enter in a designator being edited: rename it (one undo step); a duplicate is refused with a
/// message.
fn on_bom_commit(ev: On<InlineEditCommit>, q: Query<&BomDesignator>, mut commands: Commands) {
    let Ok(d) = q.get(ev.entity).cloned() else { return };
    let Some(item) = d.item else { return };
    let value = ev.value.trim().to_string();
    if value == d.refdes {
        return;
    }
    commands.queue(move |w: &mut World| {
        let Some((el, b, _)) = super::shown(w) else { return };
        let cmd = SetRefdes { element: el, board: b, item, refdes: value };
        if let Err(e) = w.resource_mut::<ActiveDocument>().execute(&cmd) {
            let theme = w.resource::<Theme>().clone();
            let mut commands = w.commands();
            show_notification(&mut commands, &theme, Notification::warning(format!("Couldn't rename {}: {e}", d.refdes)).name("pcb-refdes-toast"));
            w.flush();
            // Show the designator as it was.
            refresh(w);
        }
    });
}
