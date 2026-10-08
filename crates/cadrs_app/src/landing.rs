//! The documents page (`reference/onshape/screens/01*`–`04`).
//!
//! - Top bar: logo, search box ("Search in Owned by me"), notifications, apps, help and the
//!   user's avatar and name.
//! - Sidebar: the blue **Create ▾** button (Document…, Folder…, Import files…, …), the filters
//!   (Owned by me, Recently opened, Created by me, Shared with me, Teams, Labels, Public, Trash)
//!   and a storage note at the bottom.
//! - Main area: a heading for the active filter, collapsible sections, and the document list
//!   with sortable columns Name | Modified | Modified by | Owned by.
//!
//! Clicking a row selects it and shows it in the **Details** panel; double-clicking it (or Enter)
//! opens it (P3E.1). Right-clicking it selects it and opens a context menu with Rename… (a
//! dialog), Labels ▸, Move to trash (a confirmation dialog); in Trash, Restore. Rename, trash
//! and restore are [`LibraryCommand`]s, so Ctrl+Z / Ctrl+Shift+Z undo and redo them.
//!
//! P3E.1 (TD3, TD4.1, X5; `test-drive-gaps.md`) adds, in [`details`]: labels (Create ▸
//! Label…, the sidebar's Labels section as filters with Rename… and Delete…, a Labels column,
//! the details panel's searchable checkboxes and "Create new label"), the Details panel (Info,
//! Versions, Where used), folders that open (a breadcrumb and Back), the Type filter and a grid
//! view, the bundled samples under Explore cadrs ("Open a copy"), and Import files… (a STEP or
//! STL file becomes a new document whose Part Studio holds its Import). Every library change is
//! an undoable library command.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::ScrollArea;
use cadrs_core::library::{
    AddEntry, CreateFolder, PurgeEntry, RenameEntry, RestoreEntry, TrashEntry,
};
use cadrs_core::{
    Document, DocumentEntry, DocumentId, DocumentMeta, Filter, FolderId, ItemType, LabelId,
    Library, LibraryCommand, LibraryHistory, SortDir, SortKey,
};
use cadrs_ui::input::{TextInputField, TextInputPlaceholder};
use cadrs_ui::menu::ContextMenuAnchor;
use cadrs_ui::prelude::*;
use cadrs_ui::icon::icon_in;
use cadrs_ui::{Button, CollapsibleToggled, Icon};

use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};

#[path = "landing_details.rs"]
mod details;
pub use details::{DetailsTab, LandingDetails, Pick};
pub use details::import_file as import_mesh_document;

pub struct LandingPlugin;

impl Plugin for LandingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LandingState>()
            .init_resource::<DocLibrary>()
            .init_resource::<Thumbnails>()
            .init_resource::<LandingDetails>()
            .init_resource::<details::SampleThumbs>()
            .add_systems(
                OnEnter(AppState::Landing),
                (load_library, spawn_landing).chain(),
            )
            .add_systems(
                Update,
                (
                    sync_search,
                    rebuild_main,
                    sync_filter_items,
                    sync_search_placeholder,
                    sync_disclosures,
                    landing_shortcuts,
                )
                    .run_if(in_state(AppState::Landing)),
            )
            .add_systems(
                Update,
                (
                    details::rebuild_details,
                    details::rebuild_label_items,
                    details::filter_label_checks,
                    details::commit_description_on_blur,
                    details::on_import_picked,
                )
                    .run_if(in_state(AppState::Landing)),
            )
            .add_observer(details::on_pick_activate)
            .add_observer(details::on_pick_double_click)
            .add_observer(details::on_label_check)
            .add_observer(details::on_label_context_action)
            .add_observer(details::on_label_context_menu)
            .add_observer(details::on_sample_context_menu)
            .add_observer(details::on_sample_context_action)
            .add_observer(details::on_details_tab)
            .add_observer(details::on_details_button)
            .add_observer(details::on_description_submit)
            .add_observer(on_filter_activate)
            .add_observer(on_sort_change)
            .add_observer(on_text_submit)
            .add_observer(on_row_context_menu)
            .add_observer(on_context_action)
            .add_observer(on_section_toggled);
    }
}

// ---------------------------------------------------------------------------------------------
// State

/// What the documents page shows. Kept while a document is open, so going back returns to the
/// same filter and sort.
#[derive(Resource, Debug, Clone)]
pub struct LandingState {
    pub filter: Filter,
    pub sort_key: SortKey,
    pub sort_dir: SortDir,
    pub search: String,
    /// Open state of "Getting started", "Last opened by me" and "Folders".
    pub sections: [bool; 3],
    /// Whether the sections block is shown at all (the round toggle under it).
    pub sections_visible: bool,
    pub teams_open: bool,
    pub labels_open: bool,
    /// Whether the user picked a sort; the default sort shows no arrow, like the reference.
    pub sort_touched: bool,
    /// The folder opened (P3E.1, TD3.6): the list shows its documents under a breadcrumb.
    pub folder: Option<FolderId>,
    /// The list's Type filter (P3E.1, TD3.7).
    pub item_type: ItemType,
    /// The grid view instead of the list (P3E.1, TD3.7).
    pub grid: bool,
    /// Whether the user opened or closed "Last opened by me" (P3E.2, P3E.1 judge): until then
    /// it is open whenever it has documents.
    pub recent_touched: bool,
}

impl Default for LandingState {
    fn default() -> Self {
        Self {
            filter: Filter::OwnedByMe,
            sort_key: SortKey::Modified,
            sort_dir: SortDir::Descending,
            search: String::new(),
            sections: [false; 3],
            sections_visible: true,
            teams_open: false,
            labels_open: false,
            sort_touched: false,
            folder: None,
            item_type: ItemType::All,
            grid: false,
            recent_touched: false,
        }
    }
}

/// The document library and its undo history.
#[derive(Resource, Debug, Clone, Default)]
pub struct DocLibrary {
    pub lib: Library,
    pub history: LibraryHistory,
}

impl DocLibrary {
    /// Runs a library command and writes the changed entries to disk.
    pub fn execute(&mut self, store: &cadrs_core::Store, cmd: &dyn LibraryCommand) {
        let before = self.lib.clone();
        match self.history.execute(&mut self.lib, cmd) {
            Ok(()) => sync(store, &before, &self.lib),
            Err(e) => warn!("{}: {e}", cmd.label()),
        }
    }

    /// Undoes the last library command. Returns true if there was one.
    pub fn undo(&mut self, store: &cadrs_core::Store) -> bool {
        let before = self.lib.clone();
        let done = self.history.undo(&mut self.lib).is_some();
        if done {
            sync(store, &before, &self.lib);
        }
        done
    }

    /// Redoes the last undone library command. Returns true if there was one.
    pub fn redo(&mut self, store: &cadrs_core::Store) -> bool {
        let before = self.lib.clone();
        let done = self.history.redo(&mut self.lib).is_some();
        if done {
            sync(store, &before, &self.lib);
        }
        done
    }
}

fn sync(store: &cadrs_core::Store, before: &Library, after: &Library) {
    if let Err(e) = store.sync(before, after) {
        error!("cannot save the document library: {e}");
    }
}

/// Thumbnail images by document: the lists' small ones, and the details panel's large ones.
#[derive(Resource, Default)]
struct Thumbnails(HashMap<DocumentId, Option<Handle<Image>>>, HashMap<DocumentId, Option<Handle<Image>>>);

/// The main panel, rebuilt whenever the list changes.
#[derive(Component)]
struct MainPanel;

/// A sidebar filter item.
#[derive(Component)]
struct FilterItem(Filter);

/// Teams / Labels expanders in the sidebar.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Disclosure {
    Teams,
    Labels,
}

#[derive(Component)]
struct DisclosureChildren(Disclosure);

/// A document list row.
#[derive(Component)]
struct DocRow(DocumentId);

/// A selectable item of the page (a document or folder row or card, a sample row): a click
/// selects it, a double click opens it (P3E.1).
#[derive(Component, Clone, Copy)]
struct PickRow(Pick);

/// Which document a context menu is for.
#[derive(Component)]
struct ContextDoc(DocumentId);

/// The dialog currently open on the documents page.
#[derive(Component, Clone, Copy)]
enum LandingDialog {
    NewDocument,
    NewFolder,
    Rename(DocumentId),
    Copy(DocumentId),
    Trash(DocumentId),
    Purge(DocumentId),
    /// P3E.1: a new label, given to the document when there is one.
    NewLabel(Option<DocumentId>),
    RenameLabel(LabelId),
    DeleteLabel(LabelId),
    /// P3E.1: "Open a copy" of the bundled sample `SAMPLES[i]`.
    SampleCopy(usize),
}

const SECTION_NAMES: [&str; 3] = [
    "section-getting-started",
    "section-last-opened",
    "section-folders",
];

fn filter_icon(f: Filter) -> &'static str {
    match f {
        Filter::Explore => "idea",
        Filter::OwnedByMe => "owned-by-me",
        Filter::RecentlyOpened => "clock",
        Filter::CreatedByMe => "file",
        Filter::SharedWithMe => "shared-with-me",
        Filter::Public => "public",
        Filter::Trash => "delete",
        Filter::Label(_) => "tag",
    }
}

