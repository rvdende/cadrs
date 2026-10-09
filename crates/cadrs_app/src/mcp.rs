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
        if let Call::Screenshot(p) = &req.call {
            let max = p.max_size.unwrap_or(1600).clamp(64, 8192);
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
        Call::Screenshot(_) => Err("a screenshot is taken by take_screenshots".into()),
    }
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
            let mut v = json!({ "name": p.name });
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
            SketchEntity::Arc { cx, cy, sx, sy, ex, ey } => {
                ops.push(SketchOp::AddArc { center: v(cx, cy), start: v(sx, sy), end: v(ex, ey), construction: false });
            }
            SketchEntity::Point { x, y } => ops.push(SketchOp::AddPoint { pos: v(x, y) }),
        }
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
    let feature = FeatureId::new();
    d.execute(&AddSketch { element, feature, plane: Some(plane_ref(p.plane)) }).map_err(|e| e.to_string())?;
    if !ops.is_empty() {
        d.execute(&cadrs_core::commands::EditSketch { element, feature, op: SketchOp::Batch(ops) }).map_err(|e| e.to_string())?;
    }
    let e = d.doc.element(element).ok_or("the Part Studio is gone")?;
    let f = e.feature(feature).ok_or("the sketch is gone")?;
    let s = f.sketch().ok_or("not a sketch")?;
    Ok(json!({ "sketch": f.name, "plane": format!("{:?}", p.plane), "regions": sketch_regions(&s.geometry) }))
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
