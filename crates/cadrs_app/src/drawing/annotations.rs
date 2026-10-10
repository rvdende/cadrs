//! Annotating views (P3C.3, D5, D6, X8): the centerline, centermark, virtual sharp, dimension and
//! hole callout tools, selecting, dragging and deleting annotations, and drawing them.
//!
//! - **Tools.** Centermark (click circles or arcs, D5.3); Centerline ▾ (D5.2): point to point,
//!   line to line (edge to edge: halfway between two edges, a hole's silhouettes), circle
//!   through 3 points or centre and point; Virtual sharp (two lines, D5.4); Dimension ▾ (D6.2,
//!   D6.4): smart **D** (the type from the picks: a circle's diameter, an arc's radius, a line's
//!   length, two parallel lines' distance, two lines' angle, two points' distance), Radial
//!   **Shift+R**, Diameter **Shift+D**, point to point, line to line, angular; Hole callout (D6.7).
//!   Hovering geometry with a tool shows its snap points in orange (D6.3): line ends and
//!   midpoints, arc ends and centres, circle centres; a click near one picks the point, else the
//!   edge. Once the picks make a dimension it follows the cursor in orange (D6.8) and a click
//!   places it. Esc drops the picks, then ends the tool.
//! - **Editing.** A click selects an annotation (orange, D6.8; Ctrl adds); dragging it moves its
//!   text. A selected annotation shows its grips: dragging a dimension's or callout's attachment
//!   grip onto other geometry re-attaches it and the value updates (D6.5), with the current
//!   attachment highlighted blue; dragging a centerline's end extends it. Delete removes the
//!   selection. Every change is one undoable drawing edit.
//! - **Drawing.** Thin black lines, filled arrowheads and Inter text (the ⌴ ⌵ ↧ symbols as
//!   vector strokes, see `cadrs_drawing::annotation`); orange while selected or being placed.
//! - A right-click on an annotation opens its menu (Edit… for hole callouts, Delete); the
//!   dimension palette and the Hole callout dialog are in [`super::dim_palette`].

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::text::FontWeight;
use cadrs_core::views::ViewGeometry;
use cadrs_drawing::annotation::{
    self as ann, AnnGraphics, Annotation, AnnotationId, AnnotationKind, Centerline, CenterlineKind, CircleCenterline,
    DimKind, DimTool, EdgeRef, GripKind, HoleCallout, Pick, PointOf, PointRef, Shape,
};
use cadrs_drawing::{Drawing, DrawingOp, View, ViewId};
use cadrs_ui::input::TextInputField;
use cadrs_ui::prelude::*;

use super::view_tools::{ViewTool, edit_drawing};
use super::views::{ViewCache, view_at};
use super::{DRAWING_LAYER, DrawingUi, SheetText, active_drawing, current_view, screen_to_sheet, sheet_area};
use crate::viewport::{ActiveKind, ViewportArea, ViewportRect};
use crate::{ActiveDocument, AppState};

pub struct AnnotationsPlugin;

/// The annotation systems that draw (after the views).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnnotationDrawSet;

/// The annotation systems that read the pointer and keys (before the notes', P3C.4).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnnotationInputSet;

impl Plugin for AnnotationsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnnotationUi>()
            .init_resource::<AnnotationScene>()
            .init_gizmo_group::<AnnotationLines>()
            .add_systems(Startup, configure_gizmos)
            .add_systems(
                Update,
                (annotation_pointer, annotation_keys)
                    .chain()
                    .in_set(AnnotationInputSet)
                    .after(super::drawing_keys)
                    .before(super::view_tools::ViewToolsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                (rebuild_scene, sync_texts, sync_fills, draw_strokes, sync_hint)
                    .chain()
                    .in_set(AnnotationDrawSet)
                    .after(super::views::ViewsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut u: ResMut<AnnotationUi>, mut s: ResMut<AnnotationScene>| {
                *u = AnnotationUi::default();
                *s = AnnotationScene::default();
            })
            .add_observer(on_annotation_menu)
            .add_observer(on_tool_menu);
    }
}

/// Annotation lines (thin, a device pixel at least).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct AnnotationLines;

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (cfg, _) = store.config_mut::<AnnotationLines>();
    cfg.render_layers = RenderLayers::layer(DRAWING_LAYER);
    // At least 1.5 px, so a line between pixel rows still reads dark.
    cfg.line.width = 1.5;
}

/// Annotation ink.
pub fn ink() -> Color {
    Color::srgb_u8(0x0a, 0x0a, 0x0a)
}

/// Selected or being placed (D6.8).
pub fn orange() -> Color {
    Color::srgb_u8(0xe8, 0x74, 0x0c)
}

/// Hover: a pale amber, lighter than the selection's orange (Onshape pre-highlights in a light
/// tint and selects in full orange with grips).
pub(crate) fn hover_orange() -> Color {
    Color::srgb_u8(0xf7, 0xb9, 0x5e)
}

/// The current attachment (D6.5).
/// Dangling annotations (P3C.6, D13.3): their geometry is gone (`ex3-step9.png`).
pub fn dangling_red() -> Color {
    Color::srgb_u8(0xe0, 0x3c, 0x1f)
}

pub(crate) fn blue() -> Color {
    Color::srgb_u8(0x1f, 0x7a, 0xe0)
}

// ---------------------------------------------------------------------------------------------
// State

/// Centerline modes (D5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CenterlineMode {
    PointToPoint,
    LineToLine,
    ThreePointCircle,
    TwoPointCircle,
}

/// The active annotation tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnnTool {
    #[default]
    None,
    Dimension(DimTool),
    HoleCallout,
    Centerline(CenterlineMode),
    Centermark,
    VirtualSharp,
    /// Note (N, P3C.4): a click on the sheet places a note, on geometry a leader first.
    Note,
    /// Table (P3C.4): a click places the table set in the Table dialog.
    Table,
    /// Add leader (D9.2): a pick adds a leader to the note.
    AddLeader(cadrs_drawing::NoteId),
    /// Sheet sketch Line (P3C.7, D2.4): click the start, then each end.
    SheetLine,
    /// Sheet sketch Spline: click its points; double-click or Enter finishes.
    SheetSpline,
    /// Placing an inserted DXF or image (it follows the cursor; a click places it).
    PlaceImport,
    /// GD&T feature control frame (P3C.8): pick an edge, then place (the card's settings).
    Gdt,
    /// Datum feature symbol.
    Datum,
    /// Surface finish symbol.
    SurfaceFinish,
    /// Weld symbol.
    Weld,
    /// Placing a BOM table (P3C.5, D11.2): it follows the cursor; a click places it.
    PlaceBom,
    /// Callout (P3C.5, D11.4): pick a part's edge, then place; again until ✓.
    Callout,
}

impl AnnTool {
    /// The symbol tools with a settings card (P3C.8).
    pub fn has_card(self) -> bool {
        matches!(self, AnnTool::Gdt | AnnTool::Datum | AnnTool::SurfaceFinish | AnnTool::Weld)
    }
}

/// What the tool would pick under the cursor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hover {
    pub view: ViewId,
    pub edge: EdgeRef,
    /// The edge's current shape (view 2D).
    pub shape: Shape,
    /// The snap point the cursor is on, if any.
    pub snap: Option<(PointOf, [f64; 2])>,
}

impl Hover {
    pub fn pick(&self) -> Pick {
        match self.snap {
            Some((of, at)) => Pick::Point(PointRef { edge: self.edge, of, hint: at }),
            None => Pick::Edge(self.edge),
        }
    }
}

/// A grip (or an annotation's body) being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GripDrag {
    pub view: ViewId,
    pub id: AnnotationId,
    pub grip: GripKind,
    pub start: Vec2,
    pub at: Vec2,
    pub moving: bool,
}

/// Annotation tool and selection state (not saved).
#[derive(Resource, Debug, Default)]
pub struct AnnotationUi {
    pub tool: AnnTool,
    /// The picks so far, all in `pick_view`.
    pub picks: Vec<Pick>,
    pub pick_view: Option<ViewId>,
    pub hover: Option<Hover>,
    /// The annotation the picks make, following the cursor.
    pub preview: Option<(ViewId, Annotation)>,
    pub selected: Vec<(ViewId, AnnotationId)>,
    pub hovered: Option<(ViewId, AnnotationId)>,
    pub drag: Option<GripDrag>,
    /// The drag's result so far (drawn instead of the annotation).
    pub drag_preview: Option<(ViewId, Annotation)>,
    /// The dimension palette is open (for the one selected dimension, D6.6).
    pub palette_open: bool,
    /// The hole callout being edited, as its dialog would leave it (drawn orange, live).
    pub callout_preview: Option<(ViewId, Annotation)>,
    /// The baseline or ordinate set that further picks extend (P3C.8).
    pub extending: Option<(ViewId, AnnotationId)>,
    /// Inference lines (P3C.5, D11.6): dashed guides from other callouts to the one being
    /// placed or dragged (sheet mm).
    pub guides: Vec<([f64; 2], [f64; 2])>,
}

impl AnnotationUi {
    /// Starts `tool` (or ends it when it is already active).
    pub fn toggle(&mut self, tool: AnnTool) {
        self.tool = if self.tool == tool { AnnTool::None } else { tool };
        self.extending = None;
        self.reset_picks();
        self.selected.clear();
        self.palette_open = false;
    }

    pub fn reset_picks(&mut self) {
        self.picks.clear();
        self.pick_view = None;
        self.preview = None;
    }
}

