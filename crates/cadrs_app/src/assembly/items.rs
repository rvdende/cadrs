//! The Instances list's **Items** group (P3B.8, `intro-to-assemblies.md` A1.7, X16): "Items (n)"
//! between the instances and Loads, listing the assembly's non-geometric items
//! ([`cadrs_core::assembly::items`]: glue, labels, a purchased item not modelled). Each is a row
//! with its quantity ("× 2"); they are rows of the Bill of Materials too.
//!
//! - The header's **+** (or its menu's **Add item…**) opens the Item dialog: **Name**,
//!   **Quantity**, **Unit** (of measure: Each, Liter, Gram, …, the BOM's Unit of measure column),
//!   **Part number**, **Description**. ✓ adds it, one undo step.
//! - A row's menu: **Edit…** (the dialog on it; also a double-click), **Delete**.
//!
//! Names: `assembly-items` (the header), `assembly-items-add`, rows `item-row-<k>`, the dialog
//! `item-dialog` with `item-name`, `item-quantity`, `item-unit`, `item-part-number`, `item-description`; menu
//! items `item-edit`, `item-delete`, `items-add`.

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use cadrs_core::ElementId;
use cadrs_core::assembly::items::{DeleteItems, Item, ItemId, SetItem};
use cadrs_ui::menu::{ContextMenuAnchor, Menu, MenuAction, MenuItem};
use cadrs_ui::prelude::*;
use cadrs_ui::{DoubleClick, DoubleClickable, FeatureDialogAccept, FeatureDialogCancel, open_context_menu};

use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (rebuild_rows, sync_dialog).after(crate::parts::PartsSet).run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| commands.remove_resource::<ItemSession>())
            .add_observer(on_add)
            .add_observer(on_row_menu)
            .add_observer(on_header_menu)
            .add_observer(on_menu_action)
            .add_observer(on_double_click)
            .add_observer(on_accept)
            .add_observer(on_cancel);
    }
}

/// The "Items (n)" header row (spawned by the document shell).
#[derive(Component)]
pub struct ItemsHeader;

/// The container of the item rows (spawned by the document shell, after the header).
#[derive(Component)]
pub struct ItemRows;

/// An item's row.
#[derive(Component, Debug, Clone, Copy)]
struct ItemRow(ItemId);

/// The Item dialog open: the item (new or edited).
#[derive(Resource, Debug, Clone)]
pub struct ItemSession {
    pub element: ElementId,
    pub item: Item,
    pub editing: bool,
}

#[derive(Component)]
struct ItemDialog;

#[derive(Component, Debug, Clone, Copy)]
enum RowMenu {
    Item(ItemId),
    Header,
}

type RowsKey = Vec<(ItemId, String, u32)>;

