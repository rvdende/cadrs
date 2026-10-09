//! Pictures in a Part Studio ([`cadrs_core::picture`]): dropping PNG, JPEG or GIF files on the
//! window inserts each as an Image feature, a flat surface part showing the picture, which
//! moves and copies like any other part (Transform).
//!
//! - **Drop:** in a Part Studio (not while sketching), each dropped picture lies on the default
//!   plane that faces the view most, centred where the pointer is on that plane (the view's
//!   centre when the window doesn't know), the next ones of the same drop beside it. Each is one
//!   undoable "Insert image". Other files are left alone, with a note.
//! - **Display:** each shown part's pictures ([`cadrs_core::solid::SolidImage`]) are drawn as an
//!   unlit textured quad over the part's face, from both sides. Textures are decoded once per
//!   picture (GIFs show their first frame), at most [`MAX_TEXTURE`] pixels on a side.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{FileDragAndDrop, PrimaryWindow};
use cadrs_core::FeatureId;
use cadrs_core::document::ElementKind;
use cadrs_core::picture::{AddImage, ImageFormat};
use cadrs_core::solid::SolidImage;
use cadrs_sketch::PlaneRef;
use cadrs_ui::{Theme, show_toast};

use crate::parts::PartCache;
use crate::sketch::PartStudioMode;
use crate::viewport::{ViewportRect, ViewportView};
use crate::{ActiveDocument, AppState};

/// The largest texture side (pixels); bigger pictures are scaled down to it.
pub const MAX_TEXTURE: u32 = 2048;

/// The gap between pictures dropped together (mm).
const DROP_GAP: f64 = 10.0;

pub struct PicturePlugin;

impl Plugin for PicturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PictureTextures>()
            .add_systems(Update, drop_pictures.run_if(in_state(AppState::Document)))
            .add_systems(Update, sync_pictures.run_if(in_state(AppState::Document)));
    }
}

/// The decoded pictures, by blob hash: their texture and material (`None`: unreadable).
#[derive(Resource, Default)]
pub struct PictureTextures(HashMap<String, Option<Handle<StandardMaterial>>>);

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
fn on_plane(plane: PlaneRef, (origin, dir): (Vec3, Vec3)) -> Option<[f64; 2]> {
    let f = plane.frame();
    let v = |a: [f64; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
    let (o, n) = (v(f.origin), v(f.normal()).normalize());
    let d = dir.dot(n);
    if d.abs() < 1e-6 {
        return None;
    }
    let p = origin + dir * ((o - origin).dot(n) / d) - o;
    Some([p.dot(v(f.u).normalize()) as f64, p.dot(v(f.v).normalize()) as f64])
}

#[allow(clippy::too_many_arguments)]
fn drop_pictures(
    mut drops: MessageReader<FileDragAndDrop>,
    mut doc: Option<ResMut<ActiveDocument>>,
    mode: Option<Res<State<PartStudioMode>>>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut dropped_at: ResMut<crate::file_drop::DropPosition>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let paths: Vec<std::path::PathBuf> = drops
        .read()
        .filter_map(|d| match d {
            FileDragAndDrop::DroppedFile { path_buf, .. } => Some(path_buf.clone()),
            _ => None,
        })
        .collect();
    if paths.is_empty() {
        return;
    }
    let Some(doc) = doc.as_deref_mut() else { return };
    let Some(element) = doc.active_element().filter(|e| matches!(e.kind, ElementKind::PartStudio { .. })).map(|e| e.id) else {
        show_toast(&mut commands, &theme, "Drop pictures into a Part Studio");
        return;
    };
    if mode.is_some_and(|m| *m.get() == PartStudioMode::Sketching) {
        show_toast(&mut commands, &theme, "Close the sketch to drop pictures into the Part Studio");
        return;
    }
    let plane = facing_plane(view.view.back());
    // Where the drag last was (Wayland says; the window's cursor doesn't follow a drag there),
    // else the cursor.
    let at = dropped_at.0.take().or_else(|| windows.single().ok().and_then(|w| w.cursor_position()));
    let cursor = at.filter(|c| rect.0.contains(*c));
    let offset = cursor.map_or(Vec2::ZERO, |c| rect.offset(c));
    let mut center = on_plane(plane, view.view.ray(offset)).unwrap_or([0.0, 0.0]);
    for path in paths {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("picture").to_string();
        if ImageFormat::of_path(&path).is_none() {
            show_toast(&mut commands, &theme, format!("{name}: only PNG, JPEG and GIF pictures can be dropped here"));
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                show_toast(&mut commands, &theme, format!("Cannot read {name}: {e}"));
                continue;
            }
        };
        let cmd = AddImage { element, feature: FeatureId::new(), file_name: name.clone(), bytes: std::sync::Arc::new(bytes), plane, center };
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
            Err(e) => {
                show_toast(&mut commands, &theme, format!("Cannot insert {name}: {e}"));
            }
        }
    }
}

/// The picture's texture as an unlit, two-sided material (`None` if it can't be read).
fn picture_material(
    blob: &str,
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
        let (nw, nh) = (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1));
        rgba = image::imageops::resize(&rgba, nw, nh, image::imageops::FilterType::Triangle);
    }
    let (w, h) = rgba.dimensions();
    let texture = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        rgba.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    Some(materials.add(StandardMaterial {
        base_color_texture: Some(images.add(texture)),
        unlit: true,
        cull_mode: None,
        double_sided: true,
        // Transparent pixels show the part's surface.
        alpha_mode: AlphaMode::Blend,
        // Over the part's own face, which it lies on.
        depth_bias: 1000.0,
        ..default()
    }))
}

/// The quad a picture covers, its texture upright (row 0 at the `v` end).
fn picture_mesh(p: &SolidImage) -> Mesh {
    let v = |a: [f64; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
    let (c, u, w) = (v(p.corner), v(p.u), v(p.v));
    let n = u.cross(w).normalize_or_zero();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![c, c + u, c + u + w, c + w])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![n; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
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
    mut commands: Commands,
) {
    let want: Vec<(&str, &SolidImage)> = cache.shown().flat_map(|p| p.solid.images.iter().map(move |i| (p.name.as_str(), i))).collect();
    if *last == Some(cache.generation) && q.iter().count() == want.len() {
        return;
    }
    *last = Some(cache.generation);
    for e in &q {
        commands.entity(e).try_despawn();
    }
    for (name, pic) in want {
        let material = textures
            .0
            .entry(pic.blob.clone())
            .or_insert_with(|| picture_material(&pic.blob, &mut images, &mut materials))
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
