//! The documents page, on the glass wallpaper ([`cadrs_ui::glass`]).
//!
//! - Top bar: the app menu (the cadrs mark, [`crate::app_menu`]), whose first items here are
//!   Open documents folder, Labels ▸ (each shows its documents) and Trash ([`app_entries`]);
//!   the version, notifications ([`crate::notifications`]), the color scheme and the account
//!   ([`crate::identity`]).
//! - A command strip along the top, one height and small capitals: **New document** (with ▾
//!   for Join shared document…, From a sample…, …), Folder, the search box; at the right
//!   Import, Label, the list and grid toggles and the Type filter.
//! - The list in a glass card: a caption (DOCUMENTS and the count, or the Trash, a label or the
//!   samples with ‹ back), and the sortable columns NAME | MODIFIED | MODIFIED BY | OWNED BY.
//!
//! Clicking a row selects it; double-clicking it (or Enter) opens it (P3E.1). Right-clicking it
//! selects it and opens a context menu with Rename… (a dialog), Labels ▸, Properties…,
//! History…, Used in… (modals, in [`details`]), Move to trash (a confirmation dialog); in the
//! Trash, Restore. Rename, trash and restore are [`LibraryCommand`]s, so Ctrl+Z / Ctrl+Shift+Z
//! undo and redo them.
//!
//! P3E.1 (TD3, TD4.1, X5) adds, in [`details`]: labels (Create ▸ Label…, the Label button, a
//! label's list with Rename… and Delete… by its caption, a Labels column, the Properties
//! modal's searchable checkboxes and "Create new label"), folders that open (a breadcrumb and
//! Back), the Type filter and a grid view, the bundled samples ("Open a copy"), and Import
//! files… (a STEP or STL file becomes a new document whose Part Studio holds its Import). Every
//! library change is an undoable library command.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
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
use cadrs_ui::input::TextInputField;
use cadrs_ui::menu::ContextMenuAnchor;
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, ShortcutKey};

use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};

#[path = "landing_details.rs"]
mod details;
#[path = "landing_organize.rs"]
mod organize;
pub use details::import_file as import_mesh_document;
pub use details::{DetailsTab, LandingDetails, Pick};

pub struct LandingPlugin;

impl Plugin for LandingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LandingState>()
            .init_resource::<DocLibrary>()
            .init_resource::<Thumbnails>()
            .init_resource::<LandingDetails>()
            .init_resource::<details::SampleThumbs>()
            .add_systems(Update, details::poll_import_thumbnails)
            .add_systems(
                OnEnter(AppState::Landing),
                (load_library, spawn_landing).chain(),
            )
            .add_systems(
                Update,
                (sync_search, rebuild_main, landing_shortcuts).run_if(in_state(AppState::Landing)),
            )
            .add_systems(
                Update,
                (
                    details::rebuild_details,
                    details::filter_label_checks,
                    details::commit_description_on_blur,
                    details::on_import_picked,
                )
                    .run_if(in_state(AppState::Landing)),
            )
            .add_observer(details::on_pick_activate)
            .add_observer(details::on_pick_double_click)
            .add_observer(details::on_label_check)
            .add_observer(details::on_sample_context_menu)
            .add_observer(details::on_sample_context_action)
            .add_observer(details::on_details_dialog_close)
            .add_observer(details::on_details_button)
            .add_observer(details::on_description_submit)
            .add_observer(on_sort_change)
            .add_observer(on_text_submit)
            .add_observer(on_row_context_menu)
            .add_observer(on_row_more)
            .add_observer(on_context_action);
        organize::plugin(app);
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
    /// Whether the user picked a sort; the default sort shows no arrow, like the reference.
    pub sort_touched: bool,
    /// The folder opened (P3E.1, TD3.6): the list shows its documents under a breadcrumb.
    pub folder: Option<FolderId>,
    /// The list's Type filter (P3E.1, TD3.7).
    pub item_type: ItemType,
    /// The grid view instead of the list (P3E.1, TD3.7).
    pub grid: bool,
}

impl Default for LandingState {
    fn default() -> Self {
        Self {
            filter: Filter::OwnedByMe,
            sort_key: SortKey::Modified,
            sort_dir: SortDir::Descending,
            search: String::new(),
            sort_touched: false,
            folder: None,
            item_type: ItemType::All,
            grid: false,
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
struct Thumbnails(
    HashMap<DocumentId, Option<Handle<Image>>>,
    HashMap<DocumentId, Option<Handle<Image>>>,
);

/// The main panel, rebuilt whenever the list changes.
#[derive(Component)]
struct MainPanel;

/// The page's title, rebuilt with the list.
#[derive(Component)]
struct PageTitle;

/// The slot right of the search box for the list's view toggles and Type filter, rebuilt with
/// the list.
#[derive(Component)]
struct ListTools;

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
    /// Join shared document (`crate::collab`): a link to paste.
    Join,
    /// #112: a folder's Rename…, Copy… and Delete permanently….
    RenameFolder(FolderId),
    CopyFolder(FolderId),
    PurgeFolder(FolderId),
    /// Delete permanently… of several items in the Trash (`organize`).
    PurgeItems,
    /// Empty trash… (`organize`).
    EmptyTrash,
}

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

/// The filter's title: its label, or the label's name (P3E.1).
fn filter_title(f: Filter, lib: &Library) -> String {
    match f {
        Filter::Label(id) => lib
            .label(id)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| "Label".into()),
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
    thumbs
        .0
        .entry(id)
        .or_insert_with(|| thumbnail_image(images, store.read_thumbnail(id)?))
        .clone()
}

/// The document's large thumbnail (the details panel's; its small one if it has none).
fn large_thumbnail(
    thumbs: &mut Thumbnails,
    images: &mut Assets<Image>,
    store: &cadrs_core::Store,
    id: DocumentId,
) -> Option<Handle<Image>> {
    thumbs
        .1
        .entry(id)
        .or_insert_with(|| thumbnail_image(images, store.read_thumbnail_large(id)?))
        .clone()
}

/// A decoded thumbnail as an image.
fn thumbnail_image(images: &mut Assets<Image>, img: image::RgbaImage) -> Option<Handle<Image>> {
    let (w, h) = img.dimensions();
    Some(images.add(Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        img.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )))
}

// ---------------------------------------------------------------------------------------------
// Layout

fn spawn_landing(mut commands: Commands, theme: Res<Theme>, profile: Res<UserProfile>) {
    let t = theme.clone();
    commands
        .spawn((
            Name::new("landing"),
            bevy::input_focus::tab_navigation::TabGroup::new(0),
            DespawnOnExit(AppState::Landing),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                // A root sits in Bevy's implicit viewport grid, whose row (taffy 0.14, Bevy 0.20)
                // grows to the root's content unless the root's minimum is 0: then 100% is the
                // window, and long lists scroll inside it instead of stretching the screen.
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(t.background),
            cadrs_ui::glass::wallpaper(),
        ))
        .with_children(|root| {
            top_bar(root, &t, &profile);
            // The page: a column, centered in a wide window.
            root.spawn((
                Name::new("landing-page"),
                // On the top bar's grid: the same 8 px from the window's edges.
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    ..default()
                },
            ))
            .with_children(|page| {
                actions(page, &t);
                page.spawn(Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    column_gap: Val::Px(16.0),
                    ..default()
                })
                .with_children(|body| documents_card(body, &t));
            });
        });
}

