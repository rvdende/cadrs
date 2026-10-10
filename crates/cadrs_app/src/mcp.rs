//! The MCP server in the app (Model Context Protocol, [`cadrs_mcp`]): with the Preferences'
//! **Enable MCP server** on, AI assistants on this machine drive the running app at
//! `http://127.0.0.1:<port>/mcp`.
//!
//! - [`sync_server`] starts and stops the server as the preference changes (and at startup).
//! - [`run_calls`] takes the waiting tool calls each frame and runs them on the world, every
//!   edit through the command layer, so the assistant's steps show in the feature list and undo
//!   like the user's own. Each call answers with JSON the assistant reads.
//! - A screenshot waits until the app has settled (no [`cadrs_ui::PendingWork`]: a document
//!   loading, a rebuild, a view animation) for two frames, at most 10 s, then [`capture`]
//!   answers when the capture is ready: a PNG of the whole window, scaled to fit.

use std::net::{Ipv4Addr, SocketAddr};

use bevy::prelude::*;
use cadrs_core::commands::{AddElement, AddExtrude, AddSketch, NewElementKind};
use cadrs_core::document::ElementKind;
use cadrs_core::{BooleanOp, Document, DocumentMeta, ElementId, ExtrudeFeature, FeatureId, FeatureKind};
use cadrs_mcp::tools::{self, Operation, Plane, SketchEntity};
use cadrs_mcp::{Call, Server};
use cadrs_sketch::constraint::rectangle_constraints;
use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2 as SVec2};
use cadrs_ui::Theme;
use serde_json::{Value, json};

use crate::preferences_ui::LocalPreferences;
use crate::{ActiveDocument, AppClock, DocumentStore, UserProfile};

pub struct McpPlugin;

impl Plugin for McpPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<McpServer>()
            .init_resource::<WaitingShots>()
            .add_systems(Update, (sync_server, run_calls).chain())
            .add_systems(Last, take_screenshots);
    }
}

/// The server while it runs, and the port it was asked for.
#[derive(Resource, Default)]
pub struct McpServer {
    server: Option<Server>,
    /// The (enabled, port) the server was last set up for.
    applied: Option<(bool, u16)>,
}

impl McpServer {
    /// The endpoint while serving.
    pub fn url(&self) -> Option<String> {
        self.server.as_ref().map(Server::url)
    }
}

/// How the app runs a scenario for run_scenario: set by the binary, which has the scenario
/// runner (`cadrs_harness`). It gets the scenario's RON and an answer to call with the
/// screenshots it took, or why it failed.
#[derive(Resource, Clone, Copy)]
pub struct McpScenarioHook(pub fn(&mut World, &str, ScenarioDone) -> Result<(), String>);

/// How a scenario run answers.
pub type ScenarioDone = Box<dyn FnOnce(Result<Vec<std::path::PathBuf>, String>) + Send>;

/// Screenshots waiting for the app to settle.
#[derive(Resource, Default)]
struct WaitingShots(Vec<WaitingShot>);

struct WaitingShot {
    req: cadrs_mcp::Request,
    max: u32,
    /// Frames waited, and settled frames in a row.
    frames: u32,
    idle: u32,
}

/// Frames to wait for the app to settle before capturing anyway (about 10 s).
const SETTLE_LIMIT: u32 = 600;

/// Starts or stops the server when the preference changes.
fn sync_server(world: &mut World) {
    let mcp = world.resource::<LocalPreferences>().0.mcp;
    let want = (mcp.enabled, mcp.port);
    if world.resource::<McpServer>().applied == Some(want) {
        return;
    }
    let mut state = world.resource_mut::<McpServer>();
    state.applied = Some(want);
    state.server = None;
    if !mcp.enabled {
        return;
    }
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, mcp.port));
    let message = match Server::start(addr) {
        Ok(server) => {
            let url = server.url();
            info!("MCP server on {url}");
            state.server = Some(server);
            cadrs_ui::Notification::info(format!("MCP server on {url}"))
        }
        Err(e) => {
            warn!("cannot start the MCP server on {addr}: {e}");
            cadrs_ui::Notification::error(format!("Cannot start the MCP server on port {}: {e}", mcp.port))
        }
    };
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    cadrs_ui::show_notification(&mut commands, &theme, message.name("mcp-toast"));
    world.flush();
}

/// Runs the tool calls waiting for the app.
fn run_calls(world: &mut World) {
    let Some(server) = world.resource::<McpServer>().server.as_ref() else { return };
    let requests: Vec<_> = std::iter::from_fn(|| server.try_next()).collect();
    for req in requests {
        if let Call::RunScenario(p) = &req.call {
            let ron = p.scenario.clone();
            let Some(hook) = world.get_resource::<McpScenarioHook>().copied() else {
                req.reply(Err("this build of cadrs can't run scenarios".into()));
                continue;
            };
            // The request is answered when the scenario ends (or now, if it can't start).
            let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(req)));
            let answer = slot.clone();
            let done: ScenarioDone = Box::new(move |result| {
                let Some(req) = answer.lock().ok().and_then(|mut r| r.take()) else { return };
                match result {
                    Ok(shots) => {
                        let paths: Vec<String> = shots.iter().map(|p| p.display().to_string()).collect();
                        let caption = json!({ "screenshots": paths }).to_string();
                        match shots.last().and_then(|p| std::fs::read(p).ok()) {
                            Some(png) => req.reply_image(png, caption),
                            None => req.reply(Ok(json!({ "screenshots": paths }))),
                        }
                    }
                    Err(e) => req.reply(Err(e)),
                }
            });
            if let Err(e) = hook.0(world, &ron, done)
                && let Some(req) = slot.lock().ok().and_then(|mut r| r.take())
            {
                req.reply(Err(e));
            }
            continue;
        }
        if let Call::Screenshot(p) = &req.call {
            let max = p.max_size.unwrap_or(1600).clamp(64, 8192);
            if let Some(v) = &p.view {
                if let Err(e) = turn_view(world, v) {
                    req.reply(Err(e));
                    continue;
                }
            }
            world.resource_mut::<WaitingShots>().0.push(WaitingShot { req, max, frames: 0, idle: 0 });
            continue;
        }
        let result = run(world, &req.call);
        if let Err(e) = &result {
            info!("MCP call {:?} failed: {e}", req.call);
        }
        req.reply(result);
    }
}

fn run(world: &mut World, call: &Call) -> Result<Value, String> {
    match call {
        Call::CreateDocument(p) => create_document(world, &p.name),
        Call::OpenDocument(p) => open_document(world, &p.name),
        Call::GetDocument => describe(world),
        Call::AddPartStudio(p) => add_part_studio(world, p),
        Call::AddSketch(p) => add_sketch(world, p),
        Call::Extrude(p) => extrude(world, p),
        Call::Screenshot(_) | Call::RunScenario(_) => Err("handled by run_calls".into()),
        Call::AddFeature(p) => add_feature(world, p),
        Call::EditFeature(p) => edit_feature(world, p),
        Call::EditSketch(p) => edit_sketch(world, p).and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string())),
        Call::GetFeature(p) => get_feature(world, p),
        Call::DeleteFeature(p) => delete_feature(world, p),
        Call::ListFaces(p) => list_faces(world, p.part_studio.as_deref()),
        Call::ListEdges(p) => list_edges(world, p.part_studio.as_deref()),
        Call::Undo => {
            let mut d = doc_mut(world)?;
            let what = d.undo().ok_or("nothing to undo")?;
            Ok(json!({ "undone": what }))
        }
        Call::ImportStep(p) => import_step(world, &p.path),
        Call::ExportStep(p) => export_step(world, p),
        Call::CreateDrawing(p) => create_drawing(world, p),
        Call::AddAnnotation(p) => add_annotation(world, p),
        Call::RenameTab(p) => rename_tab(world, p),
    }
}

/// The sheet template a drawing uses when the call names none: third angle, as the course's
/// drawings are.
const DEFAULT_TEMPLATE: &str = "ANSI_C_MM.dwt";

/// "1:1" or "2:1" as a scale.
fn parse_scale(s: &str) -> Result<cadrs_drawing::standard::Scale, String> {
    let bad = || format!("scale {s:?}: write it as 1:1 or 2:1");
    let (a, b) = s.split_once(':').ok_or_else(bad)?;
    let num: u32 = a.trim().parse().map_err(|_| bad())?;
    let den: u32 = b.trim().parse().map_err(|_| bad())?;
    if num == 0 || den == 0 {
        return Err(bad());
    }
    Ok(cadrs_drawing::standard::Scale { num, den })
}

