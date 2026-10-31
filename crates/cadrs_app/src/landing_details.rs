//! P3E.1: the documents page's selection model, Details panel, labels, folders, grid view,
//! samples and Import files… (`reference/onshape/training/test-drive-gaps.md` P3E.1;
//! `test-drive/lesson-documents-page.png`).
//!
//! - **Selection.** A click on a document, folder or sample ([`super::PickRow`]) selects it
//!   ([`select`]) and opens the Details panel; a double click, or Enter, opens it
//!   ([`open_pick`]): a document opens, a folder shows its documents under a breadcrumb, a
//!   sample offers "Open a copy".
//! - **Details panel** (a glass card right of the list; `details-panel`), opened by three tabs
//!   in the list card's header (`details-toggle` Info, `details-versions` Versions,
//!   `details-where-used` Where used): the thumbnail, Owner,
//!   an editable Document description (`details-description`, saved on Enter or when it loses
//!   focus), Document labels (a search field `details-label-search`, a checkbox per label
//!   `details-label-<slug>` and `details-create-label`), Created by, Created, Modified and the
//!   location; the versions from the document's history (P3D.3); where it is used (P3G.2).
//! - **Labels.** Create ▸ Label…, the tag icon after the filter tabs (`labels-new`) and "Create
//!   new label" open `new-label-dialog`; each label is a tab (`label-item-<slug>`) that filters
//!   the list, with Rename… and Delete… on right-click; the row menu's Labels ▸ toggles them.
//! - **Samples.** Explore cadrs lists [`cadrs_core::documents_page::SAMPLES`] (`sample-row-<i>`);
//!   Open a copy (`sample-copy-dialog`) adds an editable copy to the library and opens it.
//! - **Import files…** picks a STEP or STL file (`import-files-picker`); it becomes a new
//!   document named after the file whose Part Studio holds its Import (one library step).

use std::collections::HashMap;
use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::ScrollArea;
use cadrs_core::documents_page::{SAMPLE_OWNER, SAMPLES};
use cadrs_core::library::{
    AddEntry, CreateLabel, DeleteLabel, Library, RenameLabel, SetDescription, SetLabels,
};
use cadrs_core::{DocumentEntry, DocumentId, Filter, FolderId, LabelId};
use cadrs_ui::i18n::{Translated, tr, tr_with};
use cadrs_ui::menu::ContextMenuAnchor;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    Button, CheckboxChange, FilePicked, LabelChip, details_caption, details_divider, details_value,
    show_toast, side_tab,
};

use super::{
    DocLibrary, DocRow, LandingDialog, LandingState, PickRow, Thumbnails, close_dialogs,
    dialog_buttons, field_label, field_value, large_thumbnail, open_document, thumbnail_node,
    undo_toast,
};
use crate::{AppClock, AppState, DocumentStore, UserProfile};

/// What is selected on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Doc(DocumentId),
    Folder(FolderId),
    /// `SAMPLES[i]`.
    Sample(usize),
}

/// The Details panel's tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailsTab {
    #[default]
    Info,
    Versions,
    WhereUsed,
}

/// The Details panel: open or not, its tab and the selected item. Kept while a document is
/// open, like [`LandingState`].
#[derive(Resource, Debug, Clone)]
pub struct LandingDetails {
    pub open: bool,
    pub tab: DetailsTab,
    /// The item the panel shows: the one last clicked.
    pub pick: Option<Pick>,
    /// Everything selected (#112: Ctrl+click adds or removes one, Shift+click a range), in the
    /// order picked.
    pub selection: Vec<Pick>,
    /// Where a Shift+click range starts.
    pub anchor: Option<Pick>,
    /// The Document labels section is expanded.
    pub labels_open: bool,
}

impl Default for LandingDetails {
    fn default() -> Self {
        Self {
            open: false,
            tab: DetailsTab::Info,
            pick: None,
            selection: Vec::new(),
            anchor: None,
            labels_open: true,
        }
    }
}

/// The samples' thumbnails (drawn once from their Part Studio), and the image for a copy's.
#[derive(Resource, Default)]
pub struct SampleThumbs(HashMap<usize, (Option<Handle<Image>>, Option<image::RgbaImage>)>);

/// The panel (rebuilt by [`rebuild_details`]).
#[derive(Component)]
pub struct DetailsPanel;

/// A label's checkbox in the panel (hidden when the search doesn't match its name).
#[derive(Component, Clone)]
pub struct LabelCheck {
    id: LabelId,
    name: String,
}

/// A sample row.
#[derive(Component, Clone, Copy)]
pub struct SampleRow(usize);

/// Which sample a context menu is for.
#[derive(Component, Clone, Copy)]
pub struct ContextSample(usize);

/// A name slug ("Medical Devices" → "medical-devices").
pub fn slug(s: &str) -> String {
    crate::linked::slug(s)
}