/// The bar across the top: the app menu, then the version, notifications, the color scheme
/// and the account.
fn top_bar(p: &mut ChildSpawnerCommands, t: &Theme, profile: &UserProfile) {
    // Floating, as a document's top bar is.
    p.spawn((
        Name::new("top-bar"),
        Node {
            height: Val::Px(44.0),
            flex_shrink: 0.0,
            margin: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(8.0), Val::ZERO),
            padding: UiRect::horizontal(Val::Px(6.0)),
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(t.radius_panel)),
            ..default()
        },
        cadrs_ui::glass::panel(t),
    ))
    .with_children(|bar| {
        crate::app_menu::spawn_app_button(bar, t);
        bar.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        // Right side: the version, the network, notifications, the color scheme and the account.
        bar.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|r| {
            crate::update::spawn_version(r, t);
            crate::network::spawn_button(r, t);
            crate::notifications::spawn_bell(r, t);
            // Light, dark or the system's scheme.
            r.spawn((
                cadrs_ui::scheme_toggle(t),
                cadrs_ui::glass::GlassFrame::bar(),
            ));
            crate::identity::spawn_identity_button(r, t, profile);
        });
    });
}

/// The card's caption for the list shown: DOCUMENTS, or the Trash, a label or the samples
/// with a way back; and how many items are listed.
fn fill_heading(
    h: &mut ChildSpawnerCommands,
    t: &Theme,
    filter: Filter,
    lib: &Library,
    count: usize,
) {
    if filter != Filter::OwnedByMe {
        h.spawn((
            Button::new("filter-back")
                .label("All documents")
                .icon("chevron-left")
                .icon_size(14.0)
                .ghost()
                .small()
                .tooltip("Back to all documents")
                .build(t),
            observe(|_: On<Activate>, mut state: ResMut<LandingState>| {
                state.filter = Filter::OwnedByMe;
                state.folder = None;
            }),
        ));
    }
    match filter {
        Filter::Label(id) => {
            let [r, g, b] = lib.label(id).map_or([128; 3], |l| l.colour);
            h.spawn((
                Node {
                    width: Val::Px(9.0),
                    height: Val::Px(9.0),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(r, g, b)),
            ));
        }
        Filter::OwnedByMe => {}
        f => {
            h.spawn(icon(filter_icon(f), 15.0, t.muted_foreground));
        }
    }
    let title = match filter {
        Filter::OwnedByMe => "Documents".to_string(),
        Filter::Explore => "Samples".to_string(),
        f => filter_title(f, lib),
    };
    h.spawn((
        Name::new("page-title"),
        t.text(
            title.to_uppercase(),
            t.font_sm,
            FontWeight::BOLD,
            t.muted_foreground,
        ),
    ));
    h.spawn((
        Name::new("page-count"),
        t.text(
            count.to_string(),
            t.font_sm,
            FontWeight::MEDIUM,
            t.subtle_foreground,
        ),
    ));
    // A label's Rename… and Delete… (P3E.1).
    if let Filter::Label(id) = filter {
        h.spawn((
            IconButton::new("label-more", "more-horizontal")
                .icon_size(16.0)
                .small()
                .tooltip("Label")
                .build(t),
            observe(
                |a: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                    let menu = Menu::new("label-menu")
                        .item(MenuItem::new("label-rename", "Rename…").icon("edit"))
                        .item(MenuItem::new("label-delete", "Delete…").icon("delete"))
                        .min_width(160.0);
                    open_menu(&mut commands, a.entity, menu.build(&theme));
                },
            ),
            observe(move |ev: On<MenuAction>, mut commands: Commands| {
                let item = ev.item.clone();
                commands.queue(move |w: &mut World| details::label_action(w, id, &item));
            }),
        ));
    }
}

/// The page's control size and shape, with room for an icon and a label.
pub(crate) fn page_control(n: &mut Node) {
    cadrs_ui::glass::shape_control(
        n,
        cadrs_ui::glass::CONTROL_HEIGHT,
        cadrs_ui::glass::CONTROL_RADIUS,
    );
    n.padding = UiRect::horizontal(Val::Px(12.0));
    n.column_gap = Val::Px(7.0);
}

