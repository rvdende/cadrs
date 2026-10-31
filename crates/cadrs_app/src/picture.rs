//! Pictures in a Part Studio ([`cadrs_core::picture`]): dropping PNG, JPEG or GIF files on the
//! window inserts each as an Image feature, a flat surface part showing the picture, which
//! moves and copies like any other part (Transform).
//!
//! - **Drop** ([`crate::drop_import`] routes dropped files): in a Part Studio (not while
//!   sketching), each dropped picture lies on the default plane that faces the view most,
//!   centred where the pointer is on that plane (the view's centre when the window doesn't
//!   know), the next ones of the same drop beside it. Each is one undoable "Insert image".
//! - **Display:** each shown part's pictures ([`cadrs_core::solid::SolidImage`]) are drawn as an
//!   unlit textured quad over the part's face, from both sides. Textures are decoded once per
//!   picture (GIFs show their first frame), at most [`MAX_TEXTURE`] pixels on a side.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use cadrs_core::FeatureId;
use cadrs_core::picture::AddImage;
use cadrs_core::solid::SolidImage;
use cadrs_sketch::PlaneRef;
use cadrs_ui::{Theme, show_toast};

use crate::parts::PartCache;
use crate::viewport::{ViewportRect, ViewportView};
use crate::{ActiveDocument, AppState};

/// The largest texture side (pixels); bigger pictures are scaled down to it.
pub const MAX_TEXTURE: u32 = 2048;

/// The gap between pictures dropped together (mm).
const DROP_GAP: f64 = 10.0;

/// The depth bias that lets a picture win over the face it lies on. The views are orthographic
/// with a depth range of [`crate::camera::CAMERA_FAR`], so one unit of bias is about 2.4 µm
/// there (4.8 µm nearer than the view's centre): this many lift a picture a few hundredths of a
/// millimetre. A large bias (1000 was 2.4 mm) hid whatever stood lower than that over the face,
/// a board's SMD components first, and by how much changed with where the board was in depth.
/// (Bevy also sorts blended meshes by it, in mm: it no longer puts a picture last.)
pub const SURFACE_BIAS: f32 = 8.0;

/// How far a board's skin lies off its face (mm), toward the side it faces.
pub const SKIN_LIFT: f32 = 0.01;

pub struct PicturePlugin;

impl Plugin for PicturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PictureTextures>()
            .add_systems(Update, sync_pictures.run_if(in_state(AppState::Document)));
    }
}

/// The decoded pictures, by blob hash and whether one-sided: their texture and material
/// (`None`: unreadable).
#[derive(Resource, Default)]
pub struct PictureTextures(HashMap<(String, bool), Option<Handle<StandardMaterial>>>);

/// A picture drawn over its part.
#[derive(Component)]
pub struct PictureOverlay;

/// The default plane facing the view most (`back` points to the viewer).
fn facing_plane(back: Vec3) -> PlaneRef {
    [PlaneRef::Top, PlaneRef::Front, PlaneRef::Right]
        .into_iter()
        .max_by(|a, b| {
            let facing = |p: &PlaneRef| {
                let n = p.frame().normal();
                (Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32).dot(back)).abs()
            };
            facing(a).total_cmp(&facing(b))
        })
        .unwrap_or(PlaneRef::Top)
}

