//! Real document thumbnails: an offscreen camera renders the first Part Studio in the isometric
//! view into an image. When the document closes, the image is read back and saved as the
//! document's `thumbnail.png`, which the documents page then shows.
//!
//! The thumbnail scene lives on its own render layer, so it does not depend on which tab is
//! active or on hover highlights. It shows the default planes (drawn a little stronger than in
//! the viewport so they read at 60×34 px), the first Part Studio's parts (shaded as in the
//! viewport, with black edges) and its sketches that no extrude used: closed regions filled grey
//! with dark edges. With geometry, the view zooms to fit it, so every document's thumbnail shows
//! its own.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use cadrs_core::thumbnail::{THUMB_H, THUMB_W};
use image::RgbaImage;

use crate::camera::{StandardView, ViewState};
use crate::viewport::{PLANE_HALF, PlaneKind};
use crate::{ActiveDocument, AppState, DocumentStore};

pub struct ThumbnailPlugin;

impl Plugin for ThumbnailPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<ThumbGizmos>()
            .init_resource::<ThumbnailSketches>()
            .add_systems(Startup, setup_thumbnail)
            .add_systems(Update, (draw_thumbnail_scene, drive_close));
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
);

/// The parts of a document's first Part Studio.
fn document_parts(doc: &cadrs_core::Document) -> Vec<cadrs_core::Part> {
    doc.elements
        .iter()
        .find(|e| matches!(e.kind, cadrs_core::ElementKind::PartStudio { .. }))
        .map(|e| cadrs_core::parts::parts(&e.active_features()))
        .unwrap_or_default()
}

/// The sketches of a document's first Part Studio, with their planes.
fn document_sketches(
    doc: &cadrs_core::Document,
) -> Vec<(cadrs_sketch::Sketch, cadrs_sketch::PlaneRef)> {
    doc.elements
        .iter()
        .find(|e| matches!(e.kind, cadrs_core::ElementKind::PartStudio { .. }))
        .map(|e| {
            let active = e.active_features();
            let consumed = crate::parts::consumed_sketches(&active, None);
            active
                .iter()
                .filter(|f| !consumed.contains(&f.id))
                .filter_map(|f| {
                    let s = f.sketch()?;
                    Some((s.geometry.clone(), s.plane?))
                })
                .filter(|(g, _)| !g.curves.is_empty())
                .collect()
        })
        .unwrap_or_default()
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
        Vec2::new(THUMB_W as f32, THUMB_H as f32),
        0.9,
    )
}

/// Closing the document: frames waited so far, and whether the thumbnail was saved.
#[derive(Resource, Debug, Default)]
pub struct PendingClose {
    frames: u32,
    requested: bool,
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
    let margin = 3.0;
    let size = Vec2::new(THUMB_W as f32, THUMB_H as f32) - 2.0 * margin;
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
        THUMB_W * SS,
        THUMB_H * SS,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    image.asset_usage = RenderAssetUsages::default();
    let handle = images.add(image);
    commands.insert_resource(ThumbnailTarget(handle.clone()));