/// A view as create_drawing and add_annotation describe it.
fn view_json(v: &cadrs_drawing::View) -> Value {
    json!({
        "name": v.name,
        "id": serde_json::to_value(v.id).unwrap_or(Value::Null),
        "scale": format!("{}:{}", v.scale.num, v.scale.den),
        "anchor": v.anchor.map(round),
        "frame": { "dir": v.frame.dir.map(round), "x": v.frame.x.map(round) },
    })
}

/// Creates a drawing of a Part Studio, as the Create Drawing dialog's OK does with its four
/// views, and opens it.
fn create_drawing(world: &mut World, p: &tools::CreateDrawing) -> Result<Value, String> {
    let template_name = p.template.as_deref().unwrap_or(DEFAULT_TEMPLATE);
    let templates = cadrs_drawing::template::builtin_templates();
    let template = templates.iter().find(|t| t.name.eq_ignore_ascii_case(template_name)).cloned().ok_or_else(|| {
        let names: Vec<&str> = templates.iter().map(|t| t.name.as_str()).collect();
        format!("no sheet template named {template_name:?}; the built-in ones are {}", names.join(", "))
    })?;
    let scale = p.scale.as_deref().map(parse_scale).transpose()?;
    let element = part_studio(doc(world)?, p.part_studio.as_deref())?;
    let r = cadrs_drawing::ObjectRef { element: element.0, part: None };
    let drawings = doc(world)?.doc.elements.iter().filter(|e| matches!(e.kind, ElementKind::Drawing(_))).count();
    let name = p.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Drawing {}", drawings + 1));
    let mut drawing = cadrs_drawing::Drawing::from_template(&template, Some(r));
    drawing.title.drawn_by = Some(world.resource::<UserProfile>().display_name.clone());
    drawing.title.drawn_date = Some(crate::drawing::create_dialog::today(world.resource::<AppClock>()));
    if p.four_views.unwrap_or(true) {
        let mut views = crate::drawing::create_dialog::four_views_of(&doc(world)?.doc, &drawing, r);
        if views.is_empty() {
            return Err("the Part Studio has no parts to draw".into());
        }
        // P3C.6: the drawing shows the studio as it is now, until it is updated.
        if let Some(src) = cadrs_core::drawing_source::live_source(&doc(world)?.doc, element) {
            for v in &mut views {
                v.source_hash = src.hash_of(v.reference.part);
            }
            drawing.sources.push(src);
        }
        if let Some(sheet) = drawing.sheets.first_mut() {
            if let Some(f) = views.first() {
                sheet.scale = f.scale;
            }
            sheet.views = views;
        }
    }
    if let Some(s) = scale
        && let Some(sheet) = drawing.sheets.first_mut()
    {
        sheet.scale = s;
        for v in &mut sheet.views {
            v.scale = s;
        }
    }
    let views: Vec<Value> = drawing.sheets.first().map(|s| s.views.iter().map(view_json).collect()).unwrap_or_default();
    let sheet_scale = drawing.sheets.first().map(|s| format!("{}:{}", s.scale.num, s.scale.den)).unwrap_or_default();
    let element_drawing = cadrs_core::Element::drawing(name.clone(), drawing);
    let id = element_drawing.id;
    let mut d = doc_mut(world)?;
    let after = d.active;
    d.execute(&cadrs_core::commands::InsertElement { element: element_drawing, after, label: "Create Drawing".into() })
        .map_err(|e| e.to_string())?;
    d.set_active(id);
    Ok(json!({ "drawing": name, "template": template.name, "scale": sheet_scale, "views": views }))
}

/// Adds an annotation to a view of a drawing (the drawing is made the active tab first).
fn add_annotation(world: &mut World, p: &tools::AddAnnotation) -> Result<Value, String> {
    let kind: cadrs_drawing::annotation::AnnotationKind =
        serde_json::from_value(p.annotation.clone()).map_err(|e| format!("annotation: {e}"))?;
    if let Some(name) = &p.drawing {
        let mut d = doc_mut(world)?;
        let id = d
            .doc
            .elements
            .iter()
            .find(|e| e.name == *name && matches!(e.kind, ElementKind::Drawing(_)))
            .map(|e| e.id)
            .ok_or_else(|| format!("no drawing named {name:?}"))?;
        d.set_active(id);
    }
    let view = {
        let d = doc(world)?;
        let el = d.active_element().ok_or("no tab is active")?;
        let ElementKind::Drawing(drawing) = &el.kind else {
            return Err(format!("{:?} is not a drawing", el.name));
        };
        let views: Vec<(&str, cadrs_drawing::ViewId)> =
            drawing.sheets.iter().flat_map(|s| s.views.iter()).map(|v| (v.name.as_str(), v.id)).collect();
        views
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(&p.view))
            .map(|(_, id)| *id)
            .ok_or_else(|| {
                let names: Vec<&str> = views.iter().map(|(n, _)| *n).collect();
                format!("no view named {:?} in {:?}; its views are {}", p.view, el.name, names.join(", "))
            })?
    };
    let annotation = cadrs_drawing::annotation::Annotation::new(kind);
    let id = annotation.id;
    if !crate::drawing::view_tools::edit_drawing(world, cadrs_drawing::DrawingOp::AddAnnotation { view, annotation }) {
        return Err("the annotation could not be added".into());
    }
    Ok(json!({ "view": p.view, "annotation": serde_json::to_value(id).unwrap_or(Value::Null) }))
}

/// Renames a tab.
fn rename_tab(world: &mut World, p: &tools::RenameTab) -> Result<Value, String> {
    let mut d = doc_mut(world)?;
    let id = d.doc.elements.iter().find(|e| e.name == p.name).map(|e| e.id).ok_or_else(|| format!("no tab named {:?}", p.name))?;
    d.execute(&cadrs_core::commands::RenameElement { id, name: p.to.clone() }).map_err(|e| e.to_string())?;
    describe(world)
}

/// Turns the 3D view to a standard view and zooms to fit.
fn turn_view(world: &mut World, name: &str) -> Result<(), String> {
    use crate::camera::StandardView as V;
    let s = match name.to_ascii_lowercase().as_str() {
        "front" => V::Front,
        "back" => V::Back,
        "left" => V::Left,
        "right" => V::Right,
        "top" => V::Top,
        "bottom" => V::Bottom,
        "iso" | "isometric" => V::Isometric,
        _ => return Err(format!("unknown view {name:?}: front, back, left, right, top, bottom or iso")),
    };
    let mut view = world.resource_mut::<crate::viewport::ViewportView>();
    let to = view.target().oriented(s);
    view.animate_to(to);
    crate::viewport::zoom_to_fit(world);
    Ok(())
}

/// Takes the waiting screenshots once the app has settled (after every system that flags
/// pending work this frame).
fn take_screenshots(world: &mut World) {
    if world.resource::<WaitingShots>().0.is_empty() {
        return;
    }
    let busy = world.get_resource::<cadrs_ui::PendingWork>().is_some_and(|w| w.0);
    let why = world.get_resource::<cadrs_ui::PendingWhy>().map(|w| w.0.join(", ")).unwrap_or_default();
    let shots = std::mem::take(&mut world.resource_mut::<WaitingShots>().0);
    let mut keep = Vec::new();
    for mut shot in shots {
        shot.frames += 1;
        shot.idle = if busy { 0 } else { shot.idle + 1 };
        if shot.idle >= 2 {
            capture(world, shot.req, shot.max, None);
        } else if shot.frames >= SETTLE_LIMIT {
            capture(world, shot.req, shot.max, Some(why.clone()));
        } else {
            keep.push(shot);
        }
    }
    world.resource_mut::<WaitingShots>().0.extend(keep);
}

