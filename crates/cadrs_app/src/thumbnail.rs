//! Real document thumbnails: an offscreen camera renders the document's first tab with
//! something to show in the isometric view into an image, which is read back and saved as the
//! document's `thumbnail.png` (and `thumbnail-large.png`); the documents page shows them and an
//! exported `.cadrs` carries them.
//!
//! **What it shows** ([`Drawn`]): the first tab, in tab order, that is a Part Studio with
//! features, an Assembly with instances or a PCB Studio with a board; the default planes when
//! there is none. A Part Studio is its parts (shaded as in the viewport, with black edges) and
//! its sketches that no feature used (closed regions filled grey with dark edges); an Assembly
//! its instances where the document has them; a board is drawn as its 3D view draws it
//! (components, and a native board's mask, copper and silkscreen). The view zooms to fit, so
//! every document's thumbnail shows its own. The scene lives on its own render layer, so it does
//! not depend on which tab is active or on hover highlights.
//!
//! **When it is drawn**:
//! - While the document is open ([`watch`]): once it changed (the history's revision, so no
//!   comparing of documents every frame), the view has been still for [`IDLE`] seconds (nobody
//!   is turning it, no button is down, no rebuild runs) and what the thumbnail shows is not what
//!   the saved one was drawn from: an edit to a tab the thumbnail doesn't show draws nothing,
//!   moving another tab to the front does. Builds it needs come from the kernel in the
//!   background, so the view never waits for them.
//! - Before anything that hands the thumbnail on ([`then`]): going back to the documents page
//!   ([`close_document`]), exporting the document, closing the window. Those wait for it.

use crate::GizmoGroups;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::WindowCloseRequested;
use cadrs_core::thumbnail::{THUMB_LARGE_H, THUMB_LARGE_W};
use cadrs_core::{DocumentId, ElementId, ElementKind, Feature};
use image::RgbaImage;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::camera::{StandardView, ViewState};
use crate::viewport::{PLANE_HALF, PlaneKind, ViewportView};
use crate::{ActiveDocument, AppState, DocumentStore};

pub struct ThumbnailPlugin;

impl Plugin for ThumbnailPlugin {
    fn build(&self, app: &mut App) {
        app.gizmo_group::<ThumbGizmos>()
            .init_resource::<ThumbnailSketches>()
            .init_resource::<LiveThumbnail>()
            .add_systems(Startup, setup_thumbnail)
            .add_systems(
                Update,
                (
                    draw_thumbnail_scene,
                    drive_capture,
                    close_window,
                    watch.run_if(in_state(AppState::Document)),
                ),
            );
    }
}

/// Supersampling: the thumbnail renders at twice its saved size.
const SS: u32 = 2;
const THUMB_LAYER: usize = 2;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct ThumbGizmos;

#[derive(Resource)]
pub struct ThumbnailTarget(pub Handle<Image>);

#[derive(Component)]
struct ThumbCamera;

#[derive(Component)]
struct ThumbPlane;

/// A sketch's region fill in the thumbnail scene (spawned while closing).
#[derive(Component)]
struct ThumbSketch;

/// The sketches the thumbnail shows (the first Part Studio's, with a plane), and how far
/// toward the camera their edges are drawn (so the fill does not hide them).
#[derive(Resource, Default)]
struct ThumbnailSketches(
    Vec<(cadrs_sketch::Sketch, cadrs_sketch::PlaneRef)>,
    Vec3,
    /// The parts' edge lines.
    Vec<Vec<Vec3>>,
    /// A board is shown (so no planes).
    bool,
);

/// What a thumbnail is drawn from: the document's first tab with something to show, as far as
/// the picture depends on it. Two that are equal draw the same picture, so a saved thumbnail is
/// current while the document's is the one it was drawn from.
#[derive(Clone, PartialEq)]
enum Drawn {
    /// No tab has anything to show: the default planes.
    Planes,
    /// A Part Studio: its features as built, and its parts' and features' appearances.
    Studio {
        element: ElementId,
        features: Vec<Feature>,
        props: Vec<cadrs_core::PartProps>,
        appearances: Vec<(cadrs_core::FeatureId, cadrs_core::Appearance)>,
    },
    /// An Assembly: its parts at any depth as placed, and the Part Studios they come from.
    Assembly {
        element: ElementId,
        occurrences: Vec<cadrs_core::assembly::structure::Occurrence>,
        sources: Vec<(ElementId, Vec<Feature>, Vec<cadrs_core::PartProps>)>,
    },
    /// A PCB Studio's board (the one shown there last).
    Board(Box<cadrs_core::Element>),
}

impl Drawn {
    /// The tab it shows.
    fn element(&self) -> Option<ElementId> {
        match self {
            Drawn::Planes => None,
            Drawn::Studio { element, .. } | Drawn::Assembly { element, .. } => Some(*element),
            Drawn::Board(e) => Some(e.id),
        }
    }
}