    let (config, _) = store.config_mut::<ThumbGizmos>();
    config.render_layers = RenderLayers::layer(THUMB_LAYER);
    config.line.width = 2.6;

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
            far: crate::camera::CAMERA_FAR,
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
        gizmos.linestrip(line.iter().map(|p| *p + sketches.1 * 0.5), Color::srgb_u8(0x14, 0x14, 0x14));
    }
    if sketches.0.is_empty() && sketches.2.is_empty() {
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

/// Points the thumbnail camera at the document's sketches and adds their region fills.
fn prepare_thumbnail_scene(world: &mut World) {
    let sketches = world
        .get_resource::<ActiveDocument>()
        .map(|d| document_sketches(&d.doc))
        .unwrap_or_default();
    let parts = world
        .get_resource::<ActiveDocument>()
        .map(|d| document_parts(&d.doc))
        .unwrap_or_default();
    // The parts in their appearances (PS9), opaque.
    let (props, appearances) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| {
            let e = d.doc.elements.iter().find(|e| matches!(e.kind, cadrs_core::ElementKind::PartStudio { .. }))?;
            Some((e.part_props().to_vec(), e.feature_appearances().to_vec()))
        })
        .unwrap_or_default();
    let part_pts: Vec<Vec3> = parts
        .iter()
        .flat_map(|p| p.solid.positions.iter().map(|q| Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32)))
        .collect();
    let v = thumbnail_view_with(&sketches, &part_pts);
    let part_material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(crate::parts::part_material(false));
    let mut part_lines = Vec::new();
    for part in &parts {
        let bases: Vec<crate::parts::FaceBase> = crate::parts::face_bases(part, &props, &appearances)
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
        part_lines.extend(crate::parts::part_lines(part, &v));
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
    let mut q = world
        .query_filtered::<(&mut Camera, &mut Transform, &mut Projection), With<ThumbCamera>>();
    for (mut c, mut t, mut p) in q.iter_mut(world) {
        c.is_active = true;
        *t = Transform::from_translation(v.camera_position()).with_rotation(v.rotation());
        if let Projection::Orthographic(o) = &mut *p {
            o.scale = v.scale / SS as f32;
        }
    }
    let show_planes = sketches.is_empty() && parts.is_empty();
    let mut q = world.query_filtered::<&mut Visibility, With<ThumbPlane>>();
    for mut v in q.iter_mut(world) {
        *v = if show_planes {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    *world.resource_mut::<ThumbnailSketches>() =
        ThumbnailSketches(sketches, v.back() * 1.0, part_lines);
}

fn clear_thumbnail_scene(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<ThumbSketch>>();
    let fills: Vec<Entity> = q.iter(world).collect();
    for e in fills {
        world.entity_mut(e).despawn();
    }
    world.resource_mut::<ThumbnailSketches>().0.clear();
    world.resource_mut::<ThumbnailSketches>().2.clear();
    let mut q = world.query_filtered::<&mut Camera, With<ThumbCamera>>();
    for mut c in q.iter_mut(world) {
        c.is_active = false;
    }
}

/// Starts closing the document: renders and saves the thumbnail, then shows the documents
/// page. Scratch documents (not stored) and unchanged documents close right away.
pub fn close_document(world: &mut World) {
    if world.contains_resource::<PendingClose>() {
        return;
    }
    // A stored document gets a new thumbnail only if it is new, changed while open, or has
    // none or one older than its last edit (a document.ron copied from another machine without
    // its thumbnail.png, or the app quit while it was open), so opening and closing a document
    // keeps the picture it had.
    let store = world.resource::<DocumentStore>().0.clone();
    let rerender = world.get_resource::<ActiveDocument>().is_some_and(|d| {
        d.meta.as_ref().is_some_and(|m| {
            d.fresh || d.changed_since_open() || store.thumbnail_outdated(d.doc.id, m.modified)
        })
    });
    if !rerender {
        world
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Landing);
        return;
    }
    prepare_thumbnail_scene(world);
    world.insert_resource(PendingClose::default());
}

/// Frames to wait for the thumbnail before closing anyway.
const CLOSE_TIMEOUT: u32 = 120;

fn drive_close(
    mut commands: Commands,
    pending: Option<ResMut<PendingClose>>,
    target: Res<ThumbnailTarget>,
    doc: Option<Res<ActiveDocument>>,
    store: Res<DocumentStore>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(mut pending) = pending else {
        return;
    };
    pending.frames += 1;
    let done = pending.done.load(std::sync::atomic::Ordering::SeqCst);
    if done || pending.frames > CLOSE_TIMEOUT {
        if !done {
            warn!("thumbnail capture timed out");
        }
        commands.queue(clear_thumbnail_scene);
        commands.remove_resource::<PendingClose>();
        next.set(AppState::Landing);
        return;
    }
    // Let the camera render a couple of frames first.
    if pending.frames < 3 || pending.requested {
        return;
    }
    pending.requested = true;
    let Some(id) = doc.as_ref().map(|d| d.doc.id) else {
        pending.done.store(true, std::sync::atomic::Ordering::SeqCst);
        return;
    };
    let store = store.0.clone();
    let flag = pending.done.clone();
    commands
        .spawn(Screenshot(RenderTarget::Image(target.0.clone().into())))
        .observe(move |shot: On<ScreenshotCaptured>| {
            match shot.image.clone().try_into_dynamic() {
                Ok(img) => {
                    let img = finish_thumbnail(img.to_rgba8());
                    if let Err(e) = store.write_thumbnail(id, &img) {
                        error!("cannot save the thumbnail: {e}");
                    }
                }
                Err(e) => error!("cannot read the thumbnail back: {e:?}"),
            }
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
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
    image::imageops::resize(&img, THUMB_W, THUMB_H, image::imageops::FilterType::Triangle)
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
        assert!(hi.x <= THUMB_W as f32 / 2.0 + 0.01 && hi.y <= THUMB_H as f32 / 2.0 + 0.01);
        // It fills one dimension.
        assert!(hi.x > THUMB_W as f32 / 2.0 - 4.0 || hi.y > THUMB_H as f32 / 2.0 - 4.0);
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

    #[test]
    fn finishing_unpremultiplies_and_downscales() {
        let mut img = RgbaImage::new(THUMB_W * SS, THUMB_H * SS);
        for p in img.pixels_mut() {
            // 50% coverage of pure white, premultiplied: linear 0.5 is sRGB 188.
            *p = image::Rgba([188, 188, 188, 128]);
        }
        let out = finish_thumbnail(img);
        assert_eq!(out.dimensions(), (THUMB_W, THUMB_H));
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
        let mut img = RgbaImage::new(THUMB_W * SS, THUMB_H * SS);
        img.put_pixel(100, 60, image::Rgba([255, 0, 0, 255]));
        store.write_thumbnail(doc.id, &finish_thumbnail(img)).unwrap();
        let back = store.read_thumbnail(doc.id).unwrap();
        assert_eq!(back.dimensions(), (THUMB_W, THUMB_H));
        assert!(back.get_pixel(50, 30)[0] > 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
