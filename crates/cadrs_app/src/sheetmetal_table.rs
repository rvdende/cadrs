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
//!   own into an image (as the Repair panel's), with the outline, dashed bend centrelines, every
//!   joint's label (a bend's beside the middle of its centre line, a rip's off the sheet) and a
//!   view cube of its own; right-drag orbits, middle-drag pans, the wheel zooms. Zoom to fit
//!   keeps the flat clear of the cube's box.
//! - **Its right-click menu** (lesson 15 t0026.5; SM14.1, SM15.1, SM16.1) is for the part
//!   under the pointer (hit-tested on the flat's pieces; orange while the menu is open), named
//!   in its items: New sketch on, Copy, Create drawing of and Export DXF/DWG of "Flat pattern of
//!   <part>", Select other (the part, the wall, the joint there), Zoom to fit, View normal to.
//! - **Cross-highlighting** (SM1.4): a row's joint is its faces in the folded model (a bend's
//!   cylinders and ends, a rip's two side faces). Clicking a row selects them (click again:
//!   deselects; rows add up); selecting them in the model selects the row (and scrolls to it);
//!   clicking a bend or rip in the flat view does the same. Walls too: a wall's face, edge or
//!   vertex hovered or selected in the model shows where it lies in the flat; a wall hovered or
//!   clicked in the flat hovers or selects its face in the model. A hovered row lights all of
//!   its joint's faces. The joints' labels also float by the folded model while the panel is
//!   open.
//! - **Edits** (each one undo step; `cadrs_core::sheetmetal_joint`): double-click a Radius or
//!   the calculation cell to type a value (an out-of-range value leaves that cell red with its
//!   range as the tooltip); the row's menu: **Move up**, **Move down**, **Convert <bend> to
//!   rip** / **Convert <joint> to bend** (the row stays highlighted while its menu is open); a
//!   rip's Type and Style selects. They make or edit the joint's Modify joint, except for a bend
//!   a Bend or Jog feature made, whose own radius or K factor is set (it can't be converted).
//!   After a Corner break the Type, Style and Convert are locked, with a tooltip saying why.
//!   Hems can be reordered, not edited.
//!
//! Names: `smt-panel`, `smt-context`, `smt-bends-caret`, `smt-joints-caret`, rows
//! `smt-bend-row-<i>` / `smt-joint-row-<i>` (from 0), cells `smt-bend-<i>-radius`,
//! `smt-bend-<i>-value`, selects `smt-joint-<i>-type`, `smt-joint-<i>-style`, the flat view
//! `smt-flat`, its cube `smt-flat-cube`, labels `smt-flat-label-<name>` and, by the model,
//! `smt-label-<name>`; menu items `smt-move-up`, `smt-move-down`, `smt-convert`, `smt-flat-sketch`,
//! `smt-flat-copy`, `smt-flat-drawing`, `smt-flat-export`, `smt-flat-select-other`,
//! `smt-zoom-fit`, `smt-normal-to`.

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
use cadrs_core::sheetmetal_joint::{ModifyJointFeature, PutModifyJoint, SetTableOrder, TableEdit, bend_feature_edit, bend_feature_of, modify_joint_of, radius_error, table_edit, value_error};
use cadrs_core::{ElementId, FeatureId, FeatureKind, PartId, Solid};
use cadrs_sheetmetal::model::P3;
use cadrs_sheetmetal::poly::P2;
use cadrs_sheetmetal::table::{self, Table};
use cadrs_sheetmetal::view::{FlatScene, Tris};
use cadrs_sheetmetal::{BendCalc, JointId, JointKind, RipStyle, WallId};
use cadrs_sketch::units::Quantity;
use cadrs_ui::inline_edit::{DoubleClick, DoubleClickable, InlineEdit, InlineEditCommit, InlineEditLabel, InlineEditOptions, begin_inline_edit};
use cadrs_ui::menu::{ContextMenuAnchor, ContextMenuRequested, ContextMenuTarget, Menu, MenuAction, MenuItem};
use cadrs_ui::prelude::*;
use cadrs_ui::{Column, Select, SelectChange, TableBody, TableHeader, TableRoot, TableRow, open_context_menu};

use crate::appearance::SidePanel;
use crate::camera::{StandardView, ViewState};
use crate::parts::PartCache;
use crate::viewport::{ActiveKind, ExtraHighlight, Pick, PlaneHighlight, Selection, ViewportArea, ViewportRect, ViewportView};
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
/// A part the flat view's menu is open on (lesson 15, t0026.5: the whole flat orange).
const ORANGE_MENU: Color = Color::srgb(0.93, 0.58, 0.22);

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
                    model_hover,
                    hover_to_view,
                    resize_target,
                    fit_on_open,
                    sync_cameras,
                    sync_meshes,
                    draw_flat,
                    draw_cube_arcs,
                    place_labels,
                    place_folded_labels,
                    place_flat_axes,
                )
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut t: ResMut<SmTable>, mut extra: ResMut<ExtraHighlight>| {
                let (image, cube) = (t.image.clone(), t.cube_image.clone());
                *t = SmTable { image, cube_image: cube, ..SmTable::default() };
                extra.hovered.clear();
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
    /// What is under the pointer in the panel: a row's joint, or the flat view's joint or wall.
    pub hovered: Option<FlatHit>,
    pub flat_hover: Option<FlatHit>,
    /// What the main view's pointer is on (a face, edge or vertex of the shown model's walls or
    /// joints), with what it is in the flat.
    model_hover: Option<(Pick, FlatHit)>,
    /// The flat-pattern part (its index, the context's part order) the flat view's menu was
    /// opened on, and what else lies under the pointer there (Select other).
    pub menu_part: Option<usize>,
    menu_others: Vec<FlatHit>,
    /// Where the flat view's menu was opened (flat 2D) and the pick tolerance there.
    menu_point: Option<(P2, f64)>,
    /// Table edits that couldn't be made (a Bend feature's value no K factor gives): the cell
    /// shows the typed text red with the reason, until the joint is edited again.
    edit_errors: Vec<(JointId, CellKind, String, String)>,
    /// The flat view as last laid out, and what it was laid out from.
    pub scene: Option<Arc<FlatScene>>,
    scene_of: Option<(FeatureId, u64)>,
    /// What the panel was built from (rebuilt only when it changes).
    key: Option<String>,
    /// The selected joints as last seen (a new one scrolls its row into view).
    seen: Vec<JointId>,
    /// The hover this panel put on the main view ([`ExtraHighlight::hovered`]), and what for.
    hover_set: Option<(FeatureId, FlatHit)>,
    image: Option<Handle<Image>>,
    image_size: UVec2,
    scale: f32,
    cube_image: Option<Handle<Image>>,
    meshed: Option<u64>,
    /// Where the secondary button went down over the flat view.
    press: Option<Vec2>,
    /// Frames until the flat view is fitted (the layout settles first).
    fit_in: Option<u32>,
    /// Frames until the main view is fitted again beside the opened panel.
    main_fit_in: Option<u32>,
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
            model_hover: None,
            menu_part: None,
            menu_others: Vec::new(),
            menu_point: None,
            edit_errors: Vec::new(),
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
            main_fit_in: None,
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

/// An axis letter of the flat view cube's triad.
#[derive(Component)]
struct FlatAxisLabel(Vec3);

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

/// A joint or a wall of the shown model, as the flat view picks and highlights it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum FlatHit {
    Joint(JointId),
    Wall(WallId),
    /// A model edge or vertex picked at a wall outline's side or corner in the flat (SM1.4).
    Pick(Pick),
}

impl FlatHit {
    fn joint(self) -> Option<JointId> {
        match self {
            FlatHit::Joint(j) => Some(j),
            _ => None,
        }
    }
}

/// A label in the flat view: its joint, its spot (flat 2D, on the sheet's top) and the way it
/// reads off from there.
#[derive(Component, Clone)]
struct FlatLabel(JointId, Vec<(P2, cadrs_sheetmetal::poly::V2)>);

/// A joint's label by the folded model in the main view (SM13.1), while the panel is open.
#[derive(Component, Clone, Copy)]
struct FoldedLabel(JointId, Vec3);

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

/// A pick's point on its part (a face's, an edge's middle, a vertex).
fn pick_point(cache: &PartCache, pick: Pick) -> Option<P3> {
    let part = pick.part()?;
    let solid = &cache.parts.iter().find(|p| p.id == part)?.solid;
    match pick {
        Pick::Face(_, name) => face_point(solid, solid.faces.iter().position(|f| f.name == name)?),
        Pick::Edge(_, name) => edge_point(solid, solid.edges.iter().position(|e| e.name == name)?),
        Pick::Vertex(_, name) => solid.vertices.iter().find(|v| v.name == name).map(|v| P3::new(v.point[0], v.point[1], v.point[2])),
        _ => None,
    }
}

/// What a viewport pick is in its model's flat (SM1.4): a joint (a bend's faces or edges, a
/// rip's side faces), else the wall whose face, edge or vertex it is.
pub fn hit_of_pick(cache: &PartCache, pick: Pick) -> Option<(FeatureId, FlatHit)> {
    if let Some((m, j)) = joint_of_pick(cache, pick) {
        return Some((m, FlatHit::Joint(j)));
    }
    let part = pick.part()?;
    let ctx = cache.sheet_metal.iter().find(|c| c.parts.iter().any(|(p, _)| *p == part))?;
    let p = pick_point(cache, pick)?;
    cadrs_sheetmetal::view::wall_at(&ctx.model, p, tolerance(ctx)).map(|w| (ctx.feature, FlatHit::Wall(w)))
}