/// Captures the app's window (or, headless, the surface it renders to) and answers `req` with
/// it as a PNG whose longest side is at most `max` pixels. `busy`: what the app was still doing.
fn capture(world: &mut World, req: cadrs_mcp::Request, max: u32, busy: Option<String>) {
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
    let screenshot = if let Some(surface) = world.get_resource::<cadrs_ui::RenderSurface>() {
        Screenshot(surface.target.clone())
    } else if world.query_filtered::<(), With<bevy::window::PrimaryWindow>>().iter(world).next().is_some() {
        Screenshot::primary_window()
    } else {
        req.reply(Err("cadrs has no window to capture".into()));
        return;
    };
    // The observer may run more than once in principle; the request is answered once.
    let req = std::sync::Mutex::new(Some(req));
    world.spawn(screenshot).observe(move |shot: On<ScreenshotCaptured>| {
        let Some(req) = req.lock().ok().and_then(|mut r| r.take()) else { return };
        let img = match shot.image.clone().try_into_dynamic() {
            Ok(img) => img,
            Err(e) => {
                req.reply(Err(format!("cannot read the screenshot: {e:?}")));
                return;
            }
        };
        let (w, h) = (img.width(), img.height());
        let img = if w.max(h) > max { img.resize(max, max, image::imageops::FilterType::Triangle) } else { img };
        let mut caption = format!("The cadrs window ({w} × {h} px, shown at {} × {}).", img.width(), img.height());
        if let Some(why) = &busy {
            caption += &format!(" Taken while cadrs was still busy ({why}).");
        }
        let mut png = Vec::new();
        match img.to_rgb8().write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png) {
            Ok(()) => req.reply_image(png, caption),
            Err(e) => req.reply(Err(format!("cannot encode the screenshot: {e}"))),
        }
    });
}

/// Creates a stored document (Part Studio 1 and Assembly 1) and opens it, as the documents
/// page's Create does; the open document is saved first.
fn create_document(world: &mut World, name: &str) -> Result<Value, String> {
    let name = match name.trim() {
        "" => "Untitled document",
        n => n,
    };
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let doc = Document::new(name);
    let meta = DocumentMeta::new(&user, now);
    let id = doc.id;
    store.create(&doc, &meta).map_err(|e| format!("cannot create the document: {e}"))?;
    crate::move_document::open_document(world, id);
    let mut active = world.get_resource_mut::<ActiveDocument>().ok_or("the document did not open")?;
    active.fresh = true;
    describe(world)
}

/// Opens a stored document by name or id.
fn open_document(world: &mut World, name: &str) -> Result<Value, String> {
    let (lib, _) = world.resource::<DocumentStore>().0.list();
    let name = name.trim();
    let id = lib
        .entries
        .iter()
        .filter(|e| e.meta.trashed.is_none())
        .find(|e| e.id.to_string() == name || e.name == name)
        .map(|e| e.id)
        .ok_or_else(|| format!("no document named {name:?}"))?;
    crate::move_document::open_document(world, id);
    describe(world)
}

fn doc(world: &World) -> Result<&ActiveDocument, String> {
    world.get_resource::<ActiveDocument>().ok_or_else(|| "no document is open: call create_document or open_document first".into())
}

fn doc_mut(world: &mut World) -> Result<Mut<'_, ActiveDocument>, String> {
    world.get_resource_mut::<ActiveDocument>().ok_or_else(|| "no document is open: call create_document or open_document first".into())
}

fn kind_label(kind: &ElementKind) -> &'static str {
    match kind {
        ElementKind::PartStudio { .. } => "Part Studio",
        ElementKind::Assembly => "Assembly",
        ElementKind::Drawing(_) => "Drawing",
        ElementKind::PcbStudio(_) => "PCB Studio",
        ElementKind::Render(_) => "Render Studio",
    }
}

/// The Part Studio a call names, or the active tab.
fn part_studio(d: &ActiveDocument, name: Option<&str>) -> Result<ElementId, String> {
    let e = match name {
        Some(n) => d.doc.elements.iter().find(|e| e.name == n).ok_or_else(|| format!("no tab named {n:?}"))?,
        None => d.active_element().ok_or("no tab is active")?,
    };
    if !matches!(e.kind, ElementKind::PartStudio { .. }) {
        return Err(format!("{:?} is a {}, not a Part Studio", e.name, kind_label(&e.kind)));
    }
    Ok(e.id)
}

/// The document, its tabs, and the active Part Studio's features and parts.
fn describe(world: &World) -> Result<Value, String> {
    let d = doc(world)?;
    let tabs: Vec<Value> = d
        .doc
        .elements
        .iter()
        .map(|e| json!({ "name": e.name, "kind": kind_label(&e.kind), "active": d.active == Some(e.id) }))
        .collect();
    let mut out = json!({ "document": d.doc.name, "tabs": tabs });
    if let Some(e) = d.active_element().filter(|e| matches!(e.kind, ElementKind::PartStudio { .. })) {
        out["part_studio"] = studio_state(e.name.as_str(), e.features());
    }
    if d.meta.is_some() {
        out["id"] = json!(d.doc.id.to_string());
    }
    Ok(out)
}

/// A Part Studio's features (with their rebuild errors and warnings) and parts.
fn studio_state(name: &str, features: &[cadrs_core::Feature]) -> Value {
    let build = cadrs_core::rebuild::build(features);
    let problem = |id: FeatureId, list: &[(FeatureId, String)]| list.iter().find(|(f, _)| *f == id).map(|(_, m)| m.clone());
    let features: Vec<Value> = features
        .iter()
        .map(|f| {
            let mut v = json!({ "name": f.name, "type": cadrs_core::feature_list::type_label(&f.kind) });
            if let Some(e) = problem(f.id, &build.errors) {
                v["error"] = json!(e);
            }
            if let Some(w) = problem(f.id, &build.warnings) {
                v["warning"] = json!(w);
            }
            v
        })
        .collect();
    let parts: Vec<Value> = build
        .parts
        .iter()
        .map(|p| {
            let mut v = json!({ "name": p.name, "id": p.id });
            if let Some(m) = &p.mass {
                v["volume_mm3"] = json!(round(m.volume));
            }
            if let Some((lo, hi)) = p.solid.bounds() {
                v["bbox_mm"] = json!([lo.map(round), hi.map(round)]);
            }
            v
        })
        .collect();
    json!({ "name": name, "features": features, "parts": parts })
}

/// To the micrometre (and no "-0").
fn round(x: f64) -> f64 {
    let r = (x * 1000.0).round() / 1000.0;
    if r == 0.0 { 0.0 } else { r }
}

fn add_part_studio(world: &mut World, p: &tools::AddPartStudio) -> Result<Value, String> {
    let mut d = doc_mut(world)?;
    let id = ElementId::new();
    let after = d.active;
    d.execute(&AddElement { id, kind: NewElementKind::PartStudio, name: p.name.clone(), after }).map_err(|e| e.to_string())?;
    d.set_active(id);
    describe(world)
}

fn plane_ref(p: Plane) -> PlaneRef {
    match p {
        Plane::Top => PlaneRef::Top,
        Plane::Front => PlaneRef::Front,
        Plane::Right => PlaneRef::Right,
    }
}

/// The sketch operations that draw the entities.
fn sketch_ops(entities: &[SketchEntity]) -> Result<Vec<SketchOp>, String> {
    let v = SVec2::new;
    let mut ops = Vec::new();
    for e in entities {
        match *e {
            SketchEntity::Rectangle { x0, y0, x1, y1 } => {
                if (x1 - x0).abs() < 1e-9 || (y1 - y0).abs() < 1e-9 {
                    return Err("a rectangle needs a width and a height".into());
                }
                let corners = [v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)];
                ops.push(SketchOp::AddPolyline { points: corners.to_vec(), closed: true, construction: false, label: "Add rectangle" });
                ops.push(SketchOp::AddConstraints(rectangle_constraints(corners)));
            }
            SketchEntity::Circle { cx, cy, r } => {
                if r <= 0.0 {
                    return Err("a circle's radius must be positive".into());
                }
                ops.push(SketchOp::AddCircle { center: v(cx, cy), radius: r, construction: false });
            }
            SketchEntity::Polygon { ref points } => {
                if points.len() < 3 {
                    return Err("a polygon needs at least 3 points".into());
                }
                ops.push(SketchOp::AddPolyline {
                    points: points.iter().map(|p| v(p[0], p[1])).collect(),
                    closed: true,
                    construction: false,
                    label: "Add polygon",
                });
            }
            SketchEntity::Line { x0, y0, x1, y1 } => {
                ops.push(SketchOp::AddPolyline { points: vec![v(x0, y0), v(x1, y1)], closed: false, construction: false, label: "Add line" });
            }
            SketchEntity::Outline { start, ref segments } => ops.extend(outline_ops(start, segments)?),
            SketchEntity::Arc { cx, cy, sx, sy, ex, ey } => {
                ops.push(SketchOp::AddArc { center: v(cx, cy), start: v(sx, sy), end: v(ex, ey), construction: false });
            }
            SketchEntity::Point { x, y } => ops.push(SketchOp::AddPoint { pos: v(x, y) }),
        }
    }
    Ok(ops)
}