/// Starts an annotation tool from the toolbar or a shortcut: view tools end and the view
/// selection clears.
pub fn start_tool(world: &mut World, tool: AnnTool) {
    let mut ui = world.resource_mut::<DrawingUi>();
    ui.tool = ViewTool::None;
    ui.ghost = None;
    ui.highlight_edge = None;
    ui.selected.clear();
    world.resource_mut::<AnnotationUi>().toggle(tool);
    if tool.has_card() {
        super::symbol_cards::open(world, tool);
    }
}

// ---------------------------------------------------------------------------------------------
// Access

/// The active sheet's views with their current geometry.
pub fn sheet_views(doc: &ActiveDocument, ui: &DrawingUi, cache: &ViewCache) -> Vec<(View, Option<Arc<ViewGeometry>>)> {
    let Some((id, d)) = active_drawing(doc) else {
        return Vec::new();
    };
    super::views::shown_views(d, ui.sheet_index(id, d), ui)
        .into_iter()
        .map(|v| {
            let g = cache.geometry(&v);
            (v, g)
        })
        .collect()
}

/// An annotation of the active drawing.
pub fn find_annotation(d: &Drawing, view: ViewId, id: AnnotationId) -> Option<(View, Annotation)> {
    let (_, v) = d.view(view)?;
    let a = v.annotations.iter().find(|a| a.id == id)?.clone();
    Some((v.clone(), a))
}

fn mm_per_px(doc: &ActiveDocument, ui: &DrawingUi) -> f64 {
    current_view(doc, ui).map(|(_, v)| 1.0 / v.ppm as f64).unwrap_or(1.0)
}

/// The edges a view draws (the ones a tool can pick).
fn drawn_edges(v: &View, g: &ViewGeometry) -> Vec<usize> {
    use cadrs_drawing::style::TangentEdges;
    use cadrs_kernel::{ProjClass, ProjVisibility};
    g.projection
        .edges
        .iter()
        .enumerate()
        // A flat pattern's bend lines while they are shown (P3I.7).
        .filter(|(_, e)| !cadrs_drawing::flat_view::is_bend_edge(e) || v.flat.as_ref().is_some_and(|f| !f.bend_lines_hidden))
        .filter(|(_, e)| match (e.visibility, e.class) {
            (ProjVisibility::Visible, ProjClass::Smooth) => v.tangent_edges != TangentEdges::Hidden,
            (ProjVisibility::Visible, _) => true,
            (ProjVisibility::Hidden, ProjClass::Smooth) => false,
            (ProjVisibility::Hidden, _) => v.hidden_lines && !v.shaded,
        })
        .map(|(i, _)| i)
        .collect()
}

/// The geometry under the sheet point `p` in `v` that `accept` takes: the nearest snap point
/// within `snap_tol` (sheet mm), else the nearest drawn edge within `tol`. (A hole's circle next
/// to a nearer line is still found by a tool that only takes circles.)
pub(crate) fn hover_in(v: &View, g: &ViewGeometry, p: Vec2, tol: f64, snap_tol: f64, accept: impl Fn(&Hover) -> bool) -> Option<Hover> {
    let local = v.from_sheet([p.x as f64, p.y as f64]);
    let k = v.scale.factor();
    let mut snaps: Vec<(f64, Hover)> = Vec::new();
    let mut edges: Vec<(f64, Hover)> = Vec::new();
    for i in drawn_edges(v, g) {
        let e = &g.projection.edges[i];
        let r = EdgeRef::of(e);
        let res = ann::resolve(v, g, &r);
        let d = ann::proj_shape(e).distance(local).min(res.shape.distance(local)) * k;
        for (of, at) in res.shape.snap_points() {
            let ds = ((at[0] - local[0]).hypot(at[1] - local[1])) * k;
            // A circle's centre snaps from anywhere near it; ends and midpoints only on the edge.
            let near_edge = d <= snap_tol * 2.5 || of == PointOf::Center;
            if ds <= snap_tol && near_edge {
                snaps.push((ds, Hover { view: v.id, edge: r, shape: res.shape, snap: Some((of, at)) }));
            }
        }
        if d <= tol {
            edges.push((d, Hover { view: v.id, edge: r, shape: res.shape, snap: None }));
        }
    }
    snaps.sort_by(|a, b| a.0.total_cmp(&b.0));
    edges.sort_by(|a, b| a.0.total_cmp(&b.0));
    snaps.into_iter().chain(edges).map(|(_, h)| h).find(|h| accept(h))
}

/// What the tool accepts under the cursor.
fn tool_accepts(tool: AnnTool, v: &View, g: &ViewGeometry, picks: &[Pick], h: &Hover) -> Option<Pick> {
    let pick = h.pick();
    let is_circle = matches!(h.shape, Shape::Circle { .. });
    let is_line = matches!(h.shape, Shape::Line { .. });
    let center = || {
        is_circle.then_some(Pick::Point(PointRef { edge: h.edge, of: PointOf::Center, hint: [0.0, 0.0] }))
    };
    match tool {
        AnnTool::None | AnnTool::Table | AnnTool::SheetLine | AnnTool::SheetSpline | AnnTool::PlaceImport | AnnTool::PlaceBom => None,
        AnnTool::Callout => Some(Pick::Edge(h.edge)),
        AnnTool::Gdt | AnnTool::Datum | AnnTool::SurfaceFinish | AnnTool::Weld => Some(Pick::Edge(h.edge)),
        AnnTool::Dimension(DimTool::Chamfer) => is_line.then_some(Pick::Edge(h.edge)),
        AnnTool::Dimension(DimTool::ArcLength) => {
            matches!(h.shape, Shape::Circle { arc: Some(_), .. }).then_some(Pick::Edge(h.edge))
        }
        AnnTool::Dimension(DimTool::Baseline) => Some(match pick {
            Pick::Edge(_) if is_circle => center()?,
            p => p,
        }),
        AnnTool::Dimension(DimTool::Ordinate) => match pick {
            Pick::Point(_) => Some(pick),
            Pick::Edge(_) if is_circle => center(),
            Pick::Edge(_) if is_line => Some(Pick::Point(PointRef { edge: h.edge, of: PointOf::Mid, hint: h.shape.polyline()[0] })),
            _ => None,
        },
        AnnTool::Note | AnnTool::AddLeader(_) => Some(pick),
        AnnTool::Dimension(t) => {
            // Radial and diameter take the circle even when its centre snaps, line-to-line and
            // angular the line even near its ends or middle.
            let whole = match t {
                DimTool::Radial | DimTool::Diameter => is_circle,
                DimTool::LineToLine | DimTool::Angular => is_line,
                _ => false,
            };
            let pick = if whole { Pick::Edge(h.edge) } else { pick };
            ann::accepts(t, v, g, picks, &pick).then_some(pick)
        }
        AnnTool::HoleCallout => (is_circle && ann::hole_of(g, &h.edge).is_some()).then_some(Pick::Edge(h.edge)),
        AnnTool::Centermark => is_circle.then_some(Pick::Edge(h.edge)),
        AnnTool::VirtualSharp => is_line.then_some(Pick::Edge(h.edge)),
        AnnTool::Centerline(CenterlineMode::LineToLine) => is_line.then_some(Pick::Edge(h.edge)),
        AnnTool::Centerline(_) => match pick {
            Pick::Point(_) => Some(pick),
            Pick::Edge(_) => center(),
        },
    }
}