/// The command strip along the top of the page: New document, Folder and the search box, then
/// (in [`ListTools`], rebuilt with the list) Import, Label, the view toggles and the Type
/// filter. One height, small capitals.
fn actions(p: &mut ChildSpawnerCommands, t: &Theme) {
    p.spawn((
        Name::new("landing-actions"),
        cadrs_ui::spring::Appear::fly_up(),
        Node {
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        },
    ))
    .with_children(|a| {
        create_button(a, t);
        a.spawn((
            Button::new("new-folder-button")
                .label("New folder")
                .icon("folder-new")
                .icon_size(16.0)
                .outline()
                .tooltip("New folder")
                .build(t),
            observe(
                |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                    open_new_folder_dialog(&mut commands, &theme);
                },
            ),
        ))
        .insert(cadrs_ui::glass::control(t))
        .entry::<Node>()
        .and_modify(|mut n| page_control(&mut n));
        a.spawn(
            TextInput::new("search")
                .placeholder("Search…")
                .leading_icon("search")
                .height(cadrs_ui::glass::CONTROL_HEIGHT)
                .width(Val::Px(260.0))
                .padding(12.0)
                .radius(cadrs_ui::glass::CONTROL_RADIUS)
                .background(t.control)
                .border(Color::NONE)
                .cleanable()
                .build(t),
        )
        .insert(cadrs_ui::glass::Glass::input_rim())
        .entry::<Node>()
        .and_modify(|mut n| {
            n.min_width = Val::Px(180.0);
            n.max_width = Val::Px(520.0);
            n.flex_grow = 1.0;
        });
        a.spawn(Node {
            flex_grow: 1.0,
            min_width: Val::Px(8.0),
            ..default()
        });
        a.spawn((
            Name::new("list-tools"),
            ListTools,
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
        ));
    });
}

/// **New document** — at once — with a ▾ for the rest of the Create menu (Join shared
/// document…, From a sample…, …), in a soft glow: the page's one accent.
fn create_button(p: &mut ChildSpawnerCommands, t: &Theme) {
    p.spawn((
        Name::new("create-group"),
        Node {
            height: Val::Px(cadrs_ui::glass::CONTROL_HEIGHT),
            flex_shrink: 0.0,
            border_radius: BorderRadius::all(Val::Px(cadrs_ui::glass::CONTROL_RADIUS)),
            ..default()
        },
        cadrs_ui::glass::glow(t),
    ))
    .with_children(|g| {
        g.spawn((
            Button::new("create-document")
                .label("New document")
                .icon("file-new")
                .icon_size(16.0)
                .primary()
                .tooltip("Create a new document")
                .build(t),
            observe(
                |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                    open_new_document_dialog(&mut commands, &theme);
                },
            ),
        ))
        .entry::<Node>()
        .and_modify(|mut n| {
            page_control(&mut n);
            n.height = Val::Percent(100.0);
            n.border_radius = BorderRadius::left(Val::Px(cadrs_ui::glass::CONTROL_RADIUS));
        });
        g.spawn((
            IconButton::new("create", "caret-down")
                .primary()
                .icon_size(14.0)
                .tooltip("More ways to start: join a shared document, open a sample")
                .build(t),
            observe(open_create_menu),
            observe(on_create_menu_action),
        ))
        .entry::<Node>()
        .and_modify(|mut n| {
            n.height = Val::Percent(100.0);
            n.width = Val::Px(26.0);
            n.margin = UiRect::left(Val::Px(1.0));
            n.border_radius = BorderRadius::right(Val::Px(cadrs_ui::glass::CONTROL_RADIUS));
        });
    });
}