/// An outline's segments as sketch lines and arcs. Each line and arc is its own curve; the sketch
/// merges their ends where they meet, so the segments close into one loop.
fn outline_ops(start: [f64; 2], segments: &[tools::PathSegment]) -> Result<Vec<SketchOp>, String> {
    let v = |p: [f64; 2]| SVec2::new(p[0], p[1]);
    if segments.len() < 2 {
        return Err("an outline needs at least two segments to close".into());
    }
    let mut ops = Vec::new();
    let mut from = start;
    for (i, seg) in segments.iter().enumerate() {
        let n = i + 1;
        match seg {
            tools::PathSegment::Line { to } => {
                if v(*to).distance(v(from)) < 1e-9 {
                    return Err(format!("outline segment {n} has no length"));
                }
                ops.push(SketchOp::AddPolyline { points: vec![v(from), v(*to)], closed: false, construction: false, label: "Add line" });
                from = *to;
            }
            tools::PathSegment::Arc { to, center, ccw } => {
                let c = v(*center);
                let r = c.distance(v(from));
                if r < 1e-9 || (c.distance(v(*to)) - r).abs() > 1e-6 * r.max(1.0) {
                    return Err(format!("outline segment {n}: the arc's end is not on its circle about its center"));
                }
                if v(*to).distance(v(from)) < 1e-9 {
                    return Err(format!("outline segment {n} has no length"));
                }
                // AddArc runs counter-clockwise from its start: a clockwise arc is one from `to` back to `from`.
                let (s, e) = if *ccw { (from, *to) } else { (*to, from) };
                ops.push(SketchOp::AddArc { center: c, start: v(s), end: v(e), construction: false });
                from = *to;
            }
        }
    }
    if v(from).distance(v(start)) > 1e-6 {
        return Err("the outline must end where it starts".into());
    }
    Ok(ops)
}

