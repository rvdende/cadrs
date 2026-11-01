//! The app menu: the cadrs mark at the top left opens it, on the documents page and in a
//! document. In a document it has the way back to the documents and what the document menu
//! had (rename, export, a version, the workspaces, the units, a full rebuild); on the documents
//! page, the documents folder, the labels and the Trash. Both end with Preferences, the
//! keyboard shortcuts and GitHub (the repository, in the browser).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use cadrs_ui::i18n::tr;
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, MenuAction, MenuEntry, MenuItem, open_menu};

use crate::AppState;

pub struct AppMenuPlugin;

impl Plugin for AppMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_app_menu)
            .add_observer(on_app_menu_action);
    }
}

/// The cadrs mark that opens the app menu.
#[derive(Component)]
struct AppMenuButton;

/// The cadrs mark, its name and a caret: the app menu's button.
pub fn spawn_app_button(bar: &mut ChildSpawnerCommands, t: &Theme) {
    bar.spawn((
        Button::new("app-menu")
            .ghost()
            .tooltip(tr("shell-app-menu"))
            .build(t),
        AppMenuButton,
    ))
    .entry::<Node>()
    .and_modify(|mut n| {
        n.height = Val::Px(32.0);
        n.padding = UiRect::new(Val::Px(4.0), Val::Px(8.0), Val::ZERO, Val::ZERO);
        n.column_gap = Val::Px(8.0);
    })
    .entity()
    .with_children(|b| {
        b.spawn((
            Node {
                width: Val::Px(24.0),
                height: Val::Px(24.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(Val::Px(t.radius)),
                ..default()
            },
            BackgroundColor(t.primary),
            Pickable::IGNORE,
        ))
        .with_child((icon("part", 16.0, t.primary_foreground), Pickable::IGNORE));
        b.spawn((
            t.text("cadrs", t.font_xl, FontWeight::SEMIBOLD, t.foreground),
            cadrs_ui::style::InheritFg,
            Pickable::IGNORE,
        ));
        b.spawn((
            icon("caret-down", 12.0, t.muted_foreground),
            cadrs_ui::style::InheritFg,
            Pickable::IGNORE,
        ));
    });
}

/// What the app menu ends with, everywhere.
fn app_entries() -> Vec<MenuEntry> {
    vec![
        MenuItem::new("app-preferences", tr("shell-preferences-ellipsis"))
            .icon("settings")
            .into(),
        MenuItem::new("app-shortcuts", tr("shell-app-menu-shortcuts"))
            .icon("keyboard")
            .shortcut("Shift+/")
            .into(),
        MenuEntry::Separator,
        MenuItem::new("app-github", tr("shell-app-menu-github"))
            .icon("github")
            .into(),
    ]
}

/// The document's part of the app menu; `workspaces` lists Main and the branches to switch to
/// (their items are `crate::workspaces`'s).
fn document_entries(workspaces: Vec<MenuEntry>) -> Vec<MenuEntry> {
    // Main alone: how to get another.
    let mut workspaces = workspaces;
    if workspaces.is_empty() {
        workspaces.push(
            MenuItem::new("workspace-item-0", "Main")
                .icon("location")
                .checked(true)
                .into(),
        );
    }
    if workspaces.len() <= 1 {
        workspaces.push(MenuEntry::Separator);
        workspaces.push(
            MenuItem::new(
                "document-menu-workspace-hint",
                tr("shell-app-menu-workspace-hint"),
            )
            .icon("history")
            .disabled(true)
            .into(),
        );
    }
    let workspace = MenuItem::new("document-menu-workspaces", tr("shell-app-menu-workspace"))
        .icon("branches")
        .submenu(workspaces);
    vec![
        MenuItem::new("app-documents", tr("shell-app-menu-all-documents"))
            .icon("home")
            .into(),
        MenuEntry::Separator,
        MenuItem::new("document-menu-rename", tr("shell-app-menu-rename-document"))
            .icon("edit")
            .into(),
        MenuItem::new("document-menu-export", tr("shell-app-menu-export-document"))
            .icon("file-export")
            .into(),
        MenuItem::new(
            "document-menu-create-version",
            tr("shell-app-menu-create-version"),
        )
        .icon("versions")
        .into(),
        workspace.into(),
        MenuEntry::Separator,
        MenuItem::new("document-menu-units", tr("shell-app-menu-workspace-units"))
            .icon("measure")
            .into(),
        MenuItem::new(
            "document-menu-full-rebuild",
            tr("shell-app-menu-full-rebuild"),
        )
        .icon("sync-document")
        .into(),
        MenuEntry::Separator,
    ]
}

#[allow(clippy::too_many_arguments)]
fn on_app_menu(
    a: On<Activate>,
    q: Query<(), With<AppMenuButton>>,
    theme: Res<Theme>,
    app_state: Res<State<AppState>>,
    lib: Res<crate::landing::DocLibrary>,
    landing: Res<crate::landing::LandingState>,
    log: Option<Res<crate::history_panel::DocLog>>,
    mut commands: Commands,
) {
    if !q.contains(a.entity) {
        return;
    }
    let mut entries = match app_state.get() {
        AppState::Document => {
            let workspaces = log
                .as_ref()
                .and_then(|l| l.log.as_ref())
                .map(|l| {
                    let current = l.current_workspace();
                    l.workspace_ids()
                        .into_iter()
                        .enumerate()
                        .map(|(i, ws)| {
                            MenuItem::new(format!("workspace-item-{i}"), l.workspace_name(ws))
                                .icon(if ws.is_main() { "location" } else { "branches" })
                                .checked(ws == current)
                                .into()
                        })
                        .collect()
                })
                .unwrap_or_default();
            document_entries(workspaces)
        }
        _ => crate::landing::app_entries(&lib.lib, landing.filter),
    };
    entries.extend(app_entries());
    let menu = cadrs_ui::Menu::new("app-menu-popup")
        .min_width(240.0)
        .entries(entries);
    open_menu(&mut commands, a.entity, menu.build(&theme));
}

fn on_app_menu_action(
    ev: On<MenuAction>,
    q: Query<(), With<AppMenuButton>>,
    q_name: Query<Entity, With<crate::document::DocumentName>>,
    mut commands: Commands,
) {
    if !q.contains(ev.entity) {
        return;
    }
    match ev.item.as_str() {
        "app-preferences" => commands.queue(crate::preferences_ui::open_preferences),
        "app-shortcuts" => commands.queue(crate::shortcuts::open_shortcuts),
        "app-github" => crate::open_url(crate::GITHUB),
        "app-documents" => commands.queue(crate::thumbnail::close_document),
        "document-menu-export" => commands.queue(crate::native_document::open_export),
        "document-menu-units" => commands.queue(crate::units_dialog::open_units_dialog),
        "document-menu-full-rebuild" => commands.queue(crate::document::full_rebuild),
        // P3D.3: a version of the document as it is.
        "document-menu-create-version" => commands.queue(|world: &mut World| {
            crate::linked::open_create_version_dialog(world, crate::linked::VersionTarget::Current)
        }),
        "document-menu-rename" => {
            // The same as clicking the name.
            for e in &q_name {
                commands.trigger(Activate { entity: e });
            }
        }
        item => {
            let item = item.to_string();
            commands.queue(move |world: &mut World| {
                crate::landing::app_action(world, &item);
            });
        }
    }
}
