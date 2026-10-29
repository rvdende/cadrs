//! The Parts list under the feature list (PS2.8, X6), like Onshape's
//! (`training/intro-to-part-studios/ex1-step5.png`):
//!
//! - Groups **Parts (n)** and, once there are surfaces, **Surfaces (n)**; each part a row with
//!   its name ("Part 1", or its rename). A row selects its part (the selection model the
//!   viewport shares). The part the open Extrude dialog makes or changes is bold.
//! - Right-click a part: **Rename** (in place, one undo step), **Hide** / **Show**, **Isolate**
//!   (only that part is shown until "Exit isolate" or Esc) and **Delete** (a "Delete part"
//!   feature at the end of the list, as Onshape does), **Assign material…** and **Edit
//!   appearance…** and **Make transparent…** (P3.5), **Export…** (STEP, see
//!   [`crate::export_dialog`]). The other items of Onshape's menu are
//!   shown disabled until their milestones.
//! - A right-click opens the menu without selecting the part (it does not turn orange in the
//!   view, `ex2-step8.png`).
//! - A hidden part's row is greyed, with a crossed eye. A row's eye shows while it is hovered;
//!   a click shows or hides the part.
//! - A derived PCB board is a folder: its board part's row has a chevron, and its components
//!   are listed under it (closed at first).

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::PartId;
use cadrs_core::commands::{AddFeature, RenamePart, SetPartsHidden};
use cadrs_core::parts::PartKind;
use cadrs_ui::menu::ContextMenuAnchor;
use cadrs_ui::prelude::*;
use cadrs_ui::{InlineEditOptions, StateColors, TreeToggle, Visuals, begin_inline_edit};

use crate::document::{PartRows, tab_node_name};
use crate::parts::{PartCache, PartOverride};
use crate::viewport::{Pick, PickRow, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct PartsListPlugin;

impl Plugin for PartsListPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GroupsOpen>()
            .init_resource::<BoardsOpen>()
            .add_systems(
                Update,
                (
                    rebuild_part_list,
                    sync_isolate_bar,
                    exit_isolate_on_escape,
                    mark_menu_row,
                )
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_observer(on_part_context_menu)
            .add_observer(on_part_menu_action)
            .add_observer(on_part_rename_commit)
            .add_observer(on_group_toggle)
            .add_observer(on_board_toggle)
            .add_observer(on_part_eye)
            .add_observer(on_exit_isolate);
    }
}

/// A part row.
#[derive(Component, Debug, Clone, Copy)]
pub struct PartRow(pub PartId);

/// A group header ("Parts (n)", "Surfaces (n)").
#[derive(Component, Debug, Clone, Copy)]
struct GroupHeader(PartKind);

/// Which groups are open.
#[derive(Resource, Debug, Clone, Copy)]
struct GroupsOpen {
    parts: bool,
    surfaces: bool,
}

impl Default for GroupsOpen {
    fn default() -> Self {
        Self {
            parts: true,
            surfaces: true,
        }
    }
}

impl GroupsOpen {
    fn get(&self, k: PartKind) -> bool {
        match k {
            PartKind::Solid => self.parts,
            PartKind::Surface => self.surfaces,
        }
    }
}

/// Derived boards whose components are shown under them (by the board part).
#[derive(Resource, Debug, Clone, Default)]
struct BoardsOpen(Vec<PartId>);

/// A derived board's row: its chevron opens its components.
#[derive(Component, Debug, Clone, Copy)]
struct BoardFolder(PartId);

/// The part a context menu is for.
#[derive(Component, Debug, Clone, Copy)]
struct PartMenuFor(PartId);

/// A row's height with its margins (22 px and 1 px above and below), a group header's too: a
/// long list builds only the rows it shows by it.
pub const PART_ROW_PITCH: f32 = 24.0;

/// What the rows were built from (and the boards with their components, and the open ones).
type RowKey = (
    Vec<(PartId, String, PartKind, bool, bool, bool)>,
    bool,
    bool,
    Vec<(PartId, Vec<PartId>)>,
    Vec<PartId>,
);