fn filter_name(f: Filter) -> &'static str {
    match f {
        Filter::Explore => "filter-explore",
        Filter::OwnedByMe => "filter-owned",
        Filter::RecentlyOpened => "filter-recent",
        Filter::CreatedByMe => "filter-created",
        Filter::SharedWithMe => "filter-shared",
        Filter::Public => "filter-public",
        Filter::Trash => "filter-trash",
        Filter::Label(_) => "filter-label",
    }
}

/// The filter's title: its label, or the label's name (P3E.1).
fn filter_title(f: Filter, lib: &Library) -> String {
    match f {
        Filter::Label(id) => lib.label(id).map(|l| l.name.clone()).unwrap_or_else(|| "Label".into()),
        f => f.label().to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// Loading

fn load_library(
    store: Res<DocumentStore>,
    mut lib: ResMut<DocLibrary>,
    mut thumbs: ResMut<Thumbnails>,
) {
    let (l, errors) = store.0.list();
    for (path, e) in errors {
        warn!("skipping {}: {e}", path.display());
    }
    lib.lib = l;
    lib.history.clear();
    // Thumbnails may have changed while a document was open.
    thumbs.0.clear();
    thumbs.1.clear();
}

/// Reads the library from the store again (the scenario command `landing-reload`, after a
/// set-up wrote documents behind the page's back).
pub fn reload_library(world: &mut World) {
    let store = world.resource::<DocumentStore>().0.clone();
    let (l, _) = store.list();
    let mut lib = world.resource_mut::<DocLibrary>();
    lib.lib = l;
    lib.history.clear();
    world.resource_mut::<Thumbnails>().0.clear();
    world.resource_mut::<Thumbnails>().1.clear();
}

fn thumbnail(
    thumbs: &mut Thumbnails,
    images: &mut Assets<Image>,
    store: &cadrs_core::Store,
    id: DocumentId,
) -> Option<Handle<Image>> {
    thumbs.0.entry(id).or_insert_with(|| thumbnail_image(images, store.read_thumbnail(id)?)).clone()
}

/// The document's large thumbnail (the details panel's; its small one if it has none).
fn large_thumbnail(
    thumbs: &mut Thumbnails,
    images: &mut Assets<Image>,
    store: &cadrs_core::Store,
    id: DocumentId,
) -> Option<Handle<Image>> {
    thumbs.1.entry(id).or_insert_with(|| thumbnail_image(images, store.read_thumbnail_large(id)?)).clone()
}

/// A decoded thumbnail as an image.
fn thumbnail_image(images: &mut Assets<Image>, img: image::RgbaImage) -> Option<Handle<Image>> {
    let (w, h) = img.dimensions();
    Some(images.add(Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        img.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )))
}

// ---------------------------------------------------------------------------------------------
// Layout

fn spawn_landing(
    mut commands: Commands,
    theme: Res<Theme>,
    user: Res<UserProfile>,
    state: Res<LandingState>,
    details: Res<LandingDetails>,
) {
    let t = theme.clone();
    commands
        .spawn((
            Name::new("landing"),
            bevy::input_focus::tab_navigation::TabGroup::new(0),
            DespawnOnExit(AppState::Landing),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(t.background),
        ))
        .with_children(|root| {
            top_bar(root, &t, &user, &state);
            root.spawn(Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..default()
            })
            .with_children(|body| {
                sidebar(body, &t, &state);
                body.spawn((
                    Name::new("main-panel"),
                    MainPanel,
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::new(
                            Val::Px(10.0),
                            Val::Px(12.0),
                            Val::Px(0.0),
                            Val::Px(0.0),
                        ),
                        ..default()
                    },
                ));
                // The details rail and panel on the right edge (P3E.1).
                details::spawn_details(body, &t, &details);
            });
            footer(root, &t);
        });
}