fn image_handle(images: &mut Assets<Image>, img: &image::RgbaImage) -> Handle<Image> {
    let (w, h) = img.dimensions();
    images.add(Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        img.clone().into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

// ---------------------------------------------------------------------------------------------
// Selection

/// Selects `pick` alone (and opens the panel when `open`), marking its row or card.
pub fn select(world: &mut World, pick: Pick, open: bool) {
    set_selection(world, vec![pick], Some(pick), open);
}

/// Selects `picks` (`primary` shown in the panel, which opens when `open`), marking their rows
/// and cards.
pub fn set_selection(world: &mut World, picks: Vec<Pick>, primary: Option<Pick>, open: bool) {
    {
        let mut d = world.resource_mut::<LandingDetails>();
        let want_open = d.open || open;
        if d.pick != primary || d.open != want_open || d.selection != picks {
            d.pick = primary;
            d.open = want_open;
            d.selection = picks.clone();
        }
    }
    let mut q = world.query::<(Entity, &PickRow, Has<Selected>)>();
    let rows: Vec<(Entity, Pick, bool)> = q.iter(world).map(|(e, p, s)| (e, p.0, s)).collect();
    for (e, p, selected) in rows {
        let want = picks.contains(&p);
        if want && !selected {
            world.entity_mut(e).insert(Selected);
        } else if !want && selected {
            world.entity_mut(e).remove::<Selected>();
        }
    }
}

/// The main list's items in the order shown (a Shift+click range runs over them).
#[derive(Resource, Debug, Clone, Default)]
pub struct ListOrder(pub Vec<Pick>);

/// A click on `pick`: alone; with Ctrl, added to or taken from the selection; with Shift,
/// everything listed from the last plain or Ctrl click to it (#112, as a file manager does).
pub fn click_select(world: &mut World, pick: Pick, ctrl: bool, shift: bool) {
    let (mut picks, anchor) = {
        let d = world.resource::<LandingDetails>();
        (d.selection.clone(), d.anchor)
    };
    let order = world.resource::<ListOrder>().0.clone();
    let range = anchor.and_then(|a| {
        let i = order.iter().position(|p| *p == a)?;
        let j = order.iter().position(|p| *p == pick)?;
        Some(order[i.min(j)..=i.max(j)].to_vec())
    });
    match (shift, range) {
        (true, Some(r)) => {
            set_selection(world, r, Some(pick), false);
            return;
        }
        _ if ctrl => {
            if let Some(i) = picks.iter().position(|p| *p == pick) {
                picks.remove(i);
            } else {
                picks.push(pick);
            }
            let primary = if picks.contains(&pick) {
                Some(pick)
            } else {
                picks.last().copied()
            };
            set_selection(world, picks, primary, false);
        }
        _ => select(world, pick, false),
    }
    world.resource_mut::<LandingDetails>().anchor = Some(pick);
}

/// Opens `pick`: a document (not from the trash), a folder, or a sample's Open a copy.
pub fn open_pick(world: &mut World, pick: Pick) {
    match pick {
        Pick::Doc(id) => {
            let trashed = world
                .resource::<DocLibrary>()
                .lib
                .get(id)
                .is_none_or(|e| e.meta.trashed.is_some());
            if trashed {
                // Trashed documents must be restored first (right-click › Restore).
                return;
            }
            open_document(world, id);
        }
        Pick::Folder(f) => {
            let mut s = world.resource_mut::<LandingState>();
            s.folder = Some(f);
            if !matches!(s.filter, Filter::OwnedByMe | Filter::CreatedByMe) {
                s.filter = Filter::OwnedByMe;
            }
        }
        Pick::Sample(i) => {
            let theme = world.resource::<Theme>().clone();
            let mut commands = world.commands();
            open_sample_copy_dialog(&mut commands, &theme, i);
            world.flush();
        }
    }
}

pub fn on_pick_activate(
    ev: On<Activate>,
    q: Query<&PickRow>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    let Ok(p) = q.get(ev.entity) else { return };
    let pick = p.0;
    let ctrl = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    commands.queue(move |world: &mut World| {
        // A click ending a drag selects nothing.
        if world.resource::<super::organize::LandingDrag>().moved {
            return;
        }
        click_select(world, pick, ctrl, shift);
    });
}

pub fn on_pick_double_click(ev: On<DoubleClick>, q: Query<&PickRow>, mut commands: Commands) {
    let Ok(p) = q.get(ev.entity) else { return };
    let pick = p.0;
    commands.queue(move |world: &mut World| open_pick(world, pick));
}

// ---------------------------------------------------------------------------------------------
// Main panel pieces

/// Inside a folder: Back, the filter as a link, then each folder down to the open one, ›
/// between (#112: folders nest). The filter and the folders above are links, and each takes
/// documents and folders dropped on it.
pub fn breadcrumb(
    h: &mut ChildSpawnerCommands,
    t: &Theme,
    filter: Filter,
    title: &str,
    path: &[(FolderId, String)],
) {
    let parent = path.len().checked_sub(2).map(|k| path[k].0);
    h.spawn((
        IconButton::new("folder-back", "chevron-left")
            .icon_size(16.0)
            .tooltip(tr("shell-details-back"))
            .build(t),
        observe(move |_: On<Activate>, mut state: ResMut<LandingState>| state.folder = parent),
    ));
    h.spawn(icon(super::filter_icon(filter), 20.0, t.foreground));
    h.spawn((
        Button::new("crumb-root")
            .label(title.to_string())
            .link()
            .build(t),
        super::organize::DropFolder(None),
        observe(|_: On<Activate>, mut state: ResMut<LandingState>| state.folder = None),
    ))
    .entry::<Node>()
    .and_modify(|mut n| n.padding = UiRect::horizontal(Val::Px(2.0)));
    for (k, (id, name)) in path.iter().enumerate() {
        h.spawn(icon("chevron-right", 14.0, t.muted_foreground));
        if k + 1 < path.len() {
            let id = *id;
            h.spawn((
                Button::new(format!("crumb-folder-{k}"))
                    .label(name.clone())
                    .link()
                    .build(t),
                super::organize::DropFolder(Some(id)),
                observe(move |_: On<Activate>, mut state: ResMut<LandingState>| {
                    state.folder = Some(id)
                }),
            ))
            .entry::<Node>()
            .and_modify(|mut n| n.padding = UiRect::horizontal(Val::Px(2.0)));
            continue;
        }
        h.spawn(icon("folder", 20.0, t.foreground));
        h.spawn((
            Name::new("heading-title"),
            t.text(name.clone(), t.font_md, FontWeight::BOLD, t.foreground),
        ));
    }
}

/// The list's tools, right of the search box: in the Trash Empty trash…, Import files… and
/// Label…, and the list and grid toggles (framed like the color scheme's).
pub fn list_toolbar(a: &mut ChildSpawnerCommands, t: &Theme, state: &LandingState, lib: &Library) {
    a.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(8.0),
        ..default()
    })
    .with_children(|b| {
        if state.filter == Filter::Trash {
            let (docs, folders) = lib.trash();
            b.spawn((
                Button::new("empty-trash-button")
                    .label(tr("shell-details-empty-trash"))
                    .icon("delete")
                    .icon_size(16.0)
                    .ghost()
                    .tooltip(tr("shell-details-empty-trash-tooltip"))
                    .disabled(docs.is_empty() && folders.is_empty())
                    .build(t),
                observe(
                    |_: On<Activate>,
                     mut commands: Commands,
                     theme: Res<Theme>,
                     lib: Res<DocLibrary>| {
                        super::organize::open_empty_trash_dialog(&mut commands, &theme, &lib.lib);
                    },
                ),
            ))
            .insert(cadrs_ui::glass::control(t))
            .entry::<Node>()
            .and_modify(|mut n| super::page_control(&mut n));
        }
        for (name, icon_name, label, tip) in [
            (
                "import-button",
                "file-import",
                "common-import",
                "shell-details-import-tooltip",
            ),
            (
                "label-button",
                "tag-new",
                "shell-details-new-label",
                "shell-details-create-label-tooltip",
            ),
        ] {
            b.spawn((
                Button::new(name)
                    .label(tr(label))
                    .icon(icon_name)
                    .icon_size(16.0)
                    .ghost()
                    .tooltip(tr(tip))
                    .build(t),
                observe(
                    move |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                        if name == "import-button" {
                            commands.queue(|w: &mut World| {
                                crate::import_file::start(
                                    w,
                                    crate::import_file::ImportTarget::NewDocument,
                                )
                            });
                        } else {
                            open_new_label_dialog(&mut commands, &theme, None);
                        }
                    },
                ),
            ))
            .insert(cadrs_ui::glass::control(t))
            .entry::<Node>()
            .and_modify(|mut n| super::page_control(&mut n));
        }
        b.spawn((
            Name::new("view-modes"),
            cadrs_ui::segmented::frame(t),
            cadrs_ui::glass::GlassFrame::page(),
        ))
        .with_children(|f| {
            for (name, icon_name, tip, grid) in [
                ("view-list", "list", "shell-doc-list-view", false),
                ("view-grid", "apps", "shell-details-grid-view", true),
            ] {
                f.spawn((
                    side_tab(t, name, icon_name, &tr(tip), state.grid == grid),
                    observe(move |_: On<Activate>, mut state: ResMut<LandingState>| {
                        if state.grid != grid {
                            state.grid = grid;
                        }
                    }),
                ))
                .entry::<Node>()
                .and_modify(|mut n| n.width = Val::Px(36.0));
            }
        });
    });
}