/// A point well inside a region: the centroid of its largest triangle.
fn inside(r: &cadrs_sketch::region::Region) -> SVec2 {
    let (pts, tris) = r.triangulate();
    let mut best = (f64::MIN, SVec2::new(0.0, 0.0));
    for t in tris.chunks(3) {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let area = ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs() / 2.0;
        if area > best.0 {
            best = (area, SVec2::new((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0));
        }
    }
    best.1
}

fn sketch_regions(g: &Sketch) -> Vec<Value> {
    cadrs_sketch::region::regions(g)
        .iter()
        .map(|r| {
            let p = inside(r);
            json!({ "point": [round(p.x), round(p.y)], "area_mm2": round(r.area()) })
        })
        .collect()
}

fn add_sketch(world: &mut World, p: &tools::AddSketch) -> Result<Value, String> {
    let ops = sketch_ops(&p.entities)?;
    let mut d = doc_mut(world)?;
    let element = part_studio(&d, p.part_studio.as_deref())?;
    let features = d.doc.element(element).ok_or("the Part Studio is gone")?.features().to_vec();
    let plane = match (&p.plane, &p.face, &p.plane_feature) {
        (Some(pl), None, None) => plane_ref(*pl),
        (None, Some(face), None) => {
            let face: cadrs_core::FaceRef = serde_json::from_value(face.clone()).map_err(|e| format!("face: {e}"))?;
            cadrs_core::parts::face_plane(&features, FeatureId(face.face.op), face.face).ok_or("that face is not a planar face of a part")?
        }
        (None, None, Some(name)) => {
            let f = features.iter().find(|f| &f.name == name).ok_or_else(|| format!("no feature named {name:?}"))?;
            cadrs_core::parts::plane_feature_ref(&features, f.id).ok_or_else(|| format!("{name:?} is not a Plane feature"))?
        }
        _ => return Err("give exactly one of plane, face or plane_feature".into()),
    };
    let frame = plane.frame();
    let feature = FeatureId::new();
    d.execute(&AddSketch { element, feature, plane: Some(plane) }).map_err(|e| e.to_string())?;
    let before: std::collections::HashSet<cadrs_sketch::CurveId> =
        d.doc.element(element).and_then(|e| e.feature(feature)).and_then(|f| f.sketch()).map(|s| s.geometry.curves.keys().collect()).unwrap_or_default();
    if !ops.is_empty() {
        d.execute(&cadrs_core::commands::EditSketch { element, feature, op: SketchOp::Batch(ops) }).map_err(|e| e.to_string())?;
    }
    let e = d.doc.element(element).ok_or("the Part Studio is gone")?;
    let f = e.feature(feature).ok_or("the sketch is gone")?;
    let s = f.sketch().ok_or("not a sketch")?;
    let n = cross(frame.u, frame.v);
    let curves: Vec<Value> = s
        .geometry
        .curves
        .iter()
        .filter(|(id, _)| !before.contains(id))
        .map(|(id, c)| json!({ "id": id, "kind": format!("{c:?}").split(['(', ' ', '{']).next().unwrap_or("") }))
        .collect();
    Ok(json!({
        "sketch": f.name,
        "id": f.id.0.to_string(),
        "curves": curves,
        "frame": { "origin": frame.origin.map(round), "x": frame.u.map(round), "y": frame.v.map(round), "normal": n.map(round) },
        "regions": sketch_regions(&s.geometry),
    }))
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// A feature type's defaults, for add_feature.
fn template(kind: &str) -> Option<FeatureKind> {
    use cadrs_core::{advanced, applied, draft, pattern, plane, surfacing, transform};
    Some(match kind.to_ascii_lowercase().replace([' ', '_'], "").as_str() {
        "extrude" => FeatureKind::Extrude(ExtrudeFeature::default()),
        "revolve" => FeatureKind::Revolve(Default::default()),
        "fillet" => FeatureKind::Fillet(applied::FilletFeature::default()),
        "chamfer" => FeatureKind::Chamfer(applied::ChamferFeature::default()),
        "hole" => FeatureKind::Hole(applied::HoleFeature::default()),
        "shell" => FeatureKind::Shell(applied::ShellFeature::default()),
        "plane" => FeatureKind::Plane(plane::PlaneFeature::default()),
        "sweep" => FeatureKind::Sweep(advanced::SweepFeature::default()),
        "loft" => FeatureKind::Loft(advanced::LoftFeature::default()),
        "split" => FeatureKind::Split(advanced::SplitFeature::default()),
        "pattern" | "linearpattern" => FeatureKind::Pattern(pattern::PatternFeature::new(pattern::PatternKind::Linear)),
        "circularpattern" => FeatureKind::Pattern(pattern::PatternFeature::new(pattern::PatternKind::Circular)),
        "mirror" => FeatureKind::Mirror(pattern::MirrorFeature::default()),
        "draft" => FeatureKind::Draft(draft::DraftFeature::default()),
        "boolean" => FeatureKind::Boolean(Default::default()),
        "deletepart" => FeatureKind::DeletePart(Default::default()),
        "transform" => FeatureKind::Transform(transform::TransformFeature::default()),
        "thicken" => FeatureKind::Thicken(surfacing::ThickenFeature::default()),
        "helix" => FeatureKind::Helix(surfacing::HelixFeature::default()),
        "deleteface" => FeatureKind::DirectEdit(cadrs_core::direct_edit::DirectEditFeature::default()),
        "simplify" => FeatureKind::DirectEdit(cadrs_core::direct_edit::DirectEditFeature {
            kind: cadrs_core::direct_edit::DirectEditKind::Simplify,
            ..Default::default()
        }),
        "moveface" => FeatureKind::DirectEdit(cadrs_core::direct_edit::DirectEditFeature {
            kind: cadrs_core::direct_edit::DirectEditKind::MoveFace,
            ..Default::default()
        }),
        _ => return None,
    })
}

/// `patch` merged into `base`: objects key by key, anything else replaced.
fn merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                merge(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// `kind` with the JSON fields of `patch` merged over its own.
fn patched(kind: &FeatureKind, patch: &Value) -> Result<FeatureKind, String> {
    let mut v = serde_json::to_value(kind).map_err(|e| e.to_string())?;
    if !patch.is_null() {
        let inner = v.as_object_mut().and_then(|o| o.values_mut().next()).ok_or("this feature has no fields to set")?;
        merge(inner, patch);
    }
    serde_json::from_value(v).map_err(|e| format!("bad params: {e}"))
}

/// What a change left: the feature's name, its error or warning, and the parts.
fn outcome(world: &World, element: ElementId, feature: FeatureId) -> Result<Value, String> {
    let d = doc(world)?;
    let e = d.doc.element(element).ok_or("the Part Studio is gone")?;
    let name = e.feature(feature).map(|f| f.name.clone()).unwrap_or_default();
    let state = studio_state(e.name.as_str(), e.features());
    let mut out = json!({ "feature": name, "parts": state["parts"] });
    if let Some(f) = state["features"].as_array().and_then(|fs| fs.iter().find(|f| f["name"] == name.as_str())) {
        for k in ["error", "warning"] {
            if let Some(m) = f.get(k) {
                out[k] = m.clone();
            }
        }
    }
    Ok(out)
}

fn feature_named(d: &ActiveDocument, element: ElementId, name: &str) -> Result<cadrs_core::Feature, String> {
    let e = d.doc.element(element).ok_or("the Part Studio is gone")?;
    e.features().iter().find(|f| f.name == name).cloned().ok_or_else(|| format!("no feature named {name:?}"))
}

/// Replaces a patch's `region_points` ({"sketch": name, "points": [[x, y], …]}, the points
/// omitted for every region) with the `regions` they pick.
fn expand_regions(d: &ActiveDocument, element: ElementId, patch: &Value) -> Result<Value, String> {
    let mut patch = patch.clone();
    let Some(rp) = patch.as_object_mut().and_then(|o| o.remove("region_points")) else { return Ok(patch) };
    let name = rp["sketch"].as_str().ok_or("region_points needs a sketch name")?;
    let e = d.doc.element(element).ok_or("the Part Studio is gone")?;
    let sketch = e.features().iter().find(|f| f.name == name).ok_or_else(|| format!("no feature named {name:?}"))?;
    let FeatureKind::Sketch(s) = &sketch.kind else { return Err(format!("{name:?} is not a sketch")) };
    let regions = cadrs_sketch::region::regions(&s.geometry);
    let picked: Vec<usize> = match rp.get("points").and_then(|p| p.as_array()) {
        None => (0..regions.len()).collect(),
        Some(points) => points
            .iter()
            .map(|q| {
                let (x, y) = (q[0].as_f64().unwrap_or(f64::NAN), q[1].as_f64().unwrap_or(f64::NAN));
                cadrs_sketch::region::region_at(&regions, SVec2::new(x, y)).ok_or_else(|| format!("no region of {name:?} contains [{x}, {y}]"))
            })
            .collect::<Result<_, _>>()?,
    };
    let refs: Vec<cadrs_core::RegionRef> = picked.iter().map(|&i| cadrs_core::RegionRef::new(sketch.id, &regions[i])).collect();
    if let Some(o) = patch.as_object_mut() {
        o.insert("regions".into(), serde_json::to_value(refs).map_err(|e| e.to_string())?);
    }
    Ok(patch)
}

fn add_feature(world: &mut World, p: &tools::AddFeature) -> Result<Value, String> {
    let base = template(&p.kind).ok_or_else(|| format!("unknown feature type {:?}", p.kind))?;
    let element = part_studio(doc(world)?, p.part_studio.as_deref())?;
    let params = expand_regions(doc(world)?, element, &p.params)?;
    let kind = patched(&base, &params)?;
    if matches!(kind, FeatureKind::Sketch(_)) {
        return Err("use add_sketch for sketches".into());
    }
    let mut d = doc_mut(world)?;
    let feature = FeatureId::new();
    let base_name = cadrs_core::feature_list::type_label(&kind).to_string();
    d.execute(&cadrs_core::commands::AddFeature { element, feature, base_name, kind }).map_err(|e| e.to_string())?;
    outcome(world, element, feature)
}

fn edit_feature(world: &mut World, p: &tools::EditFeature) -> Result<Value, String> {
    let mut d = doc_mut(world)?;
    let element = part_studio(&d, p.part_studio.as_deref())?;
    let f = feature_named(&d, element, &p.name)?;
    let params = expand_regions(&d, element, &p.params)?;
    let kind = patched(&f.kind, &params)?;
    d.execute(&cadrs_core::commands::SetFeature { element, feature: f.id, kind, label: format!("Edit {}", f.name) }).map_err(|e| e.to_string())?;
    outcome(world, element, f.id)
}

/// A sketch edit as the engine's op. An array is a batch (one undoable step), and an
/// {"type": "batch", "ops": [...]}, or {"type": "add_constraints", "specs": [...]}, is read by
/// hand: the engine's `Batch` and `AddConstraints` are tuple variants, which tagged JSON can't carry.
fn sketch_op_from_json(v: &Value) -> Result<SketchOp, String> {
    let ops = |items: &Value| -> Result<Vec<SketchOp>, String> {
        items.as_array().ok_or("expected an array of edits")?.iter().map(sketch_op_from_json).collect()
    };
    if v.is_array() {
        return Ok(SketchOp::Batch(ops(v)?));
    }
    match v.get("type").and_then(Value::as_str) {
        Some("batch") => Ok(SketchOp::Batch(ops(v.get("ops").unwrap_or(&Value::Null))?)),
        Some("add_constraints") => {
            let specs = serde_json::from_value(v.get("specs").cloned().unwrap_or(Value::Null)).map_err(|e| format!("specs: {e}"))?;
            Ok(SketchOp::AddConstraints(specs))
        }
        _ => serde_json::from_value(v.clone()).map_err(|e| format!("bad sketch edit: {e}")),
    }
}

/// A sketch key as the id get_feature shows it ({"idx", "version"}).
fn entity_id(k: impl serde::Serialize) -> tools::EntityId {
    let v = serde_json::to_value(k).expect("a sketch key serializes");
    serde_json::from_value(v).expect("a sketch key is {idx, version}")
}

/// A sketch's ids of each kind, to tell what an edit created and removed.
fn sketch_ids(s: &Sketch) -> tools::SketchIds {
    tools::SketchIds {
        curves: s.curves.keys().map(entity_id).collect(),
        points: s.points.keys().map(entity_id).collect(),
        dimensions: s.dimensions.keys().map(entity_id).collect(),
        constraints: s.constraints.keys().map(entity_id).collect(),
    }
}

/// The ids in `now` that `was` lacks.
fn new_ids(now: &tools::SketchIds, was: &tools::SketchIds) -> tools::SketchIds {
    let only = |a: &[tools::EntityId], b: &[tools::EntityId]| a.iter().copied().filter(|id| !b.contains(id)).collect::<Vec<_>>();
    tools::SketchIds {
        curves: only(&now.curves, &was.curves),
        points: only(&now.points, &was.points),
        dimensions: only(&now.dimensions, &was.dimensions),
        constraints: only(&now.constraints, &was.constraints),
    }
}

/// The sketch's closed regions, each with a point inside it.
fn typed_regions(g: &Sketch) -> Vec<tools::SketchRegion> {
    cadrs_sketch::region::regions(g)
        .iter()
        .map(|r| {
            let p = inside(r);
            tools::SketchRegion { point: [round(p.x), round(p.y)], area_mm2: round(r.area()) }
        })
        .collect()
}

/// Any sketch edit: its ids created and removed, the regions, and the solve's state.
fn edit_sketch(world: &mut World, p: &tools::EditSketch) -> Result<tools::SketchEdited, String> {
    let op = sketch_op_from_json(&p.op)?;
    let mut d = doc_mut(world)?;
    let element = part_studio(&d, p.part_studio.as_deref())?;
    let f = feature_named(&d, element, &p.sketch)?;
    let before = sketch_ids(&f.sketch().ok_or_else(|| format!("{:?} is not a sketch", p.sketch))?.geometry);
    d.execute(&cadrs_core::commands::EditSketch { element, feature: f.id, op }).map_err(|e| e.to_string())?;
    let e = d.doc.element(element).ok_or("the Part Studio is gone")?;
    let g = &e.feature(f.id).and_then(|f| f.sketch()).ok_or("the sketch is gone")?.geometry;
    let after = sketch_ids(g);
    let analysis = cadrs_sketch::solve::analyze(g);
    let conflicts = cadrs_sketch::solve::conflicts(g)
        .into_iter()
        .map(|c| match c {
            cadrs_sketch::solve::Source::Constraint(id) => tools::SketchConflict::Constraint { id: entity_id(id) },
            cadrs_sketch::solve::Source::Dimension(id) => tools::SketchConflict::Dimension { id: entity_id(id) },
            cadrs_sketch::solve::Source::Arc(curve) => tools::SketchConflict::Arc { curve: entity_id(curve) },
            _ => tools::SketchConflict::Other { description: format!("{c:?}") },
        })
        .collect();
    Ok(tools::SketchEdited {
        sketch: p.sketch.clone(),
        created: new_ids(&after, &before),
        removed: new_ids(&before, &after),
        regions: typed_regions(g),
        fully_constrained: analysis.fully_constrained(),
        has_conflicts: analysis.has_conflicts(),
        conflicts,
    })
}

fn get_feature(world: &mut World, p: &tools::FeatureName) -> Result<Value, String> {
    let d = doc(world)?;
    let element = part_studio(d, p.part_studio.as_deref())?;
    let f = feature_named(d, element, &p.name)?;
    let kind = serde_json::to_value(&f.kind).map_err(|e| e.to_string())?;
    Ok(json!({ "name": f.name, "id": f.id.0.to_string(), "feature": kind }))
}

fn delete_feature(world: &mut World, p: &tools::FeatureName) -> Result<Value, String> {
    let mut d = doc_mut(world)?;
    let element = part_studio(&d, p.part_studio.as_deref())?;
    let f = feature_named(&d, element, &p.name)?;
    d.execute(&cadrs_core::commands::DeleteFeature { element, feature: f.id, label: format!("Delete {}", f.name) }).map_err(|e| e.to_string())?;
    describe(world)
}

/// The parts of a Part Studio as the rebuild makes them.
fn parts_of(world: &World, studio: Option<&str>) -> Result<Vec<cadrs_core::parts::Part>, String> {
    let d = doc(world)?;
    let element = part_studio(d, studio)?;
    let e = d.doc.element(element).ok_or("the Part Studio is gone")?;
    Ok(cadrs_core::rebuild::build(e.features()).parts.clone())
}

fn list_faces(world: &mut World, studio: Option<&str>) -> Result<Value, String> {
    let mut out = Vec::new();
    for p in parts_of(world, studio)? {
        for (i, f) in p.solid.faces.iter().enumerate() {
            let Some(seed) = p.solid.face_point(i) else { continue };
            let r = cadrs_core::FaceRef { part: p.id, face: f.name, seed };
            let mut v = json!({ "ref": r, "part": p.name });
            if let Some(k) = &f.kind {
                v["surface"] = json!(format!("{k:?}"));
            }
            if let Some(a) = f.area {
                v["area"] = json!(round(a));
            }
            if let Some(c) = f.center {
                v["center"] = json!(c.map(round));
            }
            if let Some(pl) = &f.plane {
                v["normal"] = json!(cross(pl.u, pl.v).map(round));
            }
            if let Some((o, d)) = f.axis {
                v["axis"] = json!({ "point": o.map(round), "direction": d.map(round) });
            }
            if let Some(r) = f.radius {
                v["radius"] = json!(round(r));
            }
            out.push(v);
        }
    }
    Ok(json!({ "faces": out }))
}

fn list_edges(world: &mut World, studio: Option<&str>) -> Result<Value, String> {
    let mut out = Vec::new();
    for p in parts_of(world, studio)? {
        for e in &p.solid.edges {
            let (Some(a), Some(b)) = (e.points.first(), e.points.last()) else { continue };
            let r = cadrs_core::EdgeRef { part: p.id, edge: e.name, seed: e.midpoint() };
            let len: f64 = e.points.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2) + (w[1][2] - w[0][2]).powi(2)).sqrt()).sum();
            let mut v = json!({ "ref": r, "part": p.name, "start": a.map(round), "end": b.map(round), "length": round(len) });
            if let Some(c) = &e.circle {
                v["circle"] = json!({ "center": c.center.map(round), "normal": c.normal.map(round), "radius": round(c.radius) });
            } else if e.points.len() == 2 {
                v["line"] = json!(true);
            }
            out.push(v);
        }
    }
    Ok(json!({ "edges": out }))
}

