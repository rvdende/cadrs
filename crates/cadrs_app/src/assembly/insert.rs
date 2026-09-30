//! The **Insert parts and assemblies** dialog (A1.1, A2.3, A2.6, A3.6, X3; `ex1-step3.png`,
//! `ex2-step3.png`), opened by the toolbar's Insert or **I**:
//!
//! - Source tabs **Current document** | Other documents (P3F.1) | **Standard content** (P3B.5,
//!   the fastener library, see [`super::standard`]; picks in the view select holes there); the
//!   document's name and branch ("Main") with the insert-rigid and version buttons (P3B.8,
//!   P3D.3, disabled); sub-tabs **Part Studios** | **Assemblies** (P3B.4, A17.2: the other
//!   Assembly tabs, except those that are or hold this one, each inserted as a subassembly
//!   instance); a **Search** field ("Search assemblies" on that tab); type
//!   filters (parts, surfaces; not on the Assemblies tab); the tree: each Part Studio (a row inserts all its parts) with
//!   its parts, each with a thumbnail ([`cadrs_core::assembly::thumb`]); the **Inserted: n**
//!   counter and **Undo to remove instances**.
//! - **Placement** (A2.6): clicking a row picks it up. While the pointer is in the dialog the
//!   picked item shows where it is in its Part Studio (the same position relative to the
//!   origin); in the graphics area it follows the pointer (its footprint's middle on the Top
//!   plane under the pointer) and a click drops an instance there. Each click drops another.
//!   Picking another row, or ✓, inserts an item still waiting at its studio position. Every
//!   insert is one undo step; Undo (the footer, or Ctrl+Z) removes them one by one; ✕ removes
//!   all of this dialog's inserts.

use bevy::asset::RenderAssetUsages;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{EditableText, FontWeight};
use cadrs_core::assembly::commands::InsertInstance;
use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::parts::PartKind;
use cadrs_core::{ElementId, PartId};
use cadrs_ui::prelude::*;
use cadrs_ui::{FeatureDialogAccept, FeatureDialogCancel, TabStrip};

use crate::viewport::{ViewportArea, ViewportRect, ViewportView};
use crate::{ActiveDocument, AppState};

pub struct InsertPlugin;

impl Plugin for InsertPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Thumbnails>()
            .add_systems(
                Update,
                (read_search, super::insert_linked::read_other_search, super::insert_linked::sync_linked_extra, sync_panels, rebuild_tree, insert_pointer, update_ghosts, sync_counter, insert_keys, end_on_tab_change)
                    .chain()
                    .before(crate::parts::PartsSet)
                    .after(crate::viewport::apply_view_to_camera)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
                commands.remove_resource::<InsertSession>();
            })
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_activate)
            .add_observer(on_kind_tab)
            .add_observer(on_button);
    }
}

/// What a tree row inserts.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// The Part Studio's parts (a studio row), or one part.
    pub sources: Vec<InstanceSource>,
    pub label: String,
    /// A rigid Part Studio insert's parts (P3B.8, A2.4): the one source is
    /// [`InstanceSource::Studio`].
    pub rigid: Vec<PartId>,
    /// P3G.1: a linked item (another document, or this one at a version): its sources name the
    /// copies of `link.snapshot`.
    pub link: Option<ItemLink>,
}

/// The reference a linked item is inserted with, and the copies it needs.
#[derive(Debug, Clone)]
pub struct ItemLink {
    pub reference: cadrs_core::external::SourceRef,
    pub snapshot: std::sync::Arc<cadrs_core::external::LinkSnapshot>,
}

impl PartialEq for ItemLink {
    fn eq(&self, o: &Self) -> bool {
        self.reference == o.reference && self.snapshot.root == o.snapshot.root
    }
}

/// The open Insert dialog.
#[derive(Resource, Debug, Clone)]
pub struct InsertSession {
    /// The assembly inserted into.
    pub element: ElementId,
    /// The instances this dialog inserted (in order).
    pub inserted: Vec<InstanceId>,
    /// The item picked up, if any.
    pub armed: Option<Item>,
    /// The picked item hasn't been dropped yet: ✓ or another pick inserts it at its studio
    /// position.
    pub pending: bool,
    /// The search text and the type filters (parts, surfaces).
    pub query: String,
    pub parts: bool,
    pub surfaces: bool,
    /// The Assemblies tab is chosen (A17.2).
    pub assemblies: bool,
    /// **Insert as rigid** (P3B.8, A2.4): a Part Studio row inserts one rigid instance.
    pub rigid: bool,
    /// The Standard content tab is chosen (P3B.5, A19.1).
    pub standard: bool,
    /// Standard content and linked inserts made (each one step over the whole document).
    pub std_steps: usize,
    /// The undo depth when it opened.
    mark: usize,
    /// P3G.1: the Other documents tab is chosen, and its browser.
    pub other: bool,
    pub browse: crate::linked::Browse,
    /// P3G.1 (ER3.1, ER3.2): the Current document at a version (`None`: the workspace), and its
    /// version graph shown.
    pub current: Option<crate::linked::Opened>,
    pub current_graph: bool,
}