/// The grid view: folder cards, then document cards.
pub fn grid(
    m: &mut ChildSpawnerCommands,
    t: &Theme,
    folders: &[(FolderId, String)],
    rows: Vec<(DocumentEntry, Option<Handle<Image>>, String)>,
    selection: &[Pick],
    labels: &[cadrs_core::LabelEntry],
) {
    m.spawn((
        Name::new("doc-grid"),
        ScrollArea,
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_wrap: FlexWrap::Wrap,
            align_content: AlignContent::FlexStart,
            column_gap: Val::Px(12.0),
            row_gap: Val::Px(12.0),
            overflow: Overflow::scroll_y(),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
    ))
    .with_children(|g| {
        let size = Vec2::new(176.0, 150.0);
        for (i, (id, name)) in folders.iter().enumerate() {
            g.spawn((
                GridItem::new(format!("grid-folder-{i}"), name.clone())
                    .icon("folder")
                    .size(size)
                    .selected(selection.contains(&Pick::Folder(*id)))
                    .build(t),
                PickRow(Pick::Folder(*id)),
                super::organize::FolderRow(*id),
                super::organize::DropFolder(Some(*id)),
                ContextMenuTarget,
                cadrs_ui::DoubleClickable,
            ));
        }
        for (i, (e, thumb, _)) in rows.into_iter().enumerate() {
            let mut item = GridItem::new(format!("grid-doc-{i}"), e.name.clone())
                .size(size)
                .selected(selection.contains(&Pick::Doc(e.id)));
            if let Some(h) = thumb {
                item = item.thumbnail(h);
            }
            let mut card = g.spawn((
                item.build(t),
                PickRow(Pick::Doc(e.id)),
                DocRow(e.id),
                ContextMenuTarget,
                cadrs_ui::DoubleClickable,
            ));
            // P3E.2 (P3E.1 judge): the document's labels as chips over the card's top left.
            let given: Vec<&cadrs_core::LabelEntry> = labels
                .iter()
                .filter(|l| e.meta.labels.contains(&l.id))
                .collect();
            if !given.is_empty() {
                card.with_children(|c| {
                    c.spawn((
                        Name::new(format!("grid-doc-{i}-labels")),
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(6.0),
                            left: Val::Px(6.0),
                            right: Val::Px(6.0),
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Px(3.0),
                            row_gap: Val::Px(3.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|w| {
                        for l in given {
                            w.spawn(
                                LabelChip::new(
                                    format!("grid-doc-{i}-chip-{}", slug(&l.name)),
                                    l.name.clone(),
                                    l.colour,
                                )
                                .build(t),
                            );
                        }
                    });
                });
            }
        }
    });
}

/// A sample in the Explore list.
#[derive(Clone)]
pub struct SampleListing {
    index: usize,
    thumb: Option<Handle<Image>>,
}

/// The samples, drawing their thumbnails the first time.
pub fn sample_rows(samples: &mut SampleThumbs, images: &mut Assets<Image>) -> Vec<SampleListing> {
    let dir = crate::script::fixtures_dir();
    (0..SAMPLES.len())
        .map(|i| {
            let (thumb, _) = samples.0.entry(i).or_insert_with(|| {
                let img = cadrs_core::Store::load_path(&SAMPLES[i].path(&dir))
                    .ok()
                    .and_then(|f| crate::script::studio_thumbnail(&f.document));
                (img.as_ref().map(|img| image_handle(images, img)), img)
            });
            SampleListing {
                index: i,
                thumb: thumb.clone(),
            }
        })
        .collect()
}

/// Explore cadrs: the bundled samples (TD4.1).
pub fn samples_table(
    m: &mut ChildSpawnerCommands,
    t: &Theme,
    list: Vec<SampleListing>,
    picked: Option<Pick>,
) {
    m.spawn((
        Name::new("samples-intro"),
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            padding: UiRect::new(Val::Px(8.0), Val::ZERO, Val::Px(2.0), Val::Px(12.0)),
            flex_shrink: 0.0,
            ..default()
        },
    ))
    .with_children(|i| {
        i.spawn((
            t.text(
                tr("shell-landing-samples"),
                t.font_md,
                FontWeight::SEMIBOLD,
                t.foreground,
            ),
            Translated("shell-landing-samples"),
        ));
        i.spawn((
            t.text(
                tr("shell-details-samples-intro"),
                t.font_base,
                FontWeight::NORMAL,
                t.muted_foreground,
            ),
            Translated("shell-details-samples-intro"),
        ));
    });
    let cols = vec![
        Column::new("name", tr("common-name")).width(420.0),
        Column::new("course", tr("shell-details-course")).width(260.0),
        Column::new("owned-by", tr("shell-landing-owned-by")),
    ];
    m.spawn(
        TableHeader::new("sample-col", cols.clone())
            .height(30.0)
            .build(t),
    );
    m.spawn((
        Name::new("sample-list"),
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
    .with_children(|l| {
        for s in list {
            let sample = SAMPLES[s.index];
            let name_font = t.font(t.font_md, FontWeight::MEDIUM);
            let fg = t.foreground;
            let placeholder = t.subtle_foreground;
            let thumb = s.thumb.clone();
            let i = s.index;
            let row = TableRow::new(format!("sample-row-{i}"), &cols)
                .cell(move |c| {
                    thumbnail_node(c, thumb, placeholder);
                    c.spawn((
                        Name::new(format!("sample-name-{i}")),
                        Text::new(sample.title),
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
                })
                .text_cell(t, sample.course)
                .text_cell(t, SAMPLE_OWNER)
                .selected(picked == Some(Pick::Sample(i)));
            l.spawn((
                row.build(t),
                PickRow(Pick::Sample(i)),
                SampleRow(i),
                cadrs_ui::DoubleClickable,
            ))
            .entry::<Node>()
            .and_modify(|mut n| n.height = Val::Px(38.5));
        }
    });
}

pub fn on_sample_context_menu(
    ev: On<ContextMenuRequested>,
    q: Query<&SampleRow>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(row) = q.get(ev.entity) else { return };
    let i = row.0;
    commands.queue(move |world: &mut World| select(world, Pick::Sample(i), false));
    let menu = Menu::new("sample-context-menu")
        .item(
            MenuItem::new("sample-open-copy", tr("shell-details-open-copy-ellipsis")).icon("copy"),
        )
        .min_width(170.0);
    let anchor = open_context_menu(&mut commands, ev.position, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((ContextSample(i), DespawnOnExit(AppState::Landing)));
}

pub fn on_sample_context_action(
    ev: On<MenuAction>,
    q: Query<&ContextSample, With<ContextMenuAnchor>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(s) = q.get(ev.entity) else { return };
    if ev.item == "sample-open-copy" {
        open_sample_copy_dialog(&mut commands, &theme, s.0);
    }
}

// ---------------------------------------------------------------------------------------------
// The Details panel

/// The modal holding a document's (or folder's, or sample's) Properties, History or Used in.
#[derive(Component)]
pub struct DetailsDialog;

fn details_title(tab: DetailsTab) -> String {
    tr(match tab {
        DetailsTab::Info => "shell-details-properties",
        DetailsTab::Versions => "shell-top-bar-history",
        DetailsTab::WhereUsed => "shell-details-used-in",
    })
}

/// A field's caption in the details: the catalog key of its text.
fn caption(t: &Theme, key: &'static str) -> impl Bundle {
    (details_caption(t, tr(key)), Translated(key))
}

/// Selects `pick` and opens its Properties, History or Used in (right-click on a row) in a
/// modal, which [`rebuild_details`] fills.
pub fn open_details(world: &mut World, pick: Pick, tab: DetailsTab) {
    set_selection(world, vec![pick], Some(pick), false);
    {
        let mut d = world.resource_mut::<LandingDetails>();
        d.tab = tab;
        d.open = true;
    }
    let theme = world.resource::<Theme>().clone();
    world.spawn((
        Dialog::new("details-dialog")
            .title(details_title(tab))
            .width(440.0)
            .body(|b| {
                b.spawn((
                    Name::new("details-panel"),
                    DetailsPanel,
                    // As tall as its content, scrolling past most of the window.
                    Node {
                        max_height: Val::Vh(70.0),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                ));
            })
            .build(&theme),
        DetailsDialog,
        DespawnOnExit(AppState::Landing),
    ));
}

/// Closing the modal (its ✕ or Escape) closes the details.
pub fn on_details_dialog_close(
    ev: On<DialogClose>,
    q: Query<(), With<DetailsDialog>>,
    mut d: ResMut<LandingDetails>,
) {
    if q.contains(ev.entity) && d.open {
        d.open = false;
    }
}

/// A label's Rename… and Delete… (the menu by its title).
pub fn label_action(world: &mut World, id: LabelId, item: &str) {
    let Some(label) = world.resource::<DocLibrary>().lib.label(id).cloned() else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let n = world
        .resource::<DocLibrary>()
        .lib
        .entries
        .iter()
        .filter(|e| e.meta.labels.contains(&label.id))
        .count();
    let mut commands = world.commands();
    match item {
        "label-rename" => open_rename_label_dialog(&mut commands, &theme, &label),
        "label-delete" => open_delete_label_dialog(&mut commands, &theme, &label, n),
        _ => {}
    }
    world.flush();
}

/// The panel's buttons, by name.
pub fn on_details_button(
    ev: On<Activate>,
    q: Query<&Name>,
    mut commands: Commands,
    theme: Res<Theme>,
    mut d: ResMut<LandingDetails>,
) {
    let Ok(name) = q.get(ev.entity) else { return };
    match name.as_str() {
        "details-labels-toggle" => d.labels_open = !d.labels_open,
        "details-create-label" => {
            if let Some(Pick::Doc(id)) = d.pick {
                open_new_label_dialog(&mut commands, &theme, Some(id));
            }
        }
        "details-open-copy" => {
            if let Some(Pick::Sample(i)) = d.pick {
                open_sample_copy_dialog(&mut commands, &theme, i);
            }
        }
        "details-open" => {
            if let Some(p) = d.pick {
                commands.queue(move |world: &mut World| open_pick(world, p));
            }
        }
        _ => {}
    }
}

/// Rebuilds the panel when the selection, its tab or the library changes.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_details(
    mut commands: Commands,
    theme: Res<Theme>,
    d: Res<LandingDetails>,
    lib: Res<DocLibrary>,
    store: Res<DocumentStore>,
    clock: Res<AppClock>,
    user: Res<UserProfile>,
    mut thumbs: ResMut<Thumbnails>,
    samples: Res<SampleThumbs>,
    mut images: ResMut<Assets<Image>>,
    q_panel: Query<(Entity, Ref<DetailsPanel>)>,
    q_dialog: Query<Entity, With<DetailsDialog>>,
) {
    // Closed some other way (the selection left the list): the modal goes.
    if !d.open {
        for e in &q_dialog {
            commands.entity(e).try_despawn();
        }
    }
    let Ok((panel, marker)) = q_panel.single() else {
        return;
    };
    if !(marker.is_added() || d.is_changed() || lib.is_changed()) {
        return;
    }
    commands.entity(panel).despawn_related::<Children>();
    if !d.open {
        return;
    }
    let t = theme.clone();
    // What to show, read before spawning.
    enum Content {
        None(String),
        Doc(Box<DocumentEntry>, Option<Handle<Image>>),
        Versions(String, Vec<cadrs_core::documents_page::VersionRow>),
        Used(String, Vec<cadrs_core::link_update::Usage>),
        Folder(String, String, usize),
        Sample(usize, Option<Handle<Image>>),
    }
    let content = match d.pick {
        None => Content::None(tr("shell-details-select")),
        Some(Pick::Doc(id)) => match lib.lib.get(id).cloned() {
            None => Content::None(tr("shell-details-select")),
            Some(e) => match d.tab {
                DetailsTab::Info => {
                    let th = large_thumbnail(&mut thumbs, &mut images, &store.0, id);
                    Content::Doc(Box::new(e), th)
                }
                DetailsTab::Versions => Content::Versions(
                    e.name.clone(),
                    cadrs_core::documents_page::versions(&store.0, id).unwrap_or_default(),
                ),
                DetailsTab::WhereUsed => Content::Used(
                    e.name.clone(),
                    cadrs_core::link_update::where_used(&store.0, id),
                ),
            },
        },
        Some(Pick::Folder(f)) => match lib.lib.folders.iter().find(|x| x.id == f) {
            None => Content::None(tr("shell-details-select")),
            Some(x) => match d.tab {
                DetailsTab::Info => Content::Folder(
                    x.name.clone(),
                    clock.format(x.created),
                    lib.lib.in_folder(f).len(),
                ),
                DetailsTab::Versions => Content::None(tr("shell-details-folder-no-versions")),
                DetailsTab::WhereUsed => Content::None(tr("shell-details-folder-not-used")),
            },
        },
        Some(Pick::Sample(i)) => match d.tab {
            DetailsTab::Info => Content::Sample(i, samples.0.get(&i).and_then(|(h, _)| h.clone())),
            DetailsTab::Versions => Content::None(tr("shell-details-sample-no-versions")),
            DetailsTab::WhereUsed => Content::None(tr("shell-details-sample-not-used")),
        },
    };
    let labels = lib.lib.labels_by_name();
    let label_search = String::new();
    let labels_open = d.labels_open;
    let owner_name = |who: &str| {
        if who == user.id {
            user.display_name.clone()
        } else {
            who.to_string()
        }
    };
    let folders = lib.lib.folders.clone();
    let clock = *clock;
    let user = user.clone();
    commands.entity(panel).with_children(|p| {
        p.spawn((
            Name::new("details-body"),
            ScrollArea,
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                padding: UiRect::new(Val::Px(12.0), Val::Px(12.0), Val::Px(2.0), Val::Px(12.0)),
                ..default()
            },
        ))
        .with_children(|b| match content {
            Content::None(text) => {
                b.spawn((
                    Name::new("details-empty"),
                    t.text(text, t.font_base, FontWeight::NORMAL, t.muted_foreground),
                    Node {
                        margin: UiRect::top(Val::Px(24.0)),
                        align_self: AlignSelf::Center,
                        ..default()
                    },
                ));
            }
            Content::Doc(e, th) => {
                title_row(b, &t, "file", &e.name);
                big_thumbnail(b, &t, th, "part");
                b.spawn(details_divider(&t));
                b.spawn(caption(&t, "shell-details-owner"));
                b.spawn((
                    Name::new("details-owner"),
                    details_value(&t, owner_name(&e.meta.owned_by)),
                ));
                b.spawn(details_divider(&t));
                b.spawn(caption(&t, "shell-details-description"));
                // P3E.2 (P3E.1 judge): a document in the Trash is read-only here: its
                // description and labels are shown, not edited (Restore it to edit them).
                let trashed = e.meta.trashed.is_some();
                if trashed {
                    let text = if e.meta.description.is_empty() {
                        tr("shell-details-no-description")
                    } else {
                        e.meta.description.clone()
                    };
                    b.spawn((
                        Name::new("details-description-text"),
                        details_value(&t, text),
                    ));
                } else {
                    b.spawn(
                        TextInput::new("details-description")
                            .value(e.meta.description.clone())
                            .placeholder(tr("shell-details-add-description"))
                            .height(28.0)
                            .width(Val::Percent(100.0))
                            .build(&t),
                    )
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.margin = UiRect::new(Val::ZERO, Val::ZERO, Val::Px(4.0), Val::Px(8.0))
                    });
                }
                b.spawn(details_divider(&t));
                labels_section(b, &t, &e, &labels, labels_open && !trashed, &label_search);
                if trashed {
                    b.spawn((
                        Name::new("details-trashed-note"),
                        t.text(
                            tr("shell-details-trashed-note"),
                            t.font_sm,
                            FontWeight::NORMAL,
                            t.muted_foreground,
                        ),
                        Translated("shell-details-trashed-note"),
                        Node {
                            margin: UiRect::bottom(Val::Px(6.0)),
                            max_width: Val::Px(250.0),
                            ..default()
                        },
                    ))
                    .insert(TextLayout::new(
                        bevy::text::Justify::Left,
                        bevy::text::LineBreak::WordBoundary,
                    ));
                }
                b.spawn(details_divider(&t));
                b.spawn(caption(&t, "shell-details-created-by"));
                b.spawn((
                    Name::new("details-created-by"),
                    details_value(&t, owner_name(&e.meta.created_by)),
                ));
                b.spawn(caption(&t, "shell-details-created"));
                b.spawn(details_value(&t, clock.format(e.meta.created)));
                b.spawn(caption(&t, "shell-landing-modified"));
                b.spawn(details_value(
                    &t,
                    tr_with(
                        "shell-details-modified-by",
                        &[
                            ("time", &clock.format(e.meta.modified)),
                            ("name", &user.display(&e.meta.modified_by)),
                        ],
                    ),
                ));
                b.spawn(caption(&t, "shell-details-location"));
                let location = match (
                    e.meta.trashed,
                    e.meta
                        .folder
                        .and_then(|f| folders.iter().find(|x| x.id == f)),
                ) {
                    (Some(_), _) => tr("shell-landing-trash"),
                    (None, Some(f)) => format!("{} › {}", tr("shell-landing-owned-by-me"), f.name),
                    (None, None) => tr("shell-landing-owned-by-me"),
                };
                b.spawn((Name::new("details-location"), details_value(&t, location)));
            }
            Content::Versions(name, rows) => {
                title_row(b, &t, "file", &name);
                b.spawn(details_divider(&t));
                if rows.is_empty() {
                    b.spawn((
                        Name::new("details-empty"),
                        t.text(
                            tr("shell-details-no-versions"),
                            t.font_base,
                            FontWeight::NORMAL,
                            t.muted_foreground,
                        ),
                        Translated("shell-details-no-versions"),
                        Node {
                            margin: UiRect::top(Val::Px(12.0)),
                            ..default()
                        },
                    ));
                }
                for (i, r) in rows.into_iter().enumerate() {
                    b.spawn((
                        Name::new(format!("details-version-{i}")),
                        Node {
                            column_gap: Val::Px(8.0),
                            padding: UiRect::vertical(Val::Px(8.0)),
                            border: UiRect::bottom(Val::Px(1.0)),
                            ..default()
                        },
                        BorderColor::all(t.row_separator),
                    ))
                    .with_children(|row| {
                        row.spawn(icon("versions", 16.0, t.foreground));
                        row.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(2.0),
                            ..default()
                        })
                        .with_children(|c| {
                            c.spawn(t.text(r.name, t.font_base, FontWeight::BOLD, t.foreground));
                            c.spawn(t.text(
                                format!("{} · {}", clock.format(r.time), user.display(&r.user)),
                                t.font_sm,
                                FontWeight::NORMAL,
                                t.muted_foreground,
                            ));
                            if !r.description.is_empty() {
                                c.spawn(t.text(
                                    r.description,
                                    t.font_sm,
                                    FontWeight::NORMAL,
                                    t.foreground,
                                ));
                            }
                        });
                    });
                }
            }
            Content::Used(name, uses) => {
                title_row(b, &t, "file", &name);
                b.spawn(details_divider(&t));
                if uses.is_empty() {
                    b.spawn((
                        Name::new("details-empty"),
                        t.text(
                            tr("shell-details-not-used"),
                            t.font_base,
                            FontWeight::NORMAL,
                            t.muted_foreground,
                        ),
                        Translated("shell-details-not-used"),
                        Node {
                            margin: UiRect::top(Val::Px(12.0)),
                            ..default()
                        },
                    ));
                }
                for (i, u) in uses.into_iter().enumerate() {
                    b.spawn((
                        Name::new(format!("details-used-{i}")),
                        Node {
                            column_gap: Val::Px(8.0),
                            padding: UiRect::vertical(Val::Px(8.0)),
                            border: UiRect::bottom(Val::Px(1.0)),
                            ..default()
                        },
                        BorderColor::all(t.row_separator),
                    ))
                    .with_children(|row| {
                        row.spawn(icon("link", 16.0, t.foreground));
                        row.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(2.0),
                            ..default()
                        })
                        .with_children(|c| {
                            c.spawn(t.text(
                                u.document_name,
                                t.font_base,
                                FontWeight::BOLD,
                                t.foreground,
                            ));
                            c.spawn(t.text(
                                tr_with(
                                    "shell-details-tab-uses",
                                    &[
                                        ("tab", u.tab.as_str()),
                                        ("element", u.element.as_str()),
                                        ("version", u.version.as_str()),
                                    ],
                                ),
                                t.font_sm,
                                FontWeight::NORMAL,
                                t.foreground,
                            ));
                            let n = if u.count == 1 {
                                tr("shell-details-one-use")
                            } else {
                                tr_with("shell-details-uses", &[("count", &u.count.to_string())])
                            };
                            c.spawn(t.text(n, t.font_sm, FontWeight::NORMAL, t.muted_foreground));
                        });
                    });
                }
            }
            Content::Folder(name, created, count) => {
                title_row(b, &t, "folder", &name);
                big_thumbnail(b, &t, None, "folder");
                b.spawn(details_divider(&t));
                b.spawn(caption(&t, "shell-details-owner"));
                b.spawn(details_value(&t, user.display_name.clone()));
                b.spawn(caption(&t, "shell-details-created"));
                b.spawn(details_value(&t, created));
                b.spawn(caption(&t, "shell-details-contents"));
                b.spawn((
                    Name::new("details-folder-count"),
                    details_value(&t, document_count(count)),
                ));
                b.spawn((Button::new("details-open")
                    .label(tr("shell-details-open-folder"))
                    .icon("folder")
                    .outline()
                    .build(&t),))
                    .entry::<Node>()
                    .and_modify(|mut n| n.margin = UiRect::top(Val::Px(6.0)));
            }
            Content::Sample(i, th) => {
                let s = SAMPLES[i];
                title_row(b, &t, "file", s.title);
                big_thumbnail(b, &t, th, "part");
                b.spawn((Button::new("details-open-copy")
                    .label(tr("shell-details-open-copy"))
                    .icon("copy")
                    .primary()
                    .width(Val::Percent(100.0))
                    .build(&t),))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.margin = UiRect::vertical(Val::Px(8.0));
                        n.column_gap = Val::Px(6.0);
                    });
                b.spawn(details_divider(&t));
                b.spawn(caption(&t, "shell-details-owner"));
                b.spawn(details_value(&t, SAMPLE_OWNER));
                b.spawn(caption(&t, "shell-details-course"));
                b.spawn(details_value(&t, s.course));
                b.spawn(caption(&t, "shell-details-description"));
                b.spawn((
                    Text::new(s.description),
                    t.font(t.font_base, FontWeight::NORMAL),
                    TextColor(t.foreground),
                    Node {
                        margin: UiRect::bottom(Val::Px(8.0)),
                        max_width: Val::Px(270.0),
                        ..default()
                    },
                ));
            }
        });
    });
}

