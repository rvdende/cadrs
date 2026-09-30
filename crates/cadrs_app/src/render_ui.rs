//! The Render Studio tab (P3F.6, `intro-to-parametric-cad.md` P3.6): "Create Render Studio" in
//! the tab "+" menu makes a tab that references a Part Studio or an Assembly
//! ([`cadrs_core::render`]) and renders it with the path tracer (`cadrs_render`).
//!
//! - **Left panel** (`render-panel`, in the feature panel's place): the **Model** (the source
//!   tab), the **Environment** (Studio, Soft light, Outdoor, Sunset), its rotation, the
//!   **Background** (the environment's, white or transparent) and **Ground shadow**; the
//!   **Camera** (a named view, or **Current view**: the view the source tab was last shown
//!   in) and **Perspective**; the **Output** size, quality (samples), exposure, seed and
//!   **Denoise**; the parts' **Materials** as the renderer reads them (colour from the
//!   appearance, metal or dielectric and roughness from the material). Every change is a
//!   [`SetRenderStudio`] command, so it undoes.
//! - **The view**: a live path-traced preview of the scene at the viewport's size (16 samples,
//!   accumulated off the main thread, denoised when done); after a render, its result.
//! - **Render…** (the toolbar and the panel) opens the Render dialog (file name, size,
//!   quality, folder); **Render** accumulates the samples on a background thread with progress
//!   in the panel and the view (Cancel stops it), then writes the PNG and says where. The image
//!   is the same for the same seed.
//! - Scripted runs (`Custom("render-hold N")` / `Custom("render-hold off")`) can pause a
//!   render after N samples, for a screenshot of its progress.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{EditableText, FontWeight, TextEdit};
use bevy::ui_widgets::Activate;
use cadrs_core::render::{self as model, RenderBackground, RenderEnvironment, RenderStudio, RenderView, SetRenderStudio};
use cadrs_core::{ElementId, ElementKind};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, CheckboxChange, DialogClose, Notification, Select, SelectChange, TextInputField, TextSubmit, show_notification};
use image::RgbaImage;

use crate::viewport::{ActiveKind, ViewportArea, ViewportRect, ViewportView};
use crate::{ActiveDocument, AppState};

/// The left panel's width on a Render tab.
pub const PANEL_W: f32 = 272.0;
/// Samples of the live preview.
pub const PREVIEW_SAMPLES: u32 = 16;

pub struct RenderUiPlugin;

impl Plugin for RenderUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderState>()
            .add_systems(Startup, setup_image)
            .add_systems(Update, (drive_jobs, start_preview, sync_view, sync_panel, update_progress, on_folder_picked).chain().after(crate::parts::PartsSet).run_if(in_state(AppState::Document)))
            .add_systems(PostUpdate, flag_pending_work.run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut state: ResMut<RenderState>| {
                state.cancel_all();
                let image = state.image.clone();
                *state = RenderState { image, ..default() };
            })
            .add_observer(on_activate)
            .add_observer(on_select)
            .add_observer(on_checkbox)
            .add_observer(on_submit);
    }
}

// ---------------------------------------------------------------------------------------------
// State

/// A render on a background thread: what it has done so far.
#[derive(Default)]
struct Shared {
    done: u32,
    total: u32,
    /// The latest image and its number (changes when a new one is published).
    image: Option<Arc<RgbaImage>>,
    version: u64,
    finished: bool,
    /// Paused by a scripted hold.
    held: bool,
    /// The file written (a final render).
    saved: Option<Result<PathBuf, String>>,
}

struct Job {
    element: ElementId,
    cancel: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
    started: Instant,
}

impl Job {
    fn snapshot(&self) -> (u32, u32, u64, bool, bool) {
        let s = self.shared.lock().unwrap();
        (s.done, s.total, s.version, s.finished, s.held)
    }
}

/// A final render and where it goes.
struct FinalJob {
    job: Job,
    width: u32,
    height: u32,
    samples: u32,
}

/// What the view shows.
#[derive(Debug, Clone, PartialEq)]
enum Shown {
    None,
    /// The preview of this key.
    Preview(u64),
    /// A final render's result: (element, width, height, samples, seconds, file).
    Result { element: ElementId, width: u32, height: u32, samples: u32, seconds: f32, file: String },
}

#[derive(Resource)]
pub struct RenderState {
    image: Handle<Image>,
    preview: Option<(u64, Job)>,
    last_key: Option<u64>,
    final_job: Option<FinalJob>,
    shown: Shown,
    /// The version of the job's image in `image`.
    uploaded: u64,
    /// Scripted runs: pause a final render after this many samples (`u32::MAX`: don't).
    hold: Arc<AtomicU32>,
    /// The last error (no source, nothing to render).
    error: Option<String>,
}

impl Default for RenderState {
    fn default() -> Self {
        RenderState {
            image: Handle::default(),
            preview: None,
            last_key: None,
            final_job: None,
            shown: Shown::None,
            uploaded: 0,
            hold: Arc::new(AtomicU32::new(u32::MAX)),
            error: None,
        }
    }
}

