//! The **Sheet metal table and flat view** panel (P3I.3; `simultaneous-sheet-metal.md` SM1.2–
//! SM1.4, SM13.1–SM13.5; `help/feature-tools/sheetmetalflatpatterntable-02.png`, lesson
//! `13-bend-and-joint-table`):
//!
//! - **The toggle**: the right strip's *Sheet metal table and flat view* button
//!   (`panel-sheet-metal`, icon `sheet-metal-table`), shown once a Part Studio has a sheet
//!   metal model; it opens and closes the panel, docked on the viewport's right.
//! - **Sheet metal context** (`smt-context`): one entry per Sheet metal model feature, by its
//!   name (renaming the feature renames it).
//! - **Bends** (`smt-bends`): #, Name, Radius, Angle (deg), Bend direction and the model's
//!   calculation (K Factor, Bend allowance or Bend deduction); **Other joints** (`smt-joints`):
//!   Name, Type, Style. Each title has a caret that collapses its table.
//! - **The flat view** (`smt-flat`): the flat pattern as a thin solid, drawn by a camera of its
//!   own into an image (as the Repair panel's), with the outline, dashed bend centrelines, the
//!   bends' labels and a view cube of its own; right-drag orbits, middle-drag pans, the wheel
//!   zooms; right-click for Zoom to fit.
//! - **Cross-highlighting**: a row's joint is its faces in the folded model (a bend's
//!   cylinders and ends, a rip's two side faces). Clicking a row selects them (click again:
//!   deselects; rows add up); selecting them in the model selects the row (and scrolls to it);
//!   clicking a bend or rip in the flat view does the same. Selected joints are orange in the
//!   flat view; the hovered row or flat joint shows hovered in the model.
//! - **Edits** (each a Modify joint, one undo step; `cadrs_core::sheetmetal_joint`):
//!   double-click a Radius or the calculation cell to type a value (an out-of-range value
//!   leaves the cell red with its range as the tooltip); the row's menu: **Move up**, **Move
//!   down**, **Convert <bend> to rip** / **Convert <joint> to bend**; a rip's Type and Style
//!   selects. Hems can be reordered, not edited.
//!
//! Names: `smt-panel`, `smt-context`, `smt-bends-caret`, `smt-joints-caret`, rows
//! `smt-bend-row-<i>` / `smt-joint-row-<i>` (from 0), cells `smt-bend-<i>-radius`,
//! `smt-bend-<i>-value`, selects `smt-joint-<i>-type`, `smt-joint-<i>-style`, the flat view
//! `smt-flat`, its cube `smt-flat-cube`, labels `smt-flat-label-<name>`; menu items
//! `smt-move-up`, `smt-move-down`, `smt-convert`.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::text::FontWeight;
use bevy::ui::UiGlobalTransform;
use bevy::ui_widgets::Activate;
use cadrs_core::sheetmetal::SheetMetalContext;
use cadrs_core::sheetmetal_joint::{ModifyJointFeature, PutModifyJoint, SetTableOrder, TableEdit, modify_joint_of, table_edit};
use cadrs_core::{ElementId, FeatureId, FeatureKind, PartId, Solid};
use cadrs_sheetmetal::model::P3;
use cadrs_sheetmetal::poly::P2;
use cadrs_sheetmetal::table::{self, Table};
use cadrs_sheetmetal::view::{FlatScene, Tris};
use cadrs_sheetmetal::{BendCalc, JointId, JointKind, RipStyle};
use cadrs_sketch::units::Quantity;
use cadrs_ui::inline_edit::{DoubleClick, DoubleClickable, InlineEdit, InlineEditCommit, InlineEditLabel, InlineEditOptions, begin_inline_edit};
use cadrs_ui::menu::{ContextMenuAnchor, ContextMenuRequested, ContextMenuTarget, Menu, MenuAction, MenuItem};
use cadrs_ui::prelude::*;
use cadrs_ui::{Column, Select, SelectChange, TableBody, TableHeader, TableRoot, TableRow, open_context_menu};

use crate::appearance::SidePanel;
use crate::camera::{StandardView, ViewState};
use crate::parts::PartCache;
use crate::viewport::{ActiveKind, HoverOverride, Pick, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

/// The render layer of the flat view, and of its cube's rotate arrows.
pub const FLAT_LAYER: usize = 9;
pub const FLAT_CUBE_ARC_LAYER: usize = 10;
/// The panel's width (px): about Onshape's share of the window.
pub const PANEL_W: f32 = 640.0;
/// Table rows' and headers' height (px).
const ROW_H: f32 = 30.0;
/// The selection orange (Onshape's highlight).
const ORANGE: Color = Color::srgb(0.98, 0.62, 0.25);
const ORANGE_HOVER: Color = Color::srgb(1.0, 0.80, 0.55);

pub struct SheetMetalTablePlugin;

impl Plugin for SheetMetalTablePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SmTable>()
            .init_gizmo_group::<FlatLineGizmos>()
            .init_gizmo_group::<FlatHighlightGizmos>()
            .init_gizmo_group::<FlatCubeArcGizmos>()
            .add_systems(Startup, (configure_gizmos, spawn_cameras))
            .add_observer(on_activate)
            .add_observer(on_context_menu)
            .add_observer(on_menu_action)
            .add_observer(on_double_click)
            .add_observer(on_commit)
            .add_observer(on_select_change)
            .add_systems(
                Update,
                (
                    strip_button,
                    sync_context,
                    sync_panel,
                    sync_rows,
                    hover_to_view,
                    resize_target,
                    fit_on_open,
                    sync_cameras,
                    sync_meshes,
                    draw_flat,
                    draw_cube_arcs,
                    place_labels,
                )
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut t: ResMut<SmTable>| {
                let (image, cube) = (t.image.clone(), t.cube_image.clone());
                *t = SmTable { image, cube_image: cube, ..SmTable::default() };
            });
    }
}

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct FlatLineGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct FlatHighlightGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct FlatCubeArcGizmos;

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    use bevy::gizmos::config::GizmoLineJoint;
    let (c, _) = store.config_mut::<FlatLineGizmos>();
    c.line.width = 1.4;
    c.line.joints = GizmoLineJoint::Round(4);
    c.depth_bias = -1.0;
    c.render_layers = RenderLayers::layer(FLAT_LAYER);
    let (c, _) = store.config_mut::<FlatHighlightGizmos>();
    c.line.width = 4.0;
    c.line.joints = GizmoLineJoint::Round(4);
    c.depth_bias = -1.0;
    c.render_layers = RenderLayers::layer(FLAT_LAYER);
    let (c, _) = store.config_mut::<FlatCubeArcGizmos>();
    c.render_layers = RenderLayers::layer(FLAT_CUBE_ARC_LAYER);
    c.line.width = 12.0;
}

/// The panel's state (view state: not saved, not undone).
#[derive(Resource)]
pub struct SmTable {
    /// The Sheet metal model shown (the first one if unset or gone).
    pub context: Option<FeatureId>,
    pub bends_open: bool,
    pub joints_open: bool,
    /// The flat view's camera.
    pub view: ViewState,
    /// The joint under the pointer: a row's or the flat view's.
    pub hovered: Option<JointId>,
    pub flat_hover: Option<JointId>,
    /// The flat view as last laid out, and what it was laid out from.
    pub scene: Option<Arc<FlatScene>>,
    scene_of: Option<(FeatureId, u64)>,
    /// What the panel was built from (rebuilt only when it changes).
    key: Option<String>,
    /// The selected joints as last seen (a new one scrolls its row into view).
    seen: Vec<JointId>,
    /// The hover this panel put on the main view.
    hover_set: Option<Pick>,
    image: Option<Handle<Image>>,
    image_size: UVec2,
    scale: f32,
    cube_image: Option<Handle<Image>>,
    meshed: Option<u64>,
    /// Where the secondary button went down over the flat view.
    press: Option<Vec2>,
    /// Frames until the flat view is fitted (the layout settles first).
    fit_in: Option<u32>,
}

