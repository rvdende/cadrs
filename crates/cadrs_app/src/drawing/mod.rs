//! The Drawing tab (P3C.1): a 2D sheet workspace.
//!
//! - The active sheet is drawn by its own 2D camera on [`DRAWING_LAYER`], on a grey ground
//!   (D2.1): a white sheet, the border with zones and the parametric title block, all from
//!   `cadrs_drawing::sheet_graphics` in sheet millimetres. Lines are gizmos whose width follows
//!   the zoom (the ISO 128 line widths, at least one pixel); text is `Text2d` re-rasterized at
//!   the zoomed size so it stays sharp.
//! - Navigation (D2.2, D2.3): the wheel zooms about the cursor, right- or middle-drag pans,
//!   there is no rotation, and **F** fits the sheet. Each sheet keeps its own view.
//! - A right-click on the sheet opens its menu (Sheet properties…, D2.8).
//! - **Ctrl+S** opens the Sheets flyout (D2.7); the wrench on the right edge the Drawing
//!   properties panel (D2.5). See [`panels`].
//! - The toolbar ([`toolbar`]), the Create Drawing dialog ([`create_dialog`]) and the Sheet
//!   properties dialog ([`sheet_dialog`]).
//! - Views (P3C.2): generated and drawn by [`views`], placed and moved by [`view_tools`]
//!   (Insert view, Projected view, Auxiliary view, Align view, dragging), and edited through
//!   the view context menu and its dialogs ([`view_menu`]). A right-click on a view opens its
//!   menu instead of the sheet's.
//! - Annotations (P3C.3): the centerline, centermark, virtual sharp, dimension and hole callout
//!   tools, selection, grips and drawing in [`annotations`]; the dimension palette and the Hole
//!   callout dialog in [`dim_palette`]. A right-click on an annotation opens its menu.
//! - Notes and tables (P3C.4): the Note and Table tools, typing on the sheet, grips and menus in
//!   [`notes`]; the note toolbar, the cell toolbar and the Table dialogs in [`note_bar`].
//! - Updating (P3C.6): views show the studio as the drawing kept it; the Update button turns
//!   gold when the workspace changed and Ctrl+Q (or the button) updates, see [`update`].
//! - Assembly drawings (P3C.5): views of assemblies, BOM tables and callouts, see
//!   [`bom_tools`] (the Insert BOM and Callout cards) and `cadrs_core::drawing_assembly`.

pub mod annotations;
pub mod bom_tools;
pub mod create_dialog;
pub mod dim_palette;
pub mod note_bar;
pub mod notes;
pub mod panels;
pub mod sheet_dialog;
pub mod sheet_items;
pub mod export_dialog;
pub mod toolbar;
pub mod update;
pub mod view_menu;
pub mod view_tools;
pub mod view_kind_tools;
pub mod view_linked;
pub mod symbol_cards;
pub mod views;

use std::collections::HashMap;

use bevy::camera::ScalingMode;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseScrollUnit;
use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::text::FontWeight;
use cadrs_core::{ElementId, ElementKind};
use cadrs_drawing::graphics::{Align, Weight};
use cadrs_drawing::{Drawing, Graphics, ObjectRef, ReferenceProps, SheetId};
use cadrs_ui::input::TextInputField;
use cadrs_ui::{RenderSurface, Theme};

use crate::viewport::{ActiveKind, ViewportArea, ViewportRect};
use crate::{ActiveDocument, AppState};

/// The render layer of the sheet (its camera sees only this).
pub const DRAWING_LAYER: usize = 7;

/// The grey around the sheet (`lesson-drawing-interface.png`).
pub fn ground_color() -> Color {
    Color::srgb_u8(0xe3, 0xe3, 0xe3)
}

/// Sheet line and text colour.
fn ink() -> Color {
    Color::srgb_u8(0x1a, 0x1a, 0x1a)
}

pub struct DrawingPlugin;

