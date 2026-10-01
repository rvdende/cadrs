//! P3E.4 (TD12.4–TD12.7, TD3.7, X9): workspaces, branches and merge.
//!
//! - **Workspaces** are the document history's named heads ([`HistoryLog`]): Main and the
//!   branches. The open one is the current workspace of the log; the document file holds its
//!   state and its metadata its name (the documents page shows it beside the name, TD3.7).
//!   Each workspace has its own entries and its own undo and redo ([`WorkspaceUndos`]).
//! - **Switching** ([`switch_workspace`]): from the document header's workspace name (a menu
//!   of the workspaces, the current one checked) or the History panel's other-workspace rows.
//! - **Branch to create workspace…** (TD12.4) on a version's menu only: a dialog for the name
//!   and an optional description; the branch starts as an exact copy of the version (the same
//!   tab, feature and part ids) and is opened.
//! - **Merge into current workspace…** (TD12.6) on another workspace's row: a dialog lists the
//!   tabs the source changed, each set to **Replace** with the source's (the default) or
//!   **Keep** the current workspace's. Merge applies [`MergeWorkspace`]: one history entry
//!   ("Merge from …") and one undo step; Restore to the entry before it undoes it (TD12.7).

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use cadrs_core::history_log::{HistoryLog, VersionId, WorkspaceId};
use cadrs_core::workspace_merge::{self as wm, ChangedTab, MergeWorkspace, TabChange};
use cadrs_core::{DocumentId, ElementId};
use cadrs_ui::menu::{Menu, MenuAction, MenuItem, open_context_menu, open_menu};
use cadrs_ui::{Dialog, DialogClose, Select, SelectState, TextInput, Theme, show_toast};

use crate::history_panel::{DocLog, HistoryPanel, editing};
use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile, WorkspaceUndo};

pub struct WorkspacesPlugin;

impl Plugin for WorkspacesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorkspaceUndos>()
            .add_observer(on_button)
            .add_observer(on_switcher_menu)
            .add_systems(OnExit(AppState::Document), |mut u: ResMut<WorkspaceUndos>| u.0.clear());
    }
}

/// The undo and redo of the workspaces that aren't open, by document and workspace.
#[derive(Resource, Default)]
pub struct WorkspaceUndos(HashMap<(DocumentId, WorkspaceId), WorkspaceUndo>);

/// The open document's log, if any.
fn log(world: &World) -> Option<&HistoryLog> {
    world.get_resource::<DocLog>()?.log.as_ref()
}

/// The current workspace's name ("Main" without a log).
pub fn current_name(world: &World) -> String {
    log(world).map_or_else(|| cadrs_core::history_log::MAIN_NAME.to_string(), |l| l.current_name())
}

fn store(world: &World) -> Option<cadrs_core::Store> {
    world.get_resource::<DocumentStore>().map(|s| s.0.clone())
}

/// Opens workspace `ws`: the document becomes its state, with its own undo and redo; the log
/// and the document file are written (the file holds the open workspace, TD3.7). Nothing while
/// a dialog or a sketch is open, or the document is read-only.
pub fn switch_workspace(world: &mut World, ws: WorkspaceId) {
    if editing(world) || world.get_resource::<ActiveDocument>().is_none_or(|d| d.read_only.is_some()) {
        return;
    }
    let now = world.get_resource::<AppClock>().map_or(0, |c| c.now());
    let user = world.get_resource::<UserProfile>().map(|u| u.id.clone()).unwrap_or_default();
    let store = store(world);
    let (doc, doc_id, stored) = {
        let d = world.resource::<ActiveDocument>();
        (d.doc.clone(), d.doc.id, d.meta.is_some())
    };
    let (from, state, name) = {
        let mut l = world.resource_mut::<DocLog>();
        let Some(log) = l.log.as_mut() else { return };
        let from = log.current_workspace();
        if from == ws || !log.workspace_ids().contains(&ws) {
            return;
        }
        // Anything not yet in the log goes in first (the log ends at the document).
        if log.head() != &doc {
            log.catch_up(&doc, now, &user);
        }
        let Some(state) = log.switch_to(ws) else { return };
        if let Some(s) = store.as_ref().filter(|_| stored) {
            let _ = log.save(s);
        }
        (from, state, log.current_name())
    };
    let undo = world.resource_mut::<WorkspaceUndos>().0.remove(&(doc_id, ws)).unwrap_or_default();
    let mut active = world.resource_mut::<ActiveDocument>();
    let old = active.switch_workspace(state, undo);
    if let Some(meta) = active.meta.as_mut() {
        meta.workspace = (!ws.is_main()).then_some(name);
    }
    if let Some(s) = store.as_ref()
        && let Err(e) = active.save_now(s)
    {
        warn!("switch workspace: {e}");
    }
    let stacks = (active.history.undo_len(), active.history.redo_len());
    world.resource_mut::<WorkspaceUndos>().0.insert((doc_id, from), old);
    world.resource_mut::<DocLog>().workspace_switched(stacks);
    let mut panel = world.resource_mut::<HistoryPanel>();
    panel.selected = None;
    panel.selected_version = None;
    panel.expanded.clear();
}