/// Rebuilds the groups and rows when the parts, their names or visibility change.
#[allow(clippy::too_many_arguments)]
fn rebuild_part_list(
    cache: Res<PartCache>,
    doc: Option<Res<ActiveDocument>>,
    over: Res<PartOverride>,
    open: Res<GroupsOpen>,
    boards_open: Res<BoardsOpen>,
    mut q_rows: Query<(
        Entity,
        Ref<PartRows>,
        &mut cadrs_ui::virtual_rows::VirtualRows,
    )>,
    theme: Res<Theme>,
    mut last: Local<Option<RowKey>>,
    mut commands: Commands,
) {
    let Some((container, added, mut window)) = q_rows
        .iter_mut()
        .next()
        .map(|(e, r, w)| (e, r.is_added(), w))
    else {
        return;
    };
    // A long list scrolled to rows not built.
    let moved = window.is_changed();
    // The rows only change with what they show: don't build their key every frame.
    let inputs_changed = cache.is_changed()
        || over.is_changed()
        || open.is_changed()
        || boards_open.is_changed()
        || theme.is_changed()
        || doc.as_ref().is_some_and(|d| d.is_changed());
    if !added && !moved && !inputs_changed && last.is_some() {
        return;
    }
    // P3H.6 judge: a composite part's row has the composite part icon.
    let composite = |p: PartId| {
        doc.as_deref()
            .and_then(|d| {
                Some(cadrs_core::transform::is_composite_part(
                    &d.doc,
                    d.active_element()?.id,
                    p,
                ))
            })
            .unwrap_or(false)
    };
    // While a Sheet metal model's edges are picked, the parts it makes (`t0101.0.png`), bold
    // as its preview.
    let staged = !cache.staged_parts.is_empty();
    let listed = if staged {
        &cache.staged_parts
    } else {
        &cache.parts
    };
    let rows: Vec<(PartId, String, PartKind, bool, bool, bool)> = listed
        .iter()
        // P3B.9: an assembly context's parts are not the studio's.
        .filter(|p| !cadrs_core::assembly::context::is_context(p.feature))
        .map(|p| {
            (
                p.id,
                cache.part_name(p.id).unwrap_or(&p.name).to_string(),
                p.kind,
                over.previews(p)
                    || (staged && over.staged.is_some_and(|f| p.features.contains(&f))),
                // A board component hidden by its board's layout view greys as hidden.
                cache.is_hidden_part(p.id) || cache.layout_hidden.contains(&p.id),
                composite(p.id),
            )
        })
        .collect();
    // Each derived board copy's part and its components.
    let boards: Vec<(PartId, Vec<PartId>)> = cache
        .derived
        .values()
        .flat_map(|o| {
            o.boards
                .iter()
                .map(move |b| (b.part, o.components_of(b.part)))
        })
        .filter(|(_, c)| !c.is_empty())
        .collect();
    let key = (
        rows,
        open.parts,
        open.surfaces,
        boards,
        boards_open.0.clone(),
    );
    if !added && !moved && last.as_ref() == Some(&key) {
        return;
    }
    let t = theme.clone();
    // A long list builds the rows near what it shows, spacers standing in for the rest.
    let windowed = window.window_for(window.total.max(key.0.len()));
    let mut slots = cadrs_ui::virtual_rows::Slots::new(windowed.clone());
    commands.entity(container).despawn_children();
    commands.entity(container).with_children(|c| {
        let above = windowed.is_some().then(|| {
            c.spawn(cadrs_ui::virtual_rows::spacer("part-rows-above", 0.0))
                .id()
        });
        for kind in [PartKind::Solid, PartKind::Surface] {
            let members: Vec<&(PartId, String, PartKind, bool, bool, bool)> =
                key.0.iter().filter(|r| r.2 == kind).collect();
            // Onshape shows "Parts (0)" in an empty studio, and the other groups once they have
            // something.
            if kind == PartKind::Surface && members.is_empty() {
                continue;
            }
            let (title, name) = match kind {
                PartKind::Solid => ("Parts", "parts-section-header"),
                PartKind::Surface => ("Surfaces", "surfaces-section-header"),
            };
            let is_open = open.get(kind);
            if slots.take(PART_ROW_PITCH) {
                c.spawn((
                    TreeItem::new(name, format!("{title} ({})", members.len()))
                        .disclosure(Some(is_open))
                        .left(2.0)
                        .build(&t),
                    GroupHeader(kind),
                ));
            }
            if !is_open {
                continue;
            }
            let component = |id: &PartId| key.3.iter().any(|(_, c)| c.contains(id));
            for m in members.iter().filter(|m| !component(&m.0)) {
                let folder = key.3.iter().find(|(b, _)| *b == m.0);
                let open = folder.is_some_and(|(b, _)| key.4.contains(b));
                if slots.take(PART_ROW_PITCH) {
                    spawn_row(c, &t, m, kind, 22.0, folder.map(|_| open));
                }
                if let Some((_, comps)) = folder.filter(|_| open) {
                    for cm in comps
                        .iter()
                        .filter_map(|id| members.iter().find(|r| r.0 == *id))
                    {
                        if slots.take(PART_ROW_PITCH) {
                            spawn_row(c, &t, cm, kind, 40.0, None);
                        }
                    }
                }
            }
        }
        if let Some(above) = above {
            let (h_above, h_below) = slots.spacers();
            c.commands()
                .entity(above)
                .entry::<Node>()
                .and_modify(move |mut n| n.height = Val::Px(h_above));
            c.spawn(cadrs_ui::virtual_rows::spacer("part-rows-below", h_below));
        }
    });
    window.bypass_change_detection().total = slots.count();
    *last = Some(key);
}