impl Plugin for DrawingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DrawingUi>()
            .init_resource::<SheetScreen>()
            .init_resource::<SheetScene>()
            .init_resource::<ElementClipboard>()
            .init_resource::<create_dialog::TemplateLibrary>()
            .init_gizmo_group::<SheetThin>()
            .init_gizmo_group::<SheetMedium>()
            .init_gizmo_group::<SheetThick>()
            .add_systems(Startup, (spawn_drawing_camera, configure_gizmos))
            .add_systems(OnExit(AppState::Document), reset_drawing_ui)
            .add_systems(
                Update,
                (
                    sync_drawing_chrome,
                    drawing_pointer,
                    drawing_keys,
                    fit_new_sheets,
                    apply_sheet_camera,
                    rebuild_sheet_scene,
                    size_sheet_lines,
                    draw_sheet_lines,
                    size_sheet_texts,
                )
                    .chain()
                    .after(crate::viewport::apply_view_to_camera)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(Update, deactivate_outside_documents.run_if(not(in_state(AppState::Document))))
            .add_plugins((
                panels::DrawingPanelsPlugin,
                create_dialog::CreateDrawingPlugin,
                sheet_dialog::SheetDialogPlugin,
                views::ViewsPlugin,
                view_tools::ViewToolsPlugin,
                view_linked::ViewLinkedPlugin,
                view_menu::ViewMenuPlugin,
                toolbar::DrawingToolbarPlugin,
                annotations::AnnotationsPlugin,
                bom_tools::BomToolsPlugin,
                dim_palette::DimPalettePlugin,
                notes::NotesPlugin,
                note_bar::NoteBarPlugin,
                update::UpdatePlugin,
            ))
            .add_plugins((sheet_items::SheetItemsPlugin, export_dialog::DrawingExportPlugin, symbol_cards::SymbolCardsPlugin));
    }
}

/// Thin sheet lines (0.25 mm and centre lines).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct SheetThin;

/// Medium sheet lines (0.35 mm).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct SheetMedium;

/// Thick sheet lines (0.7 mm).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct SheetThick;

/// The camera of the sheet.
#[derive(Component)]
pub struct DrawingCamera;

/// An entity of the drawn sheet (despawned when it is rebuilt).
#[derive(Component)]
struct SheetEntity;

/// A text of the sheet and its height on the sheet (mm).
#[derive(Component, Clone, Copy)]
pub(crate) struct SheetText {
    height: f32,
}

/// How one sheet is shown: the sheet point at the middle of the sheet area and the zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheetView {
    /// Sheet mm at the centre of the sheet area.
    pub center: Vec2,
    /// Logical pixels per sheet mm.
    pub ppm: f32,
    /// Still the fitted view (not zoomed or panned since): it re-fits when the sheet area
    /// changes (a side panel opens, the window resizes).
    pub fitted: bool,
}

/// Per-session drawing UI state (not saved, not undoable): each drawing's active sheet, each
/// sheet's view, and which side panels are open.
#[derive(Resource, Debug, Default)]
pub struct DrawingUi {
    pub active_sheet: HashMap<ElementId, SheetId>,
    /// Per (drawing, sheet): a duplicated or pasted drawing keeps its sheet ids.
    pub views: HashMap<(ElementId, SheetId), SheetView>,
    /// The Sheets flyout (Ctrl+S).
    pub sheets_open: bool,
    /// The Drawing properties panel (wrench).
    pub props_open: bool,
    /// The Drawing properties panel's icon tab.
    pub props_section: usize,
    drag: DragState,
    /// The active view tool (P3C.2).
    pub tool: view_tools::ViewTool,
    /// The selected views and the view under the pointer.
    pub selected: Vec<cadrs_drawing::ViewId>,
    pub hovered: Option<cadrs_drawing::ViewId>,
    /// The view the tool would place (drawn in blue, following the cursor), and the id it
    /// uses while it is a ghost.
    pub ghost: Option<cadrs_drawing::View>,
    pub ghost_id: cadrs_drawing::ViewId,
    /// The edge an auxiliary view or an alignment would use: (view, projected edge).
    pub highlight_edge: Option<(cadrs_drawing::ViewId, usize)>,
    /// The pointer on the sheet (mm), and where the primary button went down.
    pub pointer: Option<Vec2>,
    pub press: Option<Vec2>,
    /// A view being dragged, and where the drag would put the views it moves.
    pub view_drag: Option<ViewDrag>,
    pub drag_preview: Vec<(cadrs_drawing::ViewId, [f64; 2])>,
    /// An annotation tool is active (P3C.3): views are not hovered or picked.
    pub annotating: bool,
    /// The primary press went to an annotation (a tool click, a grip, an annotation).
    pub annotation_press: bool,
    /// The pointer is over an annotation (the view under it isn't highlighted).
    pub annotation_hover: bool,
    /// A boundary's points so far (spline crop, broken-out section; the view's 2D frame).
    pub tool_points: Vec<[f64; 2]>,
    /// What a view-kind tool previews (sheet mm): a cutting line, a circle, a rectangle.
    pub tool_strokes: Vec<Vec<[f64; 2]>>,
}

