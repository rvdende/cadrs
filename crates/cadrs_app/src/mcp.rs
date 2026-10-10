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
    }
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