/// The board a PCB Studio's thumbnail shows: the one shown there last, else its first.
fn studio_board(
    e: &cadrs_core::Element,
) -> Option<(
    ElementId,
    cadrs_core::pcb::BoardId,
    cadrs_core::pcb::PcbBoard,
)> {
    let s = e.pcb()?;
    let b = s.active.and_then(|id| s.board(id)).or(s.boards.first())?;
    Some((e.id, b.id, b.board.clone()))
}

/// What the document's thumbnail is drawn from: its first tab, in tab order, that has
/// something to show. Told from the document alone (nothing is built): a Part Studio with a
/// part feature or a sketch with curves, an Assembly with a part placed, a PCB Studio with a
/// board.
fn drawn(doc: &cadrs_core::Document) -> Drawn {
    for e in &doc.elements {
        match &e.kind {
            ElementKind::PartStudio { .. } => {
                let features = e.active_features();
                let shows = features.iter().any(|f| {
                    f.is_part_feature()
                        || f.sketch()
                            .is_some_and(|s| s.plane.is_some() && !s.geometry.curves.is_empty())
                });
                if shows {
                    return Drawn::Studio {
                        element: e.id,
                        features,
                        props: e.part_props().to_vec(),
                        appearances: e.feature_appearances().to_vec(),
                    };
                }
            }
            ElementKind::Assembly => {
                let Some(asm) = e.assembly_model() else {
                    continue;
                };
                let occurrences = cadrs_core::assembly::structure::occurrences(doc, asm);
                if occurrences.is_empty() {
                    continue;
                }
                let mut sources: Vec<(ElementId, Vec<Feature>, Vec<cadrs_core::PartProps>)> =
                    Vec::new();
                for o in &occurrences {
                    if sources.iter().any(|(id, ..)| *id == o.element) {
                        continue;
                    }
                    if let Some(src) = doc.element(o.element) {
                        sources.push((
                            src.id,
                            src.part_features().into_owned(),
                            src.part_props().to_vec(),
                        ));
                    }
                }
                return Drawn::Assembly {
                    element: e.id,
                    occurrences,
                    sources,
                };
            }
            ElementKind::PcbStudio(_) if studio_board(e).is_some() => {
                return Drawn::Board(Box::new(e.clone()));
            }
            _ => {}
        }
    }
    Drawn::Planes
}

/// The geometry of a thumbnail's scene.
#[derive(Default)]
struct Content {
    parts: Vec<cadrs_core::Part>,
    props: Vec<cadrs_core::PartProps>,
    appearances: Vec<(cadrs_core::FeatureId, cadrs_core::Appearance)>,
    sketches: Vec<(cadrs_sketch::Sketch, cadrs_sketch::PlaneRef)>,
    board: Option<(
        ElementId,
        cadrs_core::pcb::BoardId,
        cadrs_core::pcb::PcbBoard,
    )>,
}

/// The geometry `drawn` shows. With `wait` the builds it needs are waited for; without, they
/// are asked of the kernel and this is `None` until they are all done (ask again next frame).
fn content(world: &mut World, drawn: &Drawn, wait: bool) -> Option<Content> {
    match drawn {
        Drawn::Planes => Some(Content::default()),
        Drawn::Studio {
            features,
            props,
            appearances,
            ..
        } => {
            let parts = if !features.iter().any(Feature::is_part_feature) {
                Vec::new()
            } else if wait {
                cadrs_core::parts::parts(features)
            } else {
                let mut live = world.resource_mut::<LiveThumbnail>();
                if !live.build.as_ref().is_some_and(|(f, _)| f == features) {
                    live.build = Some((
                        features.clone(),
                        cadrs_core::rebuild::request(features.clone()),
                    ));
                }
                let build = live.build.as_mut().and_then(|(_, p)| p.poll())?;
                live.build = None;
                build.parts.clone()
            };
            let consumed = crate::parts::consumed_sketches(features, None);
            let sketches = features
                .iter()
                .filter(|f| !consumed.contains(&f.id))
                .filter_map(|f| {
                    let s = f.sketch()?;
                    Some((s.geometry.clone(), s.plane?))
                })
                .filter(|(g, _)| !g.curves.is_empty())
                .collect();
            Some(Content {
                parts,
                props: props.clone(),
                appearances: appearances.clone(),
                sketches,
                board: None,
            })
        }
        Drawn::Assembly { element, .. } => {
            world.resource_scope(|world, mut builds: Mut<crate::assembly::AssemblyParts>| {
                let doc = &world.get_resource::<ActiveDocument>()?.doc;
                let asm = doc.element(*element)?.assembly_model()?;
                let budget = (!wait).then_some(std::time::Duration::ZERO);
                let mut ready = true;
                let (mut parts, props) = cadrs_core::assembly::instance_parts(doc, asm, |e| {
                    let (build, done) = builds.build_within(doc, e, budget);
                    ready &= done;
                    build
                });
                if !ready {
                    return None;
                }
                // Hidden instances are not in the picture.
                parts.retain(|p| !props.iter().any(|q| q.part == p.id && q.hidden));
                Some(Content {
                    parts,
                    props,
                    ..default()
                })
            })
        }
        Drawn::Board(e) => Some(Content {
            board: studio_board(e),
            ..default()
        }),
    }
}

