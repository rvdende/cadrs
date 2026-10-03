//! Placing and moving views (P3C.2, D1.7, D4.1–D4.7, D7.1, D7.2, D7.5, X7).
//!
//! - **Insert view** (D4.1–D4.3; `ex1-step4.png`, `ex2-step5.png`): the "Insert view" card at the
//!   top left (the referenced object, the orientation, the scale and the named-position stub
//!   "None") and the **Select a part or assembly** browser beside it (Current document / Other
//!   documents, the document and its branch, Part Studios / Assemblies, a search field, type
//!   filters, and the studios with their parts). Picking a whole Part Studio shows a warning
//!   (D4.2). With something picked, the view follows the cursor; a click places it. It opens
//!   by itself after Create Drawing → OK (D1.7).
//! - **Projected view** (D4.4, D4.5): after the base view the tool switches to Projected view.
//!   Moving the cursor right, up, left or down of the parent previews that orthographic view
//!   (placed by the template's first or third angle), diagonally an isometric one; a click
//!   places it, a click on another view makes that the parent, Esc ends the tool.
//! - **Auxiliary view** (D4.6): click a straight edge of a view, then place the view folded out
//!   from it.
//! - **Align view vertical / horizontal** (D7.5): after the menu item, click an edge of the view;
//!   the view turns so the edge is vertical (horizontal) on the sheet.
//! - **Selecting and dragging** (D7.1, D7.2): a click selects a view (Ctrl adds), a drag moves
//!   it; aligned views slide along their parent's fold line and children follow their parent.
//!   Delete removes the selected views.

use std::collections::HashSet;

use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::commands::EditDrawing;
use cadrs_core::{ElementId, ElementKind, PartId};
use cadrs_drawing::view::{Placement, auxiliary_view, placement_for, projected_view, rotate};
use cadrs_drawing::{DrawingOp, NamedView, ObjectRef, Scale, SheetId, View, ViewId};
use cadrs_ui::prelude::*;
use cadrs_ui::input::TextInputField;
use cadrs_ui::{Select, SelectChange, TabStrip, TabStripSelect, TreeToggle};

use super::views::{ViewCache, edge_at, sheet_bounds, view_at};
use super::{DrawingUi, active_drawing, current_view, screen_to_sheet, sheet_area};
use crate::viewport::{ActiveKind, ViewportArea, ViewportRect};
use crate::{ActiveDocument, AppState};

pub struct ViewToolsPlugin;

/// The view tools' input systems (the annotation tools' run before them).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewToolsSet;

impl Plugin for ViewToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InsertViewState>()
            .add_systems(
                Update,
                (
                    view_pointer,
                    view_keys,
                    update_ghost,
                    super::view_kind_tools::preview,
                    sync_insert_dialog,
                    sync_tool_hint,
                    read_browser_search,
                )
                    .chain()
                    .in_set(ViewToolsSet)
                    .after(super::drawing_keys)
                    .before(super::views::ViewsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut s: ResMut<InsertViewState>| {
                *s = InsertViewState::default();
            })
            .add_observer(on_insert_select)
            .add_observer(on_browser_tab)
            .add_observer(on_browser_toggle);
    }
}

/// The active view tool.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ViewTool {
    #[default]
    None,
    /// The Insert view dialog is open (placing once a reference is picked).
    Insert,
    /// Projected view from `parent` (none yet: click a view).
    Projected { parent: Option<ViewId> },
    /// Auxiliary view: click an edge (parent, edge direction in its 2D frame), then place.
    Auxiliary { from: Option<(ViewId, [f64; 2])> },
    /// Align view vertical (or horizontal): click an edge of the view.
    Align { view: ViewId, vertical: bool },
    /// Section view (P3C.8): the parent and the cutting line's points (its 2D frame).
    Section { parent: Option<ViewId>, a: Option<[f64; 2]>, b: Option<[f64; 2]> },
    /// Detail view: the parent, the circle's centre and radius.
    Detail { parent: Option<ViewId>, center: Option<[f64; 2]>, radius: Option<f64> },
    /// Crop view (rectangle): the view and the first corner.
    Crop { view: Option<ViewId>, a: Option<[f64; 2]> },
    /// Break view: the view and the first break line.
    Break { view: Option<ViewId>, a: Option<[f64; 2]> },
    /// A boundary drawn point by point (spline crop, broken-out section); the points are in
    /// [`DrawingUi::tool_points`].
    Boundary { view: Option<ViewId>, use_: super::view_kind_tools::BoundaryUse },
}

impl ViewTool {
    /// One of the P3C.8 view-kind tools.
    pub fn is_view_kind(&self) -> bool {
        matches!(
            self,
            ViewTool::Section { .. } | ViewTool::Detail { .. } | ViewTool::Crop { .. } | ViewTool::Break { .. } | ViewTool::Boundary { .. }
        )
    }

    /// A view-kind tool still waiting for its view to be picked.
    pub fn picks_view(&self) -> bool {
        matches!(
            self,
            ViewTool::Section { parent: None, .. }
                | ViewTool::Detail { parent: None, .. }
                | ViewTool::Crop { view: None, .. }
                | ViewTool::Break { view: None, .. }
                | ViewTool::Boundary { view: None, .. }
        )
    }
}

/// One Part Studio in the browser: its parts (id, name).
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserStudio {
    pub id: ElementId,
    pub name: String,
    pub parts: Vec<(PartId, String)>,
}

/// The Insert view dialog's state.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct InsertViewState {
    pub reference: Option<ObjectRef>,
    pub orientation: NamedView,
    pub scale: Scale,
    pub browser_open: bool,
    /// Part Studios (0) or Assemblies (1).
    pub tab: usize,
    pub search: String,
    pub expanded: HashSet<ElementId>,
    pub studios: Vec<BrowserStudio>,
    /// The document's assemblies (P3C.5: views of them work).
    pub assemblies: Vec<(ElementId, String)>,
    pub document: String,
    /// What the UI was built from.
    built: bool,
    /// P3G.1 (D13.1, ER1.1): Current document (0) or Other documents (1), the Other documents
    /// browser, its list and thumbnails, this document at a version (`None`: the workspace) and
    /// its version graph shown, and the graph built (see [`super::view_linked`]).
    pub source_tab: usize,
    pub browse: crate::linked::Browse,
    pub other_list: crate::linked::BrowserList,
    pub other_thumbs: std::collections::HashMap<cadrs_core::DocumentId, Handle<Image>>,
    pub current: Option<crate::linked::Opened>,
    pub current_graph: bool,
    pub graph: Option<cadrs_ui::VersionGraph>,
    /// P3I.7 (SM16.2): the reference is a sheet metal part's flat pattern; the Flat patterns
    /// filter is on; and the document's flat patterns (studio, part, name).
    pub flat: bool,
    pub filter_flat: bool,
    pub flats: Vec<(ElementId, PartId, String)>,
}