impl InsertSession {
    /// The source the tree lists: the opened other document, or this one at a version; `None`
    /// for this document's workspace.
    pub fn linked_source(&self) -> Option<&crate::linked::Opened> {
        if self.other { self.browse.opened.as_ref().filter(|o| o.version.is_some()) } else { self.current.as_ref() }
    }

    /// The version graph is showing.
    pub fn graph_open(&self) -> bool {
        if self.other { self.browse.opened.is_some() && self.browse.graph } else { self.current_graph }
    }
}

/// The dialog.
#[derive(Component)]
struct InsertDialog;

/// The Current document tab's contents (hidden on the Standard content tab).
#[derive(Component)]
struct InsertDocPanel;

/// The type filters row (hidden on the Assemblies tab).
#[derive(Component)]
struct InsertFilters;

/// Ids of the instances shown while an item follows the pointer (not in the document).
const GHOST: u128 = 0x6705_7000_0000_0000_0000_0000_0000_0000;

/// The tree's container.
#[derive(Component)]
struct InsertTree;

/// A tree row.
#[derive(Component, Debug, Clone)]
struct InsertRow(Item);

/// The "Inserted: n" text.
#[derive(Component)]
struct InsertCount;

/// Thumbnails of parts and studios, by (studio, part) and the rebuild they show.
#[derive(Resource, Default)]
pub(crate) struct Thumbnails(std::collections::HashMap<(ElementId, Option<PartId>), (usize, Handle<Image>)>);

/// Opens the dialog on the active assembly (Insert, I).
pub fn open_insert_dialog(world: &mut World) {
    if world.contains_resource::<InsertSession>() {
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let mark = doc.history.undo_len();
    let doc_name = doc.doc.name.clone();
    world.insert_resource(InsertSession {
        element,
        inserted: Vec::new(),
        armed: None,
        pending: false,
        query: String::new(),
        parts: true,
        surfaces: true,
        assemblies: false,
        rigid: false,
        standard: false,
        std_steps: 0,
        mark,
        other: false,
        browse: Default::default(),
        current: None,
        current_graph: false,
    });
    let theme = world.resource::<Theme>().clone();
    let mut q = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = q.iter(world).next() else { return };
    let d = world.spawn(dialog(&theme, doc_name)).id();
    world.entity_mut(area).add_child(d);
}

fn dialog(t: &Theme, doc_name: String) -> impl Bundle {
    let tb = t.clone();
    let tf = t.clone();
    (
        InsertDialog,
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("insert-dialog")
            .title("Insert parts and assemblies")
            .plain_title()
            .width(262.0)
            .body_padding(UiRect::ZERO)
            .body(move |b| {
                let t = &tb;
                // Two-line labels, as Onshape's narrow dialog shows them.
                b.spawn(
                    TabStrip::new("insert-source")
                        .compact()
                        .tab("Current\ndocument")
                        .tab("Other\ndocuments")
                        .tab("Standard\ncontent")
                        .build(t),
                )
                .entry::<Node>()
                .and_modify(|mut n| n.height = Val::Px(36.0));
                // P3B.5: the Standard content tab's panel (see [`super::standard`]).
                b.spawn((Name::new("std-panel"), super::standard::StdPanelHost, Node { flex_direction: FlexDirection::Column, display: Display::None, ..default() }));
                // P3G.1: the Other documents browser (see [`super::insert_linked`]).
                b.spawn((Name::new("insert-other-panel"), super::insert_linked::InsertOtherPanel, Node { flex_direction: FlexDirection::Column, display: Display::None, ..default() }));
                // The document and its branch (or the version read, P3G.1), with insert rigid,
                // Create version and the version graph.
                b.spawn((Name::new("insert-current-header"), super::insert_linked::InsertCurrentHeader, Node {
                    padding: UiRect::new(Val::Px(8.0), Val::Px(6.0), Val::Px(6.0), Val::Px(4.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(4.0),
                    ..default()
                }))
                .with_children(|r| {
                    // The name is clipped before the buttons (it never runs under them).
                    r.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, flex_shrink: 1.0, min_width: Val::Px(0.0), overflow: Overflow::clip(), ..default() })
                        .with_children(|c| {
                            // Cut with "…" (Final regression review: ex1_start 03 was cut mid-letter).
                            c.spawn((
                                Name::new("insert-document-name"),
                                t.text(doc_name.clone(), 12.0, FontWeight::BOLD, t.foreground),
                                cadrs_ui::ellipsis::Ellipsis::node(),
                                cadrs_ui::ellipsis::Ellipsis::default().with_tooltip(),
                            ))
                            .insert(TextLayout::no_wrap());
                            c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(3.0), ..default() })
                                .with_children(|m| {
                                    m.spawn(cadrs_ui::icon::icon("branches", 11.0, t.muted_foreground));
                                    m.spawn((Name::new("insert-branch-label"), t.text("Main", 10.5, FontWeight::NORMAL, t.muted_foreground)));
                                });
                        });
                    for (name, icon, tip) in [
                        // P3B.8 (A2.4): a Part Studio row inserts one rigid instance.
                        ("insert-rigid", "assembly-rigid", "Insert the Part Studio as rigid"),
                        // P3G.1 (ER3.2, DV1.8): a version made on the fly, and the version graph.
                        ("insert-create-version", "versions", "Create version"),
                        ("insert-version", "branches", "Version graph: insert from the workspace or a version"),
                    ] {
                        r.spawn(ToolButton::new(name, icon).icon_size(16.0).tooltip(tip).build(t))
                            .insert(BorderColor::all(Color::srgb_u8(0xd4, 0xd4, 0xd4)))
                            .entry::<Node>()
                            .and_modify(|mut n| {
                                n.width = Val::Px(26.0);
                                n.height = Val::Px(26.0);
                                n.border = UiRect::all(Val::Px(1.0));
                                n.flex_shrink = 0.0;
                            });
                    }
                });
                // P3G.1: the version graph, in place of the list while it is open.
                b.spawn((Name::new("insert-graph-panel"), super::insert_linked::InsertGraphPanel, Node {
                    flex_direction: FlexDirection::Column,
                    display: Display::None,
                    min_height: Val::Px(150.0),
                    max_height: Val::Px(420.0),
                    overflow: Overflow::scroll_y(),
                    ..default()
                }));
                let mut doc_panel = b.spawn((InsertDocPanel, Node { flex_direction: FlexDirection::Column, ..default() }));
                let b = &mut doc_panel;
                b.with_children(|b| {
                b.spawn(TabStrip::new("insert-kind").compact().tab("Part Studios").tab("Assemblies").build(t));
                b.spawn(Node { padding: UiRect::new(Val::Px(6.0), Val::Px(6.0), Val::Px(6.0), Val::Px(4.0)), ..default() })
                    .with_children(|r| {
                        r.spawn(TextInput::new("insert-search").placeholder("Search Part Studios").height(26.0).width(Val::Percent(100.0)).build(t));
                    });
                // Type filters: parts and surfaces (sketches and curves come with their inserts).
                b.spawn((Name::new("insert-filters"), InsertFilters, Node {
                    padding: UiRect::horizontal(Val::Px(6.0)),
                    column_gap: Val::Px(2.0),
                    align_items: AlignItems::Center,
                    ..default()
                }))
                .with_children(|r| {
                    r.spawn(ToolButton::new("insert-filter-parts", "part").icon_size(16.0).selected(true).tooltip("Parts").build(t));
                    r.spawn(ToolButton::new("insert-filter-surfaces", "surface").icon_size(16.0).selected(true).tooltip("Surfaces").build(t));
                    r.spawn(Node { flex_grow: 1.0, ..default() });
                    r.spawn(ToolButton::new("insert-filter-sketches", "sketch").icon_size(16.0).disabled(true).tooltip("Sketches (not available yet)").build(t));
                });
                b.spawn((
                    Name::new("insert-tree"),
                    InsertTree,
                    Node {
                        flex_direction: FlexDirection::Column,
                        min_height: Val::Px(150.0),
                        max_height: Val::Px(330.0),
                        overflow: Overflow::scroll_y(),
                        padding: UiRect::vertical(Val::Px(4.0)),
                        ..default()
                    },
                ));
                b.spawn(Node {
                    justify_content: JustifyContent::FlexEnd,
                    padding: UiRect::new(Val::Px(8.0), Val::Px(10.0), Val::Px(4.0), Val::Px(6.0)),
                    ..default()
                })
                .with_child((Name::new("insert-count"), InsertCount, t.text("Inserted: 0", 11.5, FontWeight::BOLD, t.foreground)));
                });
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn(Node {
                    flex_grow: 1.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                    border: UiRect::top(Val::Px(1.0)),
                    ..default()
                })
                .insert(BorderColor::all(Color::srgb_u8(0xe4, 0xe4, 0xe4)))
                .with_children(|r| {
                    r.spawn(cadrs_ui::Button::new("insert-undo").label("Undo to remove instances").icon("undo").small().ghost().build(t));
                    r.spawn(cadrs_ui::icon::icon("help", 15.0, t.muted_foreground));
                });
            })
            .build(t),
    )
}

