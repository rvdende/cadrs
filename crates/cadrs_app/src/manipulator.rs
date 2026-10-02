//! The 3D drag arrow (P3E.3b): Onshape's blue manipulator arrow, a shaft with a solid cone
//! head, lit by the head light like the parts (`crate::part_shading`), drawn over the model
//! (the overlay layer) at a fixed length on screen. The Section view's offset arrow and the
//! Draft analysis's pull direction use it; a hovered or dragged arrow turns orange.
//!
//! Spawn [`arrow`] with an [`Arrow3d`] (its foot, direction and length in logical px); the
//! plugin places and recolours it every frame from the view.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::part_shading::PartShading;
use crate::viewport::ViewportView;

pub struct ManipulatorPlugin;

impl Plugin for ManipulatorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_assets).add_systems(PostUpdate, place_arrows.before(bevy::transform::TransformSystems::Propagate));
    }
}

/// An arrow manipulator: its foot (world), unit direction, length on screen (logical px) and
/// whether it is hot (hovered or dragged).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Arrow3d {
    pub base: Vec3,
    pub dir: Vec3,
    pub length_px: f32,
    pub hot: bool,
}

/// Onshape's manipulator blue, and the hot orange.
pub const ARROW_BLUE: [u8; 3] = [0x2a, 0x6c, 0xd6];
pub const ARROW_HOT: [u8; 3] = [0xff, 0x9a, 0x2e];

#[derive(Resource)]
struct ArrowAssets {
    normal: Handle<Mesh>,
    hot: Handle<Mesh>,
    material: Handle<PartShading>,
}

/// The arrow's entity: spawn it with its [`Arrow3d`].
pub fn arrow(name: impl Into<String>, a: Arrow3d) -> impl Bundle {
    (Name::new(name.into()), a, Transform::default(), Visibility::Hidden, RenderLayers::layer(crate::viewport::OVERLAY_LAYER))
}

/// The arrow's foot and tip on screen (offsets from the viewport centre) for a view.
pub fn screen_span(a: &Arrow3d, view: &crate::camera::ViewState) -> (Vec2, Vec2) {
    let tip = a.base + a.dir.normalize_or_zero() * a.length_px * view.scale;
    (view.project(a.base), view.project(tip))
}

fn make_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<PartShading>>) {
    // `cap`: not cut by a section view, not coloured by an analysis.
    let material = materials.add(PartShading { cap: true, ..PartShading::new(false, false) });
    commands.insert_resource(ArrowAssets { normal: meshes.add(arrow_mesh(ARROW_BLUE)), hot: meshes.add(arrow_mesh(ARROW_HOT)), material });
}

fn place_arrows(view: Res<ViewportView>, assets: Option<Res<ArrowAssets>>, mut q: Query<(Entity, &Arrow3d, &mut Transform, &mut Visibility, Option<&Mesh3d>)>, mut commands: Commands) {
    let Some(assets) = assets else { return };
    let v = view.view;
    for (e, a, mut t, mut vis, mesh) in &mut q {
        let dir = a.dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        let want = Transform { translation: a.base, rotation: Quat::from_rotation_arc(Vec3::Y, dir), scale: Vec3::splat(a.length_px * v.scale) };
        if *t != want {
            *t = want;
        }
        vis.set_if_neq(Visibility::Inherited);
        let handle = if a.hot { &assets.hot } else { &assets.normal };
        if mesh.map(|m| &m.0) != Some(handle) {
            commands.entity(e).insert((Mesh3d(handle.clone()), MeshMaterial3d(assets.material.clone())));
        }
    }
}

/// A unit arrow along +Y from the origin: a shaft to 0.66 and a cone to 1, its base a disc.
fn arrow_mesh(rgb: [u8; 3]) -> Mesh {
    const N: usize = 24;
    let (shaft_r, head_r, neck) = (0.035, 0.11, 0.66);
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut nor: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let ring = |k: usize| {
        let t = std::f32::consts::TAU * k as f32 / N as f32;
        (t.cos(), t.sin())
    };
    // The shaft's side.
    for k in 0..N {
        let ((c0, s0), (c1, s1)) = (ring(k), ring(k + 1));
        let i = pos.len() as u32;
        pos.extend([[c0 * shaft_r, 0.0, s0 * shaft_r], [c1 * shaft_r, 0.0, s1 * shaft_r], [c1 * shaft_r, neck, s1 * shaft_r], [c0 * shaft_r, neck, s0 * shaft_r]]);
        nor.extend([[c0, 0.0, s0], [c1, 0.0, s1], [c1, 0.0, s1], [c0, 0.0, s0]]);
        idx.extend([i, i + 2, i + 1, i, i + 3, i + 2]);
    }
    // The cone's side (normals tilted by its half angle) and its base disc.
    let slope = head_r / (1.0 - neck);
    for k in 0..N {
        let ((c0, s0), (c1, s1)) = (ring(k), ring(k + 1));
        let n = |c: f32, s: f32| Vec3::new(c, slope, s).normalize().to_array();
        let i = pos.len() as u32;
        pos.extend([[c0 * head_r, neck, s0 * head_r], [c1 * head_r, neck, s1 * head_r], [0.0, 1.0, 0.0]]);
        nor.extend([n(c0, s0), n(c1, s1), n((c0 + c1) / 2.0, (s0 + s1) / 2.0)]);
        idx.extend([i, i + 2, i + 1]);
        let j = pos.len() as u32;
        pos.extend([[c0 * head_r, neck, s0 * head_r], [c1 * head_r, neck, s1 * head_r], [0.0, neck, 0.0]]);
        nor.extend([[0.0, -1.0, 0.0]; 3]);
        idx.extend([j, j + 1, j + 2]);
    }
    let count = pos.len();
    let color = [rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0, 1.0];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![color; count])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; count])
        .with_inserted_indices(Indices::U32(idx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrow_spans_its_length_on_screen() {
        let view = crate::camera::ViewState::default();
        let a = Arrow3d { base: Vec3::new(5.0, -3.0, 12.0), dir: Vec3::Z, length_px: 60.0, hot: false };
        let (foot, tip) = screen_span(&a, &view);
        // The Z axis is foreshortened in the isometric view: at most 60 px, pointing up.
        assert!((tip - foot).length() <= 60.0 + 1e-3);
        assert!(tip.y < foot.y);
        // Reversed, the other way.
        let (f2, t2) = screen_span(&Arrow3d { dir: -Vec3::Z, ..a }, &view);
        assert!(((t2 - f2) + (tip - foot)).length() < 1e-3);
    }
}