/// The annotation the tool makes once it has enough picks (with the text at `cursor`).
fn complete(tool: AnnTool, v: &View, g: &ViewGeometry, picks: &[Pick], cursor: [f64; 2], specs: &super::symbol_cards::SymbolSpecs) -> Option<Annotation> {
    use cadrs_drawing::annotation_more as more;
    let point = |p: &Pick| match p {
        Pick::Point(pr) => Some(*pr),
        Pick::Edge(_) => None,
    };
    let kind = match (tool, picks) {
        (AnnTool::Dimension(DimTool::Chamfer), [p]) => AnnotationKind::ChamferDim(more::ChamferDim { edge: *p.edge(), text: cursor }),
        (AnnTool::Dimension(DimTool::ArcLength), [p]) => AnnotationKind::ArcLength(more::ArcLength { edge: *p.edge(), text: cursor }),
        (AnnTool::Dimension(DimTool::Baseline), [base, target]) => {
            let at = |p: &Pick| match p {
                Pick::Point(pr) => ann::resolve_point(v, g, pr).map(|x| x.0),
                Pick::Edge(e) => ann::resolve(v, g, e).shape.polyline().first().copied(),
            };
            let (a, b) = (at(base)?, at(target)?);
            let (sa, sb) = (v.to_sheet(a), v.to_sheet(b));
            let orient = if (sb[0] - sa[0]).abs() >= (sb[1] - sa[1]).abs() { ann::Orient::Horizontal } else { ann::Orient::Vertical };
            AnnotationKind::Baseline(more::Baseline { base: *base, targets: vec![*target], orient, text: cursor, spacing: 2.6 * cadrs_drawing::DrawingStyle::default().dim_text_height })
        }
        (AnnTool::Dimension(DimTool::Ordinate), [Pick::Point(o)]) => {
            let (p, _) = ann::resolve_point(v, g, o)?;
            let (sp, sc) = (v.to_sheet(p), v.to_sheet(cursor));
            // Values above or below: x ordinates (vertical leaders); beside: y ordinates.
            let vertical = (sc[0] - sp[0]).abs() > (sc[1] - sp[1]).abs();
            AnnotationKind::Ordinate(more::Ordinate { origin: *o, points: Vec::new(), vertical, level: if vertical { cursor[0] } else { cursor[1] } })
        }
        (AnnTool::Gdt, [p]) => AnnotationKind::FeatureControl(more::FeatureControl {
            characteristic: specs.gdt,
            tolerance: specs.tolerance.clone(),
            diameter: specs.diameter,
            modifier: specs.modifier,
            datums: if specs.gdt.takes_datums() { specs.datums.iter().filter(|d| !d.is_empty()).cloned().collect() } else { Vec::new() },
            edge: Some(*p.edge()),
            text: cursor,
        }),
        (AnnTool::Datum, [p]) => AnnotationKind::Datum(more::Datum { letter: specs.datum_letter.clone(), edge: *p.edge(), text: cursor }),
        (AnnTool::SurfaceFinish, [p]) => {
            AnnotationKind::SurfaceFinish(more::SurfaceFinish { kind: specs.finish, value: specs.finish_value.clone(), edge: *p.edge(), text: cursor })
        }
        (AnnTool::Weld, [p]) => AnnotationKind::Weld(more::Weld {
            edge: *p.edge(),
            text: cursor,
            arrow_side: specs.weld_arrow,
            other_side: specs.weld_other,
            size: specs.weld_size.clone(),
            all_around: specs.all_around,
        }),
        (AnnTool::Dimension(t), _) => AnnotationKind::Dimension(ann::propose(t, v, g, picks, cursor)?),
        (AnnTool::HoleCallout, [p]) => AnnotationKind::HoleCallout(HoleCallout { edge: *p.edge(), text: cursor, prefix: String::new(), last: None }),
        (AnnTool::Centermark, [p]) => AnnotationKind::Centermark(*p.edge()),
        (AnnTool::VirtualSharp, [a, b]) => AnnotationKind::VirtualSharp { a: *a.edge(), b: *b.edge() },
        (AnnTool::Centerline(CenterlineMode::LineToLine), [a, b]) => AnnotationKind::Centerline(Centerline {
            kind: CenterlineKind::Lines { a: *a.edge(), b: *b.edge() },
            extend: [0.0, 0.0],
        }),
        (AnnTool::Centerline(CenterlineMode::PointToPoint), [a, b]) => AnnotationKind::Centerline(Centerline {
            kind: CenterlineKind::Points { a: point(a)?, b: point(b)? },
            extend: [0.0, 0.0],
        }),
        (AnnTool::Centerline(CenterlineMode::ThreePointCircle), [a, b, c]) => {
            AnnotationKind::CircleCenterline(CircleCenterline::ThreePoints([point(a)?, point(b)?, point(c)?]))
        }
        (AnnTool::Centerline(CenterlineMode::TwoPointCircle), [a, b]) => {
            AnnotationKind::CircleCenterline(CircleCenterline::CenterPoint { center: point(a)?, on: point(b)? })
        }
        _ => return None,
    };
    let a = Annotation::new(kind);
    // It has to draw.
    let style = cadrs_drawing::DrawingStyle::default();
    ann::annotation_graphics(&style, v, g, &a).map(|_| a)
}

/// Whether the tool places its annotation with a click of its own (dimensions and callouts),
/// rather than on the last pick.
fn placed(tool: AnnTool) -> bool {
    matches!(tool, AnnTool::Dimension(_) | AnnTool::HoleCallout) || tool.has_card()
}

/// The picks a tool needs before it makes its annotation.
fn needs(tool: AnnTool) -> usize {
    match tool {
        AnnTool::Centermark | AnnTool::HoleCallout => 1,
        AnnTool::Dimension(DimTool::Chamfer | DimTool::ArcLength | DimTool::Ordinate) => 1,
        t if t.has_card() => 1,
        AnnTool::Centerline(CenterlineMode::ThreePointCircle) => 3,
        AnnTool::Dimension(_) => 2,
        _ => 2,
    }
}

// ---------------------------------------------------------------------------------------------
// Pointer

/// The annotation or grip under a sheet point.
pub fn annotation_at(scene: &AnnotationScene, p: Vec2, tol: f64) -> Option<(ViewId, AnnotationId)> {
    let q = [p.x as f64, p.y as f64];
    scene
        .items
        .iter()
        .rev()
        .map(|(v, id, g)| (g.distance(q), *v, *id))
        .filter(|(d, _, _)| *d <= tol)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, v, id)| (v, id))
}

fn grip_at(scene: &AnnotationScene, sel: &[(ViewId, AnnotationId)], p: Vec2, tol: f64) -> Option<(ViewId, AnnotationId, GripKind)> {
    let q = [p.x as f64, p.y as f64];
    let mut best: Option<(f64, (ViewId, AnnotationId, GripKind))> = None;
    for (v, id, g) in &scene.items {
        if !sel.contains(&(*v, *id)) {
            continue;
        }
        for (at, kind) in &g.grips {
            // The text grip is the text itself (dragged by the body).
            if *kind == GripKind::Text {
                continue;
            }
            let d = (at[0] - q[0]).hypot(at[1] - q[1]);
            if d <= tol && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, (*v, *id, *kind)));
            }
        }
    }
    best.map(|(_, x)| x)
}

