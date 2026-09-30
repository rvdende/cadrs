//! Sheet sketch geometry, inserted DXF/DWG and images (P3C.7, D2.4, X13).
//!
//! - **Line** (toolbar): click the start, then each end; the lines chain from the last end until
//!   Esc. Ends snap to the ends and points of the sheet's other sketch items.
//! - **Spline**: click its points; a double-click, Enter or Esc finishes it (Esc with fewer than
//!   two points cancels). The curve through the points is previewed in orange.
//! - **Insert DXF or DWG**: a file picker ([`cadrs_ui::open_file_picker`]), then the drawing
//!   follows the cursor (its extent's bottom-left corner on it) until a click places it as one
//!   block. A DWG goes through the external converter (`cadrs_drawing::dwg`); without one it
//!   says what to install.
//! - **Insert image**: a PNG or JPEG, placed the same way, 18 mm wide at first.
//! - **Editing**: a click selects an item (orange, Ctrl adds); dragging moves it; its grips (line
//!   ends, spline points, image corners) reshape it; an image's corners keep its aspect ratio.
//!   Delete removes the selection. Every change is one undoable drawing edit.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use cadrs_drawing::DrawingOp;
use cadrs_drawing::annotation::PlacedText;
use cadrs_drawing::sheet_sketch::{self as sk, Entity as DxfEntity, ItemGrip, ItemId, ItemKind, SketchItem};
use cadrs_ui::input::TextInputField;
use cadrs_ui::{FilePicked, Notification, Theme, show_notification};

use super::annotations::{AnnTool, AnnotationUi, SceneText, blue, hover_orange, orange, square};
use super::view_tools::edit_drawing;
use super::{DRAWING_LAYER, DrawingUi, active_drawing, current_view, screen_to_sheet, sheet_area};
use crate::viewport::{ActiveKind, ViewportArea, ViewportRect};
use crate::{ActiveDocument, AppState};

pub struct SheetItemsPlugin;

impl Plugin for SheetItemsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SheetItemsUi>()
            .init_resource::<ItemScene>()
            .add_systems(
                Update,
                items_pointer
                    .after(super::notes::NotesInputSet)
                    .before(super::view_tools::ViewToolsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                items_keys.after(super::annotations::AnnotationInputSet).run_if(in_state(AppState::Document)),
            )
            .add_systems(Update, on_file_picked.run_if(in_state(AppState::Document)))
            .add_systems(
                Update,
                (rebuild_item_scene, draw_item_strokes, sync_images)
                    .chain()
                    .after(super::views::ViewsSet)
                    .before(super::annotations::AnnotationDrawSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut u: ResMut<SheetItemsUi>, mut s: ResMut<ItemScene>| {
                *u = SheetItemsUi::default();
                *s = ItemScene::default();
            });
    }
}

// ---------------------------------------------------------------------------------------------
// State

/// A grip (or an item's body) being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemDrag {
    pub id: ItemId,
    /// `None`: the body (moves the item).
    pub grip: Option<ItemGrip>,
    pub start: Vec2,
    pub at: Vec2,
    pub moving: bool,
}

/// Sheet sketch tool and selection state (not saved).
#[derive(Resource, Debug, Default)]
pub struct SheetItemsUi {
    pub selected: Vec<ItemId>,
    pub hovered: Option<ItemId>,
    pub drag: Option<ItemDrag>,
    /// Line tool: where the next line starts.
    pub chain: Option<[f64; 2]>,
    /// Spline tool: the points so far.
    pub spline: Vec<[f64; 2]>,
    /// The DXF block or image being placed (its origin at the cursor).
    pub placing: Option<SketchItem>,
    /// The last tool click: (time, sheet point), for double-clicks.
    last_click: Option<(f64, Vec2)>,
    /// Where the file pickers start (the last folder a file came from).
    pub last_dir: Option<PathBuf>,
    /// The frame of the last tool click: the pointer system skips that press (the click already
    /// placed and selected something; the tool may have ended in the same frame).
    tool_press: Option<u32>,
}