impl Default for SmTable {
    fn default() -> Self {
        Self {
            context: None,
            bends_open: true,
            joints_open: true,
            view: ViewState::standard(StandardView::Top),
            hovered: None,
            flat_hover: None,
            scene: None,
            scene_of: None,
            key: None,
            seen: Vec::new(),
            hover_set: None,
            image: None,
            image_size: UVec2::ZERO,
            scale: 1.0,
            cube_image: None,
            meshed: None,
            press: None,
            fit_in: None,
        }
    }
}

#[derive(Component)]
struct FlatCamera;

#[derive(Component)]
struct FlatCubeCamera;

#[derive(Component)]
struct PanelRoot;

#[derive(Component)]
struct FlatBody;

#[derive(Component)]
struct FlatImage;

#[derive(Component)]
struct FlatCubeImage;

#[derive(Component)]
struct FlatMesh;

#[derive(Component)]
struct RowsScroll;

/// A row: its joint, and whether it is in the Bends table.
#[derive(Component, Clone, Copy)]
struct RowRef {
    joint: JointId,
    bend: bool,
    index: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CellKind {
    Radius,
    Value,
}

#[derive(Component, Clone, Copy)]
struct CellRef {
    joint: JointId,
    kind: CellKind,
}

/// A rip's Type or Style select.
#[derive(Component, Clone, Copy)]
enum JointSelect {
    Type(JointId),
    Style(JointId),
}

/// A label in the flat view: its joint and its spot (flat 2D, on the sheet's top).
#[derive(Component, Clone, Copy)]
struct FlatLabel(JointId, P2);

/// The joint a row menu is for.
#[derive(Component, Clone, Copy)]
struct MenuFor(JointId, bool);

#[derive(Component)]
struct FlatViewMenu;

fn target_image(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x.max(1), size.y.max(1), TextureFormat::Rgba8UnormSrgb, None);
    image.asset_usage = RenderAssetUsages::default();
    image
}

fn spawn_cameras(mut commands: Commands, mut images: ResMut<Assets<Image>>, theme: Res<Theme>, mut t: ResMut<SmTable>) {
    let image = images.add(target_image(UVec2::new(64, 64)));
    t.image = Some(image.clone());
    t.image_size = UVec2::new(64, 64);
    commands.spawn((
        Name::new("smt-flat-camera"),
        FlatCamera,
        Camera3d::default(),
        RenderTarget::Image(image.into()),
        Camera { order: -2, is_active: false, clear_color: ClearColorConfig::Custom(theme.viewport_background), ..default() },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::WindowSize,
            scale: 1.0,
            near: 0.0,
            far: crate::camera::CAMERA_FAR,
            ..OrthographicProjection::default_3d()
        }),
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::layer(FLAT_LAYER),
        Transform::default(),
    ));
    let size = (crate::view_cube::CUBE_WIDGET * crate::view_cube::CUBE_SUPERSAMPLE).as_uvec2();
    let cube = images.add(target_image(size));
    t.cube_image = Some(cube.clone());
    commands.spawn((
        Name::new("smt-flat-cube-camera"),
        FlatCubeCamera,
        Camera3d::default(),
        RenderTarget::Image(cube.into()),
        Camera { order: -1, is_active: false, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
        crate::view_cube::cube_projection(),
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::from_layers(&[crate::view_cube::CUBE_LAYER, FLAT_CUBE_ARC_LAYER]),
        Transform::default(),
    ));
}

// ---------------------------------------------------------------------------------------------
// What is shown

fn is_open(open: &SidePanel, kind: &ActiveKind) -> bool {
    *open == SidePanel::SheetMetal && *kind == ActiveKind::PartStudio
}

/// The sheet metal contexts of the active Part Studio, in feature-list order: (the feature,
/// its name). A Sheet metal model or Loft is named by its feature (a rename shows at once); a
/// context a Derived feature brought in (P3I.8, SM18) by the source model's name, where the
/// Derived feature is.
fn models(doc: &ActiveDocument, cache: &PartCache) -> Vec<(FeatureId, String)> {
    let Some(el) = doc.active_element() else { return Vec::new() };
    let features = el.features();
    let mut out: Vec<(usize, FeatureId, String)> = cache
        .sheet_metal
        .iter()
        .map(|c| match features.iter().position(|f| f.id == c.feature) {
            Some(i) => (i, c.feature, features[i].name.clone()),
            None => {
                let at = c.editors.iter().find_map(|e| features.iter().position(|f| f.id == *e)).unwrap_or(usize::MAX);
                (at, c.feature, c.name.clone())
            }
        })
        .collect();
    out.sort_by_key(|x| x.0);
    out.into_iter().map(|(_, f, n)| (f, n)).collect()
}

/// The context shown.
fn shown<'a>(t: &SmTable, cache: &'a PartCache) -> Option<&'a SheetMetalContext> {
    t.context.and_then(|f| cache.sheet_metal.iter().find(|c| c.feature == f)).or(cache.sheet_metal.first())
}

/// A point on a face of a solid (its first triangle's centre).
fn face_point(s: &Solid, i: usize) -> Option<P3> {
    let f = s.faces.get(i)?;
    if f.triangle_count == 0 {
        return None;
    }
    let k = 3 * f.first_triangle;
    let mut c = [0.0; 3];
    for j in 0..3 {
        let p = s.positions.get(*s.indices.get(k + j)? as usize)?;
        for a in 0..3 {
            c[a] += p[a] / 3.0;
        }
    }
    Some(P3::new(c[0], c[1], c[2]))
}