/// Imports a STEP file as a new stored document (its parts flattened into a Part Studio) and
/// opens it, as the documents page's Import files does.
fn import_step(world: &mut World, path: &str) -> Result<Value, String> {
    use cadrs_core::import::{ImportAs, ImportFormat, ImportIds, imported_document};
    let path = std::path::Path::new(path);
    let format = ImportFormat::of_path(path).filter(|f| f.kernel().is_some()).ok_or("only STEP and IGES files can be imported")?;
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let data = String::from_utf8_lossy(&bytes).into_owned();
    let plan = cadrs_core::rebuild::exchange::plan_import(format, data.clone().into_bytes()).wait().ok_or("the kernel thread stopped")??;
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let doc = imported_document(&plan, &file_name, data, ImportAs::PartStudio, ImportIds::fresh());
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let meta = DocumentMeta::new(&user, now);
    let id = doc.id;
    store.create(&doc, &meta).map_err(|e| format!("cannot create the document: {e}"))?;
    crate::move_document::open_document(world, id);
    let mut d = doc_mut(world)?;
    d.fresh = true;
    if let Some(ps) = d.doc.elements.iter().find(|e| matches!(e.kind, ElementKind::PartStudio { .. })).map(|e| e.id) {
        d.set_active(ps);
    }
    describe(world)
}

fn export_step(world: &mut World, p: &tools::ExportStep) -> Result<Value, String> {
    let d = doc(world)?;
    let element = part_studio(d, p.part_studio.as_deref())?;
    let features = d.doc.element(element).ok_or("the Part Studio is gone")?.features().to_vec();
    let parts = cadrs_core::rebuild::build(&features).parts.clone();
    if parts.is_empty() {
        return Err("the Part Studio has no parts".into());
    }
    let request = cadrs_core::export::StepRequest { parts: parts.iter().map(|p| (p.id, p.name.clone())).collect(), y_up: false, individual: false };
    let files = cadrs_core::rebuild::export_step(features, request).wait()?;
    let file = files.into_iter().next().ok_or("nothing was written")?;
    std::fs::write(&p.path, &file.bytes).map_err(|e| format!("{}: {e}", p.path))?;
    Ok(json!({ "path": p.path, "bytes": file.bytes.len(), "parts": parts.iter().map(|p| p.name.clone()).collect::<Vec<_>>() }))
}