impl Default for InsertViewState {
    fn default() -> Self {
        Self {
            reference: None,
            orientation: NamedView::Front,
            scale: Scale::new(1, 1),
            browser_open: true,
            tab: 0,
            search: String::new(),
            expanded: HashSet::new(),
            studios: Vec::new(),
            assemblies: Vec::new(),
            document: String::new(),
            built: false,
            source_tab: 0,
            browse: Default::default(),
            other_list: Default::default(),
            other_thumbs: Default::default(),
            current: None,
            current_graph: false,
            graph: None,
            flat: false,
            filter_flat: false,
            flats: Vec::new(),
        }
    }
}

/// The scales offered for views (D4.7).
pub fn scales(current: Scale) -> Vec<Scale> {
    let mut s: Vec<Scale> = Scale::COMMON.to_vec();
    if !s.contains(&current) {
        s.push(current);
    }
    s
}

fn exec(world: &mut World, op: DrawingOp) -> bool {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return false;
    };
    let Some((element, _)) = active_drawing(&doc) else {
        return false;
    };
    match doc.execute(&EditDrawing { element, op }) {
        Ok(()) => true,
        Err(e) => {
            warn!("drawing edit refused: {e}");
            false
        }
    }
}

/// Runs a drawing edit on the active drawing.
pub fn edit_drawing(world: &mut World, op: DrawingOp) -> bool {
    exec(world, op)
}

/// Inserting view `v` on `sheet` (P3C.6): the view shows its studio's state kept in the drawing
/// (with that state's hash of its part); the first view of a studio records the studio as it is
/// now, in the same undoable step.
pub fn insert_view_op(world: &World, sheet: SheetId, mut v: View) -> DrawingOp {
    let Some((d, doc)) = world.get_resource::<ActiveDocument>().and_then(|doc| Some((active_drawing(doc)?.1, &doc.doc))) else {
        return DrawingOp::InsertView { sheet, view: v };
    };
    if let Some(src) = d.source(v.reference.element) {
        v.source_hash = src.hash_of(v.reference.part);
        return DrawingOp::InsertView { sheet, view: v };
    }
    match cadrs_core::drawing_source::live_source(doc, cadrs_core::ElementId(v.reference.element)) {
        Some(src) => {
            v.source_hash = src.hash_of(v.reference.part);
            let insert = DrawingOp::InsertView { sheet, view: v };
            let label = insert.label();
            DrawingOp::Batch { ops: vec![DrawingOp::SetSource(src), insert], label }
        }
        None => DrawingOp::InsertView { sheet, view: v },
    }
}

/// Ends the current view tool (Esc).
pub fn end_tool(world: &mut World) {
    let mut ui = world.resource_mut::<DrawingUi>();
    ui.tool = ViewTool::None;
    ui.ghost = None;
    ui.highlight_edge = None;
    ui.tool_points.clear();
    ui.tool_strokes.clear();
}

// ---------------------------------------------------------------------------------------------
// Insert view

/// Opens the Insert view dialog and browser (the toolbar, the sheet menu, or right after
/// Create Drawing).
pub fn open_insert_view(world: &mut World) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let Some((id, d)) = active_drawing(doc) else {
        return;
    };
    let ui = world.resource::<DrawingUi>();
    let sheet = d.sheets.get(ui.sheet_index(id, d));
    // The sheet's scale once it has views (D4.7); the referenced studio opened in the tree.
    let scale = sheet.filter(|s| !s.views.is_empty()).map(|s| s.scale).unwrap_or(Scale::new(1, 1));
    let expand = sheet.and_then(|s| s.reference).map(|r| ElementId(r.element));
    let mut studios = Vec::new();
    let mut assemblies = Vec::new();
    let mut flats = Vec::new();
    for el in &doc.doc.elements {
        match &el.kind {
            ElementKind::PartStudio { .. } => {
                let build = cadrs_core::rebuild::build(el.features());
                let props = el.part_props();
                // P3I.7: every sheet metal part's flat pattern.
                for p in cadrs_core::flat_drawing::flat_parts(&build.sheet_metal) {
                    if let Some(part) = build.part(p) {
                        flats.push((el.id, p, format!("Flat pattern of {}", cadrs_core::parts::display_name(part, props))));
                    }
                }
                let parts = build
                    .parts
                    .iter()
                    .filter(|p| p.kind == cadrs_core::PartKind::Solid)
                    .map(|p| (p.id, cadrs_core::parts::display_name(p, props).to_string()))
                    .collect();
                studios.push(BrowserStudio {
                    id: el.id,
                    name: el.name.clone(),
                    parts,
                });
            }
            ElementKind::Assembly => assemblies.push((el.id, el.name.clone())),
            _ => {}
        }
    }
    let document = doc.doc.name.clone();
    // A sheet of an assembly opens the Assemblies tab with it picked (Create Drawing of an
    // assembly, D1.2).
    let sheet_asm = expand.filter(|e| assemblies.iter().any(|(a, _)| a == e));
    let mut expanded = HashSet::new();
    // The sheet's studio open; else (none, or a version's copy not listed here, P3G.1) the first.
    match expand.filter(|e| studios.iter().any(|s| s.id == *e)) {
        Some(e) => {
            expanded.insert(e);
        }
        None => {
            if let Some(s) = studios.first() {
                expanded.insert(s.id);
            }
        }
    }
    let empty_sheet = sheet.is_none_or(|s| s.views.is_empty());
    *world.resource_mut::<InsertViewState>() = InsertViewState {
        scale,
        studios,
        assemblies,
        document,
        expanded,
        tab: if sheet_asm.is_some() { 1 } else { 0 },
        reference: sheet_asm.filter(|_| empty_sheet).map(|e| ObjectRef { element: e.0, part: None }),
        flats,
        ..default()
    };
    let mut ui = world.resource_mut::<DrawingUi>();
    ui.tool = ViewTool::Insert;
    ui.ghost = None;
    ui.selected.clear();
}

/// Where the dialog and the browser go.
#[derive(Component)]
struct InsertDialogRoot;