// ---------------------------------------------------------------------------------------------
// The header's switcher

/// The workspace menu under the document header's workspace name: every workspace, the current
/// one checked.
pub fn open_switcher(world: &mut World, anchor: Entity) {
    let Some(l) = log(world) else { return };
    let current = l.current_workspace();
    let mut menu = Menu::new("workspace-menu").min_width(200.0).item_height(24.0);
    for (i, ws) in l.workspace_ids().into_iter().enumerate() {
        menu = menu.item(MenuItem::new(format!("workspace-item-{i}"), l.workspace_name(ws)).icon(if ws.is_main() { "location" } else { "branches" }).checked(ws == current));
    }
    // Under the header bar (not over the name it was opened from), lined up with the name.
    let at = (|| {
        let node = |e: Entity| -> Option<(Vec2, Vec2)> {
            let n = world.get::<ComputedNode>(e)?;
            let tf = world.get::<UiGlobalTransform>(e)?;
            let s = n.inverse_scale_factor();
            Some(((tf.translation - n.size() / 2.0) * s, (tf.translation + n.size() / 2.0) * s))
        };
        let (lo, _) = node(anchor)?;
        let bar = world.get::<ChildOf>(anchor)?.parent();
        let (_, hi) = node(bar)?;
        Some(Vec2::new(lo.x, hi.y + 4.0))
    })();
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let e = match at {
        Some(at) => open_context_menu(&mut commands, at, menu.build(&theme)),
        None => open_menu(&mut commands, anchor, menu.build(&theme)),
    };
    commands.entity(e).insert(DespawnOnExit(AppState::Document));
    world.flush();
}