/// "1 document", "3 documents".
fn document_count(n: usize) -> String {
    if n == 1 {
        tr("shell-details-one-document")
    } else {
        tr_with("shell-details-documents", &[("count", &n.to_string())])
    }
}

fn title_row(b: &mut ChildSpawnerCommands, t: &Theme, icon_name: &'static str, name: &str) {
    b.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(6.0),
        padding: UiRect::vertical(Val::Px(6.0)),
        ..default()
    })
    .with_children(|r| {
        r.spawn(icon(icon_name, 18.0, t.foreground));
        r.spawn((
            Name::new("details-title"),
            t.text(name.to_string(), t.font_md, FontWeight::BOLD, t.foreground),
        ));
    });
}

fn big_thumbnail(
    b: &mut ChildSpawnerCommands,
    t: &Theme,
    th: Option<Handle<Image>>,
    placeholder: &'static str,
) {
    let node = Node {
        width: Val::Percent(100.0),
        height: Val::Px(150.0),
        flex_shrink: 0.0,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        margin: UiRect::bottom(Val::Px(8.0)),
        ..default()
    };
    match th {
        Some(img) => {
            b.spawn((node, Name::new("details-thumbnail"))).with_child((
                ImageNode::new(img),
                Node {
                    height: Val::Px(150.0),
                    max_width: Val::Percent(100.0),
                    ..default()
                },
            ));
        }
        None => {
            b.spawn((
                node,
                Name::new("details-thumbnail"),
                BackgroundColor(Color::srgb_u8(0xf4, 0xf4, 0xf4)),
            ))
            .with_child(icon(placeholder, 40.0, t.subtle_foreground));
        }
    }
}