/// The thumbnail's view for these sketches: the planes' isometric view, or, with sketches,
/// the isometric view zoomed to fit them.
pub fn thumbnail_view_for(
    sketches: &[(cadrs_sketch::Sketch, cadrs_sketch::PlaneRef)],
) -> ViewState {
    thumbnail_view_with(sketches, &[])
}

/// [`thumbnail_view_for`], also fitting `extra` points (the parts).
pub fn thumbnail_view_with(
    sketches: &[(cadrs_sketch::Sketch, cadrs_sketch::PlaneRef)],
    extra: &[Vec3],
) -> ViewState {
    let mut pts = extra.to_vec();
    for (g, plane) in sketches {
        let frame = plane.frame();
        for id in g.curves.keys() {
            if g.curves[id].construction {
                continue;
            }
            for p in cadrs_sketch::hit::curve_polyline(g, id) {
                let w = frame.to_world(p);
                pts.push(Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32));
            }
        }
    }
    if pts.is_empty() {
        return thumbnail_view();
    }
    ViewState::standard(StandardView::Isometric).fitted(
        &pts,
        Vec2::new(THUMB_LARGE_W as f32, THUMB_LARGE_H as f32),
        0.9,
    )
}

/// What runs once the thumbnail is current ([`then`]).
type Then = Box<dyn FnOnce(&mut World) + Send + Sync>;

const CAPTURING: u8 = 0;
const SAVED: u8 = 1;
const FAILED: u8 = 2;

/// A thumbnail being drawn and saved: its scene is up and its camera is on.
#[derive(Resource)]
pub struct Capture {
    doc: DocumentId,
    /// What it is drawn from.
    drawn: Drawn,
    frames: u32,
    started: web_time::Instant,
    requested: bool,
    /// [`CAPTURING`], then [`SAVED`] or [`FAILED`].
    state: Arc<AtomicU8>,
    /// What waits for it.
    then: Vec<Then>,
}

/// The open document's thumbnail as this session knows it, and the watch for when to draw it
/// again ([`watch`]).
#[derive(Resource, Default)]
struct LiveThumbnail {
    /// The document watched.
    doc: Option<DocumentId>,
    /// Its history's revision when it was last looked at.
    revision: u64,
    /// It changed since the thumbnail was last compared with it (or has no real thumbnail).
    dirty: bool,
    /// Seconds the view has been still.
    idle: f32,
    view: Option<ViewState>,
    /// What the saved thumbnail was drawn from, once this session drew one.
    saved: Option<(DocumentId, Drawn)>,
    /// What the next one is drawn from, while its builds run.
    want: Option<Drawn>,
    /// A Part Studio's build for it, running.
    build: Option<(Vec<Feature>, cadrs_core::rebuild::Pending)>,
}

/// The view the thumbnail uses: isometric, zoomed so the planes fill the image.
pub fn thumbnail_view() -> ViewState {
    let mut v = ViewState::standard(StandardView::Isometric);
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for k in PlaneKind::ALL {
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = (k.u() * su + k.v() * sv) * PLANE_HALF;
            let s = v.project(p);
            lo = lo.min(s);
            hi = hi.max(s);
        }
    }
    let margin = 12.0;
    let size = Vec2::new(THUMB_LARGE_W as f32, THUMB_LARGE_H as f32) - 2.0 * margin;
    let extent = (hi - lo) / size;
    // `project` above used the default scale; scale so the extent fits.
    v.scale *= extent.x.max(extent.y);
    v
}