/// A part's row: `left` its indent; `folder` a board's chevron (open or not).
fn spawn_row(
    c: &mut ChildSpawnerCommands,
    t: &Theme,
    m: &(PartId, String, PartKind, bool, bool, bool),
    kind: PartKind,
    left: f32,
    folder: Option<bool>,
) {
    let (id, label, _, bold, hidden, is_composite) = m;
    let visuals = Visuals {
        background: StateColors::new(Color::NONE, t.list_hover, t.list_active, Color::NONE)
            .with_selected(t.list_selected),
        border: StateColors::all(Color::NONE),
        foreground: StateColors::all(if *hidden {
            Color::srgb_u8(0xa6, 0xa6, 0xa6)
        } else {
            t.foreground
        }),
        focus_ring: t.focus_ring,
    };
    let icon_name = match kind {
        PartKind::Solid if folder.is_some() => "pcb-studio",
        PartKind::Solid if *is_composite => "composite-part",
        PartKind::Solid => "part",
        PartKind::Surface => "surface",
    };
    let row_name = tab_node_name(label).replacen("tab-", "part-row-", 1);
    // The eye shows while the row is hovered (always, crossed, on a hidden part); a click shows
    // or hides the part.
    let tip = if *hidden {
        format!("Show {label}")
    } else {
        format!("Hide {label}")
    };
    let mut item = TreeItem::new(row_name.clone(), label.clone())
        .toggle(
            format!("{row_name}-visibility"),
            "visible",
            "hidden",
            !*hidden,
        )
        .toggle_tooltip(tip)
        .toggle_keeps_off()
        .icon(icon_name, 16.0)
        .icon_color(if *hidden {
            Color::srgb_u8(0xc4, 0xc4, 0xc4)
        } else {
            t.muted_foreground
        })
        .weight(if *bold {
            FontWeight::BOLD
        } else {
            FontWeight::MEDIUM
        })
        .left(if folder.is_some() { left - 14.0 } else { left })
        .editable();
    if folder.is_some() {
        item = item.disclosure(folder);
    }
    let mut row = c.spawn((
        item.build(t),
        PickRow(Pick::Part(*id)),
        PartRow(*id),
        ContextMenuTarget,
    ));
    if folder.is_some() {
        row.insert(BoardFolder(*id));
    }
    row.insert(visuals).entry::<Node>().and_modify(|mut n| {
        n.height = Val::Px(22.0);
        n.margin = UiRect::new(Val::Px(2.0), Val::Px(7.0), Val::Px(1.0), Val::Px(1.0));
    });
}

/// A board's chevron opens or closes its components.
fn on_board_toggle(ev: On<TreeToggle>, q: Query<&BoardFolder>, mut open: ResMut<BoardsOpen>) {
    if let Ok(f) = q.get(ev.entity) {
        match open.0.iter().position(|p| *p == f.0) {
            Some(i) => {
                open.0.remove(i);
            }
            None => open.0.push(f.0),
        }
    }
}