/// Rows of the tree: every Part Studio of the document and its parts that pass the filters.
/// A tree row: what it inserts, its thumbnail's key, and whether it is a Part Studio row.
pub(crate) type TreeRow = (Item, Option<(ElementId, Option<PartId>)>, bool);

fn tree_items(doc: &ActiveDocument, parts_of: &mut super::AssemblyParts, s: &InsertSession) -> Vec<TreeRow> {
    if let Some(src) = s.linked_source() {
        return super::insert_linked::linked_tree_items(&doc.doc, parts_of, s, src);
    }
    if s.other {
        return Vec::new();
    }
    let q = s.query.to_lowercase();
    let mut out = Vec::new();
    if s.assemblies {
        // A17.2: the other Assembly tabs; not this one, nor one that holds it (no cycles).
        for el in doc.doc.elements.iter().filter(|e| e.assembly_model().is_some()) {
            if cadrs_core::assembly::structure::contains_assembly(&doc.doc, el.id, s.element) || !el.name.to_lowercase().contains(&q) {
                continue;
            }
            out.push((Item { sources: vec![InstanceSource::Assembly { element: el.id }], label: el.name.clone(), rigid: Vec::new(), link: None }, Some((el.id, None)), false));
        }
        return out;
    }
    for el in doc.doc.elements.iter().filter(|e| e.assembly_model().is_none()) {
        let Some(build) = parts_of.build(&doc.doc, el.id) else { continue };
        let parts: Vec<&cadrs_core::Part> = build
            .parts
            .iter()
            .filter(|p| match p.kind {
                PartKind::Solid => s.parts,
                PartKind::Surface => s.surfaces,
            })
            .collect();
        let studio_hit = el.name.to_lowercase().contains(&q);
        let names: Vec<String> = parts.iter().map(|p| cadrs_core::parts::display_name(p, el.part_props()).to_string()).collect();
        // P3G.1 (ER1.4): a part's number and description match too.
        let by_property = |i: usize| {
            el.part_prop(parts[i].id).is_some_and(|pp| [&pp.properties.part_number, &pp.properties.description].iter().any(|v| v.as_ref().is_some_and(|v| v.to_lowercase().contains(&q))))
        };
        let shown: Vec<usize> = (0..parts.len()).filter(|i| studio_hit || names[*i].to_lowercase().contains(&q) || by_property(*i)).collect();
        if shown.is_empty() {
            continue;
        }
        let all: Vec<InstanceSource> = parts.iter().map(|p| InstanceSource::Part { element: el.id, part: p.id }).collect();
        // A2.4: with Insert as rigid, the studio's row inserts all its parts as one instance.
        let studio_item = if s.rigid {
            Item { sources: vec![InstanceSource::Studio { element: el.id }], label: el.name.clone(), rigid: parts.iter().map(|p| p.id).collect(), link: None }
        } else {
            Item { sources: all, label: el.name.clone(), rigid: Vec::new(), link: None }
        };
        out.push((studio_item, Some((el.id, None)), true));
        for i in shown {
            out.push((
                Item { sources: vec![InstanceSource::Part { element: el.id, part: parts[i].id }], label: names[i].clone(), rigid: Vec::new(), link: None },
                Some((el.id, Some(parts[i].id))),
                false,
            ));
        }
    }
    out
}