/// Document labels: a disclosure over a box with the search field, the label checkboxes and
/// "Create new label" (`lesson-documents-page.png`).
fn labels_section(
    b: &mut ChildSpawnerCommands,
    t: &Theme,
    e: &DocumentEntry,
    labels: &[cadrs_core::LabelEntry],
    open: bool,
    search: &str,
) {
    b.spawn((
        Name::new("details-labels-toggle"),
        bevy::ui_widgets::Button,
        bevy::picking::hover::Hovered::default(),
        Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(4.0),
            padding: UiRect::vertical(Val::Px(8.0)),
            ..default()
        },
    ))
    .with_children(|h| {
        h.spawn((
            icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                14.0,
                t.foreground,
            ),
            Pickable::IGNORE,
        ));
        h.spawn((
            t.text(
                tr("shell-landing-document-labels"),
                t.font_base,
                FontWeight::BOLD,
                t.foreground,
            ),
            Translated("shell-landing-document-labels"),
            Pickable::IGNORE,
        ));
    });
    // The labels given, as chips, under the heading.
    let given: Vec<&cadrs_core::LabelEntry> = labels
        .iter()
        .filter(|l| e.meta.labels.contains(&l.id))
        .collect();
    if !given.is_empty() {
        b.spawn((
            Name::new("details-label-chips"),
            Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(4.0),
                row_gap: Val::Px(4.0),
                margin: UiRect::bottom(Val::Px(8.0)),
                ..default()
            },
        ))
        .with_children(|w| {
            for l in given {
                w.spawn(
                    LabelChip::new(
                        format!("details-chip-{}", slug(&l.name)),
                        l.name.clone(),
                        l.colour,
                    )
                    .build(t),
                );
            }
        });
    }
    if !open {
        return;
    }
    b.spawn((
        Name::new("details-labels-box"),
        Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            padding: UiRect::all(Val::Px(8.0)),
            margin: UiRect::bottom(Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(t.radius)),
            ..default()
        },
        BorderColor::all(t.border),
    ))
    .with_children(|x| {
        x.spawn(
            TextInput::new("details-label-search")
                .placeholder(tr("shell-landing-search-labels"))
                .value(search.to_string())
                .height(26.0)
                .width(Val::Percent(100.0))
                .build(t),
        )
        .entry::<Node>()
        .and_modify(|mut n| n.margin = UiRect::bottom(Val::Px(6.0)));
        if labels.is_empty() {
            x.spawn((
                Name::new("details-no-labels"),
                t.text(
                    tr("shell-details-no-labels"),
                    t.font_sm,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ),
                Translated("shell-details-no-labels"),
                Node {
                    margin: UiRect::vertical(Val::Px(4.0)),
                    ..default()
                },
            ));
        }
        for l in labels {
            x.spawn((
                Checkbox::new(format!("details-label-{}", slug(&l.name)))
                    .label(l.name.clone())
                    .checked(e.meta.labels.contains(&l.id))
                    .build(t),
                LabelCheck {
                    id: l.id,
                    name: l.name.clone(),
                },
            ))
            .entry::<Node>()
            .and_modify(|mut n| n.padding = UiRect::left(Val::Px(6.0)));
        }
        x.spawn(Node {
            height: Val::Px(1.0),
            margin: UiRect::vertical(Val::Px(4.0)),
            ..default()
        })
        .insert(BackgroundColor(t.separator));
        let mut dark = cadrs_ui::button::visuals_for(t, cadrs_ui::ButtonVariant::Ghost);
        dark.foreground = cadrs_ui::StateColors::all(t.foreground);
        x.spawn(
            Button::new("details-create-label")
                .label(tr("shell-details-new-label"))
                .icon("tag-new")
                .icon_size(16.0)
                .ghost()
                .build(t),
        )
        .insert(dark)
        .entry::<Node>()
        .and_modify(|mut n| {
            n.column_gap = Val::Px(6.0);
            n.justify_content = JustifyContent::FlexStart;
            n.padding = UiRect::horizontal(Val::Px(4.0));
        });
    });
}