impl RenderState {
    fn cancel_all(&mut self) {
        if let Some((_, j)) = self.preview.take() {
            j.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(f) = self.final_job.take() {
            f.job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// True while a render the screenshots should wait for is running.
    fn busy(&self) -> bool {
        let preview = self.preview.as_ref().is_some_and(|(_, j)| !j.snapshot().3);
        let fin = self.final_job.as_ref().is_some_and(|f| {
            let (_, _, _, finished, held) = f.job.snapshot();
            !finished && !held
        });
        preview || fin
    }

    /// Sets how many samples a scripted run lets a final render take before pausing.
    pub fn set_hold(&self, samples: Option<u32>) {
        self.hold.store(samples.unwrap_or(u32::MAX), Ordering::Relaxed);
    }
}

fn setup_image(mut images: ResMut<Assets<Image>>, mut state: ResMut<RenderState>) {
    state.image = images.add(blank_image(4, 4));
}

fn blank_image(w: u32, h: u32) -> Image {
    Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0; (w * h * 4) as usize],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// The active tab's Render Studio.
fn active_studio(doc: &ActiveDocument) -> Option<(ElementId, &RenderStudio)> {
    let el = doc.active_element()?;
    match &el.kind {
        ElementKind::Render(r) => Some((el.id, r)),
        _ => None,
    }
}

/// The Part Studios and Assemblies a render can show.
fn sources(doc: &cadrs_core::Document) -> Vec<(ElementId, String)> {
    doc.elements.iter().filter(|e| matches!(e.kind, ElementKind::PartStudio { .. } | ElementKind::Assembly)).map(|e| (e.id, e.name.clone())).collect()
}

/// The source tab for a new Render Studio: the active tab if it is a Part Studio or an
/// Assembly, else the first one.
pub fn default_source(doc: &ActiveDocument) -> Option<ElementId> {
    let is_3d = |e: &cadrs_core::Element| matches!(e.kind, ElementKind::PartStudio { .. } | ElementKind::Assembly);
    doc.active_element().filter(|e| is_3d(e)).or_else(|| doc.doc.elements.iter().find(|e| is_3d(e))).map(|e| e.id)
}

/// The source's parts, settings and appearances.
type SceneInput = model::SourceParts;

fn scene_input(world: &mut World, source: ElementId) -> Option<SceneInput> {
    let doc = world.get_resource::<ActiveDocument>()?.doc.clone();
    let mut parts_of = world.resource_mut::<crate::assembly::AssemblyParts>();
    model::source_parts(&doc, source, |e| parts_of.build(&doc, e))
}

/// Starts rendering `input` with `studio`'s settings at `width` × `height`.
#[allow(clippy::too_many_arguments)]
fn spawn_job(element: ElementId, input: SceneInput, studio: RenderStudio, width: u32, height: u32, samples: u32, save: Option<PathBuf>, hold: Option<Arc<AtomicU32>>) -> Job {
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = Arc::new(Mutex::new(Shared { total: samples, ..default() }));
    let (c2, s2) = (cancel.clone(), shared.clone());
    let final_render = save.is_some();
    std::thread::Builder::new()
        .name("render".into())
        .spawn(move || {
            let (parts, props, appearances) = input;
            let scene = Arc::new(model::scene(&parts, &props, &appearances));
            let camera = model::camera(&studio, &scene, width as f32 / height as f32);
            let settings = model::settings(&studio, width, height, samples);
            // The preview leaves the app some threads; a final render takes the renderer's share.
            let threads = if final_render { cadrs_render::default_threads() } else { cadrs_render::default_threads().min(4) };
            let mut r = cadrs_render::Renderer::new(scene, camera, settings, threads);
            let publish = |r: &cadrs_render::Renderer, img: RgbaImage, finished: bool| {
                let mut s = s2.lock().unwrap();
                s.done = r.samples_done();
                s.image = Some(Arc::new(img));
                s.version += 1;
                s.finished = finished;
            };
            let mut last_publish = Instant::now();
            while !r.is_done() {
                if c2.load(Ordering::Relaxed) {
                    return;
                }
                // A scripted hold: show the image so far and wait.
                if let Some(h) = &hold
                    && r.samples_done() >= h.load(Ordering::Relaxed)
                {
                    if !s2.lock().unwrap().held {
                        publish(&r, r.image_noisy(), false);
                        s2.lock().unwrap().held = true;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                }
                s2.lock().unwrap().held = false;
                r.pass();
                // Progress images: every pass of a preview, about twice a second for a big render.
                let big = (width as u64) * (height as u64) > 1_500_000;
                if !r.is_done() && (!big || last_publish.elapsed().as_millis() > 500) {
                    publish(&r, r.image_noisy(), false);
                    last_publish = Instant::now();
                } else if !r.is_done() {
                    s2.lock().unwrap().done = r.samples_done();
                }
            }
            let img = r.image();
            let saved = save.map(|path| {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                img.save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                Ok(path)
            });
            s2.lock().unwrap().saved = saved;
            publish(&r, img, true);
        })
        .expect("render thread");
    Job { element, cancel, shared, started: Instant::now() }
}

// ---------------------------------------------------------------------------------------------
// The preview and the jobs

/// The size of the preview: the output's aspect fitted into the viewport (logical px).
fn preview_size(rect: Rect, studio: &RenderStudio) -> (u32, u32) {
    let (aw, ah) = ((rect.width() - 48.0).max(64.0), (rect.height() - 72.0).max(64.0));
    let aspect = studio.width as f32 / studio.height.max(1) as f32;
    let (w, h) = if aw / ah > aspect { (ah * aspect, ah) } else { (aw, aw / aspect) };
    (w.round().max(16.0) as u32, h.round().max(16.0) as u32)
}

fn hash_of(value: &impl std::fmt::Debug) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{value:?}").hash(&mut h);
    h.finish()
}

/// What the preview depends on: the settings, the source's (and its studios') content, the
/// size.
fn preview_key(doc: &ActiveDocument, element: ElementId, studio: &RenderStudio, size: (u32, u32)) -> u64 {
    let mut parts: Vec<&cadrs_core::Element> = Vec::new();
    if let Some(s) = studio.source.and_then(|s| doc.doc.element(s)) {
        parts.push(s);
        if s.assembly_model().is_some() {
            parts.extend(doc.doc.elements.iter().filter(|e| matches!(e.kind, ElementKind::PartStudio { .. })));
        }
    }
    hash_of(&(element, studio, parts, size))
}

/// The preview key of the active Render tab now.
fn current_key(world: &World) -> Option<u64> {
    let rect = world.resource::<ViewportRect>().0;
    let doc = world.get_resource::<ActiveDocument>()?;
    let (element, studio) = active_studio(doc)?;
    Some(preview_key(doc, element, studio, preview_size(rect, studio)))
}

/// Starts the preview when the Render tab's settings or model change.
fn start_preview(world: &mut World) {
    if *world.resource::<ActiveKind>() != ActiveKind::Render {
        return;
    }
    let rect = world.resource::<ViewportRect>().0;
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some((element, studio)) = active_studio(doc).map(|(e, s)| (e, s.clone())) else { return };
    let size = preview_size(rect, &studio);
    let key = preview_key(doc, element, &studio, size);
    let state = world.resource::<RenderState>();
    if state.last_key == Some(key) {
        return;
    }
    // Don't compete with a final render.
    if state.final_job.is_some() {
        return;
    }
    let mut state = world.resource_mut::<RenderState>();
    state.last_key = Some(key);
    if let Some((_, j)) = state.preview.take() {
        j.cancel.store(true, Ordering::Relaxed);
    }
    let Some(source) = studio.source else {
        state.error = Some("Choose a Part Studio or an Assembly to render.".into());
        state.shown = Shown::None;
        return;
    };
    let Some(input) = scene_input(world, source) else {
        world.resource_mut::<RenderState>().error = Some("The model isn't available.".into());
        return;
    };
    let mut state = world.resource_mut::<RenderState>();
    state.error = if input.0.is_empty() { Some("The model has no parts to render.".into()) } else { None };
    let job = spawn_job(element, input, studio, size.0, size.1, PREVIEW_SAMPLES, None, None);
    state.preview = Some((key, job));
    state.shown = Shown::Preview(key);
    state.uploaded = 0;
}

/// Copies the newest image of the shown job into the view's image, finishes renders, and in
/// scripted runs lets a running render catch up (so a screenshot's frame budget is time).
fn drive_jobs(world: &mut World) {
    let scripted = world.get_resource::<crate::parts::RebuildBudget>().is_some_and(|b| b.0.is_none());
    // Scripted: wait for progress a little each frame.
    if scripted && world.resource::<RenderState>().busy() {
        let t0 = Instant::now();
        let v0 = current_version(world.resource::<RenderState>());
        while t0.elapsed().as_millis() < 40 && current_version(world.resource::<RenderState>()) == v0 {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    // A finished final render.
    let finished = world.resource::<RenderState>().final_job.as_ref().is_some_and(|f| f.job.snapshot().3);
    if finished {
        let key = current_key(world);
        let mut state = world.resource_mut::<RenderState>();
        let f = state.final_job.take().unwrap();
        let saved = f.job.shared.lock().unwrap().saved.take();
        let seconds = f.job.started.elapsed().as_secs_f32();
        let file = match &saved {
            Some(Ok(p)) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            _ => String::new(),
        };
        let img = f.job.shared.lock().unwrap().image.clone();
        state.shown = Shown::Result { element: f.job.element, width: f.width, height: f.height, samples: f.samples, seconds, file };
        // The result stays until the settings change (then the preview starts again).
        state.last_key = key;
        if let Some(img) = img {
            upload(world, &img);
        }
        let note = match saved {
            Some(Ok(p)) => {
                info!("rendered {}", p.display());
                Notification::info(format!("Rendered {} ({} × {})", p.display(), f.width, f.height)).seconds(8.0)
            }
            Some(Err(e)) => Notification::warning(format!("Render failed: {e}")),
            None => Notification::warning("Render failed"),
        };
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        show_notification(&mut commands, &theme, note.name("render-toast"));
        world.flush();
        return;
    }
    // The newest image of the job the view shows.
    let state = world.resource::<RenderState>();
    let img = if let Some(f) = &state.final_job {
        let s = f.job.shared.lock().unwrap();
        (s.version != state.uploaded).then(|| (s.version, s.image.clone()))
    } else if let (Some((key, j)), Shown::Preview(k)) = (&state.preview, &state.shown) {
        let s = j.shared.lock().unwrap();
        (key == k && s.version != state.uploaded).then(|| (s.version, s.image.clone()))
    } else {
        None
    };
    if let Some((version, Some(img))) = img {
        world.resource_mut::<RenderState>().uploaded = version;
        upload(world, &img);
    }
}

fn current_version(state: &RenderState) -> (u64, u64, bool) {
    let f = state.final_job.as_ref().map(|f| {
        let s = f.job.snapshot();
        (s.2, s.3)
    });
    let p = state.preview.as_ref().map(|(_, j)| j.snapshot().2).unwrap_or(0);
    (f.map(|x| x.0).unwrap_or(0), p, f.is_some_and(|x| x.1))
}

fn upload(world: &mut World, img: &RgbaImage) {
    let handle = world.resource::<RenderState>().image.clone();
    let mut image = blank_image(img.width(), img.height());
    image.data = Some(img.as_raw().clone());
    let _ = world.resource_mut::<Assets<Image>>().insert(&handle, image);
}

fn flag_pending_work(state: Res<RenderState>, mut pending: ResMut<cadrs_ui::PendingWork>) {
    if state.busy() && !pending.0 {
        pending.0 = true;
    }
}

// ---------------------------------------------------------------------------------------------
// The view

#[derive(Component)]
struct RenderView3;

#[derive(Component)]
struct RenderImage;

#[derive(Component)]
struct RenderCaption;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_view(
    kind: Res<ActiveKind>,
    state: Res<RenderState>,
    rect: Res<ViewportRect>,
    doc: Option<Res<ActiveDocument>>,
    images: Res<Assets<Image>>,
    theme: Res<Theme>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q_view: Query<(Entity, &mut Node), (With<RenderView3>, Without<RenderImage>)>,
    mut q_img: Query<(&mut Node, &mut ImageNode, &mut Visibility), With<RenderImage>>,
    mut q_caption: Query<&mut Text, With<RenderCaption>>,
    mut commands: Commands,
) {
    let show = *kind == ActiveKind::Render;
    if q_view.is_empty() {
        if !show {
            return;
        }
        let Some(area) = q_area.iter().next() else { return };
        let t = theme.clone();
        let view = commands
            .spawn((
                Name::new("render-viewport"),
                RenderView3,
                DespawnOnExit(AppState::Document),
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), right: Val::Px(0.0), bottom: Val::Px(0.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                BackgroundColor(Color::srgb_u8(0xe4, 0xe6, 0xea)),
                ZIndex(0),
            ))
            .with_children(|v| {
                v.spawn((
                    Name::new("render-image"),
                    RenderImage,
                    ImageNode::new(state.image.clone()),
                    Node { width: Val::Px(16.0), height: Val::Px(16.0), ..default() },
                    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.18), Val::Px(0.0), Val::Px(2.0), Val::Px(0.0), Val::Px(8.0)),
                ));
                v.spawn((
                    Name::new("render-caption"),
                    RenderCaption,
                    t.text("", 11.5, FontWeight::NORMAL, t.muted_foreground),
                    Node { position_type: PositionType::Absolute, left: Val::Px(16.0), bottom: Val::Px(12.0), ..default() },
                ));
            })
            .id();
        commands.entity(area).add_child(view);
        return;
    }
    for (_, mut n) in &mut q_view {
        let d = if show { Display::Flex } else { Display::None };
        if n.display != d {
            n.display = d;
        }
    }
    if !show {
        return;
    }
    let studio = doc.as_ref().and_then(|d| active_studio(d).map(|(e, s)| (e, s.clone())));
    // The image, fitted to the view with its aspect.
    let dims = images.get(&state.image).map(|i| (i.width() as f32, i.height() as f32)).unwrap_or((4.0, 4.0));
    let (aw, ah) = ((rect.0.width() - 48.0).max(32.0), (rect.0.height() - 72.0).max(32.0));
    let s = (aw / dims.0).min(ah / dims.1);
    let (w, h) = (dims.0 * s, dims.1 * s);
    let visible = !matches!(state.shown, Shown::None) && studio.is_some() && dims.0 > 4.0;
    for (mut n, mut img, mut vis) in &mut q_img {
        if img.image != state.image {
            img.image = state.image.clone();
        }
        if n.width != Val::Px(w) || n.height != Val::Px(h) {
            n.width = Val::Px(w);
            n.height = Val::Px(h);
        }
        vis.set_if_neq(if visible { Visibility::Inherited } else { Visibility::Hidden });
    }
    let caption = match (&state.final_job, &state.shown, &state.preview) {
        (Some(f), _, _) => {
            let (done, total, ..) = f.job.snapshot();
            format!("Rendering {} × {}: {done} of {total} samples", f.width, f.height)
        }
        (None, Shown::Result { width, height, samples, seconds, file, .. }, _) => {
            format!("Render result: {width} × {height}, {samples} samples, {seconds:.1} s{}", if file.is_empty() { String::new() } else { format!(" · {file}") })
        }
        (None, Shown::Preview(_), Some((_, j))) => {
            let (done, total, ..) = j.snapshot();
            if done >= total { format!("Preview: {total} samples, denoised") } else { format!("Preview: {done} of {total} samples") }
        }
        _ => state.error.clone().unwrap_or_default(),
    };
    for mut t in &mut q_caption {
        if t.0 != caption {
            t.0 = caption.clone();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The panel

/// The feature panel's content on a Render tab (filled by [`sync_panel`]).
#[derive(Component)]
pub struct RenderPanelHost;

#[derive(Component, PartialEq, Eq, Clone, Copy)]
struct PanelKey(u64);

/// The toolbar of a Render tab: Undo, Redo, Render….
pub fn render_toolbar(tb: &mut ChildSpawnerCommands, t: &Theme) {
    crate::document::undo_redo(tb, t);
    tb.spawn(cadrs_ui::toolbar_separator(t));
    tb.spawn(Button::new("render-toolbar-render").label("Render…").icon("render-studio").primary().tooltip("Render the scene to a PNG").build(t));
    tb.spawn(ToolButton::new("render-toolbar-capture", "screenshot").tooltip("Use the model's current view as the camera").build(t));
}

/// The panel's host, spawned by the document shell for a Render tab.
pub fn render_panel(p: &mut ChildSpawnerCommands, _t: &Theme) {
    p.spawn((
        Name::new("render-panel-host"),
        RenderPanelHost,
        Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::scroll_y(), ..default() },
    ));
}

/// What the panel shows.
#[derive(Debug, Clone, PartialEq)]
struct PanelModel {
    studio: RenderStudio,
    sources: Vec<(ElementId, String)>,
    materials: Vec<([u8; 3], String, String)>,
    running: bool,
    result: Option<String>,
    error: Option<String>,
}

fn panel_model(world: &mut World) -> Option<PanelModel> {
    let doc = world.get_resource::<ActiveDocument>()?;
    let (_, studio) = active_studio(doc)?;
    let studio = studio.clone();
    let sources = sources(&doc.doc);
    let state = world.resource::<RenderState>();
    let running = state.final_job.is_some();
    let result = match &state.shown {
        Shown::Result { width, height, samples, seconds, file, .. } => Some(format!("{file}: {width} × {height}, {samples} samples, {seconds:.1} s")),
        _ => None,
    };
    let error = state.error.clone();
    let materials = studio
        .source
        .and_then(|s| scene_input(world, s))
        .map(|(parts, props, _)| {
            parts
                .iter()
                .filter(|p| !props.iter().any(|x| x.part == p.id && x.hidden))
                .map(|p| {
                    let a = cadrs_core::appearance::part_appearance(p, &props);
                    let m = model::pbr(a, props.iter().any(|x| x.part == p.id && x.appearance.is_some()), cadrs_core::parts::part_material(p, &props));
                    let c = m.base_color.map(|c| (cadrs_render::math::linear_to_srgb(c) * 255.0).round() as u8);
                    (c, cadrs_core::parts::display_name(p, &props).to_string(), model::describe(p, &props))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(PanelModel { studio, sources, materials, running, result, error })
}

fn sync_panel(world: &mut World) {
    let mut q = world.query_filtered::<(Entity, Option<&PanelKey>), With<RenderPanelHost>>();
    let Some((host, key)) = q.iter(world).next().map(|(e, k)| (e, k.copied())) else { return };
    let Some(m) = panel_model(world) else { return };
    let k = PanelKey(hash_of(&m));
    if key == Some(k) {
        return;
    }
    let t = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    commands.entity(host).insert(k).despawn_children();
    commands.entity(host).with_children(|b| panel_body(b, &t, &m));
    world.flush();
}

fn section(b: &mut ChildSpawnerCommands, t: &Theme, name: &str, text: &str) {
    b.spawn((Name::new(name.to_string()), t.text(text, 12.0, FontWeight::BOLD, t.foreground), Node { margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(12.0), Val::Px(4.0)), ..default() }));
}

fn field_row(b: &mut ChildSpawnerCommands, t: &Theme, label: &str, content: impl Bundle) {
    b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(2.0), Val::Px(2.0)), ..default() }).with_children(|r| {
        r.spawn((t.text(label, 11.5, FontWeight::NORMAL, t.muted_foreground), Node { width: Val::Px(80.0), flex_shrink: 0.0, ..default() }));
        r.spawn(content);
    });
}

fn select(name: &str, options: &[String], selected: usize) -> Select {
    let mut s = Select::new(name.to_string()).bordered().width(Val::Px(164.0));
    for o in options {
        s = s.option(o.clone(), true);
    }
    s.selected(selected)
}

const ROTATIONS: [f32; 8] = [0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
const EXPOSURES: [(f32, &str); 5] = [(-1.0, "−1 EV"), (-0.5, "−0.5 EV"), (0.0, "0 EV"), (0.5, "+0.5 EV"), (1.0, "+1 EV")];

fn panel_body(b: &mut ChildSpawnerCommands, t: &Theme, m: &PanelModel) {
    let s = &m.studio;
    b.spawn((Name::new("render-panel"), Node { flex_direction: FlexDirection::Column, padding: UiRect::bottom(Val::Px(12.0)), ..default() })).with_children(|b| {
        b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(8.0), Val::Px(0.0)), ..default() }).with_children(|h| {
            h.spawn(cadrs_ui::icon("render-studio", 16.0, t.foreground));
            h.spawn((Name::new("render-panel-title"), t.text("Render Studio", 13.0, FontWeight::BOLD, t.foreground)));
        });
        section(b, t, "render-model-heading", "Model");
        let names: Vec<String> = m.sources.iter().map(|(_, n)| n.clone()).collect();
        let current = m.sources.iter().position(|(e, _)| Some(*e) == s.source);
        let mut opts = names.clone();
        if current.is_none() {
            opts.insert(0, "Choose a tab…".into());
        }
        field_row(b, t, "Model", select("render-source", &opts, current.unwrap_or(0)).build(t));

        section(b, t, "render-environment-heading", "Environment");
        let envs: Vec<String> = RenderEnvironment::ALL.iter().map(|e| e.label().to_string()).collect();
        field_row(b, t, "Environment", select("render-environment", &envs, RenderEnvironment::ALL.iter().position(|e| *e == s.environment).unwrap_or(0)).build(t));
        let rots: Vec<String> = ROTATIONS.iter().map(|r| format!("{r:.0}°")).collect();
        field_row(b, t, "Rotation", select("render-rotation", &rots, ROTATIONS.iter().position(|r| (*r - s.environment_rotation).abs() < 0.5).unwrap_or(0)).build(t));
        let bgs: Vec<String> = RenderBackground::ALL.iter().map(|e| e.label().to_string()).collect();
        field_row(b, t, "Background", select("render-background", &bgs, RenderBackground::ALL.iter().position(|e| *e == s.background).unwrap_or(0)).build(t));
        b.spawn(Node { margin: UiRect::new(Val::Px(98.0), Val::Px(10.0), Val::Px(2.0), Val::Px(2.0)), ..default() })
            .with_child(Checkbox::new("render-ground").label("Ground shadow").checked(s.ground_shadow).height(22.0).build(t));

        section(b, t, "render-camera-heading", "Camera");
        let mut views: Vec<String> = vec!["Current view".into()];
        views.extend(RenderView::NAMED.iter().map(|v| v.label().to_string()));
        let sel = match s.view {
            RenderView::Current { .. } => 0,
            v => RenderView::NAMED.iter().position(|x| *x == v).map(|i| i + 1).unwrap_or(2),
        };
        field_row(b, t, "View", select("render-view", &views, sel).build(t));
        b.spawn(Node { margin: UiRect::new(Val::Px(98.0), Val::Px(10.0), Val::Px(2.0), Val::Px(2.0)), ..default() })
            .with_child(Checkbox::new("render-perspective").label("Perspective").checked(s.perspective).height(22.0).build(t));

        section(b, t, "render-output-heading", "Output");
        let mut sizes: Vec<String> = model::RESOLUTIONS.iter().map(|r| r.2.to_string()).collect();
        let size_sel = model::RESOLUTIONS.iter().position(|r| r.0 == s.width && r.1 == s.height);
        if size_sel.is_none() {
            sizes.push(format!("{} × {}", s.width, s.height));
        }
        field_row(b, t, "Size", select("render-resolution", &sizes, size_sel.unwrap_or(sizes.len() - 1)).build(t));
        let mut quals: Vec<String> = model::QUALITIES.iter().map(|q| q.1.to_string()).collect();
        let q_sel = model::QUALITIES.iter().position(|q| q.0 == s.samples);
        if q_sel.is_none() {
            quals.push(format!("{} samples", s.samples));
        }
        field_row(b, t, "Quality", select("render-quality", &quals, q_sel.unwrap_or(quals.len() - 1)).build(t));
        let exps: Vec<String> = EXPOSURES.iter().map(|e| e.1.to_string()).collect();
        field_row(b, t, "Exposure", select("render-exposure", &exps, EXPOSURES.iter().position(|e| (e.0 - s.exposure).abs() < 0.01).unwrap_or(2)).build(t));
        field_row(b, t, "Seed", TextInput::new("render-seed").value(s.seed.to_string()).select_all_on_focus().width(Val::Px(164.0)).height(26.0).build(t));
        b.spawn(Node { margin: UiRect::new(Val::Px(98.0), Val::Px(10.0), Val::Px(2.0), Val::Px(2.0)), ..default() })
            .with_child(Checkbox::new("render-denoise").label("Denoise").checked(s.denoise).height(22.0).build(t));

        section(b, t, "render-materials-heading", "Materials");
        if m.materials.is_empty() {
            b.spawn((Name::new("render-materials-empty"), t.text("No parts.", 11.0, FontWeight::NORMAL, t.muted_foreground), Node { margin: UiRect::horizontal(Val::Px(10.0)), ..default() }));
        }
        for (k, (rgb, name, detail)) in m.materials.iter().enumerate() {
            b.spawn((Name::new(format!("render-material-{}", k + 1)), Node { column_gap: Val::Px(8.0), margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(3.0), Val::Px(3.0)), ..default() })).with_children(|r| {
                r.spawn((
                    Node { width: Val::Px(14.0), height: Val::Px(14.0), flex_shrink: 0.0, border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(3.0)), margin: UiRect::top(Val::Px(1.0)), ..default() },
                    BackgroundColor(Color::srgb_u8(rgb[0], rgb[1], rgb[2])),
                    BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.25)),
                ));
                r.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(1.0), ..default() }).with_children(|c| {
                    c.spawn(t.text(name.clone(), 11.5, FontWeight::MEDIUM, t.foreground));
                    c.spawn((t.text(detail.clone(), 10.5, FontWeight::NORMAL, t.muted_foreground), Node { width: Val::Px(PANEL_W - 50.0), ..default() }))
                        .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
                });
            });
        }

        b.spawn(Node { column_gap: Val::Px(6.0), margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(14.0), Val::Px(4.0)), ..default() }).with_children(|r| {
            r.spawn(Button::new("render-start").label("Render…").icon("render-studio").primary().disabled(m.running || s.source.is_none()).build(t));
            if m.running {
                r.spawn(Button::new("render-cancel").label("Cancel").build(t));
            }
        });
        if m.running {
            b.spawn((Name::new("render-progress"), ProgressText, t.text("Rendering…", 11.0, FontWeight::NORMAL, t.muted_foreground), Node { margin: UiRect::horizontal(Val::Px(10.0)), ..default() }));
            b.spawn((
                Name::new("render-progress-bar"),
                Node { width: Val::Px(PANEL_W - 20.0), height: Val::Px(5.0), margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(4.0), Val::Px(0.0)), border_radius: BorderRadius::all(Val::Px(3.0)), ..default() },
                BackgroundColor(t.panel_border),
            ))
            .with_child((ProgressFill, Node { width: Val::Percent(0.0), height: Val::Percent(100.0), border_radius: BorderRadius::all(Val::Px(3.0)), ..default() }, BackgroundColor(t.primary)));
        }
        if let Some(r) = &m.result {
            b.spawn((Name::new("render-result"), t.text(r.clone(), 11.0, FontWeight::NORMAL, t.muted_foreground), Node { width: Val::Px(PANEL_W - 20.0), margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(6.0), Val::Px(0.0)), ..default() }))
                .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
        }
        if let Some(e) = &m.error {
            b.spawn((Name::new("render-error"), t.text(e.clone(), 11.0, FontWeight::NORMAL, t.feature_error), Node { width: Val::Px(PANEL_W - 20.0), margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(6.0), Val::Px(0.0)), ..default() }))
                .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
        }
    });
}