fn setup_thumbnail(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut store: ResMut<GizmoConfigStore>,
) {
    let mut image = Image::new_target_texture(
        THUMB_LARGE_W * SS,
        THUMB_LARGE_H * SS,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    image.asset_usage = RenderAssetUsages::default();
    let handle = images.add(image);
    commands.insert_resource(ThumbnailTarget(handle.clone()));

    let (config, _) = store.config_mut::<ThumbGizmos>();
    config.render_layers = RenderLayers::layer(THUMB_LAYER);
    // About a pixel in the lists' small copy, two in the details panel.
    config.line.width = 8.0;

    let v = thumbnail_view();
    commands.spawn((
        Name::new("thumbnail-camera"),
        ThumbCamera,
        Camera3d::default(),
        RenderTarget::Image(handle.into()),
        Camera {
            order: -2,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::WindowSize,
            scale: v.scale / SS as f32,
            near: 0.0,
            far: crate::camera::camera_far(v.scale),
            ..OrthographicProjection::default_3d()
        }),
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::layer(THUMB_LAYER),
        Transform::from_translation(v.camera_position()).with_rotation(v.rotation()),
    ));

    let fill = materials.add(StandardMaterial {
        base_color: Color::srgba_u8(0x8f, 0xab, 0xd0, 0x9c),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let quad = meshes.add(Rectangle::new(PLANE_HALF * 2.0, PLANE_HALF * 2.0));
    for kind in PlaneKind::ALL {
        commands.spawn((
            Name::new("thumbnail-plane"),
            ThumbPlane,
            Mesh3d(quad.clone()),
            MeshMaterial3d(fill.clone()),
            Transform::from_rotation(kind.rotation()),
            RenderLayers::layer(THUMB_LAYER),
        ));
    }
}

fn draw_thumbnail_scene(
    q_cam: Query<&Camera, With<ThumbCamera>>,
    sketches: Res<ThumbnailSketches>,
    mut gizmos: Gizmos<ThumbGizmos>,
) {
    if !q_cam.iter().any(|c| c.is_active) {
        return;
    }
    // With sketches or parts the thumbnail shows only them (zoomed to fit); otherwise the
    // planes.
    for line in &sketches.2 {
        gizmos.linestrip(
            line.iter().map(|p| *p + sketches.1 * 0.5),
            Color::srgb_u8(0x14, 0x14, 0x14),
        );
    }
    if sketches.0.is_empty() && sketches.2.is_empty() && !sketches.3 {
        let edge = Color::srgb_u8(0x3f, 0x5f, 0x8c);
        for k in PlaneKind::ALL {
            let c = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .map(|(a, b)| (k.u() * a + k.v() * b) * PLANE_HALF);
            for i in 0..4 {
                gizmos.line(c[i], c[(i + 1) % 4], edge);
            }
        }
    }
    let lift = sketches.1;
    let sketch_edge = Color::srgb_u8(0x2e, 0x35, 0x3d);
    for (g, plane) in &sketches.0 {
        let frame = plane.frame();
        for (id, c) in &g.curves {
            if c.construction {
                continue;
            }
            let pts = cadrs_sketch::hit::curve_polyline(g, id);
            gizmos.linestrip(
                pts.into_iter().map(|p| {
                    let w = frame.to_world(p);
                    Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32) + lift
                }),
                sketch_edge,
            );
        }
    }
}

/// With more parts than this the thumbnail draws them without their edges.
const EDGES_PART_LIMIT: usize = 200;

/// Sets the thumbnail's scene up for `content` and turns its camera on, pointed at it.
fn prepare_thumbnail_scene(world: &mut World, content: Content) {
    let Content {
        parts,
        props,
        appearances,
        sketches,
        board,
    } = content;
    let mut part_pts: Vec<Vec3> = parts
        .iter()
        .flat_map(|p| {
            p.solid
                .positions
                .iter()
                .map(|q| Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32))
        })
        .collect();
    let board_bodies = match &board {
        Some((e, b, board)) => crate::pcb::view::board_view_bodies(
            world,
            *e,
            *b,
            board,
            &mut std::collections::hash_map::DefaultHasher::new(),
        ),
        None => vec![],
    };
    part_pts.extend(
        board_bodies
            .iter()
            .flat_map(|b| b.positions.iter().map(|p| Vec3::from_array(*p))),
    );
    let v = thumbnail_view_with(&sketches, &part_pts);
    if let Some((e, b, board)) = &board {
        let (opaque, blended) = {
            let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
            (
                mats.add(crate::parts::part_material(false)),
                mats.add(crate::parts::part_material(true)),
            )
        };
        for body in &board_bodies {
            let base = crate::parts::FaceBase {
                rgb: [
                    body.color[0] as f32,
                    body.color[1] as f32,
                    body.color[2] as f32,
                ],
                alpha: body.color[3] as f32 / 255.0,
                share: [0; 2],
            };
            let mesh = world
                .resource_mut::<Assets<Mesh>>()
                .add(crate::pcb::view::body_mesh(body, &v, base, 0));
            let material = if base.alpha < 1.0 {
                blended.clone()
            } else {
                opaque.clone()
            };
            world.spawn((
                Name::new("thumbnail-board"),
                ThumbSketch,
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                RenderLayers::layer(THUMB_LAYER),
            ));
        }
        // A native board's mask, copper, pads and silkscreen.
        let design = crate::eda::design(world.resource::<ActiveDocument>(), *e, *b).cloned();
        for (_, mesh, material) in design
            .map(|d| crate::pcb::view::skins(world, &d, board.thickness()))
            .unwrap_or_default()
        {
            world.spawn((
                Name::new("thumbnail-board-skin"),
                ThumbSketch,
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                RenderLayers::layer(THUMB_LAYER),
            ));
        }
    }
    let part_material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(crate::parts::part_material(false));
    let mut part_lines = Vec::new();
    for part in &parts {
        let bases: Vec<crate::parts::FaceBase> =
            crate::parts::face_bases(part, &props, &appearances)
                .into_iter()
                .map(|b| crate::parts::FaceBase { alpha: 1.0, ..b })
                .collect();
        let mesh = crate::parts::part_mesh(part, &v, false, &bases);
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        world.spawn((
            Name::new("thumbnail-part"),
            ThumbSketch,
            Mesh3d(mesh),
            MeshMaterial3d(part_material.clone()),
            Transform::IDENTITY,
            RenderLayers::layer(THUMB_LAYER),
        ));
        // (Edges of that many parts are finer than the picture's pixels, and dear to work out.)
        if parts.len() <= EDGES_PART_LIMIT {
            part_lines.extend(crate::parts::part_lines(part, &v));
        }
    }
    let fill = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb_u8(0xc4, 0xcb, 0xd2),
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
    for (g, plane) in &sketches {
        let mesh = crate::sketch_draw::fill_mesh(g, *plane);
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        world.spawn((
            Name::new("thumbnail-sketch-fill"),
            ThumbSketch,
            Mesh3d(mesh),
            MeshMaterial3d(fill.clone()),
            // A little toward the camera, so the translucent planes do not fight with it.
            Transform::from_translation(v.back() * 0.5),
            RenderLayers::layer(THUMB_LAYER),
        ));
    }
    let mut q =
        world.query_filtered::<(&mut Camera, &mut Transform, &mut Projection), With<ThumbCamera>>();
    for (mut c, mut t, mut p) in q.iter_mut(world) {
        c.is_active = true;
        *t = Transform::from_translation(v.camera_position()).with_rotation(v.rotation());
        if let Projection::Orthographic(o) = &mut *p {
            o.scale = v.scale / SS as f32;
            o.far = crate::camera::camera_far(v.scale);
        }
    }
    let show_planes = sketches.is_empty() && parts.is_empty() && board.is_none();
    let mut q = world.query_filtered::<&mut Visibility, With<ThumbPlane>>();
    for mut v in q.iter_mut(world) {
        *v = if show_planes {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    *world.resource_mut::<ThumbnailSketches>() =
        ThumbnailSketches(sketches, v.back() * 1.0, part_lines, board.is_some());
}

fn clear_thumbnail_scene(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<ThumbSketch>>();
    let fills: Vec<Entity> = q.iter(world).collect();
    for e in fills {
        world.entity_mut(e).despawn();
    }
    world.resource_mut::<ThumbnailSketches>().0.clear();
    world.resource_mut::<ThumbnailSketches>().2.clear();
    world.resource_mut::<ThumbnailSketches>().3 = false;
    let mut q = world.query_filtered::<&mut Camera, With<ThumbCamera>>();
    for mut c in q.iter_mut(world) {
        c.is_active = false;
    }
}

/// What to draw if the open document's saved thumbnail is not current; `None` when it is, or
/// when the document has none to save (a scratch document, one open read-only at a version).
fn stale(world: &World) -> Option<Drawn> {
    let d = world.get_resource::<ActiveDocument>()?;
    let meta = d.meta.as_ref()?;
    if d.read_only.is_some() {
        return None;
    }
    let now = drawn(&d.doc);
    match &world.resource::<LiveThumbnail>().saved {
        // This session drew it: current while the document's is what it was drawn from.
        Some((id, saved)) if *id == d.doc.id => (*saved != now).then_some(now),
        // Else a stored document gets a new thumbnail only if it is new, changed while open, or
        // has none or one older than its last edit (a document.ron copied from another machine
        // without its thumbnail.png), so opening and closing a document keeps the picture it
        // had. A document whose picture is its board always gets a new one, so thumbnails saved
        // before boards were drawn (the bare planes) are replaced.
        _ => {
            let store = &world.resource::<DocumentStore>().0;
            (d.fresh
                || d.changed_since_open()
                || store.thumbnail_outdated(d.doc.id, meta.modified)
                || matches!(now, Drawn::Board(_)))
            .then_some(now)
        }
    }
}

/// Starts drawing `drawn`'s thumbnail of the open document.
fn start(world: &mut World, drawn: Drawn, content: Content, then: Vec<Then>) {
    let Some(doc) = world.get_resource::<ActiveDocument>().map(|d| d.doc.id) else {
        for t in then {
            t(world);
        }
        return;
    };
    prepare_thumbnail_scene(world, content);
    world.insert_resource(Capture {
        doc,
        drawn,
        frames: 0,
        started: web_time::Instant::now(),
        requested: false,
        state: Arc::new(AtomicU8::new(CAPTURING)),
        then,
    });
}

/// Makes the open document's saved thumbnail current (drawn and saved again if it is not),
/// then runs `then`: at once when it is current or the document has none to save. Whatever
/// hands the thumbnail on goes through here.
pub fn then(world: &mut World, then: impl FnOnce(&mut World) + Send + Sync + 'static) {
    let mut waiting: Vec<Then> = Vec::new();
    // One being drawn: wait for it, unless the document changed under it.
    if let Some(mut capture) = world.remove_resource::<Capture>() {
        let current = world
            .get_resource::<ActiveDocument>()
            .is_none_or(|d| d.doc.id != capture.doc || drawn(&d.doc) == capture.drawn);
        if current {
            capture.then.push(Box::new(then));
            world.insert_resource(capture);
            return;
        }
        waiting = std::mem::take(&mut capture.then);
        clear_thumbnail_scene(world);
    }
    waiting.push(Box::new(then));
    let content = stale(world).and_then(|d| Some((content(world, &d, true)?, d)));
    match content {
        Some((content, drawn)) => start(world, drawn, content, waiting),
        None => {
            for t in waiting {
                t(world);
            }
        }
    }
}

/// Closes the document: back to the documents page, its thumbnail current first.
pub fn close_document(world: &mut World) {
    then(world, |world| {
        world
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Landing);
    });
}

/// Closing the window waits for the thumbnail (the harness leaves the closing to us).
fn close_window(mut requests: MessageReader<WindowCloseRequested>, mut commands: Commands) {
    for r in requests.read() {
        let window = r.window;
        commands.queue(move |world: &mut World| {
            then(world, move |world| {
                if let Ok(w) = world.get_entity_mut(window) {
                    w.despawn();
                }
            });
        });
    }
}

/// How long the view is still before the open document's thumbnail is drawn again.
const IDLE: f32 = 1.0;

/// Watches the open document for when its thumbnail is worth drawing again: it changed, and
/// the view has been still for [`IDLE`]. Then [`refresh`] decides whether the picture differs.
#[allow(clippy::too_many_arguments)]
fn watch(
    doc: Option<Res<ActiveDocument>>,
    store: Res<DocumentStore>,
    view: Res<ViewportView>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    cache: Res<crate::parts::PartCache>,
    capture: Option<Res<Capture>>,
    time: Res<Time>,
    mut live: ResMut<LiveThumbnail>,
    mut commands: Commands,
) {
    let Some(doc) = doc else { return };
    let Some(meta) = doc.meta.as_ref().filter(|_| doc.read_only.is_none()) else {
        return;
    };
    let live = live.bypass_change_detection();
    let revision = doc.history.revision();
    if live.doc != Some(doc.doc.id) {
        // Just opened: a new document, or one whose thumbnail is older than its last edit,
        // gets one without waiting for a change.
        live.doc = Some(doc.doc.id);
        live.revision = revision;
        live.dirty = doc.fresh || store.0.thumbnail_outdated(doc.doc.id, meta.modified);
        live.idle = 0.0;
        live.want = None;
        live.build = None;
    }
    if live.revision != revision {
        live.revision = revision;
        live.dirty = true;
        live.idle = 0.0;
        live.want = None;
        live.build = None;
    }
    let busy = live.view != Some(view.view)
        || mouse.get_pressed().next().is_some()
        || touches.iter().next().is_some()
        || cache.rebuilding;
    live.view = Some(view.view);
    if busy {
        live.idle = 0.0;
        return;
    }
    live.idle += time.delta_secs();
    if live.dirty && live.idle >= IDLE && capture.is_none() {
        commands.queue(refresh);
    }
}

/// Draws the open document's thumbnail again if it shows something else than the saved one
/// was drawn from, once the builds it needs are there.
fn refresh(world: &mut World) {
    if world.contains_resource::<Capture>() {
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    if doc.meta.is_none() || doc.read_only.is_some() {
        return;
    }
    let (id, active) = (doc.doc.id, doc.active);
    // What it would be drawn from, worked out once per change.
    let want = match world.resource_mut::<LiveThumbnail>().want.take() {
        Some(w) => w,
        None => drawn(&world.resource::<ActiveDocument>().doc),
    };
    let saved = world
        .resource::<LiveThumbnail>()
        .saved
        .as_ref()
        .is_some_and(|(d, s)| *d == id && *s == want);
    // A page builds on its one thread, so there only a tab that is open (and so built) is
    // drawn; another tab's waits for the document to close.
    let page = cfg!(target_arch = "wasm32");
    if saved || (page && want.element().is_some_and(|e| Some(e) != active)) {
        world.resource_mut::<LiveThumbnail>().dirty = false;
        return;
    }
    match content(world, &want, page) {
        Some(content) => {
            world.resource_mut::<LiveThumbnail>().dirty = false;
            start(world, want, content, Vec::new());
        }
        // Its builds still run: again next frame.
        None => world.resource_mut::<LiveThumbnail>().want = Some(want),
    }
}

/// Frames to wait for the thumbnail before going on without it, and the seconds (a window
/// that isn't shown draws few frames).
const CAPTURE_TIMEOUT: u32 = 120;
const CAPTURE_TIMEOUT_SECS: f32 = 3.0;

fn drive_capture(
    mut commands: Commands,
    capture: Option<ResMut<Capture>>,
    target: Res<ThumbnailTarget>,
    store: Res<DocumentStore>,
    mut live: ResMut<LiveThumbnail>,
    pending: Option<ResMut<cadrs_ui::PendingWork>>,
) {
    let Some(mut capture) = capture else {
        return;
    };
    // Something waits for it (a close, an export): the app is busy until it has run.
    if let Some(mut pending) = pending.filter(|p| !capture.then.is_empty() && !p.0) {
        pending.0 = true;
    }
    capture.frames += 1;
    let state = capture.state.load(Ordering::SeqCst);
    if state != CAPTURING
        || capture.frames > CAPTURE_TIMEOUT
        || capture.started.elapsed().as_secs_f32() > CAPTURE_TIMEOUT_SECS
    {
        match state {
            SAVED => live.saved = Some((capture.doc, capture.drawn.clone())),
            FAILED => {}
            _ => warn!("thumbnail capture timed out"),
        }
        let then = std::mem::take(&mut capture.then);
        commands.remove_resource::<Capture>();
        commands.queue(move |world: &mut World| {
            clear_thumbnail_scene(world);
            for t in then {
                t(world);
            }
        });
        return;
    }
    // Let the camera render a couple of frames first.
    if capture.frames < 3 || capture.requested {
        return;
    }
    capture.requested = true;
    let id = capture.doc;
    let store = store.0.clone();
    let state = capture.state.clone();
    commands
        .spawn(Screenshot(RenderTarget::Image(target.0.clone().into())))
        .observe(move |shot: On<ScreenshotCaptured>| {
            let saved = match shot.image.clone().try_into_dynamic() {
                Ok(img) => {
                    let img = finish_thumbnail(img.to_rgba8());
                    store
                        .write_thumbnail(id, &img)
                        .inspect_err(|e| error!("cannot save the thumbnail: {e}"))
                        .is_ok()
                }
                Err(e) => {
                    error!("cannot read the thumbnail back: {e:?}");
                    false
                }
            };
            state.store(if saved { SAVED } else { FAILED }, Ordering::SeqCst);
        });
}

/// Turns the rendered image (premultiplied by blending over a transparent background) into a
/// straight-alpha thumbnail at the saved size.
pub fn finish_thumbnail(mut img: RgbaImage) -> RgbaImage {
    let to_lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let to_srgb = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    for p in img.pixels_mut() {
        let a = p[3] as f32 / 255.0;
        if a > 0.0 {
            for i in 0..3 {
                p[i] = to_srgb(to_lin(p[i]) / a);
            }
        }
    }
    image::imageops::resize(
        &img,
        THUMB_LARGE_W,
        THUMB_LARGE_H,
        image::imageops::FilterType::Triangle,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_view_fits_the_planes() {
        let v = thumbnail_view();
        let mut hi = Vec2::ZERO;
        for k in PlaneKind::ALL {
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let s = v.project((k.u() * a + k.v() * b) * PLANE_HALF);
                hi = hi.max(s.abs());
            }
        }
        assert!(
            hi.x <= THUMB_LARGE_W as f32 / 2.0 + 0.01 && hi.y <= THUMB_LARGE_H as f32 / 2.0 + 0.01
        );
        // It fills one dimension.
        assert!(
            hi.x > THUMB_LARGE_W as f32 / 2.0 - 16.0 || hi.y > THUMB_LARGE_H as f32 / 2.0 - 16.0
        );
    }

    #[test]
    fn thumbnails_zoom_to_their_sketches() {
        let mut g = cadrs_sketch::Sketch::new();
        cadrs_sketch::SketchOp::AddCircle {
            center: cadrs_sketch::Vec2::new(40.0, 20.0),
            radius: 5.0,
            construction: false,
        }
        .apply(&mut g)
        .unwrap();
        let v = thumbnail_view_for(&[(g, cadrs_sketch::PlaneRef::Top)]);
        // Centered on the circle, which fills most of the image.
        assert!(v.project(Vec3::new(40.0, 20.0, 0.0)).length() < 0.5);
        let d = v.project_vector(Vec3::X * 10.0).length();
        assert!(d > 30.0, "{d}");
        // No sketches: the planes' view.
        assert!(thumbnail_view_for(&[]).approx_eq(&thumbnail_view()));
    }

    /// A sketch of a circle on the Top plane.
    fn circle_sketch() -> Feature {
        let mut s = cadrs_core::SketchFeature::new(Some(cadrs_sketch::PlaneRef::Top));
        cadrs_sketch::SketchOp::AddCircle {
            center: cadrs_sketch::Vec2::new(0.0, 0.0),
            radius: 5.0,
            construction: false,
        }
        .apply(&mut s.geometry)
        .unwrap();
        Feature::new(
            cadrs_core::FeatureId::new(),
            "Sketch 1",
            cadrs_core::FeatureKind::Sketch(s),
        )
    }

    #[test]
    fn the_first_tab_with_something_to_show_is_drawn() {
        use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose};
        // "Part Studio 1" and "Assembly 1", both empty: the planes.
        let mut doc = cadrs_core::Document::new("Thumb");
        let (studio, assembly) = (doc.elements[0].id, doc.elements[1].id);
        assert!(drawn(&doc) == Drawn::Planes);
        // An instance in the assembly while the studio before it shows nothing: the assembly.
        doc.elements[1].assembly.instances.push(Instance::new(
            InstanceId::new(),
            InstanceSource::Part {
                element: studio,
                part: cadrs_core::PartId::new(cadrs_core::FeatureId::new(), 0),
            },
            Pose::default(),
        ));
        assert!(drawn(&doc).element() == Some(assembly));
        // A sketch in the studio: the studio, as it comes first.
        doc.elements[0]
            .features_mut()
            .unwrap()
            .push(circle_sketch());
        let of_studio = drawn(&doc);
        assert!(of_studio.element() == Some(studio));
        // A change to a tab the thumbnail doesn't show is no reason to draw it again...
        doc.elements[1].name = "Renamed".into();
        doc.elements[1].assembly.instances[0].hidden = true;
        assert!(drawn(&doc) == of_studio);
        // ...a change to the one it shows is,
        doc.elements[0]
            .features_mut()
            .unwrap()
            .push(circle_sketch());
        assert!(drawn(&doc) != of_studio);
        // and so is another tab now being the first.
        doc.elements.swap(0, 1);
        assert!(drawn(&doc).element() == Some(assembly));
    }

    #[test]
    fn finishing_unpremultiplies_and_downscales() {
        let mut img = RgbaImage::new(THUMB_LARGE_W * SS, THUMB_LARGE_H * SS);
        for p in img.pixels_mut() {
            // 50% coverage of pure white, premultiplied: linear 0.5 is sRGB 188.
            *p = image::Rgba([188, 188, 188, 128]);
        }
        let out = finish_thumbnail(img);
        assert_eq!(out.dimensions(), (THUMB_LARGE_W, THUMB_LARGE_H));
        let p = out.get_pixel(10, 10);
        assert!(p[0] >= 253 && p[3] == 128, "{p:?}");
    }

    #[test]
    fn thumbnail_saves_and_loads() {
        let dir = std::env::temp_dir().join(format!("cadrs-thumb-{}", std::process::id()));
        let store = cadrs_core::Store::new(&dir);
        let doc = cadrs_core::Document::new("Thumb");
        let meta = cadrs_core::DocumentMeta::new("me", 0);
        store.create(&doc, &meta).unwrap();
        let mut img = RgbaImage::new(THUMB_LARGE_W * SS, THUMB_LARGE_H * SS);
        for (x, y) in (80..120).flat_map(|x| (40..80).map(move |y| (x, y))) {
            img.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
        }
        store
            .write_thumbnail(doc.id, &finish_thumbnail(img))
            .unwrap();
        // The lists' small copy, and the details panel's large one.
        let back = store.read_thumbnail(doc.id).unwrap();
        assert_eq!(
            back.dimensions(),
            (
                cadrs_core::thumbnail::THUMB_W,
                cadrs_core::thumbnail::THUMB_H
            )
        );
        assert!(back.get_pixel(11, 7)[0] > 0);
        let large = store.read_thumbnail_large(doc.id).unwrap();
        assert_eq!(large.dimensions(), (THUMB_LARGE_W, THUMB_LARGE_H));
        assert!(large.get_pixel(50, 30)[0] > 0);
        let _ = cadrs_fs::remove_dir_all(dir);
    }
}
