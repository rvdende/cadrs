//! Hidden edges (P3E.3a, TD6.5, PS2.9): in the render modes **Shaded with hidden edges** and
//! **Hidden edges visible** the part edges behind faces show dashed grey through them, as in
//! Onshape. Each part's edges are a line mesh drawn with a reversed depth test
//! ([`HiddenLineMaterial`]): a fragment is kept only where a face is in front of it, so a hidden
//! edge never lies over a visible one (which the black part edges draw). The dash follows the
//! edge (UV x is the distance along it) and keeps about 6 px on screen.

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, CompareFunction, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use cadrs_core::PartId;

use crate::AppState;
use crate::parts::PartCache;
use crate::viewport::ViewportView;

pub struct HiddenEdgesPlugin;

impl Plugin for HiddenEdgesPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "hidden_edges.wgsl");
        app.add_plugins(MaterialPlugin::<HiddenLineMaterial>::default())
            .add_systems(Update, (sync_hidden_edges, sync_params).chain().after(crate::parts::PartsSet).run_if(in_state(AppState::Document)));
    }
}

/// The grey of hidden edges over the white faces of Hidden edges visible.
const HIDDEN_EDGE: Color = Color::srgb(0.50, 0.53, 0.57);
/// Over shaded faces (Shaded with hidden edges) the grey would be about as light as the faces:
/// a dark slate, so the dashes read as clearly as on white.
const HIDDEN_EDGE_SHADED: Color = Color::srgb(0.10, 0.13, 0.18);

/// The hidden edges' colour in a render mode.
fn hidden_edge_color(mode: crate::camera::RenderMode) -> Vec4 {
    if mode.shaded() { HIDDEN_EDGE_SHADED } else { HIDDEN_EDGE }.to_linear().to_vec4()
}
/// The dash period on screen (px).
const DASH_PX: f32 = 7.0;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Copy, PartialEq)]
pub struct HiddenLineMaterial {
    #[uniform(0)]
    pub params: HiddenLineParams,
}

#[derive(Debug, Clone, Copy, PartialEq, ShaderType)]
pub struct HiddenLineParams {
    /// Linear RGBA.
    pub color: Vec4,
    /// The section view's clip plane (see [`crate::part_shading::PartShadingParams`]).
    pub clip: [Vec4; crate::section_view::MAX_PLANES],
    /// x: the dash period (mm); y: the depth bias toward the eye (see `hidden_edges.wgsl`).
    pub dash: Vec4,
}

impl Material for HiddenLineMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://cadrs_app/hidden_edges.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        // Drawn after the opaque parts, against their depth.
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Reversed depth (near is larger): kept where the edge lies behind what was drawn.
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_compare = Some(CompareFunction::Less);
            ds.depth_write_enabled = Some(false);
        }
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// A part's hidden-edge lines.
#[derive(Component)]
struct HiddenLines;

/// The line mesh of a part's edges: segment pairs, UV x the distance along each edge (mm).
pub fn edge_lines(part: &cadrs_core::parts::Part) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    for e in &part.solid.edges {
        let mut along = 0.0f32;
        for w in e.points.windows(2) {
            let (a, b) = (Vec3::new(w[0][0] as f32, w[0][1] as f32, w[0][2] as f32), Vec3::new(w[1][0] as f32, w[1][1] as f32, w[1][2] as f32));
            let l = a.distance(b);
            positions.push(a.to_array());
            positions.push(b.to_array());
            uvs.push([along, 0.0]);
            uvs.push([along + l, 0.0]);
            along += l;
        }
    }
    Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
}

#[allow(clippy::too_many_arguments)]
fn sync_hidden_edges(
    cache: Res<PartCache>,
    view: Res<ViewportView>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HiddenLineMaterial>>,
    mut mat: Local<Option<Handle<HiddenLineMaterial>>>,
    q: Query<Entity, With<HiddenLines>>,
    mut last: Local<Option<(u64, Vec<PartId>)>>,
    mut commands: Commands,
) {
    let on = view.view.render.hidden_edges();
    let shown: Vec<PartId> = if on { cache.shown().map(|p| p.id).collect() } else { Vec::new() };
    let key = (cache.generation, shown.clone());
    if last.as_ref() == Some(&key) {
        return;
    }
    *last = Some(key);
    for e in &q {
        commands.entity(e).try_despawn();
    }
    if !on {
        return;
    }
    let material = mat
        .get_or_insert_with(|| {
            materials.add(HiddenLineMaterial { params: HiddenLineParams { color: HIDDEN_EDGE.to_linear().to_vec4(), clip: [Vec4::ZERO; crate::section_view::MAX_PLANES], dash: Vec4::ZERO } })
        })
        .clone();
    for part in cache.shown() {
        commands.spawn((
            Name::new(format!("hidden-edges-{}", part.name.to_lowercase().replace(' ', "-"))),
            HiddenLines,
            Mesh3d(meshes.add(edge_lines(part))),
            MeshMaterial3d(material.clone()),
            Transform::IDENTITY,
            DespawnOnExit(AppState::Document),
        ));
    }
}

/// The dash length follows the zoom; the clip plane the section view; the colour the mode.
fn sync_params(view: Res<ViewportView>, clip: Res<crate::section_view::SectionClip>, mut materials: ResMut<Assets<HiddenLineMaterial>>) {
    if !view.view.render.hidden_edges() {
        return;
    }
    let clip = clip.plane.map_or([Vec4::ZERO; crate::section_view::MAX_PLANES], |c| c.uniforms());
    // As the part edges' gizmo bias, grown in perspective (`crate::view_options`).
    let k = crate::view_options::bias_factor(&view.view);
    let dash = Vec4::new(DASH_PX * view.view.scale, (6e-5 * k).min(0.05), 0.0, 0.0);
    let color = hidden_edge_color(view.view.render);
    let ids: Vec<AssetId<HiddenLineMaterial>> = materials.ids().collect();
    for id in ids {
        if materials.get(id).is_some_and(|m| m.params.clip != clip || m.params.dash != dash || m.params.color != color)
            && let Some(mut m) = materials.get_mut(id)
        {
            m.params.clip = clip;
            m.params.dash = dash;
            m.params.color = color;
        }
    }
}