fn edge_point(s: &Solid, i: usize) -> Option<P3> {
    let e = s.edges.get(i)?;
    let p = e.points.get(e.points.len() / 2)?;
    if e.points.len() >= 2 && e.points.len() % 2 == 0 {
        let q = e.points[e.points.len() / 2 - 1];
        return Some(P3::new((p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0, (p[2] + q[2]) / 2.0));
    }
    Some(P3::new(p[0], p[1], p[2]))
}

/// How close a picked point must be to a joint (mm): the tessellation's sag on a bend.
fn tolerance(ctx: &SheetMetalContext) -> f64 {
    (0.1 * ctx.model.params.thickness).max(0.05)
}

/// The joint (and its model) a viewport pick is on: a bend's faces or edges, a rip's side
/// faces or their edges.
pub fn joint_of_pick(cache: &PartCache, pick: Pick) -> Option<(FeatureId, JointId)> {
    let part = pick.part()?;
    let ctx = cache.sheet_metal.iter().find(|c| c.parts.iter().any(|(p, _)| *p == part))?;
    let solid = &cache.parts.iter().find(|p| p.id == part)?.solid;
    let p = match pick {
        Pick::Face(_, name) => face_point(solid, solid.faces.iter().position(|f| f.name == name)?)?,
        Pick::Edge(_, name) => edge_point(solid, solid.edges.iter().position(|e| e.name == name)?)?,
        _ => return None,
    };
    cadrs_sheetmetal::view::joint_at(&ctx.model, p, tolerance(ctx)).map(|j| (ctx.feature, j))
}

/// A joint's faces in the folded model (what selecting its row selects).
pub fn joint_picks(cache: &PartCache, ctx: &SheetMetalContext, joint: JointId) -> Vec<Pick> {
    let tol = tolerance(ctx);
    let mut out = Vec::new();
    for (pid, _) in &ctx.parts {
        let Some(part) = cache.parts.iter().find(|p| p.id == *pid) else { continue };
        for (i, f) in part.solid.faces.iter().enumerate() {
            if face_point(&part.solid, i).is_some_and(|p| cadrs_sheetmetal::view::joint_at(&ctx.model, p, tol) == Some(joint)) {
                out.push(Pick::Face(*pid, f.name));
            }
        }
    }
    out
}

/// The shown model's joints selected in the view.
fn selected_joints(cache: &PartCache, ctx: &SheetMetalContext, selection: &Selection) -> Vec<JointId> {
    let mut out = Vec::new();
    for p in &selection.0 {
        if let Some((m, j)) = joint_of_pick(cache, *p)
            && m == ctx.feature
            && !out.contains(&j)
        {
            out.push(j);
        }
    }
    out
}

/// The Modify joint of a joint, if it has one.
fn modify_joint(doc: &ActiveDocument, model: FeatureId, joint: JointId) -> Option<(FeatureId, ModifyJointFeature)> {
    let el = doc.active_element()?;
    let f = modify_joint_of(el.features(), model, joint)?;
    match &f.kind {
        FeatureKind::ModifyJoint(x) => Some((f.id, x.clone())),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// The strip button and the panel

/// The strip's button shows in a Part Studio with a sheet metal model; the panel closes when
/// there is none.
fn strip_button(kind: Res<ActiveKind>, cache: Res<PartCache>, mut open: ResMut<SidePanel>, mut q: Query<(&Name, &mut Node)>) {
    let has = *kind == ActiveKind::PartStudio && !cache.sheet_metal.is_empty();
    for (n, mut node) in &mut q {
        if n.as_str() == "panel-sheet-metal" {
            let want = if has { Display::Flex } else { Display::None };
            if node.display != want {
                node.display = want;
            }
        }
    }
    if !has && *open == SidePanel::SheetMetal && !cache.rebuilding {
        *open = SidePanel::None;
    }
}

/// The context follows the models: the first one by default, and the shown one if the
/// selection is on another model's joint.
fn sync_context(mut t: ResMut<SmTable>, cache: Res<PartCache>, selection: Res<Selection>, open: Res<SidePanel>, kind: Res<ActiveKind>) {
    if !is_open(&open, &kind) {
        return;
    }
    let valid = t.context.is_some_and(|f| cache.sheet_metal.iter().any(|c| c.feature == f));
    if !valid {
        let first = cache.sheet_metal.first().map(|c| c.feature);
        if t.context != first {
            t.context = first;
        }
    }
    if selection.is_changed()
        && let Some(last) = selection.0.last()
        && let Some((m, _)) = joint_of_pick(&cache, *last)
        && t.context != Some(m)
    {
        t.context = Some(m);
    }
}

fn units(world: &World) -> cadrs_sketch::units::Units {
    world.get_resource::<crate::WorkspaceUnits>().map(|u| u.0).unwrap_or_default()
}

/// The panel, rebuilt when what it shows changes.
fn sync_panel(world: &mut World) {
    let open = is_open(world.resource::<SidePanel>(), world.resource::<ActiveKind>());
    let mut q_panel = world.query_filtered::<Entity, With<PanelRoot>>();
    let old: Vec<Entity> = q_panel.iter(world).collect();
    if !open {
        for e in old {
            world.entity_mut(e).despawn();
        }
        if world.resource::<SmTable>().key.is_some() {
            world.resource_mut::<SmTable>().key = None;
        }
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let cache = world.resource::<PartCache>();
    let t = world.resource::<SmTable>();
    let list = models(doc, cache);
    let ctx = shown(t, cache).cloned();
    let units = units(world);
    let rows = ctx.as_ref().map(|c| table::table(&c.model));
    let mjs: Vec<(JointId, Option<String>, Option<String>)> = ctx
        .as_ref()
        .map(|c| {
            c.model
                .joints
                .iter()
                .filter_map(|j| modify_joint(doc, c.feature, j.id).map(|(_, x)| (j.id, x.range_error().map(|e| e.message()), Some(x.value_expr.clone()))))
                .collect()
        })
        .unwrap_or_default();
    let key = format!("{list:?}|{:?}|{rows:?}|{mjs:?}|{}|{}|{:?}", ctx.as_ref().map(|c| c.feature), t.bends_open, t.joints_open, units);
    if t.key.as_deref() == Some(key.as_str()) && !old.is_empty() {
        return;
    }
    let (bends_open, joints_open) = (t.bends_open, t.joints_open);
    let first_open = old.is_empty();
    // Keep the rows' scroll.
    let mut q_scroll = world.query_filtered::<&ScrollPosition, With<RowsScroll>>();
    let scroll = q_scroll.iter(world).next().cloned().unwrap_or_default();
    for e in old {
        world.entity_mut(e).despawn();
    }
    {
        let mut t = world.resource_mut::<SmTable>();
        t.key = Some(key);
        if first_open {
            t.fit_in = Some(3);
        }
    }
    let mut q_area = world.query_filtered::<(Entity, &ChildOf), With<ViewportArea>>();
    let Some((area, parent)) = q_area.iter(world).next().map(|(e, c)| (e, c.parent())) else { return };
    let theme = world.resource::<Theme>().clone();
    let t = world.resource::<SmTable>();
    let (image, cube) = (t.image.clone().unwrap_or_default(), t.cube_image.clone().unwrap_or_default());
    let shown_feature = ctx.as_ref().map(|c| c.feature);
    let model_name = list.iter().find(|(f, _)| Some(*f) == shown_feature).map(|(_, n)| n.clone()).unwrap_or_default();
    let mut commands = world.commands();
    let panel = commands
        .spawn((
            Name::new("smt-panel"),
            PanelRoot,
            DespawnOnExit(AppState::Document),
            crate::appearance::side_panel_node(&theme),
        ))
        .id();
    commands.entity(panel).entry::<Node>().and_modify(|mut n| n.width = Val::Px(PANEL_W));
    let th = theme.clone();
    commands.entity(panel).with_children(|p| {
        // Sheet metal context: <model> ▾
        p.spawn(Node {
            height: Val::Px(40.0),
            flex_shrink: 0.0,
            padding: UiRect::horizontal(Val::Px(12.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(10.0),
            ..default()
        })
        .with_children(|r| {
            r.spawn(th.text("Sheet metal context:", 12.0, FontWeight::NORMAL, th.foreground));
            let mut s = Select::new("smt-context").bordered().width(Val::Px(230.0));
            for (_, n) in &list {
                s = s.option(n.clone(), true);
            }
            let sel = list.iter().position(|(f, _)| Some(*f) == shown_feature).unwrap_or(0);
            r.spawn(s.selected(sel).build(&th));
        });
        // The tables, scrolling together above the flat view.
        p.spawn((
            Name::new("smt-tables"),
            RowsScroll,
            TableBody,
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_height: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(th.panel_border),
            scroll,
        ))
        .with_children(|b| {
            if let (Some(ctx), Some(rows)) = (&ctx, &rows) {
                spawn_tables(b, &th, ctx, rows, &mjs, &units, bends_open, joints_open);
            } else {
                b.spawn(th.text("No sheet metal model", 12.0, FontWeight::NORMAL, th.muted_foreground));
            }
        });
        // The flat view.
        p.spawn((
            Name::new("smt-flat"),
            FlatBody,
            ContextMenuTarget,
            Hovered::default(),
            Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_height: Val::Px(120.0),
                border: UiRect::top(Val::Px(1.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(th.panel_border),
        ))
        .observe(on_flat_press)
        .observe(on_flat_drag)
        .observe(on_flat_scroll)
        .observe(on_flat_click)
        .observe(on_flat_move)
        .observe(|_: On<Pointer<Out>>, mut t: ResMut<SmTable>| t.flat_hover = None)
        .with_children(|b| {
            b.spawn((
                Name::new("smt-flat-image"),
                FlatImage,
                ImageNode::new(image),
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                Pickable::IGNORE,
            ));
            b.spawn((
                Name::new("smt-flat-cube"),
                FlatCubeImage,
                ImageNode::new(cube),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(crate::view_cube::CUBE_WIDGET.x),
                    height: Val::Px(crate::view_cube::CUBE_WIDGET.y),
                    ..default()
                },
            ))
            .observe(on_cube_click);
            if let Some(ctx) = &ctx {
                let scene = FlatScene::new(&ctx.model, &ctx.flat);
                for j in ctx.model.joints.iter().filter(|j| j.bend().is_some()) {
                    let Some(spot) = scene.label_spot(j.id) else { continue };
                    b.spawn((
                        Name::new(format!("smt-flat-label-{}", j.name)),
                        FlatLabel(j.id, spot),
                        Node {
                            position_type: PositionType::Absolute,
                            padding: UiRect::axes(Val::Px(4.0), Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                        Visibility::Hidden,
                        Pickable::IGNORE,
                    ))
                    .with_child((th.text(j.name.clone(), 11.0, FontWeight::MEDIUM, th.foreground), Pickable::IGNORE));
                }
            }
        });
    });
    let _ = model_name;
    world.flush();
    let at = world.get::<Children>(parent).and_then(|c| c.iter().position(|e| e == area)).map_or(0, |i| i + 1);
    world.entity_mut(parent).insert_children(at, &[panel]);
}

/// A table title with its caret.
fn caret_header(b: &mut ChildSpawnerCommands, t: &Theme, name: &'static str, title: &str, open: bool) {
    b.spawn(cadrs_ui::Button::new(name).ghost().build(t))
    .insert(Node {
        height: Val::Px(32.0),
        flex_shrink: 0.0,
        padding: UiRect::horizontal(Val::Px(8.0)),
        align_items: AlignItems::Center,
        column_gap: Val::Px(6.0),
        justify_content: JustifyContent::Start,
        ..default()
    })
    .insert(BackgroundColor(Color::srgb_u8(0xf0, 0xf0, 0xf0)))
    .with_children(|h| {
        h.spawn((cadrs_ui::icon(if open { "chevron-down" } else { "chevron-right" }, 14.0, t.foreground), Pickable::IGNORE));
        h.spawn((t.text(title, 14.0, FontWeight::MEDIUM, t.foreground), Pickable::IGNORE));
    });
}

fn number(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" { "0".into() } else { s.into() }
}

/// The calculation column's title and quantity.
fn value_title(calc: BendCalc, units: &cadrs_sketch::units::Units) -> (String, Quantity) {
    match calc {
        BendCalc::KFactor => ("K Factor".into(), Quantity::Count),
        BendCalc::BendAllowance => (format!("Bend allowance({})", units.length.symbol()), Quantity::Length),
        BendCalc::BendDeduction => (format!("Bend deduction({})", units.length.symbol()), Quantity::Length),
    }
}

/// The angle between a rip's walls (degrees), for which styles it allows.
fn rip_angle(ctx: &SheetMetalContext, j: &cadrs_sheetmetal::Joint) -> Option<f64> {
    let (a, b) = (ctx.model.wall(j.a)?, ctx.model.wall(j.b)?);
    let (na, nb) = (a.surface.normal()?, b.surface.normal()?);
    Some(na.dot(&nb).clamp(-1.0, 1.0).acos().to_degrees())
}

#[allow(clippy::too_many_arguments)]
fn spawn_tables(
    b: &mut ChildSpawnerCommands,
    t: &Theme,
    ctx: &SheetMetalContext,
    rows: &Table,
    mjs: &[(JointId, Option<String>, Option<String>)],
    units: &cadrs_sketch::units::Units,
    bends_open: bool,
    joints_open: bool,
) {
    let calc = ctx.model.params.bend_calc;
    let (value_label, value_q) = value_title(calc, units);
    let len = units.length.symbol();
    let bend_cols = vec![
        Column::new("num", "#").width(44.0),
        Column::new("name", "Name").width(104.0),
        Column::new("radius", format!("Radius({len})")).width(104.0),
        Column::new("angle", "Angle(deg)").width(96.0),
        Column::new("dir", "Bend direction").width(112.0),
        Column::new("value", value_label).width(144.0),
    ];
    let joint_cols = vec![Column::new("name", "Name").width(120.0), Column::new("type", "Type").width(120.0), Column::new("style", "Style").width(170.0)];
    let bold = |text: String| t.text(text, 12.0, FontWeight::SEMIBOLD, t.foreground);
    caret_header(b, t, "smt-bends-caret", "Bends", bends_open);
    if bends_open {
        b.spawn((Name::new("smt-bends"), TableRoot, Node { flex_direction: FlexDirection::Column, flex_shrink: 0.0, ..default() })).with_children(|tbl| {
            tbl.spawn(TableHeader::new("smt-bends-header", bend_cols.clone()).height(ROW_H).build(t));
            for (i, r) in rows.bends.iter().enumerate() {
                let joint = r.joint;
                let mj = mjs.iter().find(|(j, ..)| *j == joint);
                let err = mj.and_then(|(_, e, _)| e.clone());
                let radius = units.value(r.radius, Quantity::Length);
                let value = match (&err, mj) {
                    // An out-of-range entry shows as typed.
                    (Some(_), Some((_, _, Some(expr)))) => expr.trim_end_matches(" mm").to_string(),
                    _ => match r.value {
                        Some(v) if value_q == Quantity::Length => units.value(v.value(), Quantity::Length),
                        Some(v) => number(v.value(), 3),
                        None => "–".into(),
                    },
                };
                let editable = r.editable;
                let cells: Vec<(String, Option<CellKind>)> = vec![
                    (r.number.to_string(), None),
                    (r.name.clone(), None),
                    (radius, editable.then_some(CellKind::Radius)),
                    (number(r.angle_deg, 2), None),
                    (r.direction.label().to_string(), None),
                    (value, editable.then_some(CellKind::Value)),
                ];
                let mut tr = TableRow::new(format!("smt-bend-row-{i}"), &bend_cols).height(ROW_H);
                for (c, (text, kind)) in cells.into_iter().enumerate() {
                    let th = t.clone();
                    let red = kind == Some(CellKind::Value) && err.is_some();
                    let tip = err.clone();
                    let bold_num = c == 0;
                    tr = tr.cell(move |cell| {
                        let name = match kind {
                            Some(CellKind::Radius) => format!("smt-bend-{i}-radius"),
                            Some(CellKind::Value) => format!("smt-bend-{i}-value"),
                            None => format!("smt-bend-{i}-{c}"),
                        };
                        let mut e = cell.spawn((
                            Name::new(name),
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                overflow: Overflow::clip(),
                                ..default()
                            },
                        ));
                        if let Some(k) = kind {
                            e.insert((CellRef { joint, kind: k }, DoubleClickable, InlineEdit::default()));
                        }
                        if red {
                            e.insert((BackgroundColor(Color::srgb_u8(0xf8, 0xd7, 0xd7)), BorderColor::all(th.feature_error)));
                            e.entry::<Node>().and_modify(|mut n| n.border = UiRect::all(Val::Px(1.0)));
                            if let Some(m) = tip {
                                e.insert(Tooltip::error(m));
                            }
                        }
                        let fg = if red { th.feature_error } else { th.foreground };
                        let weight = if bold_num { FontWeight::SEMIBOLD } else { FontWeight::NORMAL };
                        e.with_child((th.text(text, 12.0, weight, fg), InlineEditLabel, Pickable::IGNORE));
                    });
                }
                tbl.spawn((tr.build(t), RowRef { joint, bend: true, index: i }));
            }
        });
    }
    caret_header(b, t, "smt-joints-caret", "Other joints", joints_open);
    if joints_open {
        b.spawn((Name::new("smt-joints"), TableRoot, Node { flex_direction: FlexDirection::Column, flex_shrink: 0.0, ..default() })).with_children(|tbl| {
            tbl.spawn(TableHeader::new("smt-joints-header", joint_cols.clone()).height(ROW_H).build(t));
            for (i, r) in rows.joints.iter().enumerate() {
                let joint = r.joint;
                let tangent = r.kind == table::JointType::Tangent;
                let ninety = ctx.model.joint(joint).and_then(|j| rip_angle(ctx, j)).is_some_and(|a| (a - 90.0).abs() < 1e-3);
                let style = r.style.unwrap_or_default();
                let name = r.name.clone();
                let th = t.clone();
                let tr = TableRow::new(format!("smt-joint-row-{i}"), &joint_cols)
                    .height(ROW_H)
                    .cell({
                        let th = th.clone();
                        move |c| {
                            c.spawn(Node { width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() })
                                .with_child((th.text(name, 12.0, FontWeight::SEMIBOLD, th.foreground), Pickable::IGNORE));
                        }
                    })
                    .cell({
                        let th = th.clone();
                        move |c| {
                            let label = if tangent { "Tangent" } else { "Rip" };
                            let options = if tangent { vec![("Tangent".to_string(), true)] } else { vec![("Rip".to_string(), true), ("Bend".to_string(), true)] };
                            dropdown(c, &th, format!("smt-joint-{i}-type"), label, Dropdown { select: JointSelect::Type(joint), options, disabled: tangent });
                        }
                    })
                    .cell(move |c| {
                        if tangent {
                            return;
                        }
                        let options = vec![
                            (RipStyle::EdgeJoint.label().to_string(), true),
                            (RipStyle::ButtDirection1.label().to_string(), ninety),
                            (RipStyle::ButtDirection2.label().to_string(), ninety),
                        ];
                        dropdown(c, &th, format!("smt-joint-{i}-style"), style.label(), Dropdown { select: JointSelect::Style(joint), options, disabled: false });
                    });
                tbl.spawn((tr.build(t), RowRef { joint, bend: false, index: i }));
            }
        });
    }
    let _ = bold;
}

/// A select in a table cell (the Select widget's menu would be clipped by the cell): the value
/// with a caret, opening its options under it.
#[derive(Component, Clone)]
struct Dropdown {
    select: JointSelect,
    options: Vec<(String, bool)>,
    disabled: bool,
}

/// The dropdown a menu is open for, with its name (its items are `<name>-option-<i>`).
#[derive(Component, Clone)]
struct DropdownFor(JointSelect, String);

fn dropdown(c: &mut ChildSpawner, t: &Theme, name: String, label: &str, d: Dropdown) {
    let fg = if d.disabled { t.muted_foreground } else { t.foreground };
    c.spawn((
        Name::new(name),
        d,
        Hovered::default(),
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(24.0),
            margin: UiRect::horizontal(Val::Px(4.0)),
            padding: UiRect::horizontal(Val::Px(4.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            border: UiRect::bottom(Val::Px(1.0)),
            overflow: Overflow::clip(),
            ..default()
        },
        BorderColor::all(t.input_border),
    ))
    .observe(on_dropdown_click)
    .with_children(|r| {
        r.spawn((t.text(label, 12.0, FontWeight::NORMAL, fg), Pickable::IGNORE));
        r.spawn((cadrs_ui::icon("caret-down-filled", 10.0, t.tool_foreground), Pickable::IGNORE));
    });
}

fn on_dropdown_click(mut c: On<Pointer<Click>>, q: Query<(&Dropdown, &Name, &ComputedNode, &UiGlobalTransform)>, theme: Res<Theme>, mut commands: Commands) {
    // A click on a row's select isn't a click on the row.
    c.propagate(false);
    if c.button != PointerButton::Primary {
        return;
    }
    let Ok((d, name, node, tr)) = q.get(c.entity) else { return };
    if d.disabled {
        return;
    }
    let s = node.inverse_scale_factor();
    let at = Vec2::new(tr.translation.x - node.size().x / 2.0, tr.translation.y + node.size().y / 2.0) * s;
    let mut menu = Menu::new(format!("{name}-menu")).min_width((node.size().x * s).max(120.0)).item_height(24.0).text_only();
    for (i, (label, enabled)) in d.options.iter().enumerate() {
        menu = menu.item(MenuItem::new(format!("{name}-option-{i}"), label.clone()).disabled(!enabled));
    }
    let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
    commands.entity(anchor).insert((DropdownFor(d.select, name.to_string()), DespawnOnExit(AppState::Document)));
}

/// Rows show selected while their joint is (in any view).
#[allow(clippy::type_complexity)]
fn sync_rows(
    mut t: ResMut<SmTable>,
    cache: Res<PartCache>,
    selection: Res<Selection>,
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    q_rows: Query<(Entity, &RowRef, Has<cadrs_ui::style::Selected>, &Hovered, &ComputedNode)>,
    mut q_scroll: Query<(&mut ScrollPosition, &ComputedNode), With<RowsScroll>>,
    mut commands: Commands,
) {
    if !is_open(&open, &kind) {
        return;
    }
    let Some(ctx) = shown(&t, &cache) else { return };
    let selected = selected_joints(&cache, ctx, &selection);
    let mut scroll_to: Option<usize> = None;
    let new: Vec<JointId> = selected.iter().filter(|j| !t.seen.contains(j)).copied().collect();
    let mut row_y = 0.0;
    for (e, r, has, _, node) in &q_rows {
        let want = selected.contains(&r.joint);
        if want && !has {
            commands.entity(e).try_insert(cadrs_ui::style::Selected);
        } else if !want && has {
            commands.entity(e).try_remove::<cadrs_ui::style::Selected>();
        }
        row_y = node.size().y * node.inverse_scale_factor();
        if new.contains(&r.joint) {
            // Bends come first, then the Other joints table under them.
            let bends = q_rows.iter().filter(|(_, x, ..)| x.bend).count();
            scroll_to = Some(if r.bend { r.index } else { bends + 2 + r.index });
        }
    }
    let hovered = q_rows.iter().find(|(_, _, _, h, _)| h.get()).map(|(_, r, ..)| r.joint).or(t.flat_hover);
    if t.hovered != hovered {
        t.hovered = hovered;
    }
    if t.seen != selected {
        t.seen = selected;
    }
    if let Some(i) = scroll_to
        && let Ok((mut sp, node)) = q_scroll.single_mut()
    {
        let h = node.size().y * node.inverse_scale_factor();
        let y = (i as f32 + 2.0) * row_y.max(ROW_H);
        if y < sp.y + ROW_H * 2.0 || y > sp.y + h - ROW_H {
            sp.y = (y - h / 2.0).max(0.0);
        }
    }
}

/// The hovered row's or flat joint's faces show hovered in the model.
fn hover_to_view(mut t: ResMut<SmTable>, cache: Res<PartCache>, mut over: ResMut<HoverOverride>) {
    let want = t.hovered.and_then(|j| shown(&t, &cache).and_then(|ctx| joint_picks(&cache, ctx, j).first().copied()));
    if want == t.hover_set {
        return;
    }
    if over.0 == t.hover_set {
        over.0 = want;
    }
    t.hover_set = want;
}

// ---------------------------------------------------------------------------------------------
// Edits

/// A table edit of `joint` in the shown model: its Modify joint edited, or a new one (one step).
fn edit_joint(world: &mut World, joint: JointId, edit: TableEdit) {
    let Some(ctx) = shown(world.resource::<SmTable>(), world.resource::<PartCache>()).cloned() else { return };
    let Some(j) = ctx.model.joint(joint).cloned() else { return };
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = doc.active_element().map(|e| e.id) else { return };
    let existing = modify_joint(doc, ctx.feature, joint);
    let x = table_edit(ctx.feature, existing.as_ref().map(|(_, x)| x), &j, &ctx.model.params, &edit);
    if existing.as_ref().is_some_and(|(_, e)| *e == x) {
        return;
    }
    let feature = existing.map_or_else(FeatureId::new, |(f, _)| f);
    let label = edit.label(&j.name);
    run(world, element, &PutModifyJoint { element, feature, joint: x, after: ctx.editors.clone(), label });
}

fn run(world: &mut World, _element: ElementId, cmd: &dyn cadrs_core::Command) {
    if crate::linked_session::refuse(world) {
        return;
    }
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = doc.execute(cmd)
    {
        warn!("sheet metal table: {e}");
    }
}

fn move_joint(world: &mut World, joint: JointId, by: isize) {
    let Some(ctx) = shown(world.resource::<SmTable>(), world.resource::<PartCache>()).cloned() else { return };
    let Some(order) = cadrs_sheetmetal::joint_edit::moved(&ctx.model, joint, by) else { return };
    let Some(element) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()).map(|e| e.id) else { return };
    let name = ctx.model.joint(joint).map(|j| j.name.clone()).unwrap_or_default();
    let label = format!("{} {name}", if by < 0 { "Move up" } else { "Move down" });
    run(world, element, &SetTableOrder { element, model: ctx.feature, order, label });
}

/// Toggles a joint's faces in the selection (a row or the flat view clicked).
fn toggle_joint(world: &mut World, joint: JointId) {
    let picks = {
        let cache = world.resource::<PartCache>();
        let Some(ctx) = shown(world.resource::<SmTable>(), cache) else { return };
        joint_picks(cache, ctx, joint)
    };
    if picks.is_empty() {
        return;
    }
    let mut sel = world.resource_mut::<Selection>();
    if picks.iter().all(|p| sel.0.contains(p)) {
        sel.0.retain(|p| !picks.contains(p));
    } else {
        for p in picks {
            if !sel.0.contains(&p) {
                sel.0.push(p);
            }
        }
    }
}

fn on_activate(a: On<Activate>, q_row: Query<&RowRef>, q_name: Query<&Name>, mut commands: Commands) {
    if let Ok(r) = q_row.get(a.entity) {
        let j = r.joint;
        commands.queue(move |world: &mut World| toggle_joint(world, j));
        return;
    }
    let Ok(n) = q_name.get(a.entity) else { return };
    match n.as_str() {
        "smt-bends-caret" => commands.queue(|world: &mut World| {
            let mut t = world.resource_mut::<SmTable>();
            t.bends_open = !t.bends_open;
        }),
        "smt-joints-caret" => commands.queue(|world: &mut World| {
            let mut t = world.resource_mut::<SmTable>();
            t.joints_open = !t.joints_open;
        }),
        _ => {}
    }
}

fn on_select_change(ev: On<SelectChange>, q_name: Query<&Name>, mut commands: Commands) {
    if q_name.get(ev.entity).is_ok_and(|n| n.as_str() == "smt-context") {
        let i = ev.index;
        commands.queue(move |world: &mut World| {
            let list = match world.get_resource::<ActiveDocument>() {
                Some(d) => models(d, world.resource::<PartCache>()),
                None => return,
            };
            if let Some((f, _)) = list.get(i) {
                let mut t = world.resource_mut::<SmTable>();
                t.context = Some(*f);
                t.fit_in = Some(2);
            }
        });
    }
}

fn on_double_click(ev: On<DoubleClick>, q: Query<&CellRef>, mut commands: Commands) {
    let Ok(c) = q.get(ev.entity) else { return };
    let (c, entity) = (*c, ev.entity);
    commands.queue(move |world: &mut World| {
        let Some(ctx) = shown(world.resource::<SmTable>(), world.resource::<PartCache>()).cloned() else { return };
        let Some(b) = ctx.model.joint(c.joint).and_then(|j| j.bend().copied()) else { return };
        let units = units(world);
        let text = match c.kind {
            CellKind::Radius => units.value(b.radius, Quantity::Length),
            CellKind::Value => {
                let p = &ctx.model.params;
                let v = b.value_or_model(p).to_calc(p.bend_calc, b.radius, p.thickness, b.angle);
                match (v, p.bend_calc) {
                    (Some(v), BendCalc::KFactor) => number(v.value(), 4),
                    (Some(v), _) => units.value(v.value(), Quantity::Length),
                    (None, _) => String::new(),
                }
            }
        };
        let theme = world.resource::<Theme>().clone();
        let mut cm = world.commands();
        let mut o = InlineEditOptions::new(format!("smt-edit-{}", if c.kind == CellKind::Radius { "radius" } else { "value" }));
        o.height = 24.0;
        begin_inline_edit(&mut cm, &theme, entity, text, o);
        world.flush();
    });
}

fn on_commit(ev: On<InlineEditCommit>, q: Query<&CellRef>, mut commands: Commands) {
    let Ok(c) = q.get(ev.entity) else { return };
    let (c, text) = (*c, ev.value.clone());
    commands.queue(move |world: &mut World| {
        let Some(ctx) = shown(world.resource::<SmTable>(), world.resource::<PartCache>()).cloned() else { return };
        let units = units(world);
        let vars = world.get_resource::<crate::variables_ui::ActiveVariables>().cloned().unwrap_or_default();
        let q = match (c.kind, ctx.model.params.bend_calc) {
            (CellKind::Radius, _) | (CellKind::Value, BendCalc::BendAllowance | BendCalc::BendDeduction) => Quantity::Length,
            (CellKind::Value, BendCalc::KFactor) => Quantity::Count,
        };
        let Some((v, expr)) = crate::sheetmetal_ui::parse(&text, q, &units, &vars) else { return };
        let Some(b) = ctx.model.joint(c.joint).and_then(|j| j.bend().copied()) else { return };
        let edit = match c.kind {
            CellKind::Radius if (v - b.radius).abs() > 1e-12 => TableEdit::Radius(v, expr),
            CellKind::Value => TableEdit::Value(v, expr),
            _ => return,
        };
        edit_joint(world, c.joint, edit);
    });
}

fn on_context_menu(ev: On<ContextMenuRequested>, q_row: Query<&RowRef>, q_flat: Query<(), With<FlatBody>>, theme: Res<Theme>, mut t: ResMut<SmTable>, mut commands: Commands) {
    if q_flat.contains(ev.entity) {
        // The end of a right-drag (an orbit) isn't a right-click.
        if t.press.take().is_some_and(|p| p.distance(ev.position) > 3.0) {
            return;
        }
        // As Onshape's (SM14.1, SM15.1, SM16.1): the flat's own actions, then the view's.
        let menu = Menu::new("smt-flat-menu")
            .min_width(230.0)
            .item_height(22.0)
            .text_only()
            .item(MenuItem::new("smt-flat-drawing", "Create drawing of flat pattern"))
            .separator()
            .item(MenuItem::new("smt-zoom-fit", "Zoom to fit"));
        let anchor = open_context_menu(&mut commands, ev.position, menu.build(&theme));
        commands.entity(anchor).insert((FlatViewMenu, DespawnOnExit(AppState::Document)));
        return;
    }
    let Ok(r) = q_row.get(ev.entity) else { return };
    let (joint, bend, at) = (r.joint, r.bend, ev.position);
    commands.queue(move |world: &mut World| {
        let Some(ctx) = shown(world.resource::<SmTable>(), world.resource::<PartCache>()).cloned() else { return };
        let Some(j) = ctx.model.joint(joint) else { return };
        let up = cadrs_sheetmetal::joint_edit::moved(&ctx.model, joint, -1).is_some();
        let down = cadrs_sheetmetal::joint_edit::moved(&ctx.model, joint, 1).is_some();
        let hem = j.bend().is_some_and(|b| b.hem);
        let tangent = matches!(j.kind, JointKind::Tangent { .. });
        let convert = if bend { format!("Convert {} to rip", j.name) } else { format!("Convert {} to bend", j.name) };
        let menu = Menu::new("smt-row-menu")
            .min_width(180.0)
            .item_height(24.0)
            .text_only()
            .item(MenuItem::new("smt-move-up", "Move up").disabled(!up))
            .item(MenuItem::new("smt-move-down", "Move down").disabled(!down))
            .separator()
            .item(MenuItem::new("smt-convert", convert).disabled(hem || tangent));
        let theme = world.resource::<Theme>().clone();
        let mut cm = world.commands();
        let anchor = open_context_menu(&mut cm, at, menu.build(&theme));
        cm.entity(anchor).insert((MenuFor(joint, bend), DespawnOnExit(AppState::Document)));
        world.flush();
    });
}

fn on_menu_action(
    ev: On<MenuAction>,
    q: Query<&MenuFor, With<ContextMenuAnchor>>,
    q_drop: Query<&DropdownFor, With<ContextMenuAnchor>>,
    q_flat: Query<(), (With<FlatViewMenu>, With<ContextMenuAnchor>)>,
    mut commands: Commands,
) {
    if let Ok(DropdownFor(sel, name)) = q_drop.get(ev.entity) {
        let Some(i) = ev.item.strip_prefix(&format!("{name}-option-")).and_then(|n| n.parse::<usize>().ok()) else { return };
        let sel = *sel;
        commands.queue(move |world: &mut World| match sel {
            JointSelect::Type(j) if i == 1 => edit_joint(world, j, TableEdit::ConvertToBend),
            JointSelect::Style(j) => {
                if let Some(style) = RipStyle::ALL.get(i) {
                    edit_joint(world, j, TableEdit::RipStyle(*style));
                }
            }
            _ => {}
        });
        return;
    }
    if q_flat.contains(ev.entity) {
        match ev.item.as_str() {
            "smt-zoom-fit" => commands.queue(zoom_to_fit),
            "smt-flat-drawing" => commands.queue(create_flat_drawing),
            _ => {}
        }
        return;
    }
    let Ok(m) = q.get(ev.entity) else { return };
    let MenuFor(joint, bend) = *m;
    match ev.item.as_str() {
        "smt-move-up" => commands.queue(move |world: &mut World| move_joint(world, joint, -1)),
        "smt-move-down" => commands.queue(move |world: &mut World| move_joint(world, joint, 1)),
        "smt-convert" => commands.queue(move |world: &mut World| edit_joint(world, joint, if bend { TableEdit::ConvertToRip } else { TableEdit::ConvertToBend })),
        _ => {}
    }
}

/// The flat view's part: the shown model's first part (P3I.7's Create drawing of flat pattern).
fn flat_part(world: &World) -> Option<(cadrs_core::ElementId, PartId)> {
    let ctx = shown(world.resource::<SmTable>(), world.resource::<PartCache>())?;
    let part = ctx.parts.first()?.0;
    let el = world.get_resource::<ActiveDocument>()?.active?;
    Some((el, part))
}

/// Create drawing of flat pattern (SM16.1), from the flat view's menu.
fn create_flat_drawing(world: &mut World) {
    let Some((el, part)) = flat_part(world) else { return };
    crate::drawing::flat_views::open_create_drawing_of_flat(world, cadrs_drawing::ObjectRef { element: el.0, part: Some((part.feature.0, part.index)) });
}

// ---------------------------------------------------------------------------------------------
// The flat view

fn resize_target(
    mut t: ResMut<SmTable>,
    mut images: ResMut<Assets<Image>>,
    q_body: Query<&ComputedNode, With<FlatBody>>,
    mut q_image: Query<&mut ImageNode, With<FlatImage>>,
    mut q_cam: Query<&mut RenderTarget, With<FlatCamera>>,
) {
    let Some(node) = q_body.iter().next() else { return };
    let size = node.size().round().as_uvec2();
    if size.x < 4 || size.y < 4 {
        return;
    }
    let scale = 1.0 / node.inverse_scale_factor();
    if size == t.image_size && (scale - t.scale).abs() < 1e-4 {
        return;
    }
    let handle = images.add(target_image(size));
    t.image = Some(handle.clone());
    t.image_size = size;
    t.scale = scale;
    for mut img in &mut q_image {
        img.image = handle.clone();
    }
    for mut target in &mut q_cam {
        *target = RenderTarget::Image(ImageRenderTarget { handle: handle.clone(), scale_factor: scale });
    }
}

/// The flat view's size (logical px).
fn body_size(world: &mut World) -> Option<Vec2> {
    let mut q = world.query_filtered::<&ComputedNode, With<FlatBody>>();
    q.iter(world).next().map(|n| n.size() * n.inverse_scale_factor())
}

/// Zoom to fit: the flat pattern fills the flat view, seen from the top.
pub fn zoom_to_fit(world: &mut World) {
    let size = body_size(world);
    let mut t = world.resource_mut::<SmTable>();
    let Some(scene) = t.scene.clone() else { return };
    let Some((lo, hi)) = scene.bounds() else { return };
    let z = scene.thickness as f32;
    let pts = [Vec3::new(lo.x as f32, lo.y as f32, 0.0), Vec3::new(hi.x as f32, hi.y as f32, z)];
    if let Some(size) = size {
        // The cube sits in the top-right corner: keep clear of it.
        let room = Vec2::new((size.x - 40.0).max(1.0), size.y);
        t.view = t.view.fitted(&pts, room, 0.85);
    }
}

fn fit_on_open(world: &mut World) {
    let mut t = world.resource_mut::<SmTable>();
    let Some(n) = t.fit_in else { return };
    if n > 0 || t.scene.is_none() {
        t.fit_in = Some(n.saturating_sub(1));
        if t.scene.is_none() && n == 0 {
            t.fit_in = None;
        }
        return;
    }
    t.fit_in = None;
    t.view = t.view.oriented(StandardView::Top);
    zoom_to_fit(world);
}

#[allow(clippy::type_complexity)]
fn sync_cameras(
    t: Res<SmTable>,
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    mut q_cam: Query<(&mut Camera, &mut Transform, &mut Projection), (With<FlatCamera>, Without<FlatCubeCamera>)>,
    mut q_cube: Query<(&mut Camera, &mut Transform), (With<FlatCubeCamera>, Without<FlatCamera>)>,
) {
    let on = is_open(&open, &kind) && t.key.is_some();
    let v = t.view;
    for (mut cam, mut tr, mut projection) in &mut q_cam {
        if cam.is_active != on {
            cam.is_active = on;
        }
        let new_t = Transform::from_translation(v.camera_position()).with_rotation(v.rotation());
        if *tr != new_t {
            *tr = new_t;
        }
        if let Projection::Orthographic(o) = &mut *projection
            && o.scale != v.scale
        {
            o.scale = v.scale;
        }
    }
    for (mut cam, mut tr) in &mut q_cube {
        if cam.is_active != on {
            cam.is_active = on;
        }
        let new_t = Transform::from_translation(v.back() * 50.0).with_rotation(v.rotation());
        if *tr != new_t {
            *tr = new_t;
        }
    }
}

fn tris_mesh(t: &Tris, colors: Vec<[f32; 4]>) -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, t.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, t.normals.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(t.indices.clone()))
}

/// A face colour lit by a head light from the view.
fn shaded(base: [f32; 3], n: [f32; 3], back: Vec3) -> [f32; 4] {
    let l = (Vec3::from(n).dot(back)).abs();
    let k = 0.62 + 0.38 * l;
    [base[0] * k, base[1] * k, base[2] * k, 1.0]
}

/// The flat pattern's meshes: the sheet, the selected and hovered bends in orange, a
/// collision in red.
#[allow(clippy::too_many_arguments)]
fn sync_meshes(
    mut t: ResMut<SmTable>,
    cache: Res<PartCache>,
    selection: Res<Selection>,
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    q: Query<Entity, With<FlatMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let ctx = if is_open(&open, &kind) { shown(&t, &cache).cloned() } else { None };
    // The scene, laid out again when the model changes.
    let want_scene = ctx.as_ref().map(|c| {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}{:?}", c.model, c.flat).hash(&mut h);
        (c.feature, h.finish())
    });
    if want_scene != t.scene_of {
        t.scene = ctx.as_ref().map(|c| Arc::new(FlatScene::new(&c.model, &c.flat)));
        // A new flat (another model, or a joint changed): fitted again.
        if t.scene_of.is_some() && want_scene.is_some() {
            t.fit_in = Some(1);
        }
        t.scene_of = want_scene;
    }
    let selected = ctx.as_ref().map(|c| selected_joints(&cache, c, &selection)).unwrap_or_default();
    let v = t.view;
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        want_scene.hash(&mut h);
        selected.hash(&mut h);
        t.hovered.hash(&mut h);
        [(v.azimuth * 2.0).round() as i32, (v.elevation * 2.0).round() as i32, (v.roll * 2.0).round() as i32].hash(&mut h);
        h.finish()
    };
    let want = ctx.is_some().then_some(key);
    if want == t.meshed {
        return;
    }
    t.meshed = want;
    for e in &q {
        commands.entity(e).try_despawn();
    }
    let Some(scene) = t.scene.clone().filter(|_| ctx.is_some()) else { return };
    let back = v.back();
    let material = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() });
    let tz = scene.thickness;
    let mut spawn = |tris: Tris, base: [f32; 3]| {
        if tris.is_empty() {
            return;
        }
        let colors = tris.normals.iter().map(|n| shaded(base, *n, back)).collect();
        commands.spawn((Name::new("smt-flat-mesh"), FlatMesh, Mesh3d(meshes.add(tris_mesh(&tris, colors))), MeshMaterial3d(material.clone()), Transform::default(), RenderLayers::layer(FLAT_LAYER), DespawnOnExit(AppState::Document)));
    };
    let all: Vec<_> = scene.pieces.iter().flat_map(|(_, v)| v.iter().cloned()).collect();
    spawn(cadrs_sheetmetal::view::slab(&all, 0.0, tz), [0.66, 0.66, 0.68]);
    let lift = tz * 1.0 + 1e-3 * scene.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0);
    for b in &scene.bends {
        let c = if selected.contains(&b.joint) {
            ORANGE
        } else if t.hovered == Some(b.joint) {
            ORANGE_HOVER
        } else {
            continue;
        };
        let s = c.to_srgba();
        spawn(cadrs_sheetmetal::view::fill(&b.region, lift), [s.red, s.green, s.blue]);
    }
    if !scene.collisions.is_empty() {
        spawn(cadrs_sheetmetal::view::fill(&scene.collisions, lift), [0.9, 0.15, 0.15]);
    }
}

fn v3(p: P2, z: f64) -> Vec3 {
    Vec3::new(p.x as f32, p.y as f32, z as f32)
}

/// Outlines, dashed bend centrelines, tangent lines, and selected or hovered rips.
fn draw_flat(t: Res<SmTable>, cache: Res<PartCache>, selection: Res<Selection>, open: Res<SidePanel>, kind: Res<ActiveKind>, mut lines: Gizmos<FlatLineGizmos>, mut hi: Gizmos<FlatHighlightGizmos>) {
    if !is_open(&open, &kind) || t.key.is_none() {
        return;
    }
    let Some(scene) = &t.scene else { return };
    let Some(ctx) = shown(&t, &cache) else { return };
    let z = scene.thickness;
    let dark = Color::srgb_u8(0x22, 0x22, 0x22);
    for p in &scene.outlines {
        for l in std::iter::once(&p.outer).chain(&p.holes) {
            lines.linestrip(l.iter().chain(l.first()).map(|q| v3(*q, z)), dark);
        }
    }
    for s in &scene.slits {
        lines.line(v3(s.a, z), v3(s.b, z), dark);
    }
    // Dashes of about 8 px, gaps of 5.
    let px = t.view.scale as f64;
    let (dash, gap) = (8.0 * px, 5.0 * px);
    let selected = selected_joints(&cache, ctx, &selection);
    for b in &scene.bends {
        let c = if selected.contains(&b.joint) { Color::srgb(0.85, 0.42, 0.05) } else { dark };
        for s in &b.center_visible {
            let len = s.len();
            if len < 1e-9 {
                continue;
            }
            let d = s.dir();
            let mut at = 0.0;
            while at < len {
                let e = (at + dash).min(len);
                lines.line(v3(s.a + d * at, z), v3(s.a + d * e, z), c);
                at = e + gap;
            }
        }
        for s in &b.tangent_visible {
            lines.line(v3(s.a, z), v3(s.b, z), Color::srgb_u8(0x55, 0x55, 0x55));
        }
    }
    for j in &scene.joints {
        let c = if selected.contains(&j.joint) {
            ORANGE
        } else if t.hovered == Some(j.joint) {
            ORANGE_HOVER
        } else {
            continue;
        };
        for s in &j.edges {
            hi.line(v3(s.a, z), v3(s.b, z), c);
        }
    }
}

fn draw_cube_arcs(t: Res<SmTable>, open: Res<SidePanel>, kind: Res<ActiveKind>, theme: Res<Theme>, mut arcs: Gizmos<FlatCubeArcGizmos>) {
    use crate::view_cube::CubeColors;
    if is_open(&open, &kind) && t.key.is_some() {
        crate::view_cube::draw_cube_arcs(&mut arcs, &t.view, theme.view_cube_arrow());
    }
}

/// The bends' labels follow the flat view; the selected ones turn orange.
#[allow(clippy::type_complexity)]
fn place_labels(
    t: Res<SmTable>,
    cache: Res<PartCache>,
    selection: Res<Selection>,
    q_body: Query<&ComputedNode, With<FlatBody>>,
    mut q: Query<(&FlatLabel, &mut Node, &mut Visibility, &ComputedNode, &Children)>,
    mut q_text: Query<&mut TextColor>,
    theme: Res<Theme>,
) {
    let Some(body) = q_body.iter().next() else { return };
    let Some(scene) = &t.scene else { return };
    let size = body.size() * body.inverse_scale_factor();
    let selected = shown(&t, &cache).map(|c| selected_joints(&cache, c, &selection)).unwrap_or_default();
    for (l, mut node, mut vis, cn, children) in &mut q {
        let p = t.view.project(v3(l.1, scene.thickness)) + size / 2.0;
        let lsize = cn.size() * cn.inverse_scale_factor();
        let (x, y) = (p.x + 4.0, p.y - lsize.y / 2.0);
        if node.left != Val::Px(x) || node.top != Val::Px(y) {
            node.left = Val::Px(x);
            node.top = Val::Px(y);
        }
        let inside = p.x >= 0.0 && p.y >= 0.0 && p.x <= size.x && p.y <= size.y;
        let want = if inside { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let c = if selected.contains(&l.0) { Color::srgb(0.85, 0.42, 0.05) } else { theme.foreground };
        for ch in children.iter() {
            if let Ok(mut tc) = q_text.get_mut(ch)
                && tc.0 != c
            {
                tc.0 = c;
            }
        }
    }
}

/// The flat point under a pointer position (on the sheet's top).
fn flat_point(t: &SmTable, q: &Query<(&ComputedNode, &UiGlobalTransform), With<FlatBody>>, pos: Vec2) -> Option<(P2, f64)> {
    let (node, tr) = q.iter().next()?;
    let s = node.inverse_scale_factor();
    let offset = pos - tr.translation * s;
    let (o, d) = t.view.ray(offset);
    let z = t.scene.as_ref()?.thickness as f32;
    if d.z.abs() < 1e-6 {
        return None;
    }
    let k = (z - o.z) / d.z;
    let p = o + d * k;
    Some((P2::new(p.x as f64, p.y as f64), 6.0 * t.view.scale as f64))
}

fn on_flat_press(ev: On<Pointer<Press>>, mut t: ResMut<SmTable>) {
    if ev.button == PointerButton::Secondary {
        t.press = Some(ev.pointer_location.position);
    }
}

fn on_flat_drag(ev: On<Pointer<Drag>>, mut t: ResMut<SmTable>) {
    match ev.button {
        PointerButton::Secondary => t.view.orbit(ev.delta),
        PointerButton::Middle => t.view.pan(ev.delta),
        _ => {}
    }
}

fn on_flat_scroll(ev: On<Pointer<bevy::picking::events::Scroll>>, mut t: ResMut<SmTable>, q: Query<(&ComputedNode, &UiGlobalTransform), With<FlatBody>>) {
    let Some((node, tr)) = q.iter().next() else { return };
    let s = node.inverse_scale_factor();
    let cursor = ev.pointer_location.position - tr.translation * s;
    let lines = if ev.unit == bevy::input::mouse::MouseScrollUnit::Line { ev.y } else { ev.y / 40.0 };
    t.view.wheel(lines, cursor);
}

fn on_flat_move(ev: On<Pointer<Move>>, mut t: ResMut<SmTable>, q: Query<(&ComputedNode, &UiGlobalTransform), With<FlatBody>>) {
    let hit = flat_point(&t, &q, ev.pointer_location.position).and_then(|(p, tol)| t.scene.as_ref()?.joint_at(p, tol));
    if t.flat_hover != hit {
        t.flat_hover = hit;
    }
}

fn on_flat_click(ev: On<Pointer<Click>>, t: Res<SmTable>, q: Query<(&ComputedNode, &UiGlobalTransform), With<FlatBody>>, mut commands: Commands) {
    if ev.button != PointerButton::Primary {
        return;
    }
    if let Some(j) = flat_point(&t, &q, ev.pointer_location.position).and_then(|(p, tol)| t.scene.as_ref()?.joint_at(p, tol)) {
        commands.queue(move |world: &mut World| toggle_joint(world, j));
    }
}

fn on_cube_click(mut ev: On<Pointer<Click>>, mut t: ResMut<SmTable>, q: Query<(&ComputedNode, &UiGlobalTransform), With<FlatCubeImage>>) {
    ev.propagate(false);
    if ev.button != PointerButton::Primary {
        return;
    }
    let Some((node, tr)) = q.iter().next() else { return };
    let s = node.inverse_scale_factor();
    let top_left = tr.translation * s - crate::view_cube::CUBE_WIDGET / 2.0;
    if let Some(v) = crate::view_cube::view_at_spot(&t.view, ev.pointer_location.position - top_left) {
        t.view = v;
    }
}

/// The ids of a model's parts (for the scenarios' set-up and tests).
pub fn model_parts(ctx: &SheetMetalContext) -> Vec<PartId> {
    ctx.parts.iter().map(|(p, _)| *p).collect()
}