fn read_search(q: Query<(&Name, &EditableText)>, session: Option<ResMut<InsertSession>>) {
    let Some(mut s) = session else { return };
    if let Some((_, t)) = q.iter().find(|(n, _)| n.as_str() == "insert-search-field") {
        let v = t.value().to_string();
        if s.query != v {
            s.query = v;
        }
    }
}

/// A thumbnail image for a studio (all its parts) or a part.
pub(crate) fn thumbnail(
    thumbs: &mut Thumbnails,
    images: &mut Assets<Image>,
    doc: &ActiveDocument,
    parts_of: &mut super::AssemblyParts,
    key: (ElementId, Option<PartId>),
) -> Option<Handle<Image>> {
    // P3G.1: a linked assembly not inserted yet: seen with its copies.
    let with_links;
    let d: &cadrs_core::Document = if doc.doc.element(key.0).is_none() && parts_of.linked_extra.iter().any(|l| l.id() == key.0) {
        with_links = crate::linked::with_links(&doc.doc, &parts_of.linked_extra);
        &with_links
    } else {
        &doc.doc
    };
    let img = if let Some(asm) = d.element(key.0).and_then(|e| e.assembly_model()) {
        // An Assembly tab: its parts where they are.
        let (parts, props) = cadrs_core::assembly::instance_parts(d, asm, |e| parts_of.build(d, e));
        // An empty one gets the placeholder (the tree draws it).
        if parts.is_empty() {
            return None;
        }
        let ptr = parts.len() + asm.instances.len() * 1000;
        if let Some((p, h)) = thumbs.0.get(&key)
            && *p == ptr
        {
            return Some(h.clone());
        }
        let list: Vec<(&cadrs_core::Solid, [u8; 3])> =
            parts.iter().map(|p| (&*p.solid, cadrs_core::appearance::part_appearance(p, &props).rgb)).collect();
        (ptr, cadrs_core::assembly::thumb::render(&list, 72))
    } else {
        let build = parts_of.build(&doc.doc, key.0)?;
        let ptr = std::sync::Arc::as_ptr(&build) as usize;
        if let Some((p, h)) = thumbs.0.get(&key)
            && *p == ptr
        {
            return Some(h.clone());
        }
        let props = d.element(key.0).map(|e| e.part_props().to_vec()).unwrap_or_default();
        let list: Vec<(&cadrs_core::Solid, [u8; 3])> = build
            .parts
            .iter()
            .filter(|p| key.1.is_none_or(|id| id == p.id))
            .map(|p| (&*p.solid, cadrs_core::appearance::part_appearance(p, &props).rgb))
            .collect();
        (ptr, cadrs_core::assembly::thumb::render(&list, 72))
    };
    let (ptr, img) = img;
    let (w, h) = img.dimensions();
    let image = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        img.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    let handle = images.add(image);
    thumbs.0.insert(key, (ptr, handle.clone()));
    Some(handle)
}

/// What the tree was built from.
type TreeKey = Vec<(String, bool, Option<(ElementId, Option<PartId>)>, bool)>;