fn extrude(world: &mut World, p: &tools::Extrude) -> Result<Value, String> {
    if p.depth.is_nan() || p.depth <= 0.0 {
        return Err("the depth must be positive (use flip to extrude the other way)".into());
    }
    let mut d = doc_mut(world)?;
    let element = part_studio(&d, p.part_studio.as_deref())?;
    let e = d.doc.element(element).ok_or("the Part Studio is gone")?;
    let sketch = e.features().iter().find(|f| f.name == p.sketch).ok_or_else(|| format!("no feature named {:?}", p.sketch))?;
    let FeatureKind::Sketch(s) = &sketch.kind else {
        return Err(format!("{:?} is not a sketch", p.sketch));
    };
    let regions = cadrs_sketch::region::regions(&s.geometry);
    if regions.is_empty() {
        return Err(format!("{:?} has no closed regions", p.sketch));
    }
    let picked: Vec<usize> = match &p.regions {
        None => (0..regions.len()).collect(),
        Some(points) => points
            .iter()
            .map(|q| cadrs_sketch::region::region_at(&regions, SVec2::new(q[0], q[1])).ok_or_else(|| format!("no region of {:?} contains [{}, {}]", p.sketch, q[0], q[1])))
            .collect::<Result<_, _>>()?,
    };
    let refs = picked.iter().map(|&i| cadrs_core::RegionRef::new(sketch.id, &regions[i])).collect();
    let extrude = ExtrudeFeature {
        regions: refs,
        depth: p.depth,
        depth_expr: format!("{} mm", p.depth),
        op: match p.operation {
            Operation::New => BooleanOp::New,
            Operation::Add => BooleanOp::Add,
            Operation::Remove => BooleanOp::Remove,
            Operation::Intersect => BooleanOp::Intersect,
        },
        symmetric: p.symmetric,
        flip: p.flip,
        ..ExtrudeFeature::default()
    };
    let feature = FeatureId::new();
    d.execute(&AddExtrude { element, feature, extrude }).map_err(|e| e.to_string())?;
    let e = d.doc.element(element).ok_or("the Part Studio is gone")?;
    let name = e.feature(feature).map(|f| f.name.clone()).unwrap_or_default();
    let state = studio_state(e.name.as_str(), e.features());
    let error = state["features"].as_array().and_then(|fs| fs.iter().find(|f| f["name"] == name.as_str())).and_then(|f| f.get("error")).cloned();
    let mut out = json!({ "feature": name, "parts": state["parts"] });
    if let Some(e) = error {
        out["error"] = e;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_core::Feature;
    use cadrs_core::plane::{PlaneEntity, PlaneFeature};

    fn line(x: f64, y: f64) -> tools::PathSegment {
        tools::PathSegment::Line { to: [x, y] }
    }

    fn arc(to: [f64; 2], center: [f64; 2], ccw: bool) -> tools::PathSegment {
        tools::PathSegment::Arc { to, center, ccw }
    }

    fn hole(cx: f64, cy: f64, r: f64) -> SketchEntity {
        SketchEntity::Circle { cx, cy, r }
    }

    #[test]
    fn a_rounded_outline_with_holes_is_one_region() {
        // The bracket's base: 72 × 35, R12 at the front corners, two Ø12 holes 48 apart.
        let base = SketchEntity::Outline {
            start: [-24.0, 0.0],
            segments: vec![
                line(24.0, 0.0),
                arc([36.0, 12.0], [24.0, 12.0], true),
                line(36.0, 35.0),
                line(-36.0, 35.0),
                line(-36.0, 12.0),
                arc([-24.0, 0.0], [-24.0, 12.0], true),
            ],
        };
        let entities = [base, hole(24.0, 12.0, 6.0), hole(-24.0, 12.0, 6.0)];
        let ops = sketch_ops(&entities).unwrap();
        let mut s = Sketch::new();
        for op in &ops {
            op.apply(&mut s).unwrap();
        }
        let regions = cadrs_sketch::region::regions(&s);
        // Each hole circle is also a region of its own; the outline's region has both as holes.
        assert_eq!(regions.len(), 3);
        let base = regions.iter().find(|r| r.holes.len() == 2).expect("the outline with its holes");
        // 72 × 35, less two R12 corners (2 × (144 − 36π)), less two Ø12 holes (2 × 36π).
        assert!((base.area() - 2232.0).abs() < 0.01, "area {}", base.area());
        // Extrude takes the region at a point: the outline at (0, 17.5) keeps the holes, the hole at (24, 12) is the pin.
        let at = |x, y| cadrs_sketch::region::region_at(&regions, SVec2::new(x, y)).map(|i| regions[i].holes.len());
        assert_eq!(at(0.0, 17.5), Some(2));
        assert_eq!(at(24.0, 12.0), Some(0));
    }

    #[test]
    fn an_arc_must_end_on_its_circle_and_the_outline_must_close() {
        let off_circle = [line(10.0, 0.0), arc([0.0, 5.0], [0.0, 0.0], true)];
        assert!(outline_ops([10.0, 0.0], &off_circle).is_err());
        let open = [line(10.0, 0.0), line(10.0, 10.0)];
        assert!(outline_ops([0.0, 0.0], &open).is_err());
    }

    /// A sketch feature on `plane` with `entities`, and its id.
    fn sketch(features: &mut Vec<Feature>, name: &str, plane: PlaneRef, entities: &[SketchEntity]) -> FeatureId {
        let mut geometry = Sketch::new();
        for op in sketch_ops(entities).unwrap() {
            op.apply(&mut geometry).unwrap();
        }
        let id = FeatureId::new();
        let kind = FeatureKind::Sketch(cadrs_core::document::SketchFeature { plane: Some(plane), disable_imprinting: false, geometry });
        features.push(Feature::new(id, name, kind));
        id
    }

    /// An extrude of the sketch region at `point` (the loop's point, not a hole's).
    fn extrude(features: &mut Vec<Feature>, sketch_id: FeatureId, point: [f64; 2], depth: f64, op: BooleanOp, flip: bool, symmetric: bool) {
        let geometry = match &features.iter().find(|f| f.id == sketch_id).unwrap().kind {
            FeatureKind::Sketch(s) => s.geometry.clone(),
            _ => unreachable!(),
        };
        let regions = cadrs_sketch::region::regions(&geometry);
        let i = cadrs_sketch::region::region_at(&regions, SVec2::new(point[0], point[1])).expect("no region at the point");
        let feature = ExtrudeFeature {
            regions: vec![cadrs_core::RegionRef::new(sketch_id, &regions[i])],
            depth,
            depth_expr: format!("{depth} mm"),
            op,
            symmetric,
            flip,
            ..ExtrudeFeature::default()
        };
        features.push(Feature::new(FeatureId::new(), "Extrude", FeatureKind::Extrude(feature)));
    }

    /// A Plane offset from the Front plane (as the add_feature call would make it).
    fn plane_from_front(features: &mut Vec<Feature>, offset: f64) -> PlaneRef {
        let plane = PlaneFeature {
            entities: vec![PlaneEntity::Plane(PlaneRef::Front)],
            offset,
            offset_expr: format!("{offset} mm"),
            flip: true,
            ..PlaneFeature::default()
        };
        let id = FeatureId::new();
        features.push(Feature::new(id, "Plane", FeatureKind::Plane(plane)));
        cadrs_core::parts::plane_feature_ref(features, id).expect("the plane")
    }

    /// The bracket: a base in two layers, an upright (its lower part 8 deep, the boss at its top
    /// 13 deep), and a 60° gusset. Sketch coordinates are as the sketch's plane gives them.
    #[test]
    fn the_bracket_rebuilds_to_its_drawing() {
        let rounded_base = |back: f64| SketchEntity::Outline {
            start: [-24.0, 0.0],
            segments: vec![
                line(24.0, 0.0),
                arc([36.0, 12.0], [24.0, 12.0], true),
                line(36.0, back),
                line(-36.0, back),
                line(-36.0, 12.0),
                arc([-24.0, 0.0], [-24.0, 12.0], true),
            ],
        };
        let mut f = Vec::new();

        // Base: the full 72 × 35 footprint 6 thick, then the 72 × 30 part 10 high.
        let base_low = sketch(&mut f, "Sketch 1", PlaneRef::Top, &[rounded_base(35.0), hole(24.0, 12.0, 6.0), hole(-24.0, 12.0, 6.0)]);
        extrude(&mut f, base_low, [0.0, 17.5], 6.0, BooleanOp::New, false, false);
        let base_high = sketch(&mut f, "Sketch 2", PlaneRef::Top, &[rounded_base(30.0), hole(24.0, 12.0, 6.0), hole(-24.0, 12.0, 6.0)]);
        extrude(&mut f, base_high, [0.0, 15.0], 10.0, BooleanOp::Add, false, false);

        // Upright, 22 mm from the front (its face), 8 deep toward the back.
        let face = plane_from_front(&mut f, 22.0);
        let upright = [
            SketchEntity::Outline {
                start: [-30.0, 10.0],
                segments: vec![
                    arc([-15.0, 25.0], [-30.0, 25.0], true),
                    line(-15.0, 60.0),
                    arc([15.0, 60.0], [0.0, 60.0], false),
                    line(15.0, 25.0),
                    arc([30.0, 10.0], [30.0, 25.0], true),
                    line(-30.0, 10.0),
                ],
            },
            hole(0.0, 60.0, 6.0),
        ];
        let upright_id = sketch(&mut f, "Sketch 3", face, &upright);
        extrude(&mut f, upright_id, [0.0, 30.0], 8.0, BooleanOp::Add, true, false);

        // Boss: the top 13 deep, 5 more toward the back.
        let back = plane_from_front(&mut f, 30.0);
        let boss = [
            SketchEntity::Outline {
                start: [-15.0, 45.0],
                segments: vec![line(15.0, 45.0), line(15.0, 60.0), arc([-15.0, 60.0], [0.0, 60.0], true), line(-15.0, 45.0)],
            },
            hole(0.0, 60.0, 6.0),
        ];
        let boss_id = sketch(&mut f, "Sketch 4", back, &boss);
        extrude(&mut f, boss_id, [0.0, 50.0], 5.0, BooleanOp::Add, true, false);

        // Gusset: 10 wide, from the front edge up the 60° slope to the upright.
        let gusset = [SketchEntity::Polygon { points: vec![[0.0, 10.0], [22.0, 10.0], [22.0, 48.0]] }];
        let gusset_id = sketch(&mut f, "Sketch 5", PlaneRef::Right, &gusset);
        extrude(&mut f, gusset_id, [15.0, 25.0], 10.0, BooleanOp::Add, false, true);

        let build = cadrs_core::rebuild::build(&f);
        assert!(build.errors.is_empty(), "rebuild errors: {:?}", build.errors);
        assert_eq!(build.parts.len(), 1);
        let part = &build.parts[0];
        let (lo, hi) = part.solid.bounds().expect("bounds");
        for (got, want) in lo.iter().chain(hi.iter()).zip([-36.0, 0.0, 0.0, 36.0, 35.0, 75.0]) {
            assert!((got - want).abs() < 1e-3, "bounds {lo:?} {hi:?}");
        }
        // Base 2232 × 6 + 1872 × 4, upright 1836.9 × 8, boss 690.3 × 5, gusset 418 × 10.
        let volume = part.mass.as_ref().expect("mass").volume;
        assert!((volume - 43206.8).abs() < 5.0, "volume {volume}");
    }

    #[test]
    fn a_plane_offset_from_front_takes_json_params() {
        // The upright's front face: 22 mm from the Front plane, on the side it faces away from.
        let params = json!({ "entities": [{ "Plane": "Front" }], "offset": 22.0, "offset_expr": "22 mm", "flip": true });
        let FeatureKind::Plane(p) = patched(&template("plane").unwrap(), &params).unwrap() else { panic!("not a plane") };
        assert_eq!(p.kind, cadrs_core::plane::PlaneType::Offset);
        assert_eq!(p.entities, vec![cadrs_core::plane::PlaneEntity::Plane(PlaneRef::Front)]);
        assert_eq!(p.offset, 22.0);
        assert!(p.flip);
    }

    #[test]
    fn sketch_ids_are_the_ids_the_sketch_serializes_with() {
        let mut s = Sketch::new();
        SketchOp::AddCircle { center: SVec2::new(0.0, 0.0), radius: 5.0, construction: false }.apply(&mut s).unwrap();
        let ids = sketch_ids(&s);
        assert_eq!(ids.curves.len(), 1);
        assert_eq!(ids.points.len(), 1);
        assert_ne!(ids.curves[0].idx, 0);
        let json = serde_json::to_string(&s).unwrap();
        let c = ids.curves[0];
        assert!(json.contains(&format!("{{\"idx\":{},\"version\":{}}}", c.idx, c.version)), "curve id {c:?} not in {json}");
        // Created: what is in `after` and not `before`.
        let created = new_ids(&ids, &tools::SketchIds::default());
        assert_eq!(created, ids);
        assert_eq!(new_ids(&tools::SketchIds::default(), &ids), tools::SketchIds::default());
    }

    #[test]
    fn sketch_edits_come_as_an_object_an_array_or_a_batch() {
        let circle = json!({ "type": "add_circle", "center": { "x": 0.0, "y": 0.0 }, "radius": 5.0, "construction": false });
        assert!(matches!(sketch_op_from_json(&circle), Ok(SketchOp::AddCircle { .. })));
        let array = json!([circle.clone(), circle.clone()]);
        assert!(matches!(sketch_op_from_json(&array), Ok(SketchOp::Batch(ops)) if ops.len() == 2));
        let batch = json!({ "type": "batch", "ops": [circle.clone()] });
        assert!(matches!(sketch_op_from_json(&batch), Ok(SketchOp::Batch(ops)) if ops.len() == 1));
        let specs = json!({ "type": "add_constraints", "specs": [] });
        assert!(matches!(sketch_op_from_json(&specs), Ok(SketchOp::AddConstraints(v)) if v.is_empty()));
        assert!(sketch_op_from_json(&json!({ "type": "not_an_edit" })).is_err());
    }

    /// The forms the edit_sketch reference gives, each as the engine reads it.
    #[test]
    fn the_edit_sketch_reference_examples_parse() {
        let p = json!({ "x": 1.0, "y": 2.0 });
        let id = json!({ "idx": 1, "version": 1 });
        let examples = [
            json!({ "type": "add_polyline", "points": [p, p], "closed": false, "construction": false }),
            json!({ "type": "add_center_rectangle", "center": p, "corners": [p, p, p, p], "construction": false }),
            json!({ "type": "add_arc", "center": p, "start": p, "end": p, "construction": false }),
            json!({ "type": "add_point", "pos": p }),
            json!({ "type": "add_ellipse", "center": p, "major": p, "minor": 2.0, "construction": false }),
            json!({ "type": "add_bezier", "points": [p, p, p, p], "construction": false }),
            json!({ "type": "add_spline", "points": [p, p, p], "periodic": false, "start_tangent": null, "end_tangent": null, "construction": false }),
            json!({ "type": "add_polygon", "center": p, "radius": 10.0, "angle": 0.0, "sides": 6, "inscribed": false, "construction": false }),
            json!({ "type": "slot", "source": id, "width": 4.0, "equal_to": null, "construction": false }),
            json!({ "type": "fillet", "corner": id, "radius": 3.0, "equal_to": null }),
            json!({ "type": "chamfer", "corner": id, "d1": 1.0, "d2": 1.0, "equal_to": null }),
            json!({ "type": "trim", "picks": [[id, p]], "points": [] }),
            json!({ "type": "extend", "curve": id, "end": id, "to": p, "by": null }),
            json!({ "type": "split", "curve": id, "at": [p] }),
            json!({ "type": "delete", "curves": [id], "points": [], "dimensions": [], "constraints": [] }),
            json!({ "type": "set_construction", "curves": [id], "construction": true }),
            json!({ "type": "move_points", "moves": [[id, p]] }),
            json!({ "type": "set_geometry", "points": [[id, p]], "radii": [[id, 5.0]] }),
            json!({ "type": "scale", "center": p, "factor": 2.0 }),
            json!({ "type": "mirror", "axis": id, "curves": [id] }),
            json!({ "type": "offset", "chain": [[id, false]], "distance": 2.0, "left": true, "label": [0.0, 0.0] }),
            json!({ "type": "add_constraint", "constraints": [{ "Coincident": [{ "Point": id }, { "Point": id }] }], "label": "Add coincident" }),
            json!({ "type": "add_constraint", "constraints": [{ "Horizontal": { "Line": { "Curve": id } } }] }),
            json!({ "type": "add_constraint", "constraints": [{ "Equal": [{ "Curve": id }, { "Curve": id }] }] }),
            json!({ "type": "add_constraint", "constraints": [{ "FixPoint": { "Point": id } }] }),
            json!({ "type": "add_constraints", "specs": [{ "Coincident": [{ "At": p }, { "Origin": null }] }] }),
            json!({ "type": "set_dimension_value", "id": id, "value": 12.0 }),
            json!({ "type": "set_dimension_driven", "id": id, "driven": true }),
            json!({ "type": "set_dimension_expr", "id": id, "expr": null }),
            json!({ "type": "move_dimension_label", "id": id, "offset": 1.0, "along": 0.0 }),
            json!({
                "type": "set_dimension",
                "dimension": { "kind": { "Horizontal": { "a": id, "b": id } }, "value": 30.0, "offset": 5.0, "along": 0.0, "driven": false },
                "moves": [], "radii": []
            }),
            json!({ "type": "set_dimension", "dimension": { "kind": { "Diameter": { "curve": id } }, "value": 12.0, "offset": 0.0 }, "moves": [], "radii": [] }),
            json!({ "type": "add_text", "origin": p, "dir": p, "height": 5.0, "style": { "text": "Hi", "font": "Inter", "bold": false, "italic": false, "mirror_h": false, "mirror_v": false } }),
        ];
        for ex in &examples {
            if let Err(e) = sketch_op_from_json(ex) {
                panic!("{ex} does not parse: {e}");
            }
        }
    }
}