/// The label search hides the checkboxes whose name doesn't contain the text.
pub fn filter_label_checks(
    q_field: Query<(&Name, &EditableText), Changed<EditableText>>,
    mut q: Query<(&LabelCheck, &mut Node)>,
) {
    for (name, text) in &q_field {
        if name.as_str() != "details-label-search-field" {
            continue;
        }
        let needle = text.value().to_string().trim().to_lowercase();
        for (c, mut node) in &mut q {
            let display = if needle.is_empty() || c.name.to_lowercase().contains(&needle) {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
    }
}

/// A label checkbox toggled: the document's labels change (one library step).
pub fn on_label_check(
    ev: On<CheckboxChange>,
    q: Query<&LabelCheck>,
    d: Res<LandingDetails>,
    lib: Res<DocLibrary>,
    store: Res<DocumentStore>,
    mut commands: Commands,
) {
    let Ok(c) = q.get(ev.entity) else { return };
    let Some(Pick::Doc(id)) = d.pick else { return };
    let Some(e) = lib.lib.get(id) else { return };
    let mut labels = e.meta.labels.clone();
    if ev.checked {
        if !labels.contains(&c.id) {
            labels.push(c.id);
        }
    } else {
        labels.retain(|l| *l != c.id);
    }
    let store = store.0.clone();
    commands.queue(move |world: &mut World| {
        world
            .resource_mut::<DocLibrary>()
            .execute(&store, &SetLabels { id, labels });
    });
}

/// The row menu's Labels ▸ item: toggles the label named by `slug`.
pub fn toggle_label_by_slug(world: &mut World, store: &cadrs_core::Store, id: DocumentId, s: &str) {
    let lib = &world.resource::<DocLibrary>().lib;
    let Some(label) = lib.labels.iter().find(|l| slug(&l.name) == s).map(|l| l.id) else {
        return;
    };
    let Some(e) = lib.get(id) else { return };
    let mut labels = e.meta.labels.clone();
    if labels.contains(&label) {
        labels.retain(|l| *l != label);
    } else {
        labels.push(label);
    }
    world
        .resource_mut::<DocLibrary>()
        .execute(store, &SetLabels { id, labels });
}

fn commit_description(world: &mut World, text: String) {
    let Some(Pick::Doc(id)) = world.resource::<LandingDetails>().pick else {
        return;
    };
    let Some(e) = world.resource::<DocLibrary>().lib.get(id) else {
        return;
    };
    if e.meta.description == text.trim() {
        return;
    }
    let store = world.resource::<DocumentStore>().0.clone();
    world.resource_mut::<DocLibrary>().execute(
        &store,
        &SetDescription {
            id,
            description: text,
        },
    );
}

/// Enter in the description field saves it.
pub fn on_description_submit(
    ev: On<TextSubmit>,
    q: Query<(&Name, &EditableText)>,
    mut commands: Commands,
) {
    if ev.entity != ev.original_event_target() {
        return;
    }
    let Ok((name, text)) = q.get(ev.entity) else {
        return;
    };
    if name.as_str() == "details-description-field" {
        let v = text.value().to_string();
        commands.queue(move |world: &mut World| commit_description(world, v));
    }
}

/// Leaving the description field saves it.
pub fn commit_description_on_blur(
    focus: Res<InputFocus>,
    mut last: Local<Option<Entity>>,
    q: Query<(&Name, &EditableText)>,
    mut commands: Commands,
) {
    let now = focus.get();
    if *last == now {
        return;
    }
    if let Some(prev) = *last
        && let Ok((name, text)) = q.get(prev)
        && name.as_str() == "details-description-field"
    {
        let v = text.value().to_string();
        commands.queue(move |world: &mut World| commit_description(world, v));
    }
    *last = now;
}

// ---------------------------------------------------------------------------------------------
// ---------------------------------------------------------------------------------------------
// Dialogs

pub fn open_new_label_dialog(commands: &mut Commands, theme: &Theme, assign: Option<DocumentId>) {
    let (t, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    commands.spawn((
        Dialog::new("new-label-dialog")
            .title(tr("shell-details-create-label"))
            .width(420.0)
            .body(move |b| {
                field_label(b, &tb, "shell-details-label-name");
                b.spawn(
                    TextInput::new("new-label-name")
                        .placeholder(tr("shell-details-label-name"))
                        .autofocus()
                        .build(&tb),
                );
                if assign.is_some() {
                    b.spawn((
                        tb.text(
                            tr("shell-details-label-given"),
                            tb.font_sm,
                            FontWeight::NORMAL,
                            tb.muted_foreground,
                        ),
                        Translated("shell-details-label-given"),
                    ));
                }
            })
            .footer(move |f| dialog_buttons(f, &tf, "new-label-dialog", "common-create"))
            .build(&t),
        LandingDialog::NewLabel(assign),
        DespawnOnExit(AppState::Landing),
    ));
}

fn open_rename_label_dialog(
    commands: &mut Commands,
    theme: &Theme,
    label: &cadrs_core::LabelEntry,
) {
    let (t, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    let name = label.name.clone();
    commands.spawn((
        Dialog::new("rename-label-dialog")
            .title(tr("shell-details-rename-label"))
            .width(420.0)
            .body(move |b| {
                field_label(b, &tb, "shell-details-label-name");
                b.spawn(
                    TextInput::new("rename-label-name")
                        .value(name)
                        .select_all_on_focus()
                        .autofocus()
                        .build(&tb),
                );
            })
            .footer(move |f| dialog_buttons(f, &tf, "rename-label-dialog", "common-rename"))
            .build(&t),
        LandingDialog::RenameLabel(label.id),
        DespawnOnExit(AppState::Landing),
    ));
}

fn open_delete_label_dialog(
    commands: &mut Commands,
    theme: &Theme,
    label: &cadrs_core::LabelEntry,
    used: usize,
) {
    let (t, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    let name = label.name.clone();
    commands.spawn((
        Dialog::new("delete-label-dialog")
            .title(tr("shell-details-delete-label"))
            .width(420.0)
            .body(move |b| {
                b.spawn((
                    Text::new(tr_with(
                        "shell-details-delete-label-question",
                        &[("name", name.as_str())],
                    )),
                    tb.font(tb.font_base, FontWeight::NORMAL),
                    TextColor(tb.foreground),
                ));
                b.spawn(tb.text(
                    tr_with(
                        "shell-details-label-taken-off",
                        &[("documents", &document_count(used))],
                    ),
                    tb.font_base,
                    FontWeight::NORMAL,
                    tb.muted_foreground,
                ));
            })
            .footer(move |f| {
                dialog_buttons(f, &tf, "delete-label-dialog", "shell-details-delete-label")
            })
            .build(&t),
        LandingDialog::DeleteLabel(label.id),
        DespawnOnExit(AppState::Landing),
    ));
}

pub fn open_sample_copy_dialog(commands: &mut Commands, theme: &Theme, i: usize) {
    let Some(s) = SAMPLES.get(i).copied() else {
        return;
    };
    let (t, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    commands.spawn((
        Dialog::new("sample-copy-dialog")
            .title(tr("shell-details-open-copy"))
            .width(460.0)
            .body(move |b| {
                b.spawn((
                    Text::new(tr_with(
                        "shell-details-sample-copy-text",
                        &[("name", s.title)],
                    )),
                    tb.font(tb.font_base, FontWeight::NORMAL),
                    TextColor(tb.foreground),
                    Node {
                        max_width: Val::Px(420.0),
                        ..default()
                    },
                ));
                field_label(b, &tb, "shell-landing-new-document-name");
                b.spawn(
                    TextInput::new("sample-copy-name")
                        .value(format!("Copy of {}", s.title))
                        .select_all_on_focus()
                        .autofocus()
                        .build(&tb),
                );
            })
            .footer(move |f| {
                dialog_buttons(f, &tf, "sample-copy-dialog", "shell-details-open-copy")
            })
            .build(&t),
        LandingDialog::SampleCopy(i),
        DespawnOnExit(AppState::Landing),
    ));
}

/// The P3E.1 dialogs' primary action.
pub fn confirm(world: &mut World, dialog: LandingDialog) {
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    match dialog {
        LandingDialog::NewLabel(assign) => {
            let name = field_value(world, "new-label-name-field")
                .trim()
                .to_string();
            if name.is_empty() {
                return;
            }
            let theme = world.resource::<Theme>().clone();
            let lib = &world.resource::<DocLibrary>().lib;
            if lib
                .labels
                .iter()
                .any(|l| l.name.to_lowercase() == name.to_lowercase())
            {
                let mut commands = world.commands();
                show_toast(
                    &mut commands,
                    &theme,
                    tr_with("shell-details-label-exists", &[("name", name.as_str())]),
                );
                world.flush();
                return;
            }
            let colour = lib.next_label_colour();
            close_dialogs(world);
            let cmd = CreateLabel {
                id: LabelId::new(),
                name: name.clone(),
                colour,
                assign: assign.into_iter().collect(),
            };
            world.resource_mut::<DocLibrary>().execute(&store, &cmd);
            undo_toast(
                world,
                tr_with("shell-details-label-created", &[("name", name.as_str())]),
            );
        }
        LandingDialog::RenameLabel(id) => {
            let name = field_value(world, "rename-label-name-field")
                .trim()
                .to_string();
            if name.is_empty() {
                return;
            }
            close_dialogs(world);
            world.resource_mut::<DocLibrary>().execute(
                &store,
                &RenameLabel {
                    id,
                    name: name.clone(),
                },
            );
            undo_toast(
                world,
                tr_with("shell-details-label-renamed", &[("name", name.as_str())]),
            );
        }
        LandingDialog::DeleteLabel(id) => {
            close_dialogs(world);
            let name = world
                .resource::<DocLibrary>()
                .lib
                .label(id)
                .map(|l| l.name.clone())
                .unwrap_or_default();
            world
                .resource_mut::<DocLibrary>()
                .execute(&store, &DeleteLabel { id });
            let mut state = world.resource_mut::<LandingState>();
            if state.filter == Filter::Label(id) {
                state.filter = Filter::OwnedByMe;
            }
            undo_toast(
                world,
                tr_with("shell-details-label-deleted", &[("name", name.as_str())]),
            );
        }
        LandingDialog::SampleCopy(i) => {
            let name = field_value(world, "sample-copy-name-field");
            if name.trim().is_empty() {
                return;
            }
            let Some(sample) = SAMPLES.get(i) else { return };
            close_dialogs(world);
            let id = DocumentId::new();
            let path = sample.path(&crate::script::fixtures_dir());
            match store.copy_from_file(&path, id, &name, &user, now) {
                Ok(mut entry) => {
                    // P3E.2 (P3E.1 judge): the copy keeps the sample's description.
                    entry.meta.description = sample.description.to_string();
                    if let Err(e) = store.update_entry(&entry) {
                        warn!("cannot save the copy's description: {e}");
                    }
                    if let Some(img) = world
                        .resource::<SampleThumbs>()
                        .0
                        .get(&i)
                        .and_then(|(_, img)| img.clone())
                    {
                        let _ = store.write_thumbnail(id, &img);
                    }
                    world
                        .resource_mut::<DocLibrary>()
                        .execute(&store, &AddEntry { entry });
                    open_document(world, id);
                }
                Err(e) => error!("cannot copy the sample {}: {e}", path.display()),
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Import files…

const IMPORT_TAG: &str = "landing-import";

pub fn on_import_picked(mut msgs: MessageReader<FilePicked>, mut commands: Commands) {
    for m in msgs.read() {
        if m.tag != IMPORT_TAG {
            continue;
        }
        let path = m.path.clone();
        commands.queue(move |world: &mut World| import_file(world, &path));
    }
}

/// A STEP or STL file as a new document: its Part Studio holds the file's Import (P3E.1,
/// TD3.1), added to the library as one undoable step.
pub fn import_file(world: &mut World, path: &Path) {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let stem = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("Imported")
        .to_string();
    let bytes = match cadrs_fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            warn!("cannot import {}: {e}", path.display());
            return;
        }
    };
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let mut doc = cadrs_core::Document::new(stem);
    let Some(studio) = doc
        .elements
        .iter()
        .find(|e| matches!(e.kind, cadrs_core::ElementKind::PartStudio { .. }))
        .map(|e| e.id)
    else {
        return;
    };
    let cmd = cadrs_core::import::AddImport {
        element: studio,
        feature: cadrs_core::FeatureId::new(),
        file_name: file_name.clone(),
        bytes: std::sync::Arc::new(bytes),
        y_axis_up: false,
        units: None,
    };
    if let Err(e) = cadrs_core::History::default().execute(&mut doc, &cmd) {
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        show_toast(
            &mut commands,
            &theme,
            tr_with(
                "shell-details-cannot-import",
                &[("name", file_name.as_str()), ("error", &e.to_string())],
            ),
        );
        world.flush();
        return;
    }
    let meta = cadrs_core::DocumentMeta::new(&user, now);
    let entry = match store.create(&doc, &meta) {
        Ok(e) => e,
        Err(e) => {
            error!("cannot store the imported document: {e}");
            return;
        }
    };
    // The thumbnail rebuilds the Part Studio: on its own thread, as a large mesh takes most of a
    // minute (issue #11). `poll_import_thumbnails` saves it.
    let (tx, rx) = std::sync::mpsc::channel();
    let snapshot = doc.clone();
    let _ = cadrs_core::background::spawn("import-thumbnail", move || {
        let _ = tx.send(crate::script::studio_thumbnail(&snapshot));
    });
    world.spawn((
        Name::new("import-thumbnail"),
        ImportThumbnail {
            id: doc.id,
            rx: std::sync::Mutex::new(rx),
        },
    ));
    world.resource_mut::<Thumbnails>().0.remove(&doc.id);
    world.resource_mut::<Thumbnails>().1.remove(&doc.id);
    let id = doc.id;
    world
        .resource_mut::<DocLibrary>()
        .execute(&store, &AddEntry { entry });
    {
        // Show it: the top level of Owned by me, in the list.
        let mut state = world.resource_mut::<LandingState>();
        if state.filter != Filter::OwnedByMe || state.folder.is_some() {
            state.filter = Filter::OwnedByMe;
            state.folder = None;
        }
    }
    select(world, Pick::Doc(id), false);
    undo_toast(
        world,
        tr_with("shell-details-imported", &[("name", file_name.as_str())]),
    );
}

/// An imported document's thumbnail, rendering on its own thread.
#[derive(Component)]
pub struct ImportThumbnail {
    id: DocumentId,
    rx: std::sync::Mutex<std::sync::mpsc::Receiver<Option<image::RgbaImage>>>,
}

/// Saves the imported documents' thumbnails once they are rendered, and shows them.
pub fn poll_import_thumbnails(
    mut commands: Commands,
    q: Query<(Entity, &ImportThumbnail)>,
    store: Res<DocumentStore>,
    mut lib: ResMut<DocLibrary>,
    mut thumbs: ResMut<Thumbnails>,
) {
    for (e, job) in &q {
        let img = match job.rx.lock().map(|rx| rx.try_recv()) {
            Ok(Ok(img)) => img,
            Ok(Err(std::sync::mpsc::TryRecvError::Empty)) => continue,
            _ => None,
        };
        commands.entity(e).despawn();
        // Not for a document the import's undo took away meanwhile.
        let Some(img) = img.filter(|_| lib.lib.entries.iter().any(|d| d.id == job.id)) else {
            continue;
        };
        if let Err(e) = store.0.write_thumbnail(job.id, &img) {
            warn!("cannot save the imported document's thumbnail: {e}");
            continue;
        }
        thumbs.0.remove(&job.id);
        thumbs.1.remove(&job.id);
        lib.set_changed();
    }
}