#[derive(Component)]
struct BrowserStudioRow(ElementId);

/// The browser's list of studios and parts.
#[derive(Component)]
struct BrowserTree;

fn reference_label(state: &InsertViewState) -> Option<(String, bool)> {
    let r = state.reference?;
    if state.flat {
        return super::flat_views::flat_label(state, &r).map(|n| (n, true));
    }
    if let Some((_, n)) = state.assemblies.iter().find(|(e, _)| e.0 == r.element) {
        return Some((n.clone(), true));
    }
    let s = state.studios.iter().find(|s| s.id.0 == r.element)?;
    match r.part {
        Some((f, index)) => {
            let id = PartId {
                feature: cadrs_core::FeatureId(f),
                index,
            };
            s.parts.iter().find(|(p, _)| *p == id).map(|(_, n)| (n.clone(), true))
        }
        None => Some((s.name.clone(), false)),
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_insert_dialog(
    ui: Res<DrawingUi>,
    kind: Res<ActiveKind>,
    mut state: ResMut<InsertViewState>,
    theme: Res<Theme>,
    q_root: Query<Entity, With<InsertDialogRoot>>,
    q_tree: Query<Entity, With<BrowserTree>>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_named: Query<(Entity, &Name, &ChildOf)>,
    mut commands: Commands,
    mut last: Local<Option<InsertViewState>>,
) {
    let open = ui.tool == ViewTool::Insert && *kind == ActiveKind::Drawing;
    if !open {
        for e in &q_root {
            commands.entity(e).despawn();
        }
        *last = None;
        return;
    }
    if last.as_ref() == Some(&*state) && !q_root.is_empty() {
        return;
    }
    // A new search only rebuilds the list, so the search field keeps its focus and caret.
    if let Some(prev) = last.as_ref()
        && !q_root.is_empty()
        && (InsertViewState { search: state.search.clone(), ..prev.clone() }) == *state
        && let Ok(tree) = q_tree.single()
    {
        *last = Some(state.clone());
        let t = theme.clone();
        let st = state.clone();
        commands.entity(tree).despawn_children();
        commands.entity(tree).with_children(|tr| tree_rows(tr, &t, &st));
        return;
    }
    // P3G.1: the same for the Other documents search: only the documents below the field.
    if let Some(prev) = last.as_ref()
        && !q_root.is_empty()
        && (InsertViewState { browse: state.browse.clone(), other_list: state.other_list.clone(), other_thumbs: state.other_thumbs.clone(), ..prev.clone() }) == *state
        && (crate::linked::Browse { search: state.browse.search.clone(), ..prev.browse.clone() }) == state.browse
        && let Some((list, _, parent)) = q_named.iter().find(|(_, n, _)| n.as_str() == "insert-view-other-list")
    {
        *last = Some(state.clone());
        let t = theme.clone();
        let st = state.clone();
        commands.entity(list).despawn();
        commands.entity(parent.parent()).with_children(|p| crate::linked::spawn_browser_list(p, &t, crate::linked::Owner::InsertView, &st.browse, &st.other_list, &st.other_thumbs));
        return;
    }
    state.built = true;
    *last = Some(state.clone());
    for e in &q_root {
        commands.entity(e).despawn();
    }
    let Ok(area) = q_area.single() else {
        return;
    };
    let t = theme.clone();
    let st = state.clone();
    let left = if ui.sheets_open { super::panels::SHEETS_WIDTH } else { 0.0 };
    commands.entity(area).with_children(|vp| {
        vp.spawn((
            Name::new("insert-view-root"),
            InsertDialogRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(0.0),
                column_gap: Val::Px(0.0),
                align_items: AlignItems::FlexStart,
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(cadrs_ui::z::DIALOG - 12),
            DespawnOnExit(AppState::Document),
        ))
        .with_children(|root| {
            insert_card(root, &t, &st);
            if st.browser_open {
                browser(root, &t, &st);
            }
        });
    });
}

fn card_node() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(2.0)),
        margin: UiRect::new(Val::Px(4.0), Val::ZERO, Val::Px(4.0), Val::ZERO),
        ..default()
    }
}

fn card_shadow() -> BoxShadow {
    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.35), Val::Px(1.0), Val::Px(2.0), Val::Px(0.0), Val::Px(6.0))
}