#[allow(clippy::too_many_arguments)]
fn rebuild_tree(
    session: Option<Res<InsertSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut parts_of: ResMut<super::AssemblyParts>,
    mut thumbs: ResMut<Thumbnails>,
    mut images: ResMut<Assets<Image>>,
    q_tree: Query<(Entity, Ref<InsertTree>)>,
    theme: Res<Theme>,
    mut last: Local<Option<TreeKey>>,
    mut commands: Commands,
) {
    let (Some(s), Some(doc)) = (session, doc) else {
        *last = None;
        return;
    };
    let Some((tree, added)) = q_tree.iter().next().map(|(e, r)| (e, r.is_added())) else { return };
    let items = tree_items(&doc, &mut parts_of, &s);
    let key: TreeKey = items
        .iter()
        .map(|(it, k, studio)| (if it.rigid.is_empty() { it.label.clone() } else { format!("{} (rigid)", it.label) }, *studio, *k, s.armed.as_ref() == Some(it)))
        .collect();
    if !added && last.as_ref() == Some(&key) {
        return;
    }
    let t = theme.clone();
    commands.entity(tree).despawn_children();
    let mut n = 0;
    for (item, k, studio) in items {
        n += 1;
        let img = k.and_then(|k| thumbnail(&mut thumbs, &mut images, &doc, &mut parts_of, k));
        let is_asm = item.sources.first().is_some_and(|s| s.is_assembly());
        let slug = slug(&item.label);
        let name = if studio {
            format!("insert-studio-{slug}")
        } else if item.sources.first().is_some_and(|s| s.is_assembly()) {
            format!("insert-assembly-{slug}")
        } else {
            format!("insert-part-{slug}")
        };
        let armed = s.armed.as_ref() == Some(&item);
        let label = item.label.clone();
        let rigid_tip = (!item.rigid.is_empty()).then(|| format!("{label}: inserted as one rigid instance ({} parts)", item.rigid.len()));
        let row = commands
            .spawn((
                TreeItem::new(name, label)
                    .height(38.0)
                    .left(if studio { 4.0 } else { 30.0 })
                    .disclosure(studio.then_some(true))
                    .selected(armed)
                    .build(&t),
                InsertRow(item),
                ChildOf(tree),
            ))
            .id();
        if let Some(tip) = rigid_tip {
            commands.entity(row).insert(Tooltip::new(tip));
        }
        if let Some(img) = img {
            let thumb = commands
                .spawn((
                    Node { width: Val::Px(34.0), height: Val::Px(34.0), flex_shrink: 0.0, ..default() },
                    ImageNode::new(img),
                    Pickable::IGNORE,
                ))
                .id();
            commands.entity(row).insert_children(if studio { 1 } else { 0 }, &[thumb]);
        } else if is_asm {
            // An empty assembly: a placeholder with the assembly icon (P3B.5 judge).
            let thumb = commands
                .spawn((
                    Name::new(format!("insert-assembly-{slug}-placeholder")),
                    Node {
                        width: Val::Px(34.0),
                        height: Val::Px(34.0),
                        flex_shrink: 0.0,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(0xf3, 0xf4, 0xf6)),
                    BorderColor::all(Color::srgb_u8(0xd4, 0xd4, 0xd8)),
                    Pickable::IGNORE,
                    children![(cadrs_ui::icon("assembly", 20.0, t.muted_foreground), Pickable::IGNORE)],
                ))
                .id();
            commands.entity(row).insert_children(0, &[thumb]);
        }
    }
    if n == 0 {
        commands.spawn((
            t.text(if s.assemblies { "No other assemblies" } else { "No Part Studios match" }, 11.5, FontWeight::NORMAL, t.muted_foreground),
            Node { margin: UiRect::all(Val::Px(10.0)), ..default() },
            ChildOf(tree),
        ));
    }
    *last = Some(key);
}

/// Part Studios | Assemblies (A2.3, A17.2); Current document | Standard content (P3B.5).
fn on_kind_tab(ev: On<cadrs_ui::TabStripSelect>, q: Query<&Name>, session: Option<ResMut<InsertSession>>, mut commands: Commands) {
    let Some(mut s) = session else { return };
    match q.get(ev.entity).map(|n| n.as_str()) {
        Ok("insert-kind") => {
            s.assemblies = ev.index == 1;
            s.armed = None;
            s.pending = false;
        }
        Ok("insert-source") => {
            s.standard = ev.index == 2;
            s.other = ev.index == 1;
            s.armed = None;
            s.pending = false;
            if s.standard {
                commands.queue(|world: &mut World| {
                    if !world.contains_resource::<super::standard::StdForm>() {
                        world.insert_resource(super::standard::StdForm::insert_default());
                    }
                });
            } else {
                commands.queue(|world: &mut World| {
                    world.remove_resource::<super::standard::StdForm>();
                });
            }
        }
        _ => {}
    }
}