/// Where a view ray meets a plane, in the plane's coordinates.
pub fn on_plane(plane: PlaneRef, (origin, dir): (Vec3, Vec3)) -> Option<[f64; 2]> {
    let f = plane.frame();
    let v = |a: [f64; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
    let (o, n) = (v(f.origin), v(f.normal()).normalize());
    let d = dir.dot(n);
    if d.abs() < 1e-6 {
        return None;
    }
    let p = origin + dir * ((o - origin).dot(n) / d) - o;
    Some([
        p.dot(v(f.u).normalize()) as f64,
        p.dot(v(f.v).normalize()) as f64,
    ])
}

/// Where a drop lands in a Part Studio: the default plane facing the view most, and the point
/// on it under `at` (window position; the view's centre when outside the view or unknown).
pub fn drop_spot(world: &World, at: Option<Vec2>) -> (PlaneRef, [f64; 2]) {
    let view = world.resource::<ViewportView>();
    let rect = world.resource::<ViewportRect>();
    let plane = facing_plane(view.view.back());
    let offset = at
        .filter(|c| rect.0.contains(*c))
        .map_or(Vec2::ZERO, |c| rect.offset(c));
    (
        plane,
        on_plane(plane, view.view.ray(offset)).unwrap_or([0.0, 0.0]),
    )
}

/// Pictures dropped on a Part Studio (not sketching; [`crate::drop_import`] routes them here):
/// each an "Insert image" step, the first centred at `at`, the next ones beside it.
pub fn insert_pictures(world: &mut World, paths: Vec<std::path::PathBuf>, at: Option<Vec2>) {
    let Some(element) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element())
        .map(|e| e.id)
    else {
        return;
    };
    let (plane, mut center) = drop_spot(world, at);
    let mut notes = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("picture")
            .to_string();
        let bytes = match cadrs_fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                notes.push(cadrs_ui::i18n::tr_with(
                    "io-cannot-read",
                    &[("name", &name), ("error", &e.to_string())],
                ));
                continue;
            }
        };
        let cmd = AddImage {
            element,
            feature: FeatureId::new(),
            file_name: name.clone(),
            bytes: std::sync::Arc::new(bytes),
            plane,
            center,
        };
        let mut doc = world.resource_mut::<ActiveDocument>();
        match doc.execute(&cmd) {
            // The next picture of this drop goes to the right of this one.
            Ok(()) => {
                let width = doc
                    .doc
                    .element(element)
                    .and_then(|el| el.feature(cmd.feature))
                    .and_then(|f| match &f.kind {
                        cadrs_core::FeatureKind::Image(x) => Some(x.width),
                        _ => None,
                    })
                    .unwrap_or(cadrs_core::picture::DEFAULT_SIZE);
                center[0] += width + DROP_GAP;
            }
            Err(e) => notes.push(cadrs_ui::i18n::tr_with(
                "io-cannot-insert",
                &[("name", &name), ("error", &e.to_string())],
            )),
        }
    }
    if !notes.is_empty() {
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        for n in notes {
            show_toast(&mut commands, &theme, n);
        }
        world.flush();
    }
}