fn insert_card(p: &mut ChildSpawnerCommands, t: &Theme, st: &InsertViewState) {
    p.spawn((
        Name::new("insert-view-dialog"),
        Node {
            width: Val::Px(204.0),
            ..card_node()
        },
        BackgroundColor(t.background),
        BorderColor::all(Color::srgb_u8(0xe0, 0xe0, 0xe0)),
        card_shadow(),
    ))
    .with_children(|c| {
        // Header: title and the red ✕.
        c.spawn((
            Node {
                height: Val::Px(26.0),
                padding: UiRect::left(Val::Px(6.0)),
                align_items: AlignItems::Center,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(t.separator),
        ))
        .with_children(|h| {
            h.spawn((
                t.text("Insert view", t.font_base, FontWeight::BOLD, t.foreground),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                Pickable::IGNORE,
            ));
            h.spawn((
                cadrs_ui::Button::new("insert-view-cancel")
                    .icon("x-bold")
                    .icon_size(14.0)
                    .ghost()
                    .tooltip("Close (Esc)")
                    .build(t),
                observe(|_: On<Activate>, mut commands: Commands| {
                    commands.queue(end_tool);
                }),
            ))
            .insert(TextColor(t.cancel));
        });
        c.spawn(Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::new(Val::Px(6.0), Val::Px(6.0), Val::Px(4.0), Val::Px(6.0)),
            row_gap: Val::Px(2.0),
            ..default()
        })
        .with_children(|b| {
            // The reference: its name (a part or studio icon), or a prompt; opens the browser.
            // P3G.1: a version's part names it ("Part 1 (V1)").
            let label = reference_label(st).map(|(n, ok)| match st.reference.and_then(|r| super::view_linked::version_suffix(st, &r)) {
                Some(v) => (format!("{n} ({v})"), ok),
                None => (n, ok),
            });
            let is_asm = st.reference.is_some_and(|r| st.assemblies.iter().any(|(e, _)| e.0 == r.element));
            let (text, color, icon_name) = match &label {
                Some((n, true)) if st.flat => (n.clone(), t.foreground, "flat-pattern"),
                Some((n, true)) if is_asm => (n.clone(), t.foreground, "assembly"),
                Some((n, true)) => (n.clone(), t.foreground, "part"),
                Some((n, false)) => (n.clone(), t.foreground, "part-studio"),
                None => ("Select a part or assembly".to_string(), t.muted_foreground, "part"),
            };
            b.spawn((
                Name::new("insert-view-reference"),
                Node {
                    height: Val::Px(26.0),
                    padding: UiRect::horizontal(Val::Px(4.0)),
                    column_gap: Val::Px(6.0),
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(if label.is_some() { Color::srgb_u8(0xc8, 0xc8, 0xc8) } else { t.primary }),
                BackgroundColor(if label.is_some() { t.background } else { Color::srgb_u8(0xee, 0xf4, 0xfb) }),
                Interaction::default(),
                observe(|_: On<Pointer<Click>>, mut s: ResMut<InsertViewState>| {
                    s.browser_open = !s.browser_open;
                }),
            ))
            .with_children(|r| {
                r.spawn((icon(icon_name, 14.0, t.tool_foreground), Pickable::IGNORE));
                r.spawn((
                    t.text(text, t.font_sm, FontWeight::NORMAL, color),
                    Node {
                        flex_grow: 1.0,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    Pickable::IGNORE,
                )).insert(TextLayout::no_wrap());
                if matches!(label, Some((_, false))) {
                    r.spawn((
                        Name::new("insert-view-studio-warning"),
                        icon("warning-filled", 14.0, Color::srgb_u8(0xe0, 0x9a, 0x10)),
                        Tooltip::new("A Part Studio is referenced, not a part"),
                    ));
                }
            });
            let mut o = Select::new("insert-view-orientation").width(Val::Percent(100.0));
            for n in NamedView::ALL {
                o = o.option(n.label(), true);
            }
            let oi = NamedView::ALL.iter().position(|n| *n == st.orientation).unwrap_or(0);
            b.spawn(o.selected(oi).build(t));
            let list = scales(st.scale);
            let mut sc = Select::new("insert-view-scale").width(Val::Percent(100.0));
            for s in &list {
                sc = sc.option(s.label(), true);
            }
            let si = list.iter().position(|s| *s == st.scale).unwrap_or(0);
            b.spawn(sc.selected(si).build(t));
            // The display state row is for assemblies' named states (P3B.8): a part has none, so
            // it isn't shown (`ex1-step4.png`). The named position / exploded view stays a stub.
            b.spawn((
                Select::new("insert-view-named-position")
                    .width(Val::Percent(100.0))
                    .option("None", true)
                    .option("Named positions and exploded views (assemblies)", false)
                    .build(t),
                Tooltip::new("Named positions and exploded views come with assemblies"),
            ));
            if matches!(label, Some((_, false))) {
                b.spawn((
                    Name::new("insert-view-warning"),
                    Node {
                        margin: UiRect::top(Val::Px(4.0)),
                        padding: UiRect::all(Val::Px(4.0)),
                        column_gap: Val::Px(4.0),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(0xfd, 0xf3, 0xdd)),
                ))
                .with_children(|w| {
                    w.spawn((icon("warning-filled", 14.0, Color::srgb_u8(0xd9, 0x8c, 0x00)), Pickable::IGNORE));
                    w.spawn((
                        t.text(
                            "A whole Part Studio is referenced. Its properties differ from a part's, so the title block and notes won't show a part's name. Expand the studio and pick the part.",
                            t.font_sm,
                            FontWeight::NORMAL,
                            t.foreground,
                        ),
                        Node {
                            max_width: Val::Px(170.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    )).insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
                });
            }
            if st.reference.is_some() {
                b.spawn((
                    t.text("Click on the sheet to place the view.", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                    Node {
                        margin: UiRect::top(Val::Px(4.0)),
                        max_width: Val::Px(188.0),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
            }
        });
    });
}

fn browser(p: &mut ChildSpawnerCommands, t: &Theme, st: &InsertViewState) {
    p.spawn((
        Name::new("insert-view-browser"),
        Node {
            width: Val::Px(262.0),
            height: Val::Px(420.0),
            ..card_node()
        },
        BackgroundColor(t.background),
        BorderColor::all(Color::srgb_u8(0xe0, 0xe0, 0xe0)),
        card_shadow(),
    ))
    .with_children(|c| {
        c.spawn((
            Node {
                height: Val::Px(28.0),
                flex_shrink: 0.0,
                padding: UiRect::left(Val::Px(8.0)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgb_u8(0xf3, 0xf5, 0xf8)),
        ))
        .with_children(|h| {
            h.spawn((
                t.text("Select a part or assembly", t.font_base, FontWeight::SEMIBOLD, t.foreground),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                Pickable::IGNORE,
            ));
            h.spawn((
                cadrs_ui::Button::new("insert-view-browser-close")
                    .icon("close")
                    .icon_size(14.0)
                    .ghost()
                    .tooltip("Close")
                    .build(t),
                observe(|_: On<Activate>, mut s: ResMut<InsertViewState>| {
                    s.browser_open = false;
                }),
            ));
        });
        c.spawn(
            // The two tabs share the browser's width (`ex1-step4.png`).
            TabStrip::new("insert-view-source-tabs")
                .tab("Current document")
                .tab("Other documents")
                .selected(st.source_tab)
                .compact()
                .build(t),
        );
        // P3G.1: the document and its branch or version, the Other documents browser, or the
        // version graph (see [`super::view_linked`]).
        if !super::view_linked::source_section(c, t, st) {
            return;
        }
        c.spawn(
            TabStrip::new("insert-view-kind-tabs")
                .tab("Part Studios")
                .tab("Assemblies")
                .selected(st.tab)
                .build(t),
        );
        c.spawn(Node {
            padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(6.0), Val::Px(2.0)),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|s| {
            s.spawn(
                TextInput::new("insert-view-search")
                    .placeholder(if st.tab == 0 { "Search parts or sketches" } else { "Search assemblies" })
                    .value(st.search.clone())
                    .width(Val::Percent(100.0))
                    .build(t),
            );
        });
        // Type filters: parts (on) and sketches.
        c.spawn(Node {
            padding: UiRect::new(Val::Px(6.0), Val::Px(6.0), Val::Px(2.0), Val::Px(2.0)),
            column_gap: Val::Px(2.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|f| {
            f.spawn((
                ToolButton::new("insert-view-filter-parts", "part")
                    .icon_size(16.0)
                    .selected(!st.filter_flat)
                    .tooltip("Parts")
                    .build(t),
                observe(|_: On<Activate>, mut s: ResMut<InsertViewState>| {
                    s.filter_flat = false;
                }),
            ));
            f.spawn(
                ToolButton::new("insert-view-filter-sketches", "sketch")
                    .icon_size(16.0)
                    .disabled(true)
                    .tooltip("Sketches: views of sketches come later")
                    .build(t),
            );
            // P3I.7 (SM16.2): sheet metal flat patterns.
            super::flat_views::flat_filter_button(f, t, st);
        });
        c.spawn((
            Name::new("insert-view-tree"),
            BrowserTree,
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(t.separator),
        ))
        .with_children(|tree| tree_rows(tree, t, st));
    });
}

/// The browser's rows: the studios (expanded to their parts) or the assemblies, filtered by the
/// search.
fn tree_rows(tree: &mut ChildSpawnerCommands, t: &Theme, st: &InsertViewState) {
    let q = st.search.trim().to_lowercase();
    if st.tab == 0 && st.filter_flat {
        super::flat_views::flat_rows(tree, t, st, &q);
        return;
    }
    if st.tab == 1 {
        if st.assemblies.is_empty() {
            tree.spawn((
                t.text("No assemblies in this document", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                Node {
                    margin: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
            ));
        }
        for (i, (id, a)) in st.assemblies.iter().enumerate() {
            if !q.is_empty() && !a.to_lowercase().contains(&q) {
                continue;
            }
            let r = ObjectRef { element: id.0, part: None };
            tree.spawn((
                TreeItem::new(format!("browser-assembly-{}", i + 1), a.clone())
                    .icon("assembly", 22.0)
                    .icon_color(t.tool_foreground)
                    .selected(st.reference == Some(r))
                    .left(10.0)
                    .height(34.0)
                    .build(t),
                Tooltip::new(a.clone()),
                observe(move |_: On<Activate>, mut state: ResMut<InsertViewState>| {
                    state.reference = Some(r);
                    state.flat = false;
                }),
            ));
        }
        return;
    }
    for (si, s) in st.studios.iter().enumerate() {
        let parts: Vec<&(PartId, String)> = s
            .parts
            .iter()
            .filter(|(_, n)| q.is_empty() || n.to_lowercase().contains(&q) || s.name.to_lowercase().contains(&q))
            .collect();
        if !q.is_empty() && parts.is_empty() && !s.name.to_lowercase().contains(&q) {
            continue;
        }
        let open = st.expanded.contains(&s.id) || !q.is_empty();
        let studio_selected = st.reference.is_some_and(|r| r.element == s.id.0 && r.part.is_none());
        let sid = s.id;
        tree.spawn((
            TreeItem::new(format!("browser-studio-{}", si + 1), s.name.clone())
                .disclosure(Some(open))
                .icon("part-studio", 22.0)
                .icon_color(t.tool_foreground)
                .selected(studio_selected)
                .left(4.0)
                .height(34.0)
                .build(t),
            BrowserStudioRow(sid),
            observe(move |_: On<Activate>, mut state: ResMut<InsertViewState>| {
                state.reference = Some(ObjectRef {
                    element: sid.0,
                    part: None,
                });
                state.flat = false;
            }),
        ));
        if !open {
            continue;
        }
        for (pi, (pid, name)) in parts.into_iter().enumerate() {
            let r = ObjectRef {
                element: s.id.0,
                part: Some((pid.feature.0, pid.index)),
            };
            tree.spawn((
                TreeItem::new(format!("browser-part-{}-{}", si + 1, pi + 1), name.clone())
                    .icon("part", 20.0)
                    .icon_color(t.tool_foreground)
                    .selected(st.reference == Some(r))
                    .left(40.0)
                    .height(32.0)
                    .build(t),
                observe(move |_: On<Activate>, mut state: ResMut<InsertViewState>| {
                    state.reference = Some(r);
                    state.flat = false;
                }),
            ));
        }
    }
}

fn on_browser_toggle(ev: On<TreeToggle>, q: Query<&BrowserStudioRow>, mut state: ResMut<InsertViewState>) {
    if let Ok(row) = q.get(ev.entity)
        && !state.expanded.remove(&row.0)
    {
        state.expanded.insert(row.0);
    }
}

fn on_browser_tab(ev: On<TabStripSelect>, q: Query<&Name>, mut state: ResMut<InsertViewState>) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == "insert-view-kind-tabs") && state.tab != ev.index {
        state.tab = ev.index;
        state.search.clear();
    }
}

fn on_insert_select(ev: On<SelectChange>, q: Query<&Name>, mut state: ResMut<InsertViewState>) {
    let Ok(name) = q.get(ev.entity) else {
        return;
    };
    match name.as_str() {
        "insert-view-orientation" => {
            if let Some(n) = NamedView::ALL.get(ev.index) {
                state.orientation = *n;
            }
        }
        "insert-view-scale" => {
            if let Some(s) = scales(state.scale).get(ev.index) {
                state.scale = *s;
            }
        }
        _ => {}
    }
}

fn read_browser_search(q: Query<(&Name, &bevy::text::EditableText)>, mut state: ResMut<InsertViewState>) {
    if let Some((_, t)) = q.iter().find(|(n, _)| n.as_str() == "insert-view-search-field") {
        let v = t.value().to_string();
        if state.search != v {
            state.search = v;
        }
    }
    // P3G.1: the Other documents search.
    if let Some((_, t)) = q.iter().find(|(n, _)| n.as_str() == "insert-view-other-search-field") {
        let v = t.value().to_string();
        if state.browse.search != v {
            state.browse.search = v;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The pointer

pub(super) fn pointer_over_sheet(hover: &HoverMap, q_area: &Query<Entity, With<ViewportArea>>) -> bool {
    let Some(hits) = hover.get(&PointerId::Mouse) else {
        return false;
    };
    q_area.iter().any(|e| hits.contains_key(&e))
}

/// Sheet millimetres per logical pixel now.
fn mm_per_px(doc: &ActiveDocument, ui: &DrawingUi) -> f64 {
    current_view(doc, ui).map(|(_, v)| 1.0 / v.ppm as f64).unwrap_or(1.0)
}

#[allow(clippy::too_many_arguments)]
fn view_pointer(
    mut inputs: MessageReader<PointerInput>,
    kind: Res<ActiveKind>,
    doc: Option<Res<ActiveDocument>>,
    rect: Res<ViewportRect>,
    hover: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    keys: Res<ButtonInput<KeyCode>>,
    cache: Res<ViewCache>,
    mut ui: ResMut<DrawingUi>,
    mut commands: Commands,
) {
    if *kind != ActiveKind::Drawing {
        inputs.clear();
        return;
    }
    let Some(doc) = doc else {
        inputs.clear();
        return;
    };
    let Some((((element, _), view), (_, d))) = current_view(&doc, &ui).zip(active_drawing(&doc)) else {
        inputs.clear();
        return;
    };
    let index = ui.sheet_index(element, d);
    let area = sheet_area(&rect, &ui);
    let over = pointer_over_sheet(&hover, &q_area) && q_menus.is_empty();
    let pad = 6.0 * mm_per_px(&doc, &ui);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        let p = screen_to_sheet(view, area, pos);
        match input.action {
            PointerAction::Move { .. } => {
                ui.pointer = Some(p);
                if let Some(drag) = &mut ui.view_drag {
                    let delta = p - drag.start;
                    if !drag.moving && delta.length() as f64 > 3.0 * pad / 6.0 {
                        drag.moving = true;
                    }
                    if drag.moving {
                        let id = drag.view;
                        ui.drag_preview = d.drag_view(id, [delta.x as f64, delta.y as f64]);
                    }
                }
            }
            PointerAction::Press(PointerButton::Primary) if over && !ui.annotation_press => {
                ui.pointer = Some(p);
                ui.press = Some(p);
                if ui.tool == ViewTool::None {
                    let hit = view_at(d, index, &cache, p, pad);
                    match hit {
                        Some(v) => {
                            if ctrl {
                                if let Some(i) = ui.selected.iter().position(|s| *s == v) {
                                    ui.selected.remove(i);
                                } else {
                                    ui.selected.push(v);
                                }
                            } else if !ui.selected.contains(&v) {
                                ui.selected = vec![v];
                            }
                            ui.view_drag = Some(super::ViewDrag {
                                view: v,
                                start: p,
                                moving: false,
                            });
                        }
                        None => {
                            if !ctrl {
                                ui.selected.clear();
                            }
                        }
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) => {
                let drag = ui.view_drag.take();
                let preview = std::mem::take(&mut ui.drag_preview);
                if let Some(drag) = drag
                    && drag.moving
                    && !preview.is_empty()
                {
                    commands.queue(move |w: &mut World| {
                        exec(w, DrawingOp::MoveViews { moves: preview });
                    });
                    continue;
                }
                let pressed_here = ui.press.take().is_some_and(|q| (q - p).length() as f64 <= pad);
                if over && pressed_here && ui.tool != ViewTool::None {
                    commands.queue(move |w: &mut World| tool_click(w, p));
                }
            }
            PointerAction::Cancel => {
                ui.view_drag = None;
                ui.drag_preview.clear();
            }
            _ => {}
        }
    }
    // Hover: the view under the pointer (not while dragging; kept while a menu is open, so
    // the view whose menu it is stays highlighted).
    if !q_menus.is_empty() {
        return;
    }
    let hovered = match (ui.pointer, over && !ui.annotating && !ui.annotation_hover, ui.view_drag.is_some()) {
        (Some(p), true, false) => match ui.tool {
            ViewTool::None | ViewTool::Projected { .. } | ViewTool::Auxiliary { from: None } => view_at(d, index, &cache, p, pad),
            t if t.picks_view() => view_at(d, index, &cache, p, pad),
            ViewTool::Align { view, .. } => Some(view),
            _ => None,
        },
        _ => None,
    };
    if ui.hovered != hovered {
        ui.hovered = hovered;
    }
    // The edge an auxiliary view or an alignment would use.
    let highlight = match (ui.tool, ui.pointer, over) {
        (ViewTool::Auxiliary { from: None }, Some(p), true) | (ViewTool::Align { .. }, Some(p), true) => {
            let target = match ui.tool {
                ViewTool::Align { view, .. } => Some(view),
                _ => view_at(d, index, &cache, p, pad),
            };
            target.and_then(|id| {
                let v = d.sheets[index].view(id)?;
                let g = cache.geometry(v)?;
                edge_at(v, &g, p, 2.0 * pad).map(|(e, _)| (id, e))
            })
        }
        _ => None,
    };
    if ui.highlight_edge != highlight {
        ui.highlight_edge = highlight;
    }
}

/// The centre of a view's projection in its 2D frame, if known.
fn center2d(cache: &ViewCache, v: &View) -> Option<[f64; 2]> {
    let g = cache.geometry(v)?;
    let (lo, hi) = g.bounds.or_else(|| g.projection.bounds().map(|(a, b)| ([a.x, a.y], [b.x, b.y])))?;
    Some([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0])
}

/// A view's centre on the sheet.
pub fn sheet_center(cache: &ViewCache, v: &View) -> [f64; 2] {
    let g = cache.geometry(v);
    let (lo, hi) = sheet_bounds(v, g.as_deref());
    [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]
}

/// Moves an aligned view along its fold line so its centre (not its anchor) is at the cursor's
/// place along the line.
fn center_on_line(v: &mut View, n: [f64; 2], c: Option<[f64; 2]>) {
    if let Some(c) = c {
        let off = rotate([c[0] * v.scale.factor(), c[1] * v.scale.factor()], v.rotation);
        let t = off[0] * n[0] + off[1] * n[1];
        v.anchor = [v.anchor[0] - n[0] * t, v.anchor[1] - n[1] * t];
    }
}

/// The ghost of the view the tool would place now.
fn update_ghost(
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    insert: Res<InsertViewState>,
    mut cache: ResMut<ViewCache>,
    mut ui: ResMut<DrawingUi>,
) {
    // The view-kind tools make their own ghost (`view_kind_tools::preview`).
    if ui.tool.is_view_kind() {
        return;
    }
    let ghost = (|| {
        let doc = doc.as_deref().filter(|_| *kind == ActiveKind::Drawing)?;
        let (id, d) = active_drawing(doc)?;
        let index = ui.sheet_index(id, d);
        let p = ui.pointer?;
        let cursor = [p.x as f64, p.y as f64];
        let ghost_id = ui.ghost_id;
        match ui.tool {
            ViewTool::Insert => {
                let r = insert.reference?;
                // P3I.7: a flat pattern view only from the Flat patterns filter (SM16.2).
                let mut v = if insert.flat {
                    View::flat_pattern(r, insert.orientation, insert.scale, cursor)
                } else {
                    View::base(r, insert.orientation, insert.scale, cursor)
                };
                v.id = ghost_id;
                v.hidden_lines = d.style.hidden_lines;
                v.tangent_edges = d.style.tangent_edges;
                // P3G.1: a linked source's copy, not in the document until the view is placed.
                match super::view_linked::ghost_doc(&doc.doc, &insert) {
                    Some(with_links) => cache.ensure(&with_links, Some(d), &v),
                    None => cache.ensure(&doc.doc, Some(d), &v),
                };
                if let Some(c) = center2d(&cache, &v) {
                    let k = v.scale.factor();
                    v.anchor = [cursor[0] - c[0] * k, cursor[1] - c[1] * k];
                }
                Some(v)
            }
            ViewTool::Projected { parent: Some(pid) } => {
                let parent = d.sheets.get(index)?.view(pid)?;
                // Over the parent (or another view): no ghost.
                if ui.hovered.is_some() {
                    return None;
                }
                let pc = sheet_center(&cache, parent);
                let placement = placement_for([cursor[0] - pc[0], cursor[1] - pc[1]])?;
                let mut v = projected_view(parent, placement, d.projection, cursor, None);
                v.id = ghost_id;
                v.scale = d.effective_scale(pid).unwrap_or(v.scale);
                cache.ensure(&doc.doc, Some(d), &v);
                let c = center2d(&cache, &v);
                match placement {
                    Placement::Ortho(n) => center_on_line(&mut v, rotate(n, parent.rotation), c),
                    Placement::Iso(..) => {
                        if let Some(c) = c {
                            let off = rotate([c[0] * v.scale.factor(), c[1] * v.scale.factor()], v.rotation);
                            v.anchor = [cursor[0] - off[0], cursor[1] - off[1]];
                        }
                    }
                }
                Some(v)
            }
            ViewTool::Auxiliary { from: Some((pid, edge)) } => {
                let parent = d.sheets.get(index)?.view(pid)?;
                if ui.hovered == Some(pid) {
                    return None;
                }
                let mut v = auxiliary_view(parent, edge, d.projection, cursor);
                v.id = ghost_id;
                v.scale = d.effective_scale(pid).unwrap_or(v.scale);
                cache.ensure(&doc.doc, Some(d), &v);
                let c = center2d(&cache, &v);
                if let Some(n) = v.fold {
                    center_on_line(&mut v, rotate(n, parent.rotation), c);
                }
                Some(v)
            }
            _ => None,
        }
    })();
    if ui.ghost != ghost {
        ui.ghost = ghost;
    }
}

/// A click with a view tool active.
fn tool_click(world: &mut World, p: Vec2) {
    if super::view_kind_tools::click(world, p, false) {
        return;
    }
    let tool = world.resource::<DrawingUi>().tool;
    let Some((element, index, d)) = world.get_resource::<ActiveDocument>().and_then(|doc| {
        let (id, d) = active_drawing(doc)?;
        let ui = world.resource::<DrawingUi>();
        Some((id, ui.sheet_index(id, d), d.clone()))
    }) else {
        return;
    };
    let sheet_id = d.sheets[index].id;
    let pad = {
        let doc = world.resource::<ActiveDocument>();
        6.0 * mm_per_px(doc, world.resource::<DrawingUi>())
    };
    let hit = view_at(&d, index, world.resource::<ViewCache>(), p, pad);
    let ghost = world.resource::<DrawingUi>().ghost.clone();
    match tool {
        ViewTool::Insert => {
            if let Some(mut v) = ghost {
                v.id = ViewId::new();
                // P3G.1 (D13.1): a view of a version (or of another document) stores the frozen
                // copy it shows with it, in one undo step.
                let links = super::view_linked::pending_links(world, v.reference.element);
                let placed = if links.is_empty() {
                    let op = insert_view_op(world, sheet_id, v.clone());
                    exec(world, op)
                } else {
                    let doc = world.resource::<ActiveDocument>();
                    let with = crate::linked::with_links(&doc.doc, &links);
                    let op = super::view_linked::insert_view_op_in(&with, &d, sheet_id, v.clone());
                    let cmd = cadrs_core::external::WithLinks { links, command: EditDrawing { element, op } };
                    match world.resource_mut::<ActiveDocument>().execute(&cmd) {
                        Ok(()) => true,
                        Err(e) => {
                            warn!("drawing edit refused: {e}");
                            false
                        }
                    }
                };
                if placed {
                    let mut ui = world.resource_mut::<DrawingUi>();
                    ui.tool = ViewTool::Projected { parent: Some(v.id) };
                    ui.selected.clear();
                    ui.ghost = None;
                    ui.ghost_id = ViewId::new();
                }
            }
        }
        ViewTool::Projected { parent } => match (hit, ghost) {
            (Some(h), _) if Some(h) != parent || parent.is_none() => {
                let mut ui = world.resource_mut::<DrawingUi>();
                ui.tool = ViewTool::Projected { parent: Some(h) };
                ui.selected = vec![h];
            }
            (_, Some(mut v)) => {
                v.id = ViewId::new();
                let _ = element;
                let op = insert_view_op(world, sheet_id, v);
                if exec(world, op) {
                    let mut ui = world.resource_mut::<DrawingUi>();
                    ui.ghost = None;
                    ui.ghost_id = ViewId::new();
                }
            }
            _ => {}
        },
        ViewTool::Auxiliary { from: None } => {
            let Some(h) = hit else { return };
            let cache = world.resource::<ViewCache>();
            let Some(v) = d.sheets[index].view(h) else { return };
            let Some(g) = cache.geometry(v) else { return };
            if let Some((_, dir)) = edge_at(v, &g, p, 2.0 * pad) {
                let mut ui = world.resource_mut::<DrawingUi>();
                ui.tool = ViewTool::Auxiliary { from: Some((h, dir)) };
                ui.selected.clear();
                ui.highlight_edge = None;
            }
        }
        ViewTool::Auxiliary { from: Some(_) } => {
            if let Some(mut v) = ghost {
                v.id = ViewId::new();
                let op = insert_view_op(world, sheet_id, v);
                if exec(world, op) {
                    let mut ui = world.resource_mut::<DrawingUi>();
                    ui.tool = ViewTool::None;
                    ui.ghost = None;
                    ui.ghost_id = ViewId::new();
                    ui.selected.clear();
                }
            }
        }
        ViewTool::Align { view, vertical } => {
            let cache = world.resource::<ViewCache>();
            let Some(v) = d.sheets[index].view(view).cloned() else { return };
            let Some(g) = cache.geometry(&v) else { return };
            let Some((_, dir)) = edge_at(&v, &g, p, 2.0 * pad) else { return };
            let mut nv = v.clone();
            nv.rotation = align_rotation(v.rotation, dir, vertical);
            if nv.fold.is_some() {
                nv.align_suppressed = true;
            }
            let label = if vertical { "Align view vertical" } else { "Align view horizontal" };
            exec(world, DrawingOp::SetView { view: nv, label: label.into() });
            end_tool(world);
            world.resource_mut::<DrawingUi>().selected.clear();
        }
        _ => {}
    }
}

/// The rotation that turns an edge with 2D direction `dir` (in the view's frame) vertical or
/// horizontal on the sheet, by the smallest turn from `rotation`.
pub fn align_rotation(rotation: f64, dir: [f64; 2], vertical: bool) -> f64 {
    use std::f64::consts::{FRAC_PI_2, PI};
    let on_sheet = dir[1].atan2(dir[0]) + rotation;
    let target = if vertical { FRAC_PI_2 } else { 0.0 };
    // Lines: angles modulo π.
    let mut turn = (target - on_sheet).rem_euclid(PI);
    if turn > FRAC_PI_2 {
        turn -= PI;
    }
    let r = rotation + turn;
    // Keep it in (−π, π].
    let r = (r + PI).rem_euclid(2.0 * PI) - PI;
    if r.abs() < 1e-12 { 0.0 } else { r }
}

/// Esc ends the tool (or clears the selection); Delete removes the selected views.
#[allow(clippy::too_many_arguments)]
fn view_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    kind: Res<ActiveKind>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<TextInputField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    mut ui: ResMut<DrawingUi>,
    mut commands: Commands,
) {
    if *kind != ActiveKind::Drawing {
        keys_in.clear();
        return;
    }
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    for k in keys_in.read() {
        if k.state != bevy::input::ButtonState::Pressed || typing || !q_dialogs.is_empty() || !q_menus.is_empty() {
            continue;
        }
        match k.key_code {
            KeyCode::Enter | KeyCode::NumpadEnter if matches!(ui.tool, ViewTool::Boundary { view: Some(_), .. }) => {
                commands.queue(|w: &mut World| {
                    super::view_kind_tools::enter(w);
                });
            }
            KeyCode::Escape => {
                ui.tool_points.clear();
                ui.tool_strokes.clear();
                if ui.tool != ViewTool::None {
                    // Ending Projected view drops the parent's highlight too.
                    if matches!(ui.tool, ViewTool::Projected { .. } | ViewTool::Auxiliary { .. }) {
                        ui.selected.clear();
                    }
                    ui.tool = ViewTool::None;
                    ui.ghost = None;
                    ui.highlight_edge = None;
                } else {
                    ui.selected.clear();
                }
            }
            KeyCode::Delete | KeyCode::Backspace if ui.tool == ViewTool::None && !ui.selected.is_empty() => {
                let ids = std::mem::take(&mut ui.selected);
                commands.queue(move |w: &mut World| {
                    exec(w, DrawingOp::DeleteViews { ids });
                });
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The hint

#[derive(Component)]
struct ToolHint;

#[allow(clippy::too_many_arguments)]
fn sync_tool_hint(
    ui: Res<DrawingUi>,
    kind: Res<ActiveKind>,
    insert: Res<InsertViewState>,
    theme: Res<Theme>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q: Query<(Entity, &Children), With<ToolHint>>,
    mut q_text: Query<&mut Text>,
    mut commands: Commands,
) {
    let text = if *kind != ActiveKind::Drawing {
        None
    } else {
        match ui.tool {
            ViewTool::None => None,
            ViewTool::Insert if insert.reference.is_none() => Some("Select a part or assembly to insert".to_string()),
            ViewTool::Insert => Some("Click on the sheet to place the view".to_string()),
            ViewTool::Projected { parent: None } => Some("Projected view: click a view to project from".to_string()),
            ViewTool::Projected { parent: Some(_) } => Some(
                "Projected view: move away from the view and click to place · Esc to finish".to_string(),
            ),
            ViewTool::Auxiliary { from: None } => Some("Auxiliary view: click a straight edge of a view".to_string()),
            ViewTool::Auxiliary { from: Some(_) } => Some("Auxiliary view: click to place the view".to_string()),
            ViewTool::Align { vertical: true, .. } => Some("Align view vertical: click an edge of the view".to_string()),
            ViewTool::Align { vertical: false, .. } => Some("Align view horizontal: click an edge of the view".to_string()),
            t => super::view_kind_tools::hint(t).map(str::to_string),
        }
    };
    match (text, q.single_mut()) {
        (None, Ok((e, _))) => commands.entity(e).despawn(),
        (Some(t), Ok((_, children))) => {
            if let Some(&c) = children.first()
                && let Ok(mut tx) = q_text.get_mut(c)
                && tx.0 != t
            {
                tx.0 = t;
            }
        }
        (Some(t), Err(_)) => {
            let Ok(area) = q_area.single() else {
                return;
            };
            let th = theme.clone();
            commands.entity(area).with_children(|vp| {
                vp.spawn((
                    Name::new("drawing-tool-hint"),
                    ToolHint,
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(14.0),
                        left: Val::Percent(50.0),
                        margin: UiRect::left(Val::Px(-200.0)),
                        width: Val::Px(400.0),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    Pickable::IGNORE,
                    ZIndex(3),
                    DespawnOnExit(AppState::Document),
                ))
                .with_children(|h| {
                    h.spawn((
                        th.text(t, th.font_sm, FontWeight::MEDIUM, Color::WHITE),
                        Node {
                            padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.12, 0.12, 0.12, 0.82)),
                        Pickable::IGNORE,
                    ));
                });
            });
        }
        (None, Err(_)) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align_turns_by_the_smallest_angle() {
        use std::f64::consts::FRAC_PI_2;
        // A horizontal edge made vertical: a quarter turn.
        assert!((align_rotation(0.0, [1.0, 0.0], true) - FRAC_PI_2).abs() < 1e-12
            || (align_rotation(0.0, [1.0, 0.0], true) + FRAC_PI_2).abs() < 1e-12);
        // An edge at 30° made horizontal: −30°.
        let a = 30f64.to_radians();
        assert!((align_rotation(0.0, [a.cos(), a.sin()], false) + a).abs() < 1e-12);
        // Already vertical: no turn.
        assert_eq!(align_rotation(0.0, [0.0, 1.0], true), 0.0);
    }
}