/// The tab's panel shown (Current document, Other documents or Standard content); the version
/// graph in place of the list while it is open (P3G.1); on the Assemblies tab the search reads
/// "Search assemblies" and the part / surface filters are hidden.
#[allow(clippy::type_complexity)]
fn sync_panels(
    session: Option<Res<InsertSession>>,
    mut q: Query<
        (&mut Node, Has<InsertDocPanel>, Has<super::standard::StdPanelHost>, Has<InsertFilters>, Has<super::insert_linked::InsertOtherPanel>, Has<super::insert_linked::InsertCurrentHeader>),
        Or<(
            With<InsertDocPanel>,
            With<super::standard::StdPanelHost>,
            With<InsertFilters>,
            With<super::insert_linked::InsertOtherPanel>,
            With<super::insert_linked::InsertCurrentHeader>,
            With<super::insert_linked::InsertGraphPanel>,
        )>,
    >,
    mut q_text: Query<(&Name, &mut Text)>,
) {
    let Some(s) = session else { return };
    let show = |b: bool| if b { Display::Flex } else { Display::None };
    let graph = !s.standard && s.graph_open();
    for (mut n, doc_panel, std_panel, filters, other, header) in &mut q {
        let want = if doc_panel {
            !s.standard && !graph && (!s.other || s.linked_source().is_some())
        } else if std_panel {
            s.standard
        } else if filters {
            !s.assemblies
        } else if other {
            s.other && !s.standard
        } else if header {
            !s.other && !s.standard
        } else {
            graph
        };
        if n.display != show(want) {
            n.display = show(want);
        }
    }
    let want = if s.assemblies { "Search assemblies" } else { "Search Part Studios" };
    // The Current document's branch or version (ER3.2: "Main", or the version inserted from).
    let branch = s.current.as_ref().map(|o| o.version_label()).unwrap_or_else(|| "Main".into());
    for (name, mut t) in &mut q_text {
        if name.as_str() == "insert-search-placeholder" && t.0 != want {
            t.0 = want.to_string();
        }
        if name.as_str() == "insert-branch-label" && t.0 != branch {
            t.0 = branch.clone();
        }
    }
}

/// A node-name slug: "DC Motor Mount (stand-in)" → "dc-motor-mount-stand-in".
pub fn slug(label: &str) -> String {
    let mut out = String::new();
    for c in label.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Inserts `item` at `pose` (one command per part, so Undo removes them one by one; a linked
/// item's parts together with the copies they need, one step, P3G.1).
fn insert_item(world: &mut World, item: &Item, pose: Pose) {
    let Some(element) = world.get_resource::<InsertSession>().map(|s| s.element) else { return };
    if let Some(link) = &item.link {
        let instances: Vec<Instance> = item
            .sources
            .iter()
            .map(|src| match src {
                InstanceSource::Studio { element: el } => Instance::studio(InstanceId::new(), *el, item.rigid.clone(), pose),
                _ => Instance::new(InstanceId::new(), *src, pose),
            })
            .collect();
        let ids: Vec<InstanceId> = instances.iter().map(|i| i.id).collect();
        let cmd = cadrs_core::external::InsertLinked { element, snapshot: (*link.snapshot).clone(), instances, reference: link.reference };
        let result = world.resource_mut::<ActiveDocument>().execute(&cmd);
        match result {
            Ok(()) => {
                if let Some(mut s) = world.get_resource_mut::<InsertSession>() {
                    s.inserted.extend(ids);
                    s.std_steps += 1;
                }
            }
            // DV1.5: a circular reference is refused, and says so (the error toast).
            Err(e) => {
                crate::linked::error_toast(world, e.to_string());
                if let Some(mut s) = world.get_resource_mut::<InsertSession>() {
                    s.armed = None;
                    s.pending = false;
                }
            }
        }
        return;
    }
    for src in &item.sources {
        let id = InstanceId::new();
        let instance = match src {
            InstanceSource::Studio { element: el } => Instance::studio(id, *el, item.rigid.clone(), pose),
            _ => Instance::new(id, *src, pose),
        };
        if super::run(world, &InsertInstance { element, instance })
            && let Some(mut s) = world.get_resource_mut::<InsertSession>()
        {
            s.inserted.push(id);
        }
    }
}

/// The middle of the footprint (XY bounding box, at z = 0) of what `item` inserts, in its
/// studio's coordinates.
fn footprint_middle(world: &mut World, item: &Item) -> Vec3 {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    let mut doc = world.resource::<ActiveDocument>().doc.clone();
    let mut parts_of = world.resource_mut::<super::AssemblyParts>();
    if let Some(l) = &item.link {
        doc = crate::linked::with_links(&doc, &l.snapshot.links);
    }
    for src in &item.sources {
        // A subassembly: its parts where they are in its tab.
        let placed: Vec<(ElementId, PartId, Pose)> = match src {
            InstanceSource::Part { element, part } => vec![(*element, *part, Pose::IDENTITY)],
            InstanceSource::Studio { element } => item.rigid.iter().map(|p| (*element, *p, Pose::IDENTITY)).collect(),
            InstanceSource::Assembly { element } => doc
                .element(*element)
                .and_then(|e| e.assembly_model())
                .map(|a| cadrs_core::assembly::structure::occurrences(&doc, a).into_iter().map(|o| (o.element, o.part, o.pose)).collect())
                .unwrap_or_default(),
        };
        for (el, part, pose) in placed {
            let Some(b) = parts_of.build(&doc, el) else { continue };
            if let Some(p) = b.part(part) {
                for q in &p.solid.positions {
                    let w = pose.apply(*q);
                    let v = Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32);
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
    }
    if lo.x > hi.x {
        return Vec3::ZERO;
    }
    let m = (lo + hi) / 2.0;
    Vec3::new(m.x, m.y, 0.0)
}

/// The pose that puts `item`'s footprint middle under the pointer, on the Top plane (on the
/// plane through the origin facing the viewer when the Top plane is seen edge-on).
pub(crate) fn pose_at_pointer(world: &mut World, item: &Item, screen: Vec2) -> Pose {
    let middle = footprint_middle(world, item);
    let view = world.resource::<ViewportView>().view;
    let offset = world.resource::<ViewportRect>().offset(screen);
    let (o, d) = view.ray(offset);
    let p = if d.z.abs() > 0.15 {
        o + d * (-o.z / d.z)
    } else {
        let n = view.back();
        o + d * (n.dot(-o) / n.dot(d))
    };
    let t = p - middle;
    Pose::translation([t.x as f64, t.y as f64, t.z as f64])
}

/// Picks up a row's item (a click in the tree).
fn on_activate(a: On<Activate>, q: Query<&InsertRow>, mut commands: Commands) {
    let Ok(row) = q.get(a.entity) else { return };
    let item = row.0.clone();
    commands.queue(move |world: &mut World| arm(world, item));
}

fn arm(world: &mut World, item: Item) {
    let Some(s) = world.get_resource::<InsertSession>().cloned() else { return };
    // An item still waiting is inserted where it is in its Part Studio.
    if s.pending
        && let Some(prev) = s.armed.clone()
    {
        insert_item(world, &prev, Pose::IDENTITY);
    }
    if let Some(mut s) = world.get_resource_mut::<InsertSession>() {
        s.armed = Some(item);
        s.pending = true;
    }
}

/// Clicks in the graphics area drop the picked item there.
#[allow(clippy::too_many_arguments)]
fn insert_pointer(
    mut inputs: MessageReader<PointerInput>,
    session: Option<Res<InsertSession>>,
    hover: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut grab: ResMut<super::ViewportGrab>,
    mut down: Local<Option<Vec2>>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        inputs.clear();
        *down = None;
        return;
    };
    let over = crate::viewport::pointer_over_viewport(&hover, &q_area);
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) if over && s.armed.is_some() => {
                grab.0 = true;
                *down = Some(pos);
            }
            PointerAction::Release(PointerButton::Primary) => {
                if let Some(d) = down.take()
                    && d.distance(pos) < 4.0
                    && let Some(item) = s.armed.clone()
                {
                    commands.queue(move |world: &mut World| {
                        let pose = pose_at_pointer(world, &item, pos);
                        insert_item(world, &item, pose);
                        if let Some(mut s) = world.get_resource_mut::<InsertSession>() {
                            s.pending = false;
                        }
                    });
                }
            }
            _ => {}
        }
    }
}