fn top_bar(p: &mut ChildSpawnerCommands, t: &Theme, user: &UserProfile, state: &LandingState) {
    p.spawn((
        Name::new("top-bar"),
        Node {
            height: Val::Px(t.top_bar_height),
            flex_shrink: 0.0,
            padding: UiRect::new(Val::Px(12.0), Val::Px(8.0), Val::ZERO, Val::ZERO),
            align_items: AlignItems::Center,
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(t.title_bar),
        BorderColor::all(Color::srgb_u8(0xdc, 0xdc, 0xdc)),
    ))
    .with_children(|bar| {
        // Logo: a neutral mark and the name.
        bar.spawn((
            Name::new("logo"),
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(7.0),
                width: Val::Px(294.0),
                flex_shrink: 0.0,
                ..default()
            },
        ))
        .with_children(|logo| {
            logo.spawn((
                Node {
                    width: Val::Px(22.0),
                    height: Val::Px(22.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(Val::Px(5.0)),
                    ..default()
                },
                BackgroundColor(t.primary),
            ))
            .with_child(icon("part", 16.0, Color::WHITE));
            logo.spawn(t.text("cadrs", t.font_xl, FontWeight::NORMAL, t.muted_foreground));
        });
        // Search box with a scope dropdown and a search button.
        bar.spawn((
            Name::new("search-box"),
            Node {
                align_items: AlignItems::Center,
                width: Val::Px(358.0),
                flex_shrink: 0.0,
                ..default()
            },
        ))
        .with_children(|s| {
            s.spawn(
                TextInput::new("search")
                    .placeholder(format!("Search in {}", state.filter.label()))
                    .height(26.0)
                    .width(Val::Px(331.0))
                    .build(t),
            )
            .with_child((
                Name::new("search-scope"),
                icon_in(
                    "caret-down",
                    14.0,
                    t.foreground,
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(6.0),
                        ..default()
                    },
                ),
                Pickable::IGNORE,
            ));
            s.spawn(
                IconButton::new("search-button", "search")
                    .icon_size(18.0)
                    .tooltip("Search")
                    .build(t),
            )
            .insert((
                Node {
                    width: Val::Px(28.0),
                    height: Val::Px(26.0),
                    margin: UiRect::left(Val::Px(-1.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::right(Val::Px(t.radius)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                cadrs_ui::Visuals {
                    background: cadrs_ui::StateColors::new(
                        Color::srgb_u8(0xf4, 0xf4, 0xf4),
                        t.ghost_hover,
                        t.ghost_active,
                        t.ghost_hover,
                    ),
                    border: cadrs_ui::StateColors::all(Color::srgb_u8(0xbd, 0xbd, 0xbd)),
                    foreground: cadrs_ui::StateColors::all(t.foreground),
                    focus_ring: t.focus_ring,
                },
            ));
        });
        bar.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        // Right side: notifications, apps, help, user.
        bar.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(2.0),
            ..default()
        })
        .with_children(|r| {
            let mut dark = cadrs_ui::button::visuals_for(t, cadrs_ui::ButtonVariant::Ghost);
            dark.foreground = cadrs_ui::StateColors::all(t.foreground);
            for (name, icon_name, tip) in [
                ("notifications", "notifications", "Notifications"),
                ("apps", "apps", "Apps"),
            ] {
                r.spawn(
                    IconButton::new(name, icon_name)
                        .icon_size(20.0)
                        .tooltip(tip)
                        .build(t),
                )
                .insert(dark.clone());
            }
            r.spawn(
                Button::new("help")
                    .icon("help")
                    .icon_size(20.0)
                    .ghost()
                    .dropdown_caret()
                    .tooltip("Help")
                    .build(t),
            )
            .insert(dark.clone())
            .entry::<Node>()
            .and_modify(|mut n| {
                n.padding = UiRect::horizontal(Val::Px(4.0));
                n.column_gap = Val::Px(4.0);
            });
            let mut account = r.spawn(
                Button::new("account")
                    .label(user.display_name.clone())
                    .ghost()
                    .dropdown_caret()
                    .tooltip("Account")
                    .build(t),
            );
            account.insert(dark);
            account.entry::<Node>().and_modify(|mut n| {
                n.padding = UiRect::horizontal(Val::Px(4.0));
                n.column_gap = Val::Px(6.0);
            });
            let account = account.id();
            let avatar = r
                .commands_mut()
                .spawn((
                    Avatar::new("account-avatar", user.display_name.clone())
                        .size(22.0)
                        .build(t),
                    Pickable::IGNORE,
                ))
                .id();
            r.commands_mut()
                .entity(account)
                .insert_children(0, &[avatar]);
        });
    });
}

fn sidebar(p: &mut ChildSpawnerCommands, t: &Theme, state: &LandingState) {
    p.spawn((
        Name::new("sidebar"),
        Node {
            width: Val::Px(t.sidebar_width),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(8.0), Val::Px(10.0)),
            border: UiRect::right(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(t.sidebar),
        BorderColor::all(t.separator),
    ))
    .with_children(|s| {
        s.spawn(Node {
            justify_content: JustifyContent::Center,
            margin: UiRect::bottom(Val::Px(14.0)),
            ..default()
        })
        .with_children(|c| {
            c.spawn((
                Button::new("create")
                    .label("Create")
                    .primary()
                    .large()
                    .dropdown_caret()
                    .width(Val::Px(138.0))
                    .build(t),
                observe(open_create_menu),
                observe(on_create_menu_action),
            ))
            .entry::<Node>()
            .and_modify(|mut n| n.column_gap = Val::Px(6.0));
        });
        let item = |name: &'static str, icon_name: &'static str, label: &str| {
            ListItem::new(name)
                .icon(icon_name)
                .label(label)
                .height(29.4)
                .padding_left(14.0)
                .icon_size(18.0)
                .selection_indicator()
                .weight(FontWeight::MEDIUM)
        };
        let before = [
            Filter::Explore,
            Filter::OwnedByMe,
            Filter::RecentlyOpened,
            Filter::CreatedByMe,
            Filter::SharedWithMe,
        ];
        let filter_item = |s: &mut ChildSpawnerCommands, f: Filter| {
            s.spawn((
                item(filter_name(f), filter_icon(f), f.label())
                    .selected(state.filter == f)
                    .weight(if state.filter == f {
                        FontWeight::BOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .build(t),
                FilterItem(f),
            ));
        };
        for f in before {
            filter_item(s, f);
        }
        for (d, name, icon_name, label, empty) in [
            (Disclosure::Teams, "filter-teams", "users", "Teams", "No teams"),
            (Disclosure::Labels, "filter-labels", "tag", "Labels", "No labels"),
        ] {
            let open = match d {
                Disclosure::Teams => state.teams_open,
                Disclosure::Labels => state.labels_open,
            };
            let li = item(name, icon_name, label).disclosure(Some(open));
            let mut row = s.spawn((li.build(t), d));
            if d == Disclosure::Labels {
                // P3E.1: the tag-new icon creates a label (Create ▸ Label…).
                let mut v = cadrs_ui::button::visuals_for(t, cadrs_ui::ButtonVariant::Ghost);
                v.foreground = cadrs_ui::StateColors::all(t.muted_foreground);
                row.with_children(|r| {
                    r.spawn((
                        IconButton::new("labels-new", "tag-new").icon_size(16.0).small().tooltip("Create label").build(t),
                        observe(|_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                            details::open_new_label_dialog(&mut commands, &theme, None);
                        }),
                    ))
                    .insert(v)
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.position_type = PositionType::Absolute;
                        n.right = Val::Px(8.0);
                    });
                });
            }
            if d == Disclosure::Labels {
                // Filled with the labels by `details::rebuild_label_items`.
                s.spawn((
                    DisclosureChildren(d),
                    Name::new("labels-list"),
                    Node {
                        display: if open { Display::Flex } else { Display::None },
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                ));
                continue;
            }
            s.spawn((
                DisclosureChildren(d),
                Node {
                    display: if open { Display::Flex } else { Display::None },
                    height: Val::Px(26.0),
                    padding: UiRect::left(Val::Px(44.0)),
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_child(t.text(empty, t.font_base, FontWeight::NORMAL, t.subtle_foreground));
        }
        filter_item(s, Filter::Public);
        filter_item(s, Filter::Trash);

        // Bottom: where documents are kept.
        s.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        s.spawn(Node {
            justify_content: JustifyContent::Center,
            margin: UiRect::bottom(Val::Px(16.0)),
            column_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|b| {
            b.spawn(t.text("Storage:", t.font_base, FontWeight::SEMIBOLD, t.foreground));
            b.spawn(t.text("Local", t.font_base, FontWeight::NORMAL, t.foreground));
        });
        s.spawn(Node {
            padding: UiRect::horizontal(Val::Px(9.0)),
            ..default()
        })
        .with_children(|b| {
            b.spawn((
                Button::new("open-data-folder")
                    .label("Open documents folder")
                    .outline()
                    .width(Val::Percent(100.0))
                    .build(t),
                observe(
                    |_: On<Activate>, store: Res<DocumentStore>| {
                        let dir = store.0.root().to_path_buf();
                        let _ = std::fs::create_dir_all(&dir);
                        let opener = if cfg!(windows) { "explorer" } else { "xdg-open" };
                        if let Err(e) = std::process::Command::new(opener).arg(&dir).spawn() {
                            warn!("cannot open {}: {e}", dir.display());
                        }
                    },
                ),
            ));
        });
    });
}

fn footer(p: &mut ChildSpawnerCommands, t: &Theme) {
    p.spawn((
        Name::new("footer"),
        Node {
            height: Val::Px(26.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            border: UiRect::top(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(t.separator),
    ))
    .with_children(|f| {
        for text in [
            "© 2026 cadrs contributors",
            "Documents are stored on this computer",
            concat!("(", env!("CARGO_PKG_VERSION"), ")"),
        ] {
            f.spawn(Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_child(t.text(text, t.font_xs, FontWeight::NORMAL, t.muted_foreground));
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Main panel

fn columns(state: &LandingState, labels: bool) -> Vec<Column> {
    let sort = |k: SortKey| {
        if state.sort_key != k || !state.sort_touched {
            ColumnSort::Default
        } else if state.sort_dir == SortDir::Ascending {
            ColumnSort::Ascending
        } else {
            ColumnSort::Descending
        }
    };
    // P3E.1: a Labels column once the library has labels (TD3.7); the others make room.
    let mut v = vec![
        Column::new("name", "Name")
            .width(if labels { 360.0 } else { 420.0 })
            .sortable()
            .sort(sort(SortKey::Name)),
    ];
    if labels {
        v.push(Column::new("labels", "Labels").width(200.0));
    }
    v.push(
        Column::new("modified", "Modified")
            .width(if labels { 170.0 } else { 184.0 })
            .sortable()
            .sort(sort(SortKey::Modified))
            .first_sort(ColumnSort::Descending),
    );
    v.push(
        Column::new("modified-by", "Modified by")
            .width(if labels { 150.0 } else { 184.0 })
            .sortable()
            .sort(sort(SortKey::ModifiedBy)),
    );
    v.push(Column::new("owned-by", "Owned by").sortable().sort(sort(SortKey::OwnedBy)));
    v
}

/// A folder listed in the main panel.
#[derive(Clone)]
struct FolderListing {
    id: FolderId,
    name: String,
    date: String,
}

#[allow(clippy::too_many_arguments)]
fn rebuild_main(
    mut commands: Commands,
    theme: Res<Theme>,
    state: Res<LandingState>,
    lib: Res<DocLibrary>,
    store: Res<DocumentStore>,
    clock: Res<AppClock>,
    user: Res<UserProfile>,
    details_state: Res<LandingDetails>,
    mut thumbs: ResMut<Thumbnails>,
    mut samples: ResMut<details::SampleThumbs>,
    mut images: ResMut<Assets<Image>>,
    q_panel: Query<(Entity, Ref<MainPanel>)>,
) {
    let Ok((panel, marker)) = q_panel.single() else {
        return;
    };
    if !(marker.is_added() || state.is_changed() || lib.is_changed()) {
        return;
    }
    let t = theme.clone();
    commands.entity(panel).despawn_related::<Children>();
    let picked = details_state.pick;
    let folder = state.folder.and_then(|f| lib.lib.folders.iter().find(|x| x.id == f).cloned());
    let mut entries = lib.lib.view(
        state.filter,
        &user.id,
        &state.search,
        state.sort_key,
        state.sort_dir,
    );
    if let Some(f) = &folder {
        entries.retain(|e| e.meta.folder == Some(f.id));
    } else if state.filter == Filter::OwnedByMe && state.search.trim().is_empty() {
        // P3E.2 (P3E.1 judge; decision): like Onshape, the root of Owned by me lists the
        // folders and the documents in no folder; a filed document is listed inside its folder
        // (and found by a search, which looks everywhere).
        entries.retain(|e| e.meta.folder.is_none());
    }
    if !state.item_type.shows_documents() {
        entries.clear();
    }
    let recent: Vec<DocumentEntry> = lib
        .lib
        .view(
            Filter::RecentlyOpened,
            &user.id,
            "",
            SortKey::Modified,
            SortDir::Descending,
        )
        .into_iter()
        .take(6)
        .collect();
    let has_labels = !lib.lib.labels.is_empty();
    let cols = columns(&state, has_labels);
    let folders: Vec<(FolderId, String)> = lib.lib.folders.iter().map(|f| (f.id, f.name.clone())).collect();
    // Folders are listed above the documents, as in Onshape's list view.
    let folder_rows: Vec<FolderListing> = if folder.is_none()
        && state.item_type.shows_folders()
        && matches!(state.filter, Filter::OwnedByMe | Filter::CreatedByMe)
    {
        let needle = state.search.trim().to_lowercase();
        let mut v: Vec<&cadrs_core::FolderEntry> = lib
            .lib
            .folders
            .iter()
            .filter(|f| f.owned_by == user.id && (needle.is_empty() || f.name.to_lowercase().contains(&needle)))
            .collect();
        v.sort_by(|a, b| cadrs_core::library::natural_cmp(&a.name, &b.name));
        v.into_iter()
            .map(|f| FolderListing { id: f.id, name: f.name.clone(), date: clock.format(f.created) })
            .collect()
    } else {
        Vec::new()
    };
    let rows: Vec<(DocumentEntry, Option<Handle<Image>>, String)> = entries
        .into_iter()
        .map(|e| {
            let thumb = thumbnail(&mut thumbs, &mut images, &store.0, e.id);
            let date = clock.format(e.meta.modified);
            (e, thumb, date)
        })
        .collect();
    let recent: Vec<(DocumentEntry, Option<Handle<Image>>)> = recent
        .into_iter()
        .map(|e| {
            let thumb = thumbnail(&mut thumbs, &mut images, &store.0, e.id);
            (e, thumb)
        })
        .collect();
    let samples_list = if state.filter == Filter::Explore {
        Some(details::sample_rows(&mut samples, &mut images))
    } else {
        None
    };
    // P3E.2 (P3E.1 judge): the selection is cleared when the selected item leaves the view
    // (moved to the trash, deleted, or not listed under the new filter), and the Details panel
    // with it.
    let listed_here = state.filter == Filter::OwnedByMe && folder.is_none();
    let visible = match picked {
        None => true,
        Some(Pick::Doc(id)) => rows.iter().any(|r| r.0.id == id) || (listed_here && recent.iter().any(|r| r.0.id == id)),
        Some(Pick::Folder(f)) => folder_rows.iter().any(|x| x.id == f) || (listed_here && folders.iter().any(|x| x.0 == f)),
        Some(Pick::Sample(_)) => state.filter == Filter::Explore,
    };
    let picked = if visible {
        picked
    } else {
        commands.queue(|w: &mut World| w.resource_mut::<LandingDetails>().pick = None);
        None
    };
    let title = filter_title(state.filter, &lib.lib);
    let labels = lib.lib.labels.clone();
    let state = state.clone();
    let user = user.clone();

    commands.entity(panel).with_children(|m| {
        // Heading, or a breadcrumb inside a folder (P3E.1, TD3.6).
        m.spawn((
            Name::new("heading"),
            Node {
                height: Val::Px(47.0),
                flex_shrink: 0.0,
                padding: UiRect::left(Val::Px(8.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|h| match &folder {
            Some(f) => details::breadcrumb(h, &t, state.filter, &title, &f.name),
            None => {
                h.spawn(icon(filter_icon(state.filter), 20.0, t.foreground));
                h.spawn((
                    Name::new("heading-title"),
                    t.text(title.clone(), t.font_md, FontWeight::BOLD, t.foreground),
                ));
            }
        });

        let show_sections = state.filter == Filter::OwnedByMe && folder.is_none();
        if show_sections {
            sections(m, &t, &state, recent, folders, picked);
        }

        if let Some(list) = samples_list {
            details::samples_table(m, &t, list, picked);
            return;
        }

        // The toolbar row: list/grid toggles and the Type filter (P3E.1), "+ Add".
        let toolbar = show_sections || folder.is_some() || matches!(state.filter, Filter::Label(_));
        m.spawn(Node {
            height: Val::Px(if toolbar { 40.0 } else { 8.0 }),
            flex_shrink: 0.0,
            justify_content: JustifyContent::FlexEnd,
            align_items: AlignItems::Center,
            padding: UiRect::right(Val::Px(4.0)),
            ..default()
        })
        .with_children(|a| {
            if toolbar {
                details::list_toolbar(a, &t, &state);
                a.spawn(Node { flex_grow: 1.0, ..default() });
            }
            if show_sections {
                let mut dark = cadrs_ui::button::visuals_for(&t, cadrs_ui::ButtonVariant::Ghost);
                dark.foreground = cadrs_ui::StateColors::all(t.foreground);
                a.spawn((
                    Button::new("add")
                        .label("Add")
                        .icon("plus-bold")
                        .icon_size(14.0)
                        .ghost()
                        .build(&t),
                    observe(open_create_menu),
                    observe(on_create_menu_action),
                ))
                .insert(dark)
                .entry::<Node>()
                .and_modify(|mut n| {
                    n.column_gap = Val::Px(4.0);
                    n.padding = UiRect::horizontal(Val::Px(6.0));
                });
            }
        });

        if rows.is_empty() && folder_rows.is_empty() {
            if !state.grid {
                m.spawn(TableHeader::new("col", cols.clone()).height(30.0).build(&t));
            }
            let (icon_name, title, text, create) = match state.filter {
                _ if !state.search.trim().is_empty() => (
                    "search",
                    "No matching documents".to_string(),
                    format!("Nothing in {} matches \"{}\".", title, state.search.trim()),
                    false,
                ),
                _ if folder.is_some() => (
                    "folder",
                    "This folder is empty".into(),
                    "Right-click a document and choose Move to… to put it here.".into(),
                    false,
                ),
                _ if state.item_type != ItemType::All => (
                    "filter",
                    format!("No {}", state.item_type.label().to_lowercase()),
                    "Choose Type ▸ All to see everything.".into(),
                    false,
                ),
                Filter::Label(_) => (
                    "tag",
                    "No documents with this label".into(),
                    "Give documents this label in the Details panel or with right-click ▸ Labels.".into(),
                    false,
                ),
                Filter::Trash => (
                    "delete",
                    "Trash is empty".into(),
                    "Documents you move to the trash appear here.".into(),
                    false,
                ),
                Filter::SharedWithMe => (
                    "shared-with-me",
                    "Nothing shared with you".into(),
                    "Documents other people share with you appear here.".into(),
                    false,
                ),
                Filter::Public => (
                    "public",
                    "No public documents".into(),
                    "cadrs keeps your documents on this computer.".into(),
                    false,
                ),
                Filter::RecentlyOpened => (
                    "clock",
                    "No recently opened documents".into(),
                    "Documents you open appear here.".into(),
                    false,
                ),
                _ => (
                    "file",
                    "No documents yet".into(),
                    "Create a document to start modeling.".into(),
                    true,
                ),
            };
            empty_state(m, &t, icon_name, &title, &text, create);
            return;
        }
        if state.grid {
            details::grid(m, &t, &folder_rows_simple(&folder_rows), rows, picked, &labels);
            return;
        }

        // The list.
        m.spawn(TableHeader::new("col", cols.clone()).height(30.0).build(&t));
        m.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|wrap| {
        let list = wrap.spawn((
            Name::new("doc-list"),
            ScrollArea,
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                padding: UiRect::right(Val::Px(14.0)),
                ..default()
            },
        ))
        .with_children(|list| {
            for (i, f) in folder_rows.into_iter().enumerate() {
                let name_font = t.font(t.font_md, FontWeight::MEDIUM);
                let fg = t.foreground;
                let label = format!("folder-name-{i}");
                let name = f.name.clone();
                let mut row = TableRow::new(format!("folder-row-{i}"), &cols)
                    .cell(move |c| {
                        c.spawn((
                            Node {
                                width: Val::Px(60.0),
                                height: Val::Px(34.0),
                                flex_shrink: 0.0,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_child((icon("folder", 24.0, fg), Pickable::IGNORE));
                        c.spawn((
                            Name::new(label),
                            Text::new(name),
                            name_font,
                            TextColor(fg),
                            TextLayout::no_wrap(),
                            cadrs_ui::InheritFg,
                            Pickable::IGNORE,
                            Node {
                                margin: UiRect::left(Val::Px(4.0)),
                                ..default()
                            },
                        ));
                    });
                if has_labels {
                    row = row.cell(|_| {});
                }
                let row = row
                    .text_cell(&t, f.date)
                    .text_cell(&t, "me")
                    .text_cell(&t, "me")
                    .selected(picked == Some(Pick::Folder(f.id)));
                list.spawn((row.build(&t), PickRow(Pick::Folder(f.id)), cadrs_ui::DoubleClickable))
                    .entry::<Node>()
                    .and_modify(|mut n| n.height = Val::Px(38.5));
            }
            for (i, (e, thumb, date)) in rows.into_iter().enumerate() {
                let name_font = t.font(t.font_md, FontWeight::MEDIUM);
                let fg = t.foreground;
                let tag_color = Color::srgb_u8(0x33, 0x33, 0x33);
                let tag_font = t.font(t.font_xs, FontWeight::NORMAL);
                let placeholder = t.subtle_foreground;
                let name = e.name.clone();
                // TD3.7: the workspace last opened (P3E.4).
                let workspace = e.meta.workspace_name().to_string();
                let name_label = format!("doc-name-{i}");
                let mut row = TableRow::new(format!("doc-row-{i}"), &cols)
                    .cell(move |c| {
                        thumbnail_node(c, thumb, placeholder);
                        c.spawn((
                            Name::new(name_label),
                            Text::new(name),
                            name_font,
                            TextColor(fg),
                            TextLayout::no_wrap(),
                            cadrs_ui::InheritFg,
                            Pickable::IGNORE,
                            Node {
                                margin: UiRect::left(Val::Px(4.0)),
                                ..default()
                            },
                        ));
                        c.spawn((
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(2.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|tag| {
                            tag.spawn((icon("location", 12.0, tag_color), Pickable::IGNORE));
                            tag.spawn((
                                Text::new(workspace),
                                tag_font,
                                TextColor(tag_color),
                                TextLayout::no_wrap(),
                                Pickable::IGNORE,
                            ));
                        });
                    });
                if has_labels {
                    let chips: Vec<(String, String, [u8; 3])> = e
                        .meta
                        .labels
                        .iter()
                        .filter_map(|l| labels.iter().find(|x| x.id == *l))
                        .map(|l| (format!("doc-label-{i}-{}", details::slug(&l.name)), l.name.clone(), l.colour))
                        .collect();
                    let tc = t.clone();
                    row = row.cell(move |c| {
                        c.spawn((
                            Node { column_gap: Val::Px(4.0), overflow: Overflow::clip(), ..default() },
                            Pickable::IGNORE,
                        ))
                        .with_children(|w| {
                            for (name, label, colour) in chips {
                                w.spawn(cadrs_ui::LabelChip::new(name, label, colour).build(&tc));
                            }
                        });
                    });
                }
                let row = row
                    .text_cell(&t, date)
                    .text_cell(&t, user.display(&e.meta.modified_by))
                    .text_cell(&t, user.display(&e.meta.owned_by))
                    .selected(picked == Some(Pick::Doc(e.id)));
                list.spawn((row.build(&t), DocRow(e.id), PickRow(Pick::Doc(e.id)), cadrs_ui::DoubleClickable))
                    .entry::<Node>()
                    .and_modify(|mut n| n.height = Val::Px(38.5));
            }
        })
        .id();
        wrap.spawn(cadrs_ui::vertical_scrollbar(&t, "doc-list-scrollbar", list));
        });
    });
}

fn folder_rows_simple(v: &[FolderListing]) -> Vec<(FolderId, String)> {
    v.iter().map(|f| (f.id, f.name.clone())).collect()
}

fn thumbnail_node(c: &mut ChildSpawner, thumb: Option<Handle<Image>>, placeholder: Color) {
    let node = Node {
        width: Val::Px(60.0),
        height: Val::Px(34.0),
        flex_shrink: 0.0,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    match thumb {
        Some(img) => {
            c.spawn((ImageNode::new(img), node, Pickable::IGNORE));
        }
        None => {
            c.spawn((node, Pickable::IGNORE))
                .with_child((icon("part", 20.0, placeholder), Pickable::IGNORE));
        }
    }
}

fn sections(
    m: &mut ChildSpawnerCommands,
    t: &Theme,
    state: &LandingState,
    recent: Vec<(DocumentEntry, Option<Handle<Image>>)>,
    folders: Vec<(FolderId, String)>,
    picked: Option<Pick>,
) {
    let tc = t.clone();
    m.spawn((
        Name::new("sections"),
        Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: Val::Px(2.0),
            display: if state.sections_visible {
                Display::Flex
            } else {
                Display::None
            },
            ..default()
        },
    ))
    .with_children(|s| {
        let t = tc.clone();
        s.spawn(
            Collapsible::new(SECTION_NAMES[0], "Getting started with cadrs")
                .icon("idea")
                .icon_badge()
                .highlight(true)
                .header_height(27.0)
                .open(state.sections[0])
                .content(move |c| {
                    for line in [
                        "Create ▸ Document… makes a new document with a Part Studio and an Assembly.",
                        "Double-click a document to open it; click it to see its details; right-click it to rename it or move it to the trash.",
                        "Click a column header to sort the list.",
                    ] {
                        c.spawn(t.text(line, t.font_base, FontWeight::NORMAL, t.muted_foreground));
                    }
                })
                .build(&tc),
        );
        let t = tc.clone();
        s.spawn(
            Collapsible::new(SECTION_NAMES[1], "Last opened by me")
                .icon("clock")
                .header_height(31.0)
                // P3E.2 (P3E.1 judge): open by default when it has documents.
                .open(state.sections[1] || (!state.recent_touched && !recent.is_empty()))
                .content(move |c| {
                    if recent.is_empty() {
                        c.spawn(t.text(
                            "Documents you open appear here.",
                            t.font_base,
                            FontWeight::NORMAL,
                            t.muted_foreground,
                        ));
                        return;
                    }
                    c.spawn(Node {
                        column_gap: Val::Px(12.0),
                        row_gap: Val::Px(8.0),
                        flex_wrap: FlexWrap::Wrap,
                        ..default()
                    })
                    .with_children(|row| {
                        for (i, (e, thumb)) in recent.into_iter().enumerate() {
                            // P3E.2 (P3E.1 judge, `lesson-documents-page.png`): a bordered card,
                            // the thumbnail left and the name right.
                            row.spawn((
                                Name::new(format!("recent-{i}")),
                                Node {
                                    flex_direction: FlexDirection::Row,
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(10.0),
                                    width: Val::Px(210.0),
                                    height: Val::Px(50.0),
                                    padding: UiRect::horizontal(Val::Px(8.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(t.radius_lg)),
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                                BorderColor::all(t.border),
                                BackgroundColor(t.background),
                            ))
                            .with_children(|card| {
                                // P3E.1 (TD3.5): double-click the thumbnail or click the name to
                                // open the document.
                                let id = e.id;
                                let mut thumb_node = card.spawn((
                                    Name::new(format!("recent-thumb-{i}")),
                                    Node { flex_shrink: 0.0, ..default() },
                                    cadrs_ui::DoubleClickable,
                                    Pickable::default(),
                                    observe(move |_: On<cadrs_ui::DoubleClick>, mut commands: Commands| {
                                        commands.queue(move |world: &mut World| open_document(world, id));
                                    }),
                                ));
                                thumb_node.with_children(|n| thumbnail_node(n, thumb, t.subtle_foreground));
                                card.spawn((
                                    Name::new(format!("recent-name-{i}")),
                                    bevy::ui_widgets::Button,
                                    bevy::picking::hover::Hovered::default(),
                                    cadrs_ui::Visuals {
                                        background: cadrs_ui::StateColors::all(Color::NONE),
                                        border: cadrs_ui::StateColors::all(Color::NONE),
                                        foreground: cadrs_ui::StateColors::new(t.foreground, t.link, t.link, t.foreground),
                                        focus_ring: t.focus_ring,
                                    },
                                    Node { padding: UiRect::horizontal(Val::Px(2.0)), ..default() },
                                    observe(move |_: On<Activate>, mut commands: Commands| {
                                        commands.queue(move |world: &mut World| open_document(world, id));
                                    }),
                                ))
                                .with_child((
                                    t.text(e.name, t.font_base, FontWeight::NORMAL, t.foreground),
                                    cadrs_ui::InheritFg,
                                    Pickable::IGNORE,
                                ));
                            });
                        }
                    });
                })
                .build(&tc),
        );
        let t = tc.clone();
        s.spawn(
            Collapsible::new(SECTION_NAMES[2], "Folders")
                .icon("folder")
                .header_height(31.0)
                .open(state.sections[2])
                .content(move |c| {
                    if folders.is_empty() {
                        c.spawn(t.text(
                            "No folders yet. Choose Create ▸ Folder… to add one.",
                            t.font_base,
                            FontWeight::NORMAL,
                            t.muted_foreground,
                        ));
                        return;
                    }
                    c.spawn(Node {
                        column_gap: Val::Px(8.0),
                        flex_wrap: FlexWrap::Wrap,
                        ..default()
                    })
                    .with_children(|row| {
                        for (i, (id, name)) in folders.into_iter().enumerate() {
                            // P3E.1 (TD3.6): a click selects the folder, a double click opens it.
                            row.spawn((
                                Name::new(format!("folder-{i}")),
                                PickRow(Pick::Folder(id)),
                                cadrs_ui::DoubleClickable,
                                bevy::ui_widgets::Button,
                                bevy::picking::hover::Hovered::default(),
                                cadrs_ui::Visuals {
                                    background: cadrs_ui::StateColors::new(Color::NONE, t.list_hover, t.list_active, Color::NONE)
                                        .with_selected(t.list_selected),
                                    border: cadrs_ui::StateColors::all(t.border).with_selected(t.primary),
                                    foreground: cadrs_ui::StateColors::all(t.foreground),
                                    focus_ring: t.focus_ring,
                                },
                                cadrs_ui::style::InitState { disabled: false, selected: picked == Some(Pick::Folder(id)), force: None },
                                Node {
                                    height: Val::Px(32.0),
                                    padding: UiRect::horizontal(Val::Px(10.0)),
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(8.0),
                                    min_width: Val::Px(160.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(t.radius_lg)),
                                    ..default()
                                },
                                BorderColor::all(t.border),
                            ))
                            .with_children(|f| {
                                f.spawn((icon("folder", 18.0, t.foreground), Pickable::IGNORE));
                                f.spawn((t.text(name, t.font_base, FontWeight::MEDIUM, t.foreground), Pickable::IGNORE));
                            });
                        }
                    });
                })
                .build(&tc),
        );
    });
    // Divider with the round show/hide toggle.
    m.spawn((
        Name::new("sections-divider"),
        Node {
            height: Val::Px(20.0),
            flex_shrink: 0.0,
            margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(4.0), Val::ZERO),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    ))
    .with_children(|d| {
        d.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: Val::Px(2.0),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(0xd6, 0xe0, 0xe6)),
        ));
        d.spawn(
            IconButton::new(
                "sections-toggle",
                if state.sections_visible {
                    "chevron-down"
                } else {
                    "chevron-up"
                },
            )
            .small()
            .tooltip(if state.sections_visible {
                "Hide sections"
            } else {
                "Show sections"
            })
            .build(t),
        )
        .insert((
            Node {
                width: Val::Px(20.0),
                height: Val::Px(20.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            cadrs_ui::Visuals {
                background: cadrs_ui::StateColors::new(
                    t.background,
                    t.ghost_hover,
                    t.ghost_active,
                    t.background,
                ),
                border: cadrs_ui::StateColors::all(t.border),
                foreground: cadrs_ui::StateColors::all(t.foreground),
                focus_ring: t.focus_ring,
            },
            observe(|_: On<Activate>, mut state: ResMut<LandingState>| {
                state.sections_visible = !state.sections_visible;
            }),
        ));
    });
}

fn empty_state(
    m: &mut ChildSpawnerCommands,
    t: &Theme,
    icon_name: &'static str,
    title: &str,
    text: &str,
    create: bool,
) {
    m.spawn((
        Name::new("empty-state"),
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(8.0),
            margin: UiRect::top(Val::Px(64.0)),
            ..default()
        },
    ))
    .with_children(|e| {
        e.spawn(icon(icon_name, 24.0, t.subtle_foreground))
            .entry::<Node>()
            .and_modify(|mut n| n.margin = UiRect::bottom(Val::Px(4.0)));
        e.spawn(t.text(title, t.font_md, FontWeight::SEMIBOLD, t.foreground));
        e.spawn(t.text(text, t.font_base, FontWeight::NORMAL, t.muted_foreground));
        if create {
            e.spawn((
                Button::new("empty-create")
                    .label("Create document")
                    .primary()
                    .build(t),
                observe(
                    |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                        open_new_document_dialog(&mut commands, &theme);
                    },
                ),
            ))
            .entry::<Node>()
            .and_modify(|mut n| n.margin = UiRect::top(Val::Px(8.0)));
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Sync systems

fn sync_filter_items(
    state: Res<LandingState>,
    q: Query<(Entity, &FilterItem, Has<Selected>, &Children)>,
    mut q_text: Query<&mut TextFont, With<Text>>,
    mut commands: Commands,
) {
    if !state.is_changed() {
        return;
    }
    for (e, f, selected, children) in &q {
        let want = f.0 == state.filter;
        let weight = if want {
            FontWeight::BOLD
        } else {
            FontWeight::MEDIUM
        };
        for c in children.iter() {
            if let Ok(mut font) = q_text.get_mut(c)
                && font.weight != weight
            {
                font.weight = weight;
            }
        }
        // `try_`: the labels' items may be rebuilt in the same frame (P3E.1).
        if want && !selected {
            commands.entity(e).try_insert(Selected);
        } else if !want && selected {
            commands.entity(e).try_remove::<Selected>();
        }
    }
}

#[allow(clippy::type_complexity)]
fn sync_search(
    mut state: ResMut<LandingState>,
    q: Query<(&Name, &EditableText), (With<TextInputField>, Changed<EditableText>)>,
) {
    for (name, text) in &q {
        if name.as_str() == "search-field" {
            let v = text.value().to_string();
            if state.search != v {
                state.search = v;
            }
        }
    }
}

fn sync_search_placeholder(
    state: Res<LandingState>,
    lib: Res<DocLibrary>,
    mut q: Query<(&Name, &mut Text), With<TextInputPlaceholder>>,
) {
    if !state.is_changed() && !lib.is_changed() {
        return;
    }
    for (name, mut text) in &mut q {
        if name.as_str() == "search-placeholder" {
            let want = format!("Search in {}", filter_title(state.filter, &lib.lib));
            if text.0 != want {
                text.0 = want;
            }
        }
    }
}

fn sync_disclosures(
    state: Res<LandingState>,
    mut q_children: Query<(&DisclosureChildren, &mut Node)>,
    q_items: Query<(&Disclosure, &Children)>,
    mut q_icons: Query<&mut Icon>,
) {
    if !state.is_changed() {
        return;
    }
    let open = |d: Disclosure| match d {
        Disclosure::Teams => state.teams_open,
        Disclosure::Labels => state.labels_open,
    };
    for (c, mut node) in &mut q_children {
        node.display = if open(c.0) {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (d, children) in &q_items {
        for child in children.iter() {
            if let Ok(mut i) = q_icons.get_mut(child)
                && i.name.starts_with("chevron-")
            {
                let want = if open(*d) {
                    "chevron-down"
                } else {
                    "chevron-right"
                };
                if i.name != want {
                    i.name = want.into();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Interaction

#[allow(clippy::too_many_arguments)]
fn landing_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<TextInputField>>,
    q_dialogs: Query<(), With<LandingDialog>>,
    mut lib: ResMut<DocLibrary>,
    store: Res<DocumentStore>,
    details_state: Res<LandingDetails>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    mut busy: Local<u8>,
    mut commands: Commands,
) {
    if focus.get().is_some_and(|f| q_fields.contains(f)) || !q_dialogs.is_empty() || !q_menus.is_empty() {
        // An Enter that a dialog, field or menu took in this frame doesn't open the selection.
        *busy = 2;
        return;
    }
    let was_busy = *busy > 0;
    *busy = busy.saturating_sub(1);
    // P3E.1: Enter opens the selected item.
    if keys.just_pressed(KeyCode::Enter)
        && !was_busy
        && let Some(pick) = details_state.pick
    {
        commands.queue(move |world: &mut World| details::open_pick(world, pick));
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let changed = if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        if shift {
            lib.redo(&store.0)
        } else {
            lib.undo(&store.0)
        }
    } else if ctrl && keys.just_pressed(KeyCode::KeyY) {
        lib.redo(&store.0)
    } else {
        false
    };
    // A toast naming the undone (or redone) action is stale.
    if changed {
        commands.queue(cadrs_ui::close_transient_toasts);
    }
}

/// Clicking a sidebar item.
fn on_filter_activate(
    ev: On<Activate>,
    q_filter: Query<&FilterItem>,
    q_disclosure: Query<&Disclosure>,
    mut state: ResMut<LandingState>,
) {
    if let Ok(f) = q_filter.get(ev.entity) {
        if state.filter != f.0 {
            state.filter = f.0;
        }
        // A filter shows its top level (P3E.1: out of an open folder).
        if state.folder.is_some() {
            state.folder = None;
        }
    } else if let Ok(d) = q_disclosure.get(ev.entity) {
        match d {
            Disclosure::Teams => state.teams_open = !state.teams_open,
            Disclosure::Labels => state.labels_open = !state.labels_open,
        }
    }
}

fn on_sort_change(ev: On<TableSortChange>, mut state: ResMut<LandingState>) {
    // Only the first hop (the header cell itself).
    if ev.entity != ev.original_event_target() {
        return;
    }
    let key = match ev.column.as_str() {
        "name" => SortKey::Name,
        "modified" => SortKey::Modified,
        "modified-by" => SortKey::ModifiedBy,
        "owned-by" => SortKey::OwnedBy,
        _ => return,
    };
    state.sort_key = key;
    state.sort_touched = true;
    state.sort_dir = match ev.sort {
        ColumnSort::Descending => SortDir::Descending,
        _ => SortDir::Ascending,
    };
}

fn on_section_toggled(ev: On<CollapsibleToggled>, q: Query<&Name>, mut state: ResMut<LandingState>) {
    let Ok(name) = q.get(ev.entity) else {
        return;
    };
    if let Some(i) = SECTION_NAMES.iter().position(|n| *n == name.as_str()) {
        // The collapsible already shows the new state; no rebuild needed.
        let state = state.bypass_change_detection();
        state.sections[i] = ev.open;
        if i == 1 {
            state.recent_touched = true;
        }
    }
}

fn create_menu(t: &Theme, name: &'static str) -> impl Bundle {
    // Dark (#333) icons, as in `screens/02`.
    Menu::new(name)
        .strong_icons()
        .item(MenuItem::new("new-document", "Document…").icon("file-new"))
        .item(
            MenuItem::new("new-folder", "Folder…").icon("folder-new"),
        )
        .separator()
        .item(
            MenuItem::new("import-files", "Import files…").icon("upload"),
        )
        .item(
            MenuItem::new("import-from", "Import from")
                .icon("cloud-upload")
                .submenu(vec![
                    MenuItem::new("import-from-dropbox", "Dropbox…").disabled(true).into(),
                    MenuItem::new("import-from-google-drive", "Google Drive…")
                        .disabled(true)
                        .into(),
                    MenuItem::new("import-from-onedrive", "OneDrive…").disabled(true).into(),
                ]),
        )
        .item(MenuItem::new("new-label", "Label…").icon("tag-new"))
        .build(t)
}

fn open_create_menu(a: On<Activate>, mut commands: Commands, theme: Res<Theme>, q: Query<&Name>) {
    let name = match q.get(a.entity).map(|n| n.as_str()) {
        Ok("add") => "add-menu",
        _ => "create-menu",
    };
    open_menu(&mut commands, a.entity, create_menu(&theme, name));
}

fn on_create_menu_action(ev: On<MenuAction>, mut commands: Commands, theme: Res<Theme>) {
    match ev.item.as_str() {
        "new-document" => open_new_document_dialog(&mut commands, &theme),
        "new-folder" => open_new_folder_dialog(&mut commands, &theme),
        // P3F.2: a STEP or IGES file as a new document.
        "import-files" => {
            commands.queue(|w: &mut World| crate::import_file::start(w, crate::import_file::ImportTarget::NewDocument));
        }
        "new-label" => details::open_new_label_dialog(&mut commands, &theme, None),
        _ => {}
    }
}

/// Opens a stored document: records the open time and switches to the document screen.
fn open_document(world: &mut World, id: DocumentId) {
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let mut file = match store.load(id) {
        Ok(f) => f,
        Err(e) => {
            error!("cannot open document {id}: {e}");
            return;
        }
    };
    file.meta.last_opened = Some(now);
    if let Err(e) = store.save(&file.document, &file.meta) {
        warn!("cannot record the open time: {e}");
    }
    world.insert_resource(ActiveDocument::stored(file.document, file.meta));
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Document);
}

/// Creates a document with Part Studio 1 and Assembly 1 and opens it.
fn create_document(world: &mut World, name: &str) {
    let name = match name.trim() {
        "" => "Untitled document",
        n => n,
    };
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let doc = Document::new(name);
    let mut meta = DocumentMeta::new(&user, now);
    meta.last_opened = Some(now);
    if let Err(e) = store.create(&doc, &meta) {
        error!("cannot create document: {e}");
        return;
    }
    let mut active = ActiveDocument::stored(doc, meta);
    active.fresh = true;
    world.insert_resource(active);
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Document);
}

fn on_row_context_menu(
    ev: On<ContextMenuRequested>,
    q: Query<&DocRow>,
    state: Res<LandingState>,
    theme: Res<Theme>,
    lib: Res<DocLibrary>,
    mut commands: Commands,
) {
    let Ok(row) = q.get(ev.entity) else {
        return;
    };
    // P3E.1: a right-click selects the row too (the Details panel follows if open).
    let id = row.0;
    commands.queue(move |world: &mut World| details::select(world, Pick::Doc(id), false));
    let menu = if state.filter == Filter::Trash {
        Menu::new("doc-context-menu")
            .item(MenuItem::new("ctx-restore", "Restore").icon("restore"))
            .item(
                MenuItem::new("ctx-delete-forever", "Delete permanently…").icon("delete"),
            )
            .min_width(190.0)
    } else {
        Menu::new("doc-context-menu")
            .item(MenuItem::new("ctx-open", "Open").icon("open-external"))
            .item(MenuItem::new("ctx-rename", "Rename…").icon("edit"))
            .separator()
            .item(MenuItem::new("ctx-copy", "Copy…").icon("copy"))
            .item({
                // P3G.1 (ER1.2, ER X8): a folder of the library (the Other documents browser's
                // locations), or back to the top level.
                let current = lib.lib.get(row.0).and_then(|e| e.meta.folder);
                let mut entries: Vec<MenuEntry> = vec![MenuItem::new("ctx-move-top", "Documents (no folder)").checked(current.is_none()).into()];
                for (i, f) in lib.lib.folders.iter().enumerate() {
                    entries.push(MenuItem::new(format!("ctx-move-folder-{i}"), f.name.clone()).icon("folder").checked(current == Some(f.id)).into());
                }
                MenuItem::new("ctx-move", "Move to…").icon("move-to-folder").submenu(entries)
            })
            .item({
                // P3E.1: the labels as checkable items, and "Create new label…".
                let current = lib.lib.get(row.0).map(|e| e.meta.labels.clone()).unwrap_or_default();
                let mut entries: Vec<MenuEntry> = lib
                    .lib
                    .labels_by_name()
                    .iter()
                    // P3E.2 (P3E.1 judge): each label with its colour dot, a check mark on those given.
                    .map(|l| MenuItem::new(format!("ctx-label-{}", details::slug(&l.name)), l.name.clone()).dot(Color::srgb_u8(l.colour[0], l.colour[1], l.colour[2])).checked(current.contains(&l.id)).into())
                    .collect();
                if !entries.is_empty() {
                    entries.push(MenuEntry::Separator);
                }
                entries.push(MenuItem::new("ctx-new-label", "Create new label…").icon("tag-new").into());
                MenuItem::new("ctx-labels", "Labels").icon("tag").submenu(entries)
            })
            .item(MenuItem::new("ctx-properties", "Details…").icon("list-details"))
            .separator()
            .item(MenuItem::new("ctx-trash", "Move to trash").icon("delete"))
            .min_width(190.0)
    };
    let anchor = open_context_menu(&mut commands, ev.position, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((ContextDoc(row.0), DespawnOnExit(AppState::Landing)));
}

fn on_context_action(
    ev: On<MenuAction>,
    q_anchor: Query<&ContextDoc, With<ContextMenuAnchor>>,
    mut commands: Commands,
    theme: Res<Theme>,
    lib: Res<DocLibrary>,
    store: Res<DocumentStore>,
) {
    let Ok(doc) = q_anchor.get(ev.entity) else {
        return;
    };
    let id = doc.0;
    let Some(entry) = lib.lib.get(id).cloned() else {
        return;
    };
    match ev.item.as_str() {
        "ctx-open" => commands.queue(move |world: &mut World| open_document(world, id)),
        "ctx-rename" => open_rename_dialog(&mut commands, &theme, &entry),
        "ctx-copy" => open_copy_dialog(&mut commands, &theme, &entry),
        "ctx-trash" => open_trash_dialog(&mut commands, &theme, &entry),
        "ctx-delete-forever" => open_purge_dialog(&mut commands, &theme, &entry),
        "ctx-properties" => commands.queue(move |world: &mut World| details::select(world, Pick::Doc(id), true)),
        "ctx-new-label" => details::open_new_label_dialog(&mut commands, &theme, Some(id)),
        m if m.starts_with("ctx-label-") => {
            let slug = m.trim_start_matches("ctx-label-").to_string();
            let store = store.0.clone();
            commands.queue(move |world: &mut World| details::toggle_label_by_slug(world, &store, id, &slug));
        }
        // P3G.1: Move to ▸ a folder (undoable, like the other library commands).
        m if m == "ctx-move-top" || m.starts_with("ctx-move-folder-") => {
            let folder = m.strip_prefix("ctx-move-folder-").and_then(|i| i.parse::<usize>().ok()).and_then(|i| lib.lib.folders.get(i)).map(|f| f.id);
            let store = store.0.clone();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<DocLibrary>().execute(&store, &cadrs_core::library::MoveToFolder { id, folder });
            });
        }
        "ctx-restore" => {
            let store = store.0.clone();
            let name = entry.name.clone();
            commands.queue(move |world: &mut World| {
                world
                    .resource_mut::<DocLibrary>()
                    .execute(&store, &RestoreEntry { id });
                undo_toast(world, format!("Restored \u{201c}{name}\u{201d}"));
            });
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Dialogs

fn field_label(p: &mut ChildSpawner, t: &Theme, text: &str) {
    // Bold in `screens/03`; ExtraBold matches its ink here (Bevy draws text lighter).
    p.spawn(t.text(text, t.font_base, FontWeight::EXTRA_BOLD, t.foreground))
        .entry::<Node>()
        .and_modify(|mut n| n.margin = UiRect::bottom(Val::Px(-2.0)));
}

fn close_dialogs(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<LandingDialog>>();
    let roots: Vec<Entity> = q.iter(world).collect();
    for e in roots {
        world.trigger(DialogClose { entity: e });
    }
}

fn field_value(world: &mut World, field: &str) -> String {
    let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == field)
        .map(|(_, t)| t.value().to_string())
        .unwrap_or_default()
}

fn doc_name(world: &World, id: DocumentId) -> String {
    world
        .resource::<DocLibrary>()
        .lib
        .get(id)
        .map(|e| e.name.clone())
        .unwrap_or_default()
}

/// Shows a 4 s toast with an "Undo" link that undoes the last library command.
fn undo_toast(world: &mut World, text: String) {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let toast = cadrs_ui::show_toast_for(&mut commands, &theme, text, 4.0);
    let undo = cadrs_ui::toast_action(&mut commands, &theme, toast, "toast-undo", "Undo");
    commands.entity(undo).insert(observe(|_: On<Activate>, mut commands: Commands| {
        commands.queue(|world: &mut World| {
            let store = world.resource::<DocumentStore>().0.clone();
            world.resource_mut::<DocLibrary>().undo(&store);
            cadrs_ui::close_toasts(world);
        });
    }));
    world.flush();
}

/// Runs the dialog's primary action.
fn confirm_dialog(world: &mut World) {
    let mut q = world.query::<&LandingDialog>();
    let Some(dialog) = q.iter(world).next().copied() else {
        return;
    };
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    match dialog {
        LandingDialog::NewFolder => {
            let name = field_value(world, "new-folder-name-field");
            if name.trim().is_empty() {
                return;
            }
            close_dialogs(world);
            let id = cadrs_core::FolderId::new();
            world.resource_mut::<DocLibrary>().execute(
                &store,
                &CreateFolder {
                    id,
                    name,
                    user,
                    now,
                },
            );
            // Show the new folder.
            let mut state = world.resource_mut::<LandingState>();
            state.sections[2] = true;
            state.sections_visible = true;
        }
        LandingDialog::Purge(id) => {
            close_dialogs(world);
            let name = doc_name(world, id);
            world
                .resource_mut::<DocLibrary>()
                .execute(&store, &PurgeEntry { id });
            undo_toast(world, format!("Deleted \u{201c}{name}\u{201d} permanently"));
        }
        LandingDialog::NewDocument => {
            let name = field_value(world, "new-document-name-field");
            close_dialogs(world);
            create_document(world, &name);
        }
        LandingDialog::Rename(id) => {
            let name = field_value(world, "rename-name-field");
            if name.trim().is_empty() {
                return;
            }
            close_dialogs(world);
            let label = format!("Renamed to \u{201c}{}\u{201d}", name.trim());
            world.resource_mut::<DocLibrary>().execute(
                &store,
                &RenameEntry {
                    id,
                    name,
                    user,
                    now,
                },
            );
            undo_toast(world, label);
        }
        LandingDialog::Copy(source) => {
            let name = field_value(world, "copy-name-field");
            if name.trim().is_empty() {
                return;
            }
            close_dialogs(world);
            let id = DocumentId::new();
            match store.copy_document(source, id, &name, &user, now) {
                Ok(entry) => {
                    world
                        .resource_mut::<DocLibrary>()
                        .execute(&store, &AddEntry { entry });
                    undo_toast(world, format!("Copied to \u{201c}{}\u{201d}", name.trim()));
                }
                Err(e) => error!("cannot copy the document: {e}"),
            }
        }
        LandingDialog::Trash(id) => {
            close_dialogs(world);
            let name = doc_name(world, id);
            world
                .resource_mut::<DocLibrary>()
                .execute(&store, &TrashEntry { id, now });
            undo_toast(world, format!("Moved \u{201c}{name}\u{201d} to trash"));
        }
        LandingDialog::NewLabel(_)
        | LandingDialog::RenameLabel(_)
        | LandingDialog::DeleteLabel(_)
        | LandingDialog::SampleCopy(_) => details::confirm(world, dialog),
    }
}

fn dialog_buttons(p: &mut ChildSpawner, t: &Theme, prefix: &str, ok: &str) {
    p.spawn((
        Button::new(format!("{prefix}-ok")).label(ok).primary().build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(confirm_dialog);
        }),
    ));
    p.spawn((
        Button::new(format!("{prefix}-cancel"))
            .label("Cancel")
            .build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(close_dialogs);
        }),
    ));
}

fn on_text_submit(ev: On<TextSubmit>, q: Query<&Name>, mut commands: Commands) {
    if ev.entity != ev.original_event_target() {
        return;
    }
    let Ok(name) = q.get(ev.entity) else {
        return;
    };
    if matches!(
        name.as_str(),
        "new-document-name-field"
            | "rename-name-field"
            | "new-folder-name-field"
            | "copy-name-field"
            | "new-label-name-field"
            | "rename-label-name-field"
            | "sample-copy-name-field"
    ) {
        commands.queue(confirm_dialog);
    }
}

fn open_new_document_dialog(commands: &mut Commands, theme: &Theme) {
    let t = theme.clone();
    let tb = theme.clone();
    let tf = theme.clone();
    commands.spawn((
        Dialog::new("new-document-dialog")
            .title("New document")
            .width(548.0)
            .body(move |b| {
                let t = &tb;
                field_label(b, t, "Document name");
                b.spawn(
                    TextInput::new("new-document-name")
                        .value("Untitled document")
                        .select_all_on_focus()
                        .autofocus()
                        .build(t),
                )
                .entry::<Node>()
                .and_modify(|mut n| n.margin = UiRect::bottom(Val::Px(8.0)));
                field_label(b, t, "Document labels");
                b.spawn(
                    TextInput::new("new-document-labels")
                        .placeholder("Search labels")
                        .disabled(true)
                        .height(32.0)
                        .build(t),
                )
                .entry::<Node>()
                .and_modify(|mut n| n.margin = UiRect::bottom(Val::Px(8.0)));
                field_label(b, t, "Document location");
                location_browser(b, t);
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("new-document-create")
                        .label("Create document")
                        .primary()
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(confirm_dialog);
                    }),
                ));
                f.spawn((
                    Button::new("new-document-cancel")
                        .label("Cancel")
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close_dialogs);
                    }),
                ));
            })
            .build(&t),
        LandingDialog::NewDocument,
        DespawnOnExit(AppState::Landing),
    ));
}

/// The "Document location" box: a folder browser that only knows the home folder for now.
fn location_browser(b: &mut ChildSpawner, t: &Theme) {
    b.spawn((
        Name::new("new-document-location"),
        Node {
            flex_direction: FlexDirection::Column,
            height: Val::Px(222.0),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(t.radius)),
            ..default()
        },
        BorderColor::all(t.input_border),
    ))
    .with_children(|l| {
        l.spawn((
            Node {
                height: Val::Px(35.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(t.separator),
        ))
        .with_children(|h| {
            h.spawn(icon("home", 18.0, t.muted_foreground));
            h.spawn(icon("chevron-left", 16.0, t.muted_foreground));
            h.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                flex_grow: 1.0,
                ..default()
            })
            .with_children(|c| {
                c.spawn(icon("owned-by-me", 16.0, t.foreground));
                c.spawn(t.text("Owned by me", t.font_base, FontWeight::BOLD, t.foreground));
            });
            h.spawn(icon("sort-descending", 16.0, t.foreground));
        });
        l.spawn(Node {
            justify_content: JustifyContent::Center,
            padding: UiRect::top(Val::Px(14.0)),
            ..default()
        })
        .with_children(|c| {
            c.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(13.0), Val::Px(14.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(t.radius)),
                    ..default()
                },
                BackgroundColor(t.info_background),
                BorderColor::all(t.info_border),
            ))
            .with_child(t.text(
                "No additional folders",
                t.font_base,
                FontWeight::NORMAL,
                Color::srgb_u8(0x2c, 0x4a, 0x66),
            ));
        });
    });
}

fn open_rename_dialog(commands: &mut Commands, theme: &Theme, entry: &DocumentEntry) {
    let t = theme.clone();
    let tb = theme.clone();
    let tf = theme.clone();
    let name = entry.name.clone();
    commands.spawn((
        Dialog::new("rename-dialog")
            .title("Rename document")
            .width(420.0)
            .body(move |b| {
                field_label(b, &tb, "Document name");
                b.spawn(
                    TextInput::new("rename-name")
                        .value(name)
                        .select_all_on_focus()
                        .autofocus()
                        .build(&tb),
                );
            })
            .footer(move |f| dialog_buttons(f, &tf, "rename-dialog", "Rename"))
            .build(&t),
        LandingDialog::Rename(entry.id),
        DespawnOnExit(AppState::Landing),
    ));
}

/// Copy…: the new document's name, prefilled "Copy of <name>" and selected.
fn open_copy_dialog(commands: &mut Commands, theme: &Theme, entry: &DocumentEntry) {
    let t = theme.clone();
    let tb = theme.clone();
    let tf = theme.clone();
    let name = format!("Copy of {}", entry.name);
    commands.spawn((
        Dialog::new("copy-dialog")
            .title("Copy document")
            .width(420.0)
            .body(move |b| {
                field_label(b, &tb, "New document name");
                b.spawn(
                    TextInput::new("copy-name")
                        .value(name)
                        .select_all_on_focus()
                        .autofocus()
                        .build(&tb),
                );
            })
            .footer(move |f| dialog_buttons(f, &tf, "copy-dialog", "Copy"))
            .build(&t),
        LandingDialog::Copy(entry.id),
        DespawnOnExit(AppState::Landing),
    ));
}

fn open_trash_dialog(commands: &mut Commands, theme: &Theme, entry: &DocumentEntry) {
    let t = theme.clone();
    let tb = theme.clone();
    let tf = theme.clone();
    let name = entry.name.clone();
    commands.spawn((
        Dialog::new("trash-dialog")
            .title("Move to trash")
            .width(420.0)
            .body(move |b| {
                b.spawn((
                    Text::new(format!("Move \u{201c}{name}\u{201d} to the trash?")),
                    tb.font(tb.font_base, FontWeight::NORMAL),
                    TextColor(tb.foreground),
                ));
                b.spawn(tb.text(
                    "You can restore it from Trash.",
                    tb.font_base,
                    FontWeight::NORMAL,
                    tb.muted_foreground,
                ));
            })
            .footer(move |f| dialog_buttons(f, &tf, "trash-dialog", "Move to trash"))
            .build(&t),
        LandingDialog::Trash(entry.id),
        DespawnOnExit(AppState::Landing),
    ));
}

fn open_new_folder_dialog(commands: &mut Commands, theme: &Theme) {
    let t = theme.clone();
    let tb = theme.clone();
    let tf = theme.clone();
    commands.spawn((
        Dialog::new("new-folder-dialog")
            .title("New folder")
            .width(420.0)
            .body(move |b| {
                field_label(b, &tb, "Folder name");
                b.spawn(
                    TextInput::new("new-folder-name")
                        .value("Untitled folder")
                        .select_all_on_focus()
                        .autofocus()
                        .build(&tb),
                );
            })
            .footer(move |f| dialog_buttons(f, &tf, "new-folder-dialog", "Create"))
            .build(&t),
        LandingDialog::NewFolder,
        DespawnOnExit(AppState::Landing),
    ));
}

fn open_purge_dialog(commands: &mut Commands, theme: &Theme, entry: &DocumentEntry) {
    let t = theme.clone();
    let tb = theme.clone();
    let tf = theme.clone();
    let name = entry.name.clone();
    commands.spawn((
        Dialog::new("purge-dialog")
            .title("Delete permanently")
            .width(420.0)
            .body(move |b| {
                b.spawn((
                    Text::new(format!("Permanently delete \u{201c}{name}\u{201d}?")),
                    tb.font(tb.font_base, FontWeight::NORMAL),
                    TextColor(tb.foreground),
                ));
                b.spawn(tb.text(
                    "You cannot restore it from Trash.",
                    tb.font_base,
                    FontWeight::NORMAL,
                    tb.muted_foreground,
                ));
            })
            .footer(move |f| dialog_buttons(f, &tf, "purge-dialog", "Delete permanently"))
            .build(&t),
        LandingDialog::Purge(entry.id),
        DespawnOnExit(AppState::Landing),
    ));
}