/// A wall's faces in the folded model: both (`both`), or only its definition face (what a
/// click in the flat selects).
fn wall_picks(cache: &PartCache, ctx: &SheetMetalContext, wall: WallId, both: bool) -> Vec<Pick> {
    let tol = tolerance(ctx);
    let mut out = Vec::new();
    for (pid, walls) in &ctx.parts {
        if !walls.contains(&wall) {
            continue;
        }
        let Some(part) = cache.parts.iter().find(|p| p.id == *pid) else { continue };
        for (i, f) in part.solid.faces.iter().enumerate() {
            let Some(p) = face_point(&part.solid, i) else { continue };
            if cadrs_sheetmetal::view::joint_at(&ctx.model, p, tol).is_some() {
                continue;
            }
            if let Some((w, other)) = cadrs_sheetmetal::model_edit::wall_at(&ctx.model, p, tol)
                && w == wall
                && (both || !other)
            {
                out.push(Pick::Face(*pid, f.name));
            }
        }
    }
    out
}

/// A joint's or a wall's faces in the folded model.
fn hit_picks(cache: &PartCache, ctx: &SheetMetalContext, hit: FlatHit, both: bool) -> Vec<Pick> {
    match hit {
        FlatHit::Joint(j) => joint_picks(cache, ctx, j),
        FlatHit::Wall(w) => wall_picks(cache, ctx, w, both),
        FlatHit::Pick(p) => vec![p],
    }
}

/// The model edge or vertex at a wall outline's side or corner in the flat: the flat point
/// carried back onto the wall's definition face ([`cadrs_sheetmetal::view::from_flat`]), then
/// the part's nearest edge or vertex there.
fn model_pick_at(cache: &PartCache, ctx: &SheetMetalContext, scene: &FlatScene, hit: cadrs_sheetmetal::view::OutlineHit) -> Option<Pick> {
    use cadrs_sheetmetal::view::OutlineHit;
    let (w, q, corner) = match hit {
        OutlineHit::Corner(w, q) => (w, q, true),
        OutlineHit::Side(w, q) => (w, q, false),
    };
    let p = cadrs_sheetmetal::view::from_flat(&ctx.model, &ctx.flat, scene, w, q)?;
    let p = [p.x, p.y, p.z];
    let (pid, _) = ctx.parts.iter().find(|(_, walls)| walls.contains(&w))?;
    let solid = &cache.parts.iter().find(|x| x.id == *pid)?.solid;
    let tol = (0.3 * ctx.model.params.thickness).max(0.05);
    let d = |a: [f64; 3]| ((a[0] - p[0]).powi(2) + (a[1] - p[1]).powi(2) + (a[2] - p[2]).powi(2)).sqrt();
    if corner {
        let v = solid.vertices.iter().map(|v| (d(v.point), v.name)).min_by(|a, b| a.0.total_cmp(&b.0))?;
        return (v.0 <= tol).then_some(Pick::Vertex(*pid, v.1));
    }
    let e = solid.edges.iter().map(|e| (e.distance(p), e.name)).min_by(|a, b| a.0.total_cmp(&b.0))?;
    (e.0 <= tol).then_some(Pick::Edge(*pid, e.1))
}

/// How a model pick shows in the flat view.
enum FlatMark {
    /// A joint (drawn as its bend region or rip edges).
    Joint,
    /// A wall's face, edge or vertex carried into the flat: polylines (a dot: one point).
    Lines(Vec<Vec<P2>>),
    /// A whole part: its outline.
    Part(usize),
}

/// Where a model pick lies in the shown model's flat (`None`: not on it).
fn flat_mark(cache: &PartCache, ctx: &SheetMetalContext, scene: &FlatScene, pick: Pick) -> Option<FlatMark> {
    if let Pick::Part(pid) = pick {
        return ctx.parts.iter().position(|(p, _)| *p == pid).map(FlatMark::Part);
    }
    let part = pick.part()?;
    if !ctx.parts.iter().any(|(p, _)| *p == part) {
        return None;
    }
    // An edge or vertex shows where it lies on its wall, even along a rip.
    let on_wall = matches!(pick, Pick::Edge(..) | Pick::Vertex(..))
        .then(|| cadrs_sheetmetal::view::wall_at(&ctx.model, pick_point(cache, pick)?, tolerance(ctx)))
        .flatten();
    let w = match on_wall {
        Some(w) => w,
        None => match hit_of_pick(cache, pick)?.1 {
            FlatHit::Joint(_) => return Some(FlatMark::Joint),
            FlatHit::Wall(w) => w,
            FlatHit::Pick(_) => return None,
        },
    };
    let solid = &cache.parts.iter().find(|p| p.id == part)?.solid;
    let to = |q: &[f64; 3]| cadrs_sheetmetal::view::to_flat(&ctx.model, &ctx.flat, scene, w, P3::new(q[0], q[1], q[2]));
    let lines: Vec<Vec<P2>> = match pick {
        Pick::Face(_, name) => solid
            .faces
            .iter()
            .find(|f| f.name == name)?
            .loops
            .iter()
            .map(|l| l.iter().chain(l.first()).filter_map(to).collect())
            .collect(),
        Pick::Edge(_, name) => vec![solid.edges.iter().find(|e| e.name == name)?.points.iter().filter_map(to).collect()],
        Pick::Vertex(_, name) => vec![vec![to(&solid.vertices.iter().find(|v| v.name == name)?.point)?]],
        _ => return None,
    };
    Some(FlatMark::Lines(lines))
}

/// A wall or a whole part filled in the flat view.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum FillOf {
    Wall(WallId),
    Part(usize),
}