/// The picked item on screen: under the pointer in the graphics area; at its studio position
/// while it waits and the pointer is elsewhere (in the dialog).
fn update_ghosts(world: &mut World) {
    let Some(s) = world.get_resource::<InsertSession>().cloned() else {
        // The Replicate dialog previews its copies as ghosts too (P3B.8).
        if world.contains_resource::<super::replicate_dialog::ReplicateSession>() {
            return;
        }
        let mut a = world.resource_mut::<super::AssemblyParts>();
        if !a.ghosts.is_empty() {
            a.ghosts.clear();
        }
        return;
    };
    // The Standard content tab places its own (see [`super::standard`]).
    if s.standard {
        return;
    }
    let over = {
        let mut q = world.query_filtered::<Entity, With<ViewportArea>>();
        let areas: Vec<Entity> = q.iter(world).collect();
        world
            .resource::<HoverMap>()
            .get(&PointerId::Mouse)
            .is_some_and(|hits| areas.iter().any(|a| hits.contains_key(a)))
    };
    let pointer = world.resource::<crate::viewport::ViewportDrag>().pointer();
    let ghosts: Vec<Instance> = match &s.armed {
        Some(item) if over => {
            let pose = pose_at_pointer(world, item, pointer);
            item.sources.iter().enumerate().map(|(k, src)| Instance::new(InstanceId::from_u128(GHOST + k as u128), *src, pose)).collect()
        }
        Some(item) if s.pending => {
            item.sources.iter().enumerate().map(|(k, src)| Instance::new(InstanceId::from_u128(GHOST + k as u128), *src, Pose::IDENTITY)).collect()
        }
        _ => Vec::new(),
    };
    let mut a = world.resource_mut::<super::AssemblyParts>();
    if a.ghosts != ghosts {
        a.ghosts = ghosts;
    }
}

/// "Inserted: n": this dialog's inserts still in the assembly (Undo removes them).
fn sync_counter(session: Option<ResMut<InsertSession>>, doc: Option<Res<ActiveDocument>>, mut q: Query<&mut Text, With<InsertCount>>) {
    let (Some(mut s), Some(doc)) = (session, doc) else { return };
    let present = |id: &InstanceId| {
        doc.doc.element(s.element).and_then(|e| e.assembly_model()).is_some_and(|a| a.instance(*id).is_some())
    };
    let keep: Vec<InstanceId> = s.inserted.iter().copied().filter(present).collect();
    if keep != s.inserted {
        s.inserted = keep;
    }
    let text = format!("Inserted: {}", s.inserted.len());
    for mut t in &mut q {
        if t.0 != text {
            t.0 = text.clone();
        }
    }
}