#[allow(clippy::too_many_arguments)]
fn annotation_pointer(
    mut inputs: MessageReader<PointerInput>,
    kind: Res<ActiveKind>,
    doc: Option<Res<ActiveDocument>>,
    rect: Res<ViewportRect>,
    hover_map: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    keys: Res<ButtonInput<KeyCode>>,
    cache: Res<ViewCache>,
    scene: Res<AnnotationScene>,
    specs: Res<super::symbol_cards::SymbolSpecs>,
    mut dui: ResMut<DrawingUi>,
    mut ui: ResMut<AnnotationUi>,
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
    let Some((((element, _), view), (_, d))) = current_view(&doc, &dui).zip(active_drawing(&doc)) else {
        inputs.clear();
        return;
    };
    let index = dui.sheet_index(element, d);
    let area = sheet_area(&rect, &dui);
    let over = super::view_tools::pointer_over_sheet(&hover_map, &q_area) && q_menus.is_empty() && q_dialogs.is_empty();
    let px = mm_per_px(&doc, &dui);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let annotating = ui.tool != AnnTool::None;
    if dui.annotating != annotating {
        dui.annotating = annotating;
    }
    let views: Vec<(View, Option<Arc<ViewGeometry>>)> = sheet_views(&doc, &dui, &cache);
    let view_of = |id: ViewId| views.iter().find(|(v, _)| v.id == id).and_then(|(v, g)| g.clone().map(|g| (v.clone(), g)));
    // What the tool would pick at `p`: in the view of the picks so far, else the view there.
    let hover_at = |tool: AnnTool, pick_view: Option<ViewId>, picks: &[Pick], p: Vec2| -> Option<Hover> {
        if tool == AnnTool::None {
            return None;
        }
        let vid = pick_view.or_else(|| view_at(d, index, &cache, p, 6.0 * px));
        vid.and_then(view_of).and_then(|(v, g)| {
            hover_in(&v, &g, p, 5.0 * px, 7.0 * px, |h| tool_accepts(tool, &v, &g, picks, h).is_some())
        })
    };
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        let p = screen_to_sheet(view, area, pos);
        match input.action {
            PointerAction::Move { .. } => {
                dui.pointer = Some(p);
                if let Some(mut drag) = ui.drag {
                    drag.at = p;
                    if !drag.moving && (p - drag.start).length() as f64 > 3.0 * px {
                        drag.moving = true;
                    }
                    ui.drag = Some(drag);
                }
            }
            PointerAction::Press(PointerButton::Primary) if over => {
                if ui.tool != AnnTool::None {
                    dui.annotation_press = true;
                    let tool = ui.tool;
                    let hover = hover_at(tool, ui.pick_view, &ui.picks, p);
                    let cursor = p;
                    commands.queue(move |w: &mut World| tool_click(w, tool, hover, cursor));
                    continue;
                }
                // A view tool takes the click (P3C.8: cutting lines cross annotations).
                if dui.tool != ViewTool::None {
                    continue;
                }
                let tol = 4.0 * px;
                if let Some((v, id, grip)) = grip_at(&scene, &ui.selected, p, 1.5 * tol) {
                    dui.annotation_press = true;
                    ui.drag = Some(GripDrag { view: v, id, grip, start: p, at: p, moving: false });
                    continue;
                }
                match annotation_at(&scene, p, tol) {
                    Some(hit) => {
                        dui.annotation_press = true;
                        dui.selected.clear();
                        if ctrl {
                            if let Some(i) = ui.selected.iter().position(|s| *s == hit) {
                                ui.selected.remove(i);
                            } else {
                                ui.selected.push(hit);
                            }
                        } else if !ui.selected.contains(&hit) {
                            ui.selected = vec![hit];
                            ui.palette_open = false;
                        }
                        ui.drag = Some(GripDrag { view: hit.0, id: hit.1, grip: GripKind::Text, start: p, at: p, moving: false });
                    }
                    None => {
                        dui.annotation_press = false;
                        if !ctrl && !ui.selected.is_empty() {
                            ui.selected.clear();
                            ui.palette_open = false;
                        }
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) => {
                dui.annotation_press = false;
                if let Some(drag) = ui.drag.take() {
                    ui.drag_preview = None;
                    if drag.moving {
                        let target = if let GripKind::Attach(_) = drag.grip {
                            view_of(drag.view).and_then(|(v, g)| hover_in(&v, &g, p, 5.0 * px, 7.0 * px, |_| true))
                        } else {
                            None
                        };
                        commands.queue(move |w: &mut World| finish_drag(w, drag, target));
                    }
                }
            }
            PointerAction::Cancel => {
                ui.drag = None;
                ui.drag_preview = None;
                dui.annotation_press = false;
            }
            _ => {}
        }
    }
    // What is under the cursor now.
    let pointer = dui.pointer;
    let hover = match (pointer, over) {
        (Some(p), true) => hover_at(ui.tool, ui.pick_view, &ui.picks, p),
        _ => None,
    };
    if ui.hover != hover {
        ui.hover = hover;
    }
    // The annotation the picks make, at the cursor (a callout's is made by
    // `bom_tools::callout_preview`).
    let preview = match (ui.tool, ui.pick_view, pointer) {
        (AnnTool::Callout, _, _) => ui.preview.clone(),
        (tool, Some(vid), Some(p)) if placed(tool) && !ui.picks.is_empty() => view_of(vid).and_then(|(v, g)| {
            let cursor = v.from_sheet([p.x as f64, p.y as f64]);
            complete(tool, &v, &g, &ui.picks, cursor, &specs).map(|a| (vid, a))
        }),
        _ => None,
    };
    if ui.preview.as_ref().map(|(v, a)| (v, &a.kind)) != preview.as_ref().map(|(v, a)| (v, &a.kind)) {
        ui.preview = preview;
    }
    // The dragged annotation as it would be.
    let dp = ui.drag.filter(|d| d.moving).and_then(|drag| {
        let (v, a) = find_annotation(d, drag.view, drag.id)?;
        let (_, g) = view_of(drag.view)?;
        // An attachment grip shows the dimension re-attached to what is under it (D6.5).
        let target = match drag.grip {
            GripKind::Attach(_) => hover_in(&v, &g, drag.at, 5.0 * px, 7.0 * px, |_| true),
            _ => None,
        };
        dragged(&v, &g, &a, drag, target).map(|a| (drag.view, a))
    });
    // A callout dragged by its text lines up with the others (D11.6).
    let dp = dp.map(|(vid, mut a)| {
        if let Some(drag) = ui.drag
            && drag.grip == GripKind::Text
            && let Some((_, v)) = d.view(vid)
        {
            let guides = snap_callout(d, index, v, &mut a);
            if ui.guides != guides {
                ui.guides = guides;
            }
        }
        (vid, a)
    });
    if dp.is_none() && ui.tool != AnnTool::Callout && !ui.guides.is_empty() {
        ui.guides.clear();
    }
    if ui.drag_preview != dp {
        ui.drag_preview = dp;
    }
    // Hover highlight of annotations (no tool, not dragging).
    let hovered = match (ui.tool, pointer, over, ui.drag) {
        (AnnTool::None, Some(p), true, None) if dui.tool == ViewTool::None => annotation_at(&scene, p, 4.0 * px),
        _ => None,
    };
    if ui.hovered != hovered {
        ui.hovered = hovered;
    }
    let over_annotation = hovered.is_some() || ui.drag.is_some();
    if dui.annotation_hover != over_annotation {
        dui.annotation_hover = over_annotation;
    }
}

/// A callout's anchor snapped to the other callouts of sheet `index` (D11.6); returns the
/// guides. Other annotations are left alone.
fn snap_callout(d: &Drawing, index: usize, v: &View, a: &mut Annotation) -> Vec<([f64; 2], [f64; 2])> {
    let AnnotationKind::Callout(c) = &mut a.kind else { return Vec::new() };
    let (t, guides) = super::bom_tools::snapped_anchor(d, index, v, v.to_sheet(c.text), Some((v.id, a.id)));
    c.text = t;
    guides
}

/// An annotation after its grip was dragged to `drag.at` (and dropped on `target`).
fn dragged(v: &View, g: &ViewGeometry, a: &Annotation, drag: GripDrag, target: Option<Hover>) -> Option<Annotation> {
    let mut a = a.clone();
    let delta = v.from_sheet([drag.at.x as f64, drag.at.y as f64]);
    let start = v.from_sheet([drag.start.x as f64, drag.start.y as f64]);
    let dv = [delta[0] - start[0], delta[1] - start[1]];
    match (&mut a.kind, drag.grip) {
        (AnnotationKind::Dimension(d), GripKind::Text) => {
            d.text = [d.text[0] + dv[0], d.text[1] + dv[1]];
        }
        (AnnotationKind::HoleCallout(c), GripKind::Text) => {
            c.text = [c.text[0] + dv[0], c.text[1] + dv[1]];
        }
        (AnnotationKind::Ordinate(o), GripKind::Text) => {
            o.level += if o.vertical { dv[0] } else { dv[1] };
        }
        (k, GripKind::Text) if !matches!(k, AnnotationKind::Centermark(_) | AnnotationKind::Centerline(_) | AnnotationKind::CircleCenterline(_) | AnnotationKind::VirtualSharp { .. }) => {
            let t = k.text_mut()?;
            *t = [t[0] + dv[0], t[1] + dv[1]];
        }
        (AnnotationKind::Centerline(cl), GripKind::End(i)) => {
            let (p0, p1) = ann::centerline_ends(v, g, cl)?;
            let dir = [p1[0] - p0[0], p1[1] - p0[1]];
            let l = dir[0].hypot(dir[1]).max(1e-12);
            let along = (dv[0] * dir[0] + dv[1] * dir[1]) / l;
            if i == 0 {
                cl.extend[0] -= along;
            } else {
                cl.extend[1] += along;
            }
        }
        (AnnotationKind::Dimension(d), GripKind::Attach(i)) => {
            let h = target?;
            let pick = h.pick();
            match &mut d.kind {
                DimKind::Diameter(e) | DimKind::Radius(e) => {
                    if !matches!(h.shape, Shape::Circle { .. }) {
                        return None;
                    }
                    *e = h.edge;
                }
                DimKind::Distance { a: pa, b: pb, .. } => {
                    let pick = match (pick, h.shape) {
                        (Pick::Edge(e), Shape::Circle { .. }) => Pick::Point(PointRef { edge: e, of: PointOf::Center, hint: [0.0, 0.0] }),
                        (p, _) => p,
                    };
                    if i == 0 {
                        *pa = pick;
                    } else {
                        *pb = pick;
                    }
                }
                DimKind::Angle { a: ea, b: eb } => {
                    if !matches!(h.shape, Shape::Line { .. }) {
                        return None;
                    }
                    if i == 0 {
                        *ea = h.edge;
                    } else {
                        *eb = h.edge;
                    }
                }
            }
            ann::measure(v, g, d)?;
            // The text goes back between the new ends.
            ann::centre_text_in_span(v, g, d);
        }
        (AnnotationKind::HoleCallout(c), GripKind::Attach(_)) => {
            let h = target?;
            if !matches!(h.shape, Shape::Circle { .. }) || ann::hole_of(g, &h.edge).is_none() {
                return None;
            }
            c.edge = h.edge;
        }
        _ => return None,
    }
    Some(a)
}

fn finish_drag(w: &mut World, drag: GripDrag, target: Option<Hover>) {
    let Some(doc) = w.get_resource::<ActiveDocument>() else {
        return;
    };
    let Some((_, d)) = active_drawing(doc) else {
        return;
    };
    let Some((v, a)) = find_annotation(d, drag.view, drag.id) else {
        return;
    };
    let Some(g) = w.resource::<ViewCache>().geometry(&v) else {
        return;
    };
    let Some(mut new) = dragged(&v, &g, &a, drag, target) else {
        return;
    };
    if drag.grip == GripKind::Text
        && let Some(doc) = w.get_resource::<ActiveDocument>()
        && let Some((id, d)) = active_drawing(doc)
    {
        let index = w.resource::<DrawingUi>().sheet_index(id, d);
        snap_callout(d, index, &v, &mut new);
    }
    w.resource_mut::<AnnotationUi>().guides.clear();
    if new == a {
        return;
    }
    let label = match drag.grip {
        GripKind::Text => format!("Move {}", a.noun()),
        GripKind::Attach(_) => format!("Re-attach {}", a.noun()),
        GripKind::End(_) => "Extend centerline".to_string(),
    };
    edit_drawing(w, DrawingOp::SetAnnotation { view: drag.view, annotation: new, label });
}

/// A click with an annotation tool.
fn tool_click(w: &mut World, tool: AnnTool, hover: Option<Hover>, p: Vec2) {
    if tool == AnnTool::PlaceBom {
        super::bom_tools::place_bom(w, [p.x as f64, p.y as f64]);
        return;
    }
    if tool == AnnTool::Callout {
        super::bom_tools::callout_click(w, hover, p);
        return;
    }
    if matches!(tool, AnnTool::Note | AnnTool::Table | AnnTool::AddLeader(_)) {
        super::notes::tool_click(w, tool, hover, p);
        return;
    }
    if matches!(tool, AnnTool::SheetLine | AnnTool::SheetSpline | AnnTool::PlaceImport) {
        super::sheet_items::tool_click(w, tool, p);
        return;
    }
    let (views, sheet_ok) = {
        let Some(doc) = w.get_resource::<ActiveDocument>() else {
            return;
        };
        let dui = w.resource::<DrawingUi>();
        let cache = w.resource::<ViewCache>();
        (sheet_views(doc, dui, cache), active_drawing(doc).is_some())
    };
    if !sheet_ok {
        return;
    }
    let (picks, pick_view, preview) = {
        let ui = w.resource::<AnnotationUi>();
        (ui.picks.clone(), ui.pick_view, ui.preview.clone())
    };
    let view_of = |id: ViewId| views.iter().find(|(v, _)| v.id == id).and_then(|(v, g)| g.clone().map(|g| (v.clone(), g)));
    // A baseline or ordinate set just placed: each further pick adds to it (P3C.8).
    let extending = w.resource::<AnnotationUi>().extending;
    if let (AnnTool::Dimension(DimTool::Baseline | DimTool::Ordinate), Some((vid, id)), Some(h)) = (tool, extending, hover)
        && h.view == vid
        && let Some((v, g)) = view_of(vid)
        && let Some(pick) = tool_accepts(tool, &v, &g, &[], &h)
        && let Some(a) = v.annotations.iter().find(|a| a.id == id)
    {
        let mut a = a.clone();
        match (&mut a.kind, pick) {
            (AnnotationKind::Baseline(b), p) => b.targets.push(p),
            (AnnotationKind::Ordinate(o), Pick::Point(p)) => o.points.push(p),
            _ => return,
        }
        let label = format!("Extend {}", a.noun());
        edit_drawing(w, DrawingOp::SetAnnotation { view: vid, annotation: a, label });
        return;
    }
    // A pick, if the cursor is on something the tool takes (and there is room for one).
    let pick = hover.and_then(|h| {
        let (v, g) = view_of(h.view)?;
        let same_view = pick_view.is_none_or(|pv| pv == h.view);
        let room = picks.len() < needs(tool);
        (same_view && room).then(|| tool_accepts(tool, &v, &g, &picks, &h)).flatten().map(|p| (h.view, p))
    });
    // A click that places the annotation following the cursor.
    if placed(tool)
        && let Some((vid, a)) = preview
        && (pick.is_none() || picks.len() >= 2 || (needs(tool) == 1 && !picks.is_empty()))
    {
        let id = a.id;
        let set = matches!(a.kind, AnnotationKind::Baseline(_) | AnnotationKind::Ordinate(_));
        if edit_drawing(w, DrawingOp::AddAnnotation { view: vid, annotation: a }) {
            let mut ui = w.resource_mut::<AnnotationUi>();
            ui.reset_picks();
            if set {
                ui.extending = Some((vid, id));
            }
        }
        return;
    }
    let Some((vid, pick)) = pick else {
        return;
    };
    let mut new_picks = picks.clone();
    new_picks.push(pick);
    {
        let mut ui = w.resource_mut::<AnnotationUi>();
        ui.picks = new_picks.clone();
        ui.pick_view = Some(vid);
    }
    if !placed(tool) && new_picks.len() >= needs(tool) {
        let Some((v, g)) = view_of(vid) else { return };
        let cursor = v.from_sheet([p.x as f64, p.y as f64]);
        let specs = w.resource::<super::symbol_cards::SymbolSpecs>().clone();
        if let Some(a) = complete(tool, &v, &g, &new_picks, cursor, &specs) {
            edit_drawing(w, DrawingOp::AddAnnotation { view: vid, annotation: a });
        }
        w.resource_mut::<AnnotationUi>().reset_picks();
    }
}

// ---------------------------------------------------------------------------------------------
// Keys

/// D, Shift+R and Shift+D start the dimension tools (X12); Esc drops the picks, then ends the
/// tool; Delete removes the selected annotations.
#[allow(clippy::too_many_arguments)]
fn annotation_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    kind: Res<ActiveKind>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<TextInputField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    items: Res<super::sheet_items::SheetItemsUi>,
    mut ui: ResMut<AnnotationUi>,
    mut commands: Commands,
) {
    if *kind != ActiveKind::Drawing {
        keys_in.clear();
        return;
    }
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    for k in keys_in.read() {
        if k.state != bevy::input::ButtonState::Pressed || typing || !q_dialogs.is_empty() || !q_menus.is_empty() || ctrl || alt {
            continue;
        }
        let tool = match (k.key_code, shift) {
            (KeyCode::KeyD, false) => Some(AnnTool::Dimension(DimTool::Smart)),
            (KeyCode::KeyR, true) => Some(AnnTool::Dimension(DimTool::Radial)),
            (KeyCode::KeyD, true) => Some(AnnTool::Dimension(DimTool::Diameter)),
            (KeyCode::KeyN, false) => Some(AnnTool::Note),
            _ => None,
        };
        if let Some(t) = tool {
            commands.queue(move |w: &mut World| {
                if w.resource::<AnnotationUi>().tool != t {
                    start_tool(w, t);
                }
            });
            continue;
        }
        match k.key_code {
            // A line chain or spline being drawn ends first (see `sheet_items`).
            KeyCode::Escape if items.drawing() => {}
            KeyCode::Escape => {
                if ui.extending.is_some() {
                    ui.extending = None;
                    ui.reset_picks();
                } else if ui.tool == AnnTool::Callout && ui.preview.is_some() {
                    // A callout being placed: dropped; the tool stays.
                    ui.preview = None;
                    ui.guides.clear();
                    commands.queue(|w: &mut World| w.resource_mut::<super::bom_tools::CalloutUi>().attach = None);
                } else if !ui.picks.is_empty() {
                    ui.reset_picks();
                } else if ui.tool != AnnTool::None {
                    ui.tool = AnnTool::None;
                    ui.hover = None;
                } else if !ui.selected.is_empty() {
                    ui.selected.clear();
                    ui.palette_open = false;
                }
            }
            KeyCode::Delete | KeyCode::Backspace if ui.tool == AnnTool::None && !ui.selected.is_empty() => {
                let ids = std::mem::take(&mut ui.selected);
                ui.palette_open = false;
                commands.queue(move |w: &mut World| {
                    edit_drawing(w, DrawingOp::DeleteAnnotations { ids });
                });
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Menus

/// The anchor of an annotation's context menu.
#[derive(Component, Clone, Copy)]
struct AnnotationMenuFor(ViewId, AnnotationId);

/// Right-click on an annotation: its menu. Returns false when there is none under `pos`.
pub fn open_annotation_menu(world: &mut World, pos: Vec2) -> bool {
    let rect = *world.resource::<ViewportRect>();
    let hit = (|| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let ui = world.resource::<DrawingUi>();
        let (_, view) = current_view(doc, ui)?;
        let p = screen_to_sheet(view, sheet_area(&rect, ui), pos);
        annotation_at(world.resource::<AnnotationScene>(), p, 4.0 / view.ppm as f64)
    })();
    let Some((v, id)) = hit else {
        return false;
    };
    let Some(a) = world
        .get_resource::<ActiveDocument>()
        .and_then(|doc| active_drawing(doc))
        .and_then(|(_, d)| find_annotation(d, v, id))
        .map(|(_, a)| a)
    else {
        return false;
    };
    {
        let mut ui = world.resource_mut::<AnnotationUi>();
        ui.selected = vec![(v, id)];
        ui.palette_open = false;
    }
    let theme = world.resource::<Theme>().clone();
    let mut menu = Menu::new("annotation-context-menu").min_width(180.0);
    if matches!(a.kind, AnnotationKind::HoleCallout(_) | AnnotationKind::Callout(_)) {
        menu = menu.item(MenuItem::new("annotation-menu-edit", "Edit…"));
    }
    if matches!(a.kind, AnnotationKind::Dimension(_)) {
        menu = menu.item(MenuItem::new("annotation-menu-palette", "Format…"));
    }
    menu = menu.item(MenuItem::new("annotation-menu-delete", "Delete"));
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, pos, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((AnnotationMenuFor(v, id), DespawnOnExit(AppState::Document)));
    world.flush();
    true
}

fn on_annotation_menu(ev: On<MenuAction>, q: Query<&AnnotationMenuFor>, mut commands: Commands) {
    let Ok(t) = q.get(ev.entity).copied() else {
        return;
    };
    let item = ev.item.clone();
    commands.queue(move |w: &mut World| match item.as_str() {
        "annotation-menu-edit" => {
            let is_callout = w
                .get_resource::<ActiveDocument>()
                .and_then(|doc| active_drawing(doc))
                .and_then(|(_, d)| find_annotation(d, t.0, t.1))
                .is_some_and(|(_, a)| matches!(a.kind, AnnotationKind::Callout(_)));
            if is_callout {
                super::bom_tools::edit_callout(w, t.0, t.1);
            } else {
                super::dim_palette::open_callout_dialog(w, t.0, t.1);
            }
        }
        "annotation-menu-palette" => {
            let mut ui = w.resource_mut::<AnnotationUi>();
            ui.selected = vec![(t.0, t.1)];
            ui.palette_open = true;
        }
        "annotation-menu-delete" => {
            edit_drawing(w, DrawingOp::DeleteAnnotations { ids: vec![(t.0, t.1)] });
            w.resource_mut::<AnnotationUi>().selected.clear();
        }
        _ => {}
    });
}

/// The toolbar ▾ menus of the dimension and centerline buttons.
#[derive(Component, Clone, Copy)]
pub struct ToolMenuFor;

/// The dimension tools' ▾ menu.
pub const DIMENSION_TOOLS: [(&str, Option<DimTool>, &str, &str, &str); 10] = [
    ("drawing-dim-smart", Some(DimTool::Smart), "Dimension", "dimension", "D"),
    ("drawing-dim-radial", Some(DimTool::Radial), "Radial dimension", "center-arc", "Shift+R"),
    ("drawing-dim-diameter", Some(DimTool::Diameter), "Diameter dimension", "center-circle", "Shift+D"),
    ("drawing-dim-point-to-point", Some(DimTool::PointToPoint), "Point to point dimension", "ruler", ""),
    ("drawing-dim-line-to-line", Some(DimTool::LineToLine), "Line to line dimension", "constraint-parallel", ""),
    ("drawing-dim-angular", Some(DimTool::Angular), "Angular dimension", "constraint-perpendicular", ""),
    ("drawing-dim-baseline", Some(DimTool::Baseline), "Baseline dimension", "linear-pattern", ""),
    ("drawing-dim-ordinate", Some(DimTool::Ordinate), "Ordinate dimension", "list", ""),
    ("drawing-dim-chamfer", Some(DimTool::Chamfer), "Chamfer dimension", "chamfer", ""),
    ("drawing-dim-arc-length", Some(DimTool::ArcLength), "Arc length dimension", "three-point-arc", ""),
];

/// The Geometric tolerance ▾ menu (P3C.8).
pub const GDT_TOOLS: [(&str, AnnTool, &str, &str); 2] = [
    ("drawing-gdt-frame", AnnTool::Gdt, "Feature control frame", "constraint-parallel"),
    ("drawing-gdt-datum", AnnTool::Datum, "Datum feature", "tag"),
];

/// The centerline modes' ▾ menu.
pub const CENTERLINE_TOOLS: [(&str, CenterlineMode, &str, &str); 4] = [
    ("drawing-centerline-point-to-point", CenterlineMode::PointToPoint, "Point to point centerline", "construction"),
    ("drawing-centerline-line-to-line", CenterlineMode::LineToLine, "Line to line centerline", "constraint-parallel"),
    ("drawing-centerline-3-point-circle", CenterlineMode::ThreePointCircle, "3 point circle centerline", "three-point-circle"),
    ("drawing-centerline-2-point-circle", CenterlineMode::TwoPointCircle, "2 point circle centerline", "center-circle"),
];

/// Opens a tool button's ▾ menu under it.
pub fn open_tool_menu(world: &mut World, button: Entity, which: &str) {
    let Some((left, bottom)) = world.get::<ComputedNode>(button).zip(world.get::<bevy::ui::UiGlobalTransform>(button)).map(|(n, t)| {
        let s = n.inverse_scale_factor();
        ((t.translation.x - n.size().x / 2.0) * s, (t.translation.y + n.size().y / 2.0) * s)
    }) else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let mut menu = Menu::new(format!("{which}-menu")).min_width(230.0).keycap_shortcuts();
    if which == "drawing-gdt" {
        for (name, _, label, icon_name) in GDT_TOOLS {
            menu = menu.item(MenuItem::new(name, label).icon(icon_name));
        }
    } else if which == "drawing-detail-view" {
        for (name, label, icon_name) in super::view_kind_tools::VIEW_KINDS {
            menu = menu.item(MenuItem::new(name, label).icon(icon_name));
        }
    } else if which == "drawing-dimension" {
        for (name, tool, label, icon_name, key) in DIMENSION_TOOLS {
            let mut item = MenuItem::new(name, label).icon(icon_name).disabled(tool.is_none());
            if !key.is_empty() {
                item = item.shortcut(key);
            }
            menu = menu.item(item);
        }
    } else {
        for (name, _, label, icon_name) in CENTERLINE_TOOLS {
            menu = menu.item(MenuItem::new(name, label).icon(icon_name));
        }
    }
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, Vec2::new(left, bottom + 2.0), menu.build(&theme));
    commands.entity(anchor).insert((ToolMenuFor, DespawnOnExit(AppState::Document)));
    world.flush();
}

fn on_tool_menu(ev: On<MenuAction>, q: Query<(), With<ToolMenuFor>>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    let item = ev.item.clone();
    if let Some((name, _, _)) = super::view_kind_tools::VIEW_KINDS.iter().find(|k| k.0 == item) {
        let name: &'static str = name;
        commands.queue(move |w: &mut World| {
            w.resource_mut::<super::toolbar::ToolChoices>().view_kind = name;
            super::view_kind_tools::start(w, name);
        });
        return;
    }
    let tool = DIMENSION_TOOLS
        .iter()
        .find(|t| t.0 == item)
        .and_then(|t| t.1)
        .map(AnnTool::Dimension)
        .or_else(|| CENTERLINE_TOOLS.iter().find(|t| t.0 == item).map(|t| AnnTool::Centerline(t.1)))
        .or_else(|| GDT_TOOLS.iter().find(|t| t.0 == item).map(|t| t.1));
    if let Some(t) = tool {
        commands.queue(move |w: &mut World| {
            if w.resource::<AnnotationUi>().tool != t {
                start_tool(w, t);
            }
            w.resource_mut::<super::toolbar::ToolChoices>().remember(t);
        });
    }
}

// ---------------------------------------------------------------------------------------------
// The scene

/// A text of the annotations on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneText {
    pub text: ann::PlacedText,
    pub color: Color,
    pub bold: bool,
    pub italic: bool,
    /// Degrees counter-clockwise about the text's anchor (notes, P3C.4).
    pub rotation: f32,
}

impl SceneText {
    pub fn plain(text: ann::PlacedText, color: Color) -> Self {
        Self { text, color, bold: false, italic: false, rotation: 0.0 }
    }
}

/// The annotations of the active sheet as drawn now, and each one's graphics (for picking).
#[derive(Resource, Default)]
pub struct AnnotationScene {
    key: Option<SceneKey>,
    pub items: Vec<(ViewId, AnnotationId, AnnGraphics)>,
    pub strokes: Vec<(Vec<Vec2>, Color)>,
    pub fills: Vec<([Vec2; 3], Color)>,
    pub texts: Vec<SceneText>,
}

#[derive(Clone, PartialEq)]
struct SceneKey {
    views: Vec<View>,
    geometry: Vec<Option<usize>>,
    style: cadrs_drawing::DrawingStyle,
    selected: Vec<(ViewId, AnnotationId)>,
    hovered: Option<(ViewId, AnnotationId)>,
    preview: Option<(ViewId, Annotation)>,
    drag_preview: Option<(ViewId, Annotation)>,
    callout_preview: Option<(ViewId, Annotation)>,
    picks: Vec<Pick>,
    pick_view: Option<ViewId>,
    hover: Option<Hover>,
    tool: AnnTool,
    ppm: f32,
    /// The view being placed and the selected views (their decorations' texts, P3C.8).
    ghost: Option<View>,
    ghost_geometry: Option<usize>,
    view_selected: Vec<ViewId>,
    guides: Vec<([f64; 2], [f64; 2])>,
    /// The sheet's tables and the drawing's sources (callouts read them, P3C.5).
    tables: Vec<cadrs_drawing::Table>,
    sources: usize,
}

#[allow(clippy::too_many_arguments)]
fn rebuild_scene(
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    dui: Res<DrawingUi>,
    ui: Res<AnnotationUi>,
    cache: Res<ViewCache>,
    mut scene: ResMut<AnnotationScene>,
) {
    let Some(doc) = doc.filter(|_| *kind == ActiveKind::Drawing) else {
        if scene.key.is_some() {
            *scene = AnnotationScene::default();
        }
        return;
    };
    let Some((_, d)) = active_drawing(&doc) else {
        if scene.key.is_some() {
            *scene = AnnotationScene::default();
        }
        return;
    };
    let views = sheet_views(&doc, &dui, &cache);
    let ppm = current_view(&doc, &dui).map(|(_, v)| v.ppm).unwrap_or(1.0);
    let key = SceneKey {
        views: views.iter().map(|(v, _)| v.clone()).collect(),
        geometry: views.iter().map(|(_, g)| g.as_ref().map(|g| Arc::as_ptr(g) as usize)).collect(),
        style: d.style.clone(),
        selected: ui.selected.clone(),
        hovered: ui.hovered,
        preview: ui.preview.clone(),
        drag_preview: ui.drag_preview.clone(),
        callout_preview: ui.callout_preview.clone(),
        picks: ui.picks.clone(),
        pick_view: ui.pick_view,
        hover: ui.hover,
        tool: ui.tool,
        ppm,
        ghost: dui.ghost.clone(),
        ghost_geometry: dui.ghost.as_ref().and_then(|g| cache.geometry(g)).map(|g| Arc::as_ptr(&g) as usize),
        view_selected: dui.selected.clone(),
        guides: ui.guides.clone(),
        tables: active_drawing(&doc).and_then(|(id, d)| d.sheets.get(dui.sheet_index(id, d))).map(|s| s.tables.clone()).unwrap_or_default(),
        sources: d.sources.as_ptr() as usize ^ d.sources.len(),
    };
    if scene.key.as_ref() == Some(&key) {
        return;
    }
    let sheet_index = active_drawing(&doc).map(|(id, d)| dui.sheet_index(id, d)).unwrap_or(0);
    let Some(sheet) = d.sheets.get(sheet_index) else { return };
    let mut items = Vec::new();
    let mut strokes: Vec<(Vec<Vec2>, Color)> = Vec::new();
    let mut fills: Vec<([Vec2; 3], Color)> = Vec::new();
    let mut texts = Vec::new();
    let v2 = |p: &[f64; 2]| Vec2::new(p[0] as f32, p[1] as f32);
    let mut emit = |g: &AnnGraphics, color: Color, grips: bool, attached: bool, strokes: &mut Vec<(Vec<Vec2>, Color)>, fills: &mut Vec<([Vec2; 3], Color)>| {
        if attached {
            // The current attachments in blue (D6.5); ones whose geometry is gone in red, so a
            // selected (orange, D6.8) dangling annotation still shows what dangles.
            for (i, s) in g.attached.iter().enumerate() {
                let c = if g.attached_dead.get(i).copied().unwrap_or(false) { dangling_red() } else { blue() };
                strokes.push((s.iter().map(v2).collect(), c));
            }
        }
        for s in &g.strokes {
            strokes.push((s.iter().map(v2).collect(), color));
        }
        for t in &g.fills {
            fills.push((t.map(|p| v2(&p)), color));
        }
        for t in &g.texts {
            texts.push(SceneText::plain(t.clone(), color));
        }
        if grips {
            let h = 3.0 / ppm;
            for (p, k) in &g.grips {
                if *k == GripKind::Text {
                    continue;
                }
                fills.extend(square(v2(p), h, blue()));
            }
        }
    };
    for (v, g) in &views {
        let Some(g) = g else { continue };
        for a in &v.annotations {
            let key = (v.id, a.id);
            let dragged = ui
                .drag_preview
                .as_ref()
                .or(ui.callout_preview.as_ref())
                .filter(|(vid, x)| *vid == v.id && x.id == a.id)
                .map(|(_, x)| x);
            let shown = dragged.unwrap_or(a);
            let m = cadrs_drawing::assembly::SheetModel::new(d, sheet, v, &**g);
            let Some(gr) = ann::annotation_graphics(&d.style, v, &m, shown) else {
                continue;
            };
            let selected = ui.selected.contains(&key);
            let color = if selected || dragged.is_some() {
                orange()
            } else if ui.hovered == Some(key) {
                hover_orange()
            } else if gr.dangling {
                dangling_red()
            } else {
                ink()
            };
            emit(&gr, color, selected, selected, &mut strokes, &mut fills);
            items.push((v.id, a.id, gr));
        }
    }
    // The annotation being placed, in orange.
    if let Some((vid, a)) = &ui.preview
        && let Some((v, Some(g))) = views.iter().find(|(v, _)| v.id == *vid)
        && let Some(gr) = ann::annotation_graphics(&d.style, v, &cadrs_drawing::assembly::SheetModel::new(d, sheet, v, &**g), a)
    {
        emit(&gr, orange(), false, false, &mut strokes, &mut fills);
    }
    // Flat pattern views' bend notes (P3I.7).
    for (v, g) in &views {
        let Some(g) = g else { continue };
        let Some(flat) = g.flat.as_ref() else { continue };
        let selected = dui.selected.contains(&v.id) || dui.flat_preview.as_ref().is_some_and(|(id, _)| *id == v.id);
        let color = if selected { orange() } else { ink() };
        for n in cadrs_drawing::flat_view::bend_notes(&d.style, v, flat) {
            for st in &n.strokes {
                strokes.push((st.iter().map(v2).collect(), color));
            }
            for t in &n.fills {
                fills.push((t.map(|p| v2(&p)), color));
            }
            texts.push(SceneText { rotation: n.rotation as f32, ..SceneText::plain(n.text.clone(), color) });
        }
    }
    // Inference lines (D11.6): dashed, from the other callouts.
    for (a, b) in &ui.guides {
        for dash in cadrs_drawing::view::dashes(&[*a, *b], &[1.6, 1.0]) {
            strokes.push((dash.iter().map(v2).collect(), Color::srgb_u8(0x1f, 0x7a, 0xe0)));
        }
    }
    // The views' decorations: section and detail labels, letters, cutting-line arrows (P3C.8).
    {
        let mut all: Vec<View> = views.iter().map(|(v, _)| v.clone()).collect();
        let ghost = dui.ghost.as_ref().map(|g| (g.clone(), cache.geometry(g)));
        if let Some((g, _)) = &ghost {
            all.push(g.clone());
        }
        let ghost_blue = Color::srgb_u8(0x5b, 0x8f, 0xd6);
        let avoid = active_drawing(&doc)
            .and_then(|(id, d)| d.sheets.get(dui.sheet_index(id, d)))
            .map(cadrs_drawing::view_kinds::label_avoid)
            .unwrap_or_default();
        for (v, g, color) in views
            .iter()
            .map(|(v, g)| {
                let c = if dui.selected.contains(&v.id) { orange() } else { ink() };
                (v, g.clone(), c)
            })
            .chain(ghost.iter().map(|(v, g)| (v, g.clone(), ghost_blue)))
        {
            let dec = cadrs_drawing::view_kinds::view_decor(&d.style, &all, v, g.as_deref().map(|g| g as &dyn ann::ViewModel), &avoid);
            for t in &dec.fills {
                fills.push((t.map(|p| v2(&p)), color));
            }
            for t in &dec.texts {
                texts.push(SceneText::plain(t.clone(), color));
            }
        }
    }
    // The picks so far and the hovered geometry with its snap points (D6.3).
    let h = 2.6 / ppm;
    for p in &ui.picks {
        let Some((v, Some(g))) = ui.pick_view.and_then(|pv| views.iter().find(|(v, _)| v.id == pv)) else {
            continue;
        };
        let shape = ann::resolve(v, &**g, p.edge()).shape;
        strokes.push((shape.polyline().iter().map(|q| v2(&v.to_sheet(*q))).collect(), orange()));
        if let Pick::Point(pr) = p
            && let Some((at, _)) = ann::resolve_point(v, &**g, pr)
        {
            fills.extend(square(v2(&v.to_sheet(at)), h * 1.2, orange()));
        }
    }
    if let Some(hv) = &ui.hover
        && let Some((v, _)) = views.iter().find(|(v, _)| v.id == hv.view)
    {
        strokes.push((hv.shape.polyline().iter().map(|q| v2(&v.to_sheet(*q))).collect(), hover_orange()));
        for (_, at) in hv.shape.snap_points() {
            fills.extend(square(v2(&v.to_sheet(at)), h, orange()));
        }
        if let Some((_, at)) = hv.snap {
            fills.extend(square(v2(&v.to_sheet(at)), h * 1.5, orange()));
        }
    }
    scene.key = Some(key);
    scene.items = items;
    scene.strokes = strokes;
    scene.fills = fills;
    scene.texts = texts;
}

/// A filled square (two triangles) of half size `h` at `c`.
pub(crate) fn square(c: Vec2, h: f32, color: Color) -> [([Vec2; 3], Color); 2] {
    let (a, b, cc, d) = (c + Vec2::new(-h, -h), c + Vec2::new(h, -h), c + Vec2::new(h, h), c + Vec2::new(-h, h));
    [([a, b, cc], color), ([a, cc, d], color)]
}

fn draw_strokes(scene: Res<AnnotationScene>, notes: Res<super::notes::NoteScene>, kind: Res<ActiveKind>, mut g: Gizmos<AnnotationLines>) {
    if *kind != ActiveKind::Drawing {
        return;
    }
    for (pts, c) in scene.strokes.iter().chain(&notes.strokes) {
        if pts.len() >= 2 {
            g.linestrip_2d(pts.iter().copied(), *c);
        }
    }
}

/// An annotation text entity.
#[derive(Component)]
struct AnnotationText(SceneText);

fn sync_texts(
    scene: Res<AnnotationScene>,
    notes: Res<super::notes::NoteScene>,
    items: Res<super::sheet_items::ItemScene>,
    kind: Res<ActiveKind>,
    theme: Res<Theme>,
    mut q: Query<(Entity, &mut AnnotationText, &mut Text2d, &mut TextFont, &mut TextColor, &mut SheetText, &mut Transform)>,
    mut commands: Commands,
) {
    let all: Vec<SceneText> = scene.texts.iter().chain(&notes.texts).chain(&items.texts).cloned().collect();
    let want: &[SceneText] = if *kind == ActiveKind::Drawing { &all } else { &[] };
    // The texts are updated in place, in order: a text that moves (a dragged dimension) keeps
    // its entity, so its size, set by `size_sheet_texts`, is not reset. Respawning it each frame
    // drew it at its spawn size, which showed as flicker and blur while dragging.
    let mut existing = q.iter_mut();
    for t in want {
        match existing.next() {
            Some((_, mut held, mut text, mut font, mut color, mut sheet, mut tr)) => {
                if held.0 == *t {
                    continue;
                }
                if held.0.bold != t.bold || held.0.italic != t.italic {
                    let fresh = text_font(&theme, t);
                    font.font = fresh.font;
                    font.weight = fresh.weight;
                    font.style = fresh.style;
                }
                if text.0 != t.text.text {
                    text.0 = t.text.text.clone();
                }
                if color.0 != t.color {
                    color.0 = t.color;
                }
                if sheet.height != t.text.height as f32 {
                    sheet.height = t.text.height as f32;
                }
                let placed = text_transform(t);
                tr.translation = placed.translation;
                tr.rotation = placed.rotation;
                held.0 = t.clone();
            }
            None => {
                commands.spawn((
                    Name::new("annotation-text"),
                    AnnotationText(t.clone()),
                    SheetText { height: t.text.height as f32 },
                    Text2d::new(t.text.text.clone()),
                    text_font(&theme, t),
                    TextColor(t.color),
                    Anchor::CENTER_LEFT,
                    text_transform(t),
                    RenderLayers::layer(DRAWING_LAYER),
                    DespawnOnExit(AppState::Document),
                ));
            }
        }
    }
    // Texts that are no longer on the sheet.
    for (e, ..) in existing {
        commands.entity(e).despawn();
    }
}

/// A text's font at the sheet's size (set by `size_sheet_texts` once spawned): Medium and
/// ExtraBold, as Regular draws grey at sheet sizes (see `cadrs_drawing::rich`).
fn text_font(theme: &Theme, t: &SceneText) -> TextFont {
    let weight = FontWeight(if t.bold { cadrs_drawing::rich::FACE_BOLD } else { cadrs_drawing::rich::FACE_REGULAR });
    let mut font = theme.font(12.0, weight);
    if t.italic {
        font.style = bevy::text::FontStyle::Italic;
    }
    font
}

/// Where a text sits: its anchor, dropped to the capitals' middle, turned about it.
fn text_transform(t: &SceneText) -> Transform {
    let rot = t.rotation.to_radians();
    let drop = Vec2::new(0.0, TEXT_DROP * t.text.height as f32);
    let d = Vec2::new(drop.x * rot.cos() - drop.y * rot.sin(), drop.x * rot.sin() + drop.y * rot.cos());
    Transform::from_xyz(t.text.pos[0] as f32 + d.x, t.text.pos[1] as f32 + d.y, 2.0).with_rotation(Quat::from_rotation_z(rot))
}

/// Text2d centres a line's box, which sits a little above the capitals' middle in Inter: this
/// much (in cap heights) lower puts the capitals' middle on the anchor.
const TEXT_DROP: f32 = -0.08;

/// The mesh of the arrowheads, grips and snap points.
#[derive(Component)]
struct FillMesh;

/// A filled triangle and its colour.
type Tri = ([Vec2; 3], Color);

#[allow(clippy::too_many_arguments)]
fn sync_fills(
    scene: Res<AnnotationScene>,
    notes: Res<super::notes::NoteScene>,
    items: Res<super::sheet_items::ItemScene>,
    kind: Res<ActiveKind>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    q: Query<(Entity, &Mesh2d), With<FillMesh>>,
    mut last: Local<(Vec<Tri>, Vec<Tri>)>,
    mut commands: Commands,
) {
    let (fronts, backs): (Vec<Tri>, Vec<Tri>) = if *kind == ActiveKind::Drawing {
        (scene.fills.iter().chain(&notes.fills).chain(&items.fills).copied().collect(), notes.backs.clone())
    } else {
        (Vec::new(), Vec::new())
    };
    let empty = fronts.is_empty() && backs.is_empty();
    if last.0 == fronts && last.1 == backs && (!q.is_empty() || empty) {
        return;
    }
    *last = (fronts.clone(), backs.clone());
    for (e, _) in &q {
        commands.entity(e).despawn();
    }
    // Backgrounds (selected cells and text, fields) under the text, fills over it.
    for (want, z) in [(&backs, 0.5), (&fronts, 1.5)] {
        if !want.is_empty() {
            spawn_fill_mesh(&mut commands, &mut meshes, &mut materials, want, z);
        }
    }
}

fn spawn_fill_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    want: &[([Vec2; 3], Color)],
    z: f32,
) {
    let mut positions = Vec::with_capacity(want.len() * 3);
    let mut colors = Vec::with_capacity(want.len() * 3);
    for (t, c) in want {
        let lin = c.to_linear().to_f32_array();
        for p in t {
            positions.push([p.x, p.y, 0.0]);
            colors.push(lin);
        }
    }
    let n = positions.len() as u32;
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    // Both windings, so no triangle is culled.
    let mut idx: Vec<u32> = (0..n).collect();
    idx.extend((0..n / 3).flat_map(|t| [3 * t, 3 * t + 2, 3 * t + 1]));
    mesh.insert_indices(Indices::U32(idx));
    commands.spawn((
        Name::new("annotation-fills"),
        FillMesh,
        Mesh2d(meshes.add(mesh)),
        MeshMaterial2d(materials.add(ColorMaterial::from(Color::WHITE))),
        Transform::from_xyz(0.0, 0.0, z),
        RenderLayers::layer(DRAWING_LAYER),
        DespawnOnExit(AppState::Document),
    ));
}