/// The picture's texture as an unlit material, two-sided or (`one_sided`) seen only from the
/// front (`None` if it can't be read).
fn picture_material(
    blob: &str,
    one_sided: bool,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<Handle<StandardMaterial>> {
    let bytes = cadrs_core::blobs::get(blob)?;
    let mut rgba = match image::load_from_memory(&bytes) {
        Ok(i) => i.to_rgba8(),
        Err(e) => {
            warn!("cannot read a picture: {e}");
            return None;
        }
    };
    let (w, h) = rgba.dimensions();
    if w.max(h) > MAX_TEXTURE {
        let k = MAX_TEXTURE as f64 / w.max(h) as f64;
        let (nw, nh) = (
            ((w as f64 * k).round() as u32).max(1),
            ((h as f64 * k).round() as u32).max(1),
        );
        rgba = image::imageops::resize(&rgba, nw, nh, image::imageops::FilterType::Triangle);
    }
    let (w, h) = rgba.dimensions();
    let texture = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    Some(materials.add(StandardMaterial {
        base_color_texture: Some(images.add(texture)),
        unlit: true,
        // A board's skin: only from outside its face, or the other face's would show through
        // the board at an angle.
        cull_mode: one_sided.then_some(bevy::render::render_resource::Face::Back),
        double_sided: !one_sided,
        // A board's skin is drawn opaque, as the PCB Studio's 3D view draws it (its pixels off
        // the outline left out): blended, it was sorted against the board and the parts around,
        // and a translucent board drawn after it hid it. Other pictures' transparent pixels
        // show the part's surface.
        alpha_mode: if one_sided {
            AlphaMode::Mask(0.5)
        } else {
            AlphaMode::Blend
        },
        // Over the part's own face, which it lies on.
        depth_bias: SURFACE_BIAS,
        ..default()
    }))
}

/// The quad a picture covers, its texture upright (row 0 at the `v` end), its front the side
/// it faces ([`SolidImage::facing`]; else `u × v`). A board's skin lies [`SKIN_LIFT`] off its
/// face, toward that side.
fn picture_mesh(p: &SolidImage) -> Mesh {
    let v = |a: [f64; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
    let (mut c, u, w) = (v(p.corner), v(p.u), v(p.v));
    let mut n = u.cross(w).normalize_or_zero();
    // Wound so its front is the side it faces (a board's bottom skin, drawn as the layout has
    // it, faces down).
    let mut indices = vec![0, 1, 2, 0, 2, 3];
    if let Some(f) = p.facing.map(v)
        && f.dot(n) < 0.0
    {
        n = -n;
        indices = vec![0, 2, 1, 0, 3, 2];
    }
    if p.facing.is_some() {
        c += n * SKIN_LIFT;
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![c, c + u, c + u + w, c + w])
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![n; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    )
    .with_inserted_indices(Indices::U32(indices))
}

/// Keeps a textured quad over every shown part's pictures.
#[allow(clippy::too_many_arguments)]
fn sync_pictures(
    cache: Res<PartCache>,
    mut textures: ResMut<PictureTextures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    q: Query<Entity, With<PictureOverlay>>,
    mut last: Local<Option<u64>>,
    mut loading: Local<bool>,
    mut commands: Commands,
) {
    // Nothing new while the parts are as they were and no picture is still loading.
    if !cache.is_changed() && !*loading && last.is_some() {
        return;
    }
    *loading = cache
        .shown()
        .flat_map(|p| p.solid.images.iter())
        .any(|i| !cadrs_core::blobs::contains(&i.blob));
    // A derived board shown as its layout hides its skins. A picture not loaded (yet) is left
    // out, not remembered as unreadable: a board's skin made again has the same name.
    let want: Vec<(&str, &SolidImage)> = cache
        .shown()
        .filter(|p| !cache.skinless.contains(&p.id))
        .flat_map(|p| p.solid.images.iter().map(move |i| (p.name.as_str(), i)))
        .filter(|(_, i)| cadrs_core::blobs::contains(&i.blob))
        .collect();
    if *last == Some(cache.generation) && q.iter().count() == want.len() {
        return;
    }
    *last = Some(cache.generation);
    for e in &q {
        commands.entity(e).try_despawn();
    }
    for (name, pic) in want {
        let one_sided = pic.facing.is_some();
        let material = textures
            .0
            .entry((pic.blob.clone(), one_sided))
            .or_insert_with(|| picture_material(&pic.blob, one_sided, &mut images, &mut materials))
            .clone();
        let Some(material) = material else { continue };
        commands.spawn((
            Name::new(format!("picture-{}", name.to_lowercase().replace(' ', "-"))),
            PictureOverlay,
            Mesh3d(meshes.add(picture_mesh(pic))),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            DespawnOnExit(AppState::Document),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skin_lies_just_off_its_face_toward_its_side() {
        let corners = |m: &Mesh| match m.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) => p.clone(),
            _ => panic!(),
        };
        // A board's bottom skin at z 0, facing down: just under it.
        let pic = SolidImage {
            blob: String::new(),
            corner: [0.0, 0.0, 0.0],
            u: [10.0, 0.0, 0.0],
            v: [0.0, 5.0, 0.0],
            facing: Some([0.0, 0.0, -1.0]),
        };
        assert!(
            corners(&picture_mesh(&pic))
                .iter()
                .all(|p| (p[2] + SKIN_LIFT).abs() < 1e-6)
        );
        // A picture seen from both sides stays on its face.
        let pic = SolidImage {
            facing: None,
            ..pic
        };
        assert!(corners(&picture_mesh(&pic)).iter().all(|p| p[2] == 0.0));
    }

    #[test]
    fn pictures_go_on_the_plane_facing_the_view() {
        assert_eq!(facing_plane(Vec3::new(0.1, -0.2, 0.9)), PlaneRef::Top);
        assert_eq!(facing_plane(Vec3::new(0.1, -0.9, 0.2)), PlaneRef::Front);
        assert_eq!(facing_plane(Vec3::new(-0.9, 0.1, 0.2)), PlaneRef::Right);
    }

    #[test]
    fn a_ray_meets_the_plane_in_its_coordinates() {
        // Straight down onto Top at (12, -3).
        let at = on_plane(PlaneRef::Top, (Vec3::new(12.0, -3.0, 500.0), Vec3::NEG_Z)).unwrap();
        assert!((at[0] - 12.0).abs() < 1e-4 && (at[1] + 3.0).abs() < 1e-4);
        // Front: u = X, v = Z.
        let at = on_plane(PlaneRef::Front, (Vec3::new(5.0, -500.0, 7.0), Vec3::Y)).unwrap();
        assert!((at[0] - 5.0).abs() < 1e-4 && (at[1] - 7.0).abs() < 1e-4);
        // Along the plane: nowhere.
        assert!(on_plane(PlaneRef::Top, (Vec3::ZERO, Vec3::X)).is_none());
    }
}
