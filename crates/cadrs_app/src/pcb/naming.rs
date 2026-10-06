//! Creating and naming boards and components from the PCB Studio panel: + on the Boards and
//! Components headers adds one ("Board n", "Component n") with its name ready to edit in place;
//! Enter keeps the name (create and name are one undo step), Esc keeps the default.
//! Double-clicking a board or component row renames it.
//!
//! Names: `pcb-add-board`, `pcb-add-component` (the + buttons), `pcb-name-edit` (the field).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use cadrs_core::ElementId;
use cadrs_core::pcb::{AddBoard, AddComponent, BoardId, ComponentId, RenameBoard, RenameComponent};
use cadrs_ui::inline_edit::{DoubleClick, InlineEditCommit, InlineEditOptions, begin_inline_edit};
use cadrs_ui::prelude::*;

use super::{BoardRow, NativeComponentRow, active_studio, show_board};
use crate::ActiveDocument;

/// What a name edit is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Board(ElementId, BoardId),
    Component(ElementId, ComponentId),
}

/// A row to start renaming once the tree shows it (right after +).
#[derive(Resource, Default)]
struct PendingRename(Option<Target>);

/// The item + just made, with the undo depth before it: renaming it right away joins the
/// create step.
#[derive(Resource, Default)]
struct JustCreated(Option<(Target, usize)>);

pub fn register(app: &mut App) {
    app.init_resource::<PendingRename>()
        .init_resource::<JustCreated>()
        .add_systems(Update, start_pending_rename)
        .add_observer(on_add)
        .add_observer(on_row_double_click)
        .add_observer(on_rename_commit);
}

fn on_add(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(a.entity) else { return };
    match name.as_str() {
        "pcb-add-board" => commands.queue(add_board),
        "pcb-add-component" => commands.queue(add_component),
        _ => {}
    }
}

/// + under Boards: a new board, shown, its name ready to edit.
pub fn add_board(world: &mut World) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some((el, _)) = active_studio(&doc) else { return };
    let mark = doc.history.undo_len();
    if let Err(e) = doc.execute(&AddBoard::new_board(el)) {
        warn!("create board: {e}");
        return;
    }
    let Some(id) = active_studio(&doc).and_then(|(_, s)| s.active) else { return };
    let target = Target::Board(el, id);
    world.resource_mut::<JustCreated>().0 = Some((target, mark));
    world.resource_mut::<PendingRename>().0 = Some(target);
    show_board(world, el, id);
}

/// + under Components: a new component, its name ready to edit.
pub fn add_component(world: &mut World) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some((el, _)) = active_studio(&doc) else { return };
    let mark = doc.history.undo_len();
    if let Err(e) = doc.execute(&AddComponent { element: el, name: None }) {
        warn!("create component: {e}");
        return;
    }
    let Some(id) = active_studio(&doc).and_then(|(_, s)| s.components.last().map(|c| c.id)) else { return };
    let target = Target::Component(el, id);
    world.resource_mut::<JustCreated>().0 = Some((target, mark));
    world.resource_mut::<PendingRename>().0 = Some(target);
}

fn row_target(e: Entity, q_board: &Query<&BoardRow>, q_comp: &Query<&NativeComponentRow>) -> Option<Target> {
    if let Ok(r) = q_board.get(e) {
        return Some(Target::Board(r.0, r.1));
    }
    q_comp.get(e).ok().map(|r| Target::Component(r.0, r.1))
}

fn start_pending_rename(mut pending: ResMut<PendingRename>, q_board: Query<(Entity, &BoardRow)>, q_comp: Query<(Entity, &NativeComponentRow)>, mut commands: Commands) {
    let Some(target) = pending.0 else { return };
    let row = match target {
        Target::Board(el, id) => q_board.iter().find(|(_, r)| r.0 == el && r.1 == id).map(|(e, _)| e),
        Target::Component(el, id) => q_comp.iter().find(|(_, r)| r.0 == el && r.1 == id).map(|(e, _)| e),
    };
    if let Some(row) = row {
        pending.0 = None;
        commands.queue(move |world: &mut World| begin_rename(world, row, target));
    }
}

fn current_name(world: &World, target: Target) -> Option<String> {
    let doc = world.get_resource::<ActiveDocument>()?;
    let (_, s) = active_studio(doc)?;
    match target {
        Target::Board(_, id) => s.board(id).map(|b| b.name().to_string()),
        Target::Component(_, id) => s.component(id).map(|c| c.component.name.clone()),
    }
}

fn begin_rename(world: &mut World, row: Entity, target: Target) {
    let Some(name) = current_name(world, target) else { return };
    let theme = world.resource::<Theme>().clone();
    let mut opts = InlineEditOptions::new("pcb-name-edit");
    opts.width = Val::Px(200.0);
    opts.height = 20.0;
    opts.font_size = Some(theme.font_sm);
    opts.weight = FontWeight::MEDIUM;
    opts.padding = Some(2.0);
    let mut commands = world.commands();
    begin_inline_edit(&mut commands, &theme, row, name, opts);
    world.flush();
}

fn on_row_double_click(ev: On<DoubleClick>, q_board: Query<&BoardRow>, q_comp: Query<&NativeComponentRow>, mut commands: Commands) {
    let Some(target) = row_target(ev.entity, &q_board, &q_comp) else { return };
    let row = ev.entity;
    commands.queue(move |world: &mut World| {
        // A later rename is its own step.
        world.resource_mut::<JustCreated>().0 = None;
        begin_rename(world, row, target);
    });
}

fn on_rename_commit(ev: On<InlineEditCommit>, q_board: Query<&BoardRow>, q_comp: Query<&NativeComponentRow>, mut commands: Commands) {
    if ev.entity != ev.original_event_target() {
        return;
    }
    let Some(target) = row_target(ev.entity, &q_board, &q_comp) else { return };
    let value = ev.value.trim().to_string();
    commands.queue(move |world: &mut World| rename(world, target, &value));
}

fn rename(world: &mut World, target: Target, value: &str) {
    let just = world.resource_mut::<JustCreated>().0.take();
    if value.is_empty() || current_name(world, target).as_deref() == Some(value) {
        return;
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let (r, label) = match target {
        Target::Board(element, board) => (doc.execute(&RenameBoard { element, board, name: value.into() }), "Create board"),
        Target::Component(element, component) => (doc.execute(&RenameComponent { element, component, name: value.into() }), "Create component"),
    };
    match r {
        Ok(()) => {
            if let Some((t, mark)) = just
                && t == target
                && doc.history.undo_len() == mark + 2
            {
                doc.squash_since(mark, label);
            }
        }
        Err(e) => {
            let theme = world.resource::<Theme>().clone();
            let mut commands = world.commands();
            cadrs_ui::show_toast_for(&mut commands, &theme, e.to_string(), 3.0);
            world.flush();
        }
    }
}