impl SheetItemsUi {
    /// A line chain or spline is being drawn (Esc ends it before it ends the tool).
    pub fn drawing(&self) -> bool {
        self.chain.is_some() || !self.spline.is_empty()
    }

    fn reset_tool(&mut self) {
        self.chain = None;
        self.spline.clear();
        self.placing = None;
        self.last_click = None;
    }
}

/// The active sheet's items as drawn now.
#[derive(Resource, Default)]
pub struct ItemScene {
    key: Option<SceneKey>,
    /// (polyline, colour, medium weight).
    pub strokes: Vec<(Vec<Vec2>, Color, bool)>,
    pub fills: Vec<([Vec2; 3], Color)>,
    pub texts: Vec<SceneText>,
    /// Images to show (the one being placed too).
    pub images: Vec<(ItemId, sk::SheetImage)>,
}

#[derive(Clone, PartialEq)]
struct SceneKey {
    items: Vec<SketchItem>,
    selected: Vec<ItemId>,
    hovered: Option<ItemId>,
    drag: Option<ItemDrag>,
    chain: Option<[f64; 2]>,
    spline: Vec<[f64; 2]>,
    placing: Option<SketchItem>,
    pointer: Option<Vec2>,
    tool: AnnTool,
    ppm: f32,
}

fn ink() -> Color {
    super::views::ink()
}

fn f2(p: Vec2) -> [f64; 2] {
    [p.x as f64, p.y as f64]
}

fn v2(p: [f64; 2]) -> Vec2 {
    Vec2::new(p[0] as f32, p[1] as f32)
}

/// The active sheet's items.
fn sheet_items(doc: &ActiveDocument, dui: &DrawingUi) -> Option<(cadrs_drawing::SheetId, Vec<SketchItem>)> {
    let (id, d) = active_drawing(doc)?;
    let s = d.sheets.get(dui.sheet_index(id, d))?;
    Some((s.id, s.sketch.clone()))
}

/// The item as the drag would leave it.
fn dragged(item: &SketchItem, d: &ItemDrag) -> SketchItem {
    match d.grip {
        None => sk::moved(item, [(d.at.x - d.start.x) as f64, (d.at.y - d.start.y) as f64]),
        Some(g) => sk::drag_grip(item, g, f2(d.at)),
    }
}

/// The nearest sketch point within `tol` of `p` (line ends, spline points, image corners).
fn snap(items: &[SketchItem], p: Vec2, tol: f64, skip: Option<ItemId>) -> Option<[f64; 2]> {
    items
        .iter()
        .filter(|i| Some(i.id) != skip)
        .flat_map(|i| sk::item_grips(i).into_iter().map(|(q, _)| q))
        .map(|q| (q, (q[0] - p.x as f64).hypot(q[1] - p.y as f64)))
        .filter(|(_, d)| *d <= tol)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(q, _)| q)
}

/// How a line tool point was inferred (shown by the cursor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inference {
    Horizontal,
    Vertical,
    Point,
}

/// Where a line tool click lands: a snapped point, else horizontal or vertical from the chain's
/// start when within 3° or 8 px of it, else the cursor.
fn line_point(items: &[SketchItem], chain: Option<[f64; 2]>, p: Vec2, px: f64) -> ([f64; 2], Option<Inference>) {
    if let Some(q) = snap(items, p, 6.0 * px, None) {
        return (q, Some(Inference::Point));
    }
    let q = f2(p);
    if let Some(a) = chain {
        let (dx, dy) = (q[0] - a[0], q[1] - a[1]);
        let ang = dy.atan2(dx).to_degrees().rem_euclid(180.0);
        let near = |off: f64, a: f64| off.abs() <= 8.0 * px || a < 3.0;
        if near(dy, ang.min(180.0 - ang)) && dx.abs() > dy.abs() {
            return ([q[0], a[1]], Some(Inference::Horizontal));
        }
        if near(dx, (ang - 90.0).abs()) && dy.abs() > dx.abs() {
            return ([a[0], q[1]], Some(Inference::Vertical));
        }
    }
    (q, None)
}