/// The eye on a part's row shows or hides that part, as an undoable step.
fn on_part_eye(ev: On<cadrs_ui::TreeRowToggled>, q: Query<&PartRow>, mut commands: Commands) {
    let Ok(row) = q.get(ev.entity) else {
        return;
    };
    let part = row.0;
    commands.queue(move |world: &mut World| {
        let hidden = !world.resource::<PartCache>().is_hidden_part(part);
        let parts = vec![part];
        let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
            return;
        };
        let Some(element) = doc.active_element().map(|e| e.id) else {
            return;
        };
        let _ = doc.execute(&SetPartsHidden {
            element,
            parts,
            hidden,
        });
    });
}

fn on_group_toggle(ev: On<TreeToggle>, q: Query<&GroupHeader>, mut open: ResMut<GroupsOpen>) {
    if let Ok(g) = q.get(ev.entity) {
        match g.0 {
            PartKind::Solid => open.parts = !open.parts,
            PartKind::Surface => open.surfaces = !open.surfaces,
        }
    }
}

/// The parts a menu action applies to: the selected parts if the menu's part is one of them,
/// else just it.
fn targets(world: &World, part: PartId) -> Vec<PartId> {
    let selected: Vec<PartId> = world
        .resource::<Selection>()
        .0
        .iter()
        .filter_map(|p| match p {
            Pick::Part(id) => Some(*id),
            _ => None,
        })
        .collect();
    if selected.contains(&part) {
        selected
    } else {
        vec![part]
    }
}