fn on_switcher_menu(ev: On<MenuAction>, mut commands: Commands) {
    let Some(i) = ev.item.strip_prefix("workspace-item-").and_then(|n| n.parse::<usize>().ok()) else {
        return;
    };
    commands.queue(cadrs_ui::menu::close_all_menus);
    commands.queue(move |world: &mut World| {
        if let Some(ws) = log(world).and_then(|l| l.workspace_ids().get(i).copied()) {
            switch_workspace(world, ws);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Branch to create workspace…

/// The version a branch dialog branches from.
#[derive(Component, Clone, Copy)]
struct BranchFrom(VersionId);

/// **Branch to create workspace** (TD12.4): the dialog for version `i` (its index in the log).
pub fn open_branch_dialog(world: &mut World, i: usize) {
    if editing(world) {
        return;
    }
    let Some((v, vname, next)) = log(world).and_then(|l| l.versions().get(i).map(|v| (v.id(), v.name().to_string(), l.next_branch_name()))) else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("branch-dialog")
            .width(520.0)
            .title(format!("Branch to create workspace from {vname}"))
            .title_font(theme.font_lg, FontWeight::NORMAL)
            .body(move |b| {
                let t = &tb;
                b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|r| {
                    r.spawn(t.text("Name:", t.font_base, FontWeight::BOLD, t.foreground));
                    r.spawn(TextInput::new("branch-name").value(next).select_all_on_focus().autofocus().width(Val::Px(320.0)).height(30.0).build(t));
                });
                b.spawn(Node { margin: UiRect::top(Val::Px(10.0)), ..default() })
                    .with_child(t.text("Description:", t.font_base, FontWeight::BOLD, t.foreground));
                b.spawn(TextInput::new("branch-description").placeholder("Description").max_characters(10_000).width(Val::Percent(100.0)).height(54.0).build(t));
                b.spawn(Node { margin: UiRect::top(Val::Px(8.0)), ..default() }).with_child(t.text(
                    "The new workspace starts as a copy of the version. Main is not changed.",
                    t.font_sm,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn(cadrs_ui::Button::new("branch-create").label("Create").primary().build(t));
                f.spawn(cadrs_ui::Button::new("branch-cancel").label("Cancel").build(t));
            })
            .build(&theme),
        BranchFrom(v),
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

fn field(world: &mut World, name: &str) -> String {
    let mut q = world.query::<(&Name, &bevy::text::EditableText)>();
    q.iter(world).find(|(n, _)| n.as_str() == name).map(|(_, t)| t.value().to_string()).unwrap_or_default()
}

fn close<C: Component>(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<C>>();
    let dialogs: Vec<Entity> = q.iter(world).collect();
    for d in dialogs {
        world.trigger(DialogClose { entity: d });
    }
}

/// Create: the branch is made, stored and opened.
fn commit_branch(world: &mut World) {
    let mut q = world.query::<&BranchFrom>();
    let Some(BranchFrom(v)) = q.iter(world).next().copied() else { return };
    let name = field(world, "branch-name-field");
    let description = field(world, "branch-description-field");
    close::<BranchFrom>(world);
    let now = world.get_resource::<AppClock>().map_or(0, |c| c.now());
    let user = world.get_resource::<UserProfile>().map(|u| u.id.clone()).unwrap_or_default();
    let store = store(world);
    let stored = world.get_resource::<ActiveDocument>().is_some_and(|d| d.meta.is_some());
    let ws = {
        let mut l = world.resource_mut::<DocLog>();
        let Some(log) = l.log.as_mut() else { return };
        let Some(ws) = log.branch(v, &name, &description, now, &user) else { return };
        if let Some(s) = store.as_ref().filter(|_| stored) {
            let _ = log.save(s);
        }
        ws
    };
    world.resource_mut::<DocLog>().generation += 1;
    switch_workspace(world, ws);
}

// ---------------------------------------------------------------------------------------------
// Merge into current workspace…

/// What a merge dialog merges: the source and its changed tabs (in the dialog's order).
#[derive(Component, Clone)]
struct MergeFrom {
    source: WorkspaceId,
    tabs: Vec<ElementId>,
}

/// **Merge into current workspace** (TD12.6): the dialog for merging workspace `source` into
/// the current one, listing the changed tabs.
pub fn open_merge_dialog(world: &mut World, source: WorkspaceId) {
    if editing(world) || world.get_resource::<ActiveDocument>().is_none_or(|d| d.read_only.is_some()) {
        return;
    }
    let dest_doc = world.resource::<ActiveDocument>().doc.clone();
    let Some((src_doc, base, sname, dname)) = log(world).and_then(|l| {
        let dest = l.current_workspace();
        (source != dest).then_some(())?;
        Some((l.workspace_head(source)?, l.merge_base(source, dest), l.workspace_name(source), l.current_name()))
    }) else {
        return;
    };
    let changed = wm::changed_tabs(base.as_ref(), &src_doc, &dest_doc);
    let theme = world.resource::<Theme>().clone();
    if changed.is_empty() {
        let mut commands = world.commands();
        show_toast(&mut commands, &theme, format!("Nothing to merge: {sname} has no changed tabs."));
        world.flush();
        return;
    }
    let tabs = changed.iter().map(|c| c.element).collect();
    let (tb, tf) = (theme.clone(), theme.clone());
    let (s2, d2) = (sname.clone(), dname.clone());
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("merge-dialog")
            .width(660.0)
            .title(format!("Merge {sname} into {dname}"))
            .title_font(theme.font_lg, FontWeight::NORMAL)
            .body(move |b| merge_body(b, &tb, &changed, &s2, &d2))
            .footer(move |f| {
                let t = &tf;
                f.spawn(cadrs_ui::Button::new("merge-accept").label("Merge").primary().build(t));
                f.spawn(cadrs_ui::Button::new("merge-cancel").label("Cancel").build(t));
            })
            .build(&theme),
        MergeFrom { source, tabs },
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

fn merge_body(b: &mut ChildSpawner, t: &Theme, changed: &[ChangedTab], source: &str, dest: &str) {
    let intro = format!("{source} changed these tabs. For each, replace it with the branch's version or keep {dest}'s.");
    b.spawn(Node { margin: UiRect::bottom(Val::Px(10.0)), max_width: Val::Px(620.0), ..default() }).with_children(|p| {
        p.spawn(t.text(intro, t.font_base, FontWeight::NORMAL, t.foreground)).insert(TextLayout::default());
    });
    let line = Color::srgb_u8(0xdd, 0xdd, 0xdd);
    // The table: Tab | Change | Action.
    b.spawn((
        Name::new("merge-tabs"),
        Node { flex_direction: FlexDirection::Column, border: UiRect::all(Val::Px(1.0)), ..default() },
        BorderColor::all(line),
    ))
    .with_children(|tbl| {
        tbl.spawn((
            Node { height: Val::Px(26.0), align_items: AlignItems::Center, padding: UiRect::horizontal(Val::Px(8.0)), border: UiRect::bottom(Val::Px(1.0)), ..default() },
            BackgroundColor(Color::srgb_u8(0xf5, 0xf5, 0xf5)),
            BorderColor::all(line),
        ))
        .with_children(|h| {
            h.spawn((t.text("Tab", t.font_sm, FontWeight::BOLD, t.muted_foreground), Node { flex_grow: 1.0, ..default() }));
            h.spawn((t.text("Change", t.font_sm, FontWeight::BOLD, t.muted_foreground), Node { width: Val::Px(80.0), ..default() }));
            h.spawn((t.text("Action", t.font_sm, FontWeight::BOLD, t.muted_foreground), Node { width: Val::Px(300.0), ..default() }));
        });
        for (i, c) in changed.iter().enumerate() {
            let last = i + 1 == changed.len();
            tbl.spawn((
                Name::new(format!("merge-tab-row-{i}")),
                Node {
                    height: Val::Px(34.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    column_gap: Val::Px(6.0),
                    border: UiRect::bottom(Val::Px(if last { 0.0 } else { 1.0 })),
                    ..default()
                },
                BorderColor::all(line),
            ))
            .with_children(|r| {
                r.spawn(cadrs_ui::icon(if c.assembly { "assembly" } else { "part-studio" }, 16.0, t.tool_foreground));
                r.spawn((t.text(c.name.clone(), t.font_base, FontWeight::MEDIUM, t.foreground), Node { flex_grow: 1.0, ..default() }));
                let change = match c.change {
                    TabChange::Changed => "Modified",
                    TabChange::Added => "Added",
                    TabChange::Deleted => "Deleted",
                };
                r.spawn((t.text(change, t.font_sm, FontWeight::NORMAL, t.muted_foreground), Node { width: Val::Px(80.0), ..default() }));
                r.spawn(Node { width: Val::Px(300.0), ..default() }).with_child(
                    Select::new(format!("merge-tab-{i}"))
                        .bordered()
                        .option(format!("Replace with {source}"), true)
                        .option(format!("Keep {dest}"), true)
                        .selected(0)
                        // Up, so the list doesn't cover the dialog's Merge and Cancel.
                        .open_up()
                        .width(Val::Px(292.0))
                        .build(t),
                );
            });
        }
    });
}

/// Merge: the chosen tabs replaced, as one step; the dialog closes.
fn commit_merge(world: &mut World) {
    let mut q = world.query::<&MergeFrom>();
    let Some(from) = q.iter(world).next().cloned() else { return };
    let choices: HashMap<String, usize> = {
        let mut q = world.query::<(&Name, &SelectState)>();
        q.iter(world).map(|(n, s)| (n.to_string(), s.selected)).collect()
    };
    close::<MergeFrom>(world);
    let replace: Vec<ElementId> = from.tabs.iter().enumerate().filter(|(i, _)| choices.get(&format!("merge-tab-{i}")).copied().unwrap_or(0) == 0).map(|(_, id)| *id).collect();
    // Keep for every tab: there is nothing to merge, so no (empty) "Merge from …" entry.
    if replace.is_empty() {
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        show_toast(&mut commands, &theme, "Nothing to merge");
        world.flush();
        return;
    }
    let Some((source, name)) = log(world).and_then(|l| Some((l.workspace_head(from.source)?, l.workspace_name(from.source)))) else {
        return;
    };
    let mut active = world.resource_mut::<ActiveDocument>();
    let merged = wm::merge(&active.doc, &source, &replace);
    if let Err(e) = active.execute(&MergeWorkspace { state: Box::new(merged), source: name }) {
        warn!("merge: {e}");
    }
}

fn on_button(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(n) = q.get(a.entity) else { return };
    match n.as_str() {
        "branch-create" => commands.queue(commit_branch),
        "branch-cancel" => commands.queue(close::<BranchFrom>),
        "merge-accept" => commands.queue(commit_merge),
        "merge-cancel" => commands.queue(close::<MergeFrom>),
        "branch-label" => {
            let e = a.entity;
            commands.queue(move |world: &mut World| open_switcher(world, e));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_core::samples::gasket as g;

    fn depth(world: &World, el: ElementId, f: cadrs_core::FeatureId) -> f64 {
        world.resource::<ActiveDocument>().doc.element(el).and_then(|e| e.feature(f)).and_then(|f| f.extrude()).map(|e| e.depth).unwrap()
    }

    /// P3E.4 judge: each workspace keeps its own undo across switches: back in Main, an undo
    /// undoes Main's last edit (not the branch's).
    #[test]
    fn an_undo_in_main_after_switching_back_undoes_mains_last_edit() {
        let mut world = World::new();
        let doc = g::document().unwrap();
        let mut log = HistoryLog::start(&doc, 1_000, "me");
        let v1 = log.create_version("V1", "", 1_010, "me");
        let branch = log.branch(v1, "Alternate Gasket Thickness", "", 1_020, "me").unwrap();
        world.insert_resource(ActiveDocument::new(doc));
        let mut doc_log = DocLog::default();
        doc_log.log = Some(log);
        world.insert_resource(doc_log);
        world.init_resource::<WorkspaceUndos>();
        world.init_resource::<HistoryPanel>();
        // Main: the Manifold 20 → 25 mm.
        g::set_depth(world.resource_mut::<ActiveDocument>().as_mut(), g::MANIFOLD, g::MANIFOLD_EXTRUDE, 25.0).unwrap();
        // The branch: the gasket 2 → 1 mm.
        switch_workspace(&mut world, branch);
        assert_eq!(current_name(&world), "Alternate Gasket Thickness");
        assert_eq!(depth(&world, g::MANIFOLD, g::MANIFOLD_EXTRUDE), g::MANIFOLD_T, "the branch starts at V1");
        assert_eq!(world.resource::<ActiveDocument>().history.undo_len(), 0, "the branch has no undo of Main's");
        g::set_depth(world.resource_mut::<ActiveDocument>().as_mut(), g::GASKET, g::GASKET_EXTRUDE, g::THIN_GASKET_T).unwrap();
        // Back to Main: its undo undoes the Manifold's edit; the gasket is Main's 2 mm.
        switch_workspace(&mut world, WorkspaceId::MAIN);
        assert_eq!(depth(&world, g::MANIFOLD, g::MANIFOLD_EXTRUDE), 25.0);
        assert_eq!(depth(&world, g::GASKET, g::GASKET_EXTRUDE), g::GASKET_T);
        assert!(world.resource_mut::<ActiveDocument>().undo().is_some());
        assert_eq!(depth(&world, g::MANIFOLD, g::MANIFOLD_EXTRUDE), g::MANIFOLD_T, "Main's last edit undone");
        assert_eq!(depth(&world, g::GASKET, g::GASKET_EXTRUDE), g::GASKET_T);
        // And the branch kept its own: switching to it and undoing takes its gasket back to 2.
        switch_workspace(&mut world, branch);
        assert_eq!(depth(&world, g::GASKET, g::GASKET_EXTRUDE), g::THIN_GASKET_T);
        assert!(world.resource_mut::<ActiveDocument>().undo().is_some());
        assert_eq!(depth(&world, g::GASKET, g::GASKET_EXTRUDE), g::GASKET_T);
    }
}