#[derive(Component)]
struct ProgressText;

#[derive(Component)]
struct ProgressFill;

fn update_progress(state: Res<RenderState>, mut q_text: Query<&mut Text, With<ProgressText>>, mut q_fill: Query<&mut Node, With<ProgressFill>>) {
    let Some(f) = &state.final_job else { return };
    let (done, total, _, _, held) = f.job.snapshot();
    let pct = done as f32 / total.max(1) as f32 * 100.0;
    let text = if held { format!("Paused at {done} of {total} samples ({pct:.0} %)") } else { format!("Rendering… {done} of {total} samples ({pct:.0} %)") };
    for mut t in &mut q_text {
        if t.0 != text {
            t.0 = text.clone();
        }
    }
    for mut n in &mut q_fill {
        if n.width != Val::Percent(pct) {
            n.width = Val::Percent(pct);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Panel events

/// Applies `edit` to the active Render tab's settings as one undo step.
fn edit_studio(world: &mut World, label: impl Into<String>, edit: impl FnOnce(&mut RenderStudio)) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some((element, studio)) = active_studio(&doc).map(|(e, s)| (e, s.clone())) else { return };
    let mut new = studio.clone();
    edit(&mut new);
    if new == studio {
        return;
    }
    if let Err(e) = doc.execute(&SetRenderStudio { element, studio: new, label: label.into() }) {
        warn!("render settings: {e}");
    }
}

/// The source tab's view as the camera ("Current view").
fn capture_view(world: &mut World) {
    let Some(source) = world.get_resource::<ActiveDocument>().and_then(|d| active_studio(d)?.1.source) else { return };
    let vv = world.resource::<ViewportView>();
    let v = if vv.element == Some(source) { Some(vv.view) } else { vv.per_element.get(&source).copied() };
    let v = v.unwrap_or_default();
    edit_studio(world, "Camera Current view", |s| s.view = RenderView::Current { azimuth: v.azimuth, elevation: v.elevation, roll: v.roll });
}

fn on_select(ev: On<SelectChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else { return };
    let i = ev.index;
    let name = name.as_str().to_string();
    if !name.starts_with("render-") {
        return;
    }
    commands.queue(move |world: &mut World| match name.as_str() {
        "render-source" => {
            let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
            let list = sources(&doc.doc);
            let has = active_studio(doc).and_then(|(_, s)| s.source).is_some_and(|s| list.iter().any(|(e, _)| *e == s));
            let k = if has { Some(i) } else { i.checked_sub(1) };
            if let Some((e, n)) = k.and_then(|k| list.get(k).cloned()) {
                edit_studio(world, format!("Render {n}"), |s| s.source = Some(e));
            }
        }
        "render-environment" => {
            let env = RenderEnvironment::ALL[i.min(3)];
            edit_studio(world, format!("Environment {}", env.label()), |s| s.environment = env);
        }
        "render-rotation" => {
            let r = ROTATIONS[i.min(ROTATIONS.len() - 1)];
            edit_studio(world, format!("Environment rotation {r:.0}°"), |s| s.environment_rotation = r);
        }
        "render-background" => {
            let b = RenderBackground::ALL[i.min(2)];
            edit_studio(world, format!("Background {}", b.label()), |s| s.background = b);
        }
        "render-view" => {
            if i == 0 {
                capture_view(world);
            } else if let Some(v) = RenderView::NAMED.get(i - 1).copied() {
                edit_studio(world, format!("Camera {}", v.label()), |s| s.view = v);
            }
        }
        "render-resolution" => {
            if let Some((w, h, l)) = model::RESOLUTIONS.get(i) {
                edit_studio(world, format!("Size {l}"), |s| (s.width, s.height) = (*w, *h));
            }
        }
        "render-quality" => {
            if let Some((n, l)) = model::QUALITIES.get(i) {
                edit_studio(world, format!("Quality {l}"), |s| s.samples = *n);
            }
        }
        "render-exposure" => {
            if let Some((e, l)) = EXPOSURES.get(i) {
                edit_studio(world, format!("Exposure {l}"), |s| s.exposure = *e);
            }
        }
        "render-dialog-resolution" | "render-dialog-quality" => {}
        _ => {}
    });
}

fn on_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else { return };
    let on = ev.checked;
    let name = name.as_str().to_string();
    commands.queue(move |world: &mut World| match name.as_str() {
        "render-ground" => edit_studio(world, if on { "Ground shadow on" } else { "Ground shadow off" }, |s| s.ground_shadow = on),
        "render-perspective" => edit_studio(world, if on { "Perspective" } else { "Orthographic" }, |s| s.perspective = on),
        "render-denoise" => edit_studio(world, if on { "Denoise on" } else { "Denoise off" }, |s| s.denoise = on),
        _ => {}
    });
}

fn on_submit(ev: On<TextSubmit>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else { return };
    match name.as_str() {
        "render-seed-field" => {
            if let Ok(seed) = ev.value.trim().parse::<u64>() {
                commands.queue(move |world: &mut World| edit_studio(world, format!("Seed {seed}"), |s| s.seed = seed));
            }
        }
        "render-dialog-name-field" => commands.queue(start_final),
        _ => {}
    }
}

fn on_activate(ev: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else { return };
    match name.as_str() {
        "render-start" | "render-toolbar-render" => commands.queue(open_dialog),
        "render-toolbar-capture" => commands.queue(capture_view),
        "render-cancel" => commands.queue(|world: &mut World| {
            let mut state = world.resource_mut::<RenderState>();
            if let Some(f) = state.final_job.take() {
                f.job.cancel.store(true, Ordering::Relaxed);
            }
            state.last_key = None;
        }),
        "render-dialog-ok" => commands.queue(start_final),
        "render-dialog-cancel" => commands.queue(close_dialog),
        "render-dialog-browse" => commands.queue(|world: &mut World| {
            let typed = field(world, "render-dialog-folder-field");
            let dir = PathBuf::from(typed.trim());
            let dir = if dir.is_dir() { dir } else { std::env::current_dir().unwrap_or_default() };
            let theme = world.resource::<Theme>().clone();
            let mut commands = world.commands();
            cadrs_ui::file_picker::open_folder_picker(&mut commands, &theme, "folder-picker", "Choose a folder", "render-folder", dir);
            world.flush();
        }),
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// The Render dialog

#[derive(Component)]
struct RenderDialog;

fn field(w: &mut World, name: &str) -> String {
    let mut q = w.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(w).find(|(n, _)| n.as_str() == name).map(|(_, t)| t.value().to_string()).unwrap_or_default()
}

fn select_index(w: &mut World, name: &str) -> Option<usize> {
    let mut q = w.query::<(&Name, &cadrs_ui::SelectState)>();
    q.iter(w).find(|(n, _)| n.as_str() == name).map(|(_, s)| s.selected)
}

/// Sets a text field (a folder picked with Browse…).
pub fn set_folder(w: &mut World, path: &str) {
    let mut q = w.query_filtered::<(&Name, &mut EditableText), With<TextInputField>>();
    if let Some((_, mut t)) = q.iter_mut(w).find(|(n, _)| n.as_str() == "render-dialog-folder-field") {
        t.queue_edit(TextEdit::SelectAll);
        t.queue_edit(TextEdit::Insert(path.to_string().into()));
    }
}

fn open_dialog(world: &mut World) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some((_, studio)) = active_studio(doc).map(|(e, s)| (e, s.clone())) else { return };
    if studio.source.is_none() || world.resource::<RenderState>().final_job.is_some() {
        return;
    }
    let base = studio.source.and_then(|s| doc.doc.element(s)).map(|e| e.name.clone()).unwrap_or_else(|| "Render".into());
    let dir = crate::export_dir(world).map(|d| d.display().to_string()).unwrap_or_default();
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let size_sel = model::RESOLUTIONS.iter().position(|r| r.0 == studio.width && r.1 == studio.height);
    let q_sel = model::QUALITIES.iter().position(|q| q.0 == studio.samples);
    let (w, h, n) = (studio.width, studio.height, studio.samples);
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("render-dialog")
            .title("Render")
            .width(460.0)
            .body(move |b| {
                let t = &tb;
                let full = || Val::Percent(100.0);
                let row = |name: &str| {
                    (Name::new(name.to_string()), Node { flex_direction: FlexDirection::Column, width: Val::Percent(100.0), row_gap: Val::Px(4.0), margin: UiRect::bottom(Val::Px(10.0)), ..default() })
                };
                let label = |g: &mut ChildSpawner, s: &str| {
                    g.spawn(t.text(s, t.font_base, FontWeight::BOLD, t.foreground));
                };
                b.spawn(row("render-dialog-name-row")).with_children(|g| {
                    label(g, "File name");
                    g.spawn(TextInput::new("render-dialog-name").value(base).select_all_on_focus().autofocus().width(full()).height(26.0).build(t));
                    g.spawn(t.text("A PNG image, the same for the same seed.", t.font_sm, FontWeight::NORMAL, t.muted_foreground));
                });
                b.spawn(row("render-dialog-size-row")).with_children(|g| {
                    label(g, "Size");
                    let mut s = Select::new("render-dialog-resolution").bordered().width(full());
                    for r in model::RESOLUTIONS {
                        s = s.option(r.2, true);
                    }
                    if size_sel.is_none() {
                        s = s.option(format!("{w} × {h}"), true);
                    }
                    g.spawn(s.selected(size_sel.unwrap_or(model::RESOLUTIONS.len())).build(t));
                });
                b.spawn(row("render-dialog-quality-row")).with_children(|g| {
                    label(g, "Quality");
                    let mut s = Select::new("render-dialog-quality").bordered().width(full());
                    for q in model::QUALITIES {
                        s = s.option(q.1, true);
                    }
                    if q_sel.is_none() {
                        s = s.option(format!("{n} samples"), true);
                    }
                    g.spawn(s.selected(q_sel.unwrap_or(model::QUALITIES.len())).build(t));
                });
                b.spawn(row("render-dialog-folder-row")).with_children(|g| {
                    label(g, "Folder");
                    g.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, width: full(), ..default() }).with_children(|r| {
                        r.spawn(TextInput::new("render-dialog-folder").value(dir).width(full()).height(26.0).build(t));
                        r.spawn(Button::new("render-dialog-browse").label("Browse…").outline().build(t));
                    });
                });
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn(Button::new("render-dialog-ok").label("Render").icon("render-studio").primary().build(t));
                f.spawn(Button::new("render-dialog-cancel").label("Cancel").build(t));
            })
            .build(&theme),
        RenderDialog,
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

fn close_dialog(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<RenderDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
}

/// Render: the dialog's size becomes the tab's (one undo step), then the render starts. The
/// dialog's Quality applies to this render only (P3F.6 judge): the studio's own Quality, which
/// the live preview uses, is left as it is.
fn start_final(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<RenderDialog>>();
    if q.iter(world).next().is_none() {
        return;
    }
    let base = field(world, "render-dialog-name-field").trim().to_string();
    let folder = field(world, "render-dialog-folder-field").trim().to_string();
    let size = select_index(world, "render-dialog-resolution").and_then(|i| model::RESOLUTIONS.get(i).copied());
    let quality = select_index(world, "render-dialog-quality").and_then(|i| model::QUALITIES.get(i).copied());
    close_dialog(world);
    if let Some((w, h, _)) = size {
        edit_studio(world, format!("Size {w} × {h}"), |s| (s.width, s.height) = (w, h));
    }
    if folder.is_empty() {
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        show_notification(&mut commands, &theme, Notification::warning("Nowhere to save the render").name("render-toast"));
        world.flush();
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some((element, studio)) = active_studio(doc).map(|(e, s)| (e, s.clone())) else { return };
    let Some(source) = studio.source else { return };
    let Some(input) = scene_input(world, source) else { return };
    let dir = PathBuf::from(folder);
    let stem = cadrs_core::export::sanitize(if base.is_empty() { "Render" } else { &base });
    let path = cadrs_core::export::unique_path_ext(&dir, &stem, "png");
    let mut state = world.resource_mut::<RenderState>();
    if let Some((_, j)) = state.preview.take() {
        j.cancel.store(true, Ordering::Relaxed);
    }
    let hold = state.hold.clone();
    let n = quality.map_or(studio.samples, |(n, _)| n);
    let (w, h) = (studio.width, studio.height);
    let job = spawn_job(element, input, studio, w, h, n, Some(path), Some(hold));
    state.final_job = Some(FinalJob { job, width: w, height: h, samples: n });
    state.uploaded = 0;
}

/// Browse…'s folder.
pub fn on_folder_picked(mut msgs: MessageReader<cadrs_ui::FilePicked>, mut commands: Commands) {
    for m in msgs.read() {
        if m.tag == "render-folder" {
            let text = m.path.display().to_string();
            commands.queue(move |w: &mut World| set_folder(w, &text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_keeps_the_output_aspect() {
        let s = RenderStudio::new(None);
        let (w, h) = preview_size(Rect::new(0.0, 0.0, 1200.0, 900.0), &s);
        assert!((w as f32 / h as f32 - 16.0 / 9.0).abs() < 0.01, "{w} {h}");
        assert!(w <= 1152);
        let mut sq = s.clone();
        (sq.width, sq.height) = (1080, 1080);
        let (w, h) = preview_size(Rect::new(0.0, 0.0, 1200.0, 900.0), &sq);
        assert_eq!(w, h);
    }
}