/// Enter accepts, Esc cancels.
#[allow(clippy::too_many_arguments)]
fn insert_keys(
    mut keys: MessageReader<KeyboardInput>,
    session: Option<Res<InsertSession>>,
    std_form: Option<Res<super::standard::StdForm>>,
    focus: Res<bevy::input_focus::InputFocus>,
    q_fields: Query<(), With<cadrs_ui::input::TextInputField>>,
    mut commands: Commands,
) {
    if session.is_none() {
        keys.clear();
        return;
    }
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    // Esc while standard content is being placed stops placing (not the dialog).
    let placing = std_form.as_ref().is_some_and(|f| f.placing || f.autosize_pick);
    for k in keys.read() {
        if k.state != ButtonState::Pressed {
            continue;
        }
        match k.key_code {
            KeyCode::Enter if !typing => commands.queue(accept),
            KeyCode::Escape if placing => commands.queue(|world: &mut World| {
                if let Some(mut f) = world.get_resource_mut::<super::standard::StdForm>() {
                    f.placing = false;
                    f.autosize_pick = false;
                }
            }),
            KeyCode::Escape => commands.queue(cancel),
            _ => {}
        }
    }
}

fn end_on_tab_change(session: Option<Res<InsertSession>>, doc: Option<Res<ActiveDocument>>, mut commands: Commands) {
    if let (Some(s), Some(doc)) = (session, doc)
        && doc.active != Some(s.element)
    {
        commands.queue(close);
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<InsertDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<InsertDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(cancel);
    }
}

/// ✓: an item still waiting goes in at its studio position; the dialog closes.
pub fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<InsertSession>().cloned() else { return };
    // The holes standard content went on stay selected while the dialog is open (for
    // stacking); ✓ clears them.
    if s.std_steps > 0 {
        world.resource_mut::<crate::viewport::Selection>().0.clear();
    }
    if s.pending
        && let Some(item) = s.armed
    {
        insert_item(world, &item, Pose::IDENTITY);
    }
    close(world);
}

/// ✕: removes this dialog's inserts and closes it.
pub fn cancel(world: &mut World) {
    let Some(s) = world.get_resource::<InsertSession>().cloned() else { return };
    if !s.inserted.is_empty()
        && let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
    {
        // Standard content inserts edit the whole document: undo back to where the dialog
        // opened.
        if s.std_steps > 0 {
            while doc.history.undo_len() > s.mark {
                if doc.undo().is_none() {
                    break;
                }
            }
        }
        doc.discard_element_since(s.mark, s.element);
    }
    close(world);
}

fn close(world: &mut World) {
    world.remove_resource::<InsertSession>();
    if world.get_resource::<super::standard::StdForm>().is_some_and(|f| f.mode == super::standard::StdMode::Insert) {
        world.remove_resource::<super::standard::StdForm>();
    }
    world.resource_mut::<super::AssemblyParts>().ghosts.clear();
    let mut q = world.query_filtered::<Entity, With<InsertDialog>>();
    let dialogs: Vec<Entity> = q.iter(world).collect();
    for d in dialogs {
        world.entity_mut(d).despawn();
    }
}

fn on_button(a: On<Activate>, q: Query<&Name>, session: Option<Res<InsertSession>>, mut commands: Commands) {
    if session.is_none() {
        return;
    }
    let Ok(name) = q.get(a.entity) else { return };
    let name = name.as_str().to_string();
    if name == "insert-filter-parts" || name == "insert-filter-surfaces" || name == "insert-rigid" {
        let on = match name.as_str() {
            "insert-rigid" => !session.as_ref().is_some_and(|s| s.rigid),
            "insert-filter-parts" => !session.as_ref().is_some_and(|s| s.parts),
            _ => !session.as_ref().is_some_and(|s| s.surfaces),
        };
        if on {
            commands.entity(a.entity).try_insert(Selected);
        } else {
            commands.entity(a.entity).try_remove::<Selected>();
        }
    }
    commands.queue(move |world: &mut World| on_dialog_button(world, &name));
}

/// The footer's Undo and the type filters.
fn on_dialog_button(world: &mut World, name: &str) {
    match name {
        "insert-undo" => {
            let Some(s) = world.get_resource::<InsertSession>().cloned() else { return };
            if s.inserted.is_empty() {
                return;
            }
            if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
                doc.undo();
            }
        }
        "insert-rigid" => {
            if let Some(mut s) = world.get_resource_mut::<InsertSession>() {
                s.rigid = !s.rigid;
                s.armed = None;
            }
        }
        // P3G.1: the version graph (ER3.2) and Create version (DV1.8).
        "insert-version" => {
            if let Some(mut s) = world.get_resource_mut::<InsertSession>() {
                s.current_graph = !s.current_graph;
            }
        }
        "insert-create-version" => crate::linked::open_create_version_dialog(world, crate::linked::VersionTarget::Current),
        "insert-filter-parts" | "insert-filter-surfaces" => {
            if let Some(mut s) = world.get_resource_mut::<InsertSession>() {
                if name == "insert-filter-parts" {
                    s.parts = !s.parts;
                } else {
                    s.surfaces = !s.surfaces;
                }
            }
        }
        _ => {}
    }
}