/// What a model pick fills in the flat: a wall's face fills its wall, a part all of its flat.
fn fill_of(cache: &PartCache, ctx: &SheetMetalContext, pick: Pick) -> Option<FillOf> {
    match pick {
        Pick::Part(pid) => ctx.parts.iter().position(|(p, _)| *p == pid).map(FillOf::Part),
        Pick::Face(pid, _) if ctx.parts.iter().any(|(p, _)| *p == pid) => match hit_of_pick(cache, pick)? {
            (_, FlatHit::Wall(w)) => Some(FillOf::Wall(w)),
            _ => None,
        },
        _ => None,
    }
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
        let mut q_folded = world.query_filtered::<Entity, With<FoldedLabel>>();
        for e in q_folded.iter(world).collect::<Vec<_>>() {
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
    let features: Vec<cadrs_core::Feature> = doc.active_element().map(|e| e.features().to_vec()).unwrap_or_default();
    let mut mjs: Vec<CellState> = ctx.as_ref().map(|c| cell_states(doc, &features, c)).unwrap_or_default();
    // Edits that couldn't be made show red with why.
    for (j, kind, text, msg) in &t.edit_errors {
        let i = match mjs.iter().position(|c| c.joint == *j) {
            Some(i) => i,
            None => {
                mjs.push(CellState { joint: *j, radius_error: None, radius_text: None, value_error: None, value_text: None, made_by: None });
                mjs.len() - 1
            }
        };
        let c = &mut mjs[i];
        match kind {
            CellKind::Radius => (c.radius_error, c.radius_text) = (Some(msg.clone()), Some(text.clone())),
            CellKind::Value => (c.value_error, c.value_text) = (Some(msg.clone()), Some(text.clone())),
        }
    }
    let key = format!("{list:?}|{:?}|{rows:?}|{mjs:?}|{}|{}|{:?}", ctx.as_ref().map(|c| (c.feature, c.corner_broken)), t.bends_open, t.joints_open, units);
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
    let mut q_folded = world.query_filtered::<Entity, With<FoldedLabel>>();
    for e in q_folded.iter(world).collect::<Vec<_>>() {
        world.entity_mut(e).despawn();
    }
    {
        let mut t = world.resource_mut::<SmTable>();
        t.key = Some(key);
        if first_open {
            t.fit_in = Some(3);
            // The main view is narrower now: the model is fitted into what is left of it.
            t.main_fit_in = Some(4);
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
        // The tables, scrolling together above the flat view, with a scrollbar on the right
        // while they overflow (frame t0008).
        p.spawn((
            Name::new("smt-tables-frame"),
            Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_height: Val::Px(0.0),
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(th.panel_border),
        ))
        .with_children(|f| {
            let list = f
                .spawn((
                    Name::new("smt-tables"),
                    RowsScroll,
                    TableBody,
                    cadrs_ui::scrollbar::ScrollGutter(14.0),
                    Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    scroll,
                ))
                .with_children(|b| {
                    if let (Some(ctx), Some(rows)) = (&ctx, &rows) {
                        spawn_tables(b, &th, ctx, rows, &mjs, &units, bends_open, joints_open);
                    } else {
                        b.spawn(th.text("No sheet metal model", 12.0, FontWeight::NORMAL, th.muted_foreground));
                    }
                })
                .id();
            f.spawn(cadrs_ui::vertical_scrollbar(&th, "smt-tables-scrollbar", list));
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
            .observe(on_cube_click)
            .with_children(|c| {
                // The triad's letters (the flat is seen from the top: X and Y).
                for (axis, letter, color) in [(Vec3::X, "X", Color::srgb_u8(0xd0, 0x30, 0x30)), (Vec3::Y, "Y", Color::srgb_u8(0x3a, 0x9a, 0x3a)), (Vec3::Z, "Z", th.axis_z)] {
                    c.spawn((
                        Name::new(format!("smt-flat-axis-{}", letter.to_lowercase())),
                        FlatAxisLabel(axis),
                        Node { position_type: PositionType::Absolute, ..default() },
                        th.text(letter, 12.0, FontWeight::BOLD, color),
                        Pickable::IGNORE,
                    ));
                }
            });
            if let Some(ctx) = &ctx {
                // Every bend's, rip's and tangent joint's label (SM13.1).
                let scene = FlatScene::new(&ctx.model, &ctx.flat);
                for j in &ctx.model.joints {
                    let places = scene.label_places(j.id);
                    if places.is_empty() {
                        continue;
                    }
                    b.spawn((
                        Name::new(format!("smt-flat-label-{}", j.name)),
                        FlatLabel(j.id, places),
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
    // The joints' labels by the folded model (SM13.1: "the same labels float next to each bend
    // in the viewport and the flat view").
    if let Some(ctx) = &ctx {
        for j in &ctx.model.joints {
            let Some(spot) = cadrs_sheetmetal::view::label_spot(&ctx.model, j.id) else { continue };
            let label = commands
                .spawn((
                    Name::new(format!("smt-label-{}", j.name)),
                    FoldedLabel(j.id, Vec3::new(spot.x as f32, spot.y as f32, spot.z as f32)),
                    DespawnOnExit(AppState::Document),
                    Node {
                        position_type: PositionType::Absolute,
                        padding: UiRect::axes(Val::Px(4.0), Val::Px(1.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                    BorderColor::all(Color::srgb_u8(0xb0, 0xb0, 0xb0)),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                ))
                .with_child((th.text(j.name.clone(), 11.0, FontWeight::MEDIUM, th.foreground), Pickable::IGNORE))
                .id();
            commands.entity(area).add_child(label);
        }
    }
    world.flush();
    let at = world.get::<Children>(parent).and_then(|c| c.iter().position(|e| e == area)).map_or(0, |i| i + 1);
    world.entity_mut(parent).insert_children(at, &[panel]);
}

/// A bend row's editable cells as the document has them: an out-of-range Radius or value (red,
/// with its range) shown as typed, and whether a Bend or Jog feature made the bend (its own
/// parameters are edited, SM13.3).
#[derive(Debug, Clone, PartialEq)]
struct CellState {
    joint: JointId,
    radius_error: Option<String>,
    radius_text: Option<String>,
    value_error: Option<String>,
    value_text: Option<String>,
    /// The Bend or Jog feature that made it.
    made_by: Option<String>,
}

fn cell_states(doc: &ActiveDocument, features: &[cadrs_core::Feature], ctx: &SheetMetalContext) -> Vec<CellState> {
    let mut out = Vec::new();
    for j in &ctx.model.joints {
        if let Some(f) = bend_feature_of(features, ctx, j.id) {
            let b = match &f.kind {
                FeatureKind::SheetMetalTool(cadrs_core::sheetmetal_tools::SheetMetalTool::Bend(b)) => b,
                FeatureKind::SheetMetalTool(cadrs_core::sheetmetal_tools::SheetMetalTool::Jog(x)) => &x.bend,
                _ => continue,
            };
            let r_bad = !b.use_model_radius && !(b.radius > 0.0);
            let k_bad = !b.use_model_k && !(0.0..=1.0).contains(&b.k_factor);
            out.push(CellState {
                joint: j.id,
                radius_error: r_bad.then(|| "The bend radius must be greater than 0".to_string()),
                radius_text: r_bad.then(|| b.radius_expr.trim_end_matches(" mm").to_string()),
                value_error: k_bad.then(|| "The K Factor must be between 0 and 1".to_string()),
                value_text: k_bad.then(|| b.k_expr.clone()),
                made_by: Some(f.name.clone()),
            });
            continue;
        }
        let Some((_, x)) = modify_joint(doc, ctx.feature, j.id) else { continue };
        let bend = x.joint_type == cadrs_core::sheetmetal_joint::JointType::Bend;
        let r = (bend && !x.use_model_radius).then(|| radius_error(x.radius)).flatten().map(|e| e.message());
        let v = (bend && !x.use_model_value).then(|| value_error(x.calc, x.value)).flatten().map(|e| e.message());
        out.push(CellState {
            joint: j.id,
            radius_text: r.as_ref().map(|_| x.radius_expr.trim_end_matches(" mm").to_string()),
            radius_error: r,
            value_text: v.as_ref().map(|_| x.value_expr.trim_end_matches(" mm").to_string()),
            value_error: v,
            made_by: None,
        });
    }
    out
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

/// Why a joint's Type and Style (and Convert) are locked after a Corner break (SM11.4).
const CORNER_BREAK_LOCK: &str = "Locked by a Corner break on this model.\nMake Corner breaks after the joints are final.";

/// The shaded "#" column's fill.
const NUMBER_FILL: Color = Color::srgb(0.945, 0.945, 0.945);

#[allow(clippy::too_many_arguments)]
fn spawn_tables(
    b: &mut ChildSpawnerCommands,
    t: &Theme,
    ctx: &SheetMetalContext,
    rows: &Table,
    cells: &[CellState],
    units: &cadrs_sketch::units::Units,
    bends_open: bool,
    joints_open: bool,
) {
    let calc = ctx.model.params.bend_calc;
    let (value_label, value_q) = value_title(calc, units);
    let len = units.length.symbol();
    // The Bends table fills the panel's width (`sheetmetalflatpatterntable-02.png`).
    let bend_cols = vec![
        Column::new("num", "#").grow(0.55).shaded(NUMBER_FILL),
        Column::new("name", "Name").grow(1.15),
        Column::new("radius", format!("Radius({len})")).grow(1.15),
        Column::new("angle", "Angle(deg)").grow(1.1),
        Column::new("dir", "Bend direction").grow(1.25),
        Column::new("value", value_label).grow(1.45),
    ];
    let joint_cols = vec![Column::new("name", "Name").width(110.0).shaded(NUMBER_FILL), Column::new("type", "Type").width(110.0), Column::new("style", "Style").width(220.0)];
    let locked = ctx.corner_broken;
    caret_header(b, t, "smt-bends-caret", "Bends", bends_open);
    if bends_open {
        b.spawn((Name::new("smt-bends"), TableRoot, Node { flex_direction: FlexDirection::Column, flex_shrink: 0.0, ..default() })).with_children(|tbl| {
            tbl.spawn(TableHeader::new("smt-bends-header", bend_cols.clone()).height(ROW_H).grid().build(t));
            for (i, r) in rows.bends.iter().enumerate() {
                let joint = r.joint;
                let st = cells.iter().find(|c| c.joint == joint);
                let radius = match st.and_then(|c| c.radius_text.clone()) {
                    // An out-of-range entry shows as typed.
                    Some(typed) => typed,
                    None => units.value(r.radius, Quantity::Length),
                };
                let value = match st.and_then(|c| c.value_text.clone()) {
                    Some(typed) => typed,
                    None => match r.value {
                        Some(v) if value_q == Quantity::Length => units.value(v.value(), Quantity::Length),
                        Some(v) => number(v.value(), 3),
                        None => "–".into(),
                    },
                };
                let editable = r.editable;
                let made_by = st.and_then(|c| c.made_by.clone());
                let cells_of: Vec<(String, Option<CellKind>, Option<String>)> = vec![
                    (r.number.to_string(), None, None),
                    (r.name.clone(), None, None),
                    (radius, editable.then_some(CellKind::Radius), st.and_then(|c| c.radius_error.clone())),
                    (number(r.angle_deg, 2), None, None),
                    (r.direction.label().to_string(), None, None),
                    (value, editable.then_some(CellKind::Value), st.and_then(|c| c.value_error.clone())),
                ];
                let mut tr = TableRow::new(format!("smt-bend-row-{i}"), &bend_cols).height(ROW_H).grid();
                for (c, (text, kind, err)) in cells_of.into_iter().enumerate() {
                    let th = t.clone();
                    // Only the cell whose value is out of range is red (the Radius for a radius).
                    let red = kind.is_some() && err.is_some();
                    let made_by = made_by.clone();
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
                            if !red && let Some(f) = &made_by {
                                e.insert(Tooltip::new(format!("Set in {f}: a new value edits {f}")));
                            }
                        }
                        if red {
                            e.insert((BackgroundColor(Color::srgb_u8(0xf8, 0xd7, 0xd7)), BorderColor::all(th.feature_error)));
                            e.entry::<Node>().and_modify(|mut n| n.border = UiRect::all(Val::Px(1.0)));
                            if let Some(m) = err {
                                e.insert(Tooltip::error(m));
                            }
                        }
                        let fg = if red { th.feature_error } else { th.foreground };
                        let weight = if bold_num { FontWeight::BOLD } else { FontWeight::MEDIUM };
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
            tbl.spawn(TableHeader::new("smt-joints-header", joint_cols.clone()).height(ROW_H).grid().build(t));
            for (i, r) in rows.joints.iter().enumerate() {
                let joint = r.joint;
                let tangent = r.kind == table::JointType::Tangent;
                let ninety = ctx.model.joint(joint).and_then(|j| rip_angle(ctx, j)).is_some_and(|a| (a - 90.0).abs() < 1e-3);
                let style = r.style.unwrap_or_default();
                let name = r.name.clone();
                let th = t.clone();
                let tr = TableRow::new(format!("smt-joint-row-{i}"), &joint_cols)
                    .height(ROW_H)
                    .grid()
                    .cell({
                        let th = th.clone();
                        move |c| {
                            c.spawn(Node { width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() })
                                .with_child((th.text(name, 12.0, FontWeight::BOLD, th.foreground), Pickable::IGNORE));
                        }
                    })
                    .cell({
                        let th = th.clone();
                        move |c| {
                            let label = if tangent { "Tangent" } else { "Rip" };
                            let options = if tangent { vec![("Tangent".to_string(), true)] } else { vec![("Rip".to_string(), true), ("Bend".to_string(), true)] };
                            let tip = locked.then_some(CORNER_BREAK_LOCK);
                            dropdown(c, &th, format!("smt-joint-{i}-type"), label, Dropdown { select: JointSelect::Type(joint), options, disabled: tangent || locked }, tip);
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
                        let tip = locked.then_some(CORNER_BREAK_LOCK);
                        dropdown(c, &th, format!("smt-joint-{i}-style"), style.label(), Dropdown { select: JointSelect::Style(joint), options, disabled: locked }, tip);
                    });
                tbl.spawn((tr.build(t), RowRef { joint, bend: false, index: i }));
            }
        });
    }
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

fn dropdown(c: &mut ChildSpawner, t: &Theme, name: String, label: &str, d: Dropdown, tip: Option<&str>) {
    let fg = if d.disabled { t.muted_foreground } else { t.foreground };
    let mut e = c.spawn((
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
    ));
    if let Some(tip) = tip {
        e.insert(Tooltip::new(tip));
    }
    e.observe(on_dropdown_click).with_children(|r| {
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
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_rows(
    mut t: ResMut<SmTable>,
    cache: Res<PartCache>,
    selection: Res<Selection>,
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    q_rows: Query<(Entity, &RowRef, Has<cadrs_ui::style::Selected>, &Hovered, &ComputedNode, Option<&cadrs_ui::style::ForceState>)>,
    mut q_scroll: Query<(&mut ScrollPosition, &ComputedNode), With<RowsScroll>>,
    q_menus: Query<&MenuFor, With<ContextMenuAnchor>>,
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
    // The row whose menu is open stays highlighted while it is (frame t0016).
    let menu_row = q_menus.iter().next().map(|m| (m.0, m.1));
    for (e, r, has, _, node, force) in &q_rows {
        let want = selected.contains(&r.joint);
        if want && !has {
            commands.entity(e).try_insert(cadrs_ui::style::Selected);
        } else if !want && has {
            commands.entity(e).try_remove::<cadrs_ui::style::Selected>();
        }
        let menu = menu_row == Some((r.joint, r.bend));
        if menu && force.is_none() {
            commands.entity(e).try_insert(cadrs_ui::style::ForceState(cadrs_ui::style::VisualState::Pressed));
        } else if !menu && force.is_some() {
            commands.entity(e).try_remove::<cadrs_ui::style::ForceState>();
        }
        row_y = node.size().y * node.inverse_scale_factor();
        if new.contains(&r.joint) {
            // Bends come first, then the Other joints table under them.
            let bends = q_rows.iter().filter(|(_, x, ..)| x.bend).count();
            scroll_to = Some(if r.bend { r.index } else { bends + 2 + r.index });
        }
    }
    let hovered = q_rows.iter().find(|(_, _, _, h, ..)| h.get()).map(|(_, r, ..)| FlatHit::Joint(r.joint)).or(t.flat_hover);
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

/// What the main view's pointer is on, in the shown model's flat (SM1.4: a hovered face, edge
/// or vertex of a wall, or a joint's, lights up in the flat view).
fn model_hover(mut t: ResMut<SmTable>, cache: Res<PartCache>, highlight: Res<PlaneHighlight>, open: Res<SidePanel>, kind: Res<ActiveKind>) {
    let pick = if is_open(&open, &kind) { highlight.viewport } else { None };
    if t.model_hover.map(|(p, _)| p) == pick && !cache.is_changed() {
        return;
    }
    let shown_model = shown(&t, &cache).map(|c| c.feature);
    let want = pick.and_then(|p| hit_of_pick(&cache, p).filter(|(m, _)| Some(*m) == shown_model).map(|(_, h)| (p, h)));
    if t.model_hover != want {
        t.model_hover = want;
    }
}

/// The hovered row's joint, or the flat view's joint or wall, shows hovered in the model: all of
/// its faces (a bend's cylinders and ends, a rip's two sides, a wall's faces).
fn hover_to_view(mut t: ResMut<SmTable>, cache: Res<PartCache>, mut extra: ResMut<ExtraHighlight>, open: Res<SidePanel>, kind: Res<ActiveKind>) {
    let want = t.hovered.filter(|_| is_open(&open, &kind)).and_then(|h| shown(&t, &cache).map(|c| (c.feature, h)));
    if want == t.hover_set && !(want.is_some() && cache.is_changed()) {
        return;
    }
    let picks = match want.and_then(|(_, h)| shown(&t, &cache).map(|ctx| (ctx, h))) {
        Some((ctx, h)) => hit_picks(&cache, ctx, h, true),
        None => Vec::new(),
    };
    if extra.hovered != picks {
        extra.hovered = picks;
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
    // A bend a Bend or Jog feature made: that feature's own radius or K factor (SM13.3).
    let features: Vec<cadrs_core::Feature> = doc.active_element().map(|e| e.features().to_vec()).unwrap_or_default();
    if let Some(f) = bend_feature_of(&features, &ctx, joint) {
        let cell = match &edit {
            TableEdit::Radius(_, x) => Some((CellKind::Radius, x.clone())),
            TableEdit::Value(_, x) => Some((CellKind::Value, x.clone())),
            _ => None,
        };
        match bend_feature_edit(f, &j, &ctx.model.params, &edit) {
            Ok(f) => {
                let label = edit.label(&j.name);
                world.resource_mut::<SmTable>().edit_errors.retain(|e| e.0 != joint);
                run(world, element, &cadrs_core::commands::ReplaceFeature { element, feature: f, label });
            }
            // Shown in the cell: the typed text, red, with why as its tooltip.
            Err(e) => match cell {
                Some((kind, text)) => {
                    let mut t = world.resource_mut::<SmTable>();
                    t.edit_errors.retain(|x| !(x.0 == joint && x.1 == kind));
                    t.edit_errors.push((joint, kind, text.trim_end_matches(" mm").to_string(), e));
                }
                None => warn!("sheet metal table: {e}"),
            },
        }
        return;
    }
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
    toggle_hit(world, FlatHit::Joint(joint));
}

/// Toggles a joint's faces, or a wall's face, in the selection.
fn toggle_hit(world: &mut World, hit: FlatHit) {
    let picks = {
        let cache = world.resource::<PartCache>();
        let Some(ctx) = shown(world.resource::<SmTable>(), cache) else { return };
        hit_picks(cache, ctx, hit, false)
    };
    toggle_picks(world, picks);
}

fn toggle_picks(world: &mut World, picks: Vec<Pick>) {
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

/// The flat part's name as Onshape's menu says it: "Flat pattern of <part>".
fn flat_name(world: &World, part: usize) -> Option<String> {
    let cache = world.resource::<PartCache>();
    let ctx = shown(world.resource::<SmTable>(), cache)?;
    let (pid, _) = ctx.parts.get(part)?;
    let p = cache.parts.iter().find(|p| p.id == *pid)?;
    Some(format!("Flat pattern of {}", cadrs_core::parts::display_name(p, &cache.props)))
}

/// A Select other entry's label.
/// As Onshape names them: after the feature that made them ("Face of Flange 1", "Edge of
/// Sheet metal model 1"); a joint by its table name.
fn hit_label(world: &World, ctx: &SheetMetalContext, hit: FlatHit) -> String {
    let named = |f: FeatureId| feature_name(world, f).unwrap_or_else(|| ctx.name.clone());
    match hit {
        FlatHit::Joint(j) => ctx.model.joint(j).map_or_else(|| "Joint".into(), |j| j.name.clone()),
        FlatHit::Wall(w) => format!("Face of {}", named(ctx.owner(cadrs_core::sheetmetal::PieceKey::Wall(w)))),
        FlatHit::Pick(p) => {
            let what = if matches!(p, Pick::Vertex(..)) { "Vertex" } else { "Edge" };
            let op = match p {
                Pick::Edge(_, n) => n.faces[0].op,
                _ => ctx.feature.0,
            };
            format!("{what} of {}", named(FeatureId(op)))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn on_context_menu(
    ev: On<ContextMenuRequested>,
    q_row: Query<&RowRef>,
    q_flat: Query<(), With<FlatBody>>,
    q_body: Query<(&ComputedNode, &UiGlobalTransform), With<FlatBody>>,
    cache: Res<PartCache>,
    mut t: ResMut<SmTable>,
    mut commands: Commands,
) {
    if q_flat.contains(ev.entity) {
        // The end of a right-drag (an orbit) isn't a right-click.
        let pressed = t.press.take();
        if pressed.is_some_and(|p| p.distance(ev.position) > 3.0) {
            return;
        }
        // What is under the pointer: the part the menu is for, and the rest (Select other).
        let at = flat_point(&t, &q_body, pressed.unwrap_or(ev.position));
        let (part, others) = match (at, t.scene.clone()) {
            (Some((p, tol)), Some(scene)) => {
                let mut others = Vec::new();
                if let Some(h @ FlatHit::Pick(_)) = flat_hit(&cache, &t, p, tol) {
                    others.push(h);
                }
                if let Some(j) = scene.joint_at(p, tol) {
                    others.push(FlatHit::Joint(j));
                }
                if let Some(w) = scene.wall_at(p) {
                    others.push(FlatHit::Wall(w));
                }
                (scene.part_at(p), others)
            }
            _ => (None, Vec::new()),
        };
        t.menu_part = part;
        t.menu_others = others;
        t.menu_point = at;
        let position = ev.position;
        commands.queue(move |world: &mut World| open_flat_menu(world, position));
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
        let features: Vec<cadrs_core::Feature> = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()).map(|e| e.features().to_vec()).unwrap_or_default();
        // A bend a Bend or Jog made stays a bend (edit or delete that feature instead).
        let made_by = bend_feature_of(&features, &ctx, joint).map(|f| f.name.clone());
        let convert = if bend { format!("Convert {} to rip", j.name) } else { format!("Convert {} to bend", j.name) };
        let mut item = MenuItem::new("smt-convert", convert).disabled(hem || tangent || made_by.is_some() || ctx.corner_broken);
        if ctx.corner_broken {
            item = item.tooltip(CORNER_BREAK_LOCK);
        } else if let Some(f) = &made_by {
            item = item.tooltip(format!("{} is made by {f}: it can't be made a rip", j.name));
        } else if hem {
            item = item.tooltip("A hem's bend can't be made a rip");
        }
        let menu = Menu::new("smt-row-menu")
            .min_width(180.0)
            .item_height(24.0)
            .text_only()
            .item(MenuItem::new("smt-move-up", "Move up").disabled(!up))
            .item(MenuItem::new("smt-move-down", "Move down").disabled(!down))
            .separator()
            .item(item);
        let theme = world.resource::<Theme>().clone();
        let mut cm = world.commands();
        let anchor = open_context_menu(&mut cm, at, menu.build(&theme));
        cm.entity(anchor).insert((MenuFor(joint, bend), DespawnOnExit(AppState::Document)));
        world.flush();
    });
}

/// The sketch on the flat pattern drawn at a flat point (within `tol`), if any.
fn flat_sketch_at(world: &World, p: P2, tol: f64) -> Option<FeatureId> {
    let t = world.resource::<SmTable>();
    let scene = t.scene.as_ref()?;
    let ctx = shown(t, world.resource::<PartCache>())?;
    let el = world.get_resource::<ActiveDocument>()?.active_element()?;
    let features = el.active_features();
    for f in &features {
        let Some(sk) = f.sketch() else { continue };
        let Some(cadrs_sketch::PlaneRef::Feature(fp)) = sk.plane else { continue };
        let Some((model, index)) = cadrs_core::sheetmetal_flat::flat_target(&features, fp.feature) else { continue };
        if model != ctx.feature {
            continue;
        }
        let shift = scene.shifts.get(index).copied().unwrap_or_else(cadrs_sheetmetal::poly::V2::zeros);
        for l in sketch_polylines(&sk.geometry) {
            for w in l.windows(2) {
                let a = P2::new(w[0].x, w[0].y) + shift;
                let b = P2::new(w[1].x, w[1].y) + shift;
                let d = b - a;
                let k = if d.norm_squared() > 0.0 { ((p - a).dot(&d) / d.norm_squared()).clamp(0.0, 1.0) } else { 0.0 };
                if (p - (a + d * k)).norm() <= tol {
                    return Some(f.id);
                }
            }
        }
    }
    None
}

/// A feature's name in the active Part Studio.
fn feature_name(world: &World, id: FeatureId) -> Option<String> {
    world.get_resource::<ActiveDocument>()?.active_element()?.feature(id).map(|f| f.name.clone())
}

/// The flat view's menu, as Onshape's (lesson 15, t0026.5; SM14.1, SM15.1, SM16.1): Edit the
/// sheet metal model (and the flat sketch under the pointer, with Show dimensions), Show
/// dependencies; New sketch, Copy, Create drawing and Export DXF/DWG of the clicked part's flat
/// pattern; Select other; Add comment; Zoom to fit, Zoom to selection, View normal to; Delete
/// the part.
fn open_flat_menu(world: &mut World, position: Vec2) {
    use cadrs_ui::menu::MenuEntry;
    let (part, others, at) = {
        let t = world.resource::<SmTable>();
        (t.menu_part, t.menu_others.clone(), t.menu_point)
    };
    let name = part.and_then(|p| flat_name(world, p));
    let model = shown(world.resource::<SmTable>(), world.resource::<PartCache>()).map(|c| c.feature);
    let model_name = model.and_then(|m| feature_name(world, m));
    let sketch = at.and_then(|(p, tol)| flat_sketch_at(world, p, tol));
    let sketch_name = sketch.and_then(|f| feature_name(world, f));
    let dims_shown = sketch.is_some_and(|f| world.resource::<crate::feature_menu::ShownDimensions>().0.contains(&f));
    let off = name.is_none();
    let of = name.clone().unwrap_or_else(|| "flat pattern".into());
    let tip = "Right-click a flat pattern";
    let with_tip = |i: MenuItem| if off { i.tooltip(tip) } else { i };
    let others_entries: Vec<MenuEntry> = {
        let cache = world.resource::<PartCache>();
        let ctx = shown(world.resource::<SmTable>(), cache);
        let mut v: Vec<MenuEntry> = Vec::new();
        if let Some(n) = &name {
            v.push(MenuEntry::Item(MenuItem::new("smt-select-other-part", n.clone())));
        }
        if let Some(ctx) = ctx {
            for (i, h) in others.iter().enumerate() {
                v.push(MenuEntry::Item(MenuItem::new(format!("smt-select-other-{i}"), hit_label(world, ctx, *h))));
            }
        }
        v
    };
    let mut entries = Vec::new();
    // The model's (and the flat sketch's) own items.
    let m = model_name.clone().unwrap_or_else(|| "sheet metal model".into());
    let mut edit_model = MenuItem::new("smt-flat-edit-model", format!("Edit {m}…")).icon("edit").disabled(model_name.is_none());
    if model_name.is_none() {
        edit_model = edit_model.tooltip("The sheet metal model is derived from another Part Studio");
    }
    entries.push(MenuEntry::Item(edit_model));
    if let Some(sn) = &sketch_name {
        entries.push(MenuEntry::Item(MenuItem::new("smt-flat-edit-sketch", format!("Edit {sn}…")).icon("sketch")));
        entries.push(MenuEntry::Item(MenuItem::new("smt-flat-dimensions", if dims_shown { "Hide dimensions" } else { "Show dimensions" }).icon("dimension")));
    }
    entries.push(MenuEntry::Item(MenuItem::new("smt-flat-dependencies", format!("Show dependencies of {m}…")).disabled(model_name.is_none())));
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::Item(with_tip(MenuItem::new("smt-flat-sketch", format!("New sketch on {of}…")).icon("sketch").disabled(off))));
    entries.push(MenuEntry::Item(with_tip(MenuItem::new("smt-flat-copy", format!("Copy {of}")).icon("copy").disabled(off))));
    entries.push(MenuEntry::Item(with_tip(MenuItem::new("smt-flat-drawing", format!("Create drawing of {of}…")).icon("file-new").disabled(off))));
    entries.push(MenuEntry::Item(with_tip(MenuItem::new("smt-flat-export", format!("Export DXF/DWG of {of}…")).icon("file-export").disabled(off))));
    entries.push(MenuEntry::Separator);
    let empty = others_entries.is_empty();
    entries.push(MenuEntry::Item(MenuItem::new("smt-flat-select-other", "Select other…").submenu(others_entries).disabled(empty)));
    // Comments aren't in cadrs yet (the Parts list's Add comment is off too).
    entries.push(MenuEntry::Item(MenuItem::new("smt-flat-comment", "Add comment").icon("comments").disabled(true)));
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::Item(MenuItem::new("smt-zoom-fit", "Zoom to fit")));
    entries.push(MenuEntry::Item(MenuItem::new("smt-zoom-selection", "Zoom to selection").disabled(off && world.resource::<Selection>().0.is_empty())));
    entries.push(MenuEntry::Item(MenuItem::new("smt-normal-to", "View normal to")));
    entries.push(MenuEntry::Separator);
    // As Onshape's: Delete the Sheet metal model feature.
    entries.push(MenuEntry::Item(MenuItem::new("smt-flat-delete", format!("Delete {m}")).icon("remove-circle").disabled(model_name.is_none())));
    let menu = Menu::new("smt-flat-menu").min_width(280.0).item_height(22.0).entries(entries);
    let theme = world.resource::<Theme>().clone();
    let mut cm = world.commands();
    let anchor = open_context_menu(&mut cm, position, menu.build(&theme));
    cm.entity(anchor).insert((FlatViewMenu, MenuSketch(sketch), DespawnOnExit(AppState::Document)));
    world.flush();
}

/// The flat sketch the flat view's menu was opened on.
#[derive(Component, Clone, Copy)]
struct MenuSketch(Option<FeatureId>);

/// Zoom to selection in the flat view: the selected walls and parts (as filled there), else
/// the part the menu is on.
fn zoom_to_selection(world: &mut World) {
    let size = body_size(world);
    let mut extra: Vec<P2> = Vec::new();
    let polys: Vec<cadrs_sheetmetal::poly::Polygon> = {
        let t = world.resource::<SmTable>();
        let cache = world.resource::<PartCache>();
        let (Some(scene), Some(ctx)) = (t.scene.as_ref(), shown(t, cache)) else { return };
        let mut v = Vec::new();
        for p in &world.resource::<Selection>().0 {
            // Edges and vertices: where they lie in the flat (a vertex with some room round it).
            if matches!(p, Pick::Edge(..) | Pick::Vertex(..))
                && let Some(FlatMark::Lines(ls)) = flat_mark(cache, ctx, scene, *p)
            {
                for q in ls.into_iter().flatten() {
                    let r = 5.0 * ctx.model.params.thickness.max(1.0);
                    extra.extend([q + cadrs_sheetmetal::poly::V2::new(-r, -r), q + cadrs_sheetmetal::poly::V2::new(r, r)]);
                }
            }
            match fill_of(cache, ctx, *p) {
                Some(FillOf::Wall(w)) => v.extend(scene.wall_region(w)),
                Some(FillOf::Part(i)) => v.extend(scene.part_region(i)),
                None => {}
            }
            if let Some((_, FlatHit::Joint(j))) = hit_of_pick(cache, *p) {
                v.extend(scene.bends.iter().filter(|b| b.joint == j).flat_map(|b| b.region.iter().cloned()));
            }
        }
        if v.is_empty()
            && extra.is_empty()
            && let Some(i) = t.menu_part
        {
            v = scene.part_region(i);
        }
        v
    };
    let Some(size) = size else { return };
    let mut t = world.resource_mut::<SmTable>();
    let z = t.scene.as_ref().map_or(0.0, |s| s.thickness as f32);
    let pts: Vec<Vec3> = polys.iter().flat_map(|p| p.outer.iter().copied()).chain(extra).map(|q| Vec3::new(q.x as f32, q.y as f32, z)).collect();
    if pts.is_empty() {
        return;
    }
    let cube = crate::view_cube::CUBE_WIDGET;
    t.view = t.view.fitted(&pts, Vec2::new((size.x - cube.x).max(1.0), size.y), 0.8);
    t.view.pan(Vec2::new(-cube.x / 2.0, 0.0));
}

fn on_menu_action(
    ev: On<MenuAction>,
    q: Query<&MenuFor, With<ContextMenuAnchor>>,
    q_drop: Query<&DropdownFor, With<ContextMenuAnchor>>,
    q_flat: Query<&MenuSketch, (With<FlatViewMenu>, With<ContextMenuAnchor>)>,
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
    if let Ok(MenuSketch(sketch)) = q_flat.get(ev.entity) {
        let item = ev.item.to_string();
        let sketch = *sketch;
        commands.queue(move |world: &mut World| {
            let model = shown(world.resource::<SmTable>(), world.resource::<PartCache>()).map(|c| c.feature);
            match item.as_str() {
                "smt-zoom-fit" => zoom_to_fit(world),
                "smt-zoom-selection" => zoom_to_selection(world),
                "smt-flat-edit-model" => {
                    if let Some(m) = model {
                        crate::document::edit_feature(world, m);
                    }
                }
                "smt-flat-edit-sketch" => {
                    if let Some(f) = sketch {
                        crate::document::edit_feature(world, f);
                    }
                }
                "smt-flat-dimensions" => {
                    if let Some(f) = sketch {
                        crate::feature_menu::toggle_dimensions(world, f);
                    }
                }
                "smt-flat-dependencies" => {
                    if let Some(m) = model {
                        crate::feature_list::show_dependencies(world, m);
                    }
                }
                "smt-flat-delete" => {
                    let (Some(m), Some(element)) = (model, world.get_resource::<ActiveDocument>().and_then(|d| d.active)) else { return };
                    if crate::linked_session::refuse(world) {
                        return;
                    }
                    let label = format!("Delete {}", feature_name(world, m).unwrap_or_default());
                    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
                        && let Err(e) = doc.execute(&cadrs_core::commands::DeleteFeature { element, feature: m, label })
                    {
                        warn!("cannot delete the sheet metal model: {e}");
                    }
                    world.resource_mut::<Selection>().0.clear();
                }
                "smt-normal-to" => {
                    {
                        let mut t = world.resource_mut::<SmTable>();
                        t.view = t.view.oriented(StandardView::Top);
                    }
                    zoom_to_fit(world);
                }
                "smt-flat-drawing" => create_flat_drawing(world),
                "smt-flat-export" => {
                    if let Some((_, part, _)) = flat_part(world) {
                        crate::flat_export_dialog::open(world, part);
                    }
                }
                "smt-flat-sketch" => {
                    if let Some((_, part, model)) = flat_part(world) {
                        // P3I.6: a sketch on the flat pattern plane of the part.
                        let index = crate::flat_export_dialog::flat_ref(world, part).map_or(0, |r| r.index);
                        crate::flat_ui::begin_flat_sketch(world, model, index);
                    }
                }
                "smt-flat-copy" => copy_flat(world),
                "smt-select-other-part" => {
                    if let Some((_, part, _)) = flat_part(world) {
                        toggle_picks(world, vec![Pick::Part(part)]);
                    }
                }
                other => {
                    if let Some(i) = other.strip_prefix("smt-select-other-").and_then(|n| n.parse::<usize>().ok())
                        && let Some(h) = world.resource::<SmTable>().menu_others.get(i).copied()
                    {
                        toggle_hit(world, h);
                    }
                }
            }
        });
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

/// The part the flat view's menu was opened on (right-clicked), with its Part Studio and its
/// model: what the menu's actions act on.
fn flat_part(world: &World) -> Option<(cadrs_core::ElementId, PartId, FeatureId)> {
    let t = world.resource::<SmTable>();
    let ctx = shown(t, world.resource::<PartCache>())?;
    let (part, _) = ctx.parts.get(t.menu_part?)?;
    let el = world.get_resource::<ActiveDocument>()?.active?;
    Some((el, *part, ctx.feature))
}

/// Copy Flat pattern of <part>: its outline (and tear slits) as sketch lines on the sketch
/// clipboard, to paste into a sketch with Ctrl+V (as Copy sketch).
fn copy_flat(world: &mut World) {
    let Some(i) = world.resource::<SmTable>().menu_part else { return };
    let Some(name) = flat_name(world, i) else { return };
    let sketch = {
        let Some(ctx) = shown(world.resource::<SmTable>(), world.resource::<PartCache>()) else { return };
        let Some(part) = ctx.flat.parts.get(i) else { return };
        let mut g = cadrs_sketch::Sketch::new();
        let v = |q: P2| cadrs_sketch::Vec2::new(q.x, q.y);
        for poly in &part.outline {
            for l in std::iter::once(&poly.outer).chain(&poly.holes) {
                for k in 0..l.len() {
                    let (a, b) = (l[k], l[(k + 1) % l.len()]);
                    if (b - a).norm() > 1e-9 {
                        g.add_line(v(a), v(b));
                    }
                }
            }
        }
        for s in part.slits() {
            g.add_line(v(s.a), v(s.b));
        }
        g
    };
    world.resource_mut::<crate::feature_menu::SketchClipboard>().0 = Some((name.clone(), sketch));
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    cadrs_ui::show_notification(&mut commands, &theme, cadrs_ui::Notification::info(format!("{name} copied. Paste it into a sketch with Ctrl+V.")).name("smt-flat-copied-toast"));
    world.flush();
}

/// Create drawing of flat pattern (SM16.1), from the flat view's menu.
fn create_flat_drawing(world: &mut World) {
    let Some((el, part, _)) = flat_part(world) else { return };
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
        // The cube's box sits in the top-right corner: the flat goes left of it or under it,
        // whichever shows it larger, centred in what is left.
        let cube = crate::view_cube::CUBE_WIDGET;
        let beside = Vec2::new((size.x - cube.x).max(1.0), size.y);
        let under = Vec2::new(size.x, (size.y - cube.y).max(1.0));
        let a = t.view.fitted(&pts, beside, 0.9);
        let b = t.view.fitted(&pts, under, 0.9);
        let mut v = if a.scale <= b.scale { a } else { b };
        if a.scale <= b.scale {
            v.pan(Vec2::new(-cube.x / 2.0, 0.0));
        } else {
            v.pan(Vec2::new(0.0, cube.y / 2.0));
        }
        t.view = v;
    }
}

fn fit_on_open(world: &mut World) {
    {
        let mut t = world.resource_mut::<SmTable>();
        match t.main_fit_in {
            Some(0) => {
                t.main_fit_in = None;
                crate::viewport::zoom_to_fit(world);
            }
            Some(n) => t.main_fit_in = Some(n - 1),
            None => {}
        }
    }
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
    (selection, extra): (Res<Selection>, Res<ExtraHighlight>),
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    q: Query<Entity, With<FlatMesh>>,
    q_menu: Query<(), (With<FlatViewMenu>, With<ContextMenuAnchor>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    // The Modify joint dialog's joint (`ExtraHighlight::selected`) shows selected here too.
    let selection = Selection(selection.0.iter().chain(extra.selected.iter()).copied().collect());
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
    // The joint hovered in the panel or in the main view.
    let hovered_joint = t.hovered.and_then(FlatHit::joint).or(t.model_hover.and_then(|(_, h)| h.joint()));
    // The part the flat view's menu is open on is orange all over (lesson 15, t0026.5).
    let menu_part = t.menu_part.filter(|_| !q_menu.is_empty());
    // Walls and parts filled orange (lesson 13 t0052, lesson 15 t0026.5): selected ones (a
    // wall's face or a part picked anywhere) in the selection orange, a hovered one paler.
    let mut fills: Vec<(FillOf, bool)> = Vec::new();
    if let Some(c) = &ctx {
        for p in &selection.0 {
            if let Some(f) = fill_of(&cache, c, *p) {
                fills.push((f, true));
            }
        }
        let hovered = match t.hovered {
            Some(FlatHit::Wall(w)) => Some(FillOf::Wall(w)),
            Some(FlatHit::Pick(p)) => fill_of(&cache, c, p),
            _ => t.model_hover.and_then(|(p, _)| fill_of(&cache, c, p)),
        };
        if let Some(h) = hovered
            && !fills.iter().any(|(f, _)| *f == h)
        {
            fills.push((h, false));
        }
    }
    let v = t.view;
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        want_scene.hash(&mut h);
        selected.hash(&mut h);
        hovered_joint.hash(&mut h);
        menu_part.hash(&mut h);
        fills.hash(&mut h);
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
    spawn(cadrs_sheetmetal::view::slab(&all, 0.0, tz), [0.55, 0.56, 0.58]);
    let lift = tz * 1.0 + 1e-3 * scene.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0);
    for (f, sel) in &fills {
        let polys = match f {
            FillOf::Wall(w) => scene.wall_region(*w),
            FillOf::Part(i) => scene.part_region(*i),
        };
        let s = (if *sel { ORANGE_MENU } else { ORANGE_HOVER }).to_srgba();
        spawn(cadrs_sheetmetal::view::fill(&polys, tz + 1e-4 * lift), [s.red, s.green, s.blue]);
    }
    if let Some(i) = menu_part {
        let s = ORANGE_MENU.to_srgba();
        spawn(cadrs_sheetmetal::view::slab(&scene.part_region(i), 0.0, tz + 1e-3 * lift), [s.red, s.green, s.blue]);
    }
    for b in &scene.bends {
        let c = if selected.contains(&b.joint) {
            ORANGE
        } else if hovered_joint == Some(b.joint) {
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

/// Outlines, dashed bend centrelines, tangent lines, forms (SM20.3), sketches on the flat
/// (P3I.6, SM14) and selected or hovered rips.
#[allow(clippy::too_many_arguments)]
fn draw_flat(
    t: Res<SmTable>,
    cache: Res<PartCache>,
    (selection, extra): (Res<Selection>, Res<ExtraHighlight>),
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    doc: Option<Res<ActiveDocument>>,
    mut lines: Gizmos<FlatLineGizmos>,
    mut hi: Gizmos<FlatHighlightGizmos>,
) {
    // The Modify joint dialog's joint (`ExtraHighlight::selected`) shows selected here too.
    let selection = Selection(selection.0.iter().chain(extra.selected.iter()).copied().collect());
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
    // Forms: their outlines and a centermark.
    let form = Color::srgb_u8(0x1f, 0x5f, 0xa8);
    for (ls, c) in &scene.forms {
        for l in ls {
            let pts = l.points.iter().chain(l.closed.then(|| l.points.first()).flatten());
            lines.linestrip(pts.map(|q| v3(*q, z)), form);
        }
        let r = 3.0 * px;
        lines.line(v3(*c - cadrs_sheetmetal::poly::V2::new(r, 0.0), z), v3(*c + cadrs_sheetmetal::poly::V2::new(r, 0.0), z), form);
        lines.line(v3(*c - cadrs_sheetmetal::poly::V2::new(0.0, r), z), v3(*c + cadrs_sheetmetal::poly::V2::new(0.0, r), z), form);
    }
    // Sketches on the flat pattern (in the flat's coordinates, moved with their part).
    if let Some(el) = doc.as_ref().and_then(|d| d.active_element()) {
        let features = el.active_features();
        let sketch_color = Color::srgb_u8(0x2b, 0x6c, 0xd8);
        for f in &features {
            let Some(sk) = f.sketch() else { continue };
            let Some(cadrs_sketch::PlaneRef::Feature(fp)) = sk.plane else { continue };
            let Some((model, index)) = cadrs_core::sheetmetal_flat::flat_target(&features, fp.feature) else { continue };
            if model != ctx.feature {
                continue;
            }
            let shift = scene.shifts.get(index).copied().unwrap_or_else(cadrs_sheetmetal::poly::V2::zeros);
            for l in sketch_polylines(&sk.geometry) {
                lines.linestrip(l.into_iter().map(|q| v3(cadrs_sheetmetal::poly::P2::new(q.x, q.y) + shift, z + 1e-3)), sketch_color);
            }
        }
    }
    let hovered_joint = t.hovered.and_then(FlatHit::joint).or(t.model_hover.and_then(|(_, h)| h.joint()));
    for j in &scene.joints {
        let c = if selected.contains(&j.joint) {
            ORANGE
        } else if hovered_joint == Some(j.joint) {
            ORANGE_HOVER
        } else {
            continue;
        };
        for s in &j.edges {
            hi.line(v3(s.a, z), v3(s.b, z), c);
        }
    }
    // Walls (SM1.4): a wall hovered in the flat is outlined; a face, edge or vertex of a wall
    // selected or hovered in the model shows where it lies in the flat; a selected part is
    // outlined whole.
    let zz = z + 1e-3;
    let outline = |hi: &mut Gizmos<FlatHighlightGizmos>, polys: &[cadrs_sheetmetal::poly::Polygon], c: Color| {
        for p in polys {
            for l in std::iter::once(&p.outer).chain(&p.holes) {
                hi.linestrip(l.iter().chain(l.first()).map(|q| v3(*q, zz)), c);
            }
        }
    };
    if let Some(FlatHit::Wall(w)) = t.hovered {
        outline(&mut hi, &scene.wall_region(w), ORANGE_HOVER);
    }
    let mark = |hi: &mut Gizmos<FlatHighlightGizmos>, m: FlatMark, c: Color| match m {
        FlatMark::Joint => {}
        FlatMark::Part(i) => outline(hi, &scene.part_region(i), c),
        FlatMark::Lines(ls) => {
            for l in ls {
                if l.len() == 1 {
                    // A vertex: a small ring.
                    let r = 4.0 * px;
                    let o = l[0];
                    hi.linestrip((0..=16).map(|k| {
                        let a = k as f64 / 16.0 * std::f64::consts::TAU;
                        v3(o + cadrs_sheetmetal::poly::V2::new(r * a.cos(), r * a.sin()), zz)
                    }), c);
                } else {
                    hi.linestrip(l.iter().map(|q| v3(*q, zz)), c);
                }
            }
        }
    };
    // Faces are filled (`sync_meshes`); their outline is drawn over the fill.
    for p in &selection.0 {
        if let Some(m) = flat_mark(&cache, ctx, scene, *p) {
            mark(&mut hi, m, ORANGE);
        }
    }
    let hovered_pick = match t.hovered {
        Some(FlatHit::Pick(p)) => Some(p),
        _ => t.model_hover.map(|(p, _)| p),
    };
    if let Some(p) = hovered_pick
        && !selection.0.contains(&p)
        && let Some(m) = flat_mark(&cache, ctx, scene, p)
    {
        mark(&mut hi, m, ORANGE_HOVER);
    }
}

/// A sketch's curves as polylines (its own coordinates).
fn sketch_polylines(g: &cadrs_sketch::Sketch) -> Vec<Vec<cadrs_sketch::Vec2>> {
    let mut out = Vec::new();
    for (id, c) in g.curves.iter() {
        match c.kind {
            cadrs_sketch::CurveKind::Line { a, b } => out.push(vec![g.pos(a), g.pos(b)]),
            cadrs_sketch::CurveKind::Circle { center, radius } => {
                let o = g.pos(center);
                out.push((0..=48).map(|k| {
                    let a = k as f64 / 48.0 * std::f64::consts::TAU;
                    cadrs_sketch::Vec2::new(o.x + radius * a.cos(), o.y + radius * a.sin())
                }).collect());
            }
            cadrs_sketch::CurveKind::Arc { .. } => {
                if let Some(a) = g.arc_geom(id) {
                    out.push((0..=24).map(|k| a.point_at(a.start_angle + a.sweep * k as f64 / 24.0)).collect());
                }
            }
            _ => {}
        }
    }
    out
}

fn draw_cube_arcs(t: Res<SmTable>, open: Res<SidePanel>, kind: Res<ActiveKind>, theme: Res<Theme>, mut arcs: Gizmos<FlatCubeArcGizmos>) {
    use crate::view_cube::CubeColors;
    if is_open(&open, &kind) && t.key.is_some() {
        crate::view_cube::draw_cube_arcs(&mut arcs, &t.view, theme.view_cube_arrow());
    }
}

/// A label's text colour: orange when its joint is selected, pale orange when hovered.
fn label_color(j: JointId, selected: &[JointId], hovered: Option<JointId>, theme: &Theme) -> Color {
    if selected.contains(&j) {
        Color::srgb(0.85, 0.42, 0.05)
    } else if hovered == Some(j) {
        Color::srgb(0.95, 0.55, 0.15)
    } else {
        theme.foreground
    }
}

/// The joints' labels follow the flat view, each beside its spot on its own side (a bend's
/// beside the middle of its centre line, a rip's off the sheet); the selected ones turn orange.
#[allow(clippy::type_complexity)]
fn place_labels(
    t: Res<SmTable>,
    cache: Res<PartCache>,
    (selection, extra): (Res<Selection>, Res<ExtraHighlight>),
    q_body: Query<&ComputedNode, With<FlatBody>>,
    mut q: Query<(&FlatLabel, &mut Node, &mut Visibility, &ComputedNode, &Children)>,
    mut q_text: Query<&mut TextColor>,
    theme: Res<Theme>,
) {
    // The Modify joint dialog's joint (`ExtraHighlight::selected`) shows selected here too.
    let selection = Selection(selection.0.iter().chain(extra.selected.iter()).copied().collect());
    let Some(body) = q_body.iter().next() else { return };
    let Some(scene) = &t.scene else { return };
    let size = body.size() * body.inverse_scale_factor();
    let selected = shown(&t, &cache).map(|c| selected_joints(&cache, c, &selection)).unwrap_or_default();
    let hovered = t.hovered.and_then(FlatHit::joint).or(t.model_hover.and_then(|(_, h)| h.joint()));
    // The sheet's outlines on screen: labels keep off them (they would read as the edge's).
    let z = scene.thickness;
    let outline: Vec<(Vec2, Vec2)> = scene
        .outlines
        .iter()
        .flat_map(|p| std::iter::once(&p.outer).chain(&p.holes))
        .flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
        .map(|(a, b)| (t.view.project(v3(a, z)) + size / 2.0, t.view.project(v3(b, z)) + size / 2.0))
        .collect();
    let crosses = |r: &Rect| outline.iter().any(|(a, b)| segment_hits_rect(*a, *b, *r));
    // Bends first (their spot is the clearer one), then rips; each label tries its places (a
    // rip has one by each of its edges), moved further out along its direction or slid along
    // the line, for a spot inside the view that overlaps no label placed before and no outline.
    let mut items: Vec<_> = q.iter_mut().collect();
    items.sort_by_key(|(l, ..)| (scene.bends.iter().all(|b| b.joint != l.0), l.0 .0));
    let mut placed: Vec<Rect> = Vec::new();
    for (l, mut node, mut vis, cn, children) in items {
        let lsize = cn.size() * cn.inverse_scale_factor();
        let step = lsize.y.max(8.0);
        let mut tries: Vec<(f32, f32)> = Vec::new();
        for o in 0..4 {
            for sl in [0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0] {
                tries.push((o as f32 * step * 0.5, sl * step));
            }
        }
        let mut cands: Vec<Rect> = Vec::new();
        for (spot, dir) in &l.1 {
            let p = t.view.project(v3(*spot, z)) + size / 2.0;
            // The label's box sits on the side its direction points to, 4 px off the spot.
            let d = t.view.project_vector(Vec3::new(dir.x as f32, dir.y as f32, 0.0)).normalize_or_zero();
            let along = Vec2::new(-d.y, d.x);
            let reach = (d.x.abs() * lsize.x + d.y.abs() * lsize.y) / 2.0;
            for (o, sl) in &tries {
                let c = p + d * (4.0 + reach + o) + along * *sl;
                cands.push(Rect::from_center_size(c, lsize + Vec2::splat(2.0)));
            }
        }
        let inside = |r: &Rect| r.min.x >= 0.0 && r.min.y >= 0.0 && r.max.x <= size.x && r.max.y <= size.y;
        let free = |r: &Rect| placed.iter().all(|q| q.intersect(*r).is_empty());
        let Some(&first) = cands.first() else { continue };
        let r = cands
            .iter()
            .find(|r| inside(r) && free(r) && !crosses(r))
            .or_else(|| cands.iter().find(|r| inside(r) && free(r)))
            .copied()
            .unwrap_or(first);
        placed.push(r);
        let (x, y) = (r.min.x + 1.0, r.min.y + 1.0);
        if node.left != Val::Px(x) || node.top != Val::Px(y) {
            node.left = Val::Px(x);
            node.top = Val::Px(y);
        }
        let inside = x >= 0.0 && y >= 0.0 && x + lsize.x <= size.x && y + lsize.y <= size.y;
        let want = if inside { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let c = label_color(l.0, &selected, hovered, &theme);
        for ch in children.iter() {
            if let Ok(mut tc) = q_text.get_mut(ch)
                && tc.0 != c
            {
                tc.0 = c;
            }
        }
    }
}

/// Whether segment `a`–`b` touches rectangle `r`.
fn segment_hits_rect(a: Vec2, b: Vec2, r: Rect) -> bool {
    if r.contains(a) || r.contains(b) {
        return true;
    }
    let cross = |p: Vec2, q: Vec2, u: Vec2, v: Vec2| {
        let d = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
        let (d1, d2, d3, d4) = (d(u, v, p), d(u, v, q), d(p, q, u), d(p, q, v));
        (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
    };
    let c = [r.min, Vec2::new(r.max.x, r.min.y), r.max, Vec2::new(r.min.x, r.max.y)];
    (0..4).any(|i| cross(a, b, c[i], c[(i + 1) % 4]))
}

/// The joints' labels by the folded model in the main view (SM13.1), while the panel is open:
/// each just off its joint (a bend's outside, halfway; a rip's middle).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place_folded_labels(
    t: Res<SmTable>,
    cache: Res<PartCache>,
    selection: Res<Selection>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    mut q: Query<(&FoldedLabel, &mut Node, &mut Visibility, &ComputedNode, &Children)>,
    mut q_text: Query<&mut TextColor>,
    theme: Res<Theme>,
) {
    let on = is_open(&open, &kind) && !cache.rebuilding;
    let selected = shown(&t, &cache).map(|c| selected_joints(&cache, c, &selection)).unwrap_or_default();
    let hovered = t.hovered.and_then(FlatHit::joint).or(t.model_hover.and_then(|(_, h)| h.joint()));
    let v = view.view;
    let size = rect.0.size();
    // Labels that would overlap one placed before move up or down a line.
    let mut items: Vec<_> = q.iter_mut().collect();
    items.sort_by_key(|(l, ..)| l.0 .0);
    let mut placed: Vec<Rect> = Vec::new();
    for (l, mut node, mut vis, cn, children) in items {
        let p = rect.to_screen(v.project(l.1)) - rect.0.min;
        let lsize = cn.size() * cn.inverse_scale_factor();
        let base = Vec2::new(p.x + 6.0, p.y - lsize.y - 4.0);
        let step = lsize.y + 2.0;
        let r = [0.0, -step, step, -2.0 * step, 2.0 * step]
            .iter()
            .map(|dy| Rect::from_corners(base + Vec2::new(0.0, *dy), base + Vec2::new(0.0, *dy) + lsize))
            .find(|r| placed.iter().all(|q| q.intersect(*r).is_empty()))
            .unwrap_or(Rect::from_corners(base, base + lsize));
        placed.push(r);
        let (x, y) = (r.min.x, r.min.y);
        if node.left != Val::Px(x) || node.top != Val::Px(y) {
            node.left = Val::Px(x);
            node.top = Val::Px(y);
        }
        let inside = x >= 0.0 && y >= 0.0 && x + lsize.x <= size.x && y + lsize.y <= size.y;
        let want = if on && inside { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let c = label_color(l.0, &selected, hovered, &theme);
        for ch in children.iter() {
            if let Ok(mut tc) = q_text.get_mut(ch)
                && tc.0 != c
            {
                tc.0 = c;
            }
        }
    }
}

/// The flat view cube's axis letters follow its view, as the main cube's do.
fn place_flat_axes(t: Res<SmTable>, mut q: Query<(&FlatAxisLabel, &ComputedNode, &mut Node, &mut TextColor)>) {
    for (axis, cn, mut node, mut color) in &mut q {
        let (p, a) = crate::view_cube::axis_label_spot(&t.view, axis.0, cn.size() * cn.inverse_scale_factor());
        if (color.0.alpha() - a).abs() > 1e-3 {
            color.0.set_alpha(a);
        }
        if node.left != Val::Px(p.x) || node.top != Val::Px(p.y) {
            node.left = Val::Px(p.x);
            node.top = Val::Px(p.y);
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

/// What a flat point picks, as the main view does: a model vertex at a wall outline's corner,
/// a bend or rip, a model edge along a wall outline's side, else the wall (its face).
fn flat_hit(cache: &PartCache, t: &SmTable, p: P2, tol: f64) -> Option<FlatHit> {
    let scene = t.scene.as_ref()?;
    let ctx = shown(t, cache);
    let outline = |corner: f64, side: f64| {
        let ctx = ctx?;
        let h = scene.outline_at(p, corner, side)?;
        // A rip's side is its model edge too (the rip is in the table and Select other).
        model_pick_at(cache, ctx, scene, h).map(FlatHit::Pick)
    };
    outline(tol, 0.0)
        .or_else(|| outline(0.0, tol * 0.8))
        .or_else(|| scene.joint_at(p, tol).map(FlatHit::Joint))
        .or_else(|| scene.wall_at(p).map(FlatHit::Wall))
}

fn on_flat_move(ev: On<Pointer<Move>>, mut t: ResMut<SmTable>, cache: Res<PartCache>, q: Query<(&ComputedNode, &UiGlobalTransform), With<FlatBody>>) {
    let hit = flat_point(&t, &q, ev.pointer_location.position).and_then(|(p, tol)| flat_hit(&cache, &t, p, tol));
    if t.flat_hover != hit {
        t.flat_hover = hit;
    }
}

fn on_flat_click(ev: On<Pointer<Click>>, t: Res<SmTable>, cache: Res<PartCache>, q: Query<(&ComputedNode, &UiGlobalTransform), With<FlatBody>>, mut commands: Commands) {
    if ev.button != PointerButton::Primary {
        return;
    }
    if let Some(h) = flat_point(&t, &q, ev.pointer_location.position).and_then(|(p, tol)| flat_hit(&cache, &t, p, tol)) {
        commands.queue(move |world: &mut World| toggle_hit(world, h));
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