/// The list's card.
fn documents_card(p: &mut ChildSpawnerCommands, t: &Theme) {
    p.spawn((
        Name::new("documents-card"),
        Node {
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            padding: UiRect::new(Val::Px(12.0), Val::Px(8.0), Val::Px(6.0), Val::Px(6.0)),
            ..cadrs_ui::glass::panel_node(t)
        },
        cadrs_ui::glass::panel(t),
        cadrs_ui::spring::Appear::fly_up().delay(0.05),
    ))
    .with_children(|card| {
        // The caption, filled by `rebuild_main`.
        card.spawn((
            Name::new("landing-heading"),
            PageTitle,
            Node {
                flex_shrink: 0.0,
                height: Val::Px(30.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                padding: UiRect::left(Val::Px(4.0)),
                ..default()
            },
        ));
        card.spawn((
            Name::new("main-panel"),
            MainPanel,
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ));
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
    // The name takes twice the others' share of the free width, so a narrow window still
    // shows every column.
    let mut v = vec![
        Column::new("name", "Name")
            .grow(2.0)
            .min_width(200.0)
            .sortable()
            .sort(sort(SortKey::Name)),
    ];
    if labels {
        v.push(Column::new("labels", "Labels").width(200.0));
    }
    v.push(
        Column::new("modified", "Modified")
            .grow(1.0)
            .min_width(150.0)
            .sortable()
            .sort(sort(SortKey::Modified))
            .first_sort(ColumnSort::Descending),
    );
    v.push(
        Column::new("modified-by", "Modified by")
            .grow(0.8)
            .min_width(110.0)
            .sortable()
            .sort(sort(SortKey::ModifiedBy)),
    );
    v.push(
        Column::new("owned-by", "Owned by")
            .grow(0.8)
            .min_width(110.0)
            .sortable()
            .sort(sort(SortKey::OwnedBy)),
    );
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
    q_tools: Query<Entity, With<ListTools>>,
    q_title: Query<Entity, With<PageTitle>>,
) {
    let Ok((panel, marker)) = q_panel.single() else {
        return;
    };
    if !(marker.is_added() || state.is_changed() || lib.is_changed()) {
        return;
    }
    let t = theme.clone();
    commands.entity(panel).despawn_related::<Children>();
    // The list's view toggles and Type filter (P3E.1), right of the search box; the samples
    // have neither.
    for tools in &q_tools {
        commands.entity(tools).despawn_related::<Children>();
        if state.filter != Filter::Explore {
            commands
                .entity(tools)
                .with_children(|a| details::list_toolbar(a, &t, &state, &lib.lib));
        }
    }
    let picked = details_state.pick;
    // A folder in the trash (or in one that is) isn't opened.
    let folder = state
        .folder
        .filter(|f| state.filter == Filter::Trash || !lib.lib.folder_hidden(*f))
        .and_then(|f| lib.lib.folder(f).cloned());
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
    let has_labels = !lib.lib.labels.is_empty();
    let cols = columns(&state, has_labels);
    // Folders are listed above the documents: at the top level or
    // in the folder open (#112: folders nest; a search finds them anywhere), and in the Trash
    // the folders moved there.
    let folder_rows: Vec<FolderListing> = if state.item_type.shows_folders()
        && (matches!(state.filter, Filter::OwnedByMe | Filter::CreatedByMe)
            || (state.filter == Filter::Trash && folder.is_none()))
    {
        let needle = state.search.trim().to_lowercase();
        let trash = state.filter == Filter::Trash;
        let mut v: Vec<&cadrs_core::FolderEntry> = lib
            .lib
            .folders
            .iter()
            .filter(|f| {
                let placed = if trash {
                    // Only the folders moved to the trash themselves (their contents go with
                    // them).
                    f.trashed.is_some() && f.parent.is_none_or(|p| !lib.lib.folder_hidden(p))
                } else if !needle.is_empty() && folder.is_none() {
                    !lib.lib.folder_hidden(f.id)
                } else {
                    f.trashed.is_none() && f.parent == folder.as_ref().map(|x| x.id)
                };
                placed
                    && f.owned_by == user.id
                    && (needle.is_empty() || f.name.to_lowercase().contains(&needle))
            })
            .collect();
        v.sort_by(|a, b| cadrs_core::library::natural_cmp(&a.name, &b.name));
        v.into_iter()
            .map(|f| FolderListing {
                id: f.id,
                name: f.name.clone(),
                date: clock.format(f.created),
            })
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
    let samples_list = if state.filter == Filter::Explore {
        Some(details::sample_rows(&mut samples, &mut images))
    } else {
        None
    };
    // P3E.2 (P3E.1 judge): the selection is cleared when the selected item leaves the view
    // (moved to the trash, deleted, or not listed under the new filter), and the Details panel
    // with it.
    let visible = match picked {
        None => true,
        Some(Pick::Doc(id)) => rows.iter().any(|r| r.0.id == id),
        Some(Pick::Folder(f)) => folder_rows.iter().any(|x| x.id == f),
        Some(Pick::Sample(_)) => state.filter == Filter::Explore,
    };
    let picked = if visible {
        picked
    } else {
        commands.queue(|w: &mut World| w.resource_mut::<LandingDetails>().pick = None);
        None
    };
    // #112: the selection keeps what is still listed; a Shift+click range runs over the list
    // as shown.
    let order: Vec<Pick> = folder_rows
        .iter()
        .map(|f| Pick::Folder(f.id))
        .chain(rows.iter().map(|r| Pick::Doc(r.0.id)))
        .collect();
    let selection: Vec<Pick> = details_state
        .selection
        .iter()
        .copied()
        .filter(|p| order.contains(p) || Some(*p) == picked)
        .collect();
    if selection != details_state.selection {
        let keep = selection.clone();
        commands.queue(move |w: &mut World| w.resource_mut::<LandingDetails>().selection = keep);
    }
    let count = samples_list.as_ref().map_or(order.len(), |l| l.len());
    commands.insert_resource(details::ListOrder(order));
    for h in &q_title {
        commands.entity(h).despawn_related::<Children>();
        commands
            .entity(h)
            .with_children(|h| fill_heading(h, &t, state.filter, &lib.lib, count));
    }
    let title = match state.filter {
        Filter::OwnedByMe => "Documents".to_string(),
        f => filter_title(f, &lib.lib),
    };
    let folder_path: Vec<(FolderId, String)> = folder
        .as_ref()
        .map(|f| {
            lib.lib
                .folder_path(f.id)
                .into_iter()
                .map(|x| (x.id, x.name))
                .collect()
        })
        .unwrap_or_default();
    let labels = lib.lib.labels.clone();
    let state = state.clone();
    let user = user.clone();

    commands.entity(panel).with_children(|m| {
        // Inside a folder, a breadcrumb (P3E.1, TD3.6).
        if folder.is_some() {
            m.spawn((
                Name::new("heading"),
                Node {
                    height: Val::Px(40.0),
                    flex_shrink: 0.0,
                    padding: UiRect::left(Val::Px(4.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    ..default()
                },
            ))
            .with_children(|h| details::breadcrumb(h, &t, state.filter, &title, &folder_path));
        }

        if let Some(list) = samples_list {
            details::samples_table(m, &t, list, picked);
            return;
        }

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
                    "Drag documents or folders here, or right-click one and choose Move to…."
                        .into(),
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
                    "Give documents this label in the Details panel or with right-click ▸ Labels."
                        .into(),
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
            details::grid(
                m,
                &t,
                &folder_rows_simple(&folder_rows),
                rows,
                &selection,
                &labels,
            );
            return;
        }

        // The list. The header leaves the scrollbar's room, as the rows do, so the columns line
        // up.
        m.spawn(TableHeader::new("col", cols.clone()).height(30.0).build(&t))
            .entry::<Node>()
            .and_modify(|mut n| n.padding.right = Val::Px(14.0));
        m.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|wrap| {
            let list = wrap
                .spawn((
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
                        let mut row =
                            TableRow::new(format!("folder-row-{i}"), &cols).cell(move |c| {
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
                            .selected(selection.contains(&Pick::Folder(f.id)));
                        let more = format!("folder-more-{i}");
                        list.spawn((
                            row.build(&t),
                            PickRow(Pick::Folder(f.id)),
                            organize::FolderRow(f.id),
                            organize::DropFolder(Some(f.id)),
                            cadrs_ui::DoubleClickable,
                        ))
                        .with_children(|r| row_more_button(r, &t, more))
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
                        let mut row = TableRow::new(format!("doc-row-{i}"), &cols).cell(move |c| {
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
                            // Main goes without saying: only another workspace is tagged.
                            if workspace == "Main" {
                                return;
                            }
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
                                .map(|l| {
                                    (
                                        format!("doc-label-{i}-{}", details::slug(&l.name)),
                                        l.name.clone(),
                                        l.colour,
                                    )
                                })
                                .collect();
                            let tc = t.clone();
                            row = row.cell(move |c| {
                                c.spawn((
                                    Node {
                                        column_gap: Val::Px(4.0),
                                        overflow: Overflow::clip(),
                                        ..default()
                                    },
                                    Pickable::IGNORE,
                                ))
                                .with_children(|w| {
                                    for (name, label, colour) in chips {
                                        w.spawn(
                                            cadrs_ui::LabelChip::new(name, label, colour)
                                                .build(&tc),
                                        );
                                    }
                                });
                            });
                        }
                        let row = row
                            .text_cell(&t, date)
                            .text_cell(&t, user.display(&e.meta.modified_by))
                            .text_cell(&t, user.display(&e.meta.owned_by))
                            .selected(selection.contains(&Pick::Doc(e.id)));
                        list.spawn((
                            row.build(&t),
                            DocRow(e.id),
                            PickRow(Pick::Doc(e.id)),
                            cadrs_ui::DoubleClickable,
                        ))
                        .with_children(|r| row_more_button(r, &t, format!("doc-more-{i}")))
                        .entry::<Node>()
                        .and_modify(|mut n| n.height = Val::Px(38.5));
                    }
                })
                .id();
            wrap.spawn(cadrs_ui::vertical_scrollbar(&t, "doc-list-scrollbar", list));
        });
    });
}

/// The ⋯ at a row's right end: the row's menu, for whoever doesn't right-click.
#[derive(Component)]
struct RowMore;

fn row_more_button(r: &mut ChildSpawnerCommands, t: &Theme, name: String) {
    r.spawn((
        IconButton::new(name, "more-horizontal")
            .icon_size(16.0)
            .tooltip("More actions")
            .build(t),
        RowMore,
    ))
    .entry::<Node>()
    .and_modify(|mut n| {
        n.position_type = PositionType::Absolute;
        n.right = Val::Px(8.0);
        n.top = Val::Percent(50.0);
        n.margin.top = Val::Px(-13.0);
        n.width = Val::Px(26.0);
        n.height = Val::Px(26.0);
    });
}

/// The ⋯ opens the row's context menu under it.
fn on_row_more(
    a: On<Activate>,
    q: Query<(&ChildOf, &ComputedNode, &bevy::ui::UiGlobalTransform), With<RowMore>>,
    mut commands: Commands,
) {
    let Ok((parent, node, tf)) = q.get(a.entity) else {
        return;
    };
    let s = node.inverse_scale_factor();
    let bottom_right = (tf.translation + node.size() / 2.0) * s;
    commands.trigger(ContextMenuRequested {
        entity: parent.parent(),
        position: Vec2::new(bottom_right.x - 190.0, bottom_right.y + 2.0),
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
            e.spawn(Node {
                column_gap: Val::Px(8.0),
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            })
            .with_children(|r| {
                r.spawn((
                    Button::new("empty-create")
                        .label("New document")
                        .icon("file-new")
                        .primary()
                        .build(t),
                    observe(
                        |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                            open_new_document_dialog(&mut commands, &theme);
                        },
                    ),
                ))
                .entry::<Node>()
                .and_modify(|mut n| page_control(&mut n));
                r.spawn((
                    Button::new("empty-samples")
                        .label("Open a sample")
                        .icon("idea")
                        .outline()
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| show_filter(w, Filter::Explore));
                    }),
                ))
                .insert(cadrs_ui::glass::control(t))
                .entry::<Node>()
                .and_modify(|mut n| page_control(&mut n));
            });
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Sync systems

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

// ---------------------------------------------------------------------------------------------
// Interaction

#[allow(clippy::too_many_arguments)]
fn landing_shortcuts(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<cadrs_ui::KeyboardField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    mut lib: ResMut<DocLibrary>,
    store: Res<DocumentStore>,
    details_state: Res<LandingDetails>,
    state: Res<LandingState>,
    theme: Res<Theme>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    mut busy: Local<u8>,
    mut commands: Commands,
) {
    if focus.get().is_some_and(|f| q_fields.contains(f))
        || !q_dialogs.is_empty()
        || !q_menus.is_empty()
    {
        // An Enter that a dialog, field or menu took in this frame doesn't open the selection.
        *busy = 2;
        keys_in.clear();
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
        keys_in.clear();
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let pressed: Vec<KeyCode> = keys_in
        .read()
        .filter(|k| k.state == ButtonState::Pressed)
        .map(|k| k.shortcut_code())
        .collect();
    // Delete moves the selection to the trash; in the Trash it deletes it permanently, asking
    // first.
    if pressed.contains(&KeyCode::Delete) && !ctrl && !shift {
        let items: Vec<Pick> = details_state
            .selection
            .iter()
            .copied()
            .filter(|p| !matches!(p, Pick::Sample(_)))
            .collect();
        if state.filter == Filter::Trash {
            organize::open_purge_items_dialog(&mut commands, &theme, &lib.lib, items);
        } else if !items.is_empty() {
            commands.queue(move |world: &mut World| organize::trash_items(world, &items));
        }
        return;
    }
    let changed = if ctrl && pressed.contains(&KeyCode::KeyZ) {
        if shift {
            lib.redo(&store.0)
        } else {
            lib.undo(&store.0)
        }
    } else if ctrl && pressed.contains(&KeyCode::KeyY) {
        lib.redo(&store.0)
    } else {
        false
    };
    // A toast naming the undone (or redone) action is stale.
    if changed {
        commands.queue(cadrs_ui::close_transient_toasts);
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

fn create_menu(t: &Theme, name: &'static str) -> impl Bundle {
    // Dark (#333) icons, as in `screens/02`.
    Menu::new(name)
        .strong_icons()
        .item(MenuItem::new("new-document", "New document…").icon("file-new"))
        .item(MenuItem::new("new-folder", "New folder…").icon("folder-new"))
        .separator()
        .item(MenuItem::new("import-files", "Import files…").icon("file-import"))
        .item(
            MenuItem::new("import-from", "Import from")
                .icon("cloud-upload")
                .submenu(vec![
                    MenuItem::new("import-from-dropbox", "Dropbox…")
                        .planned()
                        .into(),
                    MenuItem::new("import-from-google-drive", "Google Drive…")
                        .planned()
                        .into(),
                    MenuItem::new("import-from-onedrive", "OneDrive…")
                        .planned()
                        .into(),
                ]),
        )
        .item(MenuItem::new("new-label", "New label…").icon("tag-new"))
        .item(MenuItem::new("open-samples", "From a sample…").icon("idea"))
        .separator()
        .item(MenuItem::new("join-shared", "Join shared document…").icon("share"))
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
            commands.queue(|w: &mut World| {
                crate::import_file::start(w, crate::import_file::ImportTarget::NewDocument)
            });
        }
        "new-label" => details::open_new_label_dialog(&mut commands, &theme, None),
        "open-samples" => commands.queue(|w: &mut World| show_filter(w, Filter::Explore)),
        "join-shared" => open_join_dialog(&mut commands, &theme),
        _ => {}
    }
}

/// Shows `filter`'s list (the Trash, a label, the samples) from its top level.
pub(crate) fn show_filter(world: &mut World, filter: Filter) {
    let mut state = world.resource_mut::<LandingState>();
    if state.filter != filter {
        state.filter = filter;
    }
    if state.folder.is_some() {
        state.folder = None;
    }
}

/// Opens the folder documents are kept in, in the file manager.
pub(crate) fn open_documents_folder(world: &mut World) {
    let dir = world.resource::<DocumentStore>().0.root().to_path_buf();
    let _ = cadrs_fs::create_dir_all(&dir);
    open_folder(&dir);
}

/// Opens folder `dir` in the system's file manager.
fn open_folder(dir: &std::path::Path) {
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(opener).arg(dir).spawn() {
        warn!("cannot open {}: {e}", dir.display());
    }
}

/// The documents page's part of the app menu: the documents folder, the Trash and the labels
/// (each shows its documents), with the one shown checked; while the Trash is shown, Empty
/// trash….
pub(crate) fn app_entries(lib: &Library, filter: Filter) -> Vec<MenuEntry> {
    let mut labels: Vec<MenuEntry> = lib
        .labels_by_name()
        .iter()
        .map(|l| {
            MenuItem::new(
                format!("app-label-{}", details::slug(&l.name)),
                l.name.clone(),
            )
            .dot(Color::srgb_u8(l.colour[0], l.colour[1], l.colour[2]))
            .checked(filter == Filter::Label(l.id))
            .into()
        })
        .collect();
    if !labels.is_empty() {
        labels.push(MenuEntry::Separator);
    }
    labels.push(
        MenuItem::new("app-new-label", "New label…")
            .icon("tag-new")
            .into(),
    );
    let mut entries = vec![
        MenuItem::new("app-open-folder", "Open documents folder")
            .icon("folder")
            .into(),
        MenuItem::new("app-labels", "Labels")
            .icon("tag")
            .submenu(labels)
            .into(),
        MenuItem::new("app-trash", "Trash")
            .icon("delete")
            .checked(filter == Filter::Trash)
            .into(),
    ];
    if filter == Filter::Trash {
        let (docs, folders) = lib.trash();
        entries.push(
            MenuItem::new("app-empty-trash", "Empty trash…")
                .icon("delete")
                .disabled(docs.is_empty() && folders.is_empty())
                .into(),
        );
    }
    entries.push(MenuEntry::Separator);
    entries
}

/// Runs an app menu item of [`app_entries`]; false for one of the others.
pub(crate) fn app_action(world: &mut World, item: &str) -> bool {
    match item {
        "app-open-folder" => open_documents_folder(world),
        "app-trash" => show_filter(world, Filter::Trash),
        "app-empty-trash" => {
            let theme = world.resource::<Theme>().clone();
            let lib = world.resource::<DocLibrary>().lib.clone();
            let mut commands = world.commands();
            organize::open_empty_trash_dialog(&mut commands, &theme, &lib);
            world.flush();
        }
        "app-new-label" => {
            let theme = world.resource::<Theme>().clone();
            let mut commands = world.commands();
            details::open_new_label_dialog(&mut commands, &theme, None);
            world.flush();
        }
        m if m.starts_with("app-label-") => {
            let slug = m.trim_start_matches("app-label-");
            let id = world
                .resource::<DocLibrary>()
                .lib
                .labels
                .iter()
                .find(|l| details::slug(&l.name) == slug)
                .map(|l| l.id);
            if let Some(id) = id {
                show_filter(world, Filter::Label(id));
            }
        }
        _ => return false,
    }
    true
}

/// Opens a stored document: records the open time and switches to the document screen.
pub(crate) fn open_document(world: &mut World, id: DocumentId) {
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let mut file = match store.load(id) {
        Ok(f) => f,
        Err(e) => {
            open_failed(world, id, &e);
            return;
        }
    };
    file.meta.last_opened = Some(now);
    if let Err(e) = store.save(&file.document, &file.meta) {
        warn!("cannot record the open time: {e}");
    }
    let read_only = file
        .meta
        .shared
        .as_ref()
        .map(crate::collab::read_only_reason);
    log_opened(&file.document, &file.meta, &store.doc_dir(id));
    let mut active = ActiveDocument::stored(file.document, file.meta);
    active.read_only = read_only;
    world.insert_resource(active);
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Document);
}

/// The log's line for a document opened from the documents page: which one, where it is stored
/// and what is in it.
fn log_opened(doc: &Document, meta: &DocumentMeta, dir: &std::path::Path) {
    let tabs = doc
        .elements
        .iter()
        .map(|el| format!("{} ({})", el.name, crate::tab_manager::tab_type(el)))
        .collect::<Vec<_>>()
        .join(", ");
    let shared = meta
        .shared
        .as_ref()
        .map_or_else(String::new, |s| format!(", shared by {}", s.owner_name));
    info!(
        "opened document \"{}\" (id {}) from {}: {} tabs: {tabs}; owner {}, workspace {}{shared}",
        doc.name,
        doc.id,
        dir.display(),
        doc.elements.len(),
        meta.owned_by,
        meta.workspace.as_deref().unwrap_or("Main"),
    );
}

/// Tells the person why stored document `id` didn't open: one saved by a newer cadrs is a
/// warning to update cadrs (it isn't opened, as saving it here would lose what this cadrs
/// doesn't know), anything else an error.
pub(crate) fn open_failed(world: &mut World, id: DocumentId, e: &cadrs_core::StoreError) {
    error!("cannot open document {id}: {e}");
    let name = world
        .resource::<DocLibrary>()
        .lib
        .get(id)
        .map_or_else(|| "the document".to_string(), |d| format!("\"{}\"", d.name));
    let note = if e.is_newer() {
        cadrs_ui::Notification::warning(format!(
            "Can't open {name}: {}.",
            cadrs_core::store::newer_reason()
        ))
    } else {
        cadrs_ui::Notification::error(format!("Can't open {name}: {e}"))
    };
    let theme = world.resource::<Theme>().clone();
    cadrs_ui::show_notification(
        &mut world.commands(),
        &theme,
        note.name("open-document-toast"),
    );
    world.flush();
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
    details_state: Res<LandingDetails>,
    theme: Res<Theme>,
    lib: Res<DocLibrary>,
    mut commands: Commands,
) {
    let Ok(row) = q.get(ev.entity) else {
        return;
    };
    // #112: one of several selected: the selection's menu.
    let selection = details_state.selection.clone();
    if selection.len() > 1 && selection.contains(&Pick::Doc(row.0)) {
        let trash = state.filter == Filter::Trash;
        organize::open_multi_menu(
            &mut commands,
            &theme,
            &lib.lib,
            selection,
            trash,
            ev.position,
        );
        return;
    }
    // P3E.1: a right-click selects the row too (the Details panel follows if open).
    let id = row.0;
    let shared_info = lib.lib.get(id).and_then(|e| e.meta.shared.clone());
    let shared = shared_info.is_some();
    let may_copy = shared_info.is_none_or(|s| s.access().rights.copy);
    // Transfer ownership (`crate::collab`): one's own document shared with someone.
    let transferable = lib
        .lib
        .get(id)
        .and_then(|e| e.meta.sharing.as_ref())
        .is_some_and(|s| !s.members.is_empty());
    commands.queue(move |world: &mut World| details::select(world, Pick::Doc(id), false));
    let menu = if state.filter == Filter::Trash {
        Menu::new("doc-context-menu")
            .item(MenuItem::new("ctx-restore", "Restore").icon("restore"))
            .item(MenuItem::new("ctx-delete-forever", "Delete permanently…").icon("delete"))
            .separator()
            .item(MenuItem::new("ctx-open-folder", "Open document folder").icon("folder"))
            .item(MenuItem::new("ctx-properties", "Properties…").icon("info"))
            .min_width(190.0)
    } else {
        Menu::new("doc-context-menu")
            .item(MenuItem::new("ctx-open", "Open").icon("open-external"))
            .item(MenuItem::new("ctx-open-folder", "Open document folder").icon("folder"))
            .item(MenuItem::new("ctx-rename", "Rename…").icon("edit"))
            .separator()
            .item(
                MenuItem::new("ctx-copy", "Copy…")
                    .icon("copy")
                    .disabled(!may_copy),
            )
            .item({
                // P3G.1 (ER1.2, ER X8): a folder of the library (the Other documents browser's
                // locations, by path since #112), or back to the top level.
                let current = lib.lib.get(row.0).map(|e| e.meta.folder);
                MenuItem::new("ctx-move", "Move to…")
                    .icon("move-to-folder")
                    .submenu(organize::move_entries(&lib.lib, current, &[]))
            })
            .item({
                // P3E.1: the labels as checkable items, and "Create new label…".
                let current = lib
                    .lib
                    .get(row.0)
                    .map(|e| e.meta.labels.clone())
                    .unwrap_or_default();
                let mut entries: Vec<MenuEntry> = lib
                    .lib
                    .labels_by_name()
                    .iter()
                    // P3E.2 (P3E.1 judge): each label with its colour dot, a check mark on those given.
                    .map(|l| {
                        MenuItem::new(
                            format!("ctx-label-{}", details::slug(&l.name)),
                            l.name.clone(),
                        )
                        .dot(Color::srgb_u8(l.colour[0], l.colour[1], l.colour[2]))
                        .checked(current.contains(&l.id))
                        .into()
                    })
                    .collect();
                if !entries.is_empty() {
                    entries.push(MenuEntry::Separator);
                }
                entries.push(
                    MenuItem::new("ctx-new-label", "Create new label…")
                        .icon("tag-new")
                        .into(),
                );
                MenuItem::new("ctx-labels", "Labels")
                    .icon("tag")
                    .submenu(entries)
            })
            .separator()
            .item(MenuItem::new("ctx-properties", "Properties…").icon("info"))
            .item(MenuItem::new("ctx-history", "History…").icon("versions"))
            .item(MenuItem::new("ctx-used-in", "Used in…").icon("link"))
            .item(
                MenuItem::new("ctx-transfer", "Transfer ownership…")
                    .icon("share")
                    .disabled(!transferable),
            )
            .separator()
            .item(if shared {
                // Shared with me (`crate::collab`): the owner hears of it; the copy goes.
                MenuItem::new("ctx-unshare", "Unshare with me").icon("delete")
            } else {
                MenuItem::new("ctx-trash", "Move to trash").icon("delete")
            })
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
        // The document's own folder (`<documents>/<id>`: its document.ron, thumbnails, blobs).
        "ctx-open-folder" => open_folder(&store.0.doc_dir(id)),
        "ctx-rename" => open_rename_dialog(&mut commands, &theme, &entry),
        "ctx-copy" => open_copy_dialog(&mut commands, &theme, &entry),
        "ctx-trash" => open_trash_dialog(&mut commands, &theme, &entry),
        "ctx-transfer" => commands
            .queue(move |world: &mut World| crate::collab::share_dialog::open_transfer(world, id)),
        "ctx-unshare" => {
            let store = store.0.clone();
            let name = entry.name.clone();
            commands.queue(move |world: &mut World| {
                crate::collab::sharing::leave(world, id);
                let now = world.resource::<AppClock>().now();
                world
                    .resource_mut::<DocLibrary>()
                    .execute(&store, &TrashEntry { id, now });
                undo_toast(world, format!("Unshared \u{201c}{name}\u{201d} with you"));
            });
        }
        "ctx-delete-forever" => open_purge_dialog(&mut commands, &theme, &entry),
        "ctx-properties" | "ctx-history" | "ctx-used-in" => {
            let tab = match ev.item.as_str() {
                "ctx-history" => DetailsTab::Versions,
                "ctx-used-in" => DetailsTab::WhereUsed,
                _ => DetailsTab::Info,
            };
            commands
                .queue(move |world: &mut World| details::open_details(world, Pick::Doc(id), tab))
        }
        "ctx-new-label" => details::open_new_label_dialog(&mut commands, &theme, Some(id)),
        m if m.starts_with("ctx-label-") => {
            let slug = m.trim_start_matches("ctx-label-").to_string();
            let store = store.0.clone();
            commands.queue(move |world: &mut World| {
                details::toggle_label_by_slug(world, &store, id, &slug)
            });
        }
        // P3G.1: Move to ▸ a folder (undoable, like the other library commands).
        m if m == "ctx-move-top" || m.starts_with("ctx-move-folder-") => {
            let Some(to) = organize::move_target(&lib.lib, m) else {
                return;
            };
            commands
                .queue(move |world: &mut World| organize::move_items(world, &[Pick::Doc(id)], to));
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
    commands
        .entity(undo)
        .insert(observe(|_: On<Activate>, mut commands: Commands| {
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
            // In the folder open, if one is.
            let parent = world.resource::<LandingState>().folder;
            world.resource_mut::<DocLibrary>().execute(
                &store,
                &CreateFolder {
                    id,
                    name,
                    user,
                    now,
                    parent,
                },
            );
            // Show the new folder: it is listed under Owned by me and Created by me.
            let mut state = world.resource_mut::<LandingState>();
            if !matches!(state.filter, Filter::OwnedByMe | Filter::CreatedByMe) {
                state.filter = Filter::OwnedByMe;
            }
        }
        LandingDialog::Purge(id) => {
            close_dialogs(world);
            let name = doc_name(world, id);
            world
                .resource_mut::<DocLibrary>()
                .execute(&store, &PurgeEntry { id });
            undo_toast(world, format!("Deleted \u{201c}{name}\u{201d} permanently"));
        }
        LandingDialog::Join => {
            let link = field_value(world, "join-link-field");
            if link.trim().is_empty() {
                return;
            }
            close_dialogs(world);
            crate::collab::start_join(world, &link);
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
        LandingDialog::RenameFolder(_)
        | LandingDialog::CopyFolder(_)
        | LandingDialog::PurgeFolder(_)
        | LandingDialog::PurgeItems
        | LandingDialog::EmptyTrash => organize::confirm(world, dialog),
    }
}

fn dialog_buttons(p: &mut ChildSpawner, t: &Theme, prefix: &str, ok: &str) {
    p.spawn((
        Button::new(format!("{prefix}-ok"))
            .label(ok)
            .primary()
            .build(t),
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
            | "join-link-field"
            | "rename-name-field"
            | "new-folder-name-field"
            | "copy-name-field"
            | "new-label-name-field"
            | "rename-label-name-field"
            | "sample-copy-name-field"
            | "rename-folder-name-field"
            | "copy-folder-name-field"
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
                    Button::new("new-document-cancel").label("Cancel").build(t),
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

/// Join shared document: a link someone shared, pasted.
fn open_join_dialog(commands: &mut Commands, theme: &Theme) {
    let (tb, tf) = (theme.clone(), theme.clone());
    commands.spawn((
        Dialog::new("join-dialog")
            .title("Join shared document")
            .width(548.0)
            .body(move |b| {
                let t = &tb;
                field_label(b, t, "Link");
                b.spawn(
                    TextInput::new("join-link")
                        .placeholder("Paste the link someone shared with you")
                        .autofocus()
                        .build(t),
                );
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("join-confirm").label("Join").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(confirm_dialog);
                    }),
                ));
                f.spawn((
                    Button::new("join-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close_dialogs);
                    }),
                ));
            })
            .build(theme),
        LandingDialog::Join,
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