fn on_part_context_menu(
    ev: On<ContextMenuRequested>,
    q: Query<&PartRow>,
    cache: Res<PartCache>,
    theme: Res<Theme>,
    doc: Option<Res<ActiveDocument>>,
    mut commands: Commands,
) {
    let Ok(row) = q.get(ev.entity) else {
        return;
    };
    let name = cache.part_name(row.0).unwrap_or("part").to_string();
    let hidden = cache.is_hidden_part(row.0);
    let menu = Menu::new("part-context-menu")
        .min_width(180.0)
        .item_height(22.0)
        .item(MenuItem::new("part-rename", "Rename"))
        .item(MenuItem::new("part-properties", "Properties…"))
        .item(MenuItem::new("part-material", "Assign material…"))
        .item(MenuItem::new("part-appearance", "Edit appearance…"))
        .separator()
        .item(MenuItem::new("part-copy-here", "Copy here…").planned())
        .item(
            MenuItem::new("part-copy", format!("Copy {name}"))
                .icon("copy")
                .disabled(true),
        )
        .item(MenuItem::new("part-drawing", format!("Create Drawing of {name}…")).icon("file-new"));
    // P3I.7 (SM16.1): a sheet metal part's flat pattern.
    let sheet_metal = doc
        .as_deref()
        .and_then(|d| {
            d.active_element()
                .map(|e| e.id)
                .map(|e| crate::drawing::flat_views::is_sheet_metal_part(&d.doc, e, row.0))
        })
        .unwrap_or(false);
    let menu = if sheet_metal {
        menu.item(
            MenuItem::new("part-flat-drawing", "Create drawing of flat pattern…")
                .icon("flat-pattern"),
        )
    } else {
        menu
    };
    let menu = menu
        .item(MenuItem::new("part-export", "Export…").icon("file-export"))
        .item(
            MenuItem::new("part-where-used", "Where used…")
                .icon("tab-manager")
                .planned(),
        )
        .item(MenuItem::new("part-task", "Create task…").planned())
        .separator()
        .item(if hidden {
            MenuItem::new("part-show", "Show").icon("visible")
        } else {
            MenuItem::new("part-hide", "Hide").icon("hidden")
        })
        .item(MenuItem::new("part-isolate", "Isolate…"))
        .item(if cache.transparent.contains(&row.0) {
            MenuItem::new("part-opaque", "Make opaque")
        } else {
            MenuItem::new("part-transparent", "Make transparent…")
        })
        .separator()
        .item(
            MenuItem::new("part-comment", "Add comment")
                .icon("comments")
                .planned(),
        )
        .item(MenuItem::new("part-zoom", "Zoom to selection"))
        .separator()
        .item(MenuItem::new("part-delete", "Delete…").icon("remove-circle"));
    let anchor = open_context_menu(&mut commands, ev.position, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((PartMenuFor(row.0), DespawnOnExit(AppState::Document)));
}

fn on_part_menu_action(
    ev: On<MenuAction>,
    q_anchor: Query<&PartMenuFor, With<ContextMenuAnchor>>,
    mut commands: Commands,
) {
    let Ok(target) = q_anchor.get(ev.entity) else {
        return;
    };
    let part = target.0;
    match ev.item.as_str() {
        "part-rename" => commands.queue(move |world: &mut World| rename_part(world, part)),
        // P3E.3a: the part (or the selected parts it is among).
        "part-zoom" => commands.queue(move |world: &mut World| {
            let parts = targets(world, part);
            world.resource_mut::<crate::viewport::Selection>().0 =
                parts.into_iter().map(crate::viewport::Pick::Part).collect();
            crate::view_options::zoom_to_selection(world);
        }),
        "part-hide" | "part-show" => {
            let hidden = ev.item == "part-hide";
            commands.queue(move |world: &mut World| {
                let parts = targets(world, part);
                let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
                    return;
                };
                let Some(element) = doc.active_element().map(|e| e.id) else {
                    return;
                };
                let _ = doc.execute(&SetPartsHidden {
                    element,
                    parts,
                    hidden,
                });
            });
        }
        "part-appearance" => commands.queue(move |world: &mut World| {
            let parts = targets(world, part);
            crate::appearance::open_appearance_dialog(
                world,
                crate::appearance::AppearanceTarget::Parts(parts),
            );
        }),
        // P3B.6 (TD9.5): the part's properties.
        "part-properties" => commands.queue(move |world: &mut World| {
            let Some(element) = world
                .get_resource::<ActiveDocument>()
                .and_then(|d| d.active_element().map(|e| e.id))
            else {
                return;
            };
            crate::properties_dialog::open_properties_dialog(
                world,
                cadrs_core::properties::PropertyOwner::Part { element, part },
            );
        }),
        "part-material" => commands.queue(move |world: &mut World| {
            let parts = targets(world, part);
            crate::material_dialog::open_material_dialog(world, parts);
        }),
        // D1.2 (P3C.5): Create Drawing of the part.
        "part-drawing" => commands.queue(move |world: &mut World| {
            let Some(element) = world
                .get_resource::<ActiveDocument>()
                .and_then(|d| d.active_element().map(|e| e.id))
            else {
                return;
            };
            let r = cadrs_drawing::ObjectRef {
                element: element.0,
                part: Some((part.feature.0, part.index)),
            };
            crate::drawing::create_dialog::open_create_drawing_of_part(world, r);
        }),
        "part-flat-drawing" => commands.queue(move |world: &mut World| {
            let Some(element) = world
                .get_resource::<ActiveDocument>()
                .and_then(|d| d.active_element().map(|e| e.id))
            else {
                return;
            };
            let r = cadrs_drawing::ObjectRef {
                element: element.0,
                part: Some((part.feature.0, part.index)),
            };
            crate::drawing::flat_views::open_create_drawing_of_flat(world, r);
        }),
        "part-export" => commands.queue(move |world: &mut World| {
            let parts = targets(world, part);
            crate::export_dialog::open_export_dialog(world, parts);
        }),
        "part-transparent" | "part-opaque" => {
            let on = ev.item == "part-transparent";
            commands.queue(move |world: &mut World| {
                let parts = targets(world, part);
                world
                    .resource_mut::<PartCache>()
                    .set_transparent(&parts, on);
            });
        }
        "part-isolate" => commands.queue(move |world: &mut World| {
            let parts = targets(world, part);
            world.resource_mut::<PartCache>().isolate(Some(parts));
        }),
        "part-delete" => commands.queue(move |world: &mut World| {
            let parts = targets(world, part);
            if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
                && let Some(element) = doc.active_element().map(|e| e.id)
            {
                let _ = doc.execute(&AddFeature::delete_parts(
                    element,
                    cadrs_core::FeatureId::new(),
                    parts.clone(),
                ));
            }
            world
                .resource_mut::<Selection>()
                .0
                .retain(|p| p.part().is_none_or(|id| !parts.contains(&id)));
        }),
        _ => {}
    }
}