/// A view pressed on: dragged once the pointer moves a few pixels.
#[derive(Debug, Clone, Copy)]
pub struct ViewDrag {
    pub view: cadrs_drawing::ViewId,
    pub start: Vec2,
    pub moving: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct DragState {
    pointer: Vec2,
    /// Right or middle button held since a press over the sheet.
    panning: bool,
    secondary_down: Option<Vec2>,
    moved: f32,
}

/// A tab copied with "Copy to clipboard" (D2.10), pasted with the "+" menu.
#[derive(Resource, Debug, Default, Clone)]
pub struct ElementClipboard(pub Option<cadrs_core::Element>);

/// The active tab's drawing, if it is a Drawing tab.
pub fn active_drawing(doc: &ActiveDocument) -> Option<(ElementId, &Drawing)> {
    let el = doc.active_element()?;
    match &el.kind {
        ElementKind::Drawing(d) => Some((el.id, d)),
        _ => None,
    }
}

impl DrawingUi {
    /// The index of the active sheet of drawing `id` (the first if none was chosen or it is
    /// gone).
    pub fn sheet_index(&self, id: ElementId, d: &Drawing) -> usize {
        self.active_sheet
            .get(&id)
            .and_then(|s| d.sheet_index(*s))
            .unwrap_or(0)
    }
}

/// The part or assembly properties a sheet's title block shows (name, description and material
/// now; the part number and revision come with the property model, P3B.6).
pub fn reference_props(doc: &cadrs_core::Document, r: Option<ObjectRef>) -> ReferenceProps {
    cadrs_core::drawing_export::reference_props(doc, r)
}

// ---------------------------------------------------------------------------------------------
// Camera and chrome

fn spawn_drawing_camera(mut commands: Commands, surface: Res<RenderSurface>) {
    commands.spawn((
        Name::new("drawing-camera"),
        DrawingCamera,
        Camera2d,
        surface.target.clone(),
        Camera {
            // After the 3D camera (0), before the overlay camera that draws the UI (2); the
            // occluded-scene camera (also 1) is off while a drawing is shown.
            order: 1,
            is_active: false,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::WindowSize,
            ..OrthographicProjection::default_2d()
        }),
        RenderLayers::layer(DRAWING_LAYER),
        Tonemapping::None,
        DebandDither::Disabled,
    ));
}

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (cfg, _) = store.config_mut::<SheetThin>();
    cfg.render_layers = RenderLayers::layer(DRAWING_LAYER);
    let (cfg, _) = store.config_mut::<SheetMedium>();
    cfg.render_layers = RenderLayers::layer(DRAWING_LAYER);
    let (cfg, _) = store.config_mut::<SheetThick>();
    cfg.render_layers = RenderLayers::layer(DRAWING_LAYER);
}