// ---------------------------------------------------------------------------------------------
// Tools

/// Starts a sheet sketch tool (or ends it when it is active).
pub fn start_tool(world: &mut World, tool: AnnTool) {
    super::annotations::start_tool(world, tool);
    let mut ui = world.resource_mut::<SheetItemsUi>();
    ui.reset_tool();
    ui.selected.clear();
}

const DOUBLE_TIME: f64 = 0.45;

/// A click with a sheet sketch tool at sheet point `p`.
pub fn tool_click(w: &mut World, tool: AnnTool, p: Vec2) {
    let Some((sheet, items)) = w.get_resource::<ActiveDocument>().and_then(|doc| sheet_items(doc, w.resource::<DrawingUi>())) else {
        return;
    };
    let px = w
        .get_resource::<ActiveDocument>()
        .and_then(|doc| current_view(doc, w.resource::<DrawingUi>()))
        .map(|(_, v)| 1.0 / v.ppm as f64)
        .unwrap_or(0.1);
    let now = w.resource::<Time<Real>>().elapsed_secs_f64();
    let frame = w.resource::<bevy::diagnostic::FrameCount>().0;
    w.resource_mut::<SheetItemsUi>().tool_press = Some(frame);
    let double = {
        let ui = w.resource::<SheetItemsUi>();
        ui.last_click.is_some_and(|(t, at)| now - t < DOUBLE_TIME && (at - p).length() as f64 <= 5.0 * px)
    };
    w.resource_mut::<SheetItemsUi>().last_click = Some((now, p));
    match tool {
        AnnTool::SheetLine => {
            let chain = w.resource::<SheetItemsUi>().chain;
            let (q, _) = line_point(&items, chain, p, px);
            match chain {
                Some(a) if (a[0] - q[0]).hypot(a[1] - q[1]) > 1e-6 => {
                    let item = SketchItem::new(ItemKind::Line { a, b: q });
                    if edit_drawing(w, DrawingOp::AddSketchItems { sheet, items: vec![item] }) {
                        w.resource_mut::<SheetItemsUi>().chain = Some(q);
                    }
                }
                Some(_) => {
                    // A double-click on the chain's end ends it.
                    if double {
                        w.resource_mut::<SheetItemsUi>().chain = None;
                    }
                }
                None => w.resource_mut::<SheetItemsUi>().chain = Some(q),
            }
        }
        AnnTool::SheetSpline => {
            let q = snap(&items, p, 6.0 * px, None).unwrap_or(f2(p));
            if double {
                finish_spline(w);
                return;
            }
            w.resource_mut::<SheetItemsUi>().spline.push(q);
        }
        AnnTool::PlaceImport => {
            let Some(item) = w.resource_mut::<SheetItemsUi>().placing.take() else { return };
            let placed = sk::moved(&item, f2(p));
            let id = placed.id;
            if edit_drawing(w, DrawingOp::AddSketchItems { sheet, items: vec![placed] }) {
                w.resource_mut::<AnnotationUi>().tool = AnnTool::None;
                let mut ui = w.resource_mut::<SheetItemsUi>();
                ui.reset_tool();
                ui.selected = vec![id];
            }
        }
        _ => {}
    }
}

/// Adds the spline drawn so far (two points at least) and starts a new one.
fn finish_spline(w: &mut World) {
    let pts = std::mem::take(&mut w.resource_mut::<SheetItemsUi>().spline);
    w.resource_mut::<SheetItemsUi>().last_click = None;
    let mut clean: Vec<[f64; 2]> = Vec::new();
    for p in pts {
        if clean.last().is_none_or(|q| (q[0] - p[0]).hypot(q[1] - p[1]) > 1e-6) {
            clean.push(p);
        }
    }
    if clean.len() < 2 {
        return;
    }
    let Some((sheet, _)) = w.get_resource::<ActiveDocument>().and_then(|doc| sheet_items(doc, w.resource::<DrawingUi>())) else {
        return;
    };
    edit_drawing(w, DrawingOp::AddSketchItems { sheet, items: vec![SketchItem::new(ItemKind::Spline { points: clean })] });
}