#[allow(clippy::too_many_arguments)]
fn rebuild_rows(
    doc: Option<Res<ActiveDocument>>,
    q_rows: Query<(Entity, Ref<ItemRows>)>,
    q_header: Query<(Entity, &Children), With<ItemsHeader>>,
    q_children: Query<&Children>,
    mut q_text: Query<&mut Text>,
    theme: Res<Theme>,
    mut last: Local<Option<RowsKey>>,
    mut commands: Commands,
) {
    let Some(doc) = doc else { return };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
    let key: RowsKey = model.items.iter().map(|i| (i.id, i.name.clone(), i.quantity)).collect();
    // The header's count.
    let count = format!("Items ({})", key.len());
    for (_, children) in &q_header {
        let mut stack: Vec<Entity> = children.iter().collect();
        while let Some(e) = stack.pop() {
            if let Ok(mut t) = q_text.get_mut(e)
                && t.0.starts_with("Items (")
                && t.0 != count
            {
                t.0 = count.clone();
            }
            if let Ok(c) = q_children.get(e) {
                stack.extend(c.iter());
            }
        }
    }
    let Some((container, added)) = q_rows.iter().next().map(|(e, r)| (e, r.is_added())) else { return };
    if !added && last.as_ref() == Some(&key) {
        return;
    }
    let t = theme.clone();
    commands.entity(container).despawn_children();
    commands.entity(container).with_children(|c| {
        for (k, (id, name, qty)) in key.iter().enumerate() {
            let row_name = format!("item-row-{}", k + 1);
            let mut row = c.spawn((
                TreeItem::new(row_name.clone(), name.clone()).icon("tag", 14.0).icon_color(t.muted_foreground).left(22.0).build(&t),
                ItemRow(*id),
                ContextMenuTarget,
                DoubleClickable,
                Tooltip::new(format!("{name}\nItem (no geometry), quantity {qty}; listed in the Bill of Materials")),
            ));
            row.entry::<Node>().and_modify(|mut n| {
                n.height = Val::Px(22.0);
                n.column_gap = Val::Px(5.0);
                n.margin = UiRect::new(Val::Px(2.0), Val::Px(7.0), Val::Px(1.0), Val::Px(1.0));
            });
            row.with_children(|r| {
                r.spawn((Name::new(format!("{row_name}-quantity")), t.text(format!("× {qty}"), t.font_sm, FontWeight::NORMAL, t.muted_foreground), Node { flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
            });
        }
    });
    *last = Some(key);
}

/// Opens the Item dialog: a new item, or `item` to edit.
pub fn open(world: &mut World, item: Option<ItemId>) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
    let (item, editing) = match item.and_then(|i| model.item(i).cloned()) {
        Some(i) => (i, true),
        None => (Item::new(ItemId::new(), format!("Item {}", model.items.len() + 1), 1), false),
    };
    world.insert_resource(ItemSession { element, item, editing });
}

fn on_add(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    if q.get(a.entity).map(|n| n.as_str()) == Ok("assembly-items-add") {
        commands.queue(|world: &mut World| open(world, None));
    }
}

fn on_double_click(ev: On<DoubleClick>, q: Query<&ItemRow>, mut commands: Commands) {
    if let Ok(r) = q.get(ev.entity).copied() {
        commands.queue(move |world: &mut World| open(world, Some(r.0)));
    }
}

fn menu_at(world: &mut World, at: Vec2, menu: Menu, what: RowMenu) {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
    commands.entity(anchor).insert((what, DespawnOnExit(AppState::Document)));
    world.flush();
}

fn on_row_menu(ev: On<ContextMenuRequested>, q: Query<&ItemRow>, mut commands: Commands) {
    let Ok(r) = q.get(ev.entity).copied() else { return };
    let (at, e) = (ev.position, ev.entity);
    commands.queue(move |world: &mut World| {
        let at = super::mates_list::beside_row(world, e, at);
        let menu = Menu::new("item-menu")
            .min_width(140.0)
            .item_height(20.0)
            .item(MenuItem::new("item-edit", "Edit…").icon("edit"))
            .separator()
            .item(MenuItem::new("item-delete", "Delete").icon("remove-circle"));
        menu_at(world, at, menu, RowMenu::Item(r.0));
    });
}

fn on_header_menu(ev: On<ContextMenuRequested>, q: Query<(), With<ItemsHeader>>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    let (at, e) = (ev.position, ev.entity);
    commands.queue(move |world: &mut World| {
        let at = super::mates_list::beside_row(world, e, at);
        let menu = Menu::new("items-menu").min_width(140.0).item_height(20.0).item(MenuItem::new("items-add", "Add item…").icon("plus"));
        menu_at(world, at, menu, RowMenu::Header);
    });
}

fn on_menu_action(ev: On<MenuAction>, q: Query<&RowMenu, With<ContextMenuAnchor>>, mut commands: Commands) {
    let Ok(m) = q.get(ev.entity).copied() else { return };
    let item = ev.item.clone();
    commands.queue(move |world: &mut World| match (m, item.as_str()) {
        (RowMenu::Header, "items-add") => open(world, None),
        (RowMenu::Item(id), "item-edit") => open(world, Some(id)),
        (RowMenu::Item(id), "item-delete") => {
            if let Some(element) = world.get_resource::<ActiveDocument>().and_then(super::active_assembly) {
                super::run(world, &DeleteItems { element, items: vec![id] });
            }
        }
        _ => {}
    });
}

fn field_text(world: &mut World, name: &str) -> Option<String> {
    let field = format!("{name}-field");
    let mut q = world.query::<(&Name, &EditableText)>();
    q.iter(world).find(|(n, _)| n.as_str() == field).map(|(_, t)| t.value().to_string())
}

fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<ItemSession>().cloned() else { return };
    let mut item = s.item.clone();
    if let Some(n) = field_text(world, "item-name") {
        item.name = n.trim().to_string();
    }
    if let Some(q) = field_text(world, "item-quantity") {
        match q.trim().parse::<u32>() {
            Ok(v) => item.quantity = v,
            Err(_) => return,
        }
    }
    let opt = |v: Option<String>| v.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    item.properties.part_number = opt(field_text(world, "item-part-number"));
    // Unit of measure: one of the BOM's units ("Each" when empty).
    if let Some(u) = opt(field_text(world, "item-unit")) {
        let Some(known) = cadrs_core::properties::UNITS_OF_MEASURE.iter().find(|x| x.eq_ignore_ascii_case(&u)) else { return };
        item.properties.unit_of_measure = (*known != "Each").then(|| known.to_string());
    } else {
        item.properties.unit_of_measure = None;
    }
    item.properties.description = opt(field_text(world, "item-description"));
    if super::run(world, &SetItem { element: s.element, item }) {
        world.remove_resource::<ItemSession>();
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<ItemDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<ItemDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            world.remove_resource::<ItemSession>();
        });
    }
}

fn sync_dialog(session: Option<Res<ItemSession>>, theme: Res<Theme>, q_dialog: Query<Entity, With<ItemDialog>>, q_area: Query<Entity, With<ViewportArea>>, mut commands: Commands) {
    let Some(s) = session else {
        for e in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    if !q_dialog.is_empty() {
        return;
    }
    let Some(area) = q_area.iter().next() else { return };
    let t = theme.clone();
    let it = s.item.clone();
    let d = commands
        .spawn((
            ItemDialog,
            DespawnOnExit(AppState::Document),
            FeatureDialog::new("item-dialog")
                .title(if s.editing { format!("Edit {}", it.name) } else { "Add item".to_string() })
                .width(240.0)
                .body(move |b| {
                    let rows = [
                        ("item-name", "Name", it.name.clone()),
                        ("item-quantity", "Quantity", it.quantity.to_string()),
                        ("item-unit", "Unit", it.properties.unit_of_measure.clone().unwrap_or_else(|| "Each".into())),
                        ("item-part-number", "Part number", it.properties.part_number.clone().unwrap_or_default()),
                        ("item-description", "Description", it.properties.description.clone().unwrap_or_default()),
                    ];
                    for (name, label, value) in rows {
                        b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), margin: UiRect::vertical(Val::Px(2.0)), ..default() }).with_children(|r| {
                            r.spawn((t.text(label, t.font_sm, FontWeight::MEDIUM, t.foreground), Node { width: Val::Px(74.0), flex_shrink: 0.0, ..default() }));
                            let mut input = TextInput::new(name).value(value).height(24.0).width(Val::Px(140.0)).select_all_on_focus();
                            if name == "item-name" {
                                input = input.autofocus();
                            }
                            r.spawn(input.build(&t));
                        });
                    }
                })
                .build(&theme),
        ))
        .id();
    commands.entity(area).add_child(d);
}