/// The row whose context menu is open gets the light hover band (`ex2-step8.png`), without
/// selecting its part.
fn mark_menu_row(
    q_anchor: Query<&PartMenuFor, With<ContextMenuAnchor>>,
    q_rows: Query<(Entity, &PartRow, Has<cadrs_ui::ForceState>, Has<Selected>)>,
    mut commands: Commands,
) {
    let open: Vec<PartId> = q_anchor.iter().map(|a| a.0).collect();
    for (e, row, forced, selected) in &q_rows {
        // A selected row keeps its selection band.
        let want = open.contains(&row.0) && !selected;
        if want && !forced {
            commands
                .entity(e)
                .try_insert(cadrs_ui::ForceState(VisualState::Hover));
        } else if !want && forced {
            commands.entity(e).try_remove::<cadrs_ui::ForceState>();
        }
    }
}

/// Starts renaming a part in place in the Parts list (PS6.5): Enter or clicking elsewhere commits
/// (one undo step), Esc cancels.
pub fn rename_part(world: &mut World, part: PartId) {
    let Some(name) = world
        .resource::<PartCache>()
        .part_name(part)
        .map(str::to_string)
    else {
        return;
    };
    let mut q = world.query::<(Entity, &PartRow)>();
    let Some(row) = q.iter(world).find(|(_, r)| r.0 == part).map(|(e, _)| e) else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let mut opts = InlineEditOptions::new("part-rename");
    opts.width = Val::Px(150.0);
    opts.height = 20.0;
    opts.font_size = Some(theme.font_sm);
    opts.weight = FontWeight::MEDIUM;
    opts.padding = Some(2.0);
    let mut commands = world.commands();
    begin_inline_edit(&mut commands, &theme, row, name, opts);
    world.flush();
}

fn on_part_rename_commit(
    ev: On<InlineEditCommit>,
    q: Query<&PartRow>,
    doc: Option<ResMut<ActiveDocument>>,
) {
    if ev.entity != ev.original_event_target() {
        return;
    }
    let (Ok(row), Some(mut doc)) = (q.get(ev.entity), doc) else {
        return;
    };
    let value = ev.value.trim().to_string();
    if value.is_empty() {
        return;
    }
    if let Some(element) = doc.active_element().map(|e| e.id) {
        let _ = doc.execute(&RenamePart {
            element,
            part: row.0,
            name: value,
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Isolate

/// The "Exit isolate" bar at the top of the viewport while parts are isolated.
#[derive(Component)]
struct IsolateBar;

fn sync_isolate_bar(
    cache: Res<PartCache>,
    theme: Res<Theme>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_bar: Query<Entity, With<IsolateBar>>,
    mut commands: Commands,
) {
    let want = cache.isolated.is_some();
    if want != q_bar.is_empty() {
        return;
    }
    for e in &q_bar {
        commands.entity(e).try_despawn();
    }
    if !want {
        return;
    }
    let Some(area) = q_area.iter().next() else {
        return;
    };
    let t = theme.clone();
    let bar = commands
        .spawn((
            Name::new("isolate-bar"),
            IsolateBar,
            DespawnOnExit(AppState::Document),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(8.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-60.0)),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(0x3a, 0x3f, 0x44)),
        ))
        .with_children(|b| {
            b.spawn(t.text("Isolated", t.font_sm, FontWeight::MEDIUM, Color::WHITE));
            b.spawn(
                cadrs_ui::Button::new("exit-isolate")
                    .label("Exit isolate")
                    .size(ButtonSize::Small)
                    .build(&t),
            );
        })
        .id();
    commands.entity(area).add_child(bar);
}

fn on_exit_isolate(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    if q.get(a.entity).is_ok_and(|n| n.as_str() == "exit-isolate") {
        commands.queue(|world: &mut World| world.resource_mut::<PartCache>().isolate(None));
    }
}

fn exit_isolate_on_escape(keys: Res<ButtonInput<KeyCode>>, mut cache: ResMut<PartCache>) {
    if cache.isolated.is_some() && keys.just_pressed(KeyCode::Escape) {
        cache.isolate(None);
    }
}