// ---------------------------------------------------------------------------------------------
// The hint

#[derive(Component)]
struct AnnotationHint;

fn hint_text(ui: &AnnotationUi) -> Option<String> {
    let n = ui.picks.len();
    Some(match ui.tool {
        AnnTool::None => return None,
        AnnTool::Note if ui.picks.is_empty() => {
            "Note: click the sheet for a note, or an edge or point for a note with a leader".into()
        }
        AnnTool::Note => "Note: click to place the text".into(),
        AnnTool::Table => "Table: click the sheet to place the table".into(),
        AnnTool::AddLeader(_) => "Add leader: pick an edge or point · Esc to finish".into(),
        AnnTool::SheetLine => "Line: click the start, then the end · Esc to finish".into(),
        AnnTool::SheetSpline => "Spline: click its points · double-click or Enter to finish".into(),
        AnnTool::PlaceImport => "Click the sheet to place it · Esc to cancel".into(),
        AnnTool::PlaceBom => "Insert BOM: click the sheet to place the table (it snaps to the border's corner or the title block) · Esc to cancel".into(),
        AnnTool::Callout if ui.preview.is_none() => "Callout: pick an edge of a part in an assembly view · ✓ to finish".into(),
        AnnTool::Callout => "Callout: click to place it (it lines up with the others) · Esc to cancel".into(),
        AnnTool::Dimension(t) => {
            let name = match t {
                DimTool::Smart => "Dimension",
                DimTool::Radial => "Radial dimension",
                DimTool::Diameter => "Diameter dimension",
                DimTool::PointToPoint => "Point to point dimension",
                DimTool::LineToLine => "Line to line dimension",
                DimTool::Angular => "Angular dimension",
                DimTool::Baseline => "Baseline dimension",
                DimTool::Ordinate => "Ordinate dimension",
                DimTool::Chamfer => "Chamfer dimension",
                DimTool::ArcLength => "Arc length dimension",
            };
            if ui.extending.is_some() {
                return Some(format!("{name}: pick more points to add · Esc to finish"));
            }
            if ui.preview.is_some() {
                format!("{name}: click to place the text · Esc to cancel")
            } else {
                match t {
                    DimTool::Radial | DimTool::Diameter => format!("{name}: pick a circle or arc"),
                    DimTool::LineToLine | DimTool::Angular => format!("{name}: pick two lines"),
                    DimTool::PointToPoint => format!("{name}: pick two points"),
                    DimTool::Smart => format!("{name}: pick edges or points"),
                    DimTool::Baseline => format!("{name}: pick the base, then a point"),
                    DimTool::Ordinate => format!("{name}: pick the zero point"),
                    DimTool::Chamfer => format!("{name}: pick a chamfer's edge"),
                    DimTool::ArcLength => format!("{name}: pick an arc"),
                }
            }
        }
        AnnTool::Gdt | AnnTool::Datum | AnnTool::SurfaceFinish | AnnTool::Weld => {
            let name = match ui.tool {
                AnnTool::Gdt => "Feature control frame",
                AnnTool::Datum => "Datum feature",
                AnnTool::SurfaceFinish => "Surface finish",
                _ => "Weld symbol",
            };
            if n == 0 { format!("{name}: pick an edge") } else { format!("{name}: click to place · Esc to cancel") }
        }
        AnnTool::HoleCallout if n == 0 => "Hole callout: pick a hole's edge".into(),
        AnnTool::HoleCallout => "Hole callout: click to place the callout".into(),
        AnnTool::Centermark => "Centermark: pick circles or arcs · Esc to finish".into(),
        AnnTool::VirtualSharp => "Virtual sharp: pick two straight edges".into(),
        AnnTool::Centerline(CenterlineMode::PointToPoint) => format!("Centerline: pick two points ({n} of 2)"),
        AnnTool::Centerline(CenterlineMode::LineToLine) => format!("Centerline: pick two edges ({n} of 2)"),
        AnnTool::Centerline(CenterlineMode::ThreePointCircle) => format!("Circle centerline: pick three points ({n} of 3)"),
        AnnTool::Centerline(CenterlineMode::TwoPointCircle) => {
            if n == 0 { "Circle centerline: pick the centre".into() } else { "Circle centerline: pick a point on the circle".into() }
        }
    })
}

fn sync_hint(
    ui: Res<AnnotationUi>,
    kind: Res<ActiveKind>,
    theme: Res<Theme>,
    q_area: Query<Entity, With<ViewportArea>>,
    q: Query<(Entity, &Children), With<AnnotationHint>>,
    mut q_text: Query<&mut Text>,
    mut commands: Commands,
) {
    let text = if *kind == ActiveKind::Drawing { hint_text(&ui) } else { None };
    match (text, q.single()) {
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
                    Name::new("drawing-annotation-hint"),
                    AnnotationHint,
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(14.0),
                        left: Val::Percent(50.0),
                        margin: UiRect::left(Val::Px(-220.0)),
                        width: Val::Px(440.0),
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