fn reset_drawing_ui(mut ui: ResMut<DrawingUi>, mut scene: ResMut<SheetScene>, mut commands: Commands, q: Query<Entity, With<SheetEntity>>) {
    *ui = DrawingUi::default();
    *scene = SheetScene::default();
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn deactivate_outside_documents(mut q: Query<&mut Camera, With<DrawingCamera>>) {
    for mut c in &mut q {
        if c.is_active {
            c.is_active = false;
        }
    }
}

/// Turns the sheet camera on for Drawing tabs (and the occluded-scene camera off, as they
/// share an order) and hides the Part Studio chrome: the feature panel, view cube, the
/// right-edge panel strip and the viewport tools.
#[allow(clippy::type_complexity)]
fn sync_drawing_chrome(
    kind: Res<ActiveKind>,
    mut q_cam: Query<(&mut Camera, Option<&DrawingCamera>, &Name)>,
    mut q_nodes: Query<(&Name, &mut Node, &mut Visibility), With<Node>>,
) {
    let drawing = *kind == ActiveKind::Drawing;
    for (mut cam, is_drawing, name) in &mut q_cam {
        if is_drawing.is_some() {
            if cam.is_active != drawing {
                cam.is_active = drawing;
            }
        } else if name.as_str() == "occluded-camera" && cam.is_active == drawing {
            cam.is_active = !drawing;
        }
    }
    for (name, mut node, mut vis) in &mut q_nodes {
        match name.as_str() {
            "feature-panel" => {
                let d = if drawing { Display::None } else { Display::Flex };
                if node.display != d {
                    node.display = d;
                }
            }
            "view-cube" | "right-panel-strip" | "viewport-tools" | "origin" => {
                // P3H.3: a PCB Studio keeps the view cube and has its own right-edge toggles.
                let pcb = *kind == ActiveKind::PcbStudio && name.as_str() != "view-cube";
                let v = if kind.is_flat() || pcb {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                };
                vis.set_if_neq(v);
            }
            _ => {}
        }
    }
}

/// The part of the viewport the sheet is shown in: the viewport minus the open side panels.
pub fn sheet_area(rect: &ViewportRect, ui: &DrawingUi) -> Rect {
    let mut r = rect.0;
    if ui.sheets_open {
        r.min.x += panels::SHEETS_WIDTH;
    }
    if ui.props_open {
        r.max.x -= panels::PROPS_WIDTH;
    }
    r.max.x = r.max.x.max(r.min.x + 1.0);
    r
}

/// The view that shows the whole sheet with a margin (F).
pub fn fitted_view(sheet_mm: (f64, f64), area: Rect) -> SheetView {
    let (w, h) = (sheet_mm.0 as f32, sheet_mm.1 as f32);
    let margin = 28.0;
    let aw = (area.width() - 2.0 * margin).max(20.0);
    let ah = (area.height() - 2.0 * margin).max(20.0);
    SheetView {
        center: Vec2::new(w / 2.0, h / 2.0),
        ppm: (aw / w).min(ah / h),
        fitted: true,
    }
}

/// A screen position (logical px) on the sheet (mm).
pub fn screen_to_sheet(view: SheetView, area: Rect, p: Vec2) -> Vec2 {
    let d = (p - area.center()) / view.ppm;
    view.center + Vec2::new(d.x, -d.y)
}

/// A sheet point (mm) on the screen (logical px).
pub fn sheet_to_screen(view: SheetView, area: Rect, s: Vec2) -> Vec2 {
    let d = (s - view.center) * view.ppm;
    area.center() + Vec2::new(d.x, -d.y)
}

/// Gives a sheet shown for the first time a fitted view, and keeps a fitted view fitted.
fn fit_new_sheets(doc: Option<Res<ActiveDocument>>, rect: Res<ViewportRect>, mut ui: ResMut<DrawingUi>) {
    let Some(doc) = doc else {
        return;
    };
    let Some((id, d)) = active_drawing(&doc) else {
        return;
    };
    let i = ui.sheet_index(id, d);
    let Some(sheet) = d.sheets.get(i) else {
        return;
    };
    let key = (id, sheet.id);
    let refit = ui.views.get(&key).is_none_or(|v| v.fitted);
    if refit {
        let area = sheet_area(&rect, &ui);
        let v = fitted_view(sheet.size_mm(), area);
        if ui.views.get(&key) != Some(&v) {
            ui.views.insert(key, v);
        }
    }
}

/// The active sheet (drawing and sheet id) and its view, if a drawing is shown.
pub fn current_view(doc: &ActiveDocument, ui: &DrawingUi) -> Option<((ElementId, SheetId), SheetView)> {
    let (id, d) = active_drawing(doc)?;
    let sheet = d.sheets.get(ui.sheet_index(id, d))?;
    Some(((id, sheet.id), *ui.views.get(&(id, sheet.id))?))
}

/// Where the active sheet is on the screen: the screen point (logical px) of sheet (0, 0) and
/// the pixels per sheet mm (`None` when no drawing is shown). Scenarios address sheet points
/// with it.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct SheetScreen(pub Option<(Vec2, f32)>);

fn apply_sheet_camera(
    doc: Option<Res<ActiveDocument>>,
    ui: Res<DrawingUi>,
    rect: Res<ViewportRect>,
    surface: Res<RenderSurface>,
    kind: Res<ActiveKind>,
    mut screen: ResMut<SheetScreen>,
    mut q: Query<(&Camera, &mut Transform, &mut Projection), With<DrawingCamera>>,
) {
    let current = doc
        .as_deref()
        .filter(|_| *kind == ActiveKind::Drawing)
        .and_then(|doc| current_view(doc, &ui))
        .map(|(_, v)| (sheet_to_screen(v, sheet_area(&rect, &ui), Vec2::ZERO), v.ppm));
    if screen.0 != current {
        screen.0 = current;
    }
    let Some(doc) = doc else {
        return;
    };
    let Some((_, view)) = current_view(&doc, &ui) else {
        return;
    };
    let area = sheet_area(&rect, &ui);
    for (cam, mut t, mut proj) in &mut q {
        let window = cam
            .logical_viewport_size()
            .unwrap_or(surface.size.as_vec2());
        // The world point at the window's centre.
        let c = screen_to_sheet(view, area, window / 2.0);
        let want = Vec3::new(c.x, c.y, 0.0);
        if t.translation != want {
            t.translation = want;
        }
        if let Projection::Orthographic(o) = &mut *proj {
            let s = 1.0 / view.ppm;
            if o.scale != s {
                o.scale = s;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Input

fn pointer_over_sheet(hover: &HoverMap, q_area: &Query<Entity, With<ViewportArea>>) -> bool {
    let Some(hits) = hover.get(&PointerId::Mouse) else {
        return false;
    };
    q_area.iter().any(|e| hits.contains_key(&e))
}

#[allow(clippy::too_many_arguments)]
fn drawing_pointer(
    mut inputs: MessageReader<PointerInput>,
    kind: Res<ActiveKind>,
    doc: Option<Res<ActiveDocument>>,
    rect: Res<ViewportRect>,
    hover: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut ui: ResMut<DrawingUi>,
    mut commands: Commands,
) {
    if *kind != ActiveKind::Drawing {
        inputs.clear();
        return;
    }
    let Some(doc) = doc else {
        return;
    };
    let Some((sheet, mut view)) = current_view(&doc, &ui) else {
        inputs.clear();
        return;
    };
    let over = pointer_over_sheet(&hover, &q_area);
    let area = sheet_area(&rect, &ui);
    let mut drag = ui.drag;
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Move { .. } => {
                let delta = pos - drag.pointer;
                drag.pointer = pos;
                drag.moved += delta.length();
                if drag.panning && delta != Vec2::ZERO {
                    view.center -= Vec2::new(delta.x, -delta.y) / view.ppm;
                    view.fitted = false;
                }
            }
            PointerAction::Press(b) => {
                drag.pointer = pos;
                drag.moved = 0.0;
                if over && matches!(b, PointerButton::Secondary | PointerButton::Middle) {
                    drag.panning = true;
                }
                if b == PointerButton::Secondary {
                    drag.secondary_down = over.then_some(pos);
                }
            }
            PointerAction::Release(b) => {
                drag.pointer = pos;
                if matches!(b, PointerButton::Secondary | PointerButton::Middle) {
                    drag.panning = false;
                }
                if b == PointerButton::Secondary
                    && let Some(down) = drag.secondary_down.take()
                    && over
                    && down.distance(pos) < 4.0
                {
                    commands.queue(move |world: &mut World| {
                        view_menu::open_space_menu(world, pos);
                    });
                }
            }
            PointerAction::Scroll { unit, y, .. } => {
                if over {
                    let lines = match unit {
                        MouseScrollUnit::Line => y,
                        MouseScrollUnit::Pixel => y / 40.0,
                    };
                    let before = screen_to_sheet(view, area, pos);
                    view.ppm = (view.ppm * 1.2f32.powf(lines)).clamp(0.05, 400.0);
                    let after = screen_to_sheet(view, area, pos);
                    view.center += before - after;
                    view.fitted = false;
                }
            }
            PointerAction::Cancel => {
                drag = DragState::default();
            }
        }
    }
    ui.drag = drag;
    if ui.views.get(&sheet) != Some(&view) {
        ui.views.insert(sheet, view);
    }
}

/// F fits the sheet; Ctrl+S opens or closes the Sheets flyout; Ctrl+Q updates the drawing from
/// the workspace (X12).
#[allow(clippy::too_many_arguments)]
pub(crate) fn drawing_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    kind: Res<ActiveKind>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<TextInputField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    doc: Option<Res<ActiveDocument>>,
    rect: Res<ViewportRect>,
    mut ui: ResMut<DrawingUi>,
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
        if k.state != bevy::input::ButtonState::Pressed || typing || !q_dialogs.is_empty() || alt {
            continue;
        }
        match k.key_code {
            KeyCode::KeyF if !ctrl && !shift => {
                if let Some(doc) = doc.as_deref()
                    && let Some((id, d)) = active_drawing(doc)
                    && let Some(sheet) = d.sheets.get(ui.sheet_index(id, d))
                {
                    let area = sheet_area(&rect, &ui);
                    let v = fitted_view(sheet.size_mm(), area);
                    ui.views.insert((id, sheet.id), v);
                }
            }
            KeyCode::KeyS if ctrl && !shift => {
                ui.sheets_open = !ui.sheets_open;
            }
            KeyCode::KeyQ if ctrl && !shift => {
                commands.queue(update::start_update);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The sheet scene

/// What the sheet was last built from, and its lines and circles (drawn every frame).
#[derive(Resource, Default)]
struct SheetScene {
    key: Option<(ElementId, SheetId, Drawing, ReferenceProps)>,
    graphics: Graphics,
    /// The sheet's size, for the paper and its shadow.
    size: Vec2,
}

fn rebuild_sheet_scene(
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    ui: Res<DrawingUi>,
    theme: Res<Theme>,
    mut scene: ResMut<SheetScene>,
    q: Query<Entity, With<SheetEntity>>,
    mut commands: Commands,
) {
    let key = doc.as_deref().filter(|_| *kind == ActiveKind::Drawing).and_then(|doc| {
        let (id, d) = active_drawing(doc)?;
        let i = ui.sheet_index(id, d);
        let sheet = d.sheets.get(i)?;
        let props = reference_props(&doc.doc, sheet.reference);
        Some((id, sheet.id, d.clone(), props))
    });
    if key == scene.key {
        return;
    }
    for e in &q {
        commands.entity(e).despawn();
    }
    scene.key = key.clone();
    scene.graphics = Graphics::default();
    let Some((_, sheet_id, d, props)) = key else {
        return;
    };
    let i = d.sheet_index(sheet_id).unwrap_or(0);
    let (w, h) = d.sheets[i].size_mm();
    let size = Vec2::new(w as f32, h as f32);
    scene.size = size;
    scene.graphics = cadrs_drawing::sheet_graphics(&d, i, &props);
    let layer = RenderLayers::layer(DRAWING_LAYER);
    // The grey ground. (A camera's clear colour only applies when it is the first to draw
    // into the frame, and the 3D camera draws first, so the ground is a large sprite.)
    commands.spawn((
        Name::new("drawing-ground"),
        SheetEntity,
        Sprite::from_color(ground_color(), Vec2::splat(1.0e6)),
        Transform::from_xyz(size.x / 2.0, size.y / 2.0, -10.0),
        layer.clone(),
    ));
    // The paper, with a soft shadow on the grey.
    commands.spawn((
        Name::new("drawing-sheet-shadow"),
        SheetEntity,
        Sprite::from_color(Color::srgba(0.0, 0.0, 0.0, 0.18), size),
        Transform::from_xyz(size.x / 2.0 + 1.0, size.y / 2.0 - 1.0, -2.0),
        layer.clone(),
    ));
    commands.spawn((
        Name::new("drawing-sheet"),
        SheetEntity,
        Sprite::from_color(Color::WHITE, size),
        Transform::from_xyz(size.x / 2.0, size.y / 2.0, -1.0),
        layer.clone(),
    ));
    for (k, t) in scene.graphics.texts.iter().enumerate() {
        let anchor = match t.align {
            Align::TopLeft => Anchor::TOP_LEFT,
            Align::Center => Anchor::CENTER,
            Align::BottomRight => Anchor::BOTTOM_RIGHT,
            Align::CenterLeft => Anchor::CENTER_LEFT,
        };
        let weight = if t.bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };
        commands.spawn((
            Name::new(format!("sheet-text-{k}")),
            SheetEntity,
            SheetText {
                height: t.height as f32,
            },
            Text2d::new(t.text.clone()),
            theme.font(12.0, weight),
            TextColor(ink()),
            anchor,
            Transform::from_xyz(t.pos[0] as f32, t.pos[1] as f32, 1.0),
            layer.clone(),
        ));
    }
}

/// Inter's cap height as a fraction of the font size.
pub(crate) const CAP_HEIGHT: f32 = 0.727;

/// Keeps the sheet text's world height fixed while rasterizing it at its on-screen size.
pub(crate) fn size_sheet_texts(
    doc: Option<Res<ActiveDocument>>,
    ui: Res<DrawingUi>,
    mut q: Query<(&SheetText, &mut TextFont, &mut Transform)>,
) {
    let Some(doc) = doc else {
        return;
    };
    let Some((_, view)) = current_view(&doc, &ui) else {
        return;
    };
    for (t, mut font, mut tr) in &mut q {
        let em_mm = t.height / CAP_HEIGHT;
        // Round the pixel size so small zoom steps do not re-layout every text.
        let px = (em_mm * view.ppm).max(1.0).round();
        let want = bevy::text::FontSize::Px(px);
        if font.font_size != want {
            font.font_size = want;
        }
        let s = em_mm / px;
        if (tr.scale.x - s).abs() > 1e-6 {
            tr.scale = Vec3::new(s, s, 1.0);
        }
    }
}

/// Line widths follow the zoom: the ISO 128 width on paper, at least a pixel.
fn size_sheet_lines(doc: Option<Res<ActiveDocument>>, ui: Res<DrawingUi>, mut store: ResMut<GizmoConfigStore>) {
    let Some(doc) = doc else {
        return;
    };
    let Some((_, view)) = current_view(&doc, &ui) else {
        return;
    };
    let px = |w: Weight| (w.mm() as f32 * view.ppm).clamp(1.0, 8.0);
    store.config_mut::<SheetThin>().0.line.width = px(Weight::Thin);
    store.config_mut::<SheetMedium>().0.line.width = px(Weight::Medium);
    store.config_mut::<SheetThick>().0.line.width = px(Weight::Thick);
}

fn draw_sheet_lines(
    scene: Res<SheetScene>,
    mut thin: Gizmos<SheetThin>,
    mut medium: Gizmos<SheetMedium>,
    mut thick: Gizmos<SheetThick>,
) {
    if scene.key.is_none() {
        return;
    }
    let v = |p: [f64; 2]| Vec2::new(p[0] as f32, p[1] as f32);
    let c = ink();
    for l in &scene.graphics.lines {
        match l.weight {
            Weight::Thin | Weight::Center => thin.line_2d(v(l.a), v(l.b), c),
            Weight::Medium => medium.line_2d(v(l.a), v(l.b), c),
            Weight::Thick => thick.line_2d(v(l.a), v(l.b), c),
        }
    }
    for circle in &scene.graphics.circles {
        thin.circle_2d(Isometry2d::from_translation(v(circle.center)), circle.radius as f32, c)
            .resolution(48);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_centres_the_sheet_and_round_trips() {
        let area = Rect::new(226.0, 70.0, 1600.0, 971.0);
        let v = fitted_view((279.4, 215.9), area);
        assert_eq!(v.center, Vec2::new(139.7, 107.95));
        // The sheet's corners are inside the area.
        let bl = sheet_to_screen(v, area, Vec2::ZERO);
        let tr = sheet_to_screen(v, area, Vec2::new(279.4, 215.9));
        assert!(area.contains(bl) && area.contains(tr));
        let p = Vec2::new(700.0, 400.0);
        let back = sheet_to_screen(v, area, screen_to_sheet(v, area, p));
        assert!((back - p).length() < 1e-3);
    }
}