/// Where the pickers start: the last folder used, else `$CADRS_IMPORT_DIR`, else the working
/// directory.
fn start_dir(w: &World) -> PathBuf {
    w.resource::<SheetItemsUi>()
        .last_dir
        .clone()
        .or_else(|| std::env::var_os("CADRS_IMPORT_DIR").map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Insert DXF or DWG (toolbar): the file picker.
pub fn open_dxf_picker(w: &mut World) {
    let dir = start_dir(w);
    let theme = w.resource::<Theme>().clone();
    let mut commands = w.commands();
    cadrs_ui::open_file_picker(&mut commands, &theme, "dxf-picker", "Insert DXF or DWG", "insert-dxf", dir, &["dxf", "dwg"]);
    w.flush();
}

/// Insert image (toolbar): the file picker.
pub fn open_image_picker(w: &mut World) {
    let dir = start_dir(w);
    let theme = w.resource::<Theme>().clone();
    let mut commands = w.commands();
    cadrs_ui::open_file_picker(&mut commands, &theme, "image-picker", "Insert image", "insert-image", dir, &["png", "jpg", "jpeg"]);
    w.flush();
}

/// An image's width when it is inserted (mm).
const IMAGE_WIDTH: f64 = 18.0;

fn load_import(tag: &str, path: &std::path::Path) -> Result<SketchItem, String> {
    match tag {
        "insert-dxf" => cadrs_drawing::export::block_from_file(path, [0.0, 0.0]).map(|b| SketchItem::new(ItemKind::Block(b))),
        "insert-image" => {
            let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let px = cadrs_drawing::export::image_size(&data).ok_or("not a PNG or JPEG image")?;
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
            Ok(SketchItem::new(ItemKind::Image(sk::image_item(&name, data, px, [0.0, 0.0], IMAGE_WIDTH))))
        }
        _ => Err("unknown import".into()),
    }
}

/// A file picked for Insert DXF or Insert image: it follows the cursor to be placed.
fn on_file_picked(mut msgs: MessageReader<FilePicked>, mut commands: Commands) {
    for m in msgs.read() {
        if m.tag != "insert-dxf" && m.tag != "insert-image" {
            continue;
        }
        let (tag, path) = (m.tag.clone(), m.path.clone());
        commands.queue(move |w: &mut World| {
            w.resource_mut::<SheetItemsUi>().last_dir = path.parent().map(|p| p.to_path_buf());
            match load_import(&tag, &path) {
                Ok(item) => {
                    super::annotations::start_tool(w, AnnTool::PlaceImport);
                    let mut ui = w.resource_mut::<SheetItemsUi>();
                    ui.reset_tool();
                    ui.selected.clear();
                    ui.placing = Some(item);
                }
                Err(e) => {
                    let theme = w.resource::<Theme>().clone();
                    let mut commands = w.commands();
                    show_notification(&mut commands, &theme, Notification::warning(format!("Cannot insert {}: {e}", path.display())).name("insert-toast"));
                    w.flush();
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Pointer and keys

#[allow(clippy::too_many_arguments)]
fn items_pointer(
    mut inputs: MessageReader<PointerInput>,
    kind: Res<ActiveKind>,
    doc: Option<Res<ActiveDocument>>,
    rect: Res<ViewportRect>,
    hover_map: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    keys: Res<ButtonInput<KeyCode>>,
    ann: Res<AnnotationUi>,
    frame: Res<bevy::diagnostic::FrameCount>,
    mut dui: ResMut<DrawingUi>,
    mut ui: ResMut<SheetItemsUi>,
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
    let Some((_, view)) = current_view(&doc, &dui) else {
        inputs.clear();
        return;
    };
    let Some((_, items)) = sheet_items(&doc, &dui) else {
        inputs.clear();
        return;
    };
    let area = sheet_area(&rect, &dui);
    let over = super::view_tools::pointer_over_sheet(&hover_map, &q_area) && q_menus.is_empty() && q_dialogs.is_empty();
    let px = 1.0 / view.ppm as f64;
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let hit = |p: Vec2, tol: f64| {
        items
            .iter()
            .map(|i| (i.id, sk::item_distance(i, f2(p))))
            .filter(|(_, d)| *d <= tol)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    };
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let p = screen_to_sheet(view, area, input.location.position);
        match input.action {
            PointerAction::Move { .. } => {
                if let Some(mut d) = ui.drag {
                    d.at = p;
                    if !d.moving && (p - d.start).length() as f64 > 3.0 * px {
                        d.moving = true;
                    }
                    ui.drag = Some(d);
                }
            }
            PointerAction::Press(PointerButton::Primary) if over => {
                if ann.tool != AnnTool::None || ui.tool_press == Some(frame.0) {
                    continue;
                }
                if dui.annotation_press {
                    // An annotation, note or table took the press.
                    if !ctrl {
                        ui.selected.clear();
                    }
                    continue;
                }
                // A grip of a selected item first.
                let grip = items.iter().filter(|i| ui.selected.contains(&i.id)).find_map(|i| {
                    sk::item_grips(i)
                        .into_iter()
                        .find(|(q, _)| (q[0] - p.x as f64).hypot(q[1] - p.y as f64) <= 6.0 * px)
                        .map(|(_, g)| (i.id, g))
                });
                if let Some((id, g)) = grip {
                    dui.annotation_press = true;
                    ui.drag = Some(ItemDrag { id, grip: Some(g), start: p, at: p, moving: false });
                    continue;
                }
                match hit(p, 4.0 * px) {
                    Some(id) => {
                        dui.annotation_press = true;
                        dui.selected.clear();
                        if ctrl {
                            if let Some(i) = ui.selected.iter().position(|s| *s == id) {
                                ui.selected.remove(i);
                            } else {
                                ui.selected.push(id);
                            }
                        } else if !ui.selected.contains(&id) {
                            ui.selected = vec![id];
                        }
                        ui.drag = Some(ItemDrag { id, grip: None, start: p, at: p, moving: false });
                    }
                    None => {
                        if !ctrl {
                            ui.selected.clear();
                        }
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) => {
                if let Some(d) = ui.drag.take()
                    && d.moving
                {
                    commands.queue(move |w: &mut World| finish_drag(w, d));
                }
            }
            PointerAction::Cancel => ui.drag = None,
            _ => {}
        }
    }
    // Hover highlight (no tool, not over an annotation or note).
    let hovered = match (ann.tool, dui.pointer, over, ui.drag) {
        (AnnTool::None, Some(p), true, None) if !dui.annotation_hover => hit(p, 4.0 * px),
        _ => None,
    };
    if ui.hovered != hovered {
        ui.hovered = hovered;
    }
    if (hovered.is_some() || ui.drag.is_some()) && !dui.annotation_hover {
        dui.annotation_hover = true;
    }
}

fn finish_drag(w: &mut World, d: ItemDrag) {
    let Some((sheet, items)) = w.get_resource::<ActiveDocument>().and_then(|doc| sheet_items(doc, w.resource::<DrawingUi>())) else {
        return;
    };
    let Some(item) = items.iter().find(|i| i.id == d.id) else { return };
    // Moving a selection moves every selected item.
    let selected = w.resource::<SheetItemsUi>().selected.clone();
    let moved: Vec<SketchItem> = match d.grip {
        None if selected.contains(&d.id) => items.iter().filter(|i| selected.contains(&i.id)).map(|i| dragged(i, &d)).collect(),
        _ => vec![dragged(item, &d)],
    };
    if moved.iter().all(|m| items.contains(m)) {
        return;
    }
    let noun = item.noun();
    let label = match (d.grip, &item.kind) {
        (None, _) if moved.len() > 1 => "Move sketch entities".to_string(),
        (None, _) => format!("Move {noun}"),
        (Some(ItemGrip::Corner(_)), _) => "Resize image".to_string(),
        (Some(_), ItemKind::Line { .. }) => "Drag line end".to_string(),
        (Some(_), _) => "Drag spline point".to_string(),
    };
    edit_drawing(w, DrawingOp::SetSketchItems { sheet, items: moved, label });
}

/// Delete removes the selected items; Esc and Enter finish a line chain or spline; Esc then
/// clears the selection.
#[allow(clippy::too_many_arguments)]
fn items_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    kind: Res<ActiveKind>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<TextInputField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    ann: Res<AnnotationUi>,
    mut ui: ResMut<SheetItemsUi>,
    mut commands: Commands,
) {
    if *kind != ActiveKind::Drawing {
        keys_in.clear();
        return;
    }
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    for k in keys_in.read() {
        if k.state != bevy::input::ButtonState::Pressed || typing || !q_dialogs.is_empty() || !q_menus.is_empty() || ctrl {
            continue;
        }
        match k.key_code {
            KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter if !ui.spline.is_empty() => {
                commands.queue(finish_spline);
            }
            KeyCode::Escape if ui.chain.is_some() => ui.chain = None,
            KeyCode::Escape if ui.placing.is_some() => ui.placing = None,
            KeyCode::Escape if ann.tool == AnnTool::None && !ui.selected.is_empty() => ui.selected.clear(),
            KeyCode::Delete | KeyCode::Backspace if ann.tool == AnnTool::None && !ui.selected.is_empty() => {
                let ids = std::mem::take(&mut ui.selected);
                commands.queue(move |w: &mut World| {
                    let Some((sheet, items)) = w.get_resource::<ActiveDocument>().and_then(|doc| sheet_items(doc, w.resource::<DrawingUi>())) else {
                        return;
                    };
                    let ids: Vec<ItemId> = ids.into_iter().filter(|id| items.iter().any(|i| i.id == *id)).collect();
                    if !ids.is_empty() {
                        edit_drawing(w, DrawingOp::DeleteSketchItems { sheet, ids });
                    }
                });
            }
            _ => {}
        }
    }
    // A tool that ended (Esc, another tool) drops what it was drawing.
    if !matches!(ann.tool, AnnTool::SheetLine | AnnTool::SheetSpline | AnnTool::PlaceImport)
        && (ui.chain.is_some() || !ui.spline.is_empty() || ui.placing.is_some())
    {
        ui.reset_tool();
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

fn rebuild_item_scene(
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    dui: Res<DrawingUi>,
    ann: Res<AnnotationUi>,
    ui: Res<SheetItemsUi>,
    mut scene: ResMut<ItemScene>,
) {
    let Some(doc) = doc.filter(|_| *kind == ActiveKind::Drawing) else {
        if scene.key.is_some() {
            *scene = ItemScene::default();
        }
        return;
    };
    let Some((_, items)) = sheet_items(&doc, &dui) else {
        if scene.key.is_some() {
            *scene = ItemScene::default();
        }
        return;
    };
    let ppm = current_view(&doc, &dui).map(|(_, v)| v.ppm).unwrap_or(1.0);
    let key = SceneKey {
        items: items.clone(),
        selected: ui.selected.clone(),
        hovered: ui.hovered,
        drag: ui.drag.filter(|d| d.moving),
        chain: ui.chain,
        spline: ui.spline.clone(),
        placing: ui.placing.clone(),
        pointer: dui.pointer,
        tool: ann.tool,
        ppm,
    };
    if scene.key.as_ref() == Some(&key) {
        return;
    }
    let mut out = ItemScene::default();
    let px = 1.0 / ppm as f64;
    let grip_h = 3.0 / ppm;
    let drag = key.drag;
    let mut shown: Vec<(SketchItem, Color, bool)> = Vec::new();
    for it in &items {
        let selected = ui.selected.contains(&it.id);
        let it = match drag {
            Some(d) if d.id == it.id || (d.grip.is_none() && selected && ui.selected.contains(&d.id)) => dragged(it, &d),
            _ => it.clone(),
        };
        let color = if selected {
            orange()
        } else if ui.hovered == Some(it.id) {
            hover_orange()
        } else {
            ink()
        };
        shown.push((it, color, selected));
    }
    // What the tool is drawing, in orange.
    if let Some(p) = dui.pointer {
        match ann.tool {
            AnnTool::SheetLine => {
                let (q, inf) = line_point(&items, ui.chain, p, px);
                // The rubber band from the chain's last point to the cursor.
                if let Some(a) = ui.chain {
                    out.strokes.push((vec![v2(a), v2(q)], orange(), true));
                }
                out.fills.extend(square(v2(q), grip_h * 0.8, orange()));
                // The inference: an H or V tag beside the cursor (a snapped point is its square).
                let tag = match inf {
                    Some(Inference::Horizontal) => Some("H"),
                    Some(Inference::Vertical) => Some("V"),
                    _ => None,
                };
                if let Some(tag) = tag {
                    let h = 11.0 * px;
                    let at = [q[0] + 9.0 * px, q[1] + 11.0 * px];
                    let (w, pad) = (0.9 * h, 3.0 * px);
                    let (lo, hi) = ([at[0] - pad, at[1] - h / 2.0 - pad], [at[0] + w + pad, at[1] + h / 2.0 + pad]);
                    out.fills.push(([v2(lo), v2([hi[0], lo[1]]), v2(hi)], orange()));
                    out.fills.push(([v2(lo), v2(hi), v2([lo[0], hi[1]])], orange()));
                    out.texts.push(SceneText {
                        text: PlacedText { pos: at, height: h, text: tag.into() },
                        color: Color::WHITE,
                        bold: true,
                        italic: false,
                        rotation: 0.0,
                    });
                }
            }
            AnnTool::SheetSpline => {
                let mut pts = ui.spline.clone();
                pts.push(snap(&items, p, 6.0 * px, None).unwrap_or(f2(p)));
                if pts.len() >= 2 {
                    out.strokes.push((sk::spline_polyline(&pts, 24).into_iter().map(v2).collect(), orange(), true));
                }
                for q in &pts {
                    out.fills.extend(square(v2(*q), grip_h * 0.8, orange()));
                }
            }
            AnnTool::PlaceImport => {
                if let Some(item) = &ui.placing {
                    shown.push((sk::moved(item, f2(p)), orange(), false));
                }
            }
            _ => {}
        }
    }
    for (it, color, selected) in &shown {
        match &it.kind {
            ItemKind::Line { .. } | ItemKind::Spline { .. } => {
                for pl in sk::item_polylines(it) {
                    out.strokes.push((pl.into_iter().map(v2).collect(), *color, true));
                }
            }
            ItemKind::Block(b) => {
                for e in &b.entities {
                    match e {
                        DxfEntity::Text { at, height, text, rotation } => {
                            let (s, c) = rotation.to_radians().sin_cos();
                            let h = height * b.scale;
                            let base = sk::block_to_sheet(b, *at);
                            let mid = [base[0] - s * h / 2.0, base[1] + c * h / 2.0];
                            out.texts.push(SceneText {
                                text: PlacedText { pos: mid, height: h, text: text.clone() },
                                color: *color,
                                bold: false,
                                italic: false,
                                rotation: *rotation as f32,
                            });
                        }
                        DxfEntity::Solid { points } => {
                            let pts: Vec<Vec2> = points.iter().map(|p| v2(sk::block_to_sheet(b, *p))).collect();
                            for k in 1..pts.len().saturating_sub(1) {
                                out.fills.push(([pts[0], pts[k], pts[k + 1]], *color));
                            }
                        }
                        e => {
                            for pl in sk::entity_polylines(e) {
                                out.strokes.push((pl.into_iter().map(|p| v2(sk::block_to_sheet(b, p))).collect(), *color, false));
                            }
                        }
                    }
                }
                if *selected && let Some((lo, hi)) = sk::item_bounds(it) {
                    let r = [v2(lo), v2([hi[0], lo[1]]), v2(hi), v2([lo[0], hi[1]]), v2(lo)];
                    out.strokes.push((r.to_vec(), Color::srgb_u8(0xf3, 0xc0, 0x90), false));
                }
            }
            ItemKind::Image(img) => {
                out.images.push((it.id, img.clone()));
                if *color != ink() {
                    let c = img.corners();
                    out.strokes.push((vec![v2(c[0]), v2(c[1]), v2(c[2]), v2(c[3]), v2(c[0])], *color, false));
                }
            }
        }
        if *selected {
            for (q, _) in sk::item_grips(it) {
                out.fills.extend(square(v2(q), grip_h, blue()));
            }
        }
    }
    out.key = Some(key);
    *scene = out;
}

fn draw_item_strokes(
    scene: Res<ItemScene>,
    kind: Res<ActiveKind>,
    mut thin: Gizmos<super::views::ViewThin>,
    mut medium: Gizmos<super::views::ViewMedium>,
) {
    if *kind != ActiveKind::Drawing {
        return;
    }
    for (pts, c, m) in &scene.strokes {
        if pts.len() < 2 {
            continue;
        }
        if *m {
            medium.linestrip_2d(pts.iter().copied(), *c);
        } else {
            thin.linestrip_2d(pts.iter().copied(), *c);
        }
    }
}

/// A sheet image's sprite.
#[derive(Component)]
struct ImageSprite {
    id: ItemId,
    /// What it shows: (bytes' length and a sample, size and place).
    key: (usize, u64, [i64; 4]),
}

fn image_key(img: &sk::SheetImage) -> (usize, u64, [i64; 4]) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    img.data.hash(&mut h);
    let q = |x: f64| (x * 1000.0).round() as i64;
    (img.data.len(), h.finish(), [q(img.at[0]), q(img.at[1]), q(img.width), q(img.height)])
}

/// Spawns, moves and removes the images' sprites.
#[allow(clippy::too_many_arguments)]
fn sync_images(
    scene: Res<ItemScene>,
    kind: Res<ActiveKind>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut cache: Local<HashMap<u64, Handle<Image>>>,
    q: Query<(Entity, &ImageSprite)>,
    mut commands: Commands,
) {
    let want: &[(ItemId, sk::SheetImage)] = if *kind == ActiveKind::Drawing { &scene.images } else { &[] };
    let mut have = Vec::new();
    for (e, s) in &q {
        if want.iter().any(|(id, img)| *id == s.id && image_key(img) == s.key) && !have.contains(&s.key) {
            have.push(s.key);
        } else {
            commands.entity(e).despawn();
        }
    }
    for (id, img) in want {
        let key = image_key(img);
        if have.contains(&key) {
            continue;
        }
        let handle = match cache.get(&key.1) {
            Some(h) => h.clone(),
            None => {
                let Ok(rgba) = cadrs_drawing::export::decode_image(&img.data) else { continue };
                let (w, h) = rgba.dimensions();
                let image = Image::new(
                    Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    TextureDimension::D2,
                    rgba.into_raw(),
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                );
                let handle = images.add(image);
                if cache.len() > 32 {
                    cache.clear();
                }
                cache.insert(key.1, handle.clone());
                handle
            }
        };
        // A textured quad under the sheet's lines and text (like the shaded views' meshes).
        let size = Vec2::new(img.width as f32, img.height as f32);
        commands.spawn((
            Name::new(format!("sheet-image-{}", img.name)),
            ImageSprite { id: *id, key },
            Mesh2d(meshes.add(Rectangle::new(size.x, size.y))),
            MeshMaterial2d(materials.add(ColorMaterial {
                texture: Some(handle),
                alpha_mode: bevy::sprite_render::AlphaMode2d::Mask(0.5),
                ..ColorMaterial::from(Color::WHITE)
            })),
            Transform::from_xyz(img.at[0] as f32 + size.x / 2.0, img.at[1] as f32 + size.y / 2.0, -0.8),
            RenderLayers::layer(DRAWING_LAYER),
            DespawnOnExit(AppState::Document),
        ));
    }
}
